// Test fixtures of the canvas lot (imported by *.test.ts only, never by the app): the V1 library as the core sends
// it (transcribed from crates/n7rt-circuit/src/catalog.rs), document builders, a fake controller that records what
// the editor sends, and a monospace text measure (Plex Mono: 0.6 em, 6.6 px at 11 px).

import { LabState, type Outcome, type PreviewResult } from "../state";
import type {
  Actuator, CompId, Component, Dir, Edit, EditReply, Interact, Kind, KindDef, ParamDef, Patch, PinDef, Pt, Refusal, Role,
  Rot, StateDef, ValueMap, Wire, WireId,
} from "../types";
import type { EditorController, GestureEnv } from "./gestures";
import { SceneModel } from "./model";

const OHM = "Ω";

const pin = (id: string, short: string, pos: Pt, dir: Dir, role: Role, required = false): PinDef =>
  ({ id, name: short, short, pos, dir, role, required });
const num = (name: string, unit: string, min: number, max: number, def: number, editable = true): ParamDef =>
  ({ name, label_fr: name, unit, vtype: "number", min, max, default: def, choices: [], editable });
const int = (name: string, unit: string, min: number, max: number, def: number): ParamDef =>
  ({ name, label_fr: name, unit, vtype: "int", min, max, default: def, choices: [], editable: true });
const bool = (name: string, def: boolean): ParamDef =>
  ({ name, label_fr: name, unit: "", vtype: "bool", min: 0, max: 1, default: def, choices: [], editable: true });
const choice = (name: string, choices: string[], def: string): ParamDef =>
  ({ name, label_fr: name, unit: "", vtype: "choice", min: 0, max: 0, default: def, choices, editable: true });
const stateBool = (name: string, def: boolean, persistent = true): StateDef =>
  ({ name, label_fr: name, unit: "", vtype: "bool", min: 0, max: 1, default: def, persistent });
const rect = (x0: number, y0: number, x1: number, y1: number) => ({ x0, y0, x1, y1 });
const act = (kind: Actuator["kind"], zone: ReturnType<typeof rect>): Actuator => ({ kind, zone });

const TWO = (): PinDef[] => [pin("p1", "1", [-3, 0], "l", "passive"), pin("p2", "2", [3, 0], "r", "passive")];

function def(kind: Kind, prefix: string, body: ReturnType<typeof rect>, pins: PinDef[], params: ParamDef[],
  states: StateDef[] = [], actuators: Actuator[] = [], placeable = true): KindDef {
  return { kind, name_fr: kind, prefix, body, pins, params, states, actuators, placeable, sources_fr: "" };
}

