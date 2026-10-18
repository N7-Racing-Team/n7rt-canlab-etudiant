import { beforeAll, describe, expect, it } from "vitest";
import type { LabState } from "../state";
import type { DiagMarker, LiveItem } from "./live";
import type { SceneModel } from "./model";
import { type Palette, Renderer } from "./render";
import { comp, makeModel, makeState, wire } from "./testkit";
import { Viewport } from "./view";

// A recording stand-in for CanvasRenderingContext2D and Path2D (node has no canvas): the drawing rules are checked
// on the calls the renderer makes.
interface Call {
  name: string;
  args: unknown[];
}

class FakePath {
  readonly ops: string[] = [];
  moveTo(x: number, y: number): void {
    this.ops.push(`M${x},${y}`);
  }
  lineTo(x: number, y: number): void {
    this.ops.push(`L${x},${y}`);
  }
  closePath(): void {
    this.ops.push("Z");
  }
  rect(x: number, y: number, w: number, h: number): void {
    this.ops.push(`R${x},${y},${w},${h}`);
  }
}

class FakeCtx {
  readonly calls: Call[] = [];
  readonly canvas = { width: 1000, height: 600 };
  fillStyle: unknown = "";
  strokeStyle: unknown = "";
  lineWidth = 1;
  lineJoin = "miter";
  lineCap = "butt";
  font = "";
  textAlign = "left";
  textBaseline = "alphabetic";

  constructor() {
    for (const name of ["setTransform", "save", "restore", "beginPath", "moveTo", "lineTo", "closePath", "rect", "clip",
      "fill", "stroke", "fillRect", "strokeRect", "clearRect", "fillText", "translate", "rotate", "setLineDash"]) {
      (this as unknown as Record<string, (...a: unknown[]) => void>)[name] = (...args: unknown[]) => {
        this.calls.push({ name, args: name === "fillText" ? [...args, this.fillStyle, this.font] : args });
      };
    }
  }

  measureText(t: string): { width: number } {
    return { width: t.length * 6.6 };
  }

  createPattern(): { setTransform(): void } {
    return { setTransform: () => undefined };
  }

  named(name: string): Call[] {
    return this.calls.filter((c) => c.name === name);
  }
}

const PAL: Palette = {
  bg: "bg", grid: "grid", gridMajor: "major", symbol: "symbol", pin: "pin", wire: "wire", label: "label", value: "value",
  live: "live", liveBg: "liveBg", select: "select", selectFill: "selectFill", hover: "hover", invalid: "invalid", warn: "warn",
  font: '"IBM Plex Mono", monospace', lineCss: 1.5,
};

beforeAll(() => {
  const g = globalThis as unknown as Record<string, unknown>;
  g.Path2D = FakePath;
  g.document = { createElement: () => ({ width: 0, height: 0, getContext: () => new FakeCtx() }) };
});

function scene(): { state: LabState; model: SceneModel } {
  const state = makeState(
    [comp(1, "resistor", [10, 0], 0, "R1"), comp(2, "junction", [20, 0]), comp(3, "capacitor", [30, 0], 0, "C1"),
      comp(4, "resistor", [20, 10], 90, "R2"), comp(5, "adc_input", [40, 10], 0, "ADC1")],
    [wire(10, [1, "p2"], [2, "j"]), wire(11, [2, "j"], [3, "p1"]), wire(12, [2, "j"], [4, "p1"])],
  );
  return { state, model: makeModel(state) };
}

function renderer(k: number, dpr = 1): { r: Renderer; vp: Viewport; state: LabState; model: SceneModel } {
  const { state, model } = scene();
  const vp = new Viewport();
  vp.dpr = dpr;
  vp.resize(1000, 600);
  vp.k = k;
  vp.ox = 50;
  vp.oy = 200;
  const r = new Renderer(vp, model, state, PAL);
  r.textsReady = true;
  return { r, vp, state, model };
}

const ctx2d = (c: FakeCtx): CanvasRenderingContext2D => c as unknown as CanvasRenderingContext2D;

