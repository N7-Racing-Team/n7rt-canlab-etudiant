// Symbols of the V1 library (contract §2.2), in grid units at rotation 0, origin of the component. Leads are
// derived from the catalog pins (sent by the core), so the drawing always ends on the real connection points; the
// body drawing follows the §2.2 table. State-dependent parts (switch blade, push-button contact and cap, wiper
// arrow) are separate: they are drawn on the overlay, never cached. Pure: no DOM.

import type { Kind, KindDef, PinDef, ValueMap } from "../types";
import { type FPt, dirVec } from "./geom";
import { formatParam } from "./format";

export interface Stroke {
  pts: FPt[];
  closed: boolean;
}

export interface SymbolGeom {
  /** Pin leads, from the drawing to the connection point, which is the last point (--sch-pin). */
  leads: Stroke[];
  /** Body drawing (--sch-symbol). */
  body: Stroke[];
}

/** State-dependent parts, drawn on the overlay. */
export interface DynamicGeom {
  strokes: Stroke[];
  fills: FPt[][];
}

export type Align = "left" | "center" | "right";
export type TextRole = "label" | "value" | "title" | "pin";

/** Text at rotation 0: `at` is the alignment point horizontally and the middle of the line vertically. */
export interface TextSpec {
  text: string;
  at: FPt;
  align: Align;
  role: TextRole;
}

const line = (...pts: FPt[]): Stroke => ({ pts, closed: false });
const box = (x0: number, y0: number, x1: number, y1: number): Stroke =>
  ({ pts: [[x0, y0], [x1, y0], [x1, y1], [x0, y1]], closed: true });
const plus = (cx: number, cy: number, r: number): Stroke[] =>
  [line([cx - r, cy], [cx + r, cy]), line([cx, cy - r], [cx, cy + r])];

/** Kinds that have state-dependent parts on the overlay. */
export const DYNAMIC_KINDS: ReadonlySet<Kind> = new Set<Kind>(["switch", "push_button", "potentiometer", "mlx90377",
  "front_axle", "steering_column", "brake_hydraulics"]);

/** Kinds whose leads are vertical at rotation 0: designator on the left, value on the right. */
export const VERTICAL_AT_R0: ReadonlySet<Kind> = new Set<Kind>(["dc_supply"]);

/** Cache key variant (§2.2): `polarized` for the capacitor; the remote termination of the bus model too. */
export function symbolVariant(kind: Kind, params: ValueMap): string {
  if (kind === "capacitor" && params.polarized === true) return "polarized";
  if (kind === "can_bus" && params.remote_termination === true) return "termination";
  return "";
}

/** Distance from the drawing to the connection point along the pin direction. */
function leadLength(kind: Kind, pin: PinDef, variant: string): number {
  switch (kind) {
    case "capacitor": return pin.id === "p1" && variant === "polarized" ? 2 : 2.5;
    case "switch":
    case "push_button": return 1.5;
    default: return 1;
  }
}

function lead(p: PinDef, len: number): Stroke {
  const v = dirVec(p.dir);
  return line([p.pos[0] - v[0] * len, p.pos[1] - v[1] * len], [p.pos[0], p.pos[1]]);
}

function bodyStrokes(def: KindDef, variant: string): Stroke[] {
  const kind = def.kind;
  switch (kind) {
    case "dc_supply":
      return [
        line([0, -3], [0, -0.5]), line([0, 0.5], [0, 3]),
        line([-2, -0.5], [2, -0.5]), line([-1, 0.5], [1, 0.5]),
        ...plus(1.4, -1.4, 0.4),
      ];
    case "ground":
      return [line([-1.5, 1], [1.5, 1]), line([-1, 1.5], [1, 1.5]), line([-0.5, 2], [0.5, 2])];
    case "resistor":
    case "termination":
    case "potentiometer":
      return [box(-2, -0.75, 2, 0.75)];
    case "capacitor":
      return variant === "polarized"
        ? [box(-1, -1.5, -0.5, 1.5), line([0.5, -1.5], [0.5, 1.5]), ...plus(-1.6, -1.4, 0.35)]
        : [line([-0.5, -1.5], [-0.5, 1.5]), line([0.5, -1.5], [0.5, 1.5])];
    case "adc_input":
      return [box(-2, -2, 2, 2)];
    case "ecu_stm32":
      return [box(-7, -6, 7, 6)];
    case "can_transceiver":
      return [box(-6, -7, 6, 7)];
    case "ecu_apps":
      return [box(-7, -10, 7, 10)];
    case "ecu_wsf":
    case "ecu_sas":
    case "ecu_bps":
    case "ats682lsh":
    case "tle5014":
    case "pss260":
      return [box(def.body.x0, def.body.y0, def.body.x1, def.body.y1)];
    case "pressure_switch":
      return [box(def.body.x0, def.body.y0, def.body.x1, def.body.y1), line([-2, 0], [-1, 0]), line([1, 0], [2, 0]), line([-1, 0], [1, -0.75])];
    case "front_axle":
      return [line([-3, 0], [3, 0]), box(-3.75, -2, -2.25, 2), box(2.25, -2, 3.75, 2)];
    case "steering_column":
      return [box(-2.5, -2.5, 2.5, 2.5), line([0, 2.5], [0, 3.5])];
    case "brake_hydraulics":
      return [box(-3, -2, 3, 2), line([-3, 0], [-4, 0]), line([3, 0], [4, 0])];
    case "mlx90377":
      // Two dies above and below the pedal-travel track (x from −2 at rest to 2 at full travel).
      return [box(-6, -10, 6, 10), line([-2, 0], [2, 0]), line([-2, -0.6], [-2, 0.6]), line([2, -0.6], [2, 0.6])];
    case "can_bus":
      return [
        line([-3, -2], [9, -2]), line([-3, 2], [9, 2]),
        ...(variant === "termination"
          ? [box(8.25, -1.25, 9.75, 1.25), line([9, -2], [9, -1.25]), line([9, 1.25], [9, 2])]
          : []),
      ];
    case "switch":
      // Make contact: hinge and fixed-contact marks, so that a closed switch never reads as a plain wire. The blade
      // is state-dependent (overlay).
      return [line([-1.5, 0], [-1.5, -0.75]), line([1.5, 0], [1.5, -0.75])];
    default:
      // push_button: contact and cap are state-dependent; junction: a square, by degree.
      return [];
  }
}

