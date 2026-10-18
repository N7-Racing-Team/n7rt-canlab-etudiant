import { describe, expect, it } from "vitest";
import { hitTest, marqueeMode, marqueeSelect } from "./hit";
import { comp, makeModel, makeState, wire } from "./testkit";

// S1 switch at (0,0): pins (±3, 0), toggle zone [−1, 1] × [−2, 0]. R1 at (10,0): pins (7,0), (13,0), body
// [8, 12] × [−1, 1]. J1 junction at (20,0). C1 at (30,0): pin p1 (27,0). R2 at (20,10) turned 90°: pins (20,7) and
// (20,13). Wires 10: S1.p2–R1.p1, 11: R1.p2–J1, 12: J1–C1.p1, 13: J1–R2.p1.
function scene() {
  const state = makeState(
    [comp(1, "switch", [0, 0], 0, "S1"), comp(2, "resistor", [10, 0], 0, "R1"), comp(3, "junction", [20, 0]),
      comp(4, "capacitor", [30, 0], 0, "C1"), comp(5, "resistor", [20, 10], 90, "R2")],
    [wire(10, [1, "p2"], [2, "p1"]), wire(11, [2, "p2"], [3, "j"]), wire(12, [3, "j"], [4, "p1"]), wire(13, [3, "j"], [5, "p1"])],
  );
  return makeModel(state);
}

const TOL = 0.5;

describe("hit test priority: actuator > pin > junction > wire > body", () => {
  const m = scene();

  it("actuates before selecting the body around the zone", () => {
    expect(hitTest(m, [0, -1], TOL)).toEqual({ t: "actuator", comp: 1, kind: "toggle_switch" });
    expect(hitTest(m, [0, -1], TOL, { actuators: false })).toEqual({ t: "body", comp: 1 });
  });

  it("prefers a pin to the wire that ends on it", () => {
    expect(hitTest(m, [3.2, 0.1], TOL)).toEqual({ t: "pin", comp: 1, pin: "p2", pos: [3, 0] });
  });

  it("finds a junction, then a wire interior with its nearest grid point", () => {
    expect(hitTest(m, [20.2, -0.2], TOL)).toEqual({ t: "junction", comp: 3, pin: "j", pos: [20, 0] });
    expect(hitTest(m, [5.1, 0.3], TOL)).toEqual({ t: "wire", wire: 10, seg: 0, at: [5, 0] });
    expect(hitTest(m, [16.4, 0.2], TOL)).toEqual({ t: "wire", wire: 11, seg: 0, at: [16, 0] });
    expect(hitTest(m, [20.1, 3.4], TOL)).toEqual({ t: "wire", wire: 13, seg: 0, at: [20, 3] });
  });

  it("falls back on the body, and finds nothing outside the tolerance", () => {
    expect(hitTest(m, [10, 0.9], TOL)).toEqual({ t: "body", comp: 2 });
    expect(hitTest(m, [10, 0], TOL)).toEqual({ t: "body", comp: 2 });
    expect(hitTest(m, [5, 0.6], TOL)).toBeNull();
    expect(hitTest(m, [50, 50], TOL)).toBeNull();
    expect(hitTest(m, [10, 0], TOL, { bodies: false })).toBeNull();
  });

  it("keeps the priority when the tolerance covers several items (zoomed out)", () => {
    const tol = 5 / 2.62144; // 5 CSS px at k = −6
    expect(hitTest(m, [14, 0], tol)).toMatchObject({ t: "pin", comp: 2, pin: "p2" });
    expect(hitTest(m, [21, 0], tol)).toMatchObject({ t: "junction", comp: 3 });
  });

  it("ignores the wire whose end is being dragged", () => {
    expect(hitTest(m, [5, 0.2], TOL, { excludeWire: 10 })).toBeNull();
  });
});

describe("marquee", () => {
  const m = scene();

  it("selects enclosed items from left to right, touched items from right to left", () => {
    const r = { x0: 6.5, y0: -2, x1: 13.5, y1: 2 };
    expect(marqueeMode([6.5, -2], [13.5, 2])).toBe("enclosed");
    expect(marqueeMode([13.5, 2], [6.5, -2])).toBe("touched");
    expect(marqueeSelect(m, r, "enclosed")).toEqual({ comps: [2], wires: [] });
    expect(marqueeSelect(m, r, "touched")).toEqual({ comps: [2], wires: [10, 11] });
  });

  it("takes a junction by its point", () => {
    const r = { x0: 19.5, y0: -0.5, x1: 20.5, y1: 0.5 };
    expect(marqueeSelect(m, r, "enclosed")).toEqual({ comps: [3], wires: [] });
    expect(marqueeSelect(m, r, "touched")).toEqual({ comps: [3], wires: [11, 12, 13] });
  });

  it("takes everything inside a large rectangle", () => {
    expect(marqueeSelect(m, { x0: -10, y0: -10, x1: 40, y1: 20 }, "enclosed")).toEqual({ comps: [1, 2, 3, 4, 5], wires: [10, 11, 12, 13] });
  });
});
