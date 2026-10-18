//! Synthetic vehicle CAN bus for N7RT CAN Lab missions (contract: docs/apps-mission-slice.md §4).
//!
//! Generic, deterministic code only: frame scheduling (phase, period, counter-based PRNG jitter), signal encoding,
//! AUTOSAR E2E Profile 22 protection, alive counters, background ECUs and the ECU_APPS source model. Every mission
//! value (identifiers, layouts, scales, Data ID lists, transfer functions, status codes, oracle parameters) comes from
//! the private pack loaded at run time ([`Pack::load`]); none is compiled into this crate. Nothing here prints a frame
//! or signal name.
//!
//! The oracle (docs/distribution.md §1) is not part of the public sources: the consumer-side supervision, the E2E
//! receiver checks, the mission consumer logic, the reference consumer, the nominal-check comparison and the
//! judgement of run files live in the instructor repository and are compiled in only when `N7RT_ORACLE_DIR` names
//! them at build time (build.rs, cfg `n7rt_oracle`, [`ORACLE`]). Without them the public stand-ins of `src/public/`
//! are used: the background VCU and inverter read the bus without supervising it, and [`RefConsumer`], [`compare`]
//! and [`judge_run`] answer « oracle absent ».

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;
use std::sync::Arc;

mod codec;
pub mod e2e;
mod ecu_apps;
mod ecu_wsf;
mod ecu_sas;
mod ecu_bps;
mod source_model;
pub use ecu_wsf::{WsfParams, WsfInputs, WsfLaneInputs, WsfEdge};
pub use ecu_sas::{SasConfig, SentConfig, SasInput, SentFrame};
mod nominal;
// The consumer and comparison parameters of a pack are read only by the oracle.
#[cfg_attr(not(n7rt_oracle), allow(dead_code))]
mod pack;
mod prng;
mod run;
mod variant;
mod vehicle;
pub use vehicle::FrameFault;
mod vocab;

#[cfg(n7rt_oracle)]
mod consumer {
    include!(concat!(env!("N7RT_ORACLE_SRC"), "/consumer.rs"));
}
#[cfg(not(n7rt_oracle))]
#[path = "public/consumer.rs"]
mod consumer;

#[cfg(n7rt_oracle)]
mod consumer_wsf { include!(concat!(env!("N7RT_ORACLE_SRC"), "/consumer_wsf.rs")); }
#[cfg(n7rt_oracle)]
mod consumer_sas { include!(concat!(env!("N7RT_ORACLE_SRC"), "/consumer_sas.rs")); }
#[cfg(n7rt_oracle)]
mod consumer_bps { include!(concat!(env!("N7RT_ORACLE_SRC"), "/consumer_bps.rs")); }

#[cfg(n7rt_oracle)]
mod supervision {
    include!(concat!(env!("N7RT_ORACLE_SRC"), "/supervision.rs"));
}
#[cfg(not(n7rt_oracle))]
#[path = "public/supervision.rs"]
mod supervision;

#[cfg(n7rt_oracle)]
mod e2e_check {
    include!(concat!(env!("N7RT_ORACLE_SRC"), "/e2e_check.rs"));
}

#[cfg(n7rt_oracle)]
mod judge {
    include!(concat!(env!("N7RT_ORACLE_SRC"), "/judge.rs"));
}
#[cfg(not(n7rt_oracle))]
#[path = "public/judge.rs"]
mod judge;

pub use judge::{compare, judge_run, RefConsumer, Supervision};

/// True when this build contains the oracle (instructor builds); false for the public build.
pub const ORACLE: bool = cfg!(n7rt_oracle);

/// Edition of this build, shown by the application and `n7rt-tool --version`. The instructor text carries a marker
/// that the secret audit looks for in the student release (it must never be there).
#[cfg(n7rt_oracle)]
pub const EDITION: &str = "édition encadrant [N7RT-ORACLE-BUILD]";
#[cfg(not(n7rt_oracle))]
pub const EDITION: &str = "édition publique";