describe("scene layer", () => {
  it("paints background and grid in one fill, all wires in one stroke, squares where ≥ 3 conductors meet", () => {
    const { r } = renderer(0);
    const c = new FakeCtx();
    r.paintScene(ctx2d(c), null);
    expect(c.named("fillRect")).toEqual([{ name: "fillRect", args: [0, 0, 1000, 600] }]);
    expect(c.named("clip")).toEqual([]);
    const plainStrokes = c.named("stroke").filter((s) => s.args.length === 0);
    // Wires, then the hollow squares of the unconnected pins: two batched strokes.
    expect(plainStrokes.length).toBe(2);
    // One filled square: the junction of degree 3 at device (250, 200). Side clamp(0.5·10, 4, 8) = 5 px, made even
    // like the 2 px stroke so that it centres on the wire: [247, 253) × [197, 203).
    expect(c.named("fill").length).toBe(1);
    expect(c.named("rect").filter((x) => x.args[2] === 6)).toEqual([{ name: "rect", args: [247, 197, 6, 6] }]);
  });

  it("strokes on whole pixels at width 2 (dpr 1) and on pixel centres at width 3 (dpr 2)", () => {
    for (const [dpr, half] of [[1, 0], [2, 0.5]] as const) {
      const { r } = renderer(0, dpr);
      const c = new FakeCtx();
      r.paintScene(ctx2d(c), null);
      const coords = [...c.named("moveTo"), ...c.named("lineTo")].flatMap((x) => x.args as number[]);
      expect(coords.length).toBeGreaterThan(0);
      for (const v of coords) expect(v - Math.floor(v)).toBe(half);
      expect(c.lineJoin).toBe("miter");
      expect(c.lineCap).toBe("butt");
    }
  });

  it("repaints a dirty rectangle inside a clip, without touching the rest", () => {
    const { r } = renderer(0);
    const c = new FakeCtx();
    r.paintScene(ctx2d(c), { x0: 100, y0: 150, x1: 180, y1: 260 });
    expect(c.named("clip").length).toBe(1);
    expect(c.named("fillRect")).toEqual([{ name: "fillRect", args: [100, 150, 80, 110] }]);
  });

  it("shows texts from 7 px, pin names from 8 px, bodies only below 3 px per unit", () => {
    const texts = (k: number): string[] => {
      const { r } = renderer(k);
      const c = new FakeCtx();
      r.paintScene(ctx2d(c), null);
      return c.named("fillText").map((x) => x.args[0] as string);
    };
    expect(texts(-3)).toEqual([]);
    const at7 = texts(-2);
    expect(at7).toContain("R1");
    expect(at7).toContain("ADC");
    expect(at7).not.toContain("IN");
    expect(texts(-1)).toContain("IN");
    const { r } = renderer(-6);
    const c = new FakeCtx();
    r.paintScene(ctx2d(c), null);
    expect(c.named("stroke").every((s) => s.args.length === 0)).toBe(true);
  });

  it("builds one grid tile per zoom step: a zoom sweep back and forth reuses them and frees each tile at once", () => {
    const g = globalThis as unknown as { document: { createElement: () => unknown } };
    const saved = g.document.createElement;
    const tiles: { width: number; height: number }[] = [];
    g.document.createElement = () => {
      const t = { width: 0, height: 0, getContext: () => new FakeCtx() };
      tiles.push(t);
      return t;
    };
    try {
      const { r, vp } = renderer(0);
      for (const k of [0, 1, 2, 3, 2, 1, 0, 1, 2, 3]) {
        vp.k = k;
        r.paintScene(ctx2d(new FakeCtx()), null);
      }
      expect(tiles.length).toBe(4);
      expect(tiles.every((t) => t.width === 0 && t.height === 0)).toBe(true);
      r.invalidateCaches();
      r.paintScene(ctx2d(new FakeCtx()), null);
      expect(tiles.length).toBe(5);
    } finally {
      g.document.createElement = saved;
    }
  });
});

