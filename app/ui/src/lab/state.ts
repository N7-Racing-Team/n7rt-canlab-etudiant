// Shared state of the Physical Lab view: a read-only mirror of the Rust document, the latest values, and
// UI-only state (selection, hover, view, message). Contract: docs/physical-lab-v1.md §3.3 and §8.4.
//
// - LabState is mutated only by LabController (document, values) and by the editor and panels for UI-only
//   fields (selection, hover, view, message), always through the setters below so that listeners fire.
// - LabController talks to the core through api.ts: commands in one FIFO (one call in flight), previews and pot
//   drags latest-only, values pulled every 50 ms while the view is active and once after every command reply.
// - The UI never decides connectivity, geometry validity or electrical values: it shows what the core returned.

import { asPhysError, CommandQueue, LatestOnly, phys } from "./api";
import type {
  CommandReply, CompId, Component, CompValues, DemoEntry, Disclosure, Edit, EditReply, FileEntry, Interact,
  InteractReply, Kind, KindDef, MeterFn, Net, NetValue, Patch, PhysCommand, PhysError, PinRef, PreviewReply, Refusal, RunReply,
  RunOp, RunState, SnapshotDto, ValueMap, ValuesDto, Wire, WireId,
} from "./types";

export type Tool = "select" | "wire";

/** What changed in the document mirror. */
export type DocEvent =
  | { kind: "snapshot" }
  | { kind: "patch"; patch: Patch; netsChanged: boolean }
  | { kind: "state"; comp: CompId };

export interface Hover {
  comp?: CompId;
  wire?: WireId;
  pin?: PinRef;
  /** Key of the hovered net (display). */
  net?: PinRef;
}

export interface Message {
  kind: "info" | "error";
  text: string;
}

export interface ViewInfo {
  tool: Tool;
  zoomPercent: number;
}

export class Emitter<T> {
  private readonly fns = new Set<(e: T) => void>();

  on(fn: (e: T) => void): () => void {
    this.fns.add(fn);
    return () => this.fns.delete(fn);
  }

  emit(e: T): void {
    for (const fn of this.fns) fn(e);
  }
}

export const pinKey = (p: PinRef): string => `${p.comp}:${p.pin}`;

export class LabState {
  readonly library = new Map<Kind, KindDef>();
  /** Placeable kinds in palette order. */
  palette: KindDef[] = [];

  /** Identity of the mirrored document (SnapshotDto.doc_id). */
  docId = 0;
  rev = 0;
  name = "";
  dirty = false;
  canUndo = false;
  canRedo = false;
  run: RunState = "stop";
  simTimeNs = 0;
  disclosure: Disclosure = "exploration";

  readonly components = new Map<CompId, Component>();
  readonly wires = new Map<WireId, Wire>();
  readonly states = new Map<CompId, ValueMap>();
  nets: Net[] = [];
  private readonly netByPin = new Map<string, Net>();

  values: ValuesDto | null = null;
  readonly compValues = new Map<CompId, CompValues>();
  /** By pinKey(net key). */
  readonly netValues = new Map<string, NetValue>();

  readonly selection = { components: new Set<CompId>(), wires: new Set<WireId>() };
  hover: Hover = {};
  view: ViewInfo = { tool: "select", zoomPercent: 100 };
  message: Message | null = null;
  demos: DemoEntry[] = [];

  readonly onDoc = new Emitter<DocEvent>();
  readonly onValues = new Emitter<ValuesDto>();
  /** rev, name, dirty, undo/redo, run state, simulated time. */
  readonly onStatus = new Emitter<void>();
  readonly onSelection = new Emitter<void>();
  readonly onHover = new Emitter<void>();
  readonly onView = new Emitter<void>();
  readonly onMessage = new Emitter<Message | null>();

  kindDef(kind: Kind): KindDef {
    const d = this.library.get(kind);
    if (!d) throw new Error(`kind inconnu : ${kind}`);
    return d;
  }

