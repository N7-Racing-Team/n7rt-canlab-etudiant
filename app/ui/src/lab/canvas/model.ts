// Derived geometry of the read-only document mirror: world pins, bodies, actuator zones, texts and draw-bounds of
// components, drawn paths of wires, wire count per pin, and the spatial index — rebuilt from snapshots, updated
// item by item from patches (contract §8.5). It never decides connectivity or validity: only where things are
// drawn. Pure: no DOM (text widths come from an injected measure).

import type { ActuatorKind, Component, CompId, Dir, Kind, KindDef, Patch, PinRef, Pt, Rect, Role, Wire, WireId } from "../types";
import {
  type FPt, inflate, intersects, pathBounds, pointRect, rectOf, rotateDir, rotatePt, rotateRect, translateRect, union,
} from "./geom";
import { SpatialHash } from "./spatial";
import { type Align, type TextRole, VERTICAL_AT_R0, innerTexts, symbolGeom, symbolVariant, valueText } from "./symbols";

/** Width in CSS pixels of a text set in 400 11px IBM Plex Mono. */
export type Measure = (text: string) => number;

/** Line height of schematic text in grid units: an 11·z px font over 10·z px per unit, at any zoom. */
export const TEXT_H = 1.1;
const TEXT_GAP = 0.3;
const PIN_CLEAR = 0.3;

export interface PinGeom {
  id: string;
  pos: Pt;
  dir: Dir;
  short: string;
  role: Role;
}

export interface TextItem {
  text: string;
  /** Alignment point (horizontally, along the reading direction) and middle of the line. */
  at: FPt;
  align: Align;
  /** Read bottom to top (components at 90° and 270°, DESIGN.md §7). */
  vertical: boolean;
  role: TextRole;
  box: Rect;
}

export interface CompGeom {
  comp: Component;
  def: KindDef;
  variant: string;
  /** Rotated body rectangle, world. */
  body: Rect;
  pins: PinGeom[];
  actuators: { kind: ActuatorKind; zone: Rect }[];
  texts: TextItem[];
  /** Body ∪ leads ∪ drawing, without texts: marquee selection. */
  extent: Rect;
  /** Draw-bounds (§2.2): extent ∪ text boxes. Culling and dirty rectangles. */
  bounds: Rect;
  /** Body ∪ body strokes: what the outer texts of the other components keep clear of, with `marks`. */
  drawing: Rect;
  /** Lead segments, pins and inner texts (pin names, title). */
  marks: Rect[];
  /** The designator and the value outside the body, at their own place (SceneModel moves them off neighbours). */
  outer: OuterText[];
}

type Side = "top" | "bottom" | "left" | "right";

/** An outer text: its index in `texts`, its side and its own place (§2.2), before any move off a neighbour. */
export interface OuterText {
  k: number;
  side: Side;
  own: TextItem;
}

export interface WireGeom {
  wire: Wire;
  /** [pin a, bends…, pin b]. */
  path: Pt[];
  bounds: Rect;
}

export type IndexItem =
  | { t: "comp"; id: CompId }
  | { t: "pin"; comp: CompId; pin: string }
  | { t: "junction"; id: CompId }
  | { t: "seg"; wire: WireId; k: number };

const pinKey = (p: PinRef): string => `${p.comp}:${p.pin}`;
const sameRect = (a: Rect, b: Rect): boolean => a.x0 === b.x0 && a.y0 === b.y0 && a.x1 === b.x1 && a.y1 === b.y1;
const flipAlign = (a: Align): Align => (a === "left" ? "right" : a === "right" ? "left" : a);

/** Box of a text of width w (grid units) anchored at `at`. Vertical texts read bottom to top (towards −y). */
export function textBox(w: number, at: FPt, align: Align, vertical: boolean): Rect {
  const h2 = TEXT_H / 2;
  if (!vertical) {
    const x0 = align === "left" ? at[0] : align === "right" ? at[0] - w : at[0] - w / 2;
    return { x0, y0: at[1] - h2, x1: x0 + w, y1: at[1] + h2 };
  }
  const y1 = align === "left" ? at[1] : align === "right" ? at[1] + w : at[1] + w / 2;
  return { x0: at[0] - h2, y0: y1 - w, x1: at[0] + h2, y1 };
}

