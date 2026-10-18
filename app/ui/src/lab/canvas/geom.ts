// Pure geometry of the schematic editor: grid points, inclusive rectangles, clockwise rotations (contract §1.3),
// axis-aligned segments and the canonical path form (§3.4.1). No DOM.

import type { Dir, Pt, Rect, Rot } from "../types";

/** Continuous world point in grid units (pointer position, text anchor); grid points are integer FPts. */
export type FPt = [number, number];

/** -0 → 0: rotated coordinates must compare and serialise like the core's integers. */
const nz = (v: number): number => (v === 0 ? 0 : v);

export const samePt = (a: FPt, b: FPt): boolean => a[0] === b[0] && a[1] === b[1];

export const snapPt = (p: FPt): Pt => [nz(Math.round(p[0])), nz(Math.round(p[1]))];

/** Clockwise about the origin, y down: R90 (x, y) → (−y, x), R180 → (−x, −y), R270 → (y, −x). */
export function rotatePt(p: FPt, rot: Rot): FPt {
  const [x, y] = p;
  switch (rot) {
    case 90: return [nz(-y), nz(x)];
    case 180: return [nz(-x), nz(-y)];
    case 270: return [nz(y), nz(-x)];
    default: return [nz(x), nz(y)];
  }
}

/** Inverse rotation: the rotation that brings a rotated point back. */
export const inverseRot = (rot: Rot): Rot => ((360 - rot) % 360) as Rot;

const DIR_VEC: Record<Dir, FPt> = { l: [-1, 0], r: [1, 0], u: [0, -1], d: [0, 1] };

export const dirVec = (d: Dir): FPt => DIR_VEC[d];

export function dirOf(v: FPt): Dir {
  if (Math.abs(v[0]) >= Math.abs(v[1])) return v[0] < 0 ? "l" : "r";
  return v[1] < 0 ? "u" : "d";
}

export const rotateDir = (d: Dir, rot: Rot): Dir => dirOf(rotatePt(dirVec(d), rot));

export function rect(x0: number, y0: number, x1: number, y1: number): Rect {
  return { x0: Math.min(x0, x1), y0: Math.min(y0, y1), x1: Math.max(x0, x1), y1: Math.max(y0, y1) };
}

export const rectOf = (a: FPt, b: FPt): Rect => rect(a[0], a[1], b[0], b[1]);

export const pointRect = (p: FPt): Rect => ({ x0: p[0], y0: p[1], x1: p[0], y1: p[1] });

export function rotateRect(r: Rect, rot: Rot): Rect {
  const a = rotatePt([r.x0, r.y0], rot);
  const b = rotatePt([r.x1, r.y1], rot);
  return rectOf(a, b);
}

export const translateRect = (r: Rect, d: FPt): Rect => ({ x0: r.x0 + d[0], y0: r.y0 + d[1], x1: r.x1 + d[0], y1: r.y1 + d[1] });

export function union(a: Rect, b: Rect): Rect {
  return { x0: Math.min(a.x0, b.x0), y0: Math.min(a.y0, b.y0), x1: Math.max(a.x1, b.x1), y1: Math.max(a.y1, b.y1) };
}

export function unionAll(rs: Iterable<Rect>): Rect | null {
  let acc: Rect | null = null;
  for (const r of rs) acc = acc ? union(acc, r) : r;
  return acc;
}

/** Closed rectangles intersect (touching edges count). */
export const intersects = (a: Rect, b: Rect): boolean => a.x0 <= b.x1 && b.x0 <= a.x1 && a.y0 <= b.y1 && b.y0 <= a.y1;

export const contains = (outer: Rect, inner: Rect): boolean =>
  inner.x0 >= outer.x0 && inner.x1 <= outer.x1 && inner.y0 >= outer.y0 && inner.y1 <= outer.y1;

export const containsPoint = (r: Rect, p: FPt, tol = 0): boolean =>
  p[0] >= r.x0 - tol && p[0] <= r.x1 + tol && p[1] >= r.y0 - tol && p[1] <= r.y1 + tol;

export const inflate = (r: Rect, d: number): Rect => ({ x0: r.x0 - d, y0: r.y0 - d, x1: r.x1 + d, y1: r.y1 + d });

export const area = (r: Rect): number => Math.max(0, r.x1 - r.x0) * Math.max(0, r.y1 - r.y0);

export function pathBounds(path: readonly FPt[]): Rect | null {
  if (path.length === 0) return null;
  let r = pointRect(path[0]);
  for (let i = 1; i < path.length; i++) r = union(r, pointRect(path[i]));
  return r;
}

/** Point of segment [a, b] closest to p. */
export function nearestOnSegment(p: FPt, a: FPt, b: FPt): FPt {
  const dx = b[0] - a[0];
  const dy = b[1] - a[1];
  const len2 = dx * dx + dy * dy;
  if (len2 === 0) return [a[0], a[1]];
  const t = Math.min(1, Math.max(0, ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len2));
  return [a[0] + t * dx, a[1] + t * dy];
}

export function distToSegment(p: FPt, a: FPt, b: FPt): number {
  const q = nearestOnSegment(p, a, b);
  return Math.hypot(p[0] - q[0], p[1] - q[1]);
}

/** Exact for axis-aligned segments: the segment's bounding box is the segment. */
export const segmentTouchesRect = (a: FPt, b: FPt, r: Rect): boolean => intersects(rectOf(a, b), r);

/**
 * Canonical form (contract §3.4.1): repeatedly drop zero-length segments and interior points aligned with both
 * neighbours (spikes included) until nothing changes; the end points are kept.
 */
export function canonicalPath(path: readonly Pt[]): Pt[] {
  let pts: Pt[] = path.map((p) => [p[0], p[1]]);
  for (;;) {
    const dedup: Pt[] = [];
    for (const p of pts) if (dedup.length === 0 || !samePt(dedup[dedup.length - 1], p)) dedup.push(p);
    let changed = dedup.length !== pts.length;
    const out: Pt[] = dedup.length ? [dedup[0]] : [];
    for (let i = 1; i < dedup.length - 1; i++) {
      const a = out[out.length - 1];
      const b = dedup[i];
      const c = dedup[i + 1];
      if ((a[0] === b[0] && b[0] === c[0]) || (a[1] === b[1] && b[1] === c[1])) {
        changed = true;
        continue;
      }
      out.push(b);
    }
    if (dedup.length > 1) out.push(dedup[dedup.length - 1]);
    pts = out;
    if (!changed) return pts;
  }
}

/** Every segment horizontal or vertical. */
export function isOrthogonal(path: readonly FPt[]): boolean {
  for (let i = 0; i + 1 < path.length; i++) {
    if (path[i][0] !== path[i + 1][0] && path[i][1] !== path[i + 1][1]) return false;
  }
  return true;
}
