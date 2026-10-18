import { describe, expect, it } from "vitest";
import { K_MAX, K_MIN, Viewport, stepNearest, strokePx, unitPx, zoomOf, zoomPercent } from "./view";

describe("zoom ladder", () => {
  it("is z = 1.25^k on k ∈ [−6, 6]", () => {
    expect(K_MIN).toBe(-6);
    expect(K_MAX).toBe(6);
    expect(zoomOf(0)).toBe(1);
    expect(zoomOf(1)).toBe(1.25);
    expect(zoomOf(-1)).toBe(0.8);
    expect(zoomOf(6)).toBeCloseTo(3.814697265625, 12);
    expect(zoomOf(-6)).toBeCloseTo(0.262144, 12);
  });

  it("gives one unit = round(10·z·dpr) device pixels", () => {
    expect([-6, -1, 0, 1, 2, 6].map((k) => unitPx(k, 1))).toEqual([3, 8, 10, 13, 16, 38]);
    expect([-6, 0, 1].map((k) => unitPx(k, 2))).toEqual([5, 20, 25]);
    expect(unitPx(0, 1.25)).toBe(13);
  });

  it("draws strokes of max(1, round(1.5·dpr)) device pixels", () => {
    expect([0.5, 1, 1.25, 1.5, 2, 3].map((d) => strokePx(d))).toEqual([1, 2, 2, 2, 3, 5]);
  });

  it("maps ratios and percentages to the closest step", () => {
    expect(stepNearest(1)).toBe(0);
    expect(stepNearest(1.25)).toBe(1);
    expect(stepNearest(1.1)).toBe(0);
    expect(stepNearest(0.5)).toBe(-3);
    expect(stepNearest(100)).toBe(6);
    expect(stepNearest(0.01)).toBe(-6);
    expect(stepNearest(0)).toBe(0);
    expect(stepNearest(Number.NaN)).toBe(0);
    expect([-6, -1, 0, 1, 2].map(zoomPercent)).toEqual([26, 80, 100, 125, 156]);
  });
});

describe("world ↔ device", () => {
  it("round-trips and puts every grid point on a whole device pixel", () => {
    for (const dpr of [1, 1.25, 1.5, 2]) {
      for (let k = K_MIN; k <= K_MAX; k++) {
        const vp = new Viewport();
        vp.dpr = dpr;
        vp.k = k;
        vp.ox = 37;
        vp.oy = -12;
        for (const p of [[0, 0], [7, -3], [-10000, 10000], [123, 45]] as [number, number][]) {
          const d = vp.toDevice(p);
          expect(Number.isInteger(d[0]) && Number.isInteger(d[1])).toBe(true);
          expect(vp.toWorld(d[0], d[1])).toEqual(p);
        }
      }
    }
  });

  it("zooms about the cursor and clamps the step", () => {
    const vp = new Viewport();
    vp.resize(800, 600);
    vp.ox = 100;
    vp.oy = 50;
    const anchor: [number, number] = [313, 207];
    const before = vp.toWorld(...anchor);
    expect(vp.zoomAt(3, ...anchor)).toBe(true);
    const after = vp.toWorld(...anchor);
    expect(Math.abs(after[0] - before[0])).toBeLessThanOrEqual(0.5 / vp.u);
    expect(Math.abs(after[1] - before[1])).toBeLessThanOrEqual(0.5 / vp.u);
    expect(Number.isInteger(vp.ox) && Number.isInteger(vp.oy)).toBe(true);
    expect(vp.zoomAt(40, 0, 0)).toBe(true);
    expect(vp.k).toBe(K_MAX);
    expect(vp.zoomAt(9, 0, 0)).toBe(false);
  });

  it("pans by whole device pixels", () => {
    const vp = new Viewport();
    expect(vp.panBy(2.4, -3.6)).toBe(true);
    expect([vp.ox, vp.oy]).toEqual([2, -4]);
    expect(vp.panBy(0.2, -0.3)).toBe(false);
  });

  it("fits bounds at the largest step that leaves the margin", () => {
    const vp = new Viewport();
    vp.resize(1000, 600);
    vp.fit({ x0: 0, y0: 0, x1: 100, y1: 50 });
    // 100 units must fit in 1000 − 2·24 = 952 px: u = 8 (k = −1); u = 10 would need 1000 px.
    expect(vp.k).toBe(-1);
    expect([vp.ox, vp.oy]).toEqual([100, 100]);
    vp.fit(null);
    expect(vp.k).toBe(0);
    expect(vp.toWorld(500, 300)).toEqual([0, 0]);
  });

  it("keeps the centre of the view when the device pixel ratio changes", () => {
    const vp = new Viewport();
    vp.resize(800, 600);
    vp.ox = 400 - 25 * 10;
    vp.oy = 300 - 10 * 10;
    vp.setDpr(2, 1600, 1200);
    expect(vp.u).toBe(20);
    expect(vp.toWorld(800, 600)).toEqual([25, 10]);
  });
});