const SIDE_DIR: Record<Side, Dir> = { top: "u", bottom: "d", left: "l", right: "r" };
const OPPOSITE: Record<Side, Side> = { top: "bottom", bottom: "top", left: "right", right: "left" };
/** Slides tried each way along a side, in quarter lengths of the text box (at least half a line). */
const MAX_SLIDES = 16;
/** Touching is not overlapping: text boxes are tested shrunk by this much (grid units). */
const TOUCH = 0.05;

/**
 * Designator or value outside the body (§2.2: designator above, on the left when the component is vertical; value
 * below, on the right), moved along the side away from leads that leave the body on that side.
 */
function placeOuter(text: string, role: TextRole, side: Side, body: Rect, pins: PinGeom[], vertical: boolean, measure: Measure): TextItem {
  const w = measure(text) / 10;
  const bw = vertical ? TEXT_H : w;
  const bh = vertical ? w : TEXT_H;
  const cx = (body.x0 + body.x1) / 2;
  const cy = (body.y0 + body.y1) / 2;
  let x0: number;
  let y0: number;
  if (side === "top" || side === "bottom") {
    x0 = cx - bw / 2;
    y0 = side === "top" ? body.y0 - TEXT_GAP - bh : body.y1 + TEXT_GAP;
    const xs = pins.filter((p) => p.dir === SIDE_DIR[side] && p.pos[0] >= x0 - PIN_CLEAR && p.pos[0] <= x0 + bw + PIN_CLEAR)
      .map((p) => p.pos[0]);
    if (xs.length) x0 = role === "label" ? Math.min(...xs) - 0.5 - bw : Math.max(...xs) + 0.5;
  } else {
    x0 = side === "left" ? body.x0 - TEXT_GAP - bw : body.x1 + TEXT_GAP;
    y0 = cy - bh / 2;
    const ys = pins.filter((p) => p.dir === SIDE_DIR[side] && p.pos[1] >= y0 - PIN_CLEAR && p.pos[1] <= y0 + bh + PIN_CLEAR)
      .map((p) => p.pos[1]);
    if (ys.length) y0 = role === "label" ? Math.min(...ys) - 0.5 - bh : Math.max(...ys) + 0.5;
  }
  const box: Rect = { x0, y0, x1: x0 + bw, y1: y0 + bh };
  const my = (box.y0 + box.y1) / 2;
  if (!vertical && side === "left") return { text, at: [box.x1, my], align: "right", vertical, role, box };
  if (!vertical && side === "right") return { text, at: [box.x0, my], align: "left", vertical, role, box };
  return { text, at: [(box.x0 + box.x1) / 2, my], align: "center", vertical, role, box };
}

const shiftText = (t: TextItem, dx: number, dy: number): TextItem =>
  ({ ...t, at: [t.at[0] + dx, t.at[1] + dy], box: translateRect(t.box, [dx, dy]) });

/**
 * The places an outer text may take after its own (UX-12): slid along its side by quarter lengths of its box, nearer
 * first and up or left first, as long as it still faces the body; then the opposite side, its own place there, then
 * slid the same way. Computed only for a text that meets a neighbour.
 */
function outerPlaces(t: TextItem, side: Side, body: Rect, pins: PinGeom[], measure: Measure): TextItem[] {
  const out: TextItem[] = [];
  for (const s of [side, OPPOSITE[side]]) {
    const base = s === side ? t : placeOuter(t.text, t.role, s, body, pins, t.vertical, measure);
    if (s !== side) out.push(base);
    const across = s === "top" || s === "bottom";
    const len = across ? base.box.x1 - base.box.x0 : base.box.y1 - base.box.y0;
    const step = Math.max(len / 4, TEXT_H / 2);
    for (let j = 1; j <= MAX_SLIDES; j++) {
      let facing = false;
      for (const d of [-j * step, j * step]) {
        const c = across ? shiftText(base, d, 0) : shiftText(base, 0, d);
        const b = c.box;
        if (across ? b.x1 >= body.x0 && b.x0 <= body.x1 : b.y1 >= body.y0 && b.y0 <= body.y1) {
          out.push(c);
          facing = true;
        }
      }
      if (!facing) break;
    }
  }
  return out;
}

