import { describe, expect, it } from "vitest";
import type { Diagnostic, ValuesDto } from "../types";
import type { Rect } from "../types";
import { intersects } from "./geom";
import { type LiveItem, diagMarkers, diagSignature, layoutLive, liveItems, liveSignature, stateSignature } from "./live";
import { comp, makeModel, makeState, wire } from "./testkit";

const NB = " ";

function setup() {
  const state = makeState([comp(1, "resistor", [10, 0], 0, "R1"), comp(2, "capacitor", [30, 0], 0, "C1")], [wire(10, [1, "p2"], [2, "p1"])]);
  const model = makeModel(state);
  const values = (diagnostics: Diagnostic[] = []): ValuesDto => ({
    seq: 1, doc_id: 1, rev: 1, sim_time_ns: 0, run: "run", preview: false, disclosure: "exploration",
    nets: [{ key: { comp: 1, pin: "p2" }, name: "R1.2", voltage: { k: "valid", v: 1.25 } }],
    comps: [{
      id: 1, state: {}, op: { kind: "passive" }, measures: [],
      pins: [{ pin: "p1", voltage: { k: "floating" }, current: { k: "valid", v: 0 } }, { pin: "p2", voltage: { k: "valid", v: 1.25 }, current: { k: "valid", v: 0 } }],
    }],
    diagnostics,
    meter: { com: null, vohm: null, function: "v_dc", display: "—" },
    engine: { lag_frames: 0, slow: false, frame_us_p95: 0, substeps_last: 0, min_dt_ns: 0, islands: 1, restarts: 0 },
  });
  return { state, model, values };
}

describe("live values", () => {
  it("shows only the pins of the selected component and the hovered net, invalid readings in words", () => {
    const { state, model, values } = setup();
    state.applyValues(values());
    expect(liveItems(state, model, null)).toEqual([]);
    state.setSelection([1], []);
    expect(liveItems(state, model, null)).toEqual([
      { text: "1 flottant", at: [7, 0], dir: "l" },
      { text: `2 1.25000${NB}V`, at: [13, 0], dir: "r" },
    ]);
    state.setSelection([1, 2], []);
    expect(liveItems(state, model, null)).toEqual([]);
    state.setHover({ wire: 10, net: { comp: 1, pin: "p2" } });
    expect(liveItems(state, model, [20, 0])).toEqual([{ text: `R1.2 1.25000${NB}V`, at: [20, 0], dir: null }]);
  });

  it("has a signature that changes only when a shown string changes", () => {
    const { state, model, values } = setup();
    state.setSelection([1], []);
    state.applyValues(values());
    const a = liveSignature(liveItems(state, model, null));
    state.applyValues({ ...values(), seq: 2, sim_time_ns: 1_000_000 });
    expect(liveSignature(liveItems(state, model, null))).toBe(a);
    const v = values();
    v.comps[0].pins[1].voltage = { k: "valid", v: 1.3 };
    state.applyValues({ ...v, seq: 3 });
    expect(liveSignature(liveItems(state, model, null))).not.toBe(a);
  });
});

describe("drawn states", () => {
  it("changes its signature when a state drawn on the overlay arrives with the values", () => {
    const state = makeState([comp(1, "switch", [0, 0], 0, "S1"), comp(2, "resistor", [10, 0], 0, "R1")], []);
    const before = stateSignature(state);
    state.applyInteract({ rev: 1, comp: 2, state: {} });
    expect(stateSignature(state)).toBe(before);
    state.applyInteract({ rev: 1, comp: 1, state: { closed: true } });
    expect(stateSignature(state)).not.toBe(before);
  });
});

describe("diagnostic markers", () => {
  it("marks components, wires and pins once per place, an error winning over a warning, info unmarked", () => {
    const { state, model, values } = setup();
    state.applyValues(values([
      { code: "open_pin", severity: "info", message_fr: "", items: [{ type: "pin", comp: 1, pin: "p1" }] },
      { code: "floating_net", severity: "warning", message_fr: "", items: [{ type: "wire", id: 10 }, { type: "pin", comp: 2, pin: "p2" }] },
      { code: "resistor_overload", severity: "error", message_fr: "", items: [{ type: "component", id: 1 }] },
      { code: "cap_overvoltage", severity: "warning", message_fr: "", items: [{ type: "component", id: 1 }] },
    ]));
    const m = diagMarkers(state, model);
    expect(m).toContainEqual({ at: [12, -1], severity: "error" });
    expect(m).toContainEqual({ at: [20, 0], severity: "warning" });
    expect(m).toContainEqual({ at: [33, 0], severity: "warning" });
    expect(m.length).toBe(3);
    expect(diagSignature(m)).toBe(diagSignature([...m].reverse()));
  });
});

