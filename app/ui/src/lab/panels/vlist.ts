// Virtualised console list (contract docs/physical-lab-v1.md §8.4): fixed rows of 20 px, a fixed pool of row elements
// recycled as the list scrolls, text written with textContent inside the animation frame only. The window and the
// recycling are pure logic (tested); VirtualList is their DOM adapter.

import { el, setText } from "../../dom";

export const ROW_HEIGHT = 20;

export interface RowWindow {
  /** First item index shown. */
  first: number;
  /** One past the last item index shown. */
  end: number;
}

/** Items intersecting the viewport, plus `overscan` rows on each side, clamped to [0, total). */
export function visibleWindow(scrollTop: number, viewport: number, total: number, rowHeight = ROW_HEIGHT, overscan = 2): RowWindow {
  if (total <= 0 || viewport <= 0 || rowHeight <= 0) return { first: 0, end: 0 };
  const top = Math.max(0, scrollTop);
  const first = Math.max(0, Math.min(total, Math.floor(top / rowHeight) - overscan));
  const end = Math.max(first, Math.min(total, Math.ceil((top + viewport) / rowHeight) + overscan));
  return { first, end };
}

/** Rows needed to cover a viewport of this height with the overscan. */
export function poolSize(viewport: number, rowHeight = ROW_HEIGHT, overscan = 2): number {
  return Math.max(0, Math.ceil(viewport / rowHeight) + 1 + 2 * overscan);
}

/**
 * Assigns a pool of row slots to item indices. A slot whose index is still visible keeps it, so its row is neither
 * moved nor rewritten; freed slots take the newly visible indices in increasing order. Never more slots than needed.
 */
export class RowRecycler {
  /** slot → item index, −1 when free. */
  private slots: number[] = [];

  get size(): number {
    return this.slots.length;
  }

  indexOf(slot: number): number {
    return this.slots[slot] ?? -1;
  }

  /** Grows the pool to n slots (it never shrinks: extra slots are simply freed). */
  ensure(n: number): void {
    while (this.slots.length < n) this.slots.push(-1);
  }

  /**
   * Shows [first, end): returns the slots whose index changed, with their new index (−1: slot now unused).
   * The window must fit in the pool.
   */
  assign(first: number, end: number): { slot: number; index: number }[] {
    if (end - first > this.slots.length) throw new Error(`fenêtre de ${end - first} lignes pour ${this.slots.length} emplacements`);
    const changes: { slot: number; index: number }[] = [];
    const shown = new Set<number>();
    const free: number[] = [];
    this.slots.forEach((index, slot) => {
      if (index >= first && index < end) shown.add(index);
      else free.push(slot);
    });
    let f = 0;
    for (let index = first; index < end; index++) {
      if (shown.has(index)) continue;
      const slot = free[f++];
      this.slots[slot] = index;
      changes.push({ slot, index });
    }
    for (; f < free.length; f++) {
      const slot = free[f];
      if (this.slots[slot] !== -1) {
        this.slots[slot] = -1;
        changes.push({ slot, index: -1 });
      }
    }
    return changes;
  }
}

export interface ListRow {
  readonly row: HTMLElement;
  readonly cells: HTMLElement[];
}

/**
 * A scrolling list of fixed-height rows inside `viewport` (a `.tab-panel`). setItems() only records the data;
 * render(), called in the animation frame, reads the scroll position, recycles the rows and writes their text.
 */
export class VirtualList<T> {
  private items: readonly T[] = [];
  private readonly spacer: HTMLElement;
  private readonly rows: ListRow[] = [];
  private readonly recycler = new RowRecycler();
  private dirty = true;
  private contentChanged = true;
  private stick = true;

  constructor(
    private readonly viewport: HTMLElement,
    private readonly columns: string[],
    private readonly fill: (cells: HTMLElement[], item: T, row: HTMLElement) => void,
    onPick?: (item: T) => void,
    /** Newest last, follow the end while the reader is at the bottom (journal). */
    private readonly followEnd = false,
  ) {
    this.spacer = el("div", "lab-vlist-spacer");
    viewport.appendChild(this.spacer);
    viewport.addEventListener("scroll", () => {
      this.dirty = true;
      this.stick = viewport.scrollTop + viewport.clientHeight >= viewport.scrollHeight - 2;
    }, { passive: true });
    if (onPick) {
      viewport.addEventListener("click", (e) => {
        const row = (e.target as Element).closest<HTMLElement>(".lab-row");
        if (!row) return;
        const index = this.recycler.indexOf(Number(row.dataset.slot));
        if (index >= 0 && index < this.items.length) onPick(this.items[index]);
      });
    }
  }

  setItems(items: readonly T[]): void {
    this.items = items;
    this.dirty = true;
    this.contentChanged = true;
  }

  /** Rows must be rewritten (selection or values shown by fill() changed). */
  invalidate(): void {
    this.dirty = true;
    this.contentChanged = true;
  }

  get length(): number {
    return this.items.length;
  }

  render(): void {
    if (!this.dirty) return;
    const vp = this.viewport;
    const height = vp.clientHeight;
    if (height === 0) return; // hidden tab: rendered when shown
    this.dirty = false;
    const total = this.items.length;
    let top = vp.scrollTop;
    const spacerHeight = `${total * ROW_HEIGHT}px`;
    if (this.spacer.style.height !== spacerHeight) {
      this.spacer.style.height = spacerHeight;
      if (this.followEnd && this.stick) {
        top = Math.max(0, total * ROW_HEIGHT - height);
        vp.scrollTop = top;
      }
    }
    this.recycler.ensure(poolSize(height));
    while (this.rows.length < this.recycler.size) this.rows.push(this.newRow(this.rows.length));
    const { first, end } = visibleWindow(top, height, total);
    const refill = this.contentChanged;
    this.contentChanged = false;
    for (const { slot, index } of this.recycler.assign(first, end)) {
      const r = this.rows[slot];
      r.row.hidden = index < 0;
      if (index < 0) continue;
      r.row.style.transform = `translateY(${index * ROW_HEIGHT}px)`;
      if (!refill) this.fill(r.cells, this.items[index], r.row);
    }
    if (refill) {
      for (let slot = 0; slot < this.recycler.size; slot++) {
        const index = this.recycler.indexOf(slot);
        if (index >= 0) this.fill(this.rows[slot].cells, this.items[index], this.rows[slot].row);
      }
    }
  }

  private newRow(slot: number): ListRow {
    const row = el("div", "lab-row");
    row.dataset.slot = String(slot);
    row.hidden = true;
    const cells = this.columns.map((cls) => row.appendChild(el("span", cls)));
    this.spacer.appendChild(row);
    return { row, cells };
  }
}

/** Writes a cell's text, class and tooltip only when they changed. */
export function setCell(cell: HTMLElement, text: string, cls: string, title = ""): void {
  setText(cell, text);
  if (cell.className !== cls) cell.className = cls;
  if (cell.title !== title) cell.title = title;
}
