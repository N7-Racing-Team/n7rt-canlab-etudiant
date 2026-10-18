import { describe, expect, it } from "vitest";
import type { Kind, Rot, ValueMap } from "../types";
import { type FPt, dirVec, intersects, rectOf, rotatePt } from "./geom";
import { buildComp } from "./model";
import {
  type Stroke, alphaOfX, buttonClosed, dynamicGeom, innerTexts, symbolGeom, symbolVariant, valueText, wiperX,
} from "./symbols";
import { LIBRARY, comp, libDef, monoMeasure } from "./testkit";

const NB = " ";
const ROTS: Rot[] = [0, 90, 180, 270];
const VARIANTS: Partial<Record<Kind, string[]>> = { capacitor: ["", "polarized"], can_bus: ["", "termination"] };
const DRAWN = LIBRARY.filter((d) => d.kind !== "junction");

/** States that the overlay can show, per kind with state-dependent parts. */
const STATES: Partial<Record<Kind, { params: ValueMap; state: ValueMap }[]>> = {
  switch: [{ params: {}, state: { closed: false } }, { params: {}, state: { closed: true } }],
  push_button: [
    { params: { contact: "no", mode: "momentary" }, state: { pressed: false, latched: false } },
    { params: { contact: "no", mode: "momentary" }, state: { pressed: true, latched: false } },
  ],
  potentiometer: [0, 0.5, 1].map((position) => ({ params: {}, state: { position } })),
  mlx90377: [0, 0.25, 1].map((position) => ({ params: {}, state: { position } })),
};

function segments(s: Stroke): [FPt, FPt][] {
  const out: [FPt, FPt][] = [];
  for (let i = 0; i + 1 < s.pts.length; i++) out.push([s.pts[i], s.pts[i + 1]]);
  if (s.closed && s.pts.length > 2) out.push([s.pts[s.pts.length - 1], s.pts[0]]);
  return out;
}