const geomCache = new WeakMap<KindDef, Map<string, SymbolGeom>>();

/** Static drawing at rotation 0 for a kind and variant (cached per library entry). */
export function symbolGeom(def: KindDef, variant: string): SymbolGeom {
  let byVariant = geomCache.get(def);
  if (!byVariant) {
    byVariant = new Map();
    geomCache.set(def, byVariant);
  }
  let g = byVariant.get(variant);
  if (!g) {
    g = {
      leads: def.kind === "junction" ? [] : def.pins.map((p) => lead(p, leadLength(def.kind, p, variant))),
      body: bodyStrokes(def, variant),
    };
    byVariant.set(variant, g);
  }
  return g;
}

/** Contact closed ⇔ (momentary ? pressed : latched) XOR (contact = nc) (contract §2.1). */
export function buttonClosed(params: ValueMap, state: ValueMap | undefined): boolean {
  const held = params.mode === "latching" ? state?.latched === true : state?.pressed === true;
  return held !== (params.contact === "nc");
}

/** Wiper position α ∈ [0, 1] as returned by the core (catalog default `fallback`, 0.5, if absent). */
export function potPosition(state: ValueMap | undefined, fallback = 0.5): number {
  const v = state?.position;
  return typeof v === "number" && Number.isFinite(v) ? Math.min(1, Math.max(0, v)) : fallback;
}

/** Kinds driven by set_pot: the potentiometer wiper and the pedal travel of the position sensor. */
export const SLIDER_KINDS: ReadonlySet<Kind> = new Set<Kind>(["potentiometer", "mlx90377"]);

/** Wiper abscissa of the arrow at rotation 0: x_α = −2 + 4α (§2.2). */
export const wiperX = (alpha: number): number => -2 + 4 * alpha;

/** α of a pointer abscissa in the symbol frame, clamped to [0, 1] and quantised to 1e−3 like the core. */
export function alphaOfX(x: number): number {
  const a = Math.min(1, Math.max(0, (x + 2) / 4));
  return Math.round(a * 1000) / 1000;
}

