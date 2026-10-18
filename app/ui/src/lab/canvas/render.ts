// Canvas2D drawing of the schematic (contract §2.2, §3.4.8, §8.5; DESIGN.md §7). Scene: background and grid,
// wires in one path, symbols from Path2D cached by (kind, rotation, variant), connection squares, texts. Overlay:
// hover, selection, state-dependent parts, live values, then diagnostic markers, gesture ghosts and previews, marquee.
// Device-pixel coordinates throughout: grid points land on whole pixels, strokes of odd width get +0.5. Square
// geometry only: miter joins, butt caps, no rounded shape. Colours come from the --sch-* tokens.

import type { LabState } from "../state";
import type { KindDef, Pt, Rect, Rot, ValueMap, WireId } from "../types";
import type { DevRect } from "./dirty";
import { type FPt, inflate, intersects, rotatePt } from "./geom";
import type { GestureView } from "./gestures";
import { type DiagMarker, type LiveItem, type LivePlace, layoutLive } from "./live";
import type { CompGeom, SceneModel } from "./model";
import { type Stroke, DYNAMIC_KINDS, dynamicGeom, symbolGeom } from "./symbols";
import { type Viewport, strokePx } from "./view";

export interface Palette {
  bg: string;
  grid: string;
  gridMajor: string;
  symbol: string;
  pin: string;
  wire: string;
  label: string;
  value: string;
  live: string;
  liveBg: string;
  select: string;
  selectFill: string;
  hover: string;
  invalid: string;
  warn: string;
  /** --font-mono. */
  font: string;
  /** --sch-line, CSS pixels. */
  lineCss: number;
}

export function readPalette(el: Element): Palette {
  const cs = getComputedStyle(el);
  const v = (name: string): string => cs.getPropertyValue(name).trim();
  return {
    bg: v("--sch-bg"), grid: v("--sch-grid"), gridMajor: v("--sch-grid-major"), symbol: v("--sch-symbol"),
    pin: v("--sch-pin"), wire: v("--sch-wire"), label: v("--sch-label"), value: v("--sch-value"),
    live: v("--sch-live"), liveBg: v("--sch-live-bg"), select: v("--sch-select"), selectFill: v("--sch-select-fill"),
    hover: v("--sch-hover"), invalid: v("--sch-invalid"), warn: v("--sch-warn"),
    font: v("--font-mono") || "monospace",
    lineCss: Number.parseFloat(v("--sch-line")) || 1.5,
  };
}

/** Text size of the schematic at zoom 1, CSS pixels; texts are hidden below 7 px, pin names below 8 px. */
export const TEXT_PX = 11;

/** Rounds half away from zero, so that symmetric drawings stay symmetric once snapped. */
const sr = (v: number): number => (v < 0 ? -Math.round(-v) : Math.round(v));

/** Appends strokes rotated by rot, scaled by u and snapped, relative to the device point (ox, oy). */
function traceStrokes(p: CanvasPath, strokes: readonly Stroke[], rot: Rot, u: number, half: number, ox: number, oy: number): void {
  for (const s of strokes) {
    s.pts.forEach((q, i) => {
      const r = rotatePt(q, rot);
      const x = ox + sr(r[0] * u) + half;
      const y = oy + sr(r[1] * u) + half;
      if (i === 0) p.moveTo(x, y);
      else p.lineTo(x, y);
    });
    if (s.closed) p.closePath();
  }
}

function traceFills(p: CanvasPath, fills: readonly FPt[][], rot: Rot, u: number, ox: number, oy: number): void {
  for (const f of fills) {
    f.forEach((q, i) => {
      const r = rotatePt(q, rot);
      const x = ox + sr(r[0] * u);
      const y = oy + sr(r[1] * u);
      if (i === 0) p.moveTo(x, y);
      else p.lineTo(x, y);
    });
    p.closePath();
  }
}

interface SymbolPaths {
  leads: Path2D;
  body: Path2D;
}

/** Path2D of static symbols by (kind, rotation, variant), rebuilt when the unit size or stroke parity changes. */
class PathCache {
  private scale = "";
  private readonly map = new Map<string, SymbolPaths>();

