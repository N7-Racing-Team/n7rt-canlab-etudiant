//! Mission pack: JSON file format, validation and resolved tables (format documented in the private pack README).
//!
//! Error texts name the failing field by its JSON path (`frames[2].signals[1].start`), never a frame or signal name,
//! and never quote a value of the file.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;

use crate::codec::{self, ByteOrder};
use crate::vocab;

pub(crate) const FORMAT: &str = "n7rt-mission-pack";
pub(crate) const FORMAT_VERSION: u32 = 1;
/// Student projection (docs/distribution.md §3.1): simulation only, no oracle.
pub(crate) const STUDENT_FORMAT: &str = "n7rt-student-pack";
pub(crate) const STUDENT_FORMAT_VERSION: u32 = 1;
/// Top-level keys of a student pack. `background.vcu` holds the background VCU's consumer parameters, neutral in a
/// student pack (`neutral_consumer`): the real ones are part of the oracle.
const STUDENT_KEYS: [&str; 17] = [
    "format",
    "format_version",
    "mission",
    "bench",
    "bench_ecu",
    "bench_sensor",
    "adc_channels",
    "tick_period_ms",
    "slot_count",
    "bus",
    "power",
    "enums",
    "frames",
    "source_ecu",
    "background",
    "nominal",
    "jitter_seed",
];

// ------------------------------------------------------------------------------------------------- file types

/// Integer written as a JSON number or as a `"0x…"` string.
#[derive(Debug, Clone, Copy)]
struct Num(i64);

