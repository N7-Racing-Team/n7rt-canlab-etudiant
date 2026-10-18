// Rust ↔ TypeScript agreement (docs/physical-lab-v1.md §11): the JSON fixtures of this directory are real DTOs
// written by crates/n7rt-lab/tests/phys_fixtures.rs. Each one is decoded and every field checked against
// ../types.ts: exact key sets (no missing, no extra field), value types, literal sets and tagged unions.
//
// The validators are tied to types.ts at compile time: exact<T>() requires one check per key of T and rejects
// any other key, tagged<U>() one variant per tag value of U, oneOf<L>() every member of the literal union L. A
// change of types.ts that the validators do not follow fails `tsc`; a Rust change that types.ts does not
// follow fails this test.

import { describe, expect, it } from "vitest";
import type * as T from "../types";

type Check = (v: unknown, at: string) => void;

function fail(at: string, what: string): never {
  throw new Error(`${at}: ${what}`);
}

const optional = new WeakSet<Check>();
/** Union variant tags met while checking, by union name. */
const seen = new Map<string, Set<string>>();

function see(union: string, tag: string): void {
  let s = seen.get(union);
  if (!s) seen.set(union, (s = new Set()));
  s.add(tag);
}

const num: Check = (v, at) => {
  if (typeof v !== "number" || !Number.isFinite(v)) fail(at, `finite number expected, got ${JSON.stringify(v)}`);
};
const int: Check = (v, at) => {
  num(v, at);
  if (!Number.isInteger(v)) fail(at, `integer expected, got ${String(v)}`);
};
const str: Check = (v, at) => {
  if (typeof v !== "string") fail(at, `string expected, got ${JSON.stringify(v)}`);
};
const bool: Check = (v, at) => {
  if (typeof v !== "boolean") fail(at, `boolean expected, got ${JSON.stringify(v)}`);
};

function nullable(c: Check): Check {
  return (v, at) => {
    if (v !== null) c(v, at);
  };
}

/** A field that may be absent (TypeScript `key?:`). */
function opt(c: Check): Check {
  const f: Check = (v, at) => {
    if (v !== undefined) c(v, at);
  };
  optional.add(f);
  return f;
}

function arrayOf(c: Check): Check {
  return (v, at) => {
    if (!Array.isArray(v)) fail(at, `array expected, got ${JSON.stringify(v)}`);
    v.forEach((x, i) => c(x, `${at}[${i}]`));
  };
}

function tuple(...cs: Check[]): Check {
  return (v, at) => {
    if (!Array.isArray(v) || v.length !== cs.length) fail(at, `tuple of ${cs.length} expected, got ${JSON.stringify(v)}`);
    cs.forEach((c, i) => c(v[i], `${at}[${i}]`));
  };
}

function record(c: Check): Check {
  return (v, at) => {
    if (typeof v !== "object" || v === null || Array.isArray(v)) fail(at, "object expected");
    for (const [k, x] of Object.entries(v)) c(x, `${at}.${k}`);
  };
}

function lit<L extends string>(l: L): Check {
  return (v, at) => {
    if (v !== l) fail(at, `${JSON.stringify(l)} expected, got ${JSON.stringify(v)}`);
  };
}

/** One of the members of a string literal union; the record must list them all. */
function oneOf<L extends string>(values: Record<L, true>): Check {
  return (v, at) => {
    if (typeof v !== "string" || !Object.prototype.hasOwnProperty.call(values, v)) {
      fail(at, `one of ${Object.keys(values).join(", ")} expected, got ${JSON.stringify(v)}`);
    }
  };
}

/** An object with exactly the keys of O. */
function exact<O>() {
  return <S extends { [K in keyof O]-?: Check }>(fields: S & Record<Exclude<keyof S, keyof O>, never>): Check =>
    (v, at) => {
      if (typeof v !== "object" || v === null || Array.isArray(v)) fail(at, `object expected, got ${JSON.stringify(v)}`);
      const o = v as Record<string, unknown>;
      const checks = fields as unknown as Record<string, Check>;
      for (const k of Object.keys(o)) {
        if (!Object.prototype.hasOwnProperty.call(checks, k)) fail(at, `unexpected field "${k}"`);
      }
      for (const [k, c] of Object.entries(checks)) {
        if (!(k in o) && !optional.has(c)) fail(at, `missing field "${k}"`);
        c(o[k], `${at}.${k}`);
      }
    };
}