export const LIBRARY: KindDef[] = [
  def("dc_supply", "V", rect(-2, -3, 2, 3),
    [pin("pos", "+", [0, -4], "u", "source"), pin("neg", "−", [0, 4], "d", "source")],
    [num("voltage", "V", 0, 30, 5), num("current_limit", "A", 0.001, 5, 0.1), num("r_out", OHM, 0.001, 10, 0.01)],
    [stateBool("on", true)]),
  def("ground", "", rect(-2, 1, 2, 2), [pin("gnd", "GND", [0, 0], "u", "ground")], []),
  def("resistor", "R", rect(-2, -1, 2, 1), TWO(), [num("resistance", OHM, 0.01, 1e8, 1e4), num("power_rating", "W", 0.0625, 10, 0.25)]),
  def("capacitor", "C", rect(-2, -2, 2, 2), TWO(),
    [num("capacitance", "F", 1e-12, 1, 1e-5), num("voltage_rating", "V", 1, 1000, 25), bool("polarized", false)]),
  def("potentiometer", "RV", rect(-2, -2, 2, 1),
    [pin("a", "A", [-3, 0], "l", "passive"), pin("b", "B", [3, 0], "r", "passive"), pin("w", "W", [0, -3], "u", "passive")],
    [num("resistance", OHM, 10, 1e7, 1e4), num("power_rating", "W", 0.05, 5, 0.25)],
    [{ name: "position", label_fr: "position", unit: "", vtype: "number", min: 0, max: 1, default: 0.5, persistent: true }],
    [act("drag_pot", rect(-2, -2, 2, -1))]),
  def("push_button", "BP", rect(-2, -3, 2, 1), TWO(),
    [choice("contact", ["no", "nc"], "no"), choice("mode", ["momentary", "latching"], "momentary"),
      num("r_on", OHM, 0.001, 10, 0.1), num("r_off", OHM, 1e6, 1e12, 1e8)],
    [stateBool("pressed", false, false), stateBool("latched", false)],
    [act("press_button", rect(-1, -3, 1, -2))]),
  def("switch", "S", rect(-2, -2, 2, 1), TWO(), [num("r_on", OHM, 0.001, 10, 0.01), num("r_off", OHM, 1e6, 1e12, 1e9)],
    [stateBool("closed", false)], [act("toggle_switch", rect(-1, -2, 1, 0))]),
  def("adc_input", "ADC", rect(-2, -2, 2, 2),
    [pin("in", "IN", [-3, 0], "l", "analog_in", true), pin("ref", "REF", [0, 3], "d", "ground_pin", true)],
    [num("vref", "V", 1, 5, 3.3), int("bits", "bit", 8, 16, 12),
      { name: "r_in", label_fr: "r_in", unit: OHM, vtype: "number_or_inf", min: 1e3, max: 1e12, default: "inf", choices: [], editable: true },
      int("sample_rate", "Hz", 1, 1000, 1000)]),
  def("ecu_stm32", "U", rect(-7, -6, 7, 6),
    [pin("v5", "5V", [-8, -4], "l", "source"), pin("v3v3", "3V3", [-8, -2], "l", "source"),
      pin("gnd", "GND", [-8, 4], "l", "ground_pin", true), pin("a0", "A0", [8, -4], "r", "analog_in"),
      pin("a1", "A1", [8, -2], "r", "analog_in"), pin("can_tx", "PD1", [8, 2], "r", "logic_out"),
      pin("can_rx", "PD0", [8, 4], "r", "logic_in")],
    [choice("adc_bits", ["8", "10", "12", "14", "16"], "12"), int("adc_sample_rate", "Hz", 1, 1000, 1000),
      choice("can_mode", ["can", "hi_z"], "can"), num("r_out_5v", OHM, 0.001, 10, 0.1), num("r_out_3v3", OHM, 0.001, 10, 0.05)],
    [stateBool("usb", true)]),
  def("can_transceiver", "U", rect(-6, -7, 6, 7),
    [pin("txd", "TXD", [-7, -4], "l", "logic_in"), pin("vss", "VSS", [0, 8], "d", "ground_pin", true),
      pin("vdd", "VDD", [0, -8], "u", "power_in", true), pin("rxd", "RXD", [-7, -2], "l", "logic_out"),
      pin("vref", "VREF", [7, 4], "r", "source"), pin("canl", "CANL", [7, 2], "r", "canl"),
      pin("canh", "CANH", [7, -2], "r", "canh"), pin("rs", "Rs", [-7, 2], "l", "analog_in")],
    [num("r_in", OHM, 1e4, 5e4, 2.5e4)]),
  def("can_bus", "BUS", rect(-3, -3, 9, 5),
    [pin("canh", "CANH", [-4, -2], "l", "canh"), pin("canl", "CANL", [-4, 2], "l", "canl"),
      pin("gnd", "GND", [-4, 4], "l", "ground_pin", true)],
    [int("remote_nodes", "", 0, 30, 1), num("remote_r_in", OHM, 1e4, 5e4, 2.5e4), bool("remote_termination", true)]),
  def("termination", "RT", rect(-2, -1, 2, 1), TWO(),
    [num("resistance", OHM, 120, 120, 120, false), num("power_rating", "W", 0.25, 0.25, 0.25, false)]),
  // Hidden parameters never reach the UI: the black box and the sensor arrive without any parameter.
  def("ecu_apps", "U", rect(-7, -10, 7, 10),
    [pin("lv_in", "LV_IN", [8, -8], "r", "power_in", true), pin("gnd", "GND", [8, -6], "r", "ground_pin", true),
      pin("v5_1", "5V_1", [-8, -9], "l", "source"), pin("sig_1", "SIG_1", [-8, -5], "l", "analog_in"),
      pin("gnd_s1", "GND_S1", [-8, -3], "l", "ground_pin"), pin("v5_2", "5V_2", [-8, 2], "l", "source"),
      pin("sig_2", "SIG_2", [-8, 6], "l", "analog_in"), pin("gnd_s2", "GND_S2", [-8, 8], "l", "ground_pin"),
      pin("can_h", "CAN_H", [8, 3], "r", "canh"), pin("can_l", "CAN_L", [8, 7], "r", "canl")],
    []),
  def("mlx90377", "U", rect(-6, -10, 6, 10),
    [pin("vdd1", "VDD1", [7, -9], "r", "power_in"), pin("vss1", "VSS1", [7, -3], "r", "ground_pin"),
      pin("out1", "OUT1", [7, -5], "r", "source"), pin("vdd2", "VDD2", [7, 2], "r", "power_in"),
      pin("vss2", "VSS2", [7, 8], "r", "ground_pin"), pin("out2", "OUT2", [7, 6], "r", "source")],
    [],
    [{ name: "position", label_fr: "position", unit: "", vtype: "number", min: 0, max: 1, default: 0, persistent: true }],
    [act("drag_pot", rect(-2, -1, 2, 1))]),
  def("junction", "", rect(0, 0, 0, 0), [pin("j", "J", [0, 0], "r", "passive")], [], [], [], false),
];

