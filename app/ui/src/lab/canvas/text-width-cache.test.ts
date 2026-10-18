import { describe, expect, it, vi } from "vitest";
import { SceneModel } from "./model";
import { comp, libDef } from "./testkit";
import { TEXT_WIDTH_CACHE_LIMIT, TextWidthCache } from "./text-width-cache";

describe("text widths across document edits", () => {
  it("bounds retained labels while one component is renamed thousands of times", () => {
    const cache = new TextWidthCache();
    const read = vi.fn((text: string) => text.length * 6.6);
    const model = new SceneModel(libDef, (text) => cache.measure(text, () => read(text)));
    let previous = comp(1, "resistor", [0, 0], 0, "R0");
    model.rebuild([previous], []);
    for (let i = 1; i <= TEXT_WIDTH_CACHE_LIMIT * 2; i++) {
      const next = { ...previous, label: `R${i}` };
      model.applyPatch({ components: [[1, previous, next]], wires: [] });
      previous = next;
      expect(cache.size).toBeLessThanOrEqual(TEXT_WIDTH_CACHE_LIMIT);
    }
    expect(model.comps.size).toBe(1);
    const before = read.mock.calls.length;
    expect(cache.measure("R0", () => read("R0"))).toBe(13.2);
    expect(read.mock.calls.length).toBe(before + 1); // old label was evicted, not retained with old geometry
    expect(cache.size).toBe(TEXT_WIDTH_CACHE_LIMIT);
  });

  it("reuses exact widths, including zero, and recomputes after FIFO eviction", () => {
    const cache = new TextWidthCache(2);
    const read = vi.fn(() => 0);
    expect(cache.measure("R1", read)).toBe(0);
    expect(cache.measure("R1", read)).toBe(0);
    expect(read).toHaveBeenCalledOnce();
    cache.measure("R2", () => 10);
    cache.measure("R3", () => 20);
    expect(cache.size).toBe(2);
    expect(cache.measure("R1", () => 30)).toBe(30);
    expect(cache.size).toBe(2);
  });

  it("drops measurements when the document or font context is reset", () => {
    const cache = new TextWidthCache();
    cache.measure("R1", () => 10);
    cache.clear();
    expect(cache.size).toBe(0);
    expect(cache.measure("R1", () => 20)).toBe(20);
  });
});