/// Receiver-side E2E checks of the oracle (P22 check, life counter, state machine), for the instructor's tests.
#[cfg(n7rt_oracle)]
pub mod e2e_receiver {
    pub use crate::e2e_check::{LifeCounterChecker, P22Checker, StateMachine};
}

use pack::{PackData, Protection, Role};

// ------------------------------------------------------------------------------------------------------- pack

/// A mission pack (private, `restricted/missions/<mission>/pack-v0.json`). Opaque outside this crate except for
/// the accessors below.
#[derive(Clone)]
pub struct Pack {
    d: Arc<PackData>,
}

impl fmt::Debug for Pack {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Pack").field("mission", &self.d.mission).field("frames", &self.d.frames.len()).finish()
    }
}

/// Schedule of one frame of the pack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameTiming {
    pub period_us: u64,
    pub phase_us: u64,
    pub dlc: u8,
}

impl Pack {
    /// Reads and validates a pack file. The error text names the failing field by path, never a frame name.
    /// The bench circuit (`bench_file`, relative to the pack) is read when present; when it is absent the pack
    /// still loads and [`Pack::bench_json`] returns an empty string. A bench inlined as text (`bench`, written by
    /// [`Pack::to_json`]) takes precedence.
    pub fn load(path: &Path) -> Result<Pack, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("pack illisible ({})", e.kind()))?;
        let probe: serde_json::Value =
            serde_json::from_str(&text).map_err(|_| "pack : JSON invalide".to_string())?;
        let inline = probe.get("bench").is_some_and(|v| v.is_string());
        let bench_json = match probe.get("bench_file").and_then(|v| v.as_str()) {
            _ if inline => String::new(),
            Some(rel) if !rel.contains("..") => {
                let p = path.parent().unwrap_or(Path::new(".")).join(rel);
                match std::fs::read_to_string(&p) {
                    Ok(s) => s,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
                    Err(e) => return Err(format!("bench_file : fichier illisible ({})", e.kind())),
                }
            }
            _ => String::new(),
        };
        let d = pack::parse(&text, bench_json)?;
        Ok(Pack { d: Arc::new(d) })
    }

    /// Parses a full pack from JSON text (bench inlined as `bench`, or none).
    pub fn from_json(text: &str) -> Result<Pack, String> {
        Ok(Pack { d: Arc::new(pack::parse(text, String::new())?) })
    }

    /// Mission identifier, e.g. "apps".
    pub fn mission(&self) -> &str {
        &self.d.mission
    }

    /// Bench circuit as the canonical JSON of `n7rt_circuit::persist` (hidden parameters included); the caller loads
    /// it into the Physical Lab in mission mode.
    pub fn bench_json(&self) -> &str {
        &self.d.bench_json
    }

    /// Designator of the source ECU component in the bench (e.g. "U1"), and of the sensor (e.g. "U2").
    pub fn bench_ecu(&self) -> &str {
        &self.d.bench_ecu
    }
    pub fn bench_sensor(&self) -> &str {
        &self.d.bench_sensor
    }

    /// Tick period of the student session, ms (10 for the APPS mission).
    pub fn tick_period_ms(&self) -> u16 {
        self.d.tick_period_ms
    }

    /// Half-width of the uniform emission jitter, µs.
    pub fn jitter_us(&self) -> u64 {
        self.d.jitter_us
    }

    /// Identifiers of the matrix, in pack order.
    pub fn frame_ids(&self) -> Vec<u32> {
        self.d.frames.iter().map(|f| f.id).collect()
    }

    /// Period, phase and DLC of a frame.
    pub fn frame_timing(&self, id: u32) -> Option<FrameTiming> {
        let f = &self.d.frames[self.d.frame_index(id)?];
        Some(FrameTiming { period_us: f.period_us, phase_us: f.phase_us, dlc: f.dlc })
    }

    /// Identifier of the frame that carries an engine quantity (see [`Pack::source_vocabulary`]).
    pub fn frame_of_source(&self, source: &str) -> Option<u32> {
        self.d.sources.get(source).map(|&(fi, _)| self.d.frames[fi].id)
    }

    /// Names of the quantities computed by the engine (static background ECUs add their own, defined by the pack).
    pub fn source_vocabulary() -> &'static [&'static str] {
        vocab::all()
    }

    /// Encodes a frame from physical values keyed by source (`None` = SNA), with the given counter (P22 counter or
    /// alive counter; ignored for unprotected frames). Reserved bits are 1. Every value signal must be given.
    pub fn encode_frame(
        &self,
        id: u32,
        counter: u32,
        values: &BTreeMap<String, Option<f64>>,
    ) -> Result<Vec<u8>, String> {
        let fi = self.d.frame_index(id).ok_or_else(|| "identifiant absent de la matrice".to_string())?;
        let f = &self.d.frames[fi];
        let mut data = vec![0xFFu8; f.dlc as usize];
        for (si, s) in f.signals.iter().enumerate() {
            if s.role != Role::Value {
                continue;
            }
            let v = values.get(&s.source).ok_or_else(|| format!("frames[{fi}].signals[{si}] : valeur manquante"))?;
            codec::write_value(s, &mut data, *v);
        }
        match &f.protection {
            Protection::P22 { data_ids } => e2e::p22_protect(&mut data, (counter & 0x0F) as u8, data_ids),
            Protection::Alive { bits, signal } => {
                codec::write_raw(&f.signals[*signal], &mut data, (counter as u64 % (1u64 << bits)) as i64)
            }
            Protection::None => {}
        }
        Ok(data)
    }

    /// Decodes the value signals of a frame (`None` = SNA), keyed by source. No supervision is applied.
    pub fn decode_frame(&self, id: u32, data: &[u8]) -> Result<BTreeMap<String, Option<f64>>, String> {
        let fi = self.d.frame_index(id).ok_or_else(|| "identifiant absent de la matrice".to_string())?;
        let f = &self.d.frames[fi];
        if data.len() < f.dlc as usize {
            return Err("trame plus courte que son DLC".to_string());
        }
        Ok(f.signals
            .iter()
            .filter(|s| s.role == Role::Value)
            .map(|s| (s.source.clone(), codec::read_value(s, data)))
            .collect())
    }

    /// Raw values of every signal of a frame, headers included (`e2e.crc`, `e2e.counter`, `alive`), keyed by source.
    pub fn decode_raw(&self, id: u32, data: &[u8]) -> Result<BTreeMap<String, i64>, String> {
        let fi = self.d.frame_index(id).ok_or_else(|| "identifiant absent de la matrice".to_string())?;
        let f = &self.d.frames[fi];
        if data.len() < f.dlc as usize {
            return Err("trame plus courte que son DLC".to_string());
        }
        Ok(f.signals.iter().map(|s| (s.source.clone(), codec::read_raw(s, data))).collect())
    }

    /// The Data ID list of a frame protected by Profile 22 (sender side, public), `None` otherwise.
    pub fn p22_data_ids(&self, id: u32) -> Option<[u8; 16]> {
        let fi = self.d.frame_index(id)?;
        match &self.d.frames[fi].protection {
            Protection::P22 { data_ids } => Some(*data_ids),
            _ => None,
        }
    }

    /// Number of public slots of the mission (kept by a student pack).
    pub fn slot_count(&self) -> usize {
        self.d.slot_count
    }

    /// Resolution of a slot used by the comparison (½ LSB tolerance). `None` for a student pack.
    pub fn slot_lsb(&self, slot: usize) -> Option<f64> {
        self.d.slots.get(slot).map(|s| s.lsb)
    }

    /// Inputs an ideal sensor and an ideal bench would give the source ECU at a world position 0…1 (nominal LV,
    /// nominal sensor supply, channel ratios on the ECU calibration). For tests and runs without the Physical Lab.
    pub fn ideal_inputs(&self, position: f64) -> EcuInputs {
        let se = &self.d.source_ecu;
        let full = (1u64 << se.adc_bits) as f64;
        let code = |v_pin: f64, divider: f64| -> Option<u32> {
            Some((v_pin / divider * full / se.adc_vref_v).floor().clamp(0.0, full - 1.0) as u32)
        };
        let theta = position.clamp(0.0, 1.0) * 100.0;
        let sup = se.sup_nominal_v;
        let sig = |c: &pack::Channel| (c.a + c.b * theta / 100.0) / 100.0 * sup;
        EcuInputs {
            lv: Some(self.d.power.lv_nominal_v),
            adc: [
                code(sig(&se.channels[0]), se.sig_divider),
                code(sig(&se.channels[1]), se.sig_divider),
                code(sup, se.sup_divider),
                code(sup, se.sup_divider),
            ],
            ..EcuInputs::default()
        }
    }
}

