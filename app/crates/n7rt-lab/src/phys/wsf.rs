//! Wheel sensor timing (E9) and raw electrical measurements for the parameterized WSF source.
//! The caller computes the two static operating points through L2. No status, CAN layout or expected slot lives here.

use std::f64::consts::PI;
use n7rt_circuit::{Circuit, CompId, Component, Kind};
use n7rt_solver::SupplyMode;
use n7rt_vehicle::{WsfEdge, WsfLaneInputs};
use super::lower::{param_choice, param_int, param_num, state_num};
use super::probe::Probe;
use super::Reading;

#[derive(Debug, Clone)]
pub(crate) struct SensorParams {
    pub teeth_count: u32,
    pub duty: f64,
    pub power_on_ms: f64,
    pub reset_below_v: f64,
    pub vdd_min: f64,
    pub vdd_max: f64,
    pub i_low: f64,
    pub i_high: f64,
    pub r_reverse: f64,
    pub air_gap_mm: f64,
    pub gap_nominal_mm: f64,
    pub gap_failure_mm: f64,
}

impl SensorParams {
    pub fn from_component(c: &Component) -> Self {
        let corner=param_num(c,"tolerance_corner").clamp(-1.0,1.0);
        let tolerance=|nominal:&str,min:&str,max:&str| {
            let nominal=param_num(c,nominal);
            let edge=param_num(c,if corner<0.0 {min}else{max});
            nominal+(edge-nominal)*corner.abs()
        };
        Self { teeth_count: param_int(c, "teeth_count").max(0) as u32,
            duty: tolerance("duty","duty_min","duty_max"), power_on_ms: param_num(c, "power_on_ms"),
            reset_below_v: param_num(c, "reset_below_v"), vdd_min: param_num(c, "vdd_min"),
            vdd_max: param_num(c, "vdd_max"), i_low: tolerance("i_low","i_low_min","i_low_max"), i_high: tolerance("i_high","i_high_min","i_high_max"),
            r_reverse: param_num(c, "r_reverse"), air_gap_mm: param_num(c, "air_gap_mm"),
            gap_nominal_mm: param_num(c, "gap_nominal_mm"), gap_failure_mm: param_num(c, "gap_failure_mm") }
    }
    pub fn valid(&self) -> bool {
        [self.duty, self.power_on_ms, self.reset_below_v, self.vdd_min, self.vdd_max,
            self.i_low, self.i_high, self.r_reverse, self.air_gap_mm, self.gap_nominal_mm, self.gap_failure_mm]
            .iter().all(|v| v.is_finite()) && self.teeth_count > 0 && self.teeth_count <= 100_000
            && self.duty > 0.0 && self.duty < 1.0 && self.power_on_ms >= 0.0
            && self.vdd_min > 0.0 && self.vdd_max > self.vdd_min && self.reset_below_v <= self.vdd_min
            && self.i_low > 0.0 && self.i_high > self.i_low && self.r_reverse > 0.0
            && self.gap_failure_mm > self.gap_nominal_mm
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct LaneMeasurement {
    pub low_v_rs: Option<f64>, pub high_v_rs: Option<f64>,
    pub low_supply_v: Option<f64>, pub high_supply_v: Option<f64>,
    pub low_vdd: Option<f64>, pub high_vdd: Option<f64>,
    pub low_limited: bool, pub high_limited: bool,
}

fn valid(r: Reading<f64>) -> Option<f64> { match r { Reading::Valid { v } if v.is_finite() => Some(v), _ => None } }

/// `rail_element` identifies the L2 current-limited follower, not a mission frame or signal.
pub(crate) fn measure(low: &Probe<'_>, high: &Probe<'_>, ecu: CompId, sensor: CompId,
    lane: usize, rail_element: &str) -> LaneMeasurement {
    let (sup, input) = if lane == 0 { ("ws_l_sup", "ws_l_in") } else { ("ws_r_sup", "ws_r_in") };
    let limited = |p: &Probe<'_>| p.ctx.low.comps.get(&ecu).and_then(|c| c.elem(rail_element))
        .and_then(|e| p.res.modes.get(e.index)).copied().flatten() == Some(SupplyMode::Cc);
    LaneMeasurement { low_v_rs: valid(low.pins_between(ecu, input, "gnd")),
        high_v_rs: valid(high.pins_between(ecu, input, "gnd")),
        low_supply_v: valid(low.pins_between(ecu, sup, "gnd")),
        high_supply_v: valid(high.pins_between(ecu, sup, "gnd")),
        low_vdd: valid(low.pins_between(sensor, "vcc", "gnd")),
        high_vdd: valid(high.pins_between(sensor, "vcc", "gnd")),
        low_limited: limited(low), high_limited: limited(high) }
}

/// Computes only the physical wheel angular speed; the ECU is not given this result.
pub(crate) fn wheel_rpm(circuit: &Circuit, sensor: &Component) -> Option<f64> {
    let id = u32::try_from(param_int(sensor, "world_comp_id")).ok()?;
    let world = circuit.components().get(&CompId(id))?;
    if world.kind != Kind::FrontAxle { return None; }
    let v = state_num(circuit, world, "speed_mps");
    let delta = state_num(circuit, world, "steering_deg");
    let r = param_num(sensor, "r_eff_m"); let length = param_num(sensor, "wheelbase_m");
    let track = param_num(sensor, "track_front_m"); let ratio = param_num(sensor, "steer_ratio");
    if [v, delta, r, length, track, ratio].iter().any(|v| !v.is_finite()) || r <= 0.0 || length <= 0.0 || ratio <= 0.0 { return None; }
    let turn = (delta / ratio * PI / 180.0).tan() * track / (2.0 * length);
    let factor = match param_choice(sensor, "wheel_side") { "left" => 1.0 - turn, "right" => 1.0 + turn, _ => return None };
    if factor < 0.0 || !factor.is_finite() { return None; }
    Some(v.abs() / r * 30.0 / PI * factor)
}

fn mix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}
fn skip_tooth(seed: u64, sensor: CompId, tooth: u64, probability: f64) -> bool {
    let unit = (mix(mix(seed) ^ mix(sensor.0 as u64) ^ mix(tooth)) >> 11) as f64 / (1u64 << 53) as f64;
    unit < probability
}

#[derive(Debug, Clone)]
pub(crate) struct RotorState {
    phase: f64, tooth: u64, accepted_tooth: bool,
    pub current_high: bool,
    powered_at_ns: Option<u64>, last_end_ns: Option<u64>,
}
impl Default for RotorState {
    fn default() -> Self { Self { phase: 0.0, tooth: 0, accepted_tooth: true, current_high: true,
        powered_at_ns: None, last_end_ns: None } }
}

impl RotorState {
    /// Integrates tooth angle, emits transitions only after physical power-on, and preserves phase at standstill.
    /// The input is a physical world quantity; the output contains exclusively transitions and raw L2 readings.
    pub fn sample(&mut self, params: &SensorParams, measurement: LaneMeasurement, rpm: Option<f64>,
        sensor: CompId, start_ns: u64, end_ns: u64, seed: u64) -> WsfLaneInputs {
        if end_ns < start_ns || self.last_end_ns.is_some_and(|t| start_ns < t) { *self = Self::default(); }
        self.last_end_ns = Some(end_ns);
        let current_vdd = if self.current_high { measurement.high_vdd } else { measurement.low_vdd };
        let powered = params.valid() && current_vdd.is_some_and(|v| v.is_finite() && v >= params.reset_below_v);
        if !powered {
            self.powered_at_ns = None; self.phase = 0.0; self.tooth = 0; self.current_high = true;
        } else if self.powered_at_ns.is_none() {
            self.powered_at_ns = Some(start_ns); self.phase = 0.0; self.tooth = 0; self.current_high = true;
            let probability = ((params.air_gap_mm - params.gap_nominal_mm)
                / (params.gap_failure_mm - params.gap_nominal_mm)).clamp(0.0, 1.0);
            self.accepted_tooth = !skip_tooth(seed, sensor, self.tooth, probability);
        }
        let began_high = self.current_high;
        let mut edges = Vec::new();
        let ready_at = self.powered_at_ns.map(|t| t.saturating_add((params.power_on_ms * 1_000_000.0).round().max(0.0) as u64));
        let begin = ready_at.map_or(end_ns, |t| start_ns.max(t)).min(end_ns);
        let frequency = rpm.filter(|v| v.is_finite() && *v >= 0.0).unwrap_or(0.0) * params.teeth_count as f64 / 60.0;
        if powered && frequency > 0.0 && frequency.is_finite() {
            let duration = end_ns.saturating_sub(begin) as f64;
            let period = 1_000_000_000.0 / frequency;
            let probability = ((params.air_gap_mm - params.gap_nominal_mm)
                / (params.gap_failure_mm - params.gap_nominal_mm)).clamp(0.0, 1.0);
            let mut elapsed = 0.0;
            let mut transitions = 0usize;
            // Valid catalog bounds yield only a few edges per 1 ms. The cap also rejects hostile internal input.
            while elapsed < duration {
                transitions += 1;
                if transitions > 10_000 { return WsfLaneInputs::default(); }
                let boundary = if self.phase < params.duty { params.duty } else { 1.0 };
                let dt = ((boundary - self.phase) * period).max(0.0);
                if elapsed + dt > duration { self.phase += (duration - elapsed) / period; break; }
                elapsed += dt;
                let target_high = boundary == 1.0;
                if target_high {
                    self.phase = 0.0; self.tooth = self.tooth.wrapping_add(1);
                    self.accepted_tooth = !skip_tooth(seed, sensor, self.tooth, probability);
                } else { self.phase = params.duty; }
                if self.accepted_tooth && self.current_high != target_high {
                    self.current_high = target_high;
                    let t_ns = begin.saturating_add(elapsed.round().max(0.0) as u64).min(end_ns);
                    if edges.last().is_none_or(|e: &WsfEdge| e.t_ns < t_ns) {
                        edges.push(WsfEdge { t_ns, rising_current: target_high });
                    }
                }
            }
        }
        let present_high = began_high || self.current_high || edges.iter().any(|e| e.rising_current);
        let present_low = !began_high || !self.current_high || edges.iter().any(|e| !e.rising_current);
        WsfLaneInputs { sup_v: if self.current_high { measurement.high_supply_v } else { measurement.low_supply_v },
            low_sup_v: measurement.low_supply_v, high_sup_v: measurement.high_supply_v,
            low_v_rs: measurement.low_v_rs, high_v_rs: measurement.high_v_rs,
            present_low, present_high, limited: (present_low && measurement.low_limited) || (present_high && measurement.high_limited),
            sensor_powered: powered, edges }
    }
}

