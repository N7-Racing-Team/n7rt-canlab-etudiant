// Dirty region of the scene layer (contract §8.5 "Dessin"): the union of the draw-bounds touched by patches, in
// device pixels; a repaint above 50 % of the view, or after a view change, is a full repaint. Pure: no DOM.

/** Device-pixel rectangle, x1 > x0 and y1 > y0 when not empty. */
export interface DevRect {
  x0: number;
  y0: number;
  x1: number;
  y1: number;
}

export type Repaint = { kind: "none" } | { kind: "full" } | { kind: "rect"; rect: DevRect };

/** Share of the view above which a partial repaint becomes a full one. */
export const FULL_REPAINT_SHARE = 0.5;

export function unionDev(a: DevRect, b: DevRect): DevRect {
  return { x0: Math.min(a.x0, b.x0), y0: Math.min(a.y0, b.y0), x1: Math.max(a.x1, b.x1), y1: Math.max(a.y1, b.y1) };
}

export class DirtyRegion {
  private full = false;
  private rect: DevRect | null = null;

  markFull(): void {
    this.full = true;
  }

  add(r: DevRect): void {
    if (!(r.x1 > r.x0 && r.y1 > r.y0)) return;
    this.rect = this.rect ? unionDev(this.rect, r) : { ...r };
  }

  get pending(): boolean {
    return this.full || this.rect !== null;
  }

  /**
   * What to repaint in a view of w × h device pixels, then forget it. The rectangle is clipped to the view and
   * snapped outwards to whole pixels; off-screen changes repaint nothing.
   */
  take(w: number, h: number): Repaint {
    const full = this.full;
    const r = this.rect;
    this.full = false;
    this.rect = null;
    if (full) return { kind: "full" };
    if (!r) return { kind: "none" };
    const x0 = Math.max(0, Math.floor(r.x0));
    const y0 = Math.max(0, Math.floor(r.y0));
    const x1 = Math.min(w, Math.ceil(r.x1));
    const y1 = Math.min(h, Math.ceil(r.y1));
    if (x1 <= x0 || y1 <= y0) return { kind: "none" };
    if ((x1 - x0) * (y1 - y0) > FULL_REPAINT_SHARE * w * h) return { kind: "full" };
    return { kind: "rect", rect: { x0, y0, x1, y1 } };
  }
}