// --------------------------------------------------------------------------------------------------- vehicle

/// What the source ECU samples every 1 ms, read by the caller from the Physical Lab probe. `None` = reading not
/// available (unpowered net, floating input, solver not running): the ECU treats it like a 0 V input.
#[derive(Debug, Clone, Default)]
pub struct EcuInputs {
    /// V(LV_IN) − V(GND) of the source ECU, volts.
    pub lv: Option<f64>,
    /// ADC codes of the ECU channels, in the order of [`Pack::adc_channels`] (APPS: sig1, sig2, sup1, sup2).
    pub adc: [Option<u32>; 4],
    pub contact_v: Option<f64>,
    pub wsf: Option<WsfInputs>,
    pub sas: Option<SasInput>,
}

impl Pack {
    /// Names of the ECU ADC channels the caller must read with `CircuitProbe::adc(ecu, channel)`, in
    /// [`EcuInputs::adc`] order.
    pub fn adc_channels(&self) -> &[String] {
        &self.d.adc_channels
    }
    /// Mechanical quantities commanded by the nominal run, with the declared physical range.
    pub fn world_ranges(&self) -> &BTreeMap<String, [f64; 2]> { &self.d.nominal.world_ranges }
    pub fn sas_config(&self) -> Option<&SasConfig> { self.d.source_ecu.sas.as_ref() }
}