impl<'de> Deserialize<'de> for Num {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            I(i64),
            S(String),
        }
        match Raw::deserialize(d)? {
            Raw::I(v) => Ok(Num(v)),
            Raw::S(s) => {
                let t = s.trim();
                let v = match t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
                    Some(h) => i64::from_str_radix(h, 16),
                    None => t.parse::<i64>(),
                };
                v.map(Num).map_err(|_| serde::de::Error::custom("entier invalide"))
            }
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PackFile {
    format: String,
    format_version: u32,
    mission: String,
    variant: String,
    #[serde(default)]
    #[allow(dead_code)]
    provenance: Option<serde_json::Value>,
    /// Bench circuit inlined as text (written by `Pack::to_json`); takes precedence over `bench_file`.
    #[serde(default)]
    bench: Option<String>,
    /// Seed of the jitter and drift PRNG of the variant (16 hex digits), mixed into the session seed. Absent = 0.
    #[serde(default)]
    jitter_seed: Option<String>,
    bench_file: String,
    bench_ecu: String,
    bench_sensor: String,
    adc_channels: Vec<String>,
    tick_period_ms: u16,
    bus: BusFile,
    power: PowerFile,
    enums: BTreeMap<String, BTreeMap<String, Num>>,
    frames: Vec<FrameFile>,
    source_ecu: SourceEcuFile,
    background: BackgroundFile,
    consumer: ConsumerFile,
    slots: Vec<SlotFile>,
    nominal: NominalFile,
    compare: CompareFile,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BusFile {
    jitter_ms: f64,
    hyperperiod_ms: f64,
    min_spacing_ms: f64,
    min_margin_ms: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PowerFile {
    lv_nominal_v: f64,
    reset_below_v: f64,
    reset_exit_v: f64,
    t_boot_ms: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FrameFile {
    id: Num,
    #[serde(default)]
    #[allow(dead_code)]
    name: String,
    emitter: String,
    period_ms: f64,
    phase_ms: f64,
    dlc: u8,
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    mdt_ms: Option<f64>,
    protection: ProtectionFile,
    signals: Vec<SignalFile>,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ProtectionFile {
    P22 { data_ids: Vec<Num> },
    Alive { bits: u8 },
    None,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SignalFile {
    #[serde(default)]
    #[allow(dead_code)]
    name: String,
    start: u16,
    len: u8,
    #[serde(default)]
    order: Option<String>,
    #[serde(default)]
    signed: bool,
    #[serde(default)]
    factor: Option<f64>,
    #[serde(default)]
    offset: Option<f64>,
    #[serde(default)]
    min: Option<Num>,
    #[serde(default)]
    max: Option<Num>,
    #[serde(default)]
    sna: Option<Num>,
    #[serde(default)]
    substitute: Option<f64>,
    #[serde(default, rename = "enum")]
    enumeration: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    unit: Option<String>,
    #[serde(default)]
    qualifier: Option<QualifierFile>,
    source: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QualifierFile {
    source: String,
    ok: Num,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceEcuFile {
    #[serde(default)]
    wsf: Option<crate::ecu_wsf::WsfParams>,
    #[serde(default)]
    sas: Option<crate::ecu_sas::SasConfig>,
    #[serde(default)]
    bps: Option<crate::ecu_bps::SourceBps>,
    emitter: String,
    adc_bits: u8,
    adc_vref_v: f64,
    sig_divider: f64,
    sup_divider: f64,
    filter_samples: usize,
    ratio_undefined_below_v: f64,
    supply_window_v: [f64; 2],
    channels: Vec<ChannelFile>,
    travel_range_pct: [f64; 2],
    debounce_fault_samples: u32,
    debounce_recover_samples: u32,
    init_after_rails_ms: u32,
    sens_sup: SensSupFile,
    lv_uv_on_v: f64,
    lv_uv_off_v: f64,
    lv_avg_samples: usize,
    status_enum: String,
    mode_enum: String,
    self_heating_k: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChannelFile {
    window_pct: [f64; 2],
    a_pct: f64,
    b_pct: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SensSupFile {
    nominal_v: f64,
    tolerance_v: f64,
    rise_samples: u32,
    fall_samples: u32,
    fault_persist_ms: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BackgroundFile {
    world: WorldFile,
    vehicle: VehicleFile,
    vcu_inputs: VcuInputsFile,
    static_ecus: Vec<StaticEcuFile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WorldFile {
    veh_st: String,
    sdc_closed: u8,
    r2d_sound: u8,
    v_mps: f64,
    delta_sw_deg: f64,
    slip: f64,
    ts_btn: u8,
    r2d_btn: u8,
    drive_mode: i64,
    trq_map: i64,
    soc0: f64,
    t_amb_c: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VehicleFile {
    r_eff_m: f64,
    wheelbase_m: f64,
    track_rear_m: f64,
    steer_ratio: f64,
    gear_ratio: f64,
    c_max_nm: f64,
    c_cons_nm: f64,
    delta_max_deg: f64,
    p_max_w: f64,
    f_map_base: f64,
    f_map_step: f64,
    c_regen_nm: f64,
    p_ref_bar: f64,
    v_min_mps: f64,
    eta: f64,
    n_tc_rpm: f64,
    s_on: f64,
    s_off: f64,
    k_tc: f64,
    n_s: f64,
    r_i_ohm: f64,
    q_nom_ah: f64,
    ocv_base_v: f64,
    ocv_span_v: f64,
    dv_cell_v: f64,
    dt_cell_k: f64,
    tau_th_s: f64,
    temp_step_c: f64,
    t_motor_k_per_kw: f64,
    t_igbt_k_per_kw: f64,
    t_max_k_per_kw: f64,
    tau_trq_ms: f64,
    init_ms: u32,
    inv_ready_v: f64,
    ws_stop_ms: u32,
    mem_free_pct: [f64; 2],
    mem_free_step_pct: f64,
    mem_free_period_s: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VcuInputsFile {
    steering_angle: Option<String>,
    wheels_front: Option<[String; 2]>,
    motor_speed: Option<String>,
    brake_active: Option<String>,
    brake_pressure_front: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StaticEcuFile {
    emitter: String,
    prefix: String,
    self_heating_k: f64,
    phases: Vec<PhaseFile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PhaseFile {
    until_ms: Option<u64>,
    values: BTreeMap<String, Option<f64>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConsumerFile {
    #[serde(default)]
    mission: serde_json::Value,
    p22: TimeoutsFile,
    alive: TimeoutsFile,
    unprotected: TimeoutsFile,
    max_delta_counter: u8,
    life_counter_max_delta: u32,
    state_machine: SmFile,
    apps: AppsFile,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TimeoutsFile {
    first_timeout_periods: u32,
    timeout_periods: u32,
    stale_periods: Option<u32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SmFile {
    window_size_valid: u8,
    window_size_init: u8,
    window_size_invalid: u8,
    max_error_state_valid: u8,
    max_error_state_init: u8,
    max_error_state_invalid: u8,
    min_ok_state_valid: u8,
    min_ok_state_init: u8,
    min_ok_state_invalid: u8,
    clear_to_invalid: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AppsFile {
    travel_sources: [String; 2],
    supply_sources: [String; 2],
    max_diff_pct: f64,
    persistence_ms: u32,
    recovery_ticks: u32,
    recovery_max_pct: f64,
    substitute_pct: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SlotFile {
    slot: u8,
    output: String,
    kind: String,
    lsb: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NominalFile {
    #[serde(default)]
    world_kind: String,
    #[serde(default)]
    world_ranges: BTreeMap<String, [f64; 2]>,
    cold_start_ms: u64,
    nominal_ms: u64,
    hold_ms: u64,
    ramp_ms: [u64; 2],
    segment_ms: [u64; 2],
    position_quantum: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CompareFile {
    transition_tolerance_ms: u32,
    value_tolerance_lsb: f64,
    max_listed_mismatches: usize,
}

// ------------------------------------------------------------------------------------------- resolved types

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Role {
    Crc,
    Counter,
    Alive,
    Value,
}

#[derive(Debug, Clone)]
pub(crate) struct Signal {
    pub role: Role,
    /// Bit positions in the payload, least significant bit of the signal first.
    pub positions: Vec<u16>,
    pub signed: bool,
    pub factor: f64,
    pub offset: f64,
    pub min: i64,
    pub max: i64,
    pub sna: Option<i64>,
    pub substitute: Option<f64>,
    /// Index (same frame) of the status signal that must hold the given raw value for this signal to be valid.
    pub qualifier: Option<(usize, i64)>,
    pub source: String,
}

#[derive(Debug, Clone)]
pub(crate) enum Protection {
    P22 { data_ids: [u8; 16] },
    Alive { bits: u8, signal: usize },
    None,
}

#[derive(Debug, Clone)]
pub(crate) struct Frame {
    pub id: u32,
    pub emitter: String,
    pub period_us: u64,
    pub phase_us: u64,
    pub dlc: u8,
    /// MIXED transmission (periodic part + driver events). No driver event exists in this slice: only the
    /// periodic part is emitted.
    #[allow(dead_code)]
    pub mixed: bool,
    pub protection: Protection,
    pub signals: Vec<Signal>,
}

#[derive(Debug, Clone)]
pub(crate) struct Power {
    pub lv_nominal_v: f64,
    pub reset_below_v: f64,
    pub reset_exit_v: f64,
    pub t_boot_ms: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct Channel {
    pub window_lo: f64,
    pub window_hi: f64,
    pub a: f64,
    pub b: f64,
}

#[derive(Debug, Clone)]
pub(crate) struct SourceEcu {
    pub wsf: Option<crate::ecu_wsf::WsfParams>,
    pub sas: Option<crate::ecu_sas::SasConfig>,
    pub bps: Option<crate::ecu_bps::SourceBps>,
    pub emitter: String,
    pub adc_bits: u8,
    pub adc_vref_v: f64,
    pub sig_divider: f64,
    pub sup_divider: f64,
    pub filter_samples: usize,
    pub ratio_undefined_below_v: f64,
    pub supply_lo_v: f64,
    pub supply_hi_v: f64,
    pub channels: [Channel; 2],
    pub travel_lo: f64,
    pub travel_hi: f64,
    pub debounce_fault: u32,
    pub debounce_recover: u32,
    pub init_after_rails_ms: u64,
    pub sup_nominal_v: f64,
    pub sup_tolerance_v: f64,
    pub sup_rise: u32,
    pub sup_fall: u32,
    pub sup_fault_persist_ms: u64,
    pub lv_uv_on_v: f64,
    pub lv_uv_off_v: f64,
    pub lv_avg_samples: usize,
    /// Codes of the channel status enumeration: OK, INIT, SRC_LO, SRC_HI.
    pub st_ok: i64,
    pub st_init: i64,
    pub st_lo: i64,
    pub st_hi: i64,
    /// Codes of the ECU mode enumeration: INIT, RUN, DEGRADED, FAULT.
    pub mode_init: i64,
    pub mode_run: i64,
    pub mode_degraded: i64,
    pub mode_fault: i64,
    pub self_heating_k: f64,
}

#[derive(Debug, Clone)]
pub(crate) struct World {
    pub veh_st: i64,
    pub r2d: bool,
    pub air_closed: bool,
    pub sdc_closed: u8,
    pub r2d_sound: u8,
    pub v_mps: f64,
    pub delta_sw_deg: f64,
    pub slip: f64,
    pub ts_btn: u8,
    pub r2d_btn: u8,
    pub drive_mode: i64,
    pub trq_map: i64,
    pub soc0: f64,
    pub t_amb_c: f64,
}

#[derive(Debug, Clone)]
pub(crate) struct VehicleParams {
    pub r_eff_m: f64,
    pub wheelbase_m: f64,
    pub track_rear_m: f64,
    pub steer_ratio: f64,
    pub gear_ratio: f64,
    pub c_max_nm: f64,
    pub c_cons_nm: f64,
    pub delta_max_deg: f64,
    pub p_max_w: f64,
    pub f_map_base: f64,
    pub f_map_step: f64,
    pub c_regen_nm: f64,
    pub p_ref_bar: f64,
    pub v_min_mps: f64,
    pub eta: f64,
    pub n_tc_rpm: f64,
    pub s_on: f64,
    pub s_off: f64,
    pub k_tc: f64,
    pub n_s: f64,
    pub r_i_ohm: f64,
    pub q_nom_ah: f64,
    pub ocv_base_v: f64,
    pub ocv_span_v: f64,
    pub dv_cell_v: f64,
    pub dt_cell_k: f64,
    pub tau_th_s: f64,
    pub temp_step_c: f64,
    pub t_motor_k_per_kw: f64,
    pub t_igbt_k_per_kw: f64,
    pub t_max_k_per_kw: f64,
    pub tau_trq_ms: f64,
    pub init_ms: u64,
    pub inv_ready_v: f64,
    pub ws_stop_ms: u64,
    pub mem_free_lo: f64,
    pub mem_free_hi: f64,
    pub mem_free_step: f64,
    pub mem_free_period_ms: u64,
}

/// Codes of the background enumerations used by the engine laws.
#[derive(Debug, Clone)]
pub(crate) struct Codes {
    pub trq_disabled: i64,
    pub trq_drive: i64,
    pub trq_regen: i64,
    pub trq_safe: i64,
    pub inv_standby: i64,
    pub inv_ready: i64,
    pub inv_run: i64,
    pub inv_fault: i64,
    pub bms_init: i64,
    pub bms_idle: i64,
    /// Precharge needs a TS activation event, absent from this slice (static world).
    #[allow(dead_code)]
    pub bms_precharge: i64,
    pub bms_active: i64,
    pub imu_ok: i64,
    pub imu_init: i64,
    pub log_recording: i64,
    pub log_full: i64,
    pub ws_ok: i64,
}

#[derive(Debug, Clone)]
pub(crate) struct VcuInputs {
    pub steering_angle: Option<String>,
    pub wheels_front: Option<[String; 2]>,
    pub motor_speed: Option<String>,
    pub brake_active: Option<String>,
    pub brake_pressure_front: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct StaticEcu {
    pub emitter: String,
    pub prefix: String,
    pub self_heating_k: f64,
    /// Keys (source names without the prefix) in a fixed order.
    pub keys: Vec<String>,
    /// (until_ms exclusive, values in `keys` order); the last phase has `until_ms` = None.
    pub phases: Vec<(Option<u64>, Vec<Option<f64>>)>,
}

#[derive(Debug, Clone)]
pub(crate) struct Background {
    pub world: World,
    pub vehicle: VehicleParams,
    pub codes: Codes,
    pub vcu_inputs: VcuInputs,
    pub static_ecus: Vec<StaticEcu>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Timeouts {
    pub first_timeout_periods: u64,
    pub timeout_periods: u64,
    pub stale_periods: Option<u64>,
}

#[derive(Debug, Clone)]
pub(crate) struct AppsParams {
    pub travel_sources: [String; 2],
    pub supply_sources: [String; 2],
    pub max_diff_pct: f64,
    pub persistence_us: u64,
    pub recovery_ticks: u32,
    pub recovery_max_pct: f64,
    pub substitute_pct: f64,
}

#[derive(Debug, Clone)]
pub(crate) struct Consumer {
    pub mission: serde_json::Value,
    pub p22: Timeouts,
    pub alive: Timeouts,
    pub unprotected: Timeouts,
    pub max_delta_counter: u8,
    pub life_counter_max_delta: u32,
    pub sm: crate::e2e::SmConfig,
    pub apps: AppsParams,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SlotOutput {
    Mission(usize),
    Travel1,
    Travel2,
    Diff,
    Implausibility,
    Cut,
    Retained,
    Supply1,
    Supply2,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Slot {
    pub output: SlotOutput,
    pub logic: bool,
    pub lsb: f64,
}

#[derive(Debug, Clone)]
pub(crate) struct Nominal {
    pub world_ranges: BTreeMap<String, [f64; 2]>,
    pub cold_start_ms: u64,
    pub nominal_ms: u64,
    pub hold_ms: u64,
    pub ramp_ms: [u64; 2],
    pub segment_ms: [u64; 2],
    pub position_quantum: f64,
}

#[derive(Debug, Clone)]
pub(crate) struct CompareParams {
    pub transition_tolerance_ms: u64,
    pub value_tolerance_lsb: f64,
    pub max_listed_mismatches: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct PackData {
    pub mission: String,
    pub bench_json: String,
    pub bench_ecu: String,
    pub bench_sensor: String,
    pub adc_channels: Vec<String>,
    pub tick_period_ms: u16,
    pub jitter_us: u64,
    /// Scheduling rules of ch. 01 §2.2 (checked at load, reused by the variants).
    pub hyperperiod_us: u64,
    pub min_spacing_us: u64,
    pub min_margin_us: u64,
    pub power: Power,
    pub frames: Vec<Frame>,
    /// Source name → (frame index, signal index), value signals only.
    pub sources: BTreeMap<String, (usize, usize)>,
    pub source_ecu: SourceEcu,
    pub background: Background,
    pub consumer: Consumer,
    /// Index = slot number. Empty for a student pack.
    pub slots: Vec<Slot>,
    /// Number of public slots (the only slot information a student pack keeps).
    pub slot_count: usize,
    pub nominal: Nominal,
    pub compare: CompareParams,
    /// Variant seed of the jitter and drift PRNG, XORed into the session seed (0 = none).
    pub jitter_seed: u64,
    /// Read with `from_student_json`: no oracle (slots, comparison rules).
    pub student: bool,
    /// The JSON document the pack was read from (full or student format).
    pub raw: serde_json::Value,
}

impl PackData {
    pub fn frame_index(&self, id: u32) -> Option<usize> {
        self.frames.iter().position(|f| f.id == id)
    }

    pub fn timeouts(&self, frame: &Frame) -> Timeouts {
        match frame.protection {
            Protection::P22 { .. } => self.consumer.p22,
            Protection::Alive { .. } => self.consumer.alive,
            Protection::None => self.consumer.unprotected,
        }
    }
}

// ----------------------------------------------------------------------------------------------- validation

fn err<T>(path: &str, msg: &str) -> Result<T, String> {
    Err(format!("{path} : {msg}"))
}

/// Removes double-quoted fragments (file values) from a parser message; backquoted field names are kept.
fn sanitize_parse_error(msg: &str) -> String {
    let mut out = String::with_capacity(msg.len());
    let mut in_quote = false;
    for c in msg.chars() {
        if c == '"' {
            if !in_quote {
                out.push_str("\"…\"");
            }
            in_quote = !in_quote;
            continue;
        }
        if !in_quote {
            out.push(c);
        }
    }
    out
}

fn ms_to_us(path: &str, ms: f64) -> Result<u64, String> {
    if !ms.is_finite() || ms < 0.0 {
        return err(path, "durée invalide");
    }
    let us = (ms * 1000.0).round();
    if (us - ms * 1000.0).abs() > 1e-6 {
        return err(path, "durée non multiple de 1 µs");
    }
    Ok(us as u64)
}

fn finite(path: &str, v: f64) -> Result<f64, String> {
    if v.is_finite() {
        Ok(v)
    } else {
        err(path, "nombre invalide")
    }
}

fn positive(path: &str, v: f64) -> Result<f64, String> {
    if v.is_finite() && v > 0.0 {
        Ok(v)
    } else {
        err(path, "doit être strictement positif")
    }
}

fn code(
    enums: &BTreeMap<String, BTreeMap<String, i64>>,
    path: &str,
    name: &str,
    key: &str,
) -> Result<i64, String> {
    match enums.get(name) {
        None => err(path, "énumération absente de `enums`"),
        Some(m) => match m.get(key) {
            Some(v) => Ok(*v),
            None => err(path, "code absent de l'énumération"),
        },
    }
}

/// Reads a full pack. An inline `bench` takes precedence over `bench_json` (the text of `bench_file`).
pub(crate) fn parse(text: &str, bench_json: String) -> Result<PackData, String> {
    let file: PackFile = serde_json::from_str(text)
        .map_err(|e| format!("pack : JSON invalide ({})", sanitize_parse_error(&e.to_string())))?;
    let raw: serde_json::Value = serde_json::from_str(text).map_err(|_| "pack : JSON invalide".to_string())?;
    let bench_json = file.bench.clone().unwrap_or(bench_json);
    let mut d = validate(file, bench_json, false)?;
    d.raw = raw;
    Ok(d)
}

/// Reads a student pack (`STUDENT_FORMAT`): rebuilt as a full document without oracle, then the same validation.
pub(crate) fn parse_student(text: &str) -> Result<PackData, String> {
    use serde_json::{json, Value};
    let raw: Value = serde_json::from_str(text).map_err(|_| "pack étudiant : JSON invalide".to_string())?;
    let obj = raw.as_object().ok_or_else(|| "pack étudiant : objet JSON attendu".to_string())?;
    if obj.get("format").and_then(Value::as_str) != Some(STUDENT_FORMAT) {
        return err("format", "format de pack étudiant inconnu");
    }
    if obj.get("format_version").and_then(Value::as_u64) != Some(STUDENT_FORMAT_VERSION as u64) {
        return err("format_version", "version de format non prise en charge");
    }
    if let Some(k) = obj.keys().find(|k| !STUDENT_KEYS.contains(&k.as_str())) {
        return err(k, "champ inattendu dans un pack étudiant");
    }
    let bench = match obj.get("bench") {
        Some(Value::String(s)) => s.clone(),
        _ => return err("bench", "texte du banc attendu"),
    };
    let slot_count = match obj.get("slot_count").and_then(Value::as_u64) {
        Some(n) if (1..=8).contains(&n) => n as usize,
        _ => return err("slot_count", "entre 1 et 8 slots"),
    };
    let mut full = obj.clone();
    for k in ["format", "format_version", "bench", "slot_count"] {
        full.remove(k);
    }
    let mut background = match full.remove("background") {
        Some(Value::Object(b)) => b,
        _ => return err("background", "objet attendu"),
    };
    let vcu = background.remove("vcu").ok_or_else(|| "background.vcu : paramètres du VCU de fond absents".to_string())?;
    full.insert("background".into(), Value::Object(background));
    full.insert("consumer".into(), vcu);
    full.insert("format".into(), json!(FORMAT));
    full.insert("format_version".into(), json!(FORMAT_VERSION));
    full.insert("variant".into(), json!("student"));
    full.insert("bench_file".into(), json!("student"));
    full.insert("slots".into(), json!([]));
    // never used: `compare` and `judge_run` refuse a student pack
    full.insert(
        "compare".into(),
        json!({"transition_tolerance_ms": 0, "value_tolerance_lsb": 1.0, "max_listed_mismatches": 0}),
    );
    let file: PackFile = serde_json::from_value(Value::Object(full))
        .map_err(|e| format!("pack étudiant : JSON invalide ({})", sanitize_parse_error(&e.to_string())))?;
    let mut d = validate(file, bench, true)?;
    d.slot_count = slot_count;
    d.raw = raw;
    Ok(d)
}

/// Background VCU parameters of a student pack: the source names of the full pack (public vocabulary, needed by the
/// public stand-in), every supervision and FS value neutral (no timeout, trivial state machine, no implausibility),
/// so that no consumer parameter of the oracle reaches a student. An instructor build that loads a student pack runs
/// its background VCU on these neutral values.
fn neutral_consumer(consumer: &serde_json::Value) -> serde_json::Value {
    use serde_json::json;
    let timeouts = json!({ "first_timeout_periods": 1_000_000, "timeout_periods": 1_000_000, "stale_periods": null });
    json!({
        "p22": timeouts,
        "alive": timeouts,
        "unprotected": timeouts,
        "max_delta_counter": 14,
        "life_counter_max_delta": 1_000_000,
        "state_machine": {
            "window_size_valid": 1, "window_size_init": 1, "window_size_invalid": 1,
            "max_error_state_valid": 0, "max_error_state_init": 0, "max_error_state_invalid": 0,
            "min_ok_state_valid": 1, "min_ok_state_init": 1, "min_ok_state_invalid": 1,
            "clear_to_invalid": false
        },
        "apps": {
            "travel_sources": consumer["apps"]["travel_sources"],
            "supply_sources": consumer["apps"]["supply_sources"],
            "max_diff_pct": 100.0,
            "persistence_ms": 0,
            "recovery_ticks": 1,
            "recovery_max_pct": 100.0,
            "substitute_pct": 0.0
        }
    })
}

/// Student projection of a pack (docs/distribution.md §3.1): what the simulation needs, nothing of the oracle (no
/// slot outputs or LSB, no comparison rules, no provenance, no frame or signal names, no units, no consumer
/// parameter). The bench is inlined as text; `background.vcu` carries neutral consumer parameters
/// ([`neutral_consumer`]).
pub(crate) fn student_document(d: &PackData) -> serde_json::Value {
    use serde_json::{json, Map, Value};
    if d.student {
        return d.raw.clone();
    }
    let r = &d.raw;
    let mut o = Map::new();
    o.insert("format".into(), json!(STUDENT_FORMAT));
    o.insert("format_version".into(), json!(STUDENT_FORMAT_VERSION));
    o.insert("mission".into(), json!(d.mission));
    o.insert("bench".into(), json!(d.bench_json));
    o.insert("slot_count".into(), json!(d.slot_count));
    for k in ["bench_ecu", "bench_sensor", "adc_channels", "tick_period_ms", "bus", "power", "enums", "source_ecu", "nominal"]
    {
        o.insert(k.into(), r[k].clone());
    }
    let strip = |v: &Value, keys: &[&str]| -> Value {
        let mut v = v.clone();
        if let Some(m) = v.as_object_mut() {
            for k in keys {
                m.remove(*k);
            }
        }
        v
    };
    let frames: Vec<Value> = r["frames"]
        .as_array()
        .map(|a| a.as_slice())
        .unwrap_or_default()
        .iter()
        .map(|f| {
            let mut f = strip(f, &["name"]);
            if let Some(sigs) = f.get_mut("signals").and_then(Value::as_array_mut) {
                for s in sigs.iter_mut() {
                    *s = strip(s, &["name", "unit"]);
                }
            }
            f
        })
        .collect();
    o.insert("frames".into(), Value::Array(frames));
    let mut bg = r["background"].clone();
    if let Some(m) = bg.as_object_mut() {
        m.insert("vcu".into(), neutral_consumer(&r["consumer"]));
    }
    o.insert("background".into(), bg);
    if let Some(js) = r.get("jitter_seed").filter(|v| !v.is_null()) {
        o.insert("jitter_seed".into(), js.clone());
    }
    Value::Object(o)
}

fn validate(f: PackFile, bench_json: String, student: bool) -> Result<PackData, String> {
    if f.format != FORMAT {
        return err("format", "format de pack inconnu");
    }
    if f.format_version != FORMAT_VERSION {
        return err("format_version", "version de format non prise en charge");
    }
    if f.mission.is_empty() || f.variant.is_empty() {
        return err("mission", "identifiant vide");
    }
    if f.bench_file.is_empty() || f.bench_file.contains("..") || f.bench_file.starts_with('/') {
        return err("bench_file", "chemin relatif au pack attendu");
    }
    let source_mission = match f.mission.as_str() { "wsf" => "wsf", "sas" => "sas", "bps" => "bps", _ => "apps" };
    let adc_count = match source_mission { "wsf" => 0, "sas" => 1, "bps" => 3, _ => 4 };
    if f.adc_channels.len() != adc_count {
        return err("adc_channels", "nombre de canaux incompatible avec la mission");
    }
    if f.tick_period_ms == 0 || f.tick_period_ms > 1000 {
        return err("tick_period_ms", "période de tick hors de 1…1000 ms");
    }

    // enums
    let mut enums: BTreeMap<String, BTreeMap<String, i64>> = BTreeMap::new();
    for (name, m) in &f.enums {
        let mut seen = BTreeSet::new();
        let mut out = BTreeMap::new();
        for (k, v) in m {
            if !seen.insert(v.0) {
                return err(&format!("enums.{name}"), "code en double");
            }
            out.insert(k.clone(), v.0);
        }
        enums.insert(name.clone(), out);
    }

    // bus and power
    let jitter_us = ms_to_us("bus.jitter_ms", f.bus.jitter_ms)?;
    let hyper_us = ms_to_us("bus.hyperperiod_ms", f.bus.hyperperiod_ms)?;
    let spacing_us = ms_to_us("bus.min_spacing_ms", f.bus.min_spacing_ms)?;
    let margin_us = ms_to_us("bus.min_margin_ms", f.bus.min_margin_ms)?;
    if hyper_us == 0 {
        return err("bus.hyperperiod_ms", "doit être strictement positif");
    }
    finite("power.reset_below_v", f.power.reset_below_v)?;
    finite("power.reset_exit_v", f.power.reset_exit_v)?;
    if f.power.reset_exit_v < f.power.reset_below_v {
        return err("power.reset_exit_v", "seuil de sortie de reset sous le seuil de reset");
    }
    let power = Power {
        lv_nominal_v: positive("power.lv_nominal_v", f.power.lv_nominal_v)?,
        reset_below_v: f.power.reset_below_v,
        reset_exit_v: f.power.reset_exit_v,
        t_boot_ms: f.power.t_boot_ms as u64,
    };

    // frames
    let mut frames = Vec::with_capacity(f.frames.len());
    let mut ids = BTreeSet::new();
    let mut lists: Vec<[u8; 16]> = Vec::new();
    for (fi, ff) in f.frames.iter().enumerate() {
        let p = format!("frames[{fi}]");
        let frame = validate_frame(&p, ff, &enums)?;
        if !ids.insert(frame.id) {
            return err(&format!("{p}.id"), "identifiant en double");
        }
        if let Protection::P22 { data_ids } = &frame.protection {
            if lists.iter().any(|l| l == data_ids) {
                return err(&format!("{p}.protection.data_ids"), "liste de Data ID déjà utilisée");
            }
            lists.push(*data_ids);
        }
        if hyper_us % frame.period_us != 0 {
            return err(&format!("{p}.period_ms"), "période qui ne divise pas l'hyperpériode");
        }
        if frame.period_us - frame.phase_us < margin_us {
            return err(&format!("{p}.phase_ms"), "marge avant la fin de la fenêtre d'évaluation insuffisante");
        }
        frames.push(frame);
    }
    check_spacing(&frames, hyper_us, spacing_us)?;

    // source ECU
    let se = &f.source_ecu;
    if !frames.iter().any(|fr| fr.emitter == se.emitter) {
        return err("source_ecu.emitter", "aucune trame de cet émetteur");
    }
    if se.channels.len() != 2 {
        return err("source_ecu.channels", "deux canaux attendus");
    }
    if se.adc_bits == 0 || se.adc_bits > 24 {
        return err("source_ecu.adc_bits", "résolution invalide");
    }
    positive("source_ecu.adc_vref_v", se.adc_vref_v)?;
    positive("source_ecu.sig_divider", se.sig_divider)?;
    positive("source_ecu.sup_divider", se.sup_divider)?;
    if !(1..=4096).contains(&se.filter_samples) || !(1..=4096).contains(&se.lv_avg_samples) {
        return err("source_ecu.filter_samples", "fenêtre hors de 1…4096 échantillons");
    }
    let mut channels = Vec::new();
    for (ci, c) in se.channels.iter().enumerate() {
        let cp = format!("source_ecu.channels[{ci}]");
        if !(c.window_pct[0] < c.window_pct[1]) {
            return err(&format!("{cp}.window_pct"), "fenêtre vide");
        }
        positive(&format!("{cp}.b_pct"), c.b_pct)?;
        finite(&format!("{cp}.a_pct"), c.a_pct)?;
        channels.push(Channel { window_lo: c.window_pct[0], window_hi: c.window_pct[1], a: c.a_pct, b: c.b_pct });
    }
    let mut wsf = se.wsf.clone();
    let mut sas = se.sas.clone();
    let mut bps = se.bps.clone();
    if [wsf.is_some(), sas.is_some(), bps.is_some()].iter().filter(|v| **v).count()
        != usize::from(source_mission != "apps")
        || (f.mission == "wsf") != wsf.is_some() || (f.mission == "sas") != sas.is_some()
        || (f.mission == "bps") != bps.is_some() {
        return err("source_ecu", "configuration incompatible avec la mission");
    }
    if let Some(p) = &mut wsf {
        for (i, name) in ["OK", "INIT", "OPEN", "SHORT", "CUR_FAULT", "SUPPLY"].iter().enumerate() {
            p.status_codes[i] = code(&enums, "source_ecu.wsf.status_enum", &p.status_enum, name)?;
        }
        for (i, name) in ["INIT", "RUN", "DEGRADED", "FAULT"].iter().enumerate() {
            p.mode_codes[i] = code(&enums, "source_ecu.wsf.mode_enum", &p.mode_enum, name)?;
        }
        p.validate()?;
    }
    if let Some(p) = &mut sas {
        p.st_init = code(&enums, "source_ecu.sas", &p.status_enum, &p.st_init_name)?;
        p.st_ok = code(&enums, "source_ecu.sas", &p.status_enum, &p.st_ok_name)?;
        p.st_sensor_error = code(&enums, "source_ecu.sas", &p.status_enum, &p.st_sensor_error_name)?;
        p.st_comm_error = code(&enums, "source_ecu.sas", &p.status_enum, &p.st_comm_error_name)?;
        p.mode_init = code(&enums, "source_ecu.sas", &p.mode_enum, &p.mode_init_name)?;
        p.mode_run = code(&enums, "source_ecu.sas", &p.mode_enum, &p.mode_run_name)?;
        p.mode_degraded = code(&enums, "source_ecu.sas", &p.mode_enum, &p.mode_degraded_name)?;
        p.mode_fault = code(&enums, "source_ecu.sas", &p.mode_enum, &p.mode_fault_name)?;
        p.validate()?;
    }
    if let Some(p) = &mut bps {
        p.switch_codes[0] = code(&enums, "source_ecu.bps.switch_enum", &p.switch_enum, "OUVERT")?;
        p.switch_codes[1] = code(&enums, "source_ecu.bps.switch_enum", &p.switch_enum, "FERME")?;
        p.switch_codes[2] = frames.iter().flat_map(|fr| &fr.signals).find(|s| s.source == "bps.brk_sw")
            .and_then(|s| s.sna).ok_or("source_ecu.bps : contact sans SNA")?;
        p.status_supply = code(&enums, "source_ecu.bps", &se.status_enum, "SUPPLY")?;
        p.status_implausible = code(&enums, "source_ecu.bps", &se.status_enum, "IMPLAUS")?;
        p.validate()?;
    }
    let source_ecu = SourceEcu {
        wsf, sas, bps,
        emitter: se.emitter.clone(),
        adc_bits: se.adc_bits,
        adc_vref_v: se.adc_vref_v,
        sig_divider: se.sig_divider,
        sup_divider: se.sup_divider,
        filter_samples: se.filter_samples,
        ratio_undefined_below_v: se.ratio_undefined_below_v,
        supply_lo_v: se.supply_window_v[0],
        supply_hi_v: se.supply_window_v[1],
        channels: [channels[0].clone(), channels[1].clone()],
        travel_lo: se.travel_range_pct[0],
        travel_hi: se.travel_range_pct[1],
        debounce_fault: se.debounce_fault_samples.max(1),
        debounce_recover: se.debounce_recover_samples.max(1),
        init_after_rails_ms: se.init_after_rails_ms as u64,
        sup_nominal_v: se.sens_sup.nominal_v,
        sup_tolerance_v: se.sens_sup.tolerance_v,
        sup_rise: se.sens_sup.rise_samples.max(1),
        sup_fall: se.sens_sup.fall_samples.max(1),
        sup_fault_persist_ms: se.sens_sup.fault_persist_ms as u64,
        lv_uv_on_v: se.lv_uv_on_v,
        lv_uv_off_v: se.lv_uv_off_v,
        lv_avg_samples: se.lv_avg_samples,
        st_ok: code(&enums, "source_ecu.status_enum", &se.status_enum, "OK")?,
        st_init: code(&enums, "source_ecu.status_enum", &se.status_enum, "INIT")?,
        st_lo: code(&enums, "source_ecu.status_enum", &se.status_enum, "SRC_LO")?,
        st_hi: code(&enums, "source_ecu.status_enum", &se.status_enum, "SRC_HI")?,
        mode_init: code(&enums, "source_ecu.mode_enum", &se.mode_enum, "INIT")?,
        mode_run: code(&enums, "source_ecu.mode_enum", &se.mode_enum, "RUN")?,
        mode_degraded: code(&enums, "source_ecu.mode_enum", &se.mode_enum, "DEGRADED")?,
        mode_fault: code(&enums, "source_ecu.mode_enum", &se.mode_enum, "FAULT")?,
        self_heating_k: se.self_heating_k,
    };

    // background
    let bg = &f.background;
    let w = &bg.world;
    let veh_st = code(&enums, "background.world.veh_st", "veh_st", &w.veh_st)?;
    let r2d = veh_st == code(&enums, "background.world.veh_st", "veh_st", "R2D")?;
    let ts_active = veh_st == code(&enums, "background.world.veh_st", "veh_st", "TS_ACTIVE")?;
    let world = World {
        veh_st,
        r2d,
        air_closed: r2d || ts_active,
        sdc_closed: w.sdc_closed,
        r2d_sound: w.r2d_sound,
        v_mps: finite("background.world.v_mps", w.v_mps)?,
        delta_sw_deg: finite("background.world.delta_sw_deg", w.delta_sw_deg)?,
        slip: finite("background.world.slip", w.slip)?,
        ts_btn: w.ts_btn,
        r2d_btn: w.r2d_btn,
        drive_mode: w.drive_mode,
        trq_map: w.trq_map,
        soc0: w.soc0.clamp(0.0, 1.0),
        t_amb_c: finite("background.world.t_amb_c", w.t_amb_c)?,
    };
    let v = &bg.vehicle;
    let vp = "background.vehicle";
    let vehicle = VehicleParams {
        r_eff_m: positive(&format!("{vp}.r_eff_m"), v.r_eff_m)?,
        wheelbase_m: positive(&format!("{vp}.wheelbase_m"), v.wheelbase_m)?,
        track_rear_m: finite(&format!("{vp}.track_rear_m"), v.track_rear_m)?,
        steer_ratio: positive(&format!("{vp}.steer_ratio"), v.steer_ratio)?,
        gear_ratio: positive(&format!("{vp}.gear_ratio"), v.gear_ratio)?,
        c_max_nm: finite(&format!("{vp}.c_max_nm"), v.c_max_nm)?,
        c_cons_nm: finite(&format!("{vp}.c_cons_nm"), v.c_cons_nm)?,
        delta_max_deg: positive(&format!("{vp}.delta_max_deg"), v.delta_max_deg)?,
        p_max_w: finite(&format!("{vp}.p_max_w"), v.p_max_w)?,
        f_map_base: finite(&format!("{vp}.f_map_base"), v.f_map_base)?,
        f_map_step: finite(&format!("{vp}.f_map_step"), v.f_map_step)?,
        c_regen_nm: finite(&format!("{vp}.c_regen_nm"), v.c_regen_nm)?,
        p_ref_bar: positive(&format!("{vp}.p_ref_bar"), v.p_ref_bar)?,
        v_min_mps: finite(&format!("{vp}.v_min_mps"), v.v_min_mps)?,
        eta: positive(&format!("{vp}.eta"), v.eta)?,
        n_tc_rpm: finite(&format!("{vp}.n_tc_rpm"), v.n_tc_rpm)?,
        s_on: finite(&format!("{vp}.s_on"), v.s_on)?,
        s_off: finite(&format!("{vp}.s_off"), v.s_off)?,
        k_tc: finite(&format!("{vp}.k_tc"), v.k_tc)?,
        n_s: positive(&format!("{vp}.n_s"), v.n_s)?,
        r_i_ohm: finite(&format!("{vp}.r_i_ohm"), v.r_i_ohm)?,
        q_nom_ah: positive(&format!("{vp}.q_nom_ah"), v.q_nom_ah)?,
        ocv_base_v: finite(&format!("{vp}.ocv_base_v"), v.ocv_base_v)?,
        ocv_span_v: finite(&format!("{vp}.ocv_span_v"), v.ocv_span_v)?,
        dv_cell_v: finite(&format!("{vp}.dv_cell_v"), v.dv_cell_v)?,
        dt_cell_k: finite(&format!("{vp}.dt_cell_k"), v.dt_cell_k)?,
        tau_th_s: positive(&format!("{vp}.tau_th_s"), v.tau_th_s)?,
        temp_step_c: finite(&format!("{vp}.temp_step_c"), v.temp_step_c)?.abs(),
        t_motor_k_per_kw: finite(&format!("{vp}.t_motor_k_per_kw"), v.t_motor_k_per_kw)?,
        t_igbt_k_per_kw: finite(&format!("{vp}.t_igbt_k_per_kw"), v.t_igbt_k_per_kw)?,
        t_max_k_per_kw: finite(&format!("{vp}.t_max_k_per_kw"), v.t_max_k_per_kw)?,
        tau_trq_ms: positive(&format!("{vp}.tau_trq_ms"), v.tau_trq_ms)?,
        init_ms: v.init_ms as u64,
        inv_ready_v: finite(&format!("{vp}.inv_ready_v"), v.inv_ready_v)?,
        ws_stop_ms: v.ws_stop_ms as u64,
        mem_free_lo: v.mem_free_pct[0],
        mem_free_hi: v.mem_free_pct[1].max(v.mem_free_pct[0]),
        mem_free_step: positive(&format!("{vp}.mem_free_step_pct"), v.mem_free_step_pct)?,
        mem_free_period_ms: (positive(&format!("{vp}.mem_free_period_s"), v.mem_free_period_s)? * 1000.0).round()
            as u64,
    };
    let cp = "enums";
    let codes = Codes {
        trq_disabled: code(&enums, cp, "trq_mode", "DISABLED")?,
        trq_drive: code(&enums, cp, "trq_mode", "DRIVE")?,
        trq_regen: code(&enums, cp, "trq_mode", "REGEN")?,
        trq_safe: code(&enums, cp, "trq_mode", "SAFE_TORQUE")?,
        inv_standby: code(&enums, cp, "inv_st", "STANDBY")?,
        inv_ready: code(&enums, cp, "inv_st", "READY")?,
        inv_run: code(&enums, cp, "inv_st", "RUN")?,
        inv_fault: code(&enums, cp, "inv_st", "FAULT")?,
        bms_init: code(&enums, cp, "bms_st", "INIT")?,
        bms_idle: code(&enums, cp, "bms_st", "IDLE")?,
        bms_precharge: code(&enums, cp, "bms_st", "PRECHARGE")?,
        bms_active: code(&enums, cp, "bms_st", "ACTIVE")?,
        imu_ok: code(&enums, cp, "imu_st", "OK")?,
        imu_init: code(&enums, cp, "imu_st", "INIT")?,
        log_recording: code(&enums, cp, "log_st", "RECORDING")?,
        log_full: code(&enums, cp, "log_st", "FULL")?,
        ws_ok: code(&enums, cp, "st_ws", "OK")?,
    };
    let mut static_ecus = Vec::new();
    let mut prefixes = BTreeSet::new();
    for (ei, se) in bg.static_ecus.iter().enumerate() {
        let ep = format!("background.static_ecus[{ei}]");
        if se.emitter == source_ecu.emitter {
            return err(&format!("{ep}.emitter"), "l'ECU source ne peut pas être un ECU de fond statique");
        }
        if !vocab::valid_prefix(&se.prefix) || se.prefix == source_mission
            || (vocab::is_dynamic_prefix(&se.prefix) && se.prefix != "apps") || !prefixes.insert(se.prefix.clone())
        {
            return err(&format!("{ep}.prefix"), "préfixe invalide ou déjà utilisé");
        }
        if se.phases.is_empty() {
            return err(&format!("{ep}.phases"), "au moins une phase");
        }
        let keys: Vec<String> = se.phases[0].values.keys().cloned().collect();
        let mut phases = Vec::new();
        let mut last_until = 0u64;
        for (pi, ph) in se.phases.iter().enumerate() {
            let pp = format!("{ep}.phases[{pi}]");
            let pk: Vec<&String> = ph.values.keys().collect();
            if pk.len() != keys.len() || pk.iter().zip(keys.iter()).any(|(a, b)| *a != b) {
                return err(&format!("{pp}.values"), "les phases doivent définir les mêmes grandeurs");
            }
            if keys.iter().any(|k| k == "ecu_temp") {
                return err(&format!("{pp}.values"), "la température est calculée par le moteur");
            }
            let last = pi + 1 == se.phases.len();
            match (ph.until_ms, last) {
                (None, true) => {}
                (Some(u), false) if u > last_until => last_until = u,
                _ => return err(&format!("{pp}.until_ms"), "bornes de phase croissantes, la dernière sans borne"),
            }
            let vals = keys.iter().map(|k| ph.values[k]).collect();
            phases.push((ph.until_ms, vals));
        }
        static_ecus.push(StaticEcu {
            emitter: se.emitter.clone(),
            prefix: se.prefix.clone(),
            self_heating_k: se.self_heating_k,
            keys,
            phases,
        });
    }

    // source vocabulary: every value signal names a known quantity, at most once
    let mut sources = BTreeMap::new();
    for (fi, fr) in frames.iter().enumerate() {
        for (si, s) in fr.signals.iter().enumerate() {
            if s.role != Role::Value {
                continue;
            }
            let sp = format!("frames[{fi}].signals[{si}].source");
            let known = vocab::Dynamic::for_mission(&s.source, &f.mission).is_some() || static_ecus.iter().any(|e| {
                s.source
                    .strip_prefix(e.prefix.as_str())
                    .and_then(|r| r.strip_prefix('.'))
                    .is_some_and(|k| k == "ecu_temp" || e.keys.iter().any(|x| x == k))
            });
            if !known {
                return err(&sp, "grandeur source inconnue du moteur");
            }
            if sources.insert(s.source.clone(), (fi, si)).is_some() {
                return err(&sp, "grandeur source portée par deux signaux");
            }
            // a frame carries the quantities of its own emitter
            let owner_ok = match vocab::Dynamic::for_mission(&s.source, &f.mission) {
                Some(d) => d.owner(&source_ecu.emitter) == fr.emitter.as_str(),
                None => static_ecus
                    .iter()
                    .any(|e| e.emitter == fr.emitter && s.source.starts_with(&format!("{}.", e.prefix))),
            };
            if !owner_ok {
                return err(&sp, "grandeur d'un autre ECU que l'émetteur de la trame");
            }
        }
    }

    // consumer
    let c = &f.consumer;
    let sm = crate::e2e::SmConfig {
        window_size_valid: c.state_machine.window_size_valid,
        window_size_init: c.state_machine.window_size_init,
        window_size_invalid: c.state_machine.window_size_invalid,
        max_error_state_valid: c.state_machine.max_error_state_valid,
        max_error_state_init: c.state_machine.max_error_state_init,
        max_error_state_invalid: c.state_machine.max_error_state_invalid,
        min_ok_state_valid: c.state_machine.min_ok_state_valid,
        min_ok_state_init: c.state_machine.min_ok_state_init,
        min_ok_state_invalid: c.state_machine.min_ok_state_invalid,
        clear_to_invalid: c.state_machine.clear_to_invalid,
    };
    sm.check().map_err(|m| format!("consumer.state_machine : {m}"))?;
    if c.max_delta_counter == 0 || c.max_delta_counter > 14 {
        return err("consumer.max_delta_counter", "valeur hors de 1…14");
    }
    let to = |t: &TimeoutsFile, p: &str| -> Result<Timeouts, String> {
        if t.first_timeout_periods == 0 || t.timeout_periods == 0 || t.stale_periods == Some(0) {
            return err(p, "multiples de période nuls");
        }
        Ok(Timeouts {
            first_timeout_periods: t.first_timeout_periods as u64,
            timeout_periods: t.timeout_periods as u64,
            stale_periods: t.stale_periods.map(|s| s as u64),
        })
    };
    let a = &c.apps;
    for (i, s) in a.travel_sources.iter().chain(a.supply_sources.iter()).enumerate() {
        if !sources.contains_key(s) {
            return err(&format!("consumer.apps.sources[{i}]"), "grandeur absente de la matrice");
        }
    }
    let consumer = Consumer {
        mission: c.mission.clone(),
        p22: to(&c.p22, "consumer.p22")?,
        alive: to(&c.alive, "consumer.alive")?,
        unprotected: to(&c.unprotected, "consumer.unprotected")?,
        max_delta_counter: c.max_delta_counter,
        life_counter_max_delta: c.life_counter_max_delta.max(1),
        sm,
        apps: AppsParams {
            travel_sources: a.travel_sources.clone(),
            supply_sources: a.supply_sources.clone(),
            max_diff_pct: finite("consumer.apps.max_diff_pct", a.max_diff_pct)?,
            persistence_us: a.persistence_ms as u64 * 1000,
            recovery_ticks: a.recovery_ticks.max(1),
            recovery_max_pct: finite("consumer.apps.recovery_max_pct", a.recovery_max_pct)?,
            substitute_pct: finite("consumer.apps.substitute_pct", a.substitute_pct)?,
        },
    };
    let vi = &bg.vcu_inputs;
    let vcu_inputs = VcuInputs {
        steering_angle: vi.steering_angle.clone(),
        wheels_front: vi.wheels_front.clone(),
        motor_speed: vi.motor_speed.clone(),
        brake_active: vi.brake_active.clone(),
        brake_pressure_front: vi.brake_pressure_front.clone(),
    };
    let all_vcu: Vec<&String> = [&vi.steering_angle, &vi.motor_speed, &vi.brake_active, &vi.brake_pressure_front]
        .into_iter()
        .flatten()
        .chain(vi.wheels_front.iter().flat_map(|w| w.iter()))
        .collect();
    for s in all_vcu {
        if !sources.contains_key(s) {
            return err("background.vcu_inputs", "grandeur absente de la matrice");
        }
    }

    // slots
    let required_slots = match f.mission.as_str() { "wsf" => Some(8), "sas" => Some(5), "bps" => Some(6), _ => None };
    if !student && required_slots.is_some_and(|count| f.slots.len() != count) {
        return err("slots", "nombre de slots incompatible avec la mission");
    }
    let mut slots: Vec<Option<Slot>> = vec![None; f.slots.len()];
    for (i, s) in f.slots.iter().enumerate() {
        let sp = format!("slots[{i}]");
        let idx = s.slot as usize;
        if idx >= slots.len() || slots[idx].is_some() {
            return err(&format!("{sp}.slot"), "numéros de slot 0…n−1 uniques attendus");
        }
        let output = match s.output.as_str() {
            "travel1" => SlotOutput::Travel1,
            "travel2" => SlotOutput::Travel2,
            "diff" => SlotOutput::Diff,
            "implausibility" => SlotOutput::Implausibility,
            "cut" => SlotOutput::Cut,
            "retained" => SlotOutput::Retained,
            "supply1" => SlotOutput::Supply1,
            "supply2" => SlotOutput::Supply2,
            name if name.starts_with("mission_") && f.mission != "apps" => {
                let n: usize = name[8..].parse().map_err(|_| format!("{sp}.output : sortie inconnue"))?;
                if n >= f.slots.len() { return err(&format!("{sp}.output"), "sortie hors des slots"); }
                SlotOutput::Mission(n)
            }
            _ => return err(&format!("{sp}.output"), "sortie inconnue"),
        };
        let logic = match s.kind.as_str() {
            "value" => false,
            "logic" => true,
            _ => return err(&format!("{sp}.kind"), "type de slot inconnu"),
        };
        slots[idx] = Some(Slot { output, logic, lsb: positive(&format!("{sp}.lsb"), s.lsb)? });
    }
    let slots: Vec<Slot> = slots.into_iter().flatten().collect();
    if !student && (slots.is_empty() || slots.len() > 8) {
        return err("slots", "entre 1 et 8 slots");
    }
    let jitter_seed = match &f.jitter_seed {
        None => 0,
        Some(s) if s.len() == 16 => u64::from_str_radix(s, 16).map_err(|_| "jitter_seed : graine invalide".to_string())?,
        Some(_) => return err("jitter_seed", "graine invalide"),
    };

    let n = &f.nominal;
    if n.ramp_ms[0] == 0 || n.ramp_ms[0] > n.ramp_ms[1] || n.segment_ms[0] == 0 || n.segment_ms[0] > n.segment_ms[1] {
        return err("nominal", "bornes de durée invalides");
    }
    let mandatory = 3 * n.hold_ms + 2 * n.ramp_ms[1] + n.segment_ms[0];
    if n.nominal_ms < mandatory {
        return err("nominal.nominal_ms", "durée insuffisante pour le balayage complet");
    }
    positive("nominal.position_quantum", n.position_quantum)?;
    let world_kind = match source_mission { "wsf" => "front_axle", "sas" => "steering_column", "bps" => "brake_hydraulics", _ => "pot" };
    if !n.world_kind.is_empty() && n.world_kind != world_kind {
        return err("nominal.world_kind", "commande mécanique incompatible");
    }
    for (name, range) in &n.world_ranges {
        let valid_name = match f.mission.as_str() { "wsf" => ["speed_mps", "steering_deg"].contains(&name.as_str()),
            "sas" => name == "angle_deg", "bps" => name == "pressure_bar", _ => false };
        if !valid_name || range.iter().any(|x| !x.is_finite()) || range[0] > range[1] {
            return err("nominal.world_ranges", "plage de commande invalide");
        }
    }
    if source_mission != "apps" && n.world_ranges.is_empty() { return err("nominal.world_ranges", "commande mécanique manquante"); }
    let nominal = Nominal {
        world_ranges: n.world_ranges.clone(),
        cold_start_ms: n.cold_start_ms,
        nominal_ms: n.nominal_ms,
        hold_ms: n.hold_ms,
        ramp_ms: n.ramp_ms,
        segment_ms: n.segment_ms,
        position_quantum: n.position_quantum,
    };
    let compare = CompareParams {
        transition_tolerance_ms: f.compare.transition_tolerance_ms as u64,
        value_tolerance_lsb: positive("compare.value_tolerance_lsb", f.compare.value_tolerance_lsb)?,
        max_listed_mismatches: f.compare.max_listed_mismatches,
    };

    Ok(PackData {
        mission: f.mission,
        bench_json,
        bench_ecu: f.bench_ecu,
        bench_sensor: f.bench_sensor,
        adc_channels: f.adc_channels,
        tick_period_ms: f.tick_period_ms,
        jitter_us,
        hyperperiod_us: hyper_us,
        min_spacing_us: spacing_us,
        min_margin_us: margin_us,
        power,
        frames,
        sources,
        source_ecu,
        background: Background { world, vehicle, codes, vcu_inputs, static_ecus },
        consumer,
        slot_count: slots.len(),
        slots,
        nominal,
        compare,
        jitter_seed,
        student,
        raw: serde_json::Value::Null,
    })
}

fn validate_frame(p: &str, ff: &FrameFile, enums: &BTreeMap<String, BTreeMap<String, i64>>) -> Result<Frame, String> {
    if ff.id.0 < 0 || ff.id.0 > 0x7EF {
        return err(&format!("{p}.id"), "identifiant 11 bits hors de 0x000…0x7EF");
    }
    if ff.dlc == 0 || ff.dlc > 8 {
        return err(&format!("{p}.dlc"), "DLC hors de 1…8");
    }
    let period_us = ms_to_us(&format!("{p}.period_ms"), ff.period_ms)?;
    let phase_us = ms_to_us(&format!("{p}.phase_ms"), ff.phase_ms)?;
    if period_us == 0 || phase_us >= period_us {
        return err(&format!("{p}.phase_ms"), "phase hors de [0 ; période[");
    }
    let mixed = match ff.mode.as_deref() {
        None | Some("periodic") => false,
        Some("mixed") => true,
        Some(_) => return err(&format!("{p}.mode"), "mode d'émission inconnu"),
    };
    if let Some(m) = ff.mdt_ms {
        finite(&format!("{p}.mdt_ms"), m)?;
    }
    let nbits = ff.dlc as u16 * 8;
    let mut used = vec![false; nbits as usize];
    let mut signals = Vec::with_capacity(ff.signals.len());
    for (si, sf) in ff.signals.iter().enumerate() {
        let sp = format!("{p}.signals[{si}]");
        let order = match sf.order.as_deref() {
            None | Some("intel") => ByteOrder::Intel,
            Some("motorola") => ByteOrder::Motorola,
            Some(_) => return err(&format!("{sp}.order"), "ordre inconnu"),
        };
        if sf.len == 0 || sf.len > 32 {
            return err(&format!("{sp}.len"), "longueur hors de 1…32");
        }
        let positions = codec::positions(sf.start, sf.len, order);
        for &b in &positions {
            if b >= nbits {
                return err(&format!("{sp}.start"), "signal hors de la trame");
            }
            if used[b as usize] {
                return err(&format!("{sp}.start"), "signal qui chevauche un autre signal");
            }
            used[b as usize] = true;
        }
        let role = match sf.source.as_str() {
            vocab::HDR_CRC => Role::Crc,
            vocab::HDR_COUNTER => Role::Counter,
            vocab::HDR_ALIVE => Role::Alive,
            _ => Role::Value,
        };
        let factor = sf.factor.unwrap_or(1.0);
        if !factor.is_finite() || factor == 0.0 {
            return err(&format!("{sp}.factor"), "facteur invalide");
        }
        let offset = finite(&format!("{sp}.offset"), sf.offset.unwrap_or(0.0))?;
        let (lo, hi) = codec::raw_bounds(sf.len, sf.signed);
        let in_bounds = |v: i64| v >= lo && v <= hi;
        let min = sf.min.map(|n| n.0).unwrap_or(lo);
        let max = sf.max.map(|n| n.0).unwrap_or(hi);
        if !in_bounds(min) || !in_bounds(max) || min > max {
            return err(&format!("{sp}.min"), "plage brute invalide");
        }
        if role == Role::Value && (sf.min.is_none() || sf.max.is_none()) {
            return err(&format!("{sp}.max"), "plage émise (min, max) obligatoire");
        }
        let sna = match sf.sna {
            None => None,
            Some(n) if in_bounds(n.0) => Some(n.0),
            Some(_) => return err(&format!("{sp}.sna"), "SNA non représentable"),
        };
        if let Some(e) = &sf.enumeration {
            if !enums.contains_key(e) {
                return err(&format!("{sp}.enum"), "énumération absente de `enums`");
            }
        }
        if role != Role::Value && (sf.signed || order != ByteOrder::Intel) {
            return err(&format!("{sp}.order"), "en-tête Intel non signé attendu");
        }
        signals.push(Signal {
            role,
            positions,
            signed: sf.signed,
            factor,
            offset,
            min,
            max,
            sna,
            substitute: sf.substitute,
            qualifier: None,
            source: sf.source.clone(),
        });
    }
    // qualifiers refer to a signal of the same frame
    for (si, sf) in ff.signals.iter().enumerate() {
        if let Some(q) = &sf.qualifier {
            let idx = ff.signals.iter().position(|o| o.source == q.source);
            match idx {
                Some(i) if i != si && signals[i].role == Role::Value => signals[si].qualifier = Some((i, q.ok.0)),
                _ => return err(&format!("{p}.signals[{si}].qualifier"), "signal qualificatif absent de la trame"),
            }
        }
    }
    let count = |r: Role| signals.iter().filter(|s| s.role == r).count();
    let protection = match &ff.protection {
        ProtectionFile::P22 { data_ids } => {
            if ff.dlc != 8 {
                return err(&format!("{p}.dlc"), "le profil P22 retenu exige DLC 8");
            }
            if data_ids.len() != 16 {
                return err(&format!("{p}.protection.data_ids"), "16 Data ID attendus");
            }
            let mut ids = [0u8; 16];
            for (i, d) in data_ids.iter().enumerate() {
                if d.0 < 0 || d.0 > 255 {
                    return err(&format!("{p}.protection.data_ids[{i}]"), "octet attendu");
                }
                ids[i] = d.0 as u8;
            }
            let crc = signals.iter().position(|s| s.role == Role::Crc);
            let cnt = signals.iter().position(|s| s.role == Role::Counter);
            let ok = count(Role::Crc) == 1
                && count(Role::Counter) == 1
                && count(Role::Alive) == 0
                && crc.is_some_and(|i| signals[i].positions == codec::positions(0, 8, ByteOrder::Intel))
                && cnt.is_some_and(|i| signals[i].positions == codec::positions(8, 4, ByteOrder::Intel));
            if !ok {
                return err(&format!("{p}.signals"), "en-tête P22 attendu (CRC octet 0, compteur quartet bas de l'octet 1)");
            }
            Protection::P22 { data_ids: ids }
        }
        ProtectionFile::Alive { bits } => {
            if *bits != 4 && *bits != 8 {
                return err(&format!("{p}.protection.bits"), "compteur de vie de 4 ou 8 bits");
            }
            let a = signals.iter().position(|s| s.role == Role::Alive);
            let ok = count(Role::Alive) == 1
                && count(Role::Crc) == 0
                && count(Role::Counter) == 0
                && a.is_some_and(|i| signals[i].positions.len() == *bits as usize);
            if !ok {
                return err(&format!("{p}.signals"), "un seul compteur de vie de la largeur déclarée");
            }
            Protection::Alive { bits: *bits, signal: a.unwrap_or(0) }
        }
        ProtectionFile::None => {
            if count(Role::Crc) + count(Role::Counter) + count(Role::Alive) != 0 {
                return err(&format!("{p}.signals"), "en-tête sur une trame sans protection");
            }
            Protection::None
        }
    };
    Ok(Frame {
        id: ff.id.0 as u32,
        emitter: ff.emitter.clone(),
        period_us,
        phase_us,
        dlc: ff.dlc,
        mixed,
        protection,
        signals,
    })
}

/// Scheduled emissions of all frames over one hyperperiod are pairwise at least `spacing_us` apart (ch. 01 §2.2).
fn check_spacing(frames: &[Frame], hyper_us: u64, spacing_us: u64) -> Result<(), String> {
    let mut times: Vec<(u64, usize)> = Vec::new();
    for (fi, f) in frames.iter().enumerate() {
        let mut t = f.phase_us;
        while t < hyper_us {
            times.push((t, fi));
            t += f.period_us;
        }
    }
    times.sort();
    for w in 0..times.len() {
        let (t0, f0) = times[w];
        let (t1, f1) = if w + 1 < times.len() { times[w + 1] } else { (times[0].0 + hyper_us, times[0].1) };
        if times.len() > 1 && t1 - t0 < spacing_us {
            let fi = f0.max(f1);
            return err(&format!("frames[{fi}].phase_ms"), "deux émissions planifiées à moins de l'écart minimal");
        }
    }
    Ok(())
}
