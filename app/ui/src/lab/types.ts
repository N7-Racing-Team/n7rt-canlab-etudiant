// Mirror of the Physical Lab JSON contract: crates/n7rt-circuit/src/lib.rs (document, edits, patches) and
// crates/n7rt-lab/src/phys/mod.rs (values, commands). Keep both in step (docs/physical-lab-v1.md §8.3).
// The UI holds a read-only mirror of the document, updated only from patches and snapshots: it never decides
// connectivity, geometry validity or electrical values.

// ---------------------------------------------------------------------------------------------- document

export type CompId = number;
export type WireId = number;
/** Grid point [x, y] in grid units (1 unit = 10 CSS px at zoom 1), y down. */
export type Pt = [number, number];
export type Rot = 0 | 90 | 180 | 270;
export type Dir = "l" | "r" | "u" | "d";

export interface PinRef {
  comp: CompId;
  pin: string;
}

/** Inclusive rectangle in grid units. */
export interface Rect {
  x0: number;
  y0: number;
  x1: number;
  y1: number;
}

export type Kind =
  | "dc_supply" | "ground" | "resistor" | "capacitor" | "potentiometer" | "push_button" | "switch"
  | "adc_input" | "ecu_stm32" | "can_transceiver" | "can_bus" | "termination" | "ecu_apps" | "mlx90377" | "junction"
  | "ats682lsh" | "ecu_wsf" | "ecu_sas" | "ecu_bps" | "tle5014" | "pss260" | "pressure_switch"
  | "front_axle" | "steering_column" | "brake_hydraulics";

export type Role =
  | "passive" | "source" | "ground" | "power_in" | "ground_pin" | "analog_in" | "logic_in" | "logic_out"
  | "canh" | "canl";

export type ValueType = "number" | "number_or_inf" | "int" | "bool" | "choice";
/**
 * Parameter or state value (Rust untagged enum: integer and float are both numbers here). +∞ arrives as the
 * string "inf" (number_or_inf parameters only). A Number parameter may be sent as text: the core parses it.
 */
export type Value = number | boolean | string;
export type ValueMap = Record<string, Value>;

export interface PinDef {
  id: string;
  name: string;
  short: string;
  pos: Pt;
  dir: Dir;
  role: Role;
  required: boolean;
}

export interface ParamDef {
  name: string;
  label_fr: string;
  unit: string;
  vtype: ValueType;
  min: number;
  max: number;
  default: Value;
  choices: string[];
  editable: boolean;
}

export interface StateDef {
  name: string;
  label_fr: string;
  unit: string;
  vtype: ValueType;
  min: number;
  max: number;
  default: Value;
  /** false: transient (held push button), never saved. */
  persistent: boolean;
}

export type ActuatorKind = "toggle_switch" | "press_button" | "drag_pot";

export interface Actuator {
  kind: ActuatorKind;
  zone: Rect;
}

export interface KindDef {
  kind: Kind;
  name_fr: string;
  prefix: string;
  body: Rect;
  pins: PinDef[];
  params: ParamDef[];
  states: StateDef[];
  actuators: Actuator[];
  placeable: boolean;
  sources_fr: string;
}

export interface Component {
  id: CompId;
  kind: Kind;
  label: string;
  pos: Pt;
  rot: Rot;
  params: ValueMap;
}

export interface Wire {
  id: WireId;
  a: PinRef;
  b: PinRef;
  /** Path = [pin a, ...bends, pin b], canonical and orthogonal. */
  bends: Pt[];
}

export interface Net {
  key: PinRef;
  name: string;
  ground: boolean;
  pins: PinRef[];
  wires: WireId[];
}

// ------------------------------------------------------------------------------------------------- edits

export type Endpoint = ({ type: "pin" } & PinRef) | { type: "on_wire"; wire: WireId; at: Pt };
export type End = "a" | "b";

