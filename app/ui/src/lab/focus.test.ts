import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { keepFocusOnPress } from "./focus";

// Node has no DOM: a few elements and a document that apply Chromium's default actions — a mouse press focuses the
// button under it unless mousedown is cancelled; Space on a focused button clicks it on keyup unless keydown is
// cancelled. The view gives Space to the schematic when no button has the focus (view.ts, onKey).

class FakeElement {
  parent: FakeElement | null = null;
  ownerDocument!: FakeDocument;
  constructor(readonly tag: string, readonly id = "") {}
  closest(sel: string): FakeElement | null {
    for (let e: FakeElement | null = this; e; e = e.parent) if (e.tag === sel) return e;
    return null;
  }
}
class FakeHTMLElement extends FakeElement {
  isContentEditable = false;
  blur(): void {
    if (this.ownerDocument.activeElement === this) {
      this.ownerDocument.activeElement = this.ownerDocument.body;
      this.ownerDocument.log.push(`blur:${this.id}`);
    }
  }
}
class FakeButton extends FakeHTMLElement {
  disabled = false;
  clicks = 0;
  constructor(id: string, readonly onClick: () => void = () => undefined) {
    super("button", id);
  }
}
class FakeInput extends FakeHTMLElement {
  constructor(id: string) {
    super("input", id);
  }
}

class FakeDocument {
  readonly body = new FakeHTMLElement("body", "body");
  activeElement: FakeElement = this.body;
  readonly log: string[] = [];
  constructor(private readonly onMouseDown: ((e: MouseEvent) => void) | null) {}

  adopt<T extends FakeElement>(e: T, parent: FakeElement = this.body): T {
    e.ownerDocument = this;
    e.parent = parent;
    return e;
  }

  /** A real mouse press and release on `target`. */
  click(target: FakeElement): void {
    let prevented = false;
    const ev = { target, preventDefault: () => { prevented = true; } } as unknown as MouseEvent;
    this.onMouseDown?.(ev);
    const b = target.closest("button") as FakeButton | null;
    if (!prevented) {
      const before = this.activeElement;
      this.activeElement = b ?? this.body;
      if (before !== this.activeElement && before instanceof FakeHTMLElement && before.tag === "input") this.log.push(`blur:${before.id}`);
    }
    if (b && !b.disabled) {
      b.clicks++;
      b.onClick();
    }
  }

  /** Space pressed and released: the focused button keeps it (view.ts), anything else goes to the schematic. */
  space(): "button" | "schematic" {
    const t = this.activeElement;
    if (t instanceof FakeButton) {
      t.clicks++;
      t.onClick();
      return "button";
    }
    return "schematic";
  }
}

const g = globalThis as unknown as Record<string, unknown>;
const NAMES = ["Element", "HTMLElement", "HTMLButtonElement", "HTMLInputElement", "HTMLSelectElement", "HTMLTextAreaElement"];
const saved = new Map<string, unknown>();

beforeAll(() => {
  for (const n of NAMES) saved.set(n, g[n]);
  class Unused {}
  Object.assign(g, {
    Element: FakeElement, HTMLElement: FakeHTMLElement, HTMLButtonElement: FakeButton, HTMLInputElement: FakeInput,
    HTMLSelectElement: Unused, HTMLTextAreaElement: Unused,
  });
});

afterAll(() => {
  for (const n of NAMES) g[n] = saved.get(n);
});

function lab(onMouseDown: ((e: MouseEvent) => void) | null) {
  const doc = new FakeDocument(onMouseDown);
  const main = doc.adopt(new FakeHTMLElement("main", "phys-main"));
  let fits = 0;
  const fit = doc.adopt(new FakeButton("phys-fit", () => fits++), main);
  const icon = doc.adopt(new FakeElement("svg"), fit);
  const reset = doc.adopt(new FakeButton("phys-reset"), main);
  const canvas = doc.adopt(new FakeHTMLElement("div", "phys-canvas"), main);
  const name = doc.adopt(new FakeInput("phys-file-name"), main);
  return { doc, fit, icon, reset, canvas, name, fits: () => fits };
}

describe("mouse presses on the view's buttons", () => {
  it("leave Space to the schematic after a click on Ajuster: it pans, Ajuster is not pressed again", () => {
    const { doc, icon, fit, fits } = lab(keepFocusOnPress);
    doc.click(icon);
    expect(fits()).toBe(1);
    expect(doc.activeElement).toBe(doc.body);
    expect(doc.space()).toBe("schematic");
    expect(fits()).toBe(1);
    expect(fit.clicks).toBe(1);
  });

  it("is what the model reproduces without the fix: the clicked button takes Space", () => {
    const { doc, fit, fits } = lab(null);
    doc.click(fit);
    expect(doc.space()).toBe("button");
    expect(fits()).toBe(2);
  });

  it("keep the schematic focused, still leave a focused field (its value is committed), ignore other targets", () => {
    const { doc, reset, canvas, name } = lab(keepFocusOnPress);
    doc.activeElement = canvas;
    doc.click(reset);
    expect(reset.clicks).toBe(1);
    expect(doc.activeElement).toBe(canvas);
    doc.activeElement = name;
    doc.click(reset);
    expect(doc.log).toEqual(["blur:phys-file-name"]);
    expect(doc.activeElement).toBe(doc.body);
    expect(doc.space()).toBe("schematic");
    expect(reset.clicks).toBe(2);
  });

  it("leave disabled buttons and non-buttons to the browser", () => {
    for (const target of ["disabled", "canvas", "field"] as const) {
      const { doc, reset, canvas, name } = lab(null);
      reset.disabled = true;
      let prevented = false;
      const el = target === "disabled" ? reset : target === "canvas" ? canvas : name;
      keepFocusOnPress({ target: el, preventDefault: () => { prevented = true; } } as unknown as MouseEvent);
      expect(prevented, target).toBe(false);
      expect(doc.log, target).toEqual([]);
    }
  });
});
