// Path rules of the wire gestures (contract §8.5 "Fil", "Segment"). They shape what the UI proposes; the core
// canonicalises and validates every proposal (previews). Pure: no DOM.

import type { Pt } from "../types";
import { canonicalPath } from "./geom";

const copy = (p: Pt): Pt => [p[0], p[1]];

/** Corner of the L from `from` to `to`: horizontal leg first when hFirst. */
export const elbow = (from: Pt, to: Pt, hFirst: boolean): Pt => (hFirst ? [to[0], from[1]] : [from[0], to[1]]);

/** A horizontal segment is dragged along y, a vertical one along x. */
export const isHorizontal = (a: Pt, b: Pt): boolean => a[1] === b[1];

/**
 * Full path after dragging segment `seg` (between path[seg] and path[seg + 1]) by t units along its normal:
 * - interior segment k: Q_k += t·n and Q_k+1 += t·n;
 * - segment touching a pin: that end point is duplicated first (perpendicular stub), then the copy moves;
 * - straight wire: [Q0, Q0 + t·n, Q1 + t·n, Q1].
 */
export function dragSegment(path: readonly Pt[], seg: number, t: number): Pt[] {
  const m = path.length - 1;
  const q = path.map(copy);
  if (m < 1 || seg < 0 || seg >= m || t === 0) return q;
  const n: Pt = isHorizontal(path[seg], path[seg + 1]) ? [0, 1] : [1, 0];
  const sh = (p: Pt): Pt => [p[0] + t * n[0], p[1] + t * n[1]];
  if (m === 1) return [q[0], sh(q[0]), sh(q[1]), q[1]];
  if (seg === 0) return [q[0], sh(q[0]), sh(q[1]), ...q.slice(2)];
  if (seg === m - 1) return [...q.slice(0, m - 1), sh(q[m - 1]), sh(q[m]), q[m]];
  q[seg] = sh(q[seg]);
  q[seg + 1] = sh(q[seg + 1]);
  return q;
}

/** Bends of a full path as sent in an edit: the interior points of its canonical form. */
export const bendsOf = (path: readonly Pt[]): Pt[] => canonicalPath(path).slice(1, -1);