/** World geometry of a component (also used for placement and preview ghosts). */
export function buildComp(c: Component, def: KindDef, measure: Measure): CompGeom {
  const rot = c.rot;
  const place = (p: FPt): FPt => {
    const r = rotatePt(p, rot);
    return [r[0] + c.pos[0], r[1] + c.pos[1]];
  };
  const body = translateRect(rotateRect(def.body, rot), c.pos);
  const pins: PinGeom[] = def.pins.map((p) => ({
    id: p.id, pos: place(p.pos) as Pt, dir: rotateDir(p.dir, rot), short: p.short, role: p.role,
  }));
  const actuators = def.actuators.map((a) => ({ kind: a.kind, zone: translateRect(rotateRect(a.zone, rot), c.pos) }));
  const variant = symbolVariant(c.kind, c.params);
  const g = symbolGeom(def, variant);
  let drawing = body;
  for (const s of g.body) for (const q of s.pts) drawing = union(drawing, pointRect(place(q)));
  let extent = drawing;
  for (const s of g.leads) for (const q of s.pts) extent = union(extent, pointRect(place(q)));
  for (const p of pins) extent = union(extent, pointRect(p.pos));
  const marks: Rect[] = [];
  for (const s of g.leads) for (let i = 0; i + 1 < s.pts.length; i++) marks.push(rectOf(place(s.pts[i]), place(s.pts[i + 1])));
  for (const p of pins) marks.push(pointRect(p.pos));

  const vText = rot === 90 || rot === 270;
  const flip = rot === 90 || rot === 180;
  const texts: TextItem[] = [];
  for (const t of innerTexts(def, c.params)) {
    const at = place(t.at);
    const align = flip ? flipAlign(t.align) : t.align;
    const item: TextItem = { text: t.text, at, align, vertical: vText, role: t.role, box: textBox(measure(t.text) / 10, at, align, vText) };
    texts.push(item);
    marks.push(item.box);
  }
  const verticalComp = VERTICAL_AT_R0.has(c.kind) !== vText;
  const outer: OuterText[] = [];
  const addOuter = (text: string, role: TextRole, side: Side): void => {
    const own = placeOuter(text, role, side, body, pins, vText, measure);
    outer.push({ k: texts.length, side, own });
    texts.push(own);
  };
  if (c.label) addOuter(c.label, "label", verticalComp ? "left" : "top");
  const value = valueText(def, c.params);
  if (value) addOuter(value, "value", verticalComp ? "right" : "bottom");

  let bounds = extent;
  for (const t of texts) bounds = union(bounds, t.box);
  return { comp: c, def, variant, body, pins, actuators, texts, extent, bounds, drawing, marks, outer };
}

/** Drawn path of a wire, or null when an end pin is unknown. */
export function wirePath(w: Wire, pinPos: (ref: PinRef) => Pt | undefined): Pt[] | null {
  const a = pinPos(w.a);
  const b = pinPos(w.b);
  if (!a || !b) return null;
  return [[a[0], a[1]], ...w.bends.map((p): Pt => [p[0], p[1]]), [b[0], b[1]]];
}

export class SceneModel {
  readonly comps = new Map<CompId, CompGeom>();
  readonly wires = new Map<WireId, WireGeom>();
  readonly index = new SpatialHash<IndexItem>();
  private readonly byComp = new Map<CompId, Set<WireId>>();
  private readonly deg = new Map<string, number>();

  constructor(
    private readonly library: (kind: Kind) => KindDef | undefined,
    private readonly measure: Measure,
  ) {}

  pin(ref: PinRef): PinGeom | undefined {
    return this.comps.get(ref.comp)?.pins.find((p) => p.id === ref.pin);
  }

  pinPos(ref: PinRef): Pt | undefined {
    return this.pin(ref)?.pos;
  }

  /** Wires attached to a pin. */
  degree(ref: PinRef): number {
    return this.deg.get(pinKey(ref)) ?? 0;
  }

  /** Wires attached to a component, ascending ids. */
  wiresOf(comp: CompId): WireId[] {
    return [...(this.byComp.get(comp) ?? [])].sort((a, b) => a - b);
  }

  /** Union of every draw-bound and wire path (fit). */
  bounds(): Rect | null {
    let r: Rect | null = null;
    for (const g of this.comps.values()) r = r ? union(r, g.bounds) : g.bounds;
    for (const g of this.wires.values()) r = r ? union(r, g.bounds) : g.bounds;
    return r;
  }

  geomOf(c: Component): CompGeom | null {
    const def = this.library(c.kind);
    return def ? buildComp(c, def, this.measure) : null;
  }