  netOf(pin: PinRef): Net | undefined {
    return this.netByPin.get(pinKey(pin));
  }

  // ------------------------------------------------------------------ UI-only setters (editor, panels)

  setSelection(components: Iterable<CompId>, wires: Iterable<WireId>): void {
    this.selection.components = new Set(components);
    this.selection.wires = new Set(wires);
    this.onSelection.emit();
  }

  setHover(hover: Hover): void {
    const h = this.hover;
    if (h.comp === hover.comp && h.wire === hover.wire && h.pin?.comp === hover.pin?.comp &&
        h.pin?.pin === hover.pin?.pin && h.net?.comp === hover.net?.comp && h.net?.pin === hover.net?.pin) return;
    this.hover = hover;
    this.onHover.emit();
  }

  setView(view: Partial<ViewInfo>): void {
    this.view = { ...this.view, ...view };
    this.onView.emit();
  }

  setMessage(message: Message | null): void {
    this.message = message;
    this.onMessage.emit(message);
  }

  // ------------------------------------------------------------------ mirror updates (controller only)

  applyLibrary(kinds: KindDef[]): void {
    this.library.clear();
    for (const k of kinds) this.library.set(k.kind, k);
    this.palette = kinds.filter((k) => k.placeable);
  }

  applySnapshot(s: SnapshotDto): void {
    this.docId = s.doc_id;
    this.rev = s.rev;
    this.name = s.name;
    this.dirty = s.dirty;
    this.canUndo = s.can_undo;
    this.canRedo = s.can_redo;
    this.run = s.run;
    this.simTimeNs = s.sim_time_ns;
    this.disclosure = s.disclosure;
    this.components.clear();
    for (const c of s.components) this.components.set(c.id, c);
    this.wires.clear();
    for (const w of s.wires) this.wires.set(w.id, w);
    this.states.clear();
    for (const [id, st] of s.states) this.states.set(id, st);
    this.setNets(s.nets);
    this.pruneSelection();
    this.onDoc.emit({ kind: "snapshot" });
    this.onStatus.emit();
  }

  applyEdit(r: EditReply): void {
    this.rev = r.rev;
    this.canUndo = r.can_undo;
    this.canRedo = r.can_redo;
    this.dirty = r.dirty;
    for (const [id, , after] of r.patch.components) {
      if (after) this.components.set(id, after);
      else {
        this.components.delete(id);
        this.states.delete(id);
        this.compValues.delete(id);
      }
    }
    for (const [id, , after] of r.patch.wires) {
      if (after) this.wires.set(id, after);
      else this.wires.delete(id);
    }
    if (r.nets) this.setNets(r.nets);
    this.pruneSelection();
    this.onDoc.emit({ kind: "patch", patch: r.patch, netsChanged: r.nets !== null });
    this.onStatus.emit();
  }

  applyInteract(r: InteractReply): void {
    // A reply already in flight when its component was deleted must not revive an orphaned state.
    if (!this.components.has(r.comp)) return;
    this.states.set(r.comp, r.state);
    this.onDoc.emit({ kind: "state", comp: r.comp });
  }

  applyRun(run: RunState, simTimeNs: number): void {
    this.run = run;
    this.simTimeNs = simTimeNs;
    this.onStatus.emit();
  }

  applyValues(v: ValuesDto): void {
    // Values computed for another document, or an older revision, would show nets or pins that no longer exist.
    if (v.doc_id !== this.docId || v.rev < this.rev) return;
    this.values = v;
    this.compValues.clear();
    for (const c of v.comps) {
      this.compValues.set(c.id, c);
      this.states.set(c.id, c.state);
    }
    this.netValues.clear();
    for (const n of v.nets) this.netValues.set(pinKey(n.key), n);
    const statusChanged = this.run !== v.run || this.simTimeNs !== v.sim_time_ns || this.disclosure !== v.disclosure;
    this.run = v.run;
    this.simTimeNs = v.sim_time_ns;
    this.disclosure = v.disclosure;
    this.onValues.emit(v);
    if (statusChanged) this.onStatus.emit();
  }