  get(def: KindDef, rot: Rot, variant: string, u: number, half: number): SymbolPaths {
    const scale = `${u}|${half}`;
    if (scale !== this.scale) {
      this.map.clear();
      this.scale = scale;
    }
    const key = `${def.kind}|${rot}|${variant}`;
    let p = this.map.get(key);
    if (!p) {
      const g = symbolGeom(def, variant);
      p = { leads: new Path2D(), body: new Path2D() };
      traceStrokes(p.leads, g.leads, rot, u, half, 0, 0);
      traceStrokes(p.body, g.body, rot, u, half, 0, 0);
      this.map.set(key, p);
    }
    return p;
  }

  clear(): void {
    this.map.clear();
    this.scale = "";
  }
}

export class Renderer {
  /** Fonts awaited: texts are not drawn before (contract §8.5 "Polices"). */
  textsReady = false;
  private readonly paths = new PathCache();
  /**
   * Grid patterns by (unit, mark size, minor points, colours): at most one per ladder step, so a zoom sweep reuses
   * them instead of allocating a tile canvas and a pattern texture at every notch.
   */
  private readonly grids = new Map<string, CanvasPattern | null>();
  /** Live box places, kept while the document, the zoom and the boxes are unchanged (a pan keeps them). */
  private livePlaces: { key: string; places: LivePlace[] } = { key: "", places: [] };

  constructor(
    private readonly vp: Viewport,
    private readonly model: SceneModel,
    private readonly state: LabState,
    public pal: Palette,
  ) {}

  invalidateCaches(): void {
    this.paths.clear();
    this.grids.clear();
    this.livePlaces = { key: "", places: [] };
  }

  get stroke(): number {
    return strokePx(this.vp.dpr, this.pal.lineCss);
  }

  private get half(): number {
    return this.stroke % 2 === 1 ? 0.5 : 0;
  }

  /** Junction square: clamp(0.5·u, 4, 8) CSS px (§2.2), same parity as the stroke so that it centres on the wire. */
  private junctionSide(): number {
    const s = Math.max(1, Math.round(Math.min(8, Math.max(4, 0.5 * this.vp.cssUnit)) * this.vp.dpr));
    return (s - this.stroke) % 2 === 0 ? s : s + 1;
  }

  /** Unconnected pin: hollow square of 0.4 unit, even so that it stays crisp. */
  private hollowSide(): number {
    return Math.max(4, 2 * Math.round(0.2 * this.vp.u));
  }

  /** Device pixels added around a dirty rectangle: half a stroke and the connection squares, plus one pixel. */
  get margin(): number {
    return Math.ceil(Math.max(this.stroke / 2, this.junctionSide() / 2, this.hollowSide() / 2 + this.stroke)) + 1;
  }

  /** World rectangle → device rectangle to repaint. */
  dirtyOf(r: Rect): DevRect {
    const { ox, oy, u } = this.vp;
    const m = this.margin;
    return { x0: ox + r.x0 * u - m, y0: oy + r.y0 * u - m, x1: ox + r.x1 * u + m, y1: oy + r.y1 * u + m };
  }

  // ------------------------------------------------------------------------------------------- scene

  /** Repaints the scene, entirely (null) or inside a device rectangle: background, grid, wires, symbols, squares, texts. */
  paintScene(ctx: CanvasRenderingContext2D, region: DevRect | null): void {
    const vp = this.vp;
    const r = region ?? { x0: 0, y0: 0, x1: vp.w, y1: vp.h };
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.save();
    if (region) {
      ctx.beginPath();
      ctx.rect(r.x0, r.y0, r.x1 - r.x0, r.y1 - r.y0);
      ctx.clip();
    }
    ctx.fillStyle = this.gridPattern(ctx) ?? this.pal.bg;
    ctx.fillRect(r.x0, r.y0, r.x1 - r.x0, r.y1 - r.y0);

    const world = inflate(vp.worldOf(r.x0, r.y0, r.x1, r.y1), this.margin / vp.u);
    const comps: CompGeom[] = [];
    const junctions: CompGeom[] = [];
    const wires = new Set<WireId>();
    for (const it of this.model.index.query(world)) {
      if (it.t === "seg") wires.add(it.wire);
      else if (it.t === "comp" || it.t === "junction") {
        const g = this.model.comps.get(it.id);
        if (g) (it.t === "comp" ? comps : junctions).push(g);
      }
    }
    comps.sort((a, b) => a.comp.id - b.comp.id);
    ctx.lineJoin = "miter";
    ctx.lineCap = "butt";
    this.strokeWires(ctx, [...wires].sort((a, b) => a - b), this.pal.wire);
    this.drawSymbols(ctx, comps);
    this.drawSquares(ctx, comps, junctions);
    if (this.textsReady) this.drawTexts(ctx, comps);
    ctx.restore();
  }

