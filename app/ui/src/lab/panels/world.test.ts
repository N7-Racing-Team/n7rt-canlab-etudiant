import { describe, expect, it } from "vitest";
import { parseWorldValue, worldBounds, worldQuantity } from "./inspector";
import { dynamicGeom, DYNAMIC_KINDS, innerTexts, symbolGeom, valueText } from "../canvas/symbols";
import type { Component, Kind, KindDef, StateDef } from "../types";

const component = (kind: Kind, params = {}): Component => ({ id: 1, kind, params, label: "M1", pos: [0, 0], rot: 0 });
const state = (name: string, min: number, max: number): StateDef => ({ name, min, max, label_fr: name, unit: "°",
  vtype: "number", default: 0, persistent: true });

describe("public mechanical controls for the four missions", () => {
  it("exposes only the two axle quantities, the column angle and hydraulic pressure", () => {
    expect(worldQuantity("front_axle", "speed_mps")).toBe("speed_mps");
    expect(worldQuantity("front_axle", "steering_deg")).toBe("steering_deg");
    expect(worldQuantity("steering_column", "angle_deg")).toBe("angle_deg");
    expect(worldQuantity("brake_hydraulics", "pressure_bar")).toBe("pressure_bar");
    for (const kind of ["ecu_apps", "mlx90377", "ecu_wsf", "ats682lsh", "ecu_sas", "tle5014", "ecu_bps", "pss260"] as const) {
      expect(worldQuantity(kind, "angle_deg")).toBeNull();
    }
  });

  it("uses public mechanical stops and never widens the catalogue bounds", () => {
    const angle = state("angle_deg", -360, 360);
    expect(worldBounds(component("steering_column", { min_angle_deg: -120, max_angle_deg: 120 }), angle))
      .toEqual({ min: -120, max: 120 });
    expect(worldBounds(component("steering_column", { min_angle_deg: -999, max_angle_deg: 999 }), angle))
      .toEqual({ min: -360, max: 360 });
    expect(worldBounds(component("steering_column", { min_angle_deg: 90, max_angle_deg: -90 }), angle))
      .toEqual({ min: -360, max: -360 });
    expect(worldBounds(component("brake_hydraulics", { min_pressure_bar: 5, max_pressure_bar: 150 }), state("pressure_bar", 0, 250)))
      .toEqual({ min: 5, max: 150 });
    expect(worldBounds(component("front_axle"), state("speed_mps", 0, 100))).toEqual({ min: 0, max: 100 });
  });

  it("accepts finite signed decimal controls with commas and rejects malformed or out-of-range input", () => {
    for (const [text, expected] of [["-119,5", -119.5], ["+120", 120], [" -.5 ", -0.5], ["0", 0]] as const) {
      expect(parseWorldValue(text, -120, 120)).toBe(expected);
    }
    for (const text of ["", "NaN", "Infinity", "0x12", "1e2", "-120.1", "120.1", "2,3,4"]) {
      expect(parseWorldValue(text, -120, 120)).toBeNull();
    }
  });

  it("draws the three mechanical worlds from actual states without hidden model values", () => {
    for (const [kind, quantity] of [["front_axle", "steering_deg"], ["steering_column", "angle_deg"],
      ["brake_hydraulics", "pressure_bar"]] as const) {
      expect(DYNAMIC_KINDS.has(kind)).toBe(true);
      const atRest = dynamicGeom(kind, {}, { [quantity]: 0 });
      const moved = dynamicGeom(kind, { hidden_mapping: "do-not-render" }, { [quantity]: 60 });
      expect(moved).not.toEqual(atRest);
      const invalid = dynamicGeom(kind, {}, { [quantity]: NaN });
      expect(invalid).toEqual(atRest);
    }
  });

  it("renders all new electrical kinds with catalog body/pins and no hidden parameters", () => {
    for (const kind of ["ats682lsh", "ecu_wsf", "ecu_sas", "ecu_bps", "tle5014", "pss260", "pressure_switch"] as const) {
      const def: KindDef = { kind, body: { x0: -4, y0: -5, x1: 4, y1: 5 }, name_fr: kind, prefix: "U",
        pins: [{ id: "p", name: "Public pin", short: "P", pos: [-5, 0], dir: "l", role: "power_in", required: true }],
        params: [], states: [], actuators: [], placeable: true, sources_fr: "" };
      const geom = symbolGeom(def, "");
      expect(geom.body[0].pts).toEqual([[-4, -5], [4, -5], [4, 5], [-4, 5]]);
      expect(geom.leads[0].pts).toEqual([[-4, 0], [-5, 0]]);
      expect(innerTexts(def, { hidden: "secret" }).some((t) => t.text.includes("secret"))).toBe(false);
      expect(valueText(def, { hidden: "secret" })).toBeNull();
    }
  });
});
