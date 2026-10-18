//! Parameterized wheel-speed source ECU. Electrical plateaus and physical sensor edges are inputs;
//! this module never receives a wheel speed, a CAN identifier, a layout or consumer-side rules.

use std::collections::VecDeque;
use serde::Deserialize;

pub const SOURCES: [&str; 13] = [
    "wsf.ws_l", "wsf.ws_r", "wsf.ws_l_st", "wsf.ws_r_st", "wsf.still",
    "wsf.il_l", "wsf.ih_l", "wsf.il_r", "wsf.ih_r", "wsf.ecu_mode",
    "wsf.lv_uv", "wsf.sens_sup_flt", "wsf.v_lv",
];

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WsfParams {
    pub teeth_count: u32,
    pub capture_resolution_ns: u64,
    pub measure_window_ms: u64,
    pub stop_ms: u64,
    pub calibration_rises: u32,
    pub sensor_power_on_max_ms: u64,
    pub initial_valid_samples: u32,
    pub comparator_low_v: f64,
    pub comparator_high_v: f64,
    pub r_sense_ohm: f64,
    pub current_bands_ma: [f64; 4],
    pub supply_window_v: [f64; 2],
    pub supply_hysteresis_v: f64,
    pub debounce_fault_samples: u32,
    pub debounce_recover_samples: u32,
    pub max_speed_rpm: f64,
    pub sup_rise_samples: u32,
    pub sup_fall_samples: u32,
    pub sup_fault_persist_ms: u64,
    pub fault_recovery_ms: u64,
    pub lv_uv_on_v: f64,
    pub lv_uv_off_v: f64,
    pub lv_avg_samples: usize,
    pub status_enum: String,
    pub mode_enum: String,
    /// Resolved by the pack loader after status-code permutation: OK, INIT, OPEN, SHORT, CUR_FAULT, SUPPLY.
    #[serde(skip)]
    pub status_codes: [i64; 6],
    /// Resolved by the pack loader: INIT, RUN, DEGRADED, FAULT.
    #[serde(skip)]
    pub mode_codes: [i64; 4],
}