  /** Grid as a repeating tile of 10 × 10 units: minor points (u ≥ 8 CSS px) and a major line every 10 units. */
  private gridPattern(ctx: CanvasRenderingContext2D): CanvasPattern | null {
    const vp = this.vp;
    const u = vp.u;
    const d = Math.max(1, Math.round(vp.dpr));
    const minor = vp.cssUnit >= 8;
    const pal = this.pal;
    const key = `${u}|${d}|${minor}|${pal.bg}|${pal.grid}|${pal.gridMajor}`;
    let pattern = this.grids.get(key);
    if (pattern === undefined) {
      const size = 10 * u;
      const tile = document.createElement("canvas");
      tile.width = size;
      tile.height = size;
      const t = tile.getContext("2d");
      pattern = null;
      if (t) {
        t.fillStyle = pal.bg;
        t.fillRect(0, 0, size, size);
        if (minor) {
          t.beginPath();
          for (let i = 1; i < 10; i++) for (let j = 1; j < 10; j++) t.rect(i * u, j * u, d, d);
          t.fillStyle = pal.grid;
          t.fill();
        }
        t.beginPath();
        t.rect(0, 0, d, size);
        t.rect(0, 0, size, d);
        t.fillStyle = pal.gridMajor;
        t.fill();
        pattern = ctx.createPattern(tile, "repeat");
      }
      // The pattern holds a copy of the tile: its bitmap is released now rather than at the next collection.
      tile.width = 0;
      tile.height = 0;
      this.grids.set(key, pattern);
    }
    // Tile origin on the world origin, the d-pixel marks centred on their grid points like the strokes.
    const s = Math.floor(d / 2);
    pattern?.setTransform({ a: 1, b: 0, c: 0, d: 1, e: vp.ox - s, f: vp.oy - s });
    return pattern;
  }

  private tracePath(ctx: CanvasPath, path: readonly FPt[]): void {
    const { ox, oy, u } = this.vp;
    const h = this.half;
    path.forEach((p, i) => {
      const x = ox + sr(p[0] * u) + h;
      const y = oy + sr(p[1] * u) + h;
      if (i === 0) ctx.moveTo(x, y);
      else ctx.lineTo(x, y);
    });
  }

  private strokeWires(ctx: CanvasRenderingContext2D, ids: readonly WireId[], color: string): void {
    if (ids.length === 0) return;
    ctx.beginPath();
    for (const id of ids) {
      const g = this.model.wires.get(id);
      if (g) this.tracePath(ctx, g.path);
    }
    ctx.lineWidth = this.stroke;
    ctx.strokeStyle = color;
    ctx.stroke();
  }

  private strokePaths(ctx: CanvasRenderingContext2D, paths: readonly (readonly FPt[])[], color: string): void {
    if (paths.length === 0) return;
    ctx.beginPath();
    for (const p of paths) this.tracePath(ctx, p);
    ctx.lineWidth = this.stroke;
    ctx.strokeStyle = color;
    ctx.stroke();
  }

  private traceRect(ctx: CanvasPath, r: Rect): void {
    const { ox, oy, u } = this.vp;
    const h = this.half;
    const x0 = ox + sr(r.x0 * u) + h;
    const y0 = oy + sr(r.y0 * u) + h;
    ctx.rect(x0, y0, ox + sr(r.x1 * u) + h - x0, oy + sr(r.y1 * u) + h - y0);
  }

  private symbolPaths(g: CompGeom): SymbolPaths {
    return this.paths.get(g.def, g.comp.rot, g.variant, this.vp.u, this.half);
  }

