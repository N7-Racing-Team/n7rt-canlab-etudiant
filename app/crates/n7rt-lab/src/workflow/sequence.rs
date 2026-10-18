//! Hidden sequences of the incident steps (private spec ch. 24 §2.3–§2.8), instructor edition, generic: V-0 cold
//! start, V-A without anomaly, V-B with the anomaly (switched on at a drawn instant), V-C without it again. The
//! sensor follows piecewise linear paths whose values are drawn in the truth file's ranges; the anomaly acts on the
//! virtual reference bench (wire defect, setting) or on the vehicle (frame corruption, source parameters), never on
//! the decoys. Same seed, same frames.

use std::collections::BTreeMap;

use n7rt_circuit::{persist, Value};
use n7rt_vehicle::{BusFrame, FrameFault, Pack, Vehicle};
use serde_json::Value as Json;

use crate::phys::{PhysCommand, SystemOp, WireDefect};
use crate::workflow::bench::VirtualBench;
use crate::workflow::incident::PinName;

fn bad(what: &str) -> String {
    format!("séquence cachée invalide : {what}")
}

/// Deterministic draws (SplitMix64) from a derived seed.
pub struct Draw(u64);

impl Draw {
    pub fn new(seed: u64) -> Draw {
        Draw(seed)
    }

    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in [lo, hi].
    pub fn uniform(&mut self, lo: f64, hi: f64) -> f64 {
        let u = (self.next() >> 11) as f64 / (1u64 << 53) as f64;
        lo + (hi - lo) * u
    }
}

/// A value of the truth file: a number, a range `[lo, hi]` drawn once, or `null` (the value reached so far).
fn value(v: &Json, draw: &mut Draw, previous: f64) -> Result<f64, String> {
    match v {
        Json::Null => Ok(previous),
        Json::Number(n) => n.as_f64().filter(|x| x.is_finite()).ok_or_else(|| bad("valeur")),
        Json::Array(r) if r.len() == 2 => {
            let (lo, hi) = (r[0].as_f64().ok_or_else(|| bad("plage"))?, r[1].as_f64().ok_or_else(|| bad("plage"))?);
            if !(lo.is_finite() && hi.is_finite() && lo <= hi) {
                return Err(bad("plage"));
            }
            Ok(draw.uniform(lo, hi))
        }
        _ => Err(bad("valeur")),
    }
}

/// A sequence drawn for one hidden test: per-millisecond command of the main quantity, the instants of the phases.
pub struct Drawn {
    pub quantity: String,
    /// (absolute ms, value) waypoints, increasing in time.
    pub points: Vec<(u64, f64)>,
    pub duration_ms: u64,
    /// Start of V-B + the drawn anomaly instant, and start of V-C.
    pub anomaly_on_ms: u64,
    pub anomaly_off_ms: u64,
    /// Starts of the phases (V-0, V-A, V-B, V-C).
    pub phases: Vec<(String, u64)>,
}

impl Drawn {
    /// The command at `ms` (linear between waypoints, held after the last one).
    pub fn at(&self, ms: u64) -> f64 {
        let i = self.points.partition_point(|(t, _)| *t <= ms);
        match (i.checked_sub(1).map(|j| self.points[j]), self.points.get(i)) {
            (Some((t0, v0)), Some(&(t1, v1))) if t1 > t0 => v0 + (v1 - v0) * (ms - t0) as f64 / (t1 - t0) as f64,
            (Some((_, v0)), _) => v0,
            (None, Some(&(_, v1))) => v1,
            (None, None) => 0.0,
        }
    }
}