/** A union tagged by `tag`: one check per tag value of U. */
function tagged<U, K extends keyof U & string>(union: string, tag: K, variants: { [V in U[K] & string]: Check }): Check {
  return (v, at) => {
    if (typeof v !== "object" || v === null || Array.isArray(v)) fail(at, `object expected, got ${JSON.stringify(v)}`);
    const t = (v as Record<string, unknown>)[tag];
    const checks = variants as Record<string, Check>;
    if (typeof t !== "string" || !Object.prototype.hasOwnProperty.call(checks, t)) {
      fail(at, `${union}: unknown ${tag} ${JSON.stringify(t)}`);
    }
    see(union, t);
    checks[t](v, at);
  };
}

type Variant<U, K extends keyof U, V> = Extract<U, Record<K, V>>;

// ------------------------------------------------------------------------------------------------ document

const ROTS: Record<`${T.Rot}`, true> = { "0": true, "90": true, "180": true, "270": true };
const rot: Check = (v, at) => {
  if (typeof v !== "number" || !Object.prototype.hasOwnProperty.call(ROTS, String(v))) fail(at, `rotation expected, got ${String(v)}`);
};
const pt = tuple(int, int);
const value: Check = (v, at) => {
  if (typeof v === "number") num(v, at);
  else if (typeof v !== "boolean" && typeof v !== "string") fail(at, `parameter value expected, got ${JSON.stringify(v)}`);
};
const valueMap = record(value);

const kind = oneOf<T.Kind>({
  dc_supply: true, ground: true, resistor: true, capacitor: true, potentiometer: true, push_button: true, switch: true,
  adc_input: true, ecu_stm32: true, can_transceiver: true, can_bus: true, termination: true, ecu_apps: true, mlx90377: true,
  junction: true,
  ats682lsh: true, ecu_wsf: true, ecu_sas: true, ecu_bps: true, tle5014: true, pss260: true,
  pressure_switch: true, front_axle: true, steering_column: true, brake_hydraulics: true,
});
const role = oneOf<T.Role>({
  passive: true, source: true, ground: true, power_in: true, ground_pin: true, analog_in: true, logic_in: true,
  logic_out: true, canh: true, canl: true,
});
const dir = oneOf<T.Dir>({ l: true, r: true, u: true, d: true });
const valueType = oneOf<T.ValueType>({ number: true, number_or_inf: true, int: true, bool: true, choice: true });

const pinRef = exact<T.PinRef>()({ comp: int, pin: str });
const rect = exact<T.Rect>()({ x0: int, y0: int, x1: int, y1: int });
const pinDef = exact<T.PinDef>()({ id: str, name: str, short: str, pos: pt, dir, role, required: bool });
const paramDef = exact<T.ParamDef>()({
  name: str, label_fr: str, unit: str, vtype: valueType, min: num, max: num, default: value, choices: arrayOf(str),
  editable: bool,
});
const stateDef = exact<T.StateDef>()({
  name: str, label_fr: str, unit: str, vtype: valueType, min: num, max: num, default: value, persistent: bool,
});
const actuator = exact<T.Actuator>()({
  kind: oneOf<T.ActuatorKind>({ toggle_switch: true, press_button: true, drag_pot: true }),
  zone: rect,
});
const kindDef = exact<T.KindDef>()({
  kind, name_fr: str, prefix: str, body: rect, pins: arrayOf(pinDef), params: arrayOf(paramDef), states: arrayOf(stateDef),
  actuators: arrayOf(actuator), placeable: bool, sources_fr: str,
});
const component = exact<T.Component>()({ id: int, kind, label: str, pos: pt, rot, params: valueMap });
const wire = exact<T.Wire>()({ id: int, a: pinRef, b: pinRef, bends: arrayOf(pt) });
const net = exact<T.Net>()({ key: pinRef, name: str, ground: bool, pins: arrayOf(pinRef), wires: arrayOf(int) });