  private originOf(pos: Pt): [number, number] {
    return [this.vp.ox + pos[0] * this.vp.u, this.vp.oy + pos[1] * this.vp.u];
  }

  /** Bodies only below 3 CSS px per unit (§8.5 "Niveaux de détail"). */
  private get lod(): boolean {
    return this.vp.cssUnit < 3;
  }

  private drawSymbols(ctx: CanvasRenderingContext2D, comps: readonly CompGeom[]): void {
    ctx.lineWidth = this.stroke;
    if (this.lod) {
      ctx.beginPath();
      for (const g of comps) this.traceRect(ctx, g.body);
      ctx.strokeStyle = this.pal.symbol;
      ctx.stroke();
      return;
    }
    for (const pass of ["leads", "body"] as const) {
      ctx.strokeStyle = pass === "leads" ? this.pal.pin : this.pal.symbol;
      for (const g of comps) {
        const [x, y] = this.originOf(g.comp.pos);
        ctx.setTransform(1, 0, 0, 1, x, y);
        ctx.stroke(this.symbolPaths(g)[pass]);
      }
    }
    ctx.setTransform(1, 0, 0, 1, 0, 0);
  }

  private squareAt(ctx: CanvasPath, pos: FPt, side: number): void {
    const [x, y] = this.originOf(pos);
    const h = this.half;
    ctx.rect(Math.round(x + h - side / 2), Math.round(y + h - side / 2), side, side);
  }

  /** Filled square where ≥ 3 conductors meet, hollow square on unconnected pins (§3.4.8). */
  private drawSquares(ctx: CanvasRenderingContext2D, comps: readonly CompGeom[], junctions: readonly CompGeom[]): void {
    const js = this.junctionSide();
    ctx.beginPath();
    for (const g of junctions) {
      const p = g.pins[0];
      if (p && this.model.degree({ comp: g.comp.id, pin: p.id }) >= 3) this.squareAt(ctx, p.pos, js);
    }
    for (const g of comps) {
      for (const p of g.pins) if (this.model.degree({ comp: g.comp.id, pin: p.id }) >= 2) this.squareAt(ctx, p.pos, js);
    }
    ctx.fillStyle = this.pal.wire;
    ctx.fill();
    if (this.lod) return;
    const hs = this.hollowSide();
    const h = this.half;
    ctx.beginPath();
    for (const g of comps) {
      for (const p of g.pins) {
        if (this.model.degree({ comp: g.comp.id, pin: p.id }) !== 0) continue;
        const [x, y] = this.originOf(p.pos);
        ctx.rect(x + h - hs / 2, y + h - hs / 2, hs, hs);
      }
    }
    ctx.lineWidth = this.stroke;
    ctx.strokeStyle = this.pal.pin;
    ctx.stroke();
  }

  private drawTexts(ctx: CanvasRenderingContext2D, comps: readonly CompGeom[]): void {
    const vp = this.vp;
    const css = TEXT_PX * vp.zoom;
    if (css < 7) return;
    const pins = css >= 8;
    ctx.font = `400 ${(css * vp.dpr).toFixed(2)}px ${this.pal.font}`;
    ctx.textBaseline = "middle";
    const color = { label: this.pal.label, value: this.pal.value, title: this.pal.value, pin: this.pal.pin };
    for (const g of comps) {
      for (const t of g.texts) {
        if (t.role === "pin" && !pins) continue;
        ctx.fillStyle = color[t.role];
        ctx.textAlign = t.align;
        const [x, y] = vp.toDevice(t.at);
        if (t.vertical) {
          ctx.save();
          ctx.translate(Math.round(x), Math.round(y));
          ctx.rotate(-Math.PI / 2);
          ctx.fillText(t.text, 0, 0);
          ctx.restore();
        } else {
          ctx.fillText(t.text, Math.round(x), Math.round(y));
        }
      }
    }
  }

  // ------------------------------------------------------------------------------------------- overlay

