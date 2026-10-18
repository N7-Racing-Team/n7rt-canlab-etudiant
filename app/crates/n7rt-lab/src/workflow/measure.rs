//! Key measure of an incident (private spec ch. 24 §3.4, « contrôle de la mesure clé »), generic. The expected value
//! is computed by the engine at the conditions the student recorded: on the evidence frames for a frame quantity, on
//! a virtual copy of the recorded circuit with the anomaly for a voltage. The same quantity without the anomaly, on
//! the same circuit brought from the bench's rest to the recorded conditions by a ramp, tells whether the measure was
//! taken where the anomaly shows (pertinence). These values exist only in memory, during a verification.

use std::collections::BTreeMap;

use n7rt_circuit::{persist, Circuit, Edit, Value, WireId};
use n7rt_vehicle::{e2e, BusFrame, FrameFault, Pack, Vehicle};
use serde::{Deserialize, Serialize};

use crate::phys::{PhysCommand, SystemOp, WireDefect};
use crate::workflow::bench::VirtualBench;
use crate::workflow::incident::MeasureSpec;

/// Window of a rate or of a mean value.
const WINDOW_US: u64 = 1_000_000;
/// Frames of a proportion.
const PROPORTION_FRAMES: usize = 100;
/// Physics before a voltage is read, then frames averaged (the multimeter's integration).
const SETTLE_MS: u64 = 500;
const METER_FRAMES: u64 = 100;
/// Ramp from the bench's rest to the recorded conditions, then hold, for the value without the anomaly.
const RAMP_MS: u64 = 1_000;
const HOLD_MS: u64 = 2_500;
const SEED: u64 = 0x6D65_6173_7572_6531;

/// One raw frame of the evidence (bytes in lowercase hexadecimal).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreuveFrame {
    pub t_us: u64,
    pub id: u32,
    #[serde(default)]
    pub flags: u8,
    pub data: String,
}

/// What « Consigner la mesure » records besides the conditions: the raw frames of the last seconds and the circuit
/// (canonical text, interaction states included). Kept sealed on disk: the circuit holds the hidden parameters of
/// the mission parts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preuve {
    pub frames: Vec<PreuveFrame>,
    pub circuit: String,
}

/// The incident's anomaly as a virtual copy applies or removes it.
#[derive(Debug, Clone)]
pub enum Anomaly {
    /// The wire of the recorded circuit that carried the defect at the recording (`None`: no wire did).
    Wire { wire: Option<WireId>, defect: WireDefect },
    Frame(FrameFault),
    /// The setting of the incident and the reference value of its parameter (the value without the anomaly).
    Setting { designator: String, param: String, reference: Value },
    /// The source ECU with the incident's parameters (the pack without them is the base pack).
    Source,
}

fn hex(text: &str) -> Option<Vec<u8>> {
    if text.len() % 2 != 0 || text.len() > 128 {
        return None;
    }
    (0..text.len()).step_by(2).map(|i| u8::from_str_radix(&text[i..i + 2], 16).ok()).collect()
}

/// The evidence frames as bus frames, in time order; `None` when one does not read.
pub fn frames_of(preuve: &Preuve) -> Option<Vec<BusFrame>> {
    let mut out: Vec<BusFrame> = preuve
        .frames
        .iter()
        .map(|f| hex(&f.data).map(|data| BusFrame { t_us: f.t_us, id: f.id, flags: f.flags, dlc: data.len() as u8, data }))
        .collect::<Option<_>>()?;
    out.sort_by_key(|f| (f.t_us, f.id));
    Some(out)
}