// --------------------------------------------------------------------------------------------------- edits

const endpoint = tagged<T.Endpoint, "type">("Endpoint", "type", {
  pin: exact<Variant<T.Endpoint, "type", "pin">>()({ type: lit("pin"), comp: int, pin: str }),
  on_wire: exact<Variant<T.Endpoint, "type", "on_wire">>()({ type: lit("on_wire"), wire: int, at: pt }),
});
const ids = arrayOf(int);
const edit = tagged<T.Edit, "op">("Edit", "op", {
  add_component: exact<Variant<T.Edit, "op", "add_component">>()({
    op: lit("add_component"), kind, pos: pt, rot, params: opt(valueMap), label: opt(nullable(str)),
  }),
  delete: exact<Variant<T.Edit, "op", "delete">>()({ op: lit("delete"), components: ids, wires: ids }),
  move: exact<Variant<T.Edit, "op", "move">>()({ op: lit("move"), components: ids, wires: ids, delta: pt }),
  rotate: exact<Variant<T.Edit, "op", "rotate">>()({ op: lit("rotate"), components: ids, wires: ids }),
  set_param: exact<Variant<T.Edit, "op", "set_param">>()({ op: lit("set_param"), comp: int, name: str, value }),
  set_label: exact<Variant<T.Edit, "op", "set_label">>()({ op: lit("set_label"), comp: int, label: str }),
  add_wire: exact<Variant<T.Edit, "op", "add_wire">>()({ op: lit("add_wire"), a: endpoint, b: endpoint, bends: opt(nullable(arrayOf(pt))) }),
  move_wire_end: exact<Variant<T.Edit, "op", "move_wire_end">>()({
    op: lit("move_wire_end"), wire: int, end: oneOf<T.End>({ a: true, b: true }), to: endpoint, bends: opt(nullable(arrayOf(pt))),
  }),
  set_bends: exact<Variant<T.Edit, "op", "set_bends">>()({ op: lit("set_bends"), wire: int, bends: arrayOf(pt) }),
});
const interact = tagged<T.Interact, "op">("Interact", "op", {
  set_world: exact<Variant<T.Interact, "op", "set_world">>()({ op: lit("set_world"), comp: int,
    quantity: oneOf<T.WorldQuantity>({ speed_mps: true, steering_deg: true, angle_deg: true, pressure_bar: true }), value: num }),
  set_pot: exact<Variant<T.Interact, "op", "set_pot">>()({ op: lit("set_pot"), comp: int, position: num }),
  set_button: exact<Variant<T.Interact, "op", "set_button">>()({ op: lit("set_button"), comp: int, pressed: bool }),
  set_switch: exact<Variant<T.Interact, "op", "set_switch">>()({ op: lit("set_switch"), comp: int, closed: bool }),
  set_power: exact<Variant<T.Interact, "op", "set_power">>()({ op: lit("set_power"), comp: int, on: bool }),
});
const patch = exact<T.Patch>()({
  components: arrayOf(tuple(int, nullable(component), nullable(component))),
  wires: arrayOf(tuple(int, nullable(wire), nullable(wire))),
});
const itemRef = tagged<T.ItemRef, "type">("ItemRef", "type", {
  component: exact<Variant<T.ItemRef, "type", "component">>()({ type: lit("component"), id: int }),
  wire: exact<Variant<T.ItemRef, "type", "wire">>()({ type: lit("wire"), id: int }),
  pin: exact<Variant<T.ItemRef, "type", "pin">>()({ type: lit("pin"), comp: int, pin: str }),
});
const refusalCode = oneOf<T.RefusalCode>({
  unknown_component: true, unknown_wire: true, unknown_pin: true, unknown_kind: true, same_pin: true, same_wire: true,
  duplicate_wire: true, non_orthogonal: true, out_of_bounds: true, no_route: true, param_unknown: true, param_type: true,
  param_parse: true, param_unit: true, param_out_of_range: true, param_locked: true, param_hidden: true, mission_locked: true, bench_locked: true,
  state_unknown: true, source_clock_owned: true, internal_only: true,
  state_out_of_range: true, not_interactive: true, label_invalid: true, label_duplicate: true, endpoint_not_on_wire: true,
  junction_on_endpoint: true, four_way_junction: true, ambiguous: true, stale_revision: true, nothing_to_undo: true,
  nothing_to_redo: true,
});
const ambiguity = oneOf<T.Ambiguity>({
  pin_on_pin: true, pin_on_wire: true, wire_over_pin: true, collinear_overlap: true, self_overlap: true, bend_on_wire: true,
  wire_through_body: true,
});
const refusal = exact<T.Refusal>()({ code: refusalCode, ambiguity: nullable(ambiguity), message_fr: str, at: nullable(pt), items: arrayOf(itemRef) });
const violation = exact<T.Violation>()({ code: str, message_fr: str, items: arrayOf(itemRef), at: nullable(pt) });

