import { describe, expect, it, vi } from "vitest";
import { LabConsole } from "./console";
import { LabState } from "../state";
import type { Disclosure, SnapshotDto, ValuesDto } from "../types";

vi.mock("./vlist", () => ({
  VirtualList: class { setItems() {} invalidate() {} render() {} },
  setCell: vi.fn(),
}));
vi.mock("../api", () => ({ phys: { journal: () => Promise.resolve([]) } }));
vi.mock("../../dom", () => ({
  el: (_tag: string, className: string, textContent: string) => ({
    id: "", className, textContent, hidden: false, setAttribute: vi.fn(),
  }),
  setText: (node: { textContent: string }, text: string) => { node.textContent = text; },
}));

function fixture() {
  const note = { current: null as unknown as HTMLElement };
  const nodes = new Map<string, HTMLElement>();
  const alerts = new Map<string, boolean>();
  const root = { querySelector: (selector: string) => {
    if (!nodes.has(selector)) nodes.set(selector, {
      textContent: "", hidden: false,
      before: (node: HTMLElement) => { note.current = node; },
      classList: { toggle: (_name: string, active: boolean) => alerts.set(selector, active) },
    } as unknown as HTMLElement);
    return nodes.get(selector);
  } } as unknown as HTMLElement;
  const state = new LabState();
  state.docId = 1;
  const console = new LabConsole(root, state);
  console.mount();
  console.tabShown("phys-journal");
  return { state, console, note, alerts };
}

function values(doc_id = 1, incomplete = true, disclosure: Disclosure = "exploration"): ValuesDto {
  return {
    seq: 1, doc_id, rev: 0, sim_time_ns: 0, run: "stop", preview: true, disclosure, nets: [], comps: [],
    diagnostics: incomplete ? [{ code: "journal_incomplete", severity: "warning", items: [], message_fr: "detail" }] : [],
    meter: { com: null, vohm: null, function: "v_dc", display: "—" },
    engine: { lag_frames: 0, slow: false, frame_us_p95: 0, substeps_last: 0, min_dt_ns: 0, islands: 0, restarts: 0 },
  };
}

describe("journal loss notice", () => {
  it("shows the operator notice and journal alert only for loss on the current exploration document", () => {
    const f = fixture();
    f.state.applyValues(values(1, false));
    f.console.render(0);
    expect(f.note.current.hidden).toBe(true);
    expect(f.alerts.get("#phys-count-journal")).toBe(false);
    f.state.applyValues(values());
    f.console.render(1);
    expect(f.note.current.hidden).toBe(false);
    expect(f.note.current.textContent).toContain("certaines entrées n’ont pas pu être conservées");
    expect(f.alerts.get("#phys-count-journal")).toBe(true);
    f.console.tabShown("phys-diag");
    f.console.render(2);
    expect(f.note.current.hidden).toBe(true);
    expect(f.alerts.get("#phys-count-journal")).toBe(true);
  });

  it("clears stale loss immediately on a new document before its next values publication", () => {
    const f = fixture();
    f.state.applyValues(values());
    f.console.render(0);
    expect(f.note.current.hidden).toBe(false);
    const snapshot: SnapshotDto = { doc_id: 2, rev: 0, name: "new", dirty: false, components: [], wires: [],
      states: [], nets: [], can_undo: false, can_redo: false, run: "stop", sim_time_ns: 0, disclosure: "exploration" };
    f.state.applySnapshot(snapshot);
    expect(f.state.values?.doc_id).toBe(1); // Old values can outlive the snapshot until the next poll.
    f.console.render(1);
    expect(f.note.current.hidden).toBe(true);
    expect(f.alerts.get("#phys-count-journal")).toBe(false);
    f.state.applyValues(values(2, false));
    f.console.render(2);
    expect(f.note.current.hidden).toBe(true);
  });

  it("withholds the loss signal in mission disclosure, including a stale exploration publication", () => {
    const f = fixture();
    f.state.values = values(1, true, "mission");
    f.state.disclosure = "mission";
    f.console.render(0);
    expect(f.note.current.hidden).toBe(true);
    expect(f.alerts.get("#phys-count-journal")).toBe(false);
    f.state.values = values(1, true);
    f.state.onDoc.emit({ kind: "snapshot" });
    f.console.render(1);
    expect(f.note.current.hidden).toBe(true);
    expect(f.alerts.get("#phys-count-journal")).toBe(false);
  });
});
