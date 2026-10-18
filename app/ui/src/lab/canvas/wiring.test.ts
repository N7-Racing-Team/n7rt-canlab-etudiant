import { describe, expect, it } from "vitest";
import type { Pt, Rot } from "../types";
import { canonicalPath, isOrthogonal, rotateDir, rotatePt, rotateRect } from "./geom";
import { bendsOf, dragSegment, isHorizontal } from "./wiring";

describe("rotation (contract §1.3)", () => {
  it("turns clockwise with y down", () => {
    expect(rotatePt([2, 1], 90)).toEqual([-1, 2]);
    expect(rotatePt([2, 1], 180)).toEqual([-2, -1]);
    expect(rotatePt([2, 1], 270)).toEqual([1, -2]);
    expect(rotatePt([0, 3], 180)).toEqual([0, -3]);
    expect(Object.is(rotatePt([0, 3], 90)[1], 0)).toBe(true);
    expect((["l", "r", "u", "d"] as const).map((d) => rotateDir(d, 90))).toEqual(["u", "d", "r", "l"]);
    expect(rotateRect({ x0: -2, y0: -2, x1: 2, y1: 1 }, 90)).toEqual({ x0: -1, y0: -2, x1: 2, y1: 2 });
  });

  it("returns to the start after four quarter turns", () => {
    let p: [number, number] = [5, -7];
    for (let i = 0; i < 4; i++) p = rotatePt(p, 90 as Rot);
    expect(p).toEqual([5, -7]);
  });
});

describe("canonical path (contract §3.4.1)", () => {
  it("drops zero-length segments, aligned points and spikes, keeping the ends", () => {
    expect(canonicalPath([[0, 0], [0, 0], [3, 0], [5, 0], [5, 4]])).toEqual([[0, 0], [5, 0], [5, 4]]);
    expect(canonicalPath([[0, 0], [6, 0], [2, 0], [2, 3]])).toEqual([[0, 0], [2, 0], [2, 3]]);
    expect(canonicalPath([[0, 0], [4, 0], [4, 0], [4, 5], [4, 5]])).toEqual([[0, 0], [4, 0], [4, 5]]);
    expect(canonicalPath([[1, 1], [1, 1]])).toEqual([[1, 1]]);
  });
});

describe("segment drag (contract §8.5 « Segment »)", () => {
  it("moves both ends of an interior segment along its normal", () => {
    const path: Pt[] = [[0, 0], [5, 0], [5, 5], [10, 5]];
    expect(dragSegment(path, 1, 2)).toEqual([[0, 0], [7, 0], [7, 5], [10, 5]]);
    expect(bendsOf(dragSegment(path, 1, 2))).toEqual([[7, 0], [7, 5]]);
  });

  it("duplicates the end point first when the segment touches a pin", () => {
    const path: Pt[] = [[0, 0], [5, 0], [5, 5]];
    expect(dragSegment(path, 0, -3)).toEqual([[0, 0], [0, -3], [5, -3], [5, 5]]);
    expect(bendsOf(dragSegment(path, 0, -3))).toEqual([[0, -3], [5, -3]]);
    expect(dragSegment(path, 1, 2)).toEqual([[0, 0], [7, 0], [7, 5], [5, 5]]);
  });

  it("turns a straight wire into [Q0, Q0 + t·n, Q1 + t·n, Q1]", () => {
    expect(dragSegment([[0, 0], [10, 0]], 0, 4)).toEqual([[0, 0], [0, 4], [10, 4], [10, 0]]);
    expect(dragSegment([[0, 0], [0, 10]], 0, -2)).toEqual([[0, 0], [-2, 0], [-2, 10], [0, 10]]);
    expect(bendsOf(dragSegment([[0, 0], [10, 0]], 0, 4))).toEqual([[0, 4], [10, 4]]);
  });

  it("changes nothing for t = 0 or a segment that does not exist", () => {
    const path: Pt[] = [[0, 0], [5, 0], [5, 5]];
    expect(dragSegment(path, 1, 0)).toEqual(path);
    expect(dragSegment(path, 2, 3)).toEqual(path);
  });

  it("keeps the ends, stays orthogonal and moves the dragged segment by t (random paths)", () => {
    let s = 7;
    const r = (n: number): number => {
      s = (Math.imul(s, 1103515245) + 12345) >>> 0;
      return (s >>> 8) % n;
    };
    for (let trial = 0; trial < 300; trial++) {
      const path: Pt[] = [[r(20) - 10, r(20) - 10]];
      let horizontal = r(2) === 0;
      const n = 1 + r(5);
      for (let i = 0; i < n; i++) {
        const last = path[path.length - 1];
        const d = (1 + r(6)) * (r(2) ? 1 : -1);
        path.push(horizontal ? [last[0] + d, last[1]] : [last[0], last[1] + d]);
        horizontal = !horizontal;
      }
      const seg = r(n);
      const t = (1 + r(4)) * (r(2) ? 1 : -1);
      const out = dragSegment(path, seg, t);
      expect(out[0]).toEqual(path[0]);
      expect(out[out.length - 1]).toEqual(path[path.length - 1]);
      expect(isOrthogonal(out)).toBe(true);
      const h = isHorizontal(path[seg], path[seg + 1]);
      // The moved copy of the segment: same length, shifted by t along its normal.
      const moved = out.some((p, i) => i + 1 < out.length &&
        (h ? p[1] === path[seg][1] + t && out[i + 1][1] === p[1] && Math.abs(out[i + 1][0] - p[0]) === Math.abs(path[seg + 1][0] - path[seg][0])
          : p[0] === path[seg][0] + t && out[i + 1][0] === p[0] && Math.abs(out[i + 1][1] - p[1]) === Math.abs(path[seg + 1][1] - path[seg][1])));
      expect(moved).toBe(true);
    }
  });
});