// -------------------------------------------------------------------------------------------------- values

function reading(v: Check): Check {
  type R = T.Reading<unknown>;
  return tagged<R, "k">("Reading", "k", {
    valid: exact<Variant<R, "k", "valid">>()({ k: lit("valid"), v }),
    saturated: exact<Variant<R, "k", "saturated">>()({ k: lit("saturated"), v }),
    floating: exact<Variant<R, "k", "floating">>()({ k: lit("floating") }),
    no_reference: exact<Variant<R, "k", "no_reference">>()({ k: lit("no_reference") }),
    undefined: exact<Variant<R, "k", "undefined">>()({ k: lit("undefined") }),
    open: exact<Variant<R, "k", "open">>()({ k: lit("open") }),
    unavailable: exact<Variant<R, "k", "unavailable">>()({ k: lit("unavailable"), reason: str }),
  });
}

const adcSample = exact<T.AdcSample>()({ volts: num, code: int, bits: int, vref: num, sampled_at_ns: int });
// Logic levels reach the UI through no DTO yet (probe API only), hence no LogicLevel in types.ts.
const logicLevel = oneOf<"low" | "high">({ low: true, high: true });
const opState = tagged<T.OpState, "kind">("OpState", "kind", {
  supply: exact<Variant<T.OpState, "kind", "supply">>()({
    kind: lit("supply"), output: oneOf<T.SupplyOutput>({ off: true, cv: true, cc: true, rev: true }),
  }),
  contact: exact<Variant<T.OpState, "kind", "contact">>()({ kind: lit("contact"), closed: bool }),
  pot: exact<Variant<T.OpState, "kind", "pot">>()({ kind: lit("pot"), position: num }),
  ecu: exact<Variant<T.OpState, "kind", "ecu">>()({ kind: lit("ecu"), powered: bool }),
  transceiver: exact<Variant<T.OpState, "kind", "transceiver">>()({
    kind: lit("transceiver"),
    powered: bool,
    rs: oneOf<T.RsMode>({ high_speed: true, slope_control: true, standby: true, undefined: true, floating: true }),
    txd: oneOf<T.TxdState>({ recessive: true, dominant_requested: true, dominant_timeout: true }),
  }),
  passive: exact<Variant<T.OpState, "kind", "passive">>()({ kind: lit("passive") }),
});
const severity = oneOf<T.Severity>({ error: true, warning: true, info: true });
const diagnostic = exact<T.Diagnostic>()({ code: str, severity, message_fr: str, items: arrayOf(itemRef) });
const runState = oneOf<T.RunState>({ run: true, stop: true });
const disclosure = oneOf<T.Disclosure>({ exploration: true, mission: true });
const volts = reading(num);
const netValue = exact<T.NetValue>()({ key: pinRef, name: str, voltage: volts });
const pinValue = exact<T.PinValue>()({ pin: str, voltage: volts, current: volts });
const measure = exact<T.Measure>()({ name: str, label_fr: str, unit: str, reading: volts });
const compValues = exact<T.CompValues>()({ id: int, state: valueMap, op: opState, pins: arrayOf(pinValue), measures: arrayOf(measure) });
const engineStats = exact<T.EngineStats>()({
  lag_frames: int, slow: bool, frame_us_p95: num, substeps_last: int, min_dt_ns: int, islands: int, restarts: int,
});
const meterFn = oneOf<T.MeterFn>({ v_dc: true, ohm: true });
const meterValue = exact<T.MeterValue>()({ com: nullable(pinRef), vohm: nullable(pinRef), function: meterFn, display: str });
const valuesDto = exact<T.ValuesDto>()({
  seq: int, doc_id: int, rev: int, sim_time_ns: int, run: runState, preview: bool, disclosure, nets: arrayOf(netValue),
  comps: arrayOf(compValues), diagnostics: arrayOf(diagnostic), meter: meterValue, engine: engineStats,
});

