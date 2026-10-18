// Text geometry reuses widths, but old labels/parameters must not accumulate for the window's lifetime.
export const TEXT_WIDTH_CACHE_LIMIT = 2048;

/** Bounded FIFO cache; a miss only repeats Canvas2D.measureText, never changes the computed geometry. */
export class TextWidthCache {
  private readonly widths = new Map<string, number>();

  constructor(private readonly limit = TEXT_WIDTH_CACHE_LIMIT) {
    if (!Number.isInteger(limit) || limit < 1) throw new Error("invalid text-width cache limit");
  }

  get size(): number { return this.widths.size; }

  measure(text: string, read: () => number): number {
    const cached = this.widths.get(text);
    if (cached !== undefined) return cached;
    const width = read();
    if (this.widths.size === this.limit) {
      const oldest = this.widths.keys().next().value;
      if (oldest !== undefined) this.widths.delete(oldest);
    }
    this.widths.set(text, width);
    return width;
  }

  clear(): void { this.widths.clear(); }
}
