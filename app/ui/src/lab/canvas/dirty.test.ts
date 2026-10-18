import { describe, expect, it } from "vitest";
import { DirtyRegion, unionDev } from "./dirty";

describe("dirty region", () => {
  it("repaints the union of the changed rectangles, snapped outwards and clipped to the view", () => {
    const d = new DirtyRegion();
    d.add({ x0: 10.5, y0: 20.2, x1: 30.1, y1: 40 });
    d.add({ x0: 25, y0: 5.7, x1: 50.3, y1: 22 });
    expect(d.take(400, 300)).toEqual({ kind: "rect", rect: { x0: 10, y0: 5, x1: 51, y1: 40 } });
    expect(d.take(400, 300)).toEqual({ kind: "none" });
    d.add({ x0: -20, y0: -20, x1: 10, y1: 10 });
    expect(d.take(400, 300)).toEqual({ kind: "rect", rect: { x0: 0, y0: 0, x1: 10, y1: 10 } });
  });

  it("ignores empty and off-screen rectangles", () => {
    const d = new DirtyRegion();
    d.add({ x0: 5, y0: 5, x1: 5, y1: 9 });
    expect(d.pending).toBe(false);
    d.add({ x0: 500, y0: 10, x1: 600, y1: 20 });
    expect(d.take(400, 300)).toEqual({ kind: "none" });
  });

  it("repaints everything above 50 % of the view, or after markFull", () => {
    const d = new DirtyRegion();
    d.add({ x0: 0, y0: 0, x1: 200, y1: 300 });
    expect(d.take(400, 300)).toEqual({ kind: "rect", rect: { x0: 0, y0: 0, x1: 200, y1: 300 } });
    d.add({ x0: 0, y0: 0, x1: 201, y1: 300 });
    expect(d.take(400, 300)).toEqual({ kind: "full" });
    d.add({ x0: 0, y0: 0, x1: 10, y1: 10 });
    d.markFull();
    expect(d.take(400, 300)).toEqual({ kind: "full" });
    expect(d.pending).toBe(false);
  });

  it("unions rectangles", () => {
    expect(unionDev({ x0: 0, y0: 5, x1: 10, y1: 6 }, { x0: -3, y0: 7, x1: 2, y1: 9 })).toEqual({ x0: -3, y0: 5, x1: 10, y1: 9 });
  });
});