// ------------------------------------------------------------------------------------------------ commands

const physCommand = tagged<T.PhysCommand, "cmd">("PhysCommand", "cmd", {
  edit: exact<Variant<T.PhysCommand, "cmd", "edit">>()({ cmd: lit("edit"), base_rev: int, edit }),
  undo: exact<Variant<T.PhysCommand, "cmd", "undo">>()({ cmd: lit("undo") }),
  redo: exact<Variant<T.PhysCommand, "cmd", "redo">>()({ cmd: lit("redo") }),
  interact: exact<Variant<T.PhysCommand, "cmd", "interact">>()({ cmd: lit("interact"), interact }),
  run: exact<Variant<T.PhysCommand, "cmd", "run">>()({ cmd: lit("run") }),
  stop: exact<Variant<T.PhysCommand, "cmd", "stop">>()({ cmd: lit("stop") }),
  reset: exact<Variant<T.PhysCommand, "cmd", "reset">>()({ cmd: lit("reset") }),
  meter: exact<Variant<T.PhysCommand, "cmd", "meter">>()({ cmd: lit("meter"), com: nullable(pinRef), vohm: nullable(pinRef), function: meterFn }),
});
const journalEntry = exact<T.JournalEntry>()({ frame: int, command: physCommand });
const commandReply = tagged<T.CommandReply, "type">("CommandReply", "type", {
  edit: exact<Variant<T.CommandReply, "type", "edit">>()({
    type: lit("edit"), rev: int, patch, nets: nullable(arrayOf(net)), can_undo: bool, can_redo: bool, dirty: bool,
  }),
  interact: exact<Variant<T.CommandReply, "type", "interact">>()({ type: lit("interact"), rev: int, comp: int, state: valueMap }),
  run: exact<Variant<T.CommandReply, "type", "run">>()({ type: lit("run"), run: runState, sim_time_ns: int }),
});
const previewReply = exact<T.PreviewReply>()({ base_rev: int, patch });
const snapshotDto = exact<T.SnapshotDto>()({
  doc_id: int, rev: int, name: str, dirty: bool, components: arrayOf(component), wires: arrayOf(wire), states: arrayOf(tuple(int, valueMap)),
  nets: arrayOf(net), can_undo: bool, can_redo: bool, run: runState, sim_time_ns: int, disclosure,
});
const fileEntry = exact<T.FileEntry>()({ name: str, modified_unix_ms: int });
const demoEntry = exact<T.DemoEntry>()({ id: str, title_fr: str, summary_fr: str });
const fileError = tagged<T.FileError, "code">("FileError", "code", {
  bad_name: exact<Variant<T.FileError, "code", "bad_name">>()({ code: lit("bad_name") }),
  name_exists: exact<Variant<T.FileError, "code", "name_exists">>()({ code: lit("name_exists") }),
  not_found: exact<Variant<T.FileError, "code", "not_found">>()({ code: lit("not_found") }),
  invalid: exact<Variant<T.FileError, "code", "invalid">>()({ code: lit("invalid"), violations: arrayOf(violation) }),
  io: exact<Variant<T.FileError, "code", "io">>()({ code: lit("io"), message: str }),
  no_storage: exact<Variant<T.FileError, "code", "no_storage">>()({ code: lit("no_storage") }),
});
const physError = tagged<T.PhysError, "code">("PhysError", "code", {
  refused: exact<Variant<T.PhysError, "code", "refused">>()({ code: lit("refused"), refusal }),
  file: exact<Variant<T.PhysError, "code", "file">>()({ code: lit("file"), error: fileError }),
  engine_unavailable: exact<Variant<T.PhysError, "code", "engine_unavailable">>()({ code: lit("engine_unavailable") }),
});