  paintOverlay(ctx: CanvasRenderingContext2D, view: GestureView | null, live: readonly LiveItem[], diags: readonly DiagMarker[]): void {
    const vp = this.vp;
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.clearRect(0, 0, ctx.canvas.width, ctx.canvas.height);
    ctx.lineJoin = "miter";
    ctx.lineCap = "butt";
    const visible: CompGeom[] = [];
    for (const it of this.model.index.query(inflate(vp.visibleWorld(), 1))) {
      if (it.t !== "comp" && it.t !== "junction") continue;
      const g = this.model.comps.get(it.id);
      if (g) visible.push(g);
    }
    visible.sort((a, b) => a.comp.id - b.comp.id);
    this.drawHover(ctx);
    this.drawSelection(ctx, visible);
    if (!this.lod) this.drawDynamic(ctx, visible);
    // Live boxes are opaque: they avoid symbols and markers, and the markers (§3.7, never hidden) and the gesture
    // preview still go over them when no free place exists.
    this.drawLive(ctx, live, diags);
    this.drawMarkers(ctx, diags);
    if (view) this.drawGesture(ctx, view);
  }

  private strokeComp(ctx: CanvasRenderingContext2D, g: CompGeom, color: string): void {
    ctx.lineWidth = this.stroke;
    ctx.strokeStyle = color;
    if (g.comp.kind === "junction") {
      const p = g.pins[0];
      if (p) {
        ctx.beginPath();
        this.squareAt(ctx, p.pos, this.junctionSide());
        ctx.fillStyle = color;
        ctx.fill();
      }
      return;
    }
    if (this.lod) {
      ctx.beginPath();
      this.traceRect(ctx, g.body);
      ctx.stroke();
      return;
    }
    const p = this.symbolPaths(g);
    const [x, y] = this.originOf(g.comp.pos);
    ctx.setTransform(1, 0, 0, 1, x, y);
    ctx.stroke(p.leads);
    ctx.stroke(p.body);
    ctx.setTransform(1, 0, 0, 1, 0, 0);
  }

  private drawDynamicOf(ctx: CanvasRenderingContext2D, g: CompGeom, state: ValueMap | undefined, color: string): void {
    const dyn = dynamicGeom(g.comp.kind, g.comp.params, state);
    if (!dyn) return;
    const [x, y] = this.originOf(g.comp.pos);
    ctx.beginPath();
    traceStrokes(ctx, dyn.strokes, g.comp.rot, this.vp.u, this.half, x, y);
    ctx.lineWidth = this.stroke;
    ctx.strokeStyle = color;
    ctx.stroke();
    if (dyn.fills.length) {
      ctx.beginPath();
      traceFills(ctx, dyn.fills, g.comp.rot, this.vp.u, x, y);
      ctx.fillStyle = color;
      ctx.fill();
    }
  }

  /** Blade, contact and cap, wiper arrow: the state returned by the core (§2.2). */
  private drawDynamic(ctx: CanvasRenderingContext2D, visible: readonly CompGeom[]): void {
    const sel = this.state.selection.components;
    const hover = this.state.hover.comp;
    for (const g of visible) {
      if (!DYNAMIC_KINDS.has(g.comp.kind)) continue;
      const id = g.comp.id;
      const color = sel.has(id) ? this.pal.select : hover === id ? this.pal.hover : this.pal.symbol;
      this.drawDynamicOf(ctx, g, this.state.states.get(id), color);
    }
  }

  private drawHover(ctx: CanvasRenderingContext2D): void {
    const h = this.state.hover;
    const sel = this.state.selection;
    const wires = new Set<WireId>();
    if (h.net) {
      const net = this.state.nets.find((n) => n.key.comp === h.net?.comp && n.key.pin === h.net?.pin);
      for (const w of net?.wires ?? []) wires.add(w);
    }
    if (h.wire !== undefined) wires.add(h.wire);
    for (const w of sel.wires) wires.delete(w);
    this.strokeWires(ctx, [...wires].sort((a, b) => a - b), this.pal.hover);
    if (h.comp !== undefined && !sel.components.has(h.comp)) {
      const g = this.model.comps.get(h.comp);
      if (g && g.comp.kind !== "junction") this.strokeComp(ctx, g, this.pal.hover);
    }
    if (h.pin) {
      const pos = this.model.pinPos(h.pin);
      if (pos) {
        const side = this.junctionSide() + 2 * this.stroke + 2;
        const [x, y] = this.originOf(pos);
        const hf = this.half;
        const x0 = Math.round(x + hf - side / 2) + 0.5;
        const y0 = Math.round(y + hf - side / 2) + 0.5;
        ctx.lineWidth = 1;
        ctx.strokeStyle = this.pal.hover;
        ctx.strokeRect(x0, y0, side - 1, side - 1);
      }
    }
  }