  rebuild(components: Iterable<Component>, wires: Iterable<Wire>): void {
    this.comps.clear();
    this.wires.clear();
    this.index.clear();
    this.byComp.clear();
    this.deg.clear();
    for (const c of components) this.addComp(c);
    for (const w of wires) this.addWire(w);
    this.settle();
  }

  /** Texts were re-measured (fonts loaded): recompute every component's geometry. */
  relayout(): void {
    for (const g of [...this.comps.values()]) {
      this.removeComp(g.comp.id);
      this.addComp(g.comp);
    }
    this.settle();
  }

  /**
   * Applies a committed patch, after LabState has applied it to its maps. Returns the world rectangles to
   * repaint: old and new draw-bounds of every changed component, old and new paths of every wire in the patch or
   * attached to a changed component (its value may be unchanged while its end points moved).
   */
  applyPatch(patch: Patch): Rect[] {
    const dirty: Rect[] = [];
    const attached = new Set<WireId>();
    for (const [id, , after] of patch.components) {
      for (const w of this.byComp.get(id) ?? []) attached.add(w);
      const old = this.comps.get(id);
      if (old) {
        dirty.push(old.bounds);
        this.removeComp(id);
      }
      if (after) {
        const g = this.addComp(after);
        if (g) dirty.push(g.bounds);
      }
    }
    for (const [id, , after] of patch.wires) {
      attached.delete(id);
      const old = this.wires.get(id);
      if (old) {
        dirty.push(old.bounds);
        this.removeWire(id);
      }
      if (after) {
        const g = this.addWire(after);
        if (g) dirty.push(g.bounds);
      }
    }
    for (const id of attached) {
      const old = this.wires.get(id);
      if (!old) continue;
      dirty.push(old.bounds);
      this.removeWire(id);
      const g = this.addWire(old.wire);
      if (g) dirty.push(g.bounds);
    }
    // a neighbour came, went or moved: outer texts may move (theirs and its own), old and new places repainted
    if (patch.components.length) dirty.push(...this.settle());
    return dirty;
  }

  /**
   * Document-level pass (UX-12): an outer text (designator, value) whose own place meets another component's
   * drawing, lead, pin or inner text takes the first of its other places (outerPlaces) clear of its own marks and of
   * every neighbour, their outer texts at their own places included; else the first clear of drawings and marks;
   * else it stays. Only own places and fixed geometry are obstacles, so the result depends on the document alone,
   * never on the order of the components or of the patches. Returns the old and new draw-bounds of every component
   * whose texts changed.
   */
  private settle(): Rect[] {
    const all = [...this.comps.values()].filter((g) => g.comp.kind !== "junction");
    // what each component can block (drawing, marks, outer texts at their own places), indexed for this pass
    const reach = new SpatialHash<CompGeom>();
    for (const h of all) {
      let r = h.drawing;
      for (const m of h.marks) r = union(r, m);
      for (const o of h.outer) r = union(r, o.own.box);
      reach.set(String(h.comp.id), r, h);
    }
    const dirty: Rect[] = [];
    for (const g of all) {
      if (!g.outer.length) continue;
      const near = (b: Rect): CompGeom[] => reach.query(b).filter((h) => h !== g);
      const hard = (b: Rect, hs: CompGeom[]): boolean => hs.some((h) => intersects(h.drawing, b) || h.marks.some((m) => intersects(m, b)));
      const soft = (b: Rect, hs: CompGeom[]): boolean => hs.some((h) => h.outer.some((o) => intersects(o.own.box, b)));
      const texts = g.texts.slice();
      for (const o of g.outer) texts[o.k] = o.own;
      for (const o of g.outer) {
        const own = inflate(o.own.box, -TOUCH);
        if (!hard(own, near(own))) continue;
        let clearOfMarks: TextItem | null = null;
        let pick: TextItem | null = null;
        for (const c of outerPlaces(o.own, o.side, g.body, g.pins, this.measure)) {
          const b = inflate(c.box, -TOUCH);
          // (its other outer text keeps the usual gap: two texts side by side stay readable)
          if (g.marks.some((m) => intersects(m, b)) || g.outer.some((p) => p !== o && intersects(inflate(texts[p.k].box, TEXT_GAP), b))) continue;
          const hs = near(b);
          if (hard(b, hs)) continue;
          clearOfMarks ??= c;
          if (soft(b, hs)) continue;
          pick = c;
          break;
        }
        texts[o.k] = pick ?? clearOfMarks ?? o.own;
      }
      if (texts.every((t, k) => sameRect(t.box, g.texts[k].box))) continue;
      let bounds = g.extent;
      for (const t of texts) bounds = union(bounds, t.box);
      const next: CompGeom = { ...g, texts, bounds };
      this.comps.set(g.comp.id, next);
      this.index.set(`c${g.comp.id}`, bounds, { t: "comp", id: g.comp.id });
      dirty.push(g.bounds, bounds);
    }
    return dirty;
  }