  private setNets(nets: Net[]): void {
    this.nets = nets;
    this.netByPin.clear();
    for (const n of nets) for (const p of n.pins) this.netByPin.set(pinKey(p), n);
  }

  /** Drop selected ids that no longer exist (a split wire keeps its id; its new half is not selected). */
  private pruneSelection(): void {
    let changed = false;
    for (const id of this.selection.components) if (!this.components.has(id)) { this.selection.components.delete(id); changed = true; }
    for (const id of this.selection.wires) if (!this.wires.has(id)) { this.selection.wires.delete(id); changed = true; }
    if (changed) this.onSelection.emit();
  }
}

export type PreviewResult = { ok: true; reply: PreviewReply } | { ok: false; refusal: Refusal | null };

/** Outcome of a committed command: the reply, or the refusal (already shown as the message line). */
export type Outcome<T> = { ok: true; reply: T } | { ok: false; error: PhysError };

const VALUES_PERIOD_MS = 50;

/** Keeps a reply of the expected type; any other type is a contract violation reported as engine_unavailable. */
function narrow<T>(type: CommandReply["type"]): (o: Outcome<CommandReply>) => Outcome<T> {
  return (o) => {
    if (!o.ok) return o;
    if (o.reply.type !== type) return { ok: false, error: { code: "engine_unavailable" } };
    return { ok: true, reply: o.reply as unknown as T };
  };
}

export class LabController {
  private readonly queue = new CommandQueue();
  private readonly preview: LatestOnly<Edit, PreviewReply>;
  private previewSink: ((r: PreviewResult, edit: Edit) => void) | null = null;
  private readonly gestureChannels = new Map<string, { comp: CompId; channel: LatestOnly<Interact, CommandReply> }>();
  private channelDocId: number;
  private seq = 0;
  private polling = false;
  private pollTimer: number | null = null;
  private valuesInFlight = false;

  constructor(readonly state: LabState) {
    this.channelDocId = state.docId;
    state.onDoc.on((event) => {
      if (state.docId !== this.channelDocId) {
        this.cancelPreview();
        for (const { channel } of this.gestureChannels.values()) channel.cancel();
        this.gestureChannels.clear();
        this.channelDocId = state.docId;
      } else if (event.kind !== "state") {
        for (const [key, { comp, channel }] of this.gestureChannels) {
          if (state.components.has(comp)) continue;
          channel.cancel();
          this.gestureChannels.delete(key);
        }
      }
    });
    this.preview = new LatestOnly<Edit, PreviewReply>(
      (edit) => phys.preview(this.state.rev, edit),
      (result, edit) => {
        const sink = this.previewSink;
        if (!sink) return;
        if (result.ok) sink({ ok: true, reply: result.value }, edit);
        else {
          const e = asPhysError(result.error);
          sink({ ok: false, refusal: e.code === "refused" ? e.refusal : null }, edit);
        }
      },
    );
  }

  /** Library, document and demonstration list; call once when the view is first shown. */
  async init(): Promise<void> {
    this.state.applyLibrary(await phys.library());
    this.state.applySnapshot(await phys.snapshot());
    this.state.demos = await phys.demos();
    await this.refreshValues();
  }

  // ------------------------------------------------------------------ edits and history

  edit(edit: Edit): Promise<Outcome<EditReply>> {
    return this.exec(() => ({ cmd: "edit", base_rev: this.state.rev, edit })).then(narrow<EditReply>("edit"));
  }

  undo(): Promise<Outcome<EditReply>> {
    return this.exec(() => ({ cmd: "undo" })).then(narrow<EditReply>("edit"));
  }

