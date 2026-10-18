//! Parameter-driven pressure ECU. The physical probe supplies ADC codes and the contact voltage;
//! the pack supplies every transfer coefficient, code, threshold and duration. No CAN layout lives here.

use std::collections::VecDeque;

use serde::Deserialize;

use crate::pack::SourceEcu;
use crate::EcuInputs;

pub(crate) const SOURCES: &[&str] = &[
    "bps.p_front", "bps.p_rear", "bps.p_front_st", "bps.p_rear_st", "bps.brk_sw",
    "bps.brk_act", "bps.brk_act_q", "bps.pf_ratio", "bps.pr_ratio", "bps.sup5v",
    "bps.ecu_mode", "bps.lv_uv", "bps.sens_sup_flt", "bps.v_lv", "bps.ecu_temp",
];

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SourceBps {
    pub pressure_full_scale_bar: f64,
    pub atmospheric_bar: f64,
    pub switch_schmitt_v: [f64; 2],
    pub switch_debounce_samples: u32,
    pub switch_enum: String,
    /// Resolved by the loader: open/closed from the enum, unavailable from the signal's SNA.
    #[serde(skip)]
    pub switch_codes: [i64; 3],
    pub contact_disagree_bar: f64,
    pub contact_persist_ms: u64,
    pub contact_recover_ms: u64,
    pub brake_on_bar: f64,
    pub brake_off_bar: f64,
    #[serde(skip)]
    pub status_supply: i64,
    #[serde(skip)]
    pub status_implausible: i64,
    pub ratio_max_pct: f64,
    pub supply_report_max_v: f64,
}