/// One frame on the bus. The vehicle emits classic CAN frames (11-bit identifier, `flags` 0,
/// `data.len() == dlc as usize` ≤ 8); a hidden qualification also carries frames with N7RT-Link flags (EXT 0x01,
/// RTR 0x02, FD 0x04), whose data length follows the flags.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BusFrame {
    /// Emission instant, µs of bus time since the session origin.
    pub t_us: u64,
    pub id: u32,
    pub flags: u8,
    pub dlc: u8,
    pub data: Vec<u8>,
}

/// The whole vehicle: background ECUs, world, the mission's source ECU and the scheduler.
pub struct Vehicle {
    engine: vehicle::Engine,
}

impl Vehicle {
    /// Session origin t = 0, every ECU just out of reset (background ECUs powered; the source ECU follows its LV
    /// input). `seed` drives the jitter PRNG (counter-based, architecture §10.4).
    pub fn new(pack: &Pack, seed: u64) -> Vehicle {
        Vehicle { engine: vehicle::Engine::new(pack.d.clone(), seed) }
    }

    /// Advances bus time in 1 ms steps up to `t_us` (inclusive), calling `inputs(t_ms)` once per ms for the source
    /// ECU, and returns the frames emitted in (previous t, t_us], sorted by `t_us` then identifier. Idempotent for a
    /// `t_us` not greater than the current time (returns nothing).
    pub fn advance(&mut self, t_us: u64, inputs: &mut dyn FnMut(u64) -> EcuInputs) -> Vec<BusFrame> {
        self.engine.advance(t_us, inputs)
    }

    /// Current bus time, µs.
    pub fn time_us(&self) -> u64 {
        self.engine.now_us()
    }

    /// Swaps the source ECU's static configuration for that of `pack` (a [`Pack::with_source_overrides`] of the same
    /// pack), keeping every dynamic state: the configuration incidents of a hidden sequence switch on and off.
    pub fn reconfigure_source(&mut self, pack: &Pack) -> Result<(), String> {
        self.engine.reconfigure_source(pack.d.clone())
    }