function onSegment(p: FPt, a: FPt, b: FPt): boolean {
  const cross = (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
  return Math.abs(cross) < 1e-9 &&
    p[0] >= Math.min(a[0], b[0]) - 1e-9 && p[0] <= Math.max(a[0], b[0]) + 1e-9 &&
    p[1] >= Math.min(a[1], b[1]) - 1e-9 && p[1] <= Math.max(a[1], b[1]) + 1e-9;
}

describe("symbol geometry (contract §2.2)", () => {
  it("puts each pin at the end of a stroke of its symbol, reached along the pin direction", () => {
    for (const def of DRAWN) {
      for (const variant of VARIANTS[def.kind] ?? [""]) {
        const g = symbolGeom(def, variant);
        for (const pin of def.pins) {
          const lead = g.leads.find((s) => {
            const end = s.pts[s.pts.length - 1];
            return end[0] === pin.pos[0] && end[1] === pin.pos[1];
          });
          expect(lead, `${def.kind}/${variant}.${pin.id}`).toBeDefined();
          const inner = lead!.pts[0];
          const v = dirVec(pin.dir);
          const along = (pin.pos[0] - inner[0]) * v[0] + (pin.pos[1] - inner[1]) * v[1];
          const across = (pin.pos[0] - inner[0]) * v[1] - (pin.pos[1] - inner[1]) * v[0];
          expect(along, `${def.kind}.${pin.id} lead length`).toBeGreaterThanOrEqual(1);
          expect(across).toBe(0);
        }
      }
    }
  });

  it("joins the inner end of every lead to the drawing (state-dependent parts included)", () => {
    // The bus model's GND lead has nothing to meet: §2.2 draws only the CANH and CANL lines.
    const loose = new Set(["can_bus.gnd"]);
    for (const def of DRAWN) {
      for (const variant of VARIANTS[def.kind] ?? [""]) {
        const g = symbolGeom(def, variant);
        const strokes = [...g.body];
        for (const s of STATES[def.kind] ?? []) strokes.push(...(dynamicGeom(def.kind, s.params, s.state)?.strokes ?? []));
        for (const [i, pin] of def.pins.entries()) {
          if (loose.has(`${def.kind}.${pin.id}`)) continue;
          const inner = g.leads[i].pts[0];
          const met = strokes.some((s) => segments(s).some(([a, b]) => onSegment(inner, a, b)));
          expect(met, `${def.kind}/${variant}.${pin.id} at ${inner}`).toBe(true);
        }
      }
    }
  });

  it("keeps every pin at a lead end once the component is turned and placed", () => {
    for (const def of DRAWN) {
      for (const rot of ROTS) {
        const c = comp(1, def.kind, [5, -3], rot);
        const g = buildComp(c, def, monoMeasure);
        const ends = symbolGeom(def, g.variant).leads.map((s) => {
          const r = rotatePt(s.pts[s.pts.length - 1], rot);
          return `${r[0] + 5},${r[1] - 3}`;
        });
        for (const p of g.pins) expect(ends, `${def.kind}@${rot}.${p.id}`).toContain(`${p.pos[0]},${p.pos[1]}`);
      }
    }
  });

  it("caches by variant: polarized capacitor, remote termination of the bus", () => {
    expect(symbolVariant("capacitor", { polarized: true })).toBe("polarized");
    expect(symbolVariant("capacitor", { polarized: false })).toBe("");
    expect(symbolVariant("can_bus", { remote_termination: true })).toBe("termination");
    expect(symbolVariant("can_bus", { remote_termination: false })).toBe("");
    expect(symbolGeom(libDef("capacitor"), "polarized")).not.toEqual(symbolGeom(libDef("capacitor"), ""));
  });
});

describe("texts", () => {
  it("never cover a connection point, and outer texts never cross a lead", () => {
    for (const def of DRAWN) {
      for (const rot of ROTS) {
        const c = comp(1, def.kind, [0, 0], rot, def.prefix ? `${def.prefix}12` : "");
        const g = buildComp(c, def, monoMeasure);
        const inner = innerTexts(def, c.params).length;
        const leads = symbolGeom(def, g.variant).leads.map((s) => s.pts.map((q) => rotatePt(q, rot)));
        g.texts.forEach((t, i) => {
          const box = { x0: t.box.x0 + 0.01, y0: t.box.y0 + 0.01, x1: t.box.x1 - 0.01, y1: t.box.y1 - 0.01 };
          for (const p of g.pins) {
            const inside = p.pos[0] > box.x0 && p.pos[0] < box.x1 && p.pos[1] > box.y0 && p.pos[1] < box.y1;
            expect(inside, `${def.kind}@${rot} "${t.text}" covers ${p.id}`).toBe(false);
          }
          if (i < inner) return;
          for (const l of leads) expect(intersects(rectOf(l[0], l[1]), box), `${def.kind}@${rot} "${t.text}" crosses a lead`).toBe(false);
        });
      }
    }
  });

  it("puts the designator above (left when vertical) and the value below (right), read upright", () => {
    const r0 = buildComp(comp(1, "resistor", [0, 0], 0, "R1"), libDef("resistor"), monoMeasure);
    const [label0, value0] = r0.texts;
    expect(label0.text).toBe("R1");
    expect(label0.box.y1).toBeLessThanOrEqual(r0.body.y0);
    expect(value0.box.y0).toBeGreaterThanOrEqual(r0.body.y1);
    expect(label0.vertical).toBe(false);
    const r90 = buildComp(comp(1, "resistor", [0, 0], 90, "R1"), libDef("resistor"), monoMeasure);
    expect(r90.texts[0].box.x1).toBeLessThanOrEqual(r90.body.x0);
    expect(r90.texts[1].box.x0).toBeGreaterThanOrEqual(r90.body.x1);
    expect(r90.texts.every((t) => t.vertical)).toBe(true);
    const r180 = buildComp(comp(1, "resistor", [0, 0], 180, "R1"), libDef("resistor"), monoMeasure);
    expect(r180.texts[0].box.y1).toBeLessThanOrEqual(r180.body.y0);
    expect(r180.texts.some((t) => t.vertical)).toBe(false);
    // The supply's leads are vertical at 0°: designator on the left, annotation on the right.
    const v = buildComp(comp(1, "dc_supply", [0, 0], 0, "V1"), libDef("dc_supply"), monoMeasure);
    expect(v.texts[0].box.x1).toBeLessThanOrEqual(v.body.x0);
    expect(v.texts[1].box.x0).toBeGreaterThanOrEqual(v.body.x1);
  });

  it("shows the main value from the parameters", () => {
    const val = (kind: Kind, params: ValueMap = {}): string | null => valueText(libDef(kind), comp(1, kind, [0, 0], 0, "", params).params);
    expect(val("resistor")).toBe(`10${NB}kΩ`);
    expect(val("termination")).toBe(`120${NB}Ω`);
    expect(val("capacitor")).toBe(`10${NB}µF`);
    expect(val("potentiometer", { resistance: 4700 })).toBe(`4.7${NB}kΩ`);
    expect(val("dc_supply")).toBe(`5${NB}V · 100${NB}mA`);
    expect(val("adc_input")).toBe(`12 bits · 3.3${NB}V`);
    expect(val("switch")).toBeNull();
    const nodes = (n: number): string | undefined =>
      innerTexts(libDef("can_bus"), comp(1, "can_bus", [0, 0], 0, "", { remote_nodes: n }).params).find((t) => t.role === "value")?.text;
    expect([0, 1, 3].map(nodes)).toEqual(["0 nœud distant", "1 nœud distant", "3 nœuds distants"]);
  });
});

describe("state-dependent parts (overlay)", () => {
  it("draws the switch blade open or closed", () => {
    expect(dynamicGeom("switch", {}, { closed: false })?.strokes[0].pts).toEqual([[-1.5, 0], [1.5, -1.5]]);
    expect(dynamicGeom("switch", {}, { closed: true })?.strokes[0].pts).toEqual([[-1.5, 0], [1.5, 0]]);
  });

  it("keeps the make-contact marks at both ends of the blade, so that a closed switch is not a plain wire", () => {
    const body = symbolGeom(libDef("switch"), "").body.map((s) => s.pts);
    expect(body).toEqual([[[-1.5, 0], [-1.5, -0.75]], [[1.5, 0], [1.5, -0.75]]]);
    const closed = dynamicGeom("switch", {}, { closed: true })!.strokes[0].pts;
    for (const end of closed) expect(body.some((m) => m[0][0] === end[0] && m[0][1] === end[1])).toBe(true);
  });

  it("puts the wiper arrow at x = −2 + 4α, and α = clamp((x + 2)/4) quantised to 1e−3", () => {
    expect(wiperX(0)).toBe(-2);
    expect(wiperX(1)).toBe(2);
    expect(wiperX(0.3)).toBeCloseTo(-0.8, 12);
    const arrow = dynamicGeom("potentiometer", {}, { position: 0.25 });
    expect(arrow?.strokes[0].pts).toEqual([[0, -2], [-1, -2], [-1, -1.25]]);
    expect(arrow?.fills[0]).toContainEqual([-1, -0.75]);
    expect([-2, 2, 0, -0.8, 9, -9, 0.0021].map(alphaOfX)).toEqual([0, 1, 0.5, 0.3, 1, 0, 0.501]);
  });

  it("puts the pedal-travel knob of the sensor on its track, at rest on the left by default", () => {
    const knob = (state: ValueMap | undefined) => dynamicGeom("mlx90377", {}, state);
    expect(knob({ position: 0 })?.strokes[0].pts).toEqual([[-2, -1], [-2, 1]]);
    expect(knob({ position: 1 })?.strokes[0].pts).toEqual([[2, -1], [2, 1]]);
    expect(knob({ position: 0.25 })?.fills[0]).toEqual([[-1.4, -0.4], [-0.6, -0.4], [-0.6, 0.4], [-1.4, 0.4]]);
    expect(knob(undefined)?.strokes[0].pts).toEqual([[-2, -1], [-2, 1]]);
    // Its drag zone spans the track, so that the pointer maps to the travel like a wiper.
    const zone = libDef("mlx90377").actuators[0];
    expect(zone).toEqual({ kind: "drag_pot", zone: { x0: -2, y0: -1, x1: 2, y1: 1 } });
    expect([-2, 2].map(alphaOfX)).toEqual([0, 1]);
  });

  it("names the black box and the sensor, with their connector pins and no parameter value", () => {
    for (const kind of ["ecu_apps", "mlx90377"] as const) {
      const def = libDef(kind);
      const texts = innerTexts(def, comp(1, kind, [0, 0]).params).map((t) => t.text);
      expect(texts[0]).toBe(kind === "ecu_apps" ? "ECU APPS" : "377ADB");
      for (const p of def.pins) expect(texts).toContain(p.short);
      expect(valueText(def, comp(1, kind, [0, 0]).params)).toBeNull();
    }
  });

  it("closes the push-button contact as (momentary ? pressed : latched) XOR nc", () => {
    const cases: [string, string, boolean, boolean, boolean][] = [
      ["no", "momentary", false, false, false], ["no", "momentary", true, false, true],
      ["nc", "momentary", false, false, true], ["nc", "momentary", true, false, false],
      ["no", "latching", false, true, true], ["no", "latching", true, false, false],
      ["nc", "latching", false, true, false],
    ];
    for (const [contact, mode, pressed, latched, closed] of cases) {
      expect(buttonClosed({ contact, mode }, { pressed, latched }), `${contact}/${mode}/${pressed}/${latched}`).toBe(closed);
    }
    const blade = (pressed: boolean): FPt[] | undefined =>
      dynamicGeom("push_button", { contact: "no", mode: "momentary" }, { pressed, latched: false })?.strokes[0].pts;
    expect(blade(true)).toEqual([[-1.5, 0], [1.5, 0]]);
    expect(blade(false)).toEqual([[-1.5, -0.75], [1.5, -0.75]]);
    const latchMark = dynamicGeom("push_button", { contact: "no", mode: "latching" }, { pressed: false, latched: false });
    expect(latchMark?.strokes.length).toBe(5);
  });
});
