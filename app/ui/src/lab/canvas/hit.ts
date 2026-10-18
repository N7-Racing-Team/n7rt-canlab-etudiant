// Pointing and marquee selection on the derived geometry (contract §8.5 "Pointage", "Rectangle de sélection").
// Priority: actuator > pin > junction > wire interior > body; the tolerance is 5 CSS px expressed in grid units by
// the caller. Pure: no DOM.

import type { ActuatorKind, CompId, Pt, Rect, WireId } from "../types";
import { type FPt, area, contains, containsPoint, distToSegment, intersects, nearestOnSegment, segmentTouchesRect, snapPt } from "./geom";
import type { SceneModel } from "./model";

export type Hit =
  | { t: "actuator"; comp: CompId; kind: ActuatorKind }
  | { t: "pin"; comp: CompId; pin: string; pos: Pt }
  | { t: "junction"; comp: CompId; pin: string; pos: Pt }
  /** `at`: grid point of the segment nearest to the pointer. */
  | { t: "wire"; wire: WireId; seg: number; at: Pt }
  | { t: "body"; comp: CompId };

export interface HitFilter {
  /** Default true. */
  actuators?: boolean;
  /** Default true. */
  bodies?: boolean;
  /** Ignore this wire (its own end being dragged). */
  excludeWire?: WireId;
}

const cheb = (p: FPt, q: FPt): number => Math.max(Math.abs(p[0] - q[0]), Math.abs(p[1] - q[1]));
const dist = (p: FPt, q: FPt): number => Math.hypot(p[0] - q[0], p[1] - q[1]);

export function hitTest(model: SceneModel, p: FPt, tol: number, filter: HitFilter = {}): Hit | null {
  const items = model.index.query({ x0: p[0] - tol, y0: p[1] - tol, x1: p[0] + tol, y1: p[1] + tol });
  let actuator: Hit | null = null;
  let pin: { hit: Hit; d: number } | null = null;
  let junction: { hit: Hit; d: number } | null = null;
  let wire: { hit: Hit; d: number } | null = null;
  let body: { comp: CompId; a: number } | null = null;

  for (const it of items) {
    if (it.t === "comp") {
      const g = model.comps.get(it.id);
      if (!g) continue;
      if (filter.actuators !== false) {
        for (const a of g.actuators) {
          if (containsPoint(a.zone, p) && (!actuator || (actuator.t === "actuator" && it.id > actuator.comp))) {
            actuator = { t: "actuator", comp: it.id, kind: a.kind };
          }
        }
      }
      if (filter.bodies !== false && containsPoint(g.body, p)) {
        const a = area(g.body);
        if (!body || a < body.a || (a === body.a && it.id > body.comp)) body = { comp: it.id, a };
      }
    } else if (it.t === "pin") {
      const pos = model.pinPos({ comp: it.comp, pin: it.pin });
      if (!pos || cheb(p, pos) > tol) continue;
      const d = dist(p, pos);
      if (!pin || d < pin.d) pin = { hit: { t: "pin", comp: it.comp, pin: it.pin, pos }, d };
    } else if (it.t === "junction") {
      const pg = model.comps.get(it.id)?.pins[0];
      if (!pg || cheb(p, pg.pos) > tol) continue;
      const d = dist(p, pg.pos);
      if (!junction || d < junction.d) junction = { hit: { t: "junction", comp: it.id, pin: pg.id, pos: pg.pos }, d };
    } else {
      if (it.wire === filter.excludeWire) continue;
      const g = model.wires.get(it.wire);
      if (!g) continue;
      const a = g.path[it.k];
      const b = g.path[it.k + 1];
      const d = distToSegment(p, a, b);
      if (d > tol) continue;
      if (!wire || d < wire.d) wire = { hit: { t: "wire", wire: it.wire, seg: it.k, at: snapPt(nearestOnSegment(p, a, b)) }, d };
    }
  }
  if (actuator) return actuator;
  if (pin) return pin.hit;
  if (junction) return junction.hit;
  if (wire) return wire.hit;
  return body ? { t: "body", comp: body.comp } : null;
}

export type MarqueeMode = "enclosed" | "touched";

/** Left to right: items entirely inside; right to left: items touched (contract §8.5). */
export const marqueeMode = (a: FPt, b: FPt): MarqueeMode => (b[0] >= a[0] ? "enclosed" : "touched");

/** Components (junctions by their point, others by body, leads and drawing) and wires selected by a marquee. */
export function marqueeSelect(model: SceneModel, r: Rect, mode: MarqueeMode): { comps: CompId[]; wires: WireId[] } {
  const comps = new Set<CompId>();
  const wires = new Set<WireId>();
  for (const it of model.index.query(r)) {
    if (it.t === "comp") {
      const g = model.comps.get(it.id);
      if (g && (mode === "enclosed" ? contains(r, g.extent) : intersects(r, g.extent))) comps.add(it.id);
    } else if (it.t === "junction") {
      const pos = model.comps.get(it.id)?.pins[0]?.pos;
      if (pos && containsPoint(r, pos)) comps.add(it.id);
    } else if (it.t === "seg" && !wires.has(it.wire)) {
      const g = model.wires.get(it.wire);
      if (!g) continue;
      const hit = mode === "enclosed"
        ? g.path.every((q) => containsPoint(r, q))
        : g.path.some((q, i) => i > 0 && segmentTouchesRect(g.path[i - 1], q, r));
      if (hit) wires.add(it.wire);
    }
  }
  return { comps: [...comps].sort((a, b) => a - b), wires: [...wires].sort((a, b) => a - b) };
}
