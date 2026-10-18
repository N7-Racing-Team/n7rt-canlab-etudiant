import { describe, expect, it } from "vitest";
import type { Component, Patch, Rect, Wire } from "../types";
import { contains, intersects } from "./geom";
import { buildComp } from "./model";
import { comp, libDef, makeModel, makeState, monoMeasure, wire } from "./testkit";

// R1 (10,0) and C1 (30,0) joined by a straight wire 10 from R1.p2 (13,0) to C1.p1 (27,0); wire 11 from C1.p2 (33,0)
// to S1.p1 (37,0) of a switch at (40,0).
function scene() {
  const r1 = comp(1, "resistor", [10, 0], 0, "R1");
  const c1 = comp(2, "capacitor", [30, 0], 0, "C1");
  const s1 = comp(3, "switch", [40, 0], 0, "S1");
  const w10 = wire(10, [1, "p2"], [2, "p1"]);
  const w11 = wire(11, [2, "p2"], [3, "p1"]);
  const state = makeState([r1, c1, s1], [w10, w11]);
  return { state, model: makeModel(state), r1, c1, s1, w10, w11 };
}

const moved = (c: Component, dx: number, dy: number): Component => ({ ...c, pos: [c.pos[0] + dx, c.pos[1] + dy] });
const segsAt = (m: ReturnType<typeof makeModel>, r: Rect): number[] =>
  m.index.query(r).flatMap((it) => (it.t === "seg" ? [it.wire] : [])).sort();

describe("scene model", () => {
  it("draws wires from the pins of their components and counts the wires of each pin", () => {
    const { model } = scene();
    expect(model.wires.get(10)?.path).toEqual([[13, 0], [27, 0]]);
    expect(model.degree({ comp: 2, pin: "p1" })).toBe(1);
    expect(model.degree({ comp: 1, pin: "p1" })).toBe(0);
    expect(model.wiresOf(2)).toEqual([10, 11]);
  });

  it("re-routes a wire whose value is unchanged when both its components move (rigid move)", () => {
    const { model, r1, c1 } = scene();
    const patch: Patch = { components: [[1, r1, moved(r1, 0, 3)], [2, c1, moved(c1, 0, 3)]], wires: [] };
    const dirty = model.applyPatch(patch);
    expect(model.wires.get(10)?.path).toEqual([[13, 3], [27, 3]]);
    // Wire 11 keeps its fixed end on S1 and follows C1.p2: the core sends its repaired bends in a real patch; here
    // its path is recomputed from the moved pin.
    expect(model.wires.get(11)?.path).toEqual([[33, 3], [37, 0]]);
    expect(segsAt(model, { x0: 19, y0: -0.2, x1: 21, y1: 0.2 })).toEqual([]);
    expect(segsAt(model, { x0: 19, y0: 2.8, x1: 21, y1: 3.2 })).toEqual([10]);
    // Old and new draw-bounds of both components and of both wires.
    const covered = (r: Rect): boolean => dirty.some((d) => contains(d, r));
    expect(covered(model.comps.get(1)!.bounds)).toBe(true);
    expect(covered({ x0: 13, y0: 0, x1: 27, y1: 0 })).toBe(true);
    expect(covered({ x0: 13, y0: 3, x1: 27, y1: 3 })).toBe(true);
  });

  it("applies deletions and junction splits from a patch: index and degrees follow", () => {
    const { model, s1, w11 } = scene();
    const del: Patch = { components: [[3, s1, null]], wires: [[11, w11, null]] };
    model.applyPatch(del);
    expect(model.comps.has(3)).toBe(false);
    expect(model.wires.has(11)).toBe(false);
    expect(model.degree({ comp: 2, pin: "p2" })).toBe(0);
    expect(model.index.query({ x0: 36, y0: -3, x1: 44, y1: 3 })).toEqual([]);

    // Split of wire 10 at (20,0) by a junction 12, second half 13, and a new branch 14 from the junction to R1.p1.
    const j = comp(12, "junction", [20, 0]);
    const halfA: Wire = wire(10, [1, "p2"], [12, "j"]);
    const halfB: Wire = wire(13, [12, "j"], [2, "p1"]);
    const branch: Wire = wire(14, [12, "j"], [1, "p1"], [[20, -3], [7, -3]]);
    model.applyPatch({ components: [[12, null, j]], wires: [[10, wire(10, [1, "p2"], [2, "p1"]), halfA], [13, null, halfB], [14, null, branch]] });
    expect(model.degree({ comp: 12, pin: "j" })).toBe(3);
    expect(model.wires.get(14)?.path).toEqual([[20, 0], [20, -3], [7, -3], [7, 0]]);
    expect(model.index.query({ x0: 19.9, y0: -0.1, x1: 20.1, y1: 0.1 }).filter((it) => it.t === "junction")).toEqual([{ t: "junction", id: 12 }]);
  });

  it("computes the geometry of a previewed patch, with the attached wires", () => {
    const { model, r1 } = scene();
    const pg = model.previewGeometry({ components: [[1, r1, moved(r1, 0, 2)]], wires: [] });
    expect(pg.comps.map((g) => g.comp.pos)).toEqual([[10, 2]]);
    expect(pg.paths).toEqual([[[13, 2], [27, 0]]]);
    // The committed model is untouched.
    expect(model.wires.get(10)?.path).toEqual([[13, 0], [27, 0]]);
  });
});