impl WsfParams {
    pub fn validate(&self) -> Result<(), String> {
        let numeric = [self.comparator_low_v, self.comparator_high_v, self.r_sense_ohm,
            self.supply_hysteresis_v, self.max_speed_rpm, self.lv_uv_on_v, self.lv_uv_off_v];
        if numeric.iter().chain(self.current_bands_ma.iter()).chain(self.supply_window_v.iter()).any(|v| !v.is_finite())
            || self.r_sense_ohm <= 0.0 || self.max_speed_rpm <= 0.0
            || self.comparator_low_v >= self.comparator_high_v
            || self.current_bands_ma[0] < 0.0 || self.current_bands_ma.windows(2).any(|w| w[0] >= w[1])
            || self.supply_window_v[0] >= self.supply_window_v[1]
            || self.supply_hysteresis_v < 0.0
            || 2.0 * self.supply_hysteresis_v >= self.supply_window_v[1] - self.supply_window_v[0]
            || self.lv_uv_on_v > self.lv_uv_off_v
            || self.teeth_count == 0 || self.teeth_count > 100_000
            || self.capture_resolution_ns == 0 || self.capture_resolution_ns > 1_000_000
            || self.measure_window_ms == 0 || self.measure_window_ms > 60_000
            || self.stop_ms < self.measure_window_ms || self.stop_ms > 60_000
            || self.calibration_rises == 0 || self.calibration_rises > 100_000
            || self.sensor_power_on_max_ms > 60_000
            || self.initial_valid_samples == 0 || self.initial_valid_samples > 60_000
            || self.debounce_fault_samples == 0 || self.debounce_fault_samples > 60_000
            || self.debounce_recover_samples == 0 || self.debounce_recover_samples > 60_000
            || self.sup_rise_samples == 0 || self.sup_rise_samples > 60_000
            || self.sup_fall_samples == 0 || self.sup_fall_samples > 60_000
            || self.lv_avg_samples == 0 || self.lv_avg_samples > 60_000
            || self.sup_fault_persist_ms > 60_000 || self.fault_recovery_ms > 60_000
            || self.status_enum.is_empty() || self.mode_enum.is_empty()
        { return Err("source_ecu.wsf : paramètres invalides".into()); }
        // Bound retained timestamps even for a deliberately hostile pack.
        if self.max_speed_rpm * self.teeth_count as f64 * self.measure_window_ms as f64 / 60_000.0 > 100_000.0 {
            return Err("source_ecu.wsf : fenêtre de capture trop grande".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WsfEdge { pub t_ns: u64, pub rising_current: bool }

#[derive(Debug, Clone, Default)]
pub struct WsfLaneInputs {
    pub sup_v: Option<f64>,
    pub low_sup_v: Option<f64>,
    pub high_sup_v: Option<f64>,
    pub low_v_rs: Option<f64>,
    pub high_v_rs: Option<f64>,
    pub present_low: bool,
    pub present_high: bool,
    pub limited: bool,
    pub sensor_powered: bool,
    pub edges: Vec<WsfEdge>,
}

#[derive(Debug, Clone, Default)]
pub struct WsfInputs { pub lv: Option<f64>, pub lanes: [WsfLaneInputs; 2] }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Band { Valid, Current, Open, Supply, Short }
impl Band {
    fn rank(self) -> u8 { match self { Self::Valid => 0, Self::Current => 1, Self::Open => 2, Self::Supply => 3, Self::Short => 4 } }
    fn status(self) -> usize { match self { Self::Valid => 0, Self::Current => 4, Self::Open => 2, Self::Supply => 5, Self::Short => 3 } }
}

#[derive(Debug, Clone)]
struct Lane {
    status: usize,
    abnormal: u32,
    valid: u32,
    pending: Band,
    changed_band: Band,
    changed_count: u32,
    supply_bad: bool,
    powered_at: Option<u64>,
    last_seen_edge: Option<u64>,
    rising_count: u32,
    edges: VecDeque<u64>,
    last_output: Option<f64>,
    still: bool,
    plateau_ma: [Option<f64>; 2],
}

impl Default for Lane {
    fn default() -> Self {
        Self { status: 1, abnormal: 0, valid: 0, pending: Band::Valid,
            changed_band: Band::Valid, changed_count: 0, supply_bad: false, powered_at: None,
            last_seen_edge: None, rising_count: 0, edges: VecDeque::new(), last_output: None,
            still: false, plateau_ma: [None; 2] }
    }
}

fn number(value: Option<f64>) -> Option<f64> { value.filter(|v| v.is_finite()) }
fn worst(a: Band, b: Band) -> Band { if a.rank() >= b.rank() { a } else { b } }

// An inclusive electrical boundary must survive the solver's voltage subtraction and
// conversion back to current. This is only floating-point resolution, not a wider band.
fn at_boundary(value:f64,boundary:f64)->bool {
    (value-boundary).abs()<=8.0*f64::EPSILON*value.abs().max(boundary.abs())
}

impl Lane {
    fn discard_periods(&mut self) { self.edges.clear(); self.still = false; }

    fn band(&mut self, p: &WsfParams, i: &WsfLaneInputs) -> Band {
        let was_supply_bad = self.supply_bad;
        let supply_bad = |value: Option<f64>| match number(value) {
            None => true,
            Some(v) if was_supply_bad => v < p.supply_window_v[0] + p.supply_hysteresis_v
                || v > p.supply_window_v[1] - p.supply_hysteresis_v,
            Some(v) => v < p.supply_window_v[0] || v > p.supply_window_v[1],
        };
        self.supply_bad = (i.present_low && supply_bad(i.low_sup_v))
            || (i.present_high && supply_bad(i.high_sup_v));
        let mut band = if self.supply_bad { Band::Supply } else { Band::Valid };
        for (present, v) in [(i.present_low, i.low_v_rs), (i.present_high, i.high_v_rs)] {
            if !present { continue; }
            let Some(ma) = number(v).map(|v| v / p.r_sense_ohm * 1000.0) else {
                band = worst(band, Band::Supply); continue;
            };
            let b = if ma < p.current_bands_ma[0] && !at_boundary(ma,p.current_bands_ma[0]) { Band::Open }
                else if ma > p.current_bands_ma[3] && !at_boundary(ma,p.current_bands_ma[3]) { Band::Short }
                else if ma > p.current_bands_ma[1] && !at_boundary(ma,p.current_bands_ma[1])
                    && ma < p.current_bands_ma[2] && !at_boundary(ma,p.current_bands_ma[2]) { Band::Current }
                else { Band::Valid };
            band = worst(band, b);
        }
        if !i.present_low && !i.present_high { band = worst(band, Band::Supply); }
        band
    }

    fn sample(&mut self, p: &WsfParams, t_ms: u64, i: &WsfLaneInputs) {
        let now = t_ms.saturating_mul(1_000_000);
        self.plateau_ma = [if i.present_low { number(i.low_v_rs).map(|v| v / p.r_sense_ohm * 1000.0) } else { None },
            if i.present_high { number(i.high_v_rs).map(|v| v / p.r_sense_ohm * 1000.0) } else { None }];
        if !i.sensor_powered {
            self.powered_at = None; self.rising_count = 0; self.discard_periods();
        } else if self.powered_at.is_none() {
            self.powered_at = Some(t_ms); self.rising_count = 0; self.discard_periods();
        }
        let band = self.band(p, i);
        if band != Band::Valid {
            self.valid = 0;
            self.abnormal = self.abnormal.saturating_add(1);
            self.pending = worst(self.pending, band);
            // No period is allowed to straddle even an unconfirmed electrical anomaly.
            self.discard_periods();
            if self.status <= 1 {
                if self.abnormal >= p.debounce_fault_samples { self.status = self.pending.status(); }
            } else if band.status() != self.status {
                if self.changed_band != band { self.changed_band = band; self.changed_count = 0; }
                self.changed_count = self.changed_count.saturating_add(1);
                if self.changed_count >= p.debounce_fault_samples { self.status = band.status(); self.changed_count = 0; }
            } else { self.changed_count = 0; }
            // Remember the chronology of discarded edges too: a cached probe cannot replay them after recovery.
            for e in &i.edges { if e.t_ns <= now { self.last_seen_edge = Some(self.last_seen_edge.map_or(e.t_ns, |v| v.max(e.t_ns))); } }
            return;
        }
        self.abnormal = 0; self.pending = Band::Valid; self.changed_count = 0;
        self.valid = self.valid.saturating_add(1);
        if !i.sensor_powered { self.status = 1; return; }
        let observable = number(i.high_v_rs).is_some_and(|v| v >= p.comparator_high_v)
            && number(i.low_v_rs).is_some_and(|v| v <= p.comparator_low_v);
        for edge in &i.edges {
            if edge.t_ns > now || self.last_seen_edge.is_some_and(|v| edge.t_ns <= v) { continue; }
            self.last_seen_edge = Some(edge.t_ns);
            if !observable { continue; }
            if edge.rising_current { self.rising_count = self.rising_count.saturating_add(1); }
            else if self.rising_count >= p.calibration_rises {
                let captured = edge.t_ns / p.capture_resolution_ns * p.capture_resolution_ns;
                if self.edges.back().is_none_or(|last| captured > *last) { self.edges.push_back(captured); }
            }
        }
        self.trim(p, now);
        if self.status > 1 {
            if self.valid < p.debounce_recover_samples { return; }
            // The good-sample interval includes the real power-on and calibration. Do not restart them.
            self.status = 1;
            self.discard_periods(); self.last_output = None;
            self.last_seen_edge = Some(now);
            return;
        }
        if self.status == 1 {
            let power_ready = self.powered_at.is_some_and(|t| t_ms.saturating_sub(t) >= p.sensor_power_on_max_ms);
            let stopped = self.last_seen_edge.map_or_else(
                || self.powered_at.is_some_and(|t| t_ms.saturating_sub(t.saturating_add(p.sensor_power_on_max_ms)) >= p.stop_ms),
                |t| now.saturating_sub(t) >= p.stop_ms.saturating_mul(1_000_000));
            let validation_elapsed = self.powered_at.is_some_and(|t| t_ms.saturating_sub(t)
                >= p.sensor_power_on_max_ms.saturating_add(p.initial_valid_samples as u64));
            if power_ready && validation_elapsed && self.valid >= p.initial_valid_samples
                && (self.edges.len() >= 2 || stopped) { self.status = 0; }
        }
    }

    fn trim(&mut self, p: &WsfParams, now: u64) {
        let cutoff = now.saturating_sub(p.measure_window_ms.saturating_mul(1_000_000));
        while self.edges.len() > 2 && self.edges.get(1).is_some_and(|t| *t <= cutoff) { self.edges.pop_front(); }
        // Duplicate or artificially dense input edges cannot create unbounded retained state.
        let bound = (p.max_speed_rpm * p.teeth_count as f64 * p.measure_window_ms as f64 / 60_000.0).ceil() as usize + 3;
        while self.edges.len() > bound { self.edges.pop_front(); }
    }

    fn speed(&mut self, p: &WsfParams, t_ns: u64) -> Option<f64> {
        if self.status != 0 { self.last_output = None; self.still = false; return None; }
        if self.abnormal != 0 { return self.last_output; }
        self.trim(p, t_ns);
        let last_two: Vec<u64> = self.edges.iter().rev().copied().filter(|t| *t <= t_ns).take(2).collect();
        let Some(last) = last_two.first().copied() else { self.still = true; self.last_output = Some(0.0); return self.last_output; };
        let age = t_ns.saturating_sub(last);
        if age >= p.stop_ms.saturating_mul(1_000_000) || last_two.len() < 2 {
            self.still = true; self.last_output = Some(0.0); return self.last_output;
        }
        let cutoff = t_ns.saturating_sub(p.measure_window_ms.saturating_mul(1_000_000));
        let window: Vec<u64> = self.edges.iter().copied().filter(|t| *t > cutoff && *t <= t_ns).collect();
        let (periods, elapsed) = if window.len() >= 2 {
            ((window.len() - 1) as f64, window[window.len() - 1] - window[0])
        } else { (1.0, last_two[0] - last_two[1]) };
        let mut speed = 60_000_000_000.0 * periods / (p.teeth_count as f64 * elapsed as f64);
        if age > 0 { speed = speed.min(60_000_000_000.0 / (p.teeth_count as f64 * age as f64)); }
        self.still = false;
        self.last_output = Some(speed.clamp(0.0, p.max_speed_rpm));
        self.last_output
    }
}

#[derive(Debug, Clone)]
pub struct EcuWsf {
    p: WsfParams,
    reset_below_v: f64, reset_exit_v: f64, boot_ms: u64,
    running: bool, reset_exit_ms: u64,
    lanes: [Lane; 2], lv_hist: VecDeque<f64>, lv_avg: f64, lv_uv: bool,
    sup_flag: bool, sup_bad: u32, sup_good: u32, sup_since: Option<u64>, clear_since: Option<u64>, fault: bool,
    emission_ns: u64,
    pub run_id: u64,
}

#[cfg(all(test, n7rt_oracle))]
mod tests { include!(concat!(env!("N7RT_ORACLE_SRC"), "/tests/source_wsf.rs")); }

impl EcuWsf {
    /// New static configuration (same mission), dynamic state kept.
    pub fn reconfigure(&mut self, p: &WsfParams) {
        self.p = p.clone();
    }

    pub fn new(p: &WsfParams, reset_below_v: f64, reset_exit_v: f64, boot_ms: u64) -> Self {
        Self { p: p.clone(), reset_below_v, reset_exit_v, boot_ms, running: false, reset_exit_ms: 0,
            lanes: [Lane::default(), Lane::default()], lv_hist: VecDeque::new(), lv_avg: 0.0, lv_uv: false,
            sup_flag: false, sup_bad: 0, sup_good: 0, sup_since: None, clear_since: None, fault: false,
            emission_ns: 0, run_id: 0 }
    }
    pub fn sample(&mut self, t_ms: u64, i: &WsfInputs) {
        let lv = number(i.lv).unwrap_or(0.0);
        while self.lv_hist.len() >= self.p.lv_avg_samples.max(1) { self.lv_hist.pop_front(); }
        self.lv_hist.push_back(lv);
        self.lv_avg = self.lv_hist.iter().sum::<f64>() / self.lv_hist.len() as f64;
        if self.running && lv < self.reset_below_v {
            self.running = false; self.lanes = [Lane::default(), Lane::default()];
            self.sup_flag = false; self.sup_bad = 0; self.sup_good = 0;
            self.sup_since = None; self.clear_since = None; self.fault = false; self.lv_uv = false;
        }
        if !self.running {
            if lv < self.reset_exit_v { return; }
            self.running = true; self.reset_exit_ms = t_ms; self.run_id = self.run_id.wrapping_add(1);
        }
        self.emission_ns = t_ms.saturating_mul(1_000_000);
        for (lane, inp) in self.lanes.iter_mut().zip(&i.lanes) { lane.sample(&self.p, t_ms, inp); }
        if self.lv_avg < self.p.lv_uv_on_v { self.lv_uv = true; }
        else if self.lv_avg >= self.p.lv_uv_off_v { self.lv_uv = false; }
        let bad = self.lanes.iter().zip(&i.lanes).any(|(s, i)| s.supply_bad || i.limited);
        if bad {
            self.sup_good = 0; self.sup_bad = self.sup_bad.saturating_add(1);
            if self.sup_bad >= self.p.sup_rise_samples { self.sup_flag = true; }
        } else {
            self.sup_bad = 0; self.sup_good = self.sup_good.saturating_add(1);
            if self.sup_good >= self.p.sup_fall_samples { self.sup_flag = false; }
        }
        if self.sup_flag {
            let since = *self.sup_since.get_or_insert(t_ms); self.clear_since = None;
            if t_ms.saturating_sub(since) >= self.p.sup_fault_persist_ms { self.fault = true; }
        } else {
            self.sup_since = None;
            if self.fault {
                let since = *self.clear_since.get_or_insert(t_ms);
                if t_ms.saturating_sub(since) >= self.p.fault_recovery_ms { self.fault = false; }
            }
        }
    }
    pub fn can_emit(&self, t_ms: u64, nominal_us: u64) -> bool {
        self.running && t_ms >= self.reset_exit_ms
            && nominal_us >= self.reset_exit_ms.saturating_add(self.boot_ms).saturating_mul(1000)
    }
    /// Called with the nominal instant before reading the source fields of a frame.
    pub fn prepare_emission(&mut self, nominal_us: u64) { self.emission_ns = nominal_us.saturating_mul(1000); }
    pub fn value(&mut self, source: &str) -> Option<f64> {
        match source {
            "wsf.ws_l" => self.lanes[0].speed(&self.p, self.emission_ns),
            "wsf.ws_r" => self.lanes[1].speed(&self.p, self.emission_ns),
            "wsf.ws_l_st" => Some(self.p.status_codes[self.lanes[0].status] as f64),
            "wsf.ws_r_st" => Some(self.p.status_codes[self.lanes[1].status] as f64),
            "wsf.still" => {
                for s in &mut self.lanes { s.speed(&self.p, self.emission_ns); }
                Some(if self.lanes.iter().all(|s| s.status == 0 && s.still) { 1.0 } else { 0.0 })
            }
            "wsf.il_l" => self.lanes[0].plateau_ma[0], "wsf.ih_l" => self.lanes[0].plateau_ma[1],
            "wsf.il_r" => self.lanes[1].plateau_ma[0], "wsf.ih_r" => self.lanes[1].plateau_ma[1],
            "wsf.ecu_mode" => {
                let m = if self.fault { 3 } else if self.lanes.iter().any(|s| s.status > 1) { 2 }
                    else if self.lanes.iter().any(|s| s.status == 1) { 0 } else { 1 };
                Some(self.p.mode_codes[m] as f64)
            }
            "wsf.lv_uv" => Some(if self.lv_uv { 1.0 } else { 0.0 }),
            "wsf.sens_sup_flt" => Some(if self.sup_flag { 1.0 } else { 0.0 }),
            "wsf.v_lv" => Some(self.lv_avg), _ => None,
        }
    }
}