    /// Installs or removes a payload corruption after protection ([`FrameFault`]); the emission rank restarts at 0.
    pub fn set_frame_fault(&mut self, fault: Option<FrameFault>) -> Result<(), String> {
        self.engine.set_frame_fault(fault)
    }
}

// ----------------------------------------------------------------------------------------- reference consumer

/// Output of one slot at one tick: a published value or a reject (`n7rt_reject`), as the board reports it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SlotOut {
    Value(f64),
    Reject,
}

/// Slot outputs of one tick (index = slot number 0…7; `None` = the slot is not written at that tick).
#[derive(Debug, Clone, PartialEq)]
pub struct TickOut {
    pub t_us: u64,
    pub slots: Vec<Option<SlotOut>>,
}

// --------------------------------------------------------------------------------------------- nominal check

/// Hidden nominal test (ch. 24 §2.9.6): cold start, then nominal sweeps of the world quantity drawn from `seed`.
pub struct NominalScenario {
    s: nominal::Scenario,
}

impl NominalScenario {
    pub fn new(pack: &Pack, seed: u64) -> NominalScenario {
        NominalScenario { s: nominal::Scenario::new(&pack.d, seed) }
    }

    /// Total duration, µs of bus time.
    pub fn duration_us(&self) -> u64 {
        self.s.duration_us
    }

    /// World quantity imposed at `t_us` (APPS: pedal travel as the sensor position 0…1, quantized to 1e-3).
    pub fn position_at(&self, t_us: u64) -> f64 {
        self.s.position_at(t_us)
    }
}

/// What the board answered during a check, per tick (from `TICK_RESULT`, slot → value or reject).
#[derive(Debug, Clone, PartialEq)]
pub struct BoardTick {
    pub t_us: u64,
    pub slots: Vec<Option<SlotOut>>,
}

/// Verdict of a check. `detail` is private (trainer report only): never shown to the student.
#[derive(Debug, Clone, PartialEq)]
pub struct Verdict {
    pub pass: bool,
    pub detail: String,
}

// ------------------------------------------------------------------------------------------- distribution

/// docs/distribution.md §3.1: group variant, student projection, full JSON.
impl Pack {
    /// Group variant of ch. 01 §2.8 drawn deterministically from a 32-byte seed (phases, signal positions in the free
    /// bits, byte order, factor and offset from plausible sets, status-code permutations, the Data ID bytes drawn
    /// without replacement, jitter seed); identifiers, periods, DLC and protection kinds unchanged. Same seed ⇒ same
    /// variant. The result passes the validation of [`Pack::load`] and the variant bounds (module `variant`); the
    /// ECU encoders, the background ECUs and the reference consumer all read it, so it works end to end. Err on a
    /// student pack.
    pub fn variant(&self, seed: &[u8; 32]) -> Result<Pack, String> {
        Ok(Pack { d: Arc::new(variant::variant(&self.d, seed)?) })
    }

    /// Full pack as pretty JSON, bench inlined as text (`bench`) when known; `Pack::from_json` reads it back to the
    /// same pack (same `to_json`). On a student pack: its student JSON (the oracle is not there to write).
    pub fn to_json(&self) -> String {
        if self.d.student {
            return self.student_json();
        }
        let mut v = self.d.raw.clone();
        if let Some(o) = v.as_object_mut() {
            if self.d.bench_json.is_empty() {
                o.remove("bench");
            } else {
                o.insert("bench".into(), serde_json::Value::String(self.d.bench_json.clone()));
            }
        }
        serde_json::to_string_pretty(&v).unwrap_or_default()
    }

    /// Student pack JSON (`format` "n7rt-student-pack"): what the simulation needs — matrix (identifiers, periods,
    /// phases, DLC, protections and Data ID lists, signals without names or units), source ECU, background ECUs and
    /// world (the background VCU's supervision parameters under `background.vcu`), bench text, adc channels and
    /// designators, tick period, slot count, nominal scenario, jitter seed — and nothing of the oracle: no slot
    /// outputs or LSB, no comparison rules, no provenance, no goldens.
    pub fn student_json(&self) -> String {
        serde_json::to_string(&pack::student_document(&self.d)).unwrap_or_default()
    }

