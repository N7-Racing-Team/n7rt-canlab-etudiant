import { describe, expect, it } from "vitest";
import type { Rect } from "../types";
import { intersects } from "./geom";
import { CELL, SpatialHash } from "./spatial";

/** Deterministic pseudo-random sequence (LCG), so failures reproduce. */
function rng(seed: number): () => number {
  let s = seed >>> 0;
  return () => {
    s = (Math.imul(s, 1664525) + 1013904223) >>> 0;
    return s / 2 ** 32;
  };
}

function randomRect(r: () => number, span: number): Rect {
  const x0 = Math.round((r() - 0.5) * span);
  const y0 = Math.round((r() - 0.5) * span);
  return { x0, y0, x1: x0 + Math.round(r() * 40), y1: y0 + Math.round(r() * 40) };
}

const sorted = (xs: string[]): string[] => [...xs].sort();

describe("spatial hash", () => {
  it("answers every query like a linear scan, through inserts, updates and removals", () => {
    const r = rng(42);
    const h = new SpatialHash<string>();
    const truth = new Map<string, Rect>();
    for (let i = 0; i < 400; i++) {
      const key = `k${i}`;
      const rc = randomRect(r, 600);
      h.set(key, rc, key);
      truth.set(key, rc);
    }
    for (let i = 0; i < 150; i++) {
      const key = `k${Math.floor(r() * 400)}`;
      if (r() < 0.5) {
        const rc = randomRect(r, 600);
        h.set(key, rc, key);
        truth.set(key, rc);
      } else {
        expect(h.delete(key)).toBe(truth.delete(key));
      }
    }
    expect(h.size).toBe(truth.size);
    for (let q = 0; q < 200; q++) {
      const rc = randomRect(r, 700);
      const expected = [...truth].filter(([, v]) => intersects(v, rc)).map(([k]) => k);
      expect(sorted(h.query(rc))).toEqual(sorted(expected));
    }
    // A query wider than the occupied area takes the scan path: same answer.
    const all = { x0: -1e5, y0: -1e5, x1: 1e5, y1: 1e5 };
    expect(sorted(h.query(all))).toEqual(sorted([...truth.keys()]));
  });

  it("moves an updated item out of its old cells", () => {
    const h = new SpatialHash<number>();
    h.set("a", { x0: 0, y0: 0, x1: 2, y1: 2 }, 1);
    h.set("a", { x0: 100, y0: 100, x1: 101, y1: 101 }, 2);
    expect(h.query({ x0: 0, y0: 0, x1: 3, y1: 3 })).toEqual([]);
    expect(h.query({ x0: 99, y0: 99, x1: 100, y1: 100 })).toEqual([2]);
    expect(h.size).toBe(1);
  });

  it("finds items on cell boundaries and at negative coordinates, each once", () => {
    const h = new SpatialHash<string>();
    h.set("edge", { x0: CELL, y0: 0, x1: CELL, y1: 0 }, "edge");
    h.set("span", { x0: -40, y0: -40, x1: 40, y1: 40 }, "span");
    expect(sorted(h.query({ x0: CELL - 1, y0: -1, x1: CELL, y1: 1 }))).toEqual(["edge", "span"]);
    expect(h.query({ x0: CELL + 0.5, y0: 0, x1: CELL + 1, y1: 0 })).toEqual(["span"]);
    expect(h.query({ x0: -41, y0: -41, x1: -40, y1: -40 })).toEqual(["span"]);
    expect(h.delete("span")).toBe(true);
    expect(h.delete("span")).toBe(false);
    expect(h.query({ x0: -41, y0: -41, x1: 41, y1: 41 })).toEqual(["edge"]);
  });
});
