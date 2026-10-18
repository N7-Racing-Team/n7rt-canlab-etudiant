// Sparse spatial hash of 16 × 16-unit cells (contract §8.5 "Index spatial"): draw-bounds, wire segments, pins and
// junctions, updated item by item from patches. Serves pointing, culling and dirty rectangles. Pure: no DOM.

import type { Rect } from "../types";
import { intersects } from "./geom";

export const CELL = 16;
const SPAN = 4096;
const BIAS = 2048;

const cellOf = (v: number): number => Math.min(BIAS - 1, Math.max(-BIAS, Math.floor(v / CELL)));
const cellKey = (cx: number, cy: number): number => (cx + BIAS) * SPAN + (cy + BIAS);

interface Entry<V> {
  rect: Rect;
  value: V;
  cells: number[];
}

export class SpatialHash<V> {
  private readonly cells = new Map<number, Set<string>>();
  private readonly entries = new Map<string, Entry<V>>();

  get size(): number {
    return this.entries.size;
  }

  has(key: string): boolean {
    return this.entries.has(key);
  }

  get(key: string): V | undefined {
    return this.entries.get(key)?.value;
  }

  rectOf(key: string): Rect | undefined {
    return this.entries.get(key)?.rect;
  }

  /** Insert, or move an existing key to its new rectangle. */
  set(key: string, rect: Rect, value: V): void {
    this.delete(key);
    const keys: number[] = [];
    const cx0 = cellOf(rect.x0);
    const cx1 = cellOf(rect.x1);
    const cy0 = cellOf(rect.y0);
    const cy1 = cellOf(rect.y1);
    for (let cx = cx0; cx <= cx1; cx++) {
      for (let cy = cy0; cy <= cy1; cy++) {
        const ck = cellKey(cx, cy);
        let set = this.cells.get(ck);
        if (!set) {
          set = new Set();
          this.cells.set(ck, set);
        }
        set.add(key);
        keys.push(ck);
      }
    }
    this.entries.set(key, { rect, value, cells: keys });
  }

  delete(key: string): boolean {
    const e = this.entries.get(key);
    if (!e) return false;
    for (const ck of e.cells) {
      const set = this.cells.get(ck);
      if (!set) continue;
      set.delete(key);
      if (set.size === 0) this.cells.delete(ck);
    }
    this.entries.delete(key);
    return true;
  }

  clear(): void {
    this.cells.clear();
    this.entries.clear();
  }

  /** Values whose rectangle intersects `rect` (closed), each once. */
  query(rect: Rect): V[] {
    const out: V[] = [];
    const cx0 = cellOf(rect.x0);
    const cx1 = cellOf(rect.x1);
    const cy0 = cellOf(rect.y0);
    const cy1 = cellOf(rect.y1);
    const cellCount = (cx1 - cx0 + 1) * (cy1 - cy0 + 1);
    if (cellCount > this.cells.size) {
      // Larger than the occupied area: a scan of the entries is cheaper than walking empty cells.
      for (const e of this.entries.values()) if (intersects(e.rect, rect)) out.push(e.value);
      return out;
    }
    const seen = new Set<string>();
    for (let cx = cx0; cx <= cx1; cx++) {
      for (let cy = cy0; cy <= cy1; cy++) {
        const set = this.cells.get(cellKey(cx, cy));
        if (!set) continue;
        for (const key of set) {
          if (seen.has(key)) continue;
          seen.add(key);
          const e = this.entries.get(key);
          if (e && intersects(e.rect, rect)) out.push(e.value);
        }
      }
    }
    return out;
  }
}