    /// Reads a student pack. `RefConsumer::try_new` and `judge_run` refuse it with an error, `compare` fails with a
    /// verdict that says so, `RefConsumer::new` panics.
    pub fn from_student_json(text: &str) -> Result<Pack, String> {
        Ok(Pack { d: Arc::new(pack::parse_student(text)?) })
    }

    /// True for a pack read with `from_student_json`.
    pub fn is_student(&self) -> bool {
        self.d.student
    }

    /// The same pack with source ECU parameters replaced (`set`: dotted paths under `source_ecu` and their values),
    /// validated like a loaded pack; a student pack stays a student pack. No other behaviour than the source ECU's
    /// own. Refuses an unknown path, a value of another JSON type, or a configuration the validation rejects.
    pub fn with_source_overrides(&self, set: &[(String, serde_json::Value)]) -> Result<Pack, String> {
        let mut doc = self.d.raw.clone();
        for (path, value) in set {
            let unknown = || format!("surcharge de l'ECU source : {path} inconnu");
            let mut at = doc.get_mut("source_ecu").ok_or_else(unknown)?;
            for key in path.split('.') {
                at = at.get_mut(key).ok_or_else(unknown)?;
            }
            let same_kind = (at.is_number() && value.is_number()) || (at.is_boolean() && value.is_boolean())
                || (at.is_string() && value.is_string());
            if !same_kind {
                return Err(format!("surcharge de l'ECU source : {path} d'un autre type"));
            }
            *at = value.clone();
        }
        let text = serde_json::to_string(&doc).map_err(|_| "surcharge de l'ECU source : JSON invalide".to_string())?;
        let d = if self.d.student { pack::parse_student(&text)? } else { pack::parse(&text, self.d.bench_json.clone())? };
        Ok(Pack { d: Arc::new(d) })
    }
}

/// A frame as delivered to the board during a recorded run.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RunFrame {
    pub t_us: u64,
    pub id: u32,
    pub flags: u8,
    pub data: Vec<u8>,
}

/// One board tick of a recorded run: slot number → value or reject (None = not written).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RunTick {
    pub t_us: u64,
    /// `null` = not written, `{"v": x}` = published value, `"reject"` = n7rt_reject.
    pub slots: Vec<Option<RunSlot>>,
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunSlot {
    V(f64),
    Reject,
}

/// Recorded nominal run (docs/distribution.md §3.2), written by the student app, judged by the instructor.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RunRecord {
    pub mission: String,
    pub cohort: String,
    pub group: String,
    pub pack_id: String,
    pub app_version: String,
    pub board_uid: String,
    pub runtime_build: String,
    pub student_crc32: String,
    pub seed_vehicle: String,
    pub seed_scenario: String,
    pub duration_us: u64,
    pub tick_us: u64,
    pub started_utc: String,
    pub frames: Vec<RunFrame>,
    pub ticks: Vec<RunTick>,
}

impl RunRecord {
    /// File bytes: canonical JSON (UTF-8, no whitespace, object keys sorted by byte order, integers in decimal, no
    /// JSON float) of `{"format":"n7rt-run","version":1, …fields…, "integrity": …}`, where `integrity` is the SHA-256
    /// (lowercase hex) of the canonical JSON of the same document without that key. Frame payloads are lowercase hex
    /// strings; a slot is `null` (not written), `"reject"` or `{"v": "<decimal>"}` (shortest decimal that reads back
    /// to the same f64; "NaN", "inf", "-inf" for non-finite values). Not compressed.
    pub fn to_file_bytes(&self) -> Vec<u8> {
        run::to_bytes(self)
    }

    /// Reads a run file (any JSON layout) and checks its format, version and integrity hash on the canonical form.
    /// Errors: « fichier d'essai illisible », « version non prise en charge », « empreinte invalide : fichier
    /// altéré ».
    pub fn from_file_bytes(bytes: &[u8]) -> Result<RunRecord, String> {
        run::from_bytes(bytes)
    }
}
