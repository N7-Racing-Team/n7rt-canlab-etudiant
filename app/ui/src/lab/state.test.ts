// The mirror follows a document the core replaced by itself (mission bench loaded by the mission controller):
// a values poll with a doc_id never seen in a snapshot reloads the snapshot before applying its values; values of
// another document are never applied.

import { describe, expect, it, vi } from "vitest";
import { LIBRARY, comp } from "./canvas/testkit";
import { LabController, LabState } from "./state";
import type { Component, Disclosure, SnapshotDto, ValuesDto } from "./types";

const fake = vi.hoisted(() => ({
  calls: [] as string[],
  snapshot: null as unknown,
  values: null as unknown,
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (cmd: string) => {
    fake.calls.push(cmd);
    if (cmd === "phys_snapshot") return Promise.resolve(fake.snapshot);
    if (cmd === "phys_values") return Promise.resolve(fake.values);
    if (cmd === "phys_library") return Promise.resolve(LIBRARY);
    if (cmd === "phys_demos") return Promise.resolve([]);
    return Promise.reject({ code: "engine_unavailable" });
  },
}));

function snapshot(doc_id: number, rev: number, name: string, components: Component[], disclosure: Disclosure): SnapshotDto {
  return {
    doc_id, rev, name, dirty: false, components, wires: [], states: components.map((c) => [c.id, {}]), nets: [],
    can_undo: false, can_redo: false, run: "run", sim_time_ns: 0, disclosure,
  };
}

function values(doc_id: number, rev: number, seq: number, disclosure: Disclosure): ValuesDto {
  return {
    seq, doc_id, rev, sim_time_ns: 1_000_000, run: "run", preview: false, disclosure, nets: [], comps: [], diagnostics: [],
    meter: { com: null, vohm: null, function: "v_dc", display: "—" },
    engine: { lag_frames: 0, slow: false, frame_us_p95: 0, substeps_last: 1, min_dt_ns: 1_000_000, islands: 1, restarts: 0 },
  };
}

describe("document replaced by the core", () => {
  it("reloads the snapshot when the values carry a new doc_id, then shows the mission mode", async () => {
    const state = new LabState();
    const ctl = new LabController(state);
    fake.snapshot = snapshot(1, 3, "essai", [comp(1, "resistor", [0, 0], 0, "R1")], "exploration");
    fake.values = values(1, 3, 2, "exploration");
    await ctl.init();
    expect(state.docId).toBe(1);
    expect(state.values?.seq).toBe(2);
    state.setSelection([1], []);

    // The mission controller loads the bench from Rust: the UI only sees it through the values poll.
    const bench = [comp(4, "ecu_apps", [30, 0], 0, "U1"), comp(5, "mlx90377", [0, 0], 0, "U2")];
    fake.snapshot = snapshot(2, 5, "banc-apps", bench, "mission");
    fake.values = values(2, 5, 3, "mission");
    fake.calls.length = 0;
    await ctl.refreshValues();
    expect(fake.calls).toEqual(["phys_values", "phys_snapshot"]);
    expect(state.docId).toBe(2);
    expect(state.name).toBe("banc-apps");
    expect([...state.components.keys()]).toEqual([4, 5]);
    expect(state.components.get(4)?.params).toEqual({});
    expect(state.disclosure).toBe("mission");
    expect(state.values?.doc_id).toBe(2);
    expect(state.selection.components.size).toBe(0);

    // Late values of the previous document are dropped, and never trigger a reload.
    fake.values = values(1, 3, 4, "exploration");
    fake.calls.length = 0;
    await ctl.refreshValues();
    expect(fake.calls).toEqual(["phys_values"]);
    expect(state.values?.seq).toBe(3);
    expect(state.disclosure).toBe("mission");
  });
});