describe("live box places", () => {
  const H = 17;
  const GAP = 6;
  const U = 8;
  const boxOf = (it: LiveItem, w: number, p: { dx: number; dy: number }): Rect => ({
    x0: it.at[0] + p.dx / U, y0: it.at[1] + p.dy / U, x1: it.at[0] + (p.dx + w) / U, y1: it.at[1] + (p.dy + H) / U,
  });

  it("puts each box beyond its pin, along the pin direction, when nothing is there", () => {
    const items: LiveItem[] = [
      { text: "A", at: [0, 0], dir: "r" }, { text: "B", at: [0, 10], dir: "l" },
      { text: "C", at: [20, 0], dir: "u" }, { text: "D", at: [20, 10], dir: "d" },
    ];
    const places = layoutLive(items, [40, 40, 40, 40], H, GAP, U, () => false);
    expect(places).toEqual([
      { dx: 6, dy: -8, moved: false }, { dx: -46, dy: -8, moved: false },
      { dx: -20, dy: -23, moved: false }, { dx: -20, dy: 6, moved: false },
    ]);
  });

  it("moves a box off another symbol and off an earlier box, deterministically", () => {
    const symbol: Rect = { x0: 2, y0: -1, x1: 4, y1: 3 };
    const covers = (r: Rect) => intersects(r, symbol);
    const items: LiveItem[] = [{ text: "CANH", at: [0, 0], dir: "r" }, { text: "CANL", at: [0, 1], dir: "r" }];
    const places = layoutLive(items, [96, 96], H, GAP, U, covers);
    expect(places).toEqual(layoutLive(items, [96, 96], H, GAP, U, covers));
    const boxes = places.map((p, k) => boxOf(items[k], 96, p));
    for (const b of boxes) expect(intersects(b, symbol)).toBe(false);
    expect(intersects(boxes[0], boxes[1])).toBe(false);
    expect(places.every((p) => p.moved)).toBe(true);
    // Still beyond the pin: the box starts past the pin end.
    for (const b of boxes) expect(b.x0).toBeGreaterThan(0);
  });

  it("keeps the first place when every place is taken", () => {
    const places = layoutLive([{ text: "X", at: [0, 0], dir: "r" }], [40], H, GAP, U, () => true);
    expect(places).toEqual([{ dx: 6, dy: -8, moved: false }]);
  });

  it("clears the termination and the bus of tp1-capteur-can when U2 is selected", () => {
    const state = makeState([
      comp(2, "can_transceiver", [40, 20], 0, "U2"), comp(6, "termination", [52, 19], 90, "RT1"),
      comp(7, "can_bus", [62, 20], 0, "BUS1"), comp(11, "ground", [30, 26]), comp(12, "ground", [40, 32]),
      comp(13, "ground", [56, 28]),
    ], []);
    const model = makeModel(state);
    const u2 = model.comps.get(2)!;
    const items: LiveItem[] = u2.pins.map((p) => ({ text: `${p.short} 2.49975 V`, at: p.pos, dir: p.dir }));
    const widths = items.map((it) => Math.ceil(it.text.length * 6.6) + 6);
    const others = [6, 7, 11, 12, 13].map((id) => model.comps.get(id)!.bounds);
    const covers = (r: Rect) => [...others, u2.bounds].some((b) => intersects(r, b));
    const places = layoutLive(items, widths, H, GAP, U, covers);
    const boxes = places.map((p, k) => boxOf(items[k], widths[k], p));
    boxes.forEach((b, k) => {
      for (const o of others) expect(intersects(b, o), items[k].text).toBe(false);
      for (let j = 0; j < k; j++) expect(intersects(b, boxes[j]), `${items[k].text} / ${items[j].text}`).toBe(false);
    });
  });
});