export type Edit =
  | { op: "add_component"; kind: Kind; pos: Pt; rot: Rot; params?: ValueMap; label?: string | null }
  | { op: "delete"; components: CompId[]; wires: WireId[] }
  | { op: "move"; components: CompId[]; wires: WireId[]; delta: Pt }
  | { op: "rotate"; components: CompId[]; wires: WireId[] }
  | { op: "set_param"; comp: CompId; name: string; value: Value }
  | { op: "set_label"; comp: CompId; label: string }
  | { op: "add_wire"; a: Endpoint; b: Endpoint; bends?: Pt[] | null }
  | { op: "move_wire_end"; wire: WireId; end: End; to: Endpoint; bends?: Pt[] | null }
  | { op: "set_bends"; wire: WireId; bends: Pt[] };

export type WorldQuantity = "speed_mps" | "steering_deg" | "angle_deg" | "pressure_bar";

export type Interact =
  | { op: "set_pot"; comp: CompId; position: number }
  | { op: "set_button"; comp: CompId; pressed: boolean }
  | { op: "set_switch"; comp: CompId; closed: boolean }
  | { op: "set_power"; comp: CompId; on: boolean }
  | { op: "set_world"; comp: CompId; quantity: WorldQuantity; value: number };

/** Exact diff: [id, before | null, after | null]; null = absent. */
export interface Patch {
  components: [CompId, Component | null, Component | null][];
  wires: [WireId, Wire | null, Wire | null][];
}

export type ItemRef =
  | { type: "component"; id: CompId }
  | { type: "wire"; id: WireId }
  | ({ type: "pin" } & PinRef);

export type RefusalCode =
  | "unknown_component" | "unknown_wire" | "unknown_pin" | "unknown_kind" | "same_pin" | "same_wire"
  | "duplicate_wire" | "non_orthogonal" | "out_of_bounds" | "no_route" | "param_unknown" | "param_type"
  | "param_parse" | "param_unit" | "param_out_of_range" | "param_locked" | "param_hidden" | "mission_locked" | "bench_locked"
  | "state_unknown" | "source_clock_owned" | "internal_only"
  | "state_out_of_range" | "not_interactive"
  | "label_invalid" | "label_duplicate" | "endpoint_not_on_wire" | "junction_on_endpoint" | "four_way_junction"
  | "ambiguous"
  | "stale_revision" | "nothing_to_undo" | "nothing_to_redo";

export type Ambiguity =
  | "pin_on_pin" | "pin_on_wire" | "wire_over_pin" | "collinear_overlap" | "self_overlap" | "bend_on_wire"
  | "wire_through_body";

export interface Refusal {
  code: RefusalCode;
  ambiguity: Ambiguity | null;
  /** Shown as is (French). */
  message_fr: string;
  at: Pt | null;
  items: ItemRef[];
}

export interface Violation {
  code: string;
  message_fr: string;
  items: ItemRef[];
  at: Pt | null;
}

// ------------------------------------------------------------------------------------------------ values

/** Never a fictitious number: invalid states are variants. */
export type Reading<T> =
  | { k: "valid"; v: T }
  | { k: "saturated"; v: T }
  | { k: "floating" }
  | { k: "no_reference" }
  | { k: "undefined" }
  /** No resistive path (unpowered resistance). */
  | { k: "open" }
  | { k: "unavailable"; reason: string };

export interface AdcSample {
  volts: number;
  code: number;
  bits: number;
  vref: number;
  sampled_at_ns: number;
}

export type SupplyOutput = "off" | "cv" | "cc" | "rev";
export type RsMode = "high_speed" | "slope_control" | "standby" | "undefined" | "floating";
export type TxdState = "recessive" | "dominant_requested" | "dominant_timeout";

export type OpState =
  | { kind: "supply"; output: SupplyOutput }
  | { kind: "contact"; closed: boolean }
  | { kind: "pot"; position: number }
  | { kind: "ecu"; powered: boolean }
  | { kind: "transceiver"; powered: boolean; rs: RsMode; txd: TxdState }
  | { kind: "passive" };

export type Severity = "error" | "warning" | "info";

