//! Physical Lab engine — contract docs/physical-lab-v1.md §2.1 (models), §3.7 (diagnostics), §3.8 (disclosure),
//! §6 (probe), §8 (engine, commands, values).
//!
//! - Lowering of the [`n7rt_circuit::Circuit`] into an [`n7rt_solver::Network`] (nets and component-internal nodes,
//!   elements with stable keys, gates for integrated circuits).
//! - [`PhysCore`]: the deterministic, clock-free core (commands, frames of 1 ms, journal, replay, digest). Tests
//!   drive it directly.
//! - [`PhysLab`]: the handle used by the Tauri commands; it owns the `phys` thread that paces the core on real time
//!   in RUN (catch-up bounded to 20 frames, then lag counted), with catch_unwind and at most 3 restarts.
//! - [`PhysFrame`]: immutable results of one frame, read through [`CircuitProbe`] by the UI DTOs and, later, by
//!   instruments, the CAN engine and missions. Never a fictitious value: invalid states are [`Reading`] variants.
//!
//! Independent of the N7RT-Link session: RUN / STOP / RESET drive the simulation only. Public items below are
//! frozen for the parallel implementation lots; private modules are free.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

use n7rt_circuit::{
    Circuit, CompId, Component, Document, Edit, Interact, ItemRef, KindDef, Net, NetKey, Patch, PinRef, Refusal,
    ValueMap, Violation, Wire, WireId,
};
use serde::{Deserialize, Serialize};

pub use defects::{SystemOp, WireDefect};
pub use meter::{MeterFn, MeterValue};

mod defects;
mod diag;
mod files;
mod handle;
mod kernel;
mod lower;
mod meter;
mod probe;
mod util;
mod wsf;
mod sas;
mod bps;

/// Circuit builder shared with the integration tests (document API only).
#[cfg(test)]
#[path = "../../tests/phys_bench.rs"]
mod bench;

// ------------------------------------------------------------------------------------------------------ readings