describe("overlay layer", () => {
  it("draws the selection, state-dependent parts, markers and live values on a cleared layer", () => {
    const { r, state } = renderer(0);
    state.setSelection([1], [11]);
    const c = new FakeCtx();
    const live: LiveItem[] = [{ text: "1 2.50000 V", at: [7, 0], dir: "l" }];
    const diags: DiagMarker[] = [{ at: [12, -1], severity: "error" }];
    r.paintOverlay(ctx2d(c), null, live, diags);
    expect(c.calls[1]).toEqual({ name: "clearRect", args: [0, 0, 1000, 600] });
    const texts = c.named("fillText");
    expect(texts.map((t) => t.args[0])).toEqual(["1 2.50000 V"]);
    expect(texts[0].args[3]).toBe("live");
    // Marker: a small square centred on the item, in --sch-invalid (6 px at dpr 1).
    expect(c.named("fillRect")).toContainEqual({ name: "fillRect", args: [50 + 120 - 3, 200 - 10 - 3, 6, 6] });
  });

  it("keeps a neighbour's blade and the diagnostic markers visible around the live values (ref-pot-rc, RV1 selected)", () => {
    // Demo ref-pot-rc (crates/n7rt-circuit/demos/ref-pot-rc.json); markers on S1 and on the wire S1.2 → RV1.B,
    // where the box of pin B lands.
    const state = makeState([
      comp(1, "dc_supply", [0, 8], 0, "V1"), comp(2, "switch", [9, 0], 0, "S1"), comp(3, "potentiometer", [24, 0], 180, "RV1"),
      comp(4, "resistor", [31, 10], 0, "R1"), comp(5, "capacitor", [41, 15], 90, "C1"), comp(6, "adc_input", [31, 17], 0, "ADC1"),
      comp(7, "adc_input", [48, 10], 0, "ADC2"), comp(8, "ground", [0, 16]), comp(9, "ground", [31, 4]),
      comp(10, "ground", [31, 24]), comp(11, "ground", [41, 22]), comp(12, "ground", [48, 17]),
      comp(18, "junction", [24, 10]), comp(22, "junction", [41, 10]),
    ], [
      wire(13, [1, "pos"], [2, "p1"], [[0, 0]]), wire(14, [2, "p2"], [3, "b"]), wire(15, [3, "a"], [9, "gnd"], [[31, 0]]),
      wire(16, [1, "neg"], [8, "gnd"]), wire(17, [3, "w"], [18, "j"]), wire(19, [18, "j"], [6, "in"], [[24, 17]]),
      wire(20, [18, "j"], [4, "p1"]), wire(21, [4, "p2"], [22, "j"]), wire(23, [22, "j"], [7, "in"]),
      wire(24, [22, "j"], [5, "p1"]), wire(25, [5, "p2"], [11, "gnd"]), wire(26, [6, "ref"], [10, "gnd"]),
      wire(27, [7, "ref"], [12, "gnd"]),
    ]);
    const model = makeModel(state);
    state.setSelection([3], []);
    const reading: Record<string, string> = { a: "A 0.00000 V", b: "B 4.99999 V", w: "W 1.23967 V" };
    const live: LiveItem[] = model.comps.get(3)!.pins.map((p) => ({ text: reading[p.id], at: p.pos, dir: p.dir }));
    const diags: DiagMarker[] = [{ at: [11, -2], severity: "error" }, { at: [16.5, 0], severity: "warning" }];
    type Box = { x0: number; y0: number; x1: number; y1: number };
    const meets = (a: Box, b: Box) => a.x0 < b.x1 && b.x0 < a.x1 && a.y0 < b.y1 && b.y0 < a.y1;
    // dpr 1.25 as in the captures; every zoom step where the blade is drawn (k ≥ −5).
    for (const k of [-5, -4, -3, -2, -1, 0, 1, 2, 3, 4, 5, 6]) {
      const vp = new Viewport();
      vp.dpr = 1.25;
      vp.resize(1070, 700);
      vp.k = k;
      vp.ox = 120;
      vp.oy = 240;
      const r = new Renderer(vp, model, state, PAL);
      r.textsReady = true;
      const c = new FakeCtx();
      const drawn: { style: unknown; box: Box }[] = [];
      const rec = c as unknown as { fillRect: (...a: number[]) => void };
      const fill = rec.fillRect;
      rec.fillRect = (x: number, y: number, w: number, h: number) => {
        drawn.push({ style: c.fillStyle, box: { x0: x, y0: y, x1: x + w, y1: y + h } });
        fill(x, y, w, h);
      };
      r.paintOverlay(ctx2d(c), null, live, diags);
      const boxes = drawn.flatMap((d, i) => (d.style === "liveBg" ? [{ i, box: d.box }] : []));
      expect(boxes.length, `k=${k}`).toBe(3);
      // Every marker is painted, and no live box is painted over it afterwards.
      const markers = drawn.flatMap((d, i) => (d.style === "invalid" || d.style === "warn" ? [{ i, box: d.box }] : []));
      expect(markers.length, `k=${k}`).toBe(2);
      for (const m of markers) for (const b of boxes) if (b.i > m.i) expect(meets(m.box, b.box), `k=${k}`).toBe(false);
      // Here a free place exists: no box even lies under a marker (it would hide part of the reading).
      for (const m of markers) for (const b of boxes) expect(meets(m.box, b.box), `k=${k} under`).toBe(false);
      // The open blade of S1, (−1.5, 0) → (1.5, −1.5) around (9, 0), drawn before the boxes: none covers it.
      const [bx0, by0] = vp.toDevice([7.5, -1.5]);
      const [bx1, by1] = vp.toDevice([10.5, 0]);
      const blade = { x0: bx0 - 1, y0: by0 - 1, x1: bx1 + 1, y1: by1 + 1 };
      for (const b of boxes) expect(meets(blade, b.box), `k=${k}`).toBe(false);
    }
  });

  it("draws the gesture preview over the live values", () => {
    const { r, state } = renderer(0);
    state.setSelection([1], []);
    const c = new FakeCtx();
    const order: string[] = [];
    const rec = c as unknown as { fillRect: (...a: number[]) => void; stroke: (...a: unknown[]) => void };
    const fill = rec.fillRect;
    const stroke = rec.stroke;
    rec.fillRect = (...a: number[]) => {
      order.push(`fill:${String(c.fillStyle)}`);
      fill(...a);
    };
    rec.stroke = (...a: unknown[]) => {
      order.push(`stroke:${String(c.strokeStyle)}`);
      stroke(...a);
    };
    r.paintOverlay(ctx2d(c), { ghost: { t: "path", path: [[13, 0], [13, 5]] }, status: "pending", patch: null, refusal: null, marquee: null },
      [{ text: "2 2.50000 V", at: [13, 0], dir: "r" }], []);
    expect(order.lastIndexOf("stroke:select")).toBeGreaterThan(order.lastIndexOf("fill:liveBg"));
    expect(order.lastIndexOf("fill:liveBg")).toBeGreaterThan(-1);
  });

  it("draws a refused preview in --sch-invalid with its point", () => {
    const { r } = renderer(0);
    const c = new FakeCtx();
    const styles: unknown[] = [];
    const rec = c as unknown as { fillRect: (...a: number[]) => void };
    const fill = rec.fillRect;
    rec.fillRect = (...a: number[]) => {
      styles.push(c.fillStyle);
      fill(...a);
    };
    r.paintOverlay(ctx2d(c), {
      ghost: { t: "path", path: [[0, 5], [5, 5]] }, status: "refused", patch: null,
      refusal: { code: "ambiguous", ambiguity: "wire_over_pin", message_fr: "x", at: [5, 5], items: [] }, marquee: null,
    }, [], []);
    expect(styles).toContain("invalid");
    expect(c.strokeStyle).toBe("invalid");
  });
});