/** State-dependent parts at rotation 0, or null for kinds without any. */
export function dynamicGeom(kind: Kind, params: ValueMap, state: ValueMap | undefined): DynamicGeom | null {
  const mechanical = (name: string, fallback = 0): number => {
    const value = state?.[name];
    return typeof value === "number" && Number.isFinite(value) ? value : fallback;
  };
  switch (kind) {
    case "steering_column": {
      const theta = mechanical("angle_deg") * Math.PI / 180;
      return { strokes: [line([0, 0], [2 * Math.sin(theta), -2 * Math.cos(theta)])], fills: [] };
    }
    case "front_axle": {
      const theta = mechanical("steering_deg") * Math.PI / 180;
      const dx = 0.75 * Math.cos(theta), dy = 0.75 * Math.sin(theta);
      const speed = Math.min(1, Math.max(0, mechanical("speed_mps") / 100));
      return { strokes: [line([-3 - dx, -dy], [-3 + dx, dy]), line([3 - dx, -dy], [3 + dx, dy]),
        line([0, 2], [0, 2 - 3 * speed])], fills: [] };
    }
    case "brake_hydraulics": {
      const p = Math.min(1, Math.max(0, mechanical("pressure_bar") / 250));
      return { strokes: [line([-2 + 4 * p, -1.8], [-2 + 4 * p, 1.8])], fills: [] };
    }
    case "switch": {
      const closed = state?.closed === true;
      return { strokes: [line([-1.5, 0], closed ? [1.5, 0] : [1.5, -1.5])], fills: [] };
    }
    case "push_button": {
      const latching = params.mode === "latching";
      const capDown = state?.pressed === true || (latching && state?.latched === true);
      const yb = buttonClosed(params, state) ? 0 : -0.75;
      const yc = capDown ? -2 : -2.5;
      const strokes = [line([-1.5, yb], [1.5, yb]), line([0, yc], [0, yb]), line([-1, yc], [1, yc])];
      if (latching) strokes.push(line([0, -1.5], [0.6, -1.5]), line([0.6, -1.8], [0.6, -1.2]));
      return { strokes, fills: [] };
    }
    case "potentiometer": {
      const x = wiperX(potPosition(state));
      return {
        strokes: [line([0, -2], [x, -2], [x, -1.25])],
        fills: [[[x - 0.3, -1.25], [x + 0.3, -1.25], [x, -0.75]]],
      };
    }
    case "mlx90377": {
      // Pedal travel: a square knob on the track, x = −2 + 4·position (rest on the left).
      const x = wiperX(potPosition(state, 0));
      return {
        strokes: [line([x, -1], [x, 1])],
        fills: [[[x - 0.4, -0.4], [x + 0.4, -0.4], [x + 0.4, 0.4], [x - 0.4, 0.4]]],
      };
    }
    default:
      return null;
  }
}

const TITLES: Partial<Record<Kind, [string, FPt]>> = {
  adc_input: ["ADC", [0, -1.3]],
  ecu_stm32: ["NUCLEO-H753ZI", [0, -5]],
  can_transceiver: ["MCP2551", [0, -4.6]],
  ecu_apps: ["ECU APPS", [0, -9]],
  ecu_wsf: ["ECU WSF", [0, -3]],
  ecu_sas: ["ECU SAS", [0, -3]],
  ecu_bps: ["ECU BPS", [0, -3]],
  ats682lsh: ["ATS682LSH", [0, -3]],
  tle5014: ["014S", [0, -3]],
  pss260: ["PSS260", [0, -3]],
  // The marking printed on the real part, not its reference: finding the right datasheet is the student's job.
  mlx90377: ["377ADB", [0, -9]],
};

const NAMED_PINS: ReadonlySet<Kind> = new Set<Kind>(["adc_input", "ecu_stm32", "can_transceiver", "can_bus", "ecu_apps", "mlx90377",
  "ecu_wsf", "ecu_sas", "ecu_bps", "ats682lsh", "tle5014", "pss260"]);

function pinName(kind: Kind, p: PinDef): TextSpec {
  if (kind === "can_bus") return { text: p.short, at: [p.pos[0] + 1.4, p.pos[1] - 0.7], align: "left", role: "pin" };
  const v = dirVec(p.dir);
  const ix = p.pos[0] - v[0];
  const iy = p.pos[1] - v[1];
  switch (p.dir) {
    case "l": return { text: p.short, at: [ix + 0.4, iy], align: "left", role: "pin" };
    case "r": return { text: p.short, at: [ix - 0.4, iy], align: "right", role: "pin" };
    case "u": return { text: p.short, at: [ix, iy + 0.95], align: "center", role: "pin" };
    default: return { text: p.short, at: [ix, iy - 0.95], align: "center", role: "pin" };
  }
}

function nodesText(n: unknown): string {
  const k = typeof n === "number" && Number.isFinite(n) ? n : 0;
  return k > 1 ? `${k} nœuds distants` : `${k} nœud distant`;
}

/** Texts inside the symbol (titles, pin names, bus nodes), at rotation 0. */
export function innerTexts(def: KindDef, params: ValueMap): TextSpec[] {
  const out: TextSpec[] = [];
  const title = TITLES[def.kind];
  if (title) out.push({ text: title[0], at: title[1], align: "center", role: "title" });
  if (NAMED_PINS.has(def.kind)) for (const p of def.pins) out.push(pinName(def.kind, p));
  if (def.kind === "can_bus") out.push({ text: nodesText(params.remote_nodes), at: [2.5, 0], align: "center", role: "value" });
  return out;
}

/** Main value shown outside the body (« 10 kΩ », « 5 V · 100 mA »), or null. */
export function valueText(def: KindDef, params: ValueMap): string | null {
  const q = (name: string): string => formatParam(params[name], def.params.find((p) => p.name === name)?.unit ?? "");
  switch (def.kind) {
    case "resistor":
    case "termination":
    case "potentiometer":
      return q("resistance");
    case "capacitor":
      return q("capacitance");
    case "dc_supply":
      return `${q("voltage")} · ${q("current_limit")}`;
    case "adc_input":
      return typeof params.bits === "number" ? `${params.bits} bits · ${q("vref")}` : q("vref");
    default:
      return null;
  }
}