  private drawSelection(ctx: CanvasRenderingContext2D, visible: readonly CompGeom[]): void {
    const sel = this.state.selection;
    if (sel.components.size) {
      ctx.beginPath();
      for (const g of visible) if (sel.components.has(g.comp.id) && g.comp.kind !== "junction") this.traceRect(ctx, g.body);
      ctx.fillStyle = this.pal.selectFill;
      ctx.fill();
      for (const g of visible) if (sel.components.has(g.comp.id)) this.strokeComp(ctx, g, this.pal.select);
    }
    this.strokeWires(ctx, [...sel.wires].sort((a, b) => a - b), this.pal.select);
  }

  /** Side of a diagnostic marker, device pixels. */
  private get markerSide(): number {
    return Math.max(3, Math.round(6 * this.vp.dpr));
  }

  private markerAt(ctx: CanvasRenderingContext2D, at: FPt, color: string): void {
    const side = this.markerSide;
    const [x, y] = this.vp.toDevice(at);
    ctx.fillStyle = color;
    ctx.fillRect(Math.round(x - side / 2), Math.round(y - side / 2), side, side);
  }

  private drawMarkers(ctx: CanvasRenderingContext2D, diags: readonly DiagMarker[]): void {
    for (const m of diags) this.markerAt(ctx, m.at, m.severity === "error" ? this.pal.invalid : this.pal.warn);
  }

  private drawGesture(ctx: CanvasRenderingContext2D, v: GestureView): void {
    if (v.marquee) {
      this.drawMarquee(ctx, v.marquee.a, v.marquee.b, v.marquee.enclosed);
      return;
    }
    const color = v.status === "refused" ? this.pal.invalid : this.pal.select;
    if (v.status === "ok" && v.patch) {
      // The previewed result: components and every wire whose path changes.
      const pg = this.model.previewGeometry(v.patch);
      this.strokePaths(ctx, pg.paths, color);
      for (const g of pg.comps) {
        this.strokeComp(ctx, g, color);
        const st = this.state.states.get(g.comp.id) ?? this.defaultStates(g.def);
        if (!this.lod) this.drawDynamicOf(ctx, g, st, color);
      }
      return;
    }
    const gh = v.ghost;
    if (gh?.t === "place") {
      const g = this.model.geomOf(gh.comp);
      if (g) {
        this.strokeComp(ctx, g, color);
        if (!this.lod) this.drawDynamicOf(ctx, g, this.defaultStates(g.def), color);
      }
    } else if (gh?.t === "move") {
      const d = gh.delta;
      for (const id of gh.comps) {
        const c = this.state.components.get(id);
        const g = c ? this.model.geomOf({ ...c, pos: [c.pos[0] + d[0], c.pos[1] + d[1]] }) : null;
        if (g) this.strokeComp(ctx, g, color);
      }
      const paths: FPt[][] = [];
      for (const id of gh.wires) {
        const g = this.model.wires.get(id);
        if (g) paths.push(g.path.map((q): FPt => [q[0] + d[0], q[1] + d[1]]));
      }
      this.strokePaths(ctx, paths, color);
    } else if (gh?.t === "path") {
      this.strokePaths(ctx, [gh.path], color);
    }
    if (v.status === "refused" && v.refusal) {
      for (const it of v.refusal.items) {
        if (it.type === "component") {
          const g = this.model.comps.get(it.id);
          if (g) this.strokeComp(ctx, g, this.pal.invalid);
        } else if (it.type === "wire") {
          this.strokeWires(ctx, [it.id], this.pal.invalid);
        } else {
          const pos = this.model.pinPos(it);
          if (pos) this.markerAt(ctx, pos, this.pal.invalid);
        }
      }
      if (v.refusal.at) this.markerAt(ctx, v.refusal.at, this.pal.invalid);
    }
  }

  private defaultStates(def: KindDef): ValueMap {
    const st: ValueMap = {};
    for (const s of def.states) st[s.name] = s.default;
    return st;
  }