export const libDef = (kind: Kind): KindDef => {
  const d = LIBRARY.find((k) => k.kind === kind);
  if (!d) throw new Error(kind);
  return d;
};

/** Component with the catalog defaults, overridden by `params`. */
export function comp(id: CompId, kind: Kind, pos: Pt, rot: Rot = 0, label = "", params: ValueMap = {}): Component {
  const p: ValueMap = {};
  for (const d of libDef(kind).params) p[d.name] = d.default;
  return { id, kind, label, pos, rot, params: { ...p, ...params } };
}

export const wire = (id: WireId, a: [CompId, string], b: [CompId, string], bends: Pt[] = []): Wire =>
  ({ id, a: { comp: a[0], pin: a[1] }, b: { comp: b[0], pin: b[1] }, bends });

export function defaultStates(c: Component): ValueMap {
  const st: ValueMap = {};
  for (const s of libDef(c.kind).states) st[s.name] = s.default;
  return st;
}

export function makeState(components: Component[], wires: Wire[], states: [CompId, ValueMap][] = []): LabState {
  const s = new LabState();
  s.applyLibrary(LIBRARY);
  const all = new Map<CompId, ValueMap>(components.map((c) => [c.id, defaultStates(c)]));
  for (const [id, st] of states) all.set(id, { ...all.get(id), ...st });
  s.applySnapshot({
    doc_id: 1, rev: 1, name: "test", dirty: false, components, wires, states: [...all], nets: [], can_undo: false, can_redo: false,
    run: "stop", sim_time_ns: 0, disclosure: "exploration",
  });
  return s;
}

/** 400 11px IBM Plex Mono: every glyph advances 0.6 em. */
export const monoMeasure = (text: string): number => text.length * 6.6;

export function makeModel(state: LabState): SceneModel {
  const m = new SceneModel((k) => state.library.get(k), monoMeasure);
  m.rebuild(state.components.values(), state.wires.values());
  return m;
}

const EMPTY: Patch = { components: [], wires: [] };

function reply(): Outcome<EditReply> {
  return { ok: true, reply: { rev: 2, patch: EMPTY, nets: null, can_undo: true, can_redo: false, dirty: true } };
}

/** Records what the editor sends; previews are answered by the test. */
export class FakeController implements EditorController {
  readonly edits: Edit[] = [];
  readonly interactions: Interact[] = [];
  readonly previews: Edit[] = [];
  cancels = 0;
  undos = 0;
  redos = 0;
  private sink: ((r: PreviewResult, edit: Edit) => void) | null = null;

  edit(edit: Edit): Promise<Outcome<EditReply>> {
    this.edits.push(edit);
    return Promise.resolve(reply());
  }

  undo(): Promise<Outcome<EditReply>> {
    this.undos++;
    return Promise.resolve(reply());
  }

  redo(): Promise<Outcome<EditReply>> {
    this.redos++;
    return Promise.resolve(reply());
  }

  requestPreview(edit: Edit, sink: (r: PreviewResult, edit: Edit) => void): void {
    this.previews.push(edit);
    this.sink = sink;
  }

  cancelPreview(): void {
    this.cancels++;
    this.sink = null;
  }

  interact(interact: Interact): void {
    this.interactions.push(interact);
  }

  get lastPreview(): Edit | undefined {
    return this.previews[this.previews.length - 1];
  }

  /** Answers the newest preview request, as the core would: a patch, or a refusal. */
  answer(result: { patch: Patch } | { refusal: Refusal }): void {
    const edit = this.lastPreview;
    if (!this.sink || !edit) throw new Error("no preview in flight");
    this.sink("refusal" in result ? { ok: false, refusal: result.refusal } : { ok: true, reply: { base_rev: 1, patch: result.patch } }, edit);
  }
}

export function refusal(message_fr: string, at: Pt | null = null): Refusal {
  return { code: "ambiguous", ambiguity: "wire_through_body", message_fr, at, items: [] };
}

/** Environment at zoom 1 (10 CSS px per unit): pointing tolerance 0.5 unit, drag threshold 0.3 unit. */
export function makeEnv(unitsPerPx = 0.1): GestureEnv & { fits: number; zooms: number[]; changes: number } {
  const env = {
    fits: 0,
    zooms: [] as number[],
    changes: 0,
    unitsPerPx: () => unitsPerPx,
    fit: () => {
      env.fits++;
    },
    zoomTo: (r: number) => {
      env.zooms.push(r);
    },
    changed: () => {
      env.changes++;
    },
  };
  return env;
}