/// A frame quantity on `frames` (decoded with `pack`), taken at the time of the last frame.
pub fn frame_quantity(spec: &MeasureSpec, frames: &[BusFrame], pack: &Pack) -> Option<f64> {
    let t_end = frames.last()?.t_us;
    let of = |id: u32| frames.iter().filter(move |f| f.id == id);
    let recent = |id: u32| of(id).filter(move |f| f.t_us + WINDOW_US > t_end);
    match spec {
        MeasureSpec::Proportion { id, .. } => {
            let ids = pack.p22_data_ids(*id)?;
            let all: Vec<&BusFrame> = of(*id).collect();
            if all.len() < PROPORTION_FRAMES {
                return None;
            }
            let last = &all[all.len() - PROPORTION_FRAMES..];
            let good = last.iter().filter(|f| f.data.len() >= 2 && f.data[0] == e2e::p22_crc(&f.data, ids[(f.data[1] & 0x0F) as usize])).count();
            Some(100.0 * good as f64 / PROPORTION_FRAMES as f64)
        }
        MeasureSpec::Rate { id, .. } => Some(recent(*id).count() as f64),
        MeasureSpec::Increment { id, modulo, .. } => {
            let all: Vec<&BusFrame> = of(*id).collect();
            let [.., a, b] = all.as_slice() else { return None };
            let (ra, rb) = (pack.decode_raw(*id, &a.data).ok()?, pack.decode_raw(*id, &b.data).ok()?);
            let counter = |r: &BTreeMap<String, i64>| r.get("e2e.counter").or_else(|| r.get("alive")).copied();
            Some((counter(&rb)? - counter(&ra)?).rem_euclid(*modulo) as f64)
        }
        MeasureSpec::Raw { id, source, .. } => {
            let last = of(*id).last()?;
            pack.decode_raw(*id, &last.data).ok()?.get(source).map(|v| *v as f64)
        }
        MeasureSpec::Value { id, source, .. } => {
            let values: Vec<f64> = recent(*id).filter_map(|f| pack.decode_frame(*id, &f.data).ok()?.get(source).copied().flatten()).collect();
            (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
        }
        MeasureSpec::Voltage { .. } => None,
    }
}

/// The sensor state of the reference bench of `pack` (the mission's rest).
fn rest(pack: &Pack) -> Option<BTreeMap<String, f64>> {
    let circuit = persist::load(pack.bench_json()).ok()?;
    Some(VirtualBench::new(circuit, pack, SEED).ok()?.sensor_state())
}

/// Removes the anomaly from a virtual copy (settings back to their reference value); a wire defect is simply not
/// put on it.
fn without(bench: &mut VirtualBench, anomaly: &Anomaly) -> Option<()> {
    if let Anomaly::Setting { designator, param, reference } = anomaly {
        let comp = bench.component(designator)?;
        let rev = bench.core().document().rev();
        let edit = Edit::SetParam { comp, name: param.clone(), value: reference.clone() };
        bench.apply(&PhysCommand::Edit { base_rev: rev, edit }).ok()?;
    }
    Some(())
}

/// Mean voltage of `spec` on `circuit` at its recorded conditions, with or without the anomaly.
fn voltage(spec: &MeasureSpec, circuit: Circuit, pack: &Pack, anomaly: &Anomaly, with: bool) -> Option<f64> {
    let MeasureSpec::Voltage { pin, reference, .. } = spec else { return None };
    let mut bench = VirtualBench::new(circuit, pack, SEED).ok()?;
    match (anomaly, with) {
        (Anomaly::Wire { wire: Some(wire), defect }, true) => {
            bench.apply(&PhysCommand::System { op: SystemOp::DefectOn { wire: *wire, defect: *defect } }).ok()?;
        }
        (_, false) => without(&mut bench, anomaly)?,
        _ => {}
    }
    let (a, b) = (bench.pin(pin)?, bench.pin(reference)?);
    bench.settle(SETTLE_MS);
    bench.mean_voltage(&a, &b, METER_FRAMES)
}

/// The vehicle without the anomaly on `circuit`, its sensor ramped from the bench's rest to the recorded state.
fn healthy_frames(circuit: Circuit, base: &Pack, anomaly: &Anomaly) -> Option<Vec<BusFrame>> {
    let start = rest(base)?;
    let mut bench = VirtualBench::new(circuit, base, SEED).ok()?;
    without(&mut bench, anomaly)?;
    let target = bench.sensor_state();
    let mut vehicle = Vehicle::new(base, SEED);
    let total_us = (RAMP_MS + HOLD_MS) * 1000;
    bench
        .drive(&mut vehicle, total_us, &mut |ms, b| {
            if ms > RAMP_MS {
                return Ok(());
            }
            let k = ms as f64 / RAMP_MS as f64;
            let values: BTreeMap<String, f64> = target
                .iter()
                .map(|(q, to)| (q.clone(), start.get(q).copied().unwrap_or(0.0) * (1.0 - k) + to * k))
                .collect();
            b.set_sensor(&values)
        })
        .ok()
}

/// (expected value, value without the anomaly) of `spec` for the recorded evidence, in the canonical unit of the
/// spec; `None` when the evidence does not allow the computation (too few frames, unreadable circuit…). `base` is the
/// student pack without the incident's source parameters.
pub fn evaluate(spec: &MeasureSpec, preuve: &Preuve, base: &Pack, anomaly: &Anomaly) -> Option<(f64, f64)> {
    let circuit = persist::load(&preuve.circuit).ok()?;
    if let MeasureSpec::Voltage { .. } = spec {
        let expected = voltage(spec, circuit.clone(), base, anomaly, true)?;
        let healthy = voltage(spec, circuit, base, anomaly, false)?;
        return Some((expected, healthy));
    }
    let expected = frame_quantity(spec, &frames_of(preuve)?, base)?;
    let healthy = frame_quantity(spec, &healthy_frames(circuit, base, anomaly)?, base)?;
    Some((expected, healthy))
}