/// Draws the sequence of the truth file (`{"quantity", "segments": [{"phase", "ms", "path": [[t, v]…],
/// "anomaly_at_ms"?}…]}`, phases V-0, V-A, V-B, V-C in order) starting from `rest`. Times are relative to the start
/// of their segment; each segment starts from the value reached before it.
pub fn draw(sequence: &Json, rest: f64, draw: &mut Draw) -> Result<Drawn, String> {
    let quantity = sequence["quantity"].as_str().ok_or_else(|| bad("grandeur"))?.to_owned();
    let segments = sequence["segments"].as_array().ok_or_else(|| bad("segments"))?;
    let names: Vec<&str> = segments.iter().filter_map(|s| s["phase"].as_str()).collect();
    if names != ["V-0", "V-A", "V-B", "V-C"] {
        return Err(bad("phases V-0, V-A, V-B, V-C attendues"));
    }
    let mut points = vec![(0u64, rest)];
    let mut start = 0u64;
    let mut phases = Vec::new();
    let (mut on, mut off) = (0, 0);
    for s in segments {
        let ms = s["ms"].as_u64().filter(|m| *m > 0 && *m <= 60_000).ok_or_else(|| bad("durée"))?;
        let phase = s["phase"].as_str().unwrap_or_default();
        phases.push((phase.to_owned(), start));
        // a segment starts from the value reached at the end of the previous one (held until its first point)
        if let Some(&(t, v)) = points.last() {
            if t < start {
                points.push((start, v));
            }
        }
        for p in s["path"].as_array().ok_or_else(|| bad("trajectoire"))? {
            let t = p[0].as_u64().filter(|t| *t <= ms).ok_or_else(|| bad("instant"))?;
            let previous = points.last().map(|x| x.1).unwrap_or(rest);
            let v = value(&p[1], draw, previous)?;
            let at = start + t;
            if points.last().is_some_and(|(last, _)| *last > at) {
                return Err(bad("instants décroissants"));
            }
            points.push((at, v));
        }
        if phase == "V-B" {
            let t = match &s["anomaly_at_ms"] {
                Json::Null => 0.0,
                v => value(v, draw, 0.0)?,
            };
            on = start + (t.round().max(0.0) as u64).min(ms - 1);
        }
        if phase == "V-C" {
            off = start;
        }
        start += ms;
    }
    Ok(Drawn { quantity, points, duration_ms: start, anomaly_on_ms: on, anomaly_off_ms: off, phases })
}

/// The anomaly as the hidden sequence applies it during V-B.
#[derive(Clone, Debug)]
pub enum SeqAnomaly {
    Wire { a: PinName, b: PinName, defect: WireDefect },
    Setting { designator: String, param: String, value: f64 },
    Frame(FrameFault),
    Source(Pack),
    /// The healthy rendering of the same draw (generation assertions).
    None,
}