  redo(): Promise<Outcome<EditReply>> {
    return this.exec(() => ({ cmd: "redo" })).then(narrow<EditReply>("edit"));
  }

  /** Dry run of the gesture in progress; only the newest request is ever sent. */
  requestPreview(edit: Edit, sink: (r: PreviewResult, edit: Edit) => void): void {
    this.previewSink = sink;
    this.preview.post(edit);
  }

  cancelPreview(): void {
    this.previewSink = null;
    this.preview.cancel();
  }

  // ------------------------------------------------------------------ interactions and run control

  /** Retained drag channels, for performance checks; bounded by the current document's live components. */
  get retainedPotChannels(): number { return this.gestureChannels.size; }
  get retainedGestureChannels(): number { return this.gestureChannels.size; }

  /**
     * Pot and world drags are latest-only per component/quantity; other interactions keep FIFO order. Fire and
   * forget: the canvas shows the state returned by the core, errors go to the message line.
   */
  interact(interact: Interact): void {
    const key = interact.op === "set_pot" ? `${interact.comp}:pot`
      : interact.op === "set_world" && ["speed_mps", "steering_deg", "angle_deg", "pressure_bar"].includes(interact.quantity)
        ? `${interact.comp}:world:${interact.quantity}` : null;
    if (key !== null) {
      if (!this.state.components.has(interact.comp)) {
        // Let the core report the usual refusal without retaining a channel for a missing component.
        void this.exec(() => ({ cmd: "interact", interact }));
        return;
      }
      let ch = this.gestureChannels.get(key)?.channel;
      if (!ch) {
        ch = new LatestOnly<Interact, CommandReply>(
          (i) => phys.command({ cmd: "interact", interact: i }),
          (result) => {
            if (result.ok) this.absorb(result.value);
            else this.showError(asPhysError(result.error));
            void this.refreshValues();
          },
        );
        this.gestureChannels.set(key, { comp: interact.comp, channel: ch });
      }
      ch.post(interact);
      return;
    }
    void this.exec(() => ({ cmd: "interact", interact }));
  }

  runControl(op: RunOp): Promise<Outcome<RunReply>> {
    return this.exec(() => ({ cmd: op })).then(narrow<RunReply>("run"));
  }

  /** Places the multimeter probes and picks its function (journaled by the core, no document change). */
  meter(com: PinRef | null, vohm: PinRef | null, fn: MeterFn): Promise<Outcome<RunReply>> {
    return this.exec(() => ({ cmd: "meter", com, vohm, function: fn })).then(narrow<RunReply>("run"));
  }

  // ------------------------------------------------------------------ files

  newCircuit(): Promise<Outcome<SnapshotDto>> {
    return this.file(() => phys.newCircuit());
  }

  save(name: string, overwrite = false): Promise<Outcome<SnapshotDto>> {
    return this.file(() => phys.save(name, overwrite));
  }

  load(name: string): Promise<Outcome<SnapshotDto>> {
    return this.file(() => phys.load(name));
  }

  loadDemo(id: string): Promise<Outcome<SnapshotDto>> {
    return this.file(() => phys.loadDemo(id));
  }

  list(): Promise<FileEntry[]> {
    return this.queue.push(() => phys.list());
  }

  // ------------------------------------------------------------------ values

  /** Pull values now (after a command reply) and keep pulling every 50 ms while polling is on. */
  async refreshValues(): Promise<void> {
    if (this.valuesInFlight) return;
    this.valuesInFlight = true;
    try {
      const v = await phys.values(this.seq);
      if (v) {
        this.seq = v.seq;
        // A document the UI has not seen: the core replaced it out of band (mission bench loaded by the mission
        // controller). Mirror it before its values.
        if (v.doc_id > this.state.docId) await this.reloadDocument();
        else if (v.doc_id === this.state.docId && v.rev > this.state.rev) await this.reloadRevision(v.rev);
        this.state.applyValues(v);
      }
    } catch (e) {
      this.showError(asPhysError(e));
    } finally {
      this.valuesInFlight = false;
    }
  }