  /**
   * Geometry after a previewed patch, for the ghost: its components, and the paths of its wires plus every
   * committed wire attached to one of its components (end points moved with them).
   */
  previewGeometry(patch: Patch): { comps: CompGeom[]; paths: Pt[][] } {
    const changed = new Map<CompId, CompGeom | null>();
    for (const [id, , after] of patch.components) changed.set(id, after ? this.geomOf(after) : null);
    const pinPos = (ref: PinRef): Pt | undefined =>
      changed.has(ref.comp) ? changed.get(ref.comp)?.pins.find((p) => p.id === ref.pin)?.pos : this.pinPos(ref);
    const paths: Pt[][] = [];
    const done = new Set<WireId>();
    for (const [id, , after] of patch.wires) {
      done.add(id);
      const p = after ? wirePath(after, pinPos) : null;
      if (p) paths.push(p);
    }
    for (const id of changed.keys()) {
      for (const w of this.byComp.get(id) ?? []) {
        if (done.has(w)) continue;
        done.add(w);
        const g = this.wires.get(w);
        const p = g ? wirePath(g.wire, pinPos) : null;
        if (p) paths.push(p);
      }
    }
    const comps: CompGeom[] = [];
    for (const g of changed.values()) if (g) comps.push(g);
    return { comps, paths };
  }

  private addComp(c: Component): CompGeom | null {
    const g = this.geomOf(c);
    if (!g) return null;
    this.comps.set(c.id, g);
    if (c.kind === "junction") {
      this.index.set(`j${c.id}`, pointRect(g.pins[0]?.pos ?? c.pos), { t: "junction", id: c.id });
    } else {
      this.index.set(`c${c.id}`, g.bounds, { t: "comp", id: c.id });
      for (const p of g.pins) this.index.set(`p${c.id}:${p.id}`, pointRect(p.pos), { t: "pin", comp: c.id, pin: p.id });
    }
    return g;
  }

  private removeComp(id: CompId): void {
    const g = this.comps.get(id);
    if (!g) return;
    this.index.delete(`c${id}`);
    this.index.delete(`j${id}`);
    for (const p of g.pins) this.index.delete(`p${id}:${p.id}`);
    this.comps.delete(id);
  }

  private addWire(w: Wire): WireGeom | null {
    const path = wirePath(w, (r) => this.pinPos(r));
    const bounds = path ? pathBounds(path) : null;
    if (!path || !bounds) return null;
    const g: WireGeom = { wire: w, path, bounds };
    this.wires.set(w.id, g);
    for (let k = 0; k + 1 < path.length; k++) this.index.set(`w${w.id}:${k}`, rectOf(path[k], path[k + 1]), { t: "seg", wire: w.id, k });
    for (const end of [w.a, w.b]) {
      let set = this.byComp.get(end.comp);
      if (!set) {
        set = new Set();
        this.byComp.set(end.comp, set);
      }
      set.add(w.id);
      this.deg.set(pinKey(end), (this.deg.get(pinKey(end)) ?? 0) + 1);
    }
    return g;
  }

  private removeWire(id: WireId): void {
    const g = this.wires.get(id);
    if (!g) return;
    for (let k = 0; k + 1 < g.path.length; k++) this.index.delete(`w${id}:${k}`);
    for (const end of [g.wire.a, g.wire.b]) {
      const set = this.byComp.get(end.comp);
      set?.delete(id);
      if (set && set.size === 0) this.byComp.delete(end.comp);
      const n = (this.deg.get(pinKey(end)) ?? 1) - 1;
      if (n > 0) this.deg.set(pinKey(end), n);
      else this.deg.delete(pinKey(end));
    }
    this.wires.delete(id);
  }
}