  private drawMarquee(ctx: CanvasRenderingContext2D, a: FPt, b: FPt, enclosed: boolean): void {
    const vp = this.vp;
    const [ax, ay] = vp.toDevice(a);
    const [bx, by] = vp.toDevice(b);
    const x0 = Math.round(Math.min(ax, bx));
    const y0 = Math.round(Math.min(ay, by));
    const w = Math.max(1, Math.round(Math.max(ax, bx)) - x0);
    const h = Math.max(1, Math.round(Math.max(ay, by)) - y0);
    ctx.fillStyle = this.pal.selectFill;
    ctx.fillRect(x0, y0, w, h);
    // Touch mode (right to left) is dashed, enclosure mode solid.
    ctx.setLineDash(enclosed ? [] : [Math.round(4 * vp.dpr), Math.round(3 * vp.dpr)]);
    ctx.lineWidth = 1;
    ctx.strokeStyle = this.pal.select;
    ctx.strokeRect(x0 + 0.5, y0 + 0.5, w - 1, h - 1);
    ctx.setLineDash([]);
  }

  private drawLive(ctx: CanvasRenderingContext2D, items: readonly LiveItem[], diags: readonly DiagMarker[]): void {
    if (items.length === 0) return;
    const dpr = this.vp.dpr;
    const fs = Math.round(TEXT_PX * dpr);
    const pad = Math.round(3 * dpr);
    const gap = Math.round(6 * dpr);
    const bh = fs + 2 * pad;
    ctx.font = `400 ${fs}px ${this.pal.font}`;
    ctx.textBaseline = "middle";
    ctx.textAlign = "left";
    const widths = items.map((it) => Math.ceil(ctx.measureText(it.text).width) + 2 * pad);
    const key = `${this.state.rev}|${this.vp.u}|${dpr}|${bh}|` +
      items.map((it, k) => `${it.at[0]},${it.at[1]},${it.dir ?? ""},${widths[k]}`).join(";") + "|" +
      diags.map((m) => `${m.at[0]},${m.at[1]}`).join(";");
    if (key !== this.livePlaces.key) {
      const h = this.markerSide / 2 / this.vp.u;
      const marks: Rect[] = diags.map((m) => ({ x0: m.at[0] - h, y0: m.at[1] - h, x1: m.at[0] + h, y1: m.at[1] + h }));
      const covers = (r: Rect) => this.coversSymbol(r) || marks.some((m) => intersects(r, m));
      this.livePlaces = { key, places: layoutLive(items, widths, bh, gap, this.vp.u, covers) };
    }
    const places = this.livePlaces.places;
    const line = Math.max(1, Math.round(dpr));
    items.forEach((it, k) => {
      const [px, py] = this.vp.toDevice(it.at);
      const ax = Math.round(px);
      const ay = Math.round(py);
      const { dx, dy, moved } = places[k];
      const x = ax + dx;
      const y = ay + dy;
      const bw = widths[k];
      if (moved) {
        // Leader from the pin to the nearest point of its moved box.
        const nx = Math.min(Math.max(ax, x), x + bw);
        const ny = Math.min(Math.max(ay, y), y + bh);
        ctx.strokeStyle = this.pal.live;
        ctx.lineWidth = line;
        ctx.beginPath();
        ctx.moveTo(ax + 0.5 * (line % 2), ay + 0.5 * (line % 2));
        ctx.lineTo(nx + 0.5 * (line % 2), ny + 0.5 * (line % 2));
        ctx.stroke();
      }
      ctx.fillStyle = this.pal.liveBg;
      ctx.fillRect(x, y, bw, bh);
      ctx.fillStyle = this.pal.live;
      ctx.fillText(it.text, x + pad, y + bh / 2);
    });
  }

  /** A symbol (its drawing or one of its texts, junctions excepted) meets this world rectangle. */
  private coversSymbol(r: Rect): boolean {
    for (const it of this.model.index.query(r)) {
      if (it.t !== "comp") continue;
      const g = this.model.comps.get(it.id);
      if (g && intersects(r, g.bounds) && (intersects(r, g.extent) || g.texts.some((t) => intersects(r, t.box)))) return true;
    }
    return false;
  }
}