impl SourceBps {
    pub fn validate(&self) -> Result<(), String> {
        let positive = [self.pressure_full_scale_bar, self.ratio_max_pct, self.supply_report_max_v];
        let finite = [self.atmospheric_bar, self.contact_disagree_bar, self.brake_on_bar,
            self.brake_off_bar, self.switch_schmitt_v[0], self.switch_schmitt_v[1]];
        if positive.iter().any(|v| !v.is_finite() || *v <= 0.0)
            || finite.iter().any(|v| !v.is_finite())
            || self.switch_schmitt_v[0] >= self.switch_schmitt_v[1]
            || self.brake_off_bar >= self.brake_on_bar
            || self.switch_debounce_samples == 0 || self.switch_debounce_samples > 4096
            || self.contact_persist_ms == 0 || self.contact_recover_ms == 0
            || self.contact_persist_ms > 86_400_000 || self.contact_recover_ms > 86_400_000
            || self.switch_enum.is_empty()
            || self.status_supply < 0 || self.status_implausible < 0
            || self.status_supply == self.status_implausible
            || self.switch_codes.iter().any(|v| *v < 0)
            || self.switch_codes[0] == self.switch_codes[1]
            || self.switch_codes[0] == self.switch_codes[2]
            || self.switch_codes[1] == self.switch_codes[2]
        {
            return Err("source_ecu.bps : paramètres invalides".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status { Init, Ok, Low, High, Supply, Implausible }

impl Status {
    fn priority(self) -> u8 {
        match self { Self::Init => 4, Self::Supply => 3, Self::Low | Self::High => 2,
            Self::Implausible => 1, Self::Ok => 0 }
    }
}

#[derive(Debug, Clone)]
struct Channel {
    electrical: Status,
    status: Status,
    good: u32,
    bad: u32,
    recent: VecDeque<Status>,
    pressures: VecDeque<f64>,
    pressure: Option<f64>,
    ratio: Option<f64>,
}

impl Channel {
    fn new() -> Self {
        Self { electrical: Status::Init, status: Status::Init, good: 0, bad: 0,
            recent: VecDeque::new(), pressures: VecDeque::new(), pressure: None, ratio: None }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Disagreement { None, Contact, Rear }

fn average(values: &mut VecDeque<f64>, size: usize, value: f64) -> f64 {
    if values.len() >= size { values.pop_front(); }
    values.push_back(value);
    values.iter().sum::<f64>() / values.len() as f64
}

#[derive(Debug, Clone)]
pub(crate) struct EcuBps {
    p: SourceEcu,
    b: SourceBps,
    reset_below_v: f64,
    reset_exit_v: f64,
    boot_ms: u64,
    running: bool,
    reset_exit_ms: u64,
    pub run_id: u64,
    channels: [Channel; 2],
    lv_history: VecDeque<f64>,
    lv: f64,
    lv_uv: bool,
    supply: f64,
    supply_fault: bool,
    supply_bad: u32,
    supply_good: u32,
    supply_since: Option<u64>,
    supply_clear_since: Option<u64>,
    fault: bool,
    contact_level: Option<bool>,
    contact_count: u32,
    contact: Option<bool>,
    disagreement: Disagreement,
    disagreement_since: Option<u64>,
    recovery_since: Option<u64>,
    brake: bool,
    brake_qualified: bool,
}

impl EcuBps {
    pub fn new(p: &SourceEcu, reset_below_v: f64, reset_exit_v: f64, boot_ms: u64) -> Self {
        let b = p.bps.clone().expect("validated pressure ECU parameters");
        Self { p: p.clone(), b, reset_below_v, reset_exit_v, boot_ms, running: false,
            reset_exit_ms: 0, run_id: 0, channels: [Channel::new(), Channel::new()],
            lv_history: VecDeque::new(), lv: 0.0, lv_uv: false, supply: 0.0,
            supply_fault: false, supply_bad: 0, supply_good: 0, supply_since: None,
            supply_clear_since: None, fault: false, contact_level: None, contact_count: 0,
            contact: None, disagreement: Disagreement::None, disagreement_since: None,
            recovery_since: None, brake: false, brake_qualified: false }
    }

    /// New static configuration (same mission), dynamic state kept.
    pub fn reconfigure(&mut self, p: &SourceEcu) {
        if let Some(b) = &p.bps {
            self.p = p.clone();
            self.b = b.clone();
        }
    }

    fn reset(&mut self) {
        self.channels = [Channel::new(), Channel::new()];
        self.supply_fault = false;
        self.supply_bad = 0;
        self.supply_good = 0;
        self.supply_since = None;
        self.supply_clear_since = None;
        self.fault = false;
        self.contact_level = None;
        self.contact_count = 0;
        self.contact = None;
        self.disagreement = Disagreement::None;
        self.disagreement_since = None;
        self.recovery_since = None;
        self.brake = false;
        self.brake_qualified = false;
        self.lv_uv = false;
    }

    pub fn can_emit(&self, ms: u64, nominal_us: u64) -> bool {
        self.running && ms >= self.reset_exit_ms
            && nominal_us >= self.reset_exit_ms.saturating_add(self.boot_ms).saturating_mul(1000)
    }

    fn voltage(&self, code: Option<u32>, divider: f64) -> f64 {
        let full = (1u64 << self.p.adc_bits) as f64;
        divider * code.unwrap_or(0).min((full - 1.0) as u32) as f64 * self.p.adc_vref_v / full
    }

    pub fn sample(&mut self, ms: u64, input: &EcuInputs) {
        let lv = input.lv.filter(|v| v.is_finite()).unwrap_or(0.0);
        self.lv = average(&mut self.lv_history, self.p.lv_avg_samples, lv);
        if self.running && lv < self.reset_below_v {
            self.running = false;
            self.reset();
            return;
        }
        if !self.running {
            if lv < self.reset_exit_v { return; }
            self.running = true;
            self.reset_exit_ms = ms;
            self.run_id = self.run_id.saturating_add(1);
            self.reset();
        }
        if self.lv_uv { self.lv_uv = self.lv < self.p.lv_uv_off_v; }
        else { self.lv_uv = self.lv < self.p.lv_uv_on_v; }
        let rails_ms = self.reset_exit_ms.saturating_add(self.boot_ms);
        if ms < rails_ms { return; }
        let init = ms < rails_ms.saturating_add(self.p.init_after_rails_ms);
        self.supply = self.voltage(input.adc[2], self.p.sup_divider);
        let rail_saturated = input.adc[2].is_some_and(|v| v as u64 >= (1u64 << self.p.adc_bits) - 1);
        let undefined = self.supply < self.p.ratio_undefined_below_v || rail_saturated;
        for x in 0..2 {
            let signal = self.voltage(input.adc[x], self.p.sig_divider);
            let ratio = if undefined { None } else { Some(100.0 * signal / self.supply) };
            let cp = &self.p.channels[x];
            let electrical = if self.supply < self.p.supply_lo_v || self.supply > self.p.supply_hi_v {
                Status::Supply
            } else if ratio.is_none_or(|r| r < cp.window_lo) {
                Status::Low
            } else if ratio.is_some_and(|r| r > cp.window_hi) {
                Status::High
            } else { Status::Ok };
            let c = &mut self.channels[x];
            c.ratio = ratio.map(|r| r.min(self.b.ratio_max_pct));
            if c.recent.len() >= self.p.debounce_fault as usize { c.recent.pop_front(); }
            c.recent.push_back(electrical);
            if electrical == Status::Ok {
                if c.good == 0 && !matches!(c.electrical, Status::Ok | Status::Init) {
                    c.pressures.clear();
                }
                c.good = c.good.saturating_add(1);
                c.bad = 0;
                let absolute = self.b.pressure_full_scale_bar * (ratio.unwrap() - cp.a) / cp.b;
                c.pressure = Some(average(&mut c.pressures, self.p.filter_samples, absolute) - self.b.atmospheric_bar);
            } else {
                c.good = 0;
                c.bad = c.bad.saturating_add(1);
            }
            if init { c.electrical = Status::Init; }
            else if c.electrical == Status::Init {
                c.electrical = if c.good >= self.p.debounce_fault { Status::Ok }
                    else { c.recent.iter().copied().max_by_key(|s| s.priority()).unwrap_or(electrical) };
            } else if c.electrical == Status::Ok {
                if c.bad >= self.p.debounce_fault { c.electrical = electrical; }
            } else if c.good >= self.p.debounce_recover {
                c.electrical = Status::Ok;
            } else if electrical != Status::Ok { c.electrical = electrical; }
            c.status = c.electrical;
        }
        self.sample_contact(input.contact_v);
        if init { self.contact = None; }
        self.sample_supply(ms);
        self.sample_disagreement(ms, init);
        self.sample_brake(init);
    }

    fn sample_contact(&mut self, voltage: Option<f64>) {
        let Some(v) = voltage.filter(|v| v.is_finite()) else {
            self.contact_level = None; self.contact_count = 0; self.contact = None; return;
        };
        let level = if v < self.b.switch_schmitt_v[0] { Some(true) }
            else if v > self.b.switch_schmitt_v[1] { Some(false) } else { self.contact_level };
        if level == self.contact_level { self.contact_count = self.contact_count.saturating_add(1); }
        else { self.contact_level = level; self.contact_count = 1; }
        self.contact = if self.contact_count >= self.b.switch_debounce_samples { level } else { None };
    }

    fn sample_supply(&mut self, ms: u64) {
        if (self.supply - self.p.sup_nominal_v).abs() > self.p.sup_tolerance_v {
            self.supply_bad = self.supply_bad.saturating_add(1); self.supply_good = 0;
            if self.supply_bad >= self.p.sup_rise { self.supply_fault = true; }
        } else {
            self.supply_good = self.supply_good.saturating_add(1); self.supply_bad = 0;
            if self.supply_good >= self.p.sup_fall { self.supply_fault = false; }
        }
        if self.supply_fault {
            self.supply_clear_since = None;
            let since = *self.supply_since.get_or_insert(ms);
            if ms.saturating_sub(since) >= self.p.sup_fault_persist_ms { self.fault = true; }
        } else {
            self.supply_since = None;
            let since = *self.supply_clear_since.get_or_insert(ms);
            if ms.saturating_sub(since) >= self.p.sup_fault_persist_ms { self.fault = false; }
        }
    }

    fn sample_disagreement(&mut self, ms: u64, init: bool) {
        let rear = &self.channels[1];
        let d = !init && self.contact == Some(false) && rear.electrical == Status::Ok
            && rear.pressure.is_some_and(|p| p > self.b.contact_disagree_bar);
        if d {
            self.recovery_since = None;
            let since = *self.disagreement_since.get_or_insert(ms);
            if ms.saturating_sub(since) >= self.b.contact_persist_ms {
                let front = &self.channels[0];
                self.disagreement = if front.electrical == Status::Ok
                    && front.pressure.is_some_and(|p| p < self.b.brake_on_bar) {
                    Disagreement::Rear
                } else { Disagreement::Contact };
            }
        } else {
            self.disagreement_since = None;
            let since = *self.recovery_since.get_or_insert(ms);
            if ms.saturating_sub(since) >= self.b.contact_recover_ms {
                self.disagreement = Disagreement::None;
            }
        }
        match self.disagreement {
            Disagreement::Contact => self.contact = None,
            Disagreement::Rear if self.channels[1].electrical == Status::Ok => {
                self.channels[1].status = Status::Implausible;
            }
            _ => {}
        }
    }

    fn sample_brake(&mut self, init: bool) {
        let pressure = self.channels.iter().filter(|c| c.status == Status::Ok)
            .filter_map(|c| c.pressure).reduce(f64::max);
        self.brake_qualified = pressure.is_some() && !init;
        if init { self.brake = false; }
        else if let Some(p) = pressure {
            if p >= self.b.brake_on_bar { self.brake = true; }
            else if p < self.b.brake_off_bar { self.brake = false; }
        } else { self.brake = self.contact.unwrap_or(false); }
    }

    fn status(&self, x: usize) -> f64 {
        (match self.channels[x].status { Status::Init => self.p.st_init, Status::Ok => self.p.st_ok,
            Status::Low => self.p.st_lo, Status::High => self.p.st_hi,
            Status::Supply => self.b.status_supply, Status::Implausible => self.b.status_implausible }) as f64
    }

    fn pressure(&self, x: usize) -> Option<f64> {
        if self.channels[x].status == Status::Ok { self.channels[x].pressure } else { None }
    }

    fn mode(&self) -> f64 {
        let init = self.channels.iter().any(|c| c.status == Status::Init);
        let bad = self.channels.iter().any(|c| !matches!(c.status, Status::Ok | Status::Init));
        (if self.fault { self.p.mode_fault }
            else if bad || (!init && self.contact.is_none()) { self.p.mode_degraded }
            else if init { self.p.mode_init } else { self.p.mode_run }) as f64
    }

    pub fn value(&self, source: &str) -> Option<f64> {
        match source {
            "bps.p_front" => self.pressure(0), "bps.p_rear" => self.pressure(1),
            "bps.p_front_st" => Some(self.status(0)), "bps.p_rear_st" => Some(self.status(1)),
            "bps.brk_sw" => self.contact.map(|closed| self.b.switch_codes[closed as usize] as f64),
            "bps.brk_act" => Some(self.brake as u8 as f64),
            "bps.brk_act_q" => Some(self.brake_qualified as u8 as f64),
            "bps.pf_ratio" => self.channels[0].ratio, "bps.pr_ratio" => self.channels[1].ratio,
            "bps.sup5v" => Some(self.supply.min(self.b.supply_report_max_v)),
            "bps.ecu_mode" => Some(self.mode()), "bps.lv_uv" => Some(self.lv_uv as u8 as f64),
            "bps.sens_sup_flt" => Some(self.supply_fault as u8 as f64), "bps.v_lv" => Some(self.lv),
            // Source temperature is supplied by the common engine thermal model.
            "bps.ecu_temp" => None, _ => None,
        }
    }
}

#[cfg(all(test, n7rt_oracle))]
mod tests {
    include!(concat!(env!("N7RT_ORACLE_SRC"), "/tests/ecu_bps.rs"));
}