// --------------------------------------------------------------------------------------------------- tests

/** Fixture name → the type it must decode as. */
const FIXTURES: Record<string, Check> = {
  library: arrayOf(kindDef),
  snapshot: snapshotDto,
  preview: previewReply,
  command_edit: commandReply,
  command_edit_geometry: commandReply,
  command_interact: commandReply,
  command_run: commandReply,
  journal: arrayOf(journalEntry),
  values: valuesDto,
  values_mission: valuesDto,
  readings_f64: arrayOf(volts),
  readings_adc: arrayOf(reading(adcSample)),
  readings_logic: arrayOf(reading(logicLevel)),
  readings_undefined: arrayOf(reading(logicLevel)),
  op_states: arrayOf(opState),
  files: arrayOf(fileEntry),
  demos: arrayOf(demoEntry),
  errors: arrayOf(physError),
};

const texts = import.meta.glob<string>("./*.json", { query: "?raw", import: "default", eager: true });

function parsed(name: string): unknown {
  const text = texts[`./${name}.json`];
  if (text === undefined) throw new Error(`${name}.json missing: run cargo test -p n7rt-lab --test phys_fixtures`);
  return JSON.parse(text);
}

function checkAll(): void {
  for (const [name, check] of Object.entries(FIXTURES)) check(parsed(name), name);
}

describe("DTO fixtures written by the engine (crates/n7rt-lab/tests/phys_fixtures.rs)", () => {
  it("every fixture has a type and every type a fixture", () => {
    const names = Object.keys(texts).map((p) => p.replace(/^\.\//, "").replace(/\.json$/, "")).sort();
    expect(names).toEqual(Object.keys(FIXTURES).sort());
  });

  for (const [name, check] of Object.entries(FIXTURES)) {
    it(`${name}.json decodes as types.ts declares`, () => {
      expect(() => check(parsed(name), name)).not.toThrow();
    });
  }

  it("the fixtures cover every variant of the unions the engine sends", () => {
    checkAll();
    const all: Record<string, string[]> = {
      Reading: ["valid", "saturated", "floating", "no_reference", "undefined", "open", "unavailable"],
      PhysCommand: ["edit", "undo", "redo", "interact", "run", "stop", "reset", "meter"],
      Interact: ["set_pot", "set_button", "set_switch", "set_power"],
      CommandReply: ["edit", "interact", "run"],
      OpState: ["supply", "contact", "pot", "ecu", "transceiver", "passive"],
      PhysError: ["refused", "file", "engine_unavailable"],
      FileError: ["bad_name", "name_exists", "not_found", "invalid", "io", "no_storage"],
    };
    for (const [union, tags] of Object.entries(all)) {
      expect([...(seen.get(union) ?? [])].sort(), union).toEqual([...tags].sort());
    }
  });

  it("a checker rejects missing, extra and mistyped fields", () => {
    const good = { name: "a", modified_unix_ms: 1 };
    expect(() => fileEntry(good, "ok")).not.toThrow();
    expect(() => fileEntry({ name: "a" }, "missing")).toThrow(/missing field "modified_unix_ms"/);
    expect(() => fileEntry({ ...good, extra: 1 }, "extra")).toThrow(/unexpected field "extra"/);
    expect(() => fileEntry({ ...good, modified_unix_ms: "1" }, "type")).toThrow(/finite number expected/);
    expect(() => volts({ k: "fictitious", v: 0 }, "tag")).toThrow(/unknown k/);
  });

  it("mission values hold no omniscient value", () => {
    const v = parsed("values_mission") as T.ValuesDto;
    expect(v.disclosure).toBe("mission");
    expect(v.nets).toEqual([]);
    expect(v.diagnostics).toEqual([]);
    for (const c of v.comps) {
      expect(c.pins).toEqual([]);
      expect(c.measures).toEqual([]);
      expect(c.op.kind).not.toBe("transceiver");
    }
    const open = parsed("values") as T.ValuesDto;
    expect(open.comps.map((c) => c.id)).toEqual(v.comps.map((c) => c.id));
  });
});