export interface Diagnostic {
  code: string;
  severity: Severity;
  message_fr: string;
  items: ItemRef[];
}

export type RunState = "run" | "stop";
export type RunOp = "run" | "stop" | "reset";
export type Disclosure = "exploration" | "mission";

export interface NetValue {
  key: PinRef;
  name: string;
  voltage: Reading<number>;
}

export interface PinValue {
  pin: string;
  voltage: Reading<number>;
  current: Reading<number>;
}

export interface Measure {
  name: string;
  label_fr: string;
  unit: string;
  reading: Reading<number>;
}

export interface CompValues {
  id: CompId;
  state: ValueMap;
  op: OpState;
  pins: PinValue[];
  measures: Measure[];
}

export interface EngineStats {
  lag_frames: number;
  slow: boolean;
  frame_us_p95: number;
  substeps_last: number;
  min_dt_ns: number;
  islands: number;
  restarts: number;
}

export interface ValuesDto {
  seq: number;
  /** Identity of the core's document (SnapshotDto.doc_id): a newer one means the document was replaced out of band. */
  doc_id: number;
  rev: number;
  sim_time_ns: number;
  run: RunState;
  preview: boolean;
  disclosure: Disclosure;
  nets: NetValue[];
  comps: CompValues[];
  diagnostics: Diagnostic[];
  /** The bench multimeter, in both modes. */
  meter: MeterValue;
  engine: EngineStats;
}

export type MeterFn = "v_dc" | "ohm";

export interface MeterValue {
  com: PinRef | null;
  vohm: PinRef | null;
  function: MeterFn;
  /** The display: « 2.734 V », « 68.0 Ω », « OL » or « — ». */
  display: string;
}

// ---------------------------------------------------------------------------------------------- commands

export type PhysCommand =
  | { cmd: "edit"; base_rev: number; edit: Edit }
  | { cmd: "undo" }
  | { cmd: "redo" }
  | { cmd: "interact"; interact: Interact }
  | { cmd: "run" }
  | { cmd: "stop" }
  | { cmd: "reset" }
  | { cmd: "meter"; com: PinRef | null; vohm: PinRef | null; function: MeterFn };

export interface JournalEntry {
  frame: number;
  command: PhysCommand;
}

export interface EditReply {
  rev: number;
  patch: Patch;
  /** Full net list when connectivity changed. */
  nets: Net[] | null;
  can_undo: boolean;
  can_redo: boolean;
  dirty: boolean;
}

export interface PreviewReply {
  base_rev: number;
  patch: Patch;
}

export interface InteractReply {
  rev: number;
  comp: CompId;
  state: ValueMap;
}

export interface RunReply {
  run: RunState;
  sim_time_ns: number;
}

export type CommandReply =
  | ({ type: "edit" } & EditReply)
  | ({ type: "interact" } & InteractReply)
  | ({ type: "run" } & RunReply);

export interface SnapshotDto {
  /** Identity of the document: changes whenever the core replaces it (new, load, demonstration, mission bench). */
  doc_id: number;
  rev: number;
  name: string;
  dirty: boolean;
  components: Component[];
  wires: Wire[];
  states: [CompId, ValueMap][];
  nets: Net[];
  can_undo: boolean;
  can_redo: boolean;
  run: RunState;
  sim_time_ns: number;
  disclosure: Disclosure;
}

export interface FileEntry {
  name: string;
  modified_unix_ms: number;
}

export interface DemoEntry {
  id: string;
  title_fr: string;
  summary_fr: string;
}

export type FileError =
  | { code: "bad_name" }
  | { code: "name_exists" }
  | { code: "not_found" }
  | { code: "invalid"; violations: Violation[] }
  | { code: "io"; message: string }
  | { code: "no_storage" };

/** Rejection value of every phys_* command that can fail. */
export type PhysError =
  | { code: "refused"; refusal: Refusal }
  | { code: "file"; error: FileError }
  | { code: "engine_unavailable" };
