// Console of the Physical Lab (contract docs/physical-lab-v1.md §8.4): Diagnostics (severity, code, message; a click
// selects the items), Nets (name, pins, voltage) and Journal (the core's last 300 entries, with the refusals of the
// interface, which the core does not journal). Rows of 20 px, recycled and virtualised (./vlist.ts), written with
// textContent in the animation frame.

import { el, setText } from "../../dom";
import { phys } from "../api";
import { pinKey, type LabState, type Message } from "../state";
import type { CompId, Diagnostic, ItemRef, JournalEntry, Net, WireId } from "../types";
import { readingView } from "../units";
import { commandText, frameTime, pinName, SEVERITY_FR } from "./text";
import { setCell, VirtualList } from "./vlist";

export const JOURNAL_KEPT = 300;
const JOURNAL_MIN_INTERVAL_MS = 250;

interface JournalRow {
  frame: number;
  kind: string;
  cls: string;
  text: string;
}

const TABS = { diag: "phys-diag", nets: "phys-nets", journal: "phys-journal" } as const;

/** Merges the core's journal and the local refusals by frame; refusals after the commands of their frame. */
export function mergeJournal(core: JournalRow[], local: JournalRow[], kept = JOURNAL_KEPT): JournalRow[] {
  const out: JournalRow[] = [];
  let i = 0;
  let j = 0;
  while (i < core.length || j < local.length) {
    if (j >= local.length || (i < core.length && core[i].frame <= local[j].frame)) out.push(core[i++]);
    else out.push(local[j++]);
  }
  return out.length > kept ? out.slice(out.length - kept) : out;
}

export class LabConsole {
  private readonly diag: VirtualList<Diagnostic>;
  private readonly nets: VirtualList<Net>;
  private readonly journal: VirtualList<JournalRow>;
  private readonly counts: Record<"diag" | "nets" | "journal", HTMLElement>;
  private readonly empties: Record<"diag" | "nets" | "journal", HTMLElement>;
  private readonly journalNotice: HTMLElement;
  private journalNoticeDirty = true;
  private tab: string = TABS.diag;
  private diagDirty = true;
  private netsDirty = true;
  private journalDue = true;
  private journalInFlight = false;
  private journalAt = 0;
  private core: JournalRow[] = [];
  private local: JournalRow[] = [];
  private journalRows: JournalRow[] = [];
  private journalDirty = true;
  private seenErrors = 0;

  constructor(root: HTMLElement, private readonly state: LabState) {
    const q = (id: string) => {
      const e = root.querySelector<HTMLElement>(`#${id}`);
      if (!e) throw new Error(`élément introuvable : #${id}`);
      return e;
    };
    this.diag = new VirtualList<Diagnostic>(q("phys-diag"), ["k", "", "m"], (cells, d, row) => this.fillDiag(cells, d, row),
      (d) => this.select(d.items));
    this.nets = new VirtualList<Net>(q("phys-nets"), ["", "", "num"], (cells, n, row) => this.fillNet(cells, n, row),
      (n) => this.state.setSelection([], n.wires));
    this.journal = new VirtualList<JournalRow>(q("phys-journal"), ["t", "k", ""], (cells, r) => {
      setCell(cells[0], frameTime(r.frame), "t");
      setCell(cells[1], r.kind, `k ${r.cls}`);
      setCell(cells[2], r.text, "", r.text);
    }, undefined, true);
    this.counts = { diag: q("phys-count-diag"), nets: q("phys-count-nets"), journal: q("phys-count-journal") };
    this.empties = { diag: q("phys-diag-empty"), nets: q("phys-nets-empty"), journal: q("phys-journal-empty") };
    this.journalNotice = el("p", "empty", "Journal incomplet : certaines entrées n’ont pas pu être conservées.");
    this.journalNotice.id = "phys-journal-incomplete";
    this.journalNotice.hidden = true;
    this.journalNotice.setAttribute("role", "status");
    this.journalNotice.setAttribute("aria-live", "polite");
    q("phys-journal").before(this.journalNotice);
  }

  mount(): void {
    const s = this.state;
    s.onValues.on(() => {
      this.diagDirty = true;
      this.netsDirty = true;
      this.journalNoticeDirty = true;
    });
    s.onDoc.on(() => {
      this.netsDirty = true;
      this.journalDue = true;
      this.journalNoticeDirty = true;
    });
    s.onStatus.on(() => {
      this.journalDue = true;
    });
    s.onSelection.on(() => {
      this.diag.invalidate();
      this.nets.invalidate();
    });
    s.onMessage.on((m: Message | null) => {
      // Refusals are not journaled by the core: the interface keeps them next to the journal (§3.3).
      if (m?.kind !== "error") return;
      const frame = Math.floor(s.simTimeNs / 1e6);
      const last = this.local[this.local.length - 1];
      // The same failure repeated in the same frame (a poll that keeps failing) is one row.
      if (last && last.text === m.text && last.frame === frame) return;
      this.local.push({ frame, kind: "refus", cls: "refus", text: m.text });
      if (this.local.length > JOURNAL_KEPT) this.local.shift();
      this.journalRows = mergeJournal(this.core, this.local);
      this.journalDirty = true;
    });
  }

