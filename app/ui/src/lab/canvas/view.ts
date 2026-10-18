// View transform of the schematic (contract §8.5 "Zoom"): scale z = 1.25^k, k ∈ [−6, 6]; one grid unit is
// u = round(10·z·dpr) device pixels; the world origin sits at integer device offsets (ox, oy), so every grid
// point lands on a device pixel and strokes of width w = max(1, round(1.5·dpr)) are crisp with a +0.5 offset
// when w is odd. Pure: no DOM.

import type { Rect } from "../types";
import type { FPt } from "./geom";

export const K_MIN = -6;
export const K_MAX = 6;
const STEP = 1.25;

export const zoomOf = (k: number): number => STEP ** k;

export const clampK = (k: number): number => Math.min(K_MAX, Math.max(K_MIN, Math.round(k)));

/** Device pixels per grid unit at ladder step k. */
export const unitPx = (k: number, dpr: number): number => Math.max(1, Math.round(10 * zoomOf(k) * dpr));

/** Device stroke width for a CSS width (1.5 px by default, --sch-line). */
export const strokePx = (dpr: number, css = 1.5): number => Math.max(1, Math.round(css * dpr));

/** Ladder step closest (in scale ratio) to `ratio` (1 = 100 %). */
export function stepNearest(ratio: number): number {
  if (!Number.isFinite(ratio) || ratio <= 0) return 0;
  return clampK(Math.log(ratio) / Math.log(STEP));
}

export const zoomPercent = (k: number): number => Math.round(zoomOf(k) * 100);

export class Viewport {
  /** Ladder step. */
  k = 0;
  dpr = 1;
  /** Device position of the world origin (integers). */
  ox = 0;
  oy = 0;
  /** Visible size, device pixels. */
  w = 0;
  h = 0;

  get u(): number {
    return unitPx(this.k, this.dpr);
  }

  get zoom(): number {
    return zoomOf(this.k);
  }

  /** CSS pixels per grid unit as drawn (u rounded to device pixels): pointing distances. */
  get uCss(): number {
    return this.u / this.dpr;
  }

  /** Exact CSS pixels per grid unit, 10·z: the level-of-detail thresholds of §8.5. */
  get cssUnit(): number {
    return 10 * this.zoom;
  }

  toDevice(p: FPt): FPt {
    const u = this.u;
    return [this.ox + p[0] * u, this.oy + p[1] * u];
  }

  toWorld(x: number, y: number): FPt {
    const u = this.u;
    return [(x - this.ox) / u, (y - this.oy) / u];
  }

  /** World rectangle covered by a device rectangle. */
  worldOf(x0: number, y0: number, x1: number, y1: number): Rect {
    const a = this.toWorld(x0, y0);
    const b = this.toWorld(x1, y1);
    return { x0: Math.min(a[0], b[0]), y0: Math.min(a[1], b[1]), x1: Math.max(a[0], b[0]), y1: Math.max(a[1], b[1]) };
  }

  visibleWorld(): Rect {
    return this.worldOf(0, 0, this.w, this.h);
  }

  /** Zoom to step k keeping the world point under device (ax, ay) in place. false if nothing changed. */
  zoomAt(k: number, ax: number, ay: number): boolean {
    const nk = clampK(k);
    if (nk === this.k) return false;
    const [wx, wy] = this.toWorld(ax, ay);
    this.k = nk;
    const u = this.u;
    this.ox = Math.round(ax - wx * u);
    this.oy = Math.round(ay - wy * u);
    return true;
  }

  panBy(dx: number, dy: number): boolean {
    const ix = Math.round(dx);
    const iy = Math.round(dy);
    if (ix === 0 && iy === 0) return false;
    this.ox += ix;
    this.oy += iy;
    return true;
  }

  /** New visible size; the top-left world point stays in place. */
  resize(w: number, h: number): void {
    this.w = Math.max(0, Math.round(w));
    this.h = Math.max(0, Math.round(h));
  }

  /** New device pixel ratio and visible size; the world point at the centre of the view stays in place. */
  setDpr(dpr: number, w: number, h: number): void {
    const c = this.w > 0 && this.h > 0 ? this.toWorld(this.w / 2, this.h / 2) : this.toWorld(0, 0);
    const keepCorner = !(this.w > 0 && this.h > 0);
    this.dpr = dpr > 0 && Number.isFinite(dpr) ? dpr : 1;
    this.resize(w, h);
    const u = this.u;
    if (keepCorner) {
      this.ox = Math.round(-c[0] * u);
      this.oy = Math.round(-c[1] * u);
    } else {
      this.ox = Math.round(this.w / 2 - c[0] * u);
      this.oy = Math.round(this.h / 2 - c[1] * u);
    }
  }

  /** Centre `bounds` at the largest step that fits it inside the view minus a margin; origin centred if null. */
  fit(bounds: Rect | null, marginCss = 24): boolean {
    if (this.w <= 0 || this.h <= 0) return false;
    if (!bounds) {
      this.k = 0;
      this.ox = Math.round(this.w / 2);
      this.oy = Math.round(this.h / 2);
      return true;
    }
    const m = marginCss * this.dpr;
    const aw = Math.max(1, this.w - 2 * m);
    const ah = Math.max(1, this.h - 2 * m);
    const bw = bounds.x1 - bounds.x0;
    const bh = bounds.y1 - bounds.y0;
    let k = K_MAX;
    while (k > K_MIN && (bw * unitPx(k, this.dpr) > aw || bh * unitPx(k, this.dpr) > ah)) k--;
    this.k = k;
    const u = this.u;
    this.ox = Math.round(this.w / 2 - ((bounds.x0 + bounds.x1) / 2) * u);
    this.oy = Math.round(this.h / 2 - ((bounds.y0 + bounds.y1) / 2) * u);
    return true;
  }
}