/// Renders the drawn sequence on the reference bench of the student projection `student`.
pub fn render(student: &Pack, drawn: &Drawn, anomaly: &SeqAnomaly, vehicle_seed: u64, sensor_seed: u64) -> Result<Vec<BusFrame>, String> {
    let circuit = persist::load(student.bench_json()).map_err(|_| bad("banc de référence illisible"))?;
    let mut bench = VirtualBench::new(circuit, student, sensor_seed)?;
    let mut rest = bench.sensor_state();
    if !rest.contains_key(&drawn.quantity) {
        return Err(bad("grandeur absente du capteur"));
    }
    rest.insert(drawn.quantity.clone(), drawn.at(0));
    bench.set_sensor(&rest)?;
    let mut vehicle = Vehicle::new(student, vehicle_seed);
    let mut last = None;
    let mut each = |ms: u64, b: &mut VirtualBench| -> Result<(), String> {
        let v = (drawn.at(ms) * 1e6).round() / 1e6;
        if last == Some(v.to_bits()) {
            return Ok(());
        }
        last = Some(v.to_bits());
        b.set_sensor(&[(drawn.quantity.clone(), v)].into_iter().collect::<BTreeMap<_, _>>())
    };
    let mut frames = bench.drive(&mut vehicle, drawn.anomaly_on_ms * 1000, &mut each)?;
    // on: the bench or the vehicle
    let reference_value = match anomaly {
        SeqAnomaly::Wire { a, b, defect } => {
            let (a, b) = (bench.pin(a).ok_or_else(|| bad("broche"))?, bench.pin(b).ok_or_else(|| bad("broche"))?);
            bench.apply(&PhysCommand::System { op: SystemOp::AttachDefect { a, b, defect: *defect } })?;
            None
        }
        SeqAnomaly::Setting { designator, param, value } => {
            let comp = bench.component(designator).ok_or_else(|| bad("composant"))?;
            let before = bench.core().document().circuit().components()[&comp].params.get(param).cloned().ok_or_else(|| bad("paramètre"))?;
            bench.apply(&PhysCommand::System { op: SystemOp::SetParam { comp, name: param.clone(), value: Value::Num(*value) } })?;
            Some((comp, before))
        }
        SeqAnomaly::Frame(f) => {
            vehicle.set_frame_fault(Some(*f))?;
            None
        }
        SeqAnomaly::Source(p) => {
            vehicle.reconfigure_source(p)?;
            None
        }
        SeqAnomaly::None => None,
    };
    frames.extend(bench.drive(&mut vehicle, drawn.anomaly_off_ms * 1000, &mut each)?);
    // off
    match anomaly {
        SeqAnomaly::Wire { .. } => bench.apply(&PhysCommand::System { op: SystemOp::ClearDefects })?,
        SeqAnomaly::Setting { param, .. } => {
            if let Some((comp, before)) = reference_value {
                bench.apply(&PhysCommand::System { op: SystemOp::SetParam { comp, name: param.clone(), value: before } })?;
            }
        }
        SeqAnomaly::Frame(_) => vehicle.set_frame_fault(None)?,
        SeqAnomaly::Source(_) => vehicle.reconfigure_source(student)?,
        SeqAnomaly::None => {}
    }
    frames.extend(bench.drive(&mut vehicle, drawn.duration_ms * 1000, &mut each)?);
    Ok(frames)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draws_are_deterministic_and_in_range() {
        let mut a = Draw::new(7);
        let mut b = Draw::new(7);
        for _ in 0..1000 {
            let (x, y) = (a.uniform(2.0, 3.0), b.uniform(2.0, 3.0));
            assert_eq!(x, y);
            assert!((2.0..=3.0).contains(&x));
        }
    }

    #[test]
    fn a_sequence_is_drawn_from_its_segments() {
        let seq = serde_json::json!({"quantity": "q", "segments": [
            {"phase": "V-0", "ms": 1000, "path": [[0, 0.0]]},
            {"phase": "V-A", "ms": 2000, "path": [[1000, [10.0, 20.0]], [2000, 0.0]]},
            {"phase": "V-B", "ms": 1000, "path": [[200, 5.0]], "anomaly_at_ms": [100, 300]},
            {"phase": "V-C", "ms": 1000, "path": [[500, null]]}]});
        let d = draw(&seq, 0.0, &mut Draw::new(3)).unwrap();
        assert_eq!(d.duration_ms, 5000);
        assert!((3100..=3300).contains(&d.anomaly_on_ms), "{}", d.anomaly_on_ms);
        assert_eq!(d.anomaly_off_ms, 4000);
        let peak = d.at(2000);
        assert!((10.0..=20.0).contains(&peak));
        assert!((d.at(1500) - peak / 2.0).abs() < 1e-9, "linear ramp");
        assert_eq!(d.at(3200), 5.0);
        assert_eq!(d.at(4800), 5.0, "null holds the value reached");
        assert_eq!(d.phases.iter().map(|p| p.1).collect::<Vec<_>>(), [0, 1000, 3000, 4000]);
        let mut wrong = seq.clone();
        wrong["segments"][1]["phase"] = "V-X".into();
        assert!(draw(&wrong, 0.0, &mut Draw::new(3)).is_err());
    }
}