  /** The shell switched tab: the shown list is rendered; inspected errors stop alerting. */
  tabShown(tab: string): void {
    this.tab = tab;
    if (tab === TABS.diag) this.seenErrors = this.errorCount();
    if (tab === TABS.journal) this.journalDue = true;
    this.diag.invalidate();
    this.nets.invalidate();
    this.journal.invalidate();
    this.diagDirty = this.netsDirty = this.journalDirty = true;
    this.journalNoticeDirty = true;
  }

  invalidate(): void {
    this.tabShown(this.tab);
  }

  render(now: number): void {
    if (this.journalNoticeDirty) {
      this.journalNoticeDirty = false;
      const s = this.state;
      const incomplete = s.disclosure === "exploration" && s.values?.disclosure === "exploration" &&
        s.values.doc_id === s.docId && s.values.diagnostics.some((d) => d.code === "journal_incomplete");
      this.journalNotice.hidden = !(incomplete && this.tab === TABS.journal);
      this.counts.journal.classList.toggle("alert", incomplete);
    }
    if (this.diagDirty) {
      this.diagDirty = false;
      const list = this.state.values?.diagnostics ?? [];
      this.diag.setItems(list);
      this.empties.diag.hidden = list.length > 0;
      setText(this.counts.diag, String(list.length));
      const errors = this.errorCount();
      if (this.tab === TABS.diag) this.seenErrors = errors;
      this.counts.diag.classList.toggle("alert", errors > this.seenErrors);
    }
    if (this.netsDirty) {
      this.netsDirty = false;
      this.nets.setItems(this.state.nets);
      this.empties.nets.hidden = this.state.nets.length > 0;
      setText(this.counts.nets, String(this.state.nets.length));
    }
    if (this.journalDue && this.tab === TABS.journal) this.fetchJournal(now);
    if (this.journalDirty) {
      this.journalDirty = false;
      this.journal.setItems(this.journalRows);
      this.empties.journal.hidden = this.journalRows.length > 0;
      setText(this.counts.journal, String(this.journalRows.length));
    }
    if (this.tab === TABS.diag) this.diag.render();
    else if (this.tab === TABS.nets) this.nets.render();
    else if (this.tab === TABS.journal) this.journal.render();
  }

  private errorCount(): number {
    return (this.state.values?.diagnostics ?? []).filter((d) => d.severity === "error").length;
  }

  /** One call in flight, at most every 250 ms, only while the Journal tab is shown. */
  private fetchJournal(now: number): void {
    if (this.journalInFlight || now - this.journalAt < JOURNAL_MIN_INTERVAL_MS) return;
    this.journalDue = false;
    this.journalInFlight = true;
    this.journalAt = now;
    phys.journal(JOURNAL_KEPT).then(
      (entries: JournalEntry[]) => {
        this.core = entries.map((e) => {
          const t = commandText(this.state, e.command);
          return { frame: e.frame, kind: t.kind, cls: t.kind === "action" || t.kind === "simulation" ? t.kind : "", text: t.text };
        });
        this.journalRows = mergeJournal(this.core, this.local);
        this.journalDirty = true;
      },
      () => {
        /* journal unavailable: the previous rows stay; the next command asks again */
      },
    ).finally(() => {
      this.journalInFlight = false;
    });
  }

  private fillDiag(cells: HTMLElement[], d: Diagnostic, row: HTMLElement): void {
    setCell(cells[0], SEVERITY_FR[d.severity], `k ${d.severity}`);
    setCell(cells[1], d.code, "", d.code);
    setCell(cells[2], d.message_fr, "", d.message_fr);
    row.classList.toggle("selected", this.touchesSelection(d.items));
  }

  private fillNet(cells: HTMLElement[], n: Net, row: HTMLElement): void {
    setCell(cells[0], n.name, "", n.name);
    const pins = n.pins.map((p) => pinName(this.state, p)).join(", ");
    setCell(cells[1], pins, "", pins);
    const v = readingView(this.state.netValues.get(pinKey(n.key))?.voltage, "V");
    setCell(cells[2], v.text, v.state === "word" ? "num lab-word" : v.state === "none" ? "num lab-none" : "num", v.title);
    const sel = this.state.selection.wires;
    row.classList.toggle("selected", n.wires.length > 0 && n.wires.every((w) => sel.has(w)) && sel.size === n.wires.length);
  }

  private touchesSelection(items: ItemRef[]): boolean {
    const sel = this.state.selection;
    return items.some((it) => (it.type === "wire" ? sel.wires.has(it.id) : sel.components.has(it.type === "pin" ? it.comp : it.id)));
  }

  private select(items: ItemRef[]): void {
    const comps = new Set<CompId>();
    const wires = new Set<WireId>();
    for (const it of items) {
      if (it.type === "wire") wires.add(it.id);
      else comps.add(it.type === "pin" ? it.comp : it.id);
    }
    this.state.setSelection(comps, wires);
  }
}