  startPolling(): void {
    if (this.polling) return;
    this.polling = true;
    const tick = async () => {
      if (!this.polling) return;
      await this.refreshValues();
      if (this.polling) this.pollTimer = window.setTimeout(tick, VALUES_PERIOD_MS);
    };
    void tick();
  }

  stopPolling(): void {
    this.polling = false;
    if (this.pollTimer !== null) window.clearTimeout(this.pollTimer);
    this.pollTimer = null;
  }

  // ------------------------------------------------------------------ internals

  /** Snapshot of a document replaced by the core itself, through the FIFO (after the commands in flight). */
  private async reloadDocument(): Promise<void> {
    const s = await this.queue.push(() => phys.snapshot());
    if (s.doc_id < this.state.docId) return;
    this.cancelPreview();
    this.state.setSelection([], []);
    this.state.applySnapshot(s);
  }

  /** A revision of the same document that no reply of this mirror brought: the core changed the document out of band
   * (the mechanical setting of an incident). Mirrored once the queued commands are done, only if still behind (a reply
   * of our own usually catches up first). */
  private async reloadRevision(rev: number): Promise<void> {
    const s = await this.queue.push(async () => (this.state.rev < rev ? phys.snapshot() : null));
    if (!s || s.doc_id !== this.state.docId || s.rev <= this.state.rev) return;
    this.state.applySnapshot(s);
  }

  /** FIFO; base_rev is read when the call is actually sent, so queued edits are not stale by construction. */
  private exec(make: () => PhysCommand): Promise<Outcome<CommandReply>> {
    return this.queue.push(async (): Promise<Outcome<CommandReply>> => {
      try {
        const reply = await phys.command(make());
        this.absorb(reply);
        void this.refreshValues();
        return { ok: true, reply };
      } catch (e) {
        const error = asPhysError(e);
        if (error.code === "refused" && error.refusal.code === "stale_revision") {
          this.state.applySnapshot(await phys.snapshot());
        }
        this.showError(error);
        return { ok: false, error };
      }
    });
  }

  private absorb(reply: CommandReply): void {
    if (reply.type === "edit") this.state.applyEdit(reply);
    else if (reply.type === "interact") this.state.applyInteract(reply);
    else this.state.applyRun(reply.run, reply.sim_time_ns);
    if (this.state.message?.kind === "error") this.state.setMessage(null);
  }

  private file(call: () => Promise<SnapshotDto>): Promise<Outcome<SnapshotDto>> {
    return this.queue.push(async () => {
      try {
        const s = await call();
        this.state.setSelection([], []);
        this.state.applySnapshot(s);
        void this.refreshValues();
        return { ok: true, reply: s } as Outcome<SnapshotDto>;
      } catch (e) {
        const err = asPhysError(e);
        this.showError(err);
        return { ok: false, error: err } as Outcome<SnapshotDto>;
      }
    });
  }

  private showError(err: PhysError): void {
    let text: string;
    if (err.code === "refused") text = err.refusal.message_fr;
    else if (err.code === "engine_unavailable") text = "Moteur de simulation indisponible : relancez l'application.";
    else {
      const f = err.error;
      text = f.code === "invalid" ? `Fichier invalide : ${f.violations.map((v) => v.message_fr).slice(0, 3).join(" ; ")}`
        : f.code === "not_found" ? "Fichier introuvable."
        : f.code === "bad_name" ? "Nom de fichier invalide (lettres, chiffres, _ et -, 64 caractères au plus)."
        : f.code === "name_exists" ? "Un circuit porte déjà ce nom : confirmez pour l'écraser."
        : f.code === "no_storage" ? "Aucun dossier de données : enregistrement impossible."
        : `Erreur d'entrée/sortie : ${f.message}`;
    }
    this.state.setMessage({ kind: "error", text });
  }
}
