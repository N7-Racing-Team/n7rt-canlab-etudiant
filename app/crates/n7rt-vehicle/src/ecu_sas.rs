//! Generic source ECU for an angle sensor transported over SENT. All protocol, conversion,
//! status and timing parameters are supplied by the sealed mission pack.

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

pub(crate) const SOURCES: &[&str] = &[
    "sas.angle", "sas.speed", "sas.st", "sas.cal", "sas.sent_raw", "sas.sent_rej",
    "sas.supply", "sas.sup5v", "sas.lv_uv", "sas.sens_sup_flt", "sas.v_lv", "sas.ecu_mode",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SentConfig {
    pub sync_ticks: u32,
    pub tick_min_us: f64,
    pub tick_max_us: f64,
    pub frame_ticks: u32,
    pub frame_tolerance_ticks: f64,
    pub nibble_base_ticks: u32,
    pub nibble_bits: u8,
    pub nibble_count: usize,
    pub data_indices: Vec<usize>,
    pub counter_indices: Vec<usize>,
    pub counter_modulus: u32,
    pub inverse_index: usize,
    pub inverted_index: usize,
    pub status_index: usize,
    pub error_mask: u32,
    pub crc_index: usize,
    pub crc_indices: Vec<usize>,
    pub crc_polynomial: u32,
    pub crc_seed: u32,
    pub crc_final_zero: bool,
    pub data_min: u32,
    pub data_max: u32,
    pub error_code: u32,
}

#[cfg(all(test, n7rt_oracle))]
mod tests {
    include!(concat!(env!("N7RT_ORACLE_SRC"), "/tests/source_sas.rs"));
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SasConfig {
    pub sent: SentConfig,
    pub adc_bits: u8,
    pub adc_vref_v: f64,
    pub supply_divider: f64,
    pub calibrated: bool,
    pub zero_code: i64,
    pub direction: i64,
    pub wrap_steps: i64,
    pub turn_angle_units: i64,
    pub turn_degrees: i64,
    pub angle_units_per_degree: f64,
    pub angle_min_units: i64,
    pub angle_max_units: i64,
    pub speed_min: i64,
    pub speed_max: i64,
    pub speed_window_us: u64,
    pub history_capacity: usize,
    pub init_us: u64,
    pub absence_us: u64,
    pub recovery_us: u64,
    pub raw_max_age_us: u64,
    pub rejected_max: u32,
    pub supply_lo_v: f64,
    pub supply_hi_v: f64,
    pub supply_fault_rise_ms: u32,
    pub supply_fault_fall_ms: u32,
    pub supply_fault_persist_ms: u64,
    pub lv_avg_samples: usize,
    pub lv_uv_on_v: f64,
    pub lv_uv_off_v: f64,
    pub status_enum: String,
    pub mode_enum: String,
    pub st_init_name: String,
    pub st_ok_name: String,
    pub st_sensor_error_name: String,
    pub st_comm_error_name: String,
    pub mode_init_name: String,
    pub mode_run_name: String,
    pub mode_degraded_name: String,
    pub mode_fault_name: String,
    #[serde(skip)]
    pub st_init: i64,
    #[serde(skip)]
    pub st_ok: i64,
    #[serde(skip)]
    pub st_sensor_error: i64,
    #[serde(skip)]
    pub st_comm_error: i64,
    #[serde(skip)]
    pub mode_init: i64,
    #[serde(skip)]
    pub mode_run: i64,
    #[serde(skip)]
    pub mode_degraded: i64,
    #[serde(skip)]
    pub mode_fault: i64,
}

impl SasConfig {
    /// Reject invalid pack parameters before any arithmetic or allocation is attempted.
    pub fn validate(&self) -> Result<(), String> {
        let s = &self.sent;
        let finite = [s.tick_min_us, s.tick_max_us, s.frame_tolerance_ticks, self.adc_vref_v,
            self.supply_divider, self.angle_units_per_degree, self.supply_lo_v, self.supply_hi_v,
            self.lv_uv_on_v, self.lv_uv_off_v];
        if finite.iter().any(|v| !v.is_finite()) || s.sync_ticks == 0 || s.frame_ticks == 0
            || s.tick_min_us <= 0.0 || s.tick_max_us < s.tick_min_us
            || s.frame_tolerance_ticks < 0.0 || s.frame_tolerance_ticks >= s.frame_ticks as f64
            || !(1..=8).contains(&s.nibble_bits) || s.nibble_count == 0 || s.nibble_count > 64
            || s.data_indices.is_empty() || s.counter_indices.is_empty() || s.crc_indices.is_empty()
            || s.data_indices.len() * s.nibble_bits as usize > 32
            || s.counter_indices.len() * s.nibble_bits as usize > 32
            || s.counter_modulus < 2 || s.data_min > s.data_max
            || self.wrap_steps <= 1 || self.wrap_steps > u32::MAX as i64 || self.wrap_steps % 2 == 0
            || ![-1, 1].contains(&self.direction) || self.turn_angle_units <= 0 || self.turn_degrees <= 0
            || self.zero_code < s.data_min as i64 || self.zero_code > s.data_max as i64
            || self.angle_units_per_degree <= 0.0 || self.angle_min_units > self.angle_max_units
            || self.speed_min > self.speed_max || self.speed_window_us == 0
            || self.history_capacity < 2 || self.history_capacity > 4096
            || self.init_us == 0 || self.absence_us == 0 || self.recovery_us == 0
            || self.raw_max_age_us == 0 || self.supply_lo_v > self.supply_hi_v
            || self.supply_fault_rise_ms == 0 || self.supply_fault_fall_ms == 0
            || self.lv_avg_samples == 0 || self.lv_avg_samples > 4096
            || !(1..=30).contains(&self.adc_bits) || self.adc_vref_v <= 0.0
            || self.supply_divider <= 0.0 || self.lv_uv_on_v > self.lv_uv_off_v
            || [&self.status_enum, &self.mode_enum, &self.st_init_name, &self.st_ok_name,
                &self.st_sensor_error_name, &self.st_comm_error_name, &self.mode_init_name,
                &self.mode_run_name, &self.mode_degraded_name, &self.mode_fault_name]
                .iter().any(|name| name.is_empty())
        {
            return Err("source_ecu.sas : paramètres invalides".into());
        }
        let mask = (1u32 << s.nibble_bits) - 1;
        if s.crc_polynomial > mask || s.crc_seed > mask || s.error_mask > mask
            || [s.inverse_index, s.inverted_index, s.status_index, s.crc_index].iter()
                .chain(s.data_indices.iter()).chain(s.counter_indices.iter()).chain(s.crc_indices.iter())
                .any(|i| *i >= s.nibble_count)
        {
            return Err("source_ecu.sas.sent : paramètres invalides".into());
        }
        let min_frame_us = (s.frame_ticks as f64 - s.frame_tolerance_ticks) * s.tick_min_us;
        if min_frame_us < 1.0 || (self.speed_window_us as f64 / min_frame_us).ceil() + 2.0
            > self.history_capacity as f64 {
            return Err("source_ecu.sas : historique insuffisant".into());
        }
        Ok(())
    }
}

/// An entire candidate ends at the next synchronization edge. Intervals are measured
/// between falling edges; the receiver, rather than the physical probe, checks its CRC.
#[derive(Debug, Clone)]
pub struct SentFrame {
    pub started_us: u64,
    pub completed_us: u64,
    pub sync_us: f64,
    pub intervals_us: Vec<f64>,
}

#[derive(Debug, Clone, Default)]
pub struct SasInput {
    pub lv: Option<f64>,
    pub supply_adc: Option<u32>,
    pub frames: Vec<SentFrame>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status { Init, Ok, SensorError, CommError }

#[derive(Debug, Clone)]
pub(crate) struct EcuSas {
    p: SasConfig,
    reset_below_v: f64,
    reset_exit_v: f64,
    t_boot_ms: u64,
    running: bool,
    reset_exit_us: u64,
    now_us: u64,
    last_candidate_end: Option<u64>,
    previous_counter: Option<u32>,
    last_usable: Option<u64>,
    good_since: Option<u64>,
    comm_error: bool,
    sensor_error: bool,
    reset_eligible: bool,
    raw: Option<(u64, u32)>,
    rejected: u32,
    history: VecDeque<(u64, i64)>,
    angle: Option<f64>,
    speed: Option<f64>,
    lv_history: VecDeque<f64>,
    lv_avg: f64,
    lv_uv: bool,
    supply: f64,
    supply_fault: bool,
    supply_out: u32,
    supply_in: u32,
    fault_since: Option<u64>,
    clear_since: Option<u64>,
    fault: bool,
    pub run_id: u64,
}

fn rounded_div(n: i128, d: i128) -> i128 {
    let sign = if n < 0 { -1 } else { 1 };
    sign * ((n.abs() + d / 2) / d)
}

fn wrap(v: i64, steps: i64) -> i64 {
    let half = steps / 2;
    ((v as i128 + half as i128).rem_euclid(steps as i128) - half as i128) as i64
}

impl EcuSas {
    pub fn new(p: &SasConfig, reset_below_v: f64, reset_exit_v: f64, t_boot_ms: u64) -> Self {
        Self {
            p: p.clone(), reset_below_v, reset_exit_v, t_boot_ms,
            running: false, reset_exit_us: 0, now_us: 0, last_candidate_end: None,
            previous_counter: None, last_usable: None, good_since: None, comm_error: false,
            sensor_error: false, reset_eligible: true, raw: None, rejected: 0,
            history: VecDeque::new(), angle: None, speed: None, lv_history: VecDeque::new(),
            lv_avg: 0.0, lv_uv: false, supply: 0.0, supply_fault: false, supply_out: 0,
            supply_in: 0, fault_since: None, clear_since: None, fault: false, run_id: 0,
        }
    }

    /// New static configuration (same mission), dynamic state kept. The stored angle deltas follow the new
    /// configuration, as if computed with it: the angular speed stays continuous across a reconfiguration.
    pub fn reconfigure(&mut self, p: &SasConfig) {
        let old = &self.p;
        if old.zero_code != p.zero_code || old.direction != p.direction || old.wrap_steps != p.wrap_steps {
            for (_, d) in self.history.iter_mut() {
                *d = wrap(p.direction * (old.direction * *d + old.zero_code - p.zero_code), p.wrap_steps);
            }
        }
        self.p = p.clone();
    }

    fn reset_logic(&mut self) {
        self.last_candidate_end = None;
        self.previous_counter = None;
        self.last_usable = None;
        self.good_since = None;
        self.comm_error = false;
        self.sensor_error = false;
        self.reset_eligible = true;
        self.raw = None;
        self.rejected = 0;
        self.history.clear();
        self.angle = None;
        self.speed = None;
        self.supply_fault = false;
        self.supply_out = 0;
        self.supply_in = 0;
        self.fault_since = None;
        self.clear_since = None;
        self.fault = false;
        self.lv_uv = false;
    }

    pub fn can_emit(&self, t_ms: u64, nominal_us: u64) -> bool {
        let ready = self.reset_exit_us.saturating_add(self.t_boot_ms.saturating_mul(1000));
        self.running && t_ms.saturating_mul(1000) >= self.reset_exit_us && nominal_us >= ready
    }

    fn expire(&mut self, t_us: u64) {
        let acquisition_start = self.reset_exit_us.saturating_add(self.t_boot_ms.saturating_mul(1000));
        let anchor = self.last_usable.unwrap_or(acquisition_start);
        if t_us >= acquisition_start && t_us.saturating_sub(anchor) >= self.p.absence_us {
            self.comm_error = true;
            self.good_since = None;
            self.history.clear();
            self.angle = None;
            self.speed = None;
            if self.last_candidate_end.is_none_or(|t| t_us.saturating_sub(t) >= self.p.absence_us) {
                self.reset_eligible = true;
            }
        }
    }

    fn status(&self) -> Status {
        if self.comm_error { Status::CommError }
        else if self.sensor_error { Status::SensorError }
        else if self.good_since.is_none_or(|t| self.now_us.saturating_sub(t) < self.p.init_us) {
            Status::Init
        } else { Status::Ok }
    }

    fn nibble_transform(&self, mut crc: u32) -> u32 {
        let s = &self.p.sent;
        let top = 1 << (s.nibble_bits - 1);
        let mask = (1 << s.nibble_bits) - 1;
        for _ in 0..s.nibble_bits {
            crc = if crc & top != 0 { (crc << 1) ^ s.crc_polynomial } else { crc << 1 };
            crc &= mask;
        }
        crc
    }

    fn decode(&self, frame: &SentFrame) -> Option<(u32, u32, bool)> {
        let s = &self.p.sent;
        if !frame.sync_us.is_finite() || frame.intervals_us.len() != s.nibble_count { return None; }
        let tick = frame.sync_us / s.sync_ticks as f64;
        if tick < s.tick_min_us || tick > s.tick_max_us { return None; }
        let duration = frame.completed_us.checked_sub(frame.started_us)? as f64 / tick;
        if (duration - s.frame_ticks as f64).abs() > s.frame_tolerance_ticks { return None; }
        let mask = (1u32 << s.nibble_bits) - 1;
        let mut nibbles = Vec::with_capacity(s.nibble_count);
        for interval in &frame.intervals_us {
            if !interval.is_finite() { return None; }
            let n = (interval / tick).round() - s.nibble_base_ticks as f64;
            if n < 0.0 || n > mask as f64 { return None; }
            nibbles.push(n as u32);
        }
        if nibbles[s.inverse_index] != (!nibbles[s.inverted_index] & mask) { return None; }
        let mut crc = s.crc_seed;
        for i in &s.crc_indices { crc = self.nibble_transform(crc) ^ nibbles[*i]; }
        if s.crc_final_zero { crc = self.nibble_transform(crc); }
        if crc != nibbles[s.crc_index] { return None; }
        let read = |indices: &[usize]| indices.iter().fold(0u32, |v, i| (v << s.nibble_bits) | nibbles[*i]);
        let count = read(&s.counter_indices);
        if count >= s.counter_modulus { return None; }
        Some((read(&s.data_indices), count, nibbles[s.status_index] & s.error_mask != 0))
    }

    fn receive(&mut self, frame: &SentFrame) {
        let acquisition_start = self.reset_exit_us.saturating_add(self.t_boot_ms.saturating_mul(1000));
        if frame.started_us < acquisition_start || frame.completed_us > self.now_us
            || self.last_candidate_end.is_some_and(|t| frame.started_us < t) { return; }
        self.expire(frame.completed_us);
        let contiguous = self.last_candidate_end == Some(frame.started_us);
        self.last_candidate_end = Some(frame.completed_us);
        let reject = |this: &mut Self| {
            this.rejected = this.rejected.saturating_add(1).min(this.p.rejected_max);
            this.previous_counter = None;
        };
        let Some((data, count, error)) = self.decode(frame) else { reject(self); return; };
        if contiguous && self.previous_counter.is_some_and(|c| count != (c + 1) % self.p.sent.counter_modulus) {
            reject(self); return;
        }
        let valid_data = (self.p.sent.data_min..=self.p.sent.data_max).contains(&data);
        let reset_data = data <= self.p.sent.data_max;
        if error && reset_data && self.reset_eligible {
            self.reset_eligible = false;
            self.previous_counter = None;
            self.good_since = None;
            self.history.clear();
            self.angle = None;
            self.speed = None;
            return;
        }
        if error && (data == self.p.sent.error_code || reset_data) {
            self.previous_counter = Some(count);
            self.sensor_error = true;
            self.good_since = None;
            self.history.clear();
            self.angle = None;
            self.speed = None;
            if data == self.p.sent.error_code { self.raw = Some((frame.completed_us, data)); }
            return;
        }
        if error || !valid_data { reject(self); return; }
        self.previous_counter = Some(count);
        self.reset_eligible = false;
        self.raw = Some((frame.completed_us, data));
        self.last_usable = Some(frame.completed_us);
        let since = *self.good_since.get_or_insert(frame.completed_us);
        if frame.completed_us.saturating_sub(since) >= self.p.recovery_us {
            self.comm_error = false;
            self.sensor_error = false;
        }
        let delta = wrap(self.p.direction * (data as i64 - self.p.zero_code), self.p.wrap_steps);
        let units = rounded_div(delta as i128 * self.p.turn_angle_units as i128, self.p.wrap_steps as i128)
            .clamp(self.p.angle_min_units as i128, self.p.angle_max_units as i128);
        self.angle = self.p.calibrated.then_some(units as f64 / self.p.angle_units_per_degree);
        let threshold = frame.started_us.saturating_sub(self.p.speed_window_us);
        while self.history.len() > 1 && self.history[1].0 <= threshold { self.history.pop_front(); }
        self.speed = self.history.front().filter(|(t, _)| *t <= threshold && *t < frame.started_us)
            .map(|(t, old)| {
                let diff = wrap(delta - old, self.p.wrap_steps);
                let n = diff as i128 * self.p.turn_degrees as i128 * 1_000_000;
                rounded_div(n, self.p.wrap_steps as i128 * (frame.started_us - t) as i128)
                    .clamp(self.p.speed_min as i128, self.p.speed_max as i128) as f64
            });
        while self.history.len() >= self.p.history_capacity.max(1) { self.history.pop_front(); }
        self.history.push_back((frame.started_us, delta));
    }

    /// One millisecond of measured physical input; frame timestamps retain SENT precision.
    pub fn sample(&mut self, t_ms: u64, inp: &SasInput) {
        self.now_us = t_ms.saturating_mul(1000);
        let lv = inp.lv.filter(|v| v.is_finite()).unwrap_or(0.0);
        while self.lv_history.len() >= self.p.lv_avg_samples.max(1) { self.lv_history.pop_front(); }
        self.lv_history.push_back(lv);
        self.lv_avg = self.lv_history.iter().sum::<f64>() / self.lv_history.len() as f64;
        if self.running && lv < self.reset_below_v {
            self.running = false;
            self.reset_logic();
            return;
        }
        if !self.running {
            if lv < self.reset_exit_v { return; }
            self.running = true;
            self.reset_exit_us = self.now_us;
            self.run_id = self.run_id.saturating_add(1);
            self.reset_logic();
        }
        if self.lv_uv {
            if self.lv_avg >= self.p.lv_uv_off_v { self.lv_uv = false; }
        } else if self.lv_avg < self.p.lv_uv_on_v { self.lv_uv = true; }
        let full = 1u64 << self.p.adc_bits;
        self.supply = inp.supply_adc.map_or(0.0, |c|
            (c as u64).min(full - 1) as f64 * self.p.adc_vref_v / full as f64 * self.p.supply_divider);
        if self.supply < self.p.supply_lo_v || self.supply > self.p.supply_hi_v {
            self.supply_out = self.supply_out.saturating_add(1);
            self.supply_in = 0;
            if self.supply_out >= self.p.supply_fault_rise_ms { self.supply_fault = true; }
        } else {
            self.supply_in = self.supply_in.saturating_add(1);
            self.supply_out = 0;
            if self.supply_in >= self.p.supply_fault_fall_ms { self.supply_fault = false; }
        }
        if self.supply_fault {
            self.clear_since = None;
            let since = *self.fault_since.get_or_insert(t_ms);
            if t_ms.saturating_sub(since) >= self.p.supply_fault_persist_ms { self.fault = true; }
        } else {
            self.fault_since = None;
            let since = *self.clear_since.get_or_insert(t_ms);
            if t_ms.saturating_sub(since) >= self.p.supply_fault_persist_ms { self.fault = false; }
        }
        for frame in &inp.frames { self.receive(frame); }
        self.expire(self.now_us);
    }

    pub fn value(&mut self, source: &str) -> Option<f64> {
        let status = self.status();
        match source {
            "sas.angle" => (status == Status::Ok).then_some(self.angle).flatten(),
            "sas.speed" => (status == Status::Ok).then_some(self.speed).flatten(),
            "sas.st" => Some(match status { Status::Init => self.p.st_init, Status::Ok => self.p.st_ok,
                Status::SensorError => self.p.st_sensor_error, Status::CommError => self.p.st_comm_error } as f64),
            "sas.cal" => Some(self.p.calibrated as u8 as f64),
            "sas.sent_raw" => self.raw.filter(|(t, _)| self.now_us.saturating_sub(*t) <= self.p.raw_max_age_us)
                .map(|(_, data)| data as f64),
            "sas.sent_rej" => { let value = self.rejected; self.rejected = 0; Some(value as f64) },
            "sas.supply" | "sas.sup5v" => Some(self.supply),
            "sas.lv_uv" => Some(self.lv_uv as u8 as f64),
            "sas.sens_sup_flt" => Some(self.supply_fault as u8 as f64),
            "sas.v_lv" => Some(self.lv_avg),
            "sas.ecu_mode" => Some(if self.fault { self.p.mode_fault }
                else if matches!(status, Status::CommError | Status::SensorError) || !self.p.calibrated {
                    self.p.mode_degraded
                } else if status == Status::Init { self.p.mode_init } else { self.p.mode_run } as f64),
            _ => None,
        }
    }
}
