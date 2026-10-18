// Virtualised list logic: visible window and row recycling (no DOM).

import { describe, expect, it } from "vitest";
import { ROW_HEIGHT, RowRecycler, poolSize, visibleWindow } from "./vlist";

/** Deterministic pseudo-random sequence (LCG), so failures replay. */
function lcg(seed: number): () => number {
  let s = seed >>> 0;
  return () => {
    s = (Math.imul(s, 1664525) + 1013904223) >>> 0;
    return s / 2 ** 32;
  };
}

describe("visibleWindow", () => {
  it("covers the viewport plus the overscan, clamped to the list", () => {
    expect(visibleWindow(0, 100, 0)).toEqual({ first: 0, end: 0 });
    expect(visibleWindow(0, 0, 50)).toEqual({ first: 0, end: 0 });
    expect(visibleWindow(0, 100, 1000)).toEqual({ first: 0, end: 7 });
    expect(visibleWindow(1000, 100, 1000)).toEqual({ first: 48, end: 57 });
    expect(visibleWindow(1010, 100, 1000)).toEqual({ first: 48, end: 58 });
    expect(visibleWindow(19950, 100, 1000)).toEqual({ first: 995, end: 1000 });
    expect(visibleWindow(0, 400, 3)).toEqual({ first: 0, end: 3 });
    expect(visibleWindow(-30, 100, 1000)).toEqual({ first: 0, end: 7 });
    // Scrolled past the end of a list that just shrank.
    expect(visibleWindow(5000, 100, 10)).toEqual({ first: 10, end: 10 });
  });

  it("always contains every row intersecting the viewport and fits in the pool", () => {
    const rnd = lcg(7);
    for (let i = 0; i < 2000; i++) {
      const total = Math.floor(rnd() * 400);
      const viewport = 1 + Math.floor(rnd() * 700);
      const scrollTop = Math.floor(rnd() * Math.max(1, total * ROW_HEIGHT - viewport + 1));
      const { first, end } = visibleWindow(scrollTop, viewport, total);
      expect(end - first).toBeLessThanOrEqual(poolSize(viewport));
      for (let row = 0; row < total; row++) {
        const top = row * ROW_HEIGHT;
        const intersects = top < scrollTop + viewport && top + ROW_HEIGHT > scrollTop;
        if (intersects) {
          expect(row).toBeGreaterThanOrEqual(first);
          expect(row).toBeLessThan(end);
        }
      }
    }
  });
});

describe("RowRecycler", () => {
  const assigned = (r: RowRecycler) => {
    const out: number[] = [];
    for (let slot = 0; slot < r.size; slot++) if (r.indexOf(slot) >= 0) out.push(r.indexOf(slot));
    return out.sort((a, b) => a - b);
  };

  it("fills the pool in order, then recycles only the rows that left the window", () => {
    const r = new RowRecycler();
    r.ensure(5);
    expect(r.assign(0, 5)).toEqual([0, 1, 2, 3, 4].map((i) => ({ slot: i, index: i })));
    expect(r.assign(0, 5)).toEqual([]);
    // One row down: the slot of row 0 now shows row 5, nothing else moves.
    expect(r.assign(1, 6)).toEqual([{ slot: 0, index: 5 }]);
    expect(r.assign(3, 8)).toEqual([{ slot: 1, index: 6 }, { slot: 2, index: 7 }]);
    expect(assigned(r)).toEqual([3, 4, 5, 6, 7]);
  });

  it("frees the slots of a shrinking window once", () => {
    const r = new RowRecycler();
    r.ensure(4);
    r.assign(0, 4);
    expect(r.assign(0, 2)).toEqual([{ slot: 2, index: -1 }, { slot: 3, index: -1 }]);
    expect(r.assign(0, 2)).toEqual([]);
    expect(assigned(r)).toEqual([0, 1]);
  });

  it("refuses a window larger than the pool", () => {
    const r = new RowRecycler();
    r.ensure(3);
    expect(() => r.assign(0, 4)).toThrow();
  });

  it("keeps the window exactly assigned under random scrolling and resizing", () => {
    const rnd = lcg(42);
    const r = new RowRecycler();
    let previous = new Map<number, number>();
    for (let step = 0; step < 3000; step++) {
      const viewport = 20 + Math.floor(rnd() * 600);
      const total = Math.floor(rnd() * 500);
      const scrollTop = Math.floor(rnd() * total * ROW_HEIGHT);
      r.ensure(poolSize(viewport));
      const { first, end } = visibleWindow(scrollTop, viewport, total);
      const changes = r.assign(first, end);
      // Exactly the window, each index once.
      expect(assigned(r)).toEqual(Array.from({ length: end - first }, (_, i) => first + i));
      // A slot that kept a visible index is not reported (its row is neither moved nor rewritten).
      const changed = new Set(changes.map((c) => c.slot));
      for (let slot = 0; slot < r.size; slot++) {
        const index = r.indexOf(slot);
        if (index >= 0 && previous.get(slot) === index) expect(changed.has(slot)).toBe(false);
        if (index >= 0 && previous.get(slot) !== index) expect(changed.has(slot)).toBe(true);
      }
      previous = new Map(Array.from({ length: r.size }, (_, slot) => [slot, r.indexOf(slot)]));
    }
  });
});
