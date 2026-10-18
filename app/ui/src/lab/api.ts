// Typed access to the Physical Lab core (src-tauri/src/phys.rs → crates/n7rt-lab/src/phys). Contract:
// docs/physical-lab-v1.md §8.3.
//
// Ordering. Tauri runs async commands concurrently, so the UI serialises what must stay ordered:
// - commands (edit, undo, redo, run control, file operations) go through one FIFO with one call in flight;
// - previews and continuous interactions (pot drag) go through latest-only channels: while a call is in
//   flight only the newest request is kept, older ones are dropped, never sent.
// The model is committed before a command replies; the UI then applies the reply's patch to its mirror and
// asks for values at once (edit → values within one frame plus two IPC calls).

import { invoke } from "@tauri-apps/api/core";
import type {
  CommandReply, DemoEntry, Edit, FileEntry, JournalEntry, KindDef, PhysCommand, PhysError, PreviewReply,
  SnapshotDto, ValuesDto,
} from "./types";

export const phys = {
  library: () => invoke<KindDef[]>("phys_library"),
  snapshot: () => invoke<SnapshotDto>("phys_snapshot"),
  /** Dry run on the committed document; rejects with PhysError. */
  preview: (base_rev: number, edit: Edit) => invoke<PreviewReply>("phys_preview", { baseRev: base_rev, edit }),
  /** Rejects with PhysError (refused: nothing changed). */
  command: (command: PhysCommand) => invoke<CommandReply>("phys_command", { command }),
  /** Values newer than since_seq, or null. */
  values: (since_seq: number) => invoke<ValuesDto | null>("phys_values", { sinceSeq: since_seq }),
  journal: (n: number) => invoke<JournalEntry[]>("phys_journal", { n }),
  newCircuit: () => invoke<SnapshotDto>("phys_new"),
  save: (name: string, overwrite: boolean) => invoke<SnapshotDto>("phys_save", { name, overwrite }),
  load: (name: string) => invoke<SnapshotDto>("phys_load", { name }),
  list: () => invoke<FileEntry[]>("phys_list"),
  demos: () => invoke<DemoEntry[]>("phys_demos"),
  loadDemo: (id: string) => invoke<SnapshotDto>("phys_load_demo", { id }),
};

/** Narrow an invoke rejection to the contract's error, or wrap anything else as engine_unavailable. */
export function asPhysError(e: unknown): PhysError {
  if (typeof e === "object" && e !== null && "code" in e) return e as PhysError;
  return { code: "engine_unavailable" };
}

/** FIFO with one call in flight: calls run in submission order, each after the previous one settled. */
export class CommandQueue {
  private tail: Promise<unknown> = Promise.resolve();
  private pending = 0;

  push<T>(call: () => Promise<T>): Promise<T> {
    this.pending++;
    const run = this.tail.then(call, call);
    this.tail = run.then(
      () => { this.pending--; },
      () => { this.pending--; },
    );
    return run;
  }

  /** Calls submitted and not yet settled (the in-flight one included). */
  get size(): number {
    return this.pending;
  }
}

/**
 * Latest-only channel: post() sends at once if idle; while a call is in flight it only remembers the newest
 * request and sends it when the reply arrives. Replies to requests superseded before being sent never exist;
 * cancel() drops the remembered request and marks the in-flight reply as stale (onReply is not called).
 */
export class LatestOnly<A, R> {
  private inFlight = false;
  private next: { a: A } | null = null;
  private epoch = 0;

  constructor(
    private readonly call: (a: A) => Promise<R>,
    private readonly onReply: (result: { ok: true; value: R } | { ok: false; error: unknown }, a: A) => void,
  ) {}

  post(a: A): void {
    if (this.inFlight) {
      this.next = { a };
      return;
    }
    this.send(a);
  }

  cancel(): void {
    this.next = null;
    this.epoch++;
  }

  get busy(): boolean {
    return this.inFlight;
  }

  private send(a: A): void {
    this.inFlight = true;
    const epoch = this.epoch;
    this.call(a).then(
      (value) => this.settle(epoch, a, { ok: true, value }),
      (error) => this.settle(epoch, a, { ok: false, error }),
    );
  }

  private settle(epoch: number, a: A, result: { ok: true; value: R } | { ok: false; error: unknown }): void {
    // Keep the gate closed while the callback runs: a reentrant post must replace the queued request,
    // rather than start a second call before that queue is drained. cancel() can also clear it here.
    try {
      if (epoch === this.epoch) this.onReply(result, a);
    } finally {
      this.inFlight = false;
      const next = this.next;
      this.next = null;
      if (next) this.send(next.a);
    }
  }
}