/// A measured quantity. JSON tagged by "k": `{"k":"valid","v":2.5}`, `{"k":"floating"}`,
/// `{"k":"unavailable","reason":"solver_singular"}`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "k", rename_all = "snake_case")]
pub enum Reading<T> {
    Valid { v: T },
    /// ADC input outside [0, Vref): code clamped, sample flagged.
    Saturated { v: T },
    /// Undetermined potential (open input, leakage paths only).
    Floating,
    /// Island without ground: only differences inside its K group are defined.
    NoReference,
    /// Logic input between its thresholds.
    Undefined,
    /// No resistive path (unpowered resistance only): an infinite value never leaves the core as a number.
    Open,
    /// Not computed; stable reason code (`solver_singular`, `mode_unresolved`, `dominant_non_modelise_v1`,
    /// `txd_undefined`, `ecu_unpowered`, `ic_model`, `unknown_pin`, `not_sampled_yet`, `hidden_mission`).
    Unavailable { reason: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct AdcSample {
    /// Sampled input voltage V(in) − V(ref), volts.
    pub volts: f64,
    /// floor(volts · 2^bits / vref), clamped to [0, 2^bits − 1].
    pub code: u32,
    pub bits: u32,
    pub vref: f64,
    pub sampled_at_ns: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LogicLevel {
    Low,
    High,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SupplyOutput {
    Off,
    /// Constant voltage.
    Cv,
    /// Current limited.
    Cc,
    /// Back-driven above its setting: delivers nothing.
    Rev,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RsMode {
    HighSpeed,
    SlopeControl,
    Standby,
    /// Between the datasheet bands (0.3–0.4·VDD or 0.6–0.75·VDD).
    Undefined,
    Floating,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TxdState {
    Recessive,
    /// TXD low: dominant requested, not modelled in V1 (bus readings unavailable).
    DominantRequested,
    /// TXD low for more than the permanent-dominant timer: drivers disabled, bus recessive again.
    DominantTimeout,
}

/// Operating state of a component, as the real part would show it.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OpState {
    Supply { output: SupplyOutput },
    /// Switch and push button: the contact, not the actuator.
    Contact { closed: bool },
    /// Potentiometer wiper, or pedal travel of a position sensor (mlx90377).
    Pot { position: f64 },
    /// ECU board powered (ecu_stm32), or ECU APPS out of reset (« Alimenté » / « En reset »).
    Ecu { powered: bool },
    Transceiver { powered: bool, rs: RsMode, txd: TxdState },
    /// Resistor, capacitor, termination, ADC input, ground, bus.
    Passive,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
    Info,
}

/// An electrical rule result (contract §3.7): a computed state, never a refusal.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Diagnostic {
    /// Stable code from the contract table.
    pub code: String,
    pub severity: Severity,
    pub message_fr: String,
    pub items: Vec<ItemRef>,
}

/// Read access to one frame of results. Pins are the durable addresses (a NetKey can change after an edit).
pub trait CircuitProbe {
    fn sim_time_ns(&self) -> i64;
    /// Circuit revision these results were computed for.
    fn rev(&self) -> u64;
    /// Voltage of a pin relative to the ground net.
    fn voltage_at(&self, pin: &PinRef) -> Reading<f64>;
    /// V(a) − V(b): Valid iff both pins share a K group (solver classification).
    fn voltage_between_pins(&self, a: &PinRef, b: &PinRef) -> Reading<f64>;
    /// Current entering the component through this pin, amperes.
    fn pin_current(&self, pin: &PinRef) -> Reading<f64>;
    /// Power absorbed by the component, Σ V·I_in, watts (negative when it delivers power). Unavailable(`ic_model`)
    /// for ecu_stm32, can_transceiver, can_bus, ecu_apps and mlx90377: their controlled sources do not draw from
    /// their supply.
    fn power(&self, comp: CompId) -> Reading<f64>;
    /// Last sample of an ADC channel ("in" for an ADC input, "a0" / "a1" for the ECU, "sig1" / "sig2" (midpoints of
    /// the SIG dividers) and "sup1" / "sup2" (midpoints of the rail measurement dividers) for an ECU APPS, 12 bits,
    /// 3.3 V, every frame; Unavailable(`ecu_unpowered`) while the ECU APPS is in reset).
    fn adc(&self, comp: CompId, channel: &str) -> Reading<AdcSample>;
    /// Logic level of an input or output pin against its component's thresholds and local ground.
    fn logic(&self, pin: &PinRef) -> Reading<LogicLevel>;
    fn op_state(&self, comp: CompId) -> Option<OpState>;
    fn net_of(&self, pin: &PinRef) -> Option<NetKey>;
    fn diagnostics(&self) -> &[Diagnostic];
    /// Resistance between two pins of the circuit switched off (n7rt_solver::unpowered_resistance): Valid, Open
    /// when no resistive path. Tests and exploration only: Unavailable(`hidden_mission`) in mission mode.
    fn unpowered_resistance(&self, a: &PinRef, b: &PinRef) -> Reading<f64>;
}

/// Immutable results of one frame (or of a STOP preview), shared as `Arc<PhysFrame>`.
#[derive(Debug)]
pub struct PhysFrame {
    inner: probe::Frame,
}

impl PhysFrame {
    /// Actual mechanical quantity of this immutable frame, rather than its interaction target.
    /// Currently `angle_deg` is available for steering columns; other quantities return `None`.
    pub fn world_quantity(&self,comp:CompId,quantity:&str)->Option<f64> {
        if quantity!="angle_deg" {return None;}
        self.inner.cap.mechanical.get(&comp).copied().filter(|value|value.is_finite())
    }
    /// Electrical inputs and completed physical sensor transports from this one immutable frame.
    /// No reference-consumer value or judgment crosses this interface.
    pub fn mission_source_inputs(&self, ecu: CompId) -> Option<n7rt_vehicle::EcuInputs> {
        self.inner.cap.mission_inputs.get(&ecu).cloned()
    }
    /// At most 256 physical frame samples, in simulated ns. Missing time is observable to the caller;
    /// events are not concatenated into a fictitious newer sample.
    pub fn mission_source_history(&self,ecu:CompId)->Vec<(u64,n7rt_vehicle::EcuInputs)> {
        self.inner.cap.mission_history.get(&ecu).map(|samples|samples.iter().map(|(t,input)|(*t,(**input).clone())).collect()).unwrap_or_default()
    }
}

impl CircuitProbe for PhysFrame {
    fn sim_time_ns(&self) -> i64 {
        self.inner.cap.res.time_ns
    }
    fn rev(&self) -> u64 {
        self.inner.rev
    }
    fn voltage_at(&self, pin: &PinRef) -> Reading<f64> {
        self.inner.probe().voltage_at(pin)
    }
    fn voltage_between_pins(&self, a: &PinRef, b: &PinRef) -> Reading<f64> {
        self.inner.probe().between(a, b)
    }
    fn pin_current(&self, pin: &PinRef) -> Reading<f64> {
        self.inner.probe().pin_current(pin)
    }
    fn power(&self, comp: CompId) -> Reading<f64> {
        self.inner.probe().power(comp)
    }
    fn adc(&self, comp: CompId, channel: &str) -> Reading<AdcSample> {
        self.inner.adc(comp, channel)
    }
    fn logic(&self, pin: &PinRef) -> Reading<LogicLevel> {
        self.inner.probe().logic(pin)
    }
    fn op_state(&self, comp: CompId) -> Option<OpState> {
        self.inner.op_state(comp)
    }
    fn net_of(&self, pin: &PinRef) -> Option<NetKey> {
        self.inner.cap.ctx.netlist.net_of(pin).map(|n| n.key.clone())
    }
    fn diagnostics(&self) -> &[Diagnostic] {
        &self.inner.diagnostics
    }
    fn unpowered_resistance(&self, a: &PinRef, b: &PinRef) -> Reading<f64> {
        self.inner.unpowered_resistance(a, b)
    }
}

// ------------------------------------------------------------------------------------------- commands and DTOs

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    Run,
    Stop,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunOp {
    Run,
    Stop,
    /// t = 0, capacitors discharged, supplies CV, gates off; interaction states kept; stays in its run state.
    Reset,
}

/// Contract §3.8, architecture §9.3. Mission mode withholds, in the core, every omniscient value: net voltages, pin values,
/// measures and diagnostics; only interaction states and operating states visible on the real parts remain.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Disclosure {
    Exploration,
    Mission,
}

/// A command of the core, as journaled. JSON tagged by "cmd".
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum PhysCommand {
    Edit { base_rev: u64, edit: Edit },
    Undo,
    Redo,
    Interact { interact: Interact },
    Run,
    Stop,
    Reset,
    /// Mission engine only ([`PhysLab::command_internal`]); refused from the interface.
    System { op: SystemOp },
    /// Places the multimeter probes and picks its function (no document change, no undo).
    Meter { com: Option<PinRef>, vohm: Option<PinRef>, function: MeterFn },
}

/// Journal line: the command and the frame index at which it took effect (applied before that frame is computed).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct JournalEntry {
    pub frame: u64,
    pub command: PhysCommand,
}

/// Integrity of the live session's spilled prefix plus its in-memory tail. A disk prefix alone is never
/// a finalized complete archive, even when this status reports no lost entries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JournalStatus {
    pub retained_entries: usize,
    pub spilled_entries: u64,
    pub lost_entries: u64,
    pub archive_corrupt: bool,
    pub lossless: bool,
}

/// Reply of an edit, undo or redo: the model is committed before the reply; the electrical effect is computed at
/// the next frame boundary (RUN) or immediately as a preview (STOP).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct EditReply {
    pub rev: u64,
    pub patch: Patch,
    /// Full net list, present when connectivity changed.
    pub nets: Option<Vec<Net>>,
    pub can_undo: bool,
    pub can_redo: bool,
    pub dirty: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PreviewReply {
    pub base_rev: u64,
    pub patch: Patch,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct InteractReply {
    pub rev: u64,
    pub comp: CompId,
    pub state: ValueMap,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RunReply {
    pub run: RunState,
    pub sim_time_ns: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CommandReply {
    Edit(EditReply),
    Interact(InteractReply),
    Run(RunReply),
}

/// Whole document, for the first display and after `stale_revision`. Components carry no hidden parameter.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SnapshotDto {
    /// Identity of the document: changes whenever the core replaces it (new, load, demonstration, mission bench,
    /// restart after a panic). Also in [`ValuesDto`], so that the UI reloads a document loaded out of band.
    pub doc_id: u64,
    pub rev: u64,
    pub name: String,
    pub dirty: bool,
    pub components: Vec<Component>,
    pub wires: Vec<Wire>,
    /// Interaction states of the existing components.
    pub states: Vec<(CompId, ValueMap)>,
    pub nets: Vec<Net>,
    pub can_undo: bool,
    pub can_redo: bool,
    pub run: RunState,
    pub sim_time_ns: i64,
    pub disclosure: Disclosure,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct NetValue {
    pub key: NetKey,
    pub name: String,
    pub voltage: Reading<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PinValue {
    pub pin: String,
    pub voltage: Reading<f64>,
    pub current: Reading<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Measure {
    /// Stable name ("power", "code", "v_c", "i_out", …).
    pub name: String,
    pub label_fr: String,
    pub unit: String,
    pub reading: Reading<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CompValues {
    pub id: CompId,
    pub state: ValueMap,
    pub op: OpState,
    /// Empty in mission mode.
    pub pins: Vec<PinValue>,
    /// Empty in mission mode.
    pub measures: Vec<Measure>,
}

#[derive(Clone, Debug, PartialEq, Default, Serialize)]
pub struct EngineStats {
    /// Frames behind real time at the last tick (0 when keeping up).
    pub lag_frames: u64,
    /// The "ralenti" indicator (architecture §14): the engine could not keep real time during the last second.
    pub slow: bool,
    /// Compute time of one frame, p95 over the last second, microseconds.
    pub frame_us_p95: f64,
    pub substeps_last: i64,
    pub min_dt_ns: i64,
    pub islands: usize,
    /// Restarts after a panic of the core (at most 3, then every phys command returns engine_unavailable).
    pub restarts: u32,
}

/// Values pulled by the UI (`phys_values`), published at most every 50 ms and after each command.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ValuesDto {
    /// Increases at every publication.
    pub seq: u64,
    /// Identity of the current document ([`SnapshotDto::doc_id`]): a value the UI has not seen in a snapshot means
    /// the core replaced the document (mission bench loaded by the mission controller): reload the snapshot.
    pub doc_id: u64,
    pub rev: u64,
    pub sim_time_ns: i64,
    pub run: RunState,
    pub preview: bool,
    pub disclosure: Disclosure,
    /// Empty in mission mode.
    pub nets: Vec<NetValue>,
    pub comps: Vec<CompValues>,
    /// Empty in mission mode.
    pub diagnostics: Vec<Diagnostic>,
    /// The multimeter, in both modes.
    pub meter: MeterValue,
    pub engine: EngineStats,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FileEntry {
    pub name: String,
    pub modified_unix_ms: u64,
}

/// A visible demonstration (n7rt_circuit::demos without the hidden ones and without the text).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DemoEntry {
    pub id: String,
    pub title_fr: String,
    pub summary_fr: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum FileError {
    /// Name outside `[A-Za-z0-9][A-Za-z0-9_-]{0,63}`, or a reserved Windows name (CON, NUL, COM1…).
    BadName,
    /// A file with this name (case-insensitive) exists and overwrite was not requested.
    NameExists,
    NotFound,
    /// The file breaks invariants: nothing loaded.
    Invalid { violations: Vec<Violation> },
    Io { message: String },
    /// No data directory configured.
    NoStorage,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum PhysError {
    Refused { refusal: Refusal },
    File { error: FileError },
    /// The core panicked more than 3 times.
    EngineUnavailable,
}

// ---------------------------------------------------------------------------------------------------- the core

/// Deterministic core without clock or thread: same initial circuit, same commands at the same frames ⇒ same
/// [`PhysCore::digest`] (contract §8.2, architecture I3).
#[derive(Debug)]
pub struct PhysCore {
    inner: kernel::Core,
}

impl PhysCore {
    pub fn new(circuit: Circuit) -> Self {
        PhysCore { inner: kernel::Core::new(circuit) }
    }

    pub fn configure_source(&mut self, ecu: CompId, pack: &n7rt_vehicle::Pack) -> Result<(), String> {
        self.inner.configure_source(ecu, pack)
    }
    pub fn set_sensor_seed(&mut self,seed:u64) {self.inner.set_sensor_seed(seed);}

    pub fn document(&self) -> &Document {
        self.inner.document()
    }

    pub fn run_state(&self) -> RunState {
        self.inner.run_state()
    }

    /// Frames computed since the last RESET or load.
    pub fn frame_index(&self) -> u64 {
        self.inner.frame_index()
    }

    /// Applies a command and journals it with the current frame index. In STOP the results are recomputed at once
    /// (preview); in RUN the electrical effect waits for the next [`PhysCore::step`].
    pub fn apply(&mut self, command: &PhysCommand) -> Result<CommandReply, Refusal> {
        self.inner.apply(command)
    }

    /// Dry run of an edit on the committed document.
    pub fn preview(&self, base_rev: u64, edit: &Edit) -> Result<PreviewReply, Refusal> {
        self.inner.preview(base_rev, edit)
    }

    /// One frame of 1 ms in RUN (nothing in STOP).
    pub fn step(&mut self) {
        self.inner.step()
    }

    pub fn frame(&self) -> Arc<PhysFrame> {
        self.inner.frame()
    }

    pub fn values(&self, seq: u64, disclosure: Disclosure, engine: EngineStats) -> ValuesDto {
        self.inner.values(seq, disclosure, engine)
    }

    pub fn journal(&self) -> &[JournalEntry] {
        self.inner.journal()
    }

    /// Wire objects that carry or carried a hidden defect (existing or not), and whether one waits for a wire.
    pub fn defects(&self) -> (BTreeSet<WireId>, bool) {
        self.inner.defects()
    }

    /// FNV-1a 64 of the canonical circuit text, the interaction states, the run state, the frame index and the
    /// solver state digest.
    pub fn digest(&self) -> u64 {
        self.inner.digest()
    }
}

/// Replays a journal from `initial` up to `frames` frames; equal digest to the live core (property test I3).
pub fn replay(initial: &Circuit, journal: &[JournalEntry], frames: u64) -> PhysCore {
    PhysCore { inner: kernel::replay(initial, journal, frames) }
}

// -------------------------------------------------------------------------------------------------- the handle

#[derive(Clone, Debug, Default)]
pub struct PhysConfig {
    /// Application data directory: `circuits/<name>.json` and `sessions/<start>/journal.ndjson`. None: no storage
    /// (tests).
    pub data_dir: Option<PathBuf>,
}

/// Handle used by the Tauri commands (src-tauri/src/phys.rs). Every method is synchronous, returns after the
/// model commit and never waits for more than one frame computation.
pub struct PhysLab {
    inner: handle::Lab,
}

impl PhysLab {
    /// Starts the `phys` thread: STOP, empty circuit "sans titre", exploration mode.
    pub fn start(cfg: PhysConfig) -> PhysLab {
        PhysLab { inner: handle::Lab::start(cfg) }
    }

    pub fn configure_source(&self, ecu: CompId, pack: &n7rt_vehicle::Pack) -> Result<(), String> {
        self.inner.configure_source(ecu, pack)
    }
    pub fn set_sensor_seed(&self,seed:u64) {self.inner.set_sensor_seed(seed);}

    /// Exclusive simulated clock for a mission bus session; starts with a STOP/RESET at t=0.
    pub fn acquire_source_clock(&self) -> Result<u64, String> { self.inner.acquire_source_clock() }
    /// Samples every intervening 1 ms frame, refusing stale tokens, backwards time and gaps over 20 ms.
    pub fn advance_source_clock(&self, token:u64, ms:u64) -> Result<Arc<PhysFrame>, String> {
        self.inner.advance_source_clock(token,ms)
    }
    /// Resumes ordinary physical pacing from now, only for the current owner.
    pub fn release_source_clock(&self, token:u64) -> Result<(), String> { self.inner.release_source_clock(token) }

    pub fn library(&self) -> &'static [KindDef] {
        n7rt_circuit::library()
    }

    pub fn snapshot(&self) -> SnapshotDto {
        self.inner.snapshot()
    }

    pub fn preview(&self, base_rev: u64, edit: &Edit) -> Result<PreviewReply, PhysError> {
        self.inner.preview(base_rev, edit)
    }

    /// Command from the UI (Tauri `phys_command`). Refused (`bench_locked`, « Banc en lecture seule pendant
    /// l'essai. ») while the check lock is on, like `preview`, `load` and `load_demo` (`new_circuit` then leaves the
    /// document as it is).
    pub fn command(&self, command: PhysCommand) -> Result<CommandReply, PhysError> {
        self.inner.command(command)
    }

    /// Command of the mission controller (Rust): applied whatever the check lock. Journaled like any command; the
    /// journal of a mission bench is never shown (`journal_tail` is empty while the document is a mission bench or
    /// the disclosure is Mission).
    pub fn command_internal(&self, command: PhysCommand) -> Result<CommandReply, PhysError> {
        self.inner.command_internal(command)
    }

    /// Check lock (mission check, spec ch. 24 §2.9.1 step 3 and §7.5): while on, the bench is read-only for the UI
    /// and the interaction states it is shown (SnapshotDto.states, ValuesDto comps state and op) stay those of the
    /// moment the lock was set, so that the trajectory driven by `command_internal` is never displayed. The probe
    /// ([`PhysLab::probe`]) keeps reading the real circuit.
    pub fn set_check_lock(&self, on: bool) {
        self.inner.set_check_lock(on)
    }

    pub fn check_locked(&self) -> bool {
        self.inner.check_locked()
    }

    /// Hidden wire defects of the live core: the wire objects that carry or carried one, and whether one waits for a
    /// wire (mission controller: physical checks P-3 and P-4). `None` without a document.
    pub fn defects(&self) -> Option<(BTreeSet<WireId>, bool)> {
        self.inner.defects()
    }

    /// The document is a mission bench (another document replaced it otherwise).
    pub fn on_mission_bench(&self) -> bool {
        self.inner.on_mission_bench()
    }

    /// A copy of the live circuit (mission controller: physical checks of a verification); `None` without a document.
    pub fn circuit_copy(&self) -> Option<n7rt_circuit::Circuit> {
        self.inner.circuit_copy()
    }

    /// Values newer than `since_seq`, or None if nothing changed.
    pub fn values(&self, since_seq: u64) -> Option<ValuesDto> {
        self.inner.values(since_seq)
    }

    pub fn probe(&self) -> Arc<PhysFrame> {
        self.inner.probe()
    }

    pub fn set_disclosure(&self, disclosure: Disclosure) {
        self.inner.set_disclosure(disclosure)
    }

    /// Last `n` journal entries, newest last (Journal tab). Empty while the document is a mission bench or the
    /// disclosure is Mission.
    pub fn journal_tail(&self, n: usize) -> Vec<JournalEntry> {
        self.inner.journal_tail(n)
    }

    /// Loss status of this live session's spilled prefix plus RAM tail; no finalized archive is implied.
    /// Refused in mission mode or on a sealed bench.
    pub fn journal_status(&self) -> Result<JournalStatus, PhysError> { self.inner.journal_status() }

    /// Refuses integral export/replay after any lost entry or archive corruption. The disk file is a prefix,
    /// not a finalized full archive; callers would still need the current RAM tail and replay metadata.
    pub fn require_lossless_session(&self) -> Result<(), PhysError> { self.inner.require_lossless_session() }

    /// New, load and load_demo replace a mission bench: Exploration again, saving allowed.
    pub fn new_circuit(&self) -> SnapshotDto {
        self.inner.new_circuit()
    }

    /// Refused (`mission_locked`) while a mission bench is the document or the disclosure is Mission.
    pub fn save(&self, name: &str, overwrite: bool) -> Result<SnapshotDto, PhysError> {
        self.inner.save(name, overwrite)
    }

    pub fn load(&self, name: &str) -> Result<SnapshotDto, PhysError> {
        self.inner.load(name)
    }

    pub fn list(&self) -> Result<Vec<FileEntry>, PhysError> {
        self.inner.list()
    }

    pub fn demos(&self) -> Vec<DemoEntry> {
        self.inner.demos()
    }

    /// Loads an embedded demonstration (n7rt_circuit::demos), hidden ones included.
    pub fn load_demo(&self, id: &str) -> Result<SnapshotDto, PhysError> {
        self.inner.load_demo(id)
    }

    /// Loads a circuit given as `n7rt_circuit::persist` JSON (a mission bench, hidden parameters included) as the
    /// current document named `name`, sets `disclosure`, then RUN (docs/apps-mission-slice.md §5). In mission mode
    /// `save` is refused until another document is loaded or a new circuit is started.
    pub fn load_circuit_json(&self, name: &str, json: &str, disclosure: Disclosure) -> Result<SnapshotDto, PhysError> {
        self.inner.load_circuit_json(name, json, disclosure)
    }

    /// Component of the current document whose designator is `designator` (e.g. "U1").
    pub fn find_designator(&self, designator: &str) -> Option<CompId> {
        self.inner.find_designator(designator)
    }

    pub fn shutdown(&self) {
        self.inner.shutdown()
    }
}