describe("outer texts off the neighbours (UX-12)", () => {
  // V1, a supply turned a quarter, hangs its value « 5 V · 100 mA » (vertical text) under its body; BUS1 sits just
  // below it, its pin name « CANH » right under the supply: at its own place the value crosses « CANH » and the bus.
  const v1 = comp(1, "dc_supply", [0, 0], 90, "V1");
  const bus = comp(2, "can_bus", [0, 8], 0, "BUS1");
  const farBus = comp(2, "can_bus", [30, 8], 0, "BUS1");
  const ownBoxes = buildComp(v1, libDef("dc_supply"), monoMeasure).texts.map((t) => t.box);

  it("moves the source value off the neighbour's « CANH »: no text box meets the other component", () => {
    const model = makeModel(makeState([v1, bus], []));
    const g1 = model.comps.get(1)!;
    const g2 = model.comps.get(2)!;
    const canh = g2.texts.find((t) => t.text === "CANH")!;
    // the collision the pass removes
    expect(ownBoxes.some((b) => intersects(b, canh.box))).toBe(true);
    for (const t of g1.texts) {
      for (const u of g2.texts) expect(intersects(t.box, u.box), `${t.text} / ${u.text}`).toBe(false);
      expect(intersects(t.box, g2.drawing), t.text).toBe(false);
      for (const m of g2.marks) expect(intersects(t.box, m), t.text).toBe(false);
      expect(contains(g1.bounds, t.box)).toBe(true);
    }
    // its own texts stay apart, off its own leads, the value next to its body
    const [label, value] = g1.texts;
    expect(intersects(label.box, value.box)).toBe(false);
    for (const m of g1.marks) expect(intersects(value.box, m)).toBe(false);
    expect(value.box.x1 >= g1.body.x0 && value.box.x0 <= g1.body.x1).toBe(true);
    // the bus keeps its texts; the index follows the new draw-bounds (culling, hit test)
    expect(g2.texts).toEqual(buildComp(bus, libDef("can_bus"), monoMeasure).texts);
    expect(model.index.query(value.box).some((it) => it.t === "comp" && it.id === 1)).toBe(true);
  });

  it("gives the same places after a patch as after a rebuild, repaints old and new places, and moves back", () => {
    const model = makeModel(makeState([v1, farBus], []));
    expect(model.comps.get(1)!.texts.map((t) => t.box)).toEqual(ownBoxes);
    const dirty = model.applyPatch({ components: [[2, farBus, bus]], wires: [] });
    expect(model.comps.get(1)!.texts).toEqual(makeModel(makeState([v1, bus], [])).comps.get(1)!.texts);
    const covered = (r: Rect): boolean => dirty.some((d) => contains(d, r));
    for (const b of ownBoxes) expect(covered(b)).toBe(true);
    for (const t of model.comps.get(1)!.texts) expect(covered(t.box)).toBe(true);
    model.applyPatch({ components: [[2, bus, null]], wires: [] });
    expect(model.comps.get(1)!.texts.map((t) => t.box)).toEqual(ownBoxes);
  });
});
