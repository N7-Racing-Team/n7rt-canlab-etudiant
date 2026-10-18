//! Mission source ECU sampled every 1 ms from the Physical Lab probe (dual ratiometric travel sensor):
//! power gate (reset / reset exit / t_boot), ADC codes → pin voltages → ratio, moving average, windows with
//! asymmetric debounce and pending state, INIT after the sensor rails, travel, statuses, supply flag with
//! debounce and persistence, ECU mode, LV under-voltage flag and LV measurement.

use crate::pack::SourceEcu;
use crate::EcuInputs;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    In,
    Lo,
    Hi,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChStatus {
    Init,
    Ok,
    Lo,
    Hi,
}

#[derive(Debug, Clone)]
struct ChannelState {
    status: ChStatus,
    out_count: u32,
    in_count: u32,
    last_side: Side,
    ratio_hist: Vec<f64>,
    sup_hist: Vec<f64>,
    ratio_avg: f64,
    sup_avg: f64,
    sup_now: f64,
    ratio_undefined: bool,
    travel: f64,
    last_emitted: Option<f64>,
}

impl ChannelState {
    fn new() -> ChannelState {
        ChannelState {
            status: ChStatus::Init,
            out_count: 0,
            in_count: 0,
            last_side: Side::Lo,
            ratio_hist: Vec::new(),
            sup_hist: Vec::new(),
            ratio_avg: 0.0,
            sup_avg: 0.0,
            sup_now: 0.0,
            ratio_undefined: true,
            travel: 0.0,
            last_emitted: None,
        }
    }

    fn pending(&self) -> bool {
        self.status == ChStatus::Ok && self.out_count > 0
    }
}

fn push_avg(hist: &mut Vec<f64>, n: usize, v: f64) -> f64 {
    // a smaller window after a reconfiguration drops the oldest samples at once
    while hist.len() >= n.max(1) {
        hist.remove(0);
    }
    hist.push(v);
    hist.iter().sum::<f64>() / hist.len() as f64
}

#[derive(Debug, Clone)]
pub(crate) struct EcuApps {
    p: SourceEcu,
    reset_below_v: f64,
    reset_exit_v: f64,
    t_boot_ms: u64,
    running: bool,
    t_reset_exit: u64,
    lv_hist: Vec<f64>,
    lv_avg: f64,
    lv_uv: bool,
    ch: [ChannelState; 2],
    sup_flag: bool,
    sup_out_count: u32,
    sup_in_count: u32,
    flag_since: Option<u64>,
    clear_since: Option<u64>,
    fault: bool,
    /// Incremented at every reset exit (frame counters restart at 0).
    pub run_id: u64,
}

impl EcuApps {
    /// New static configuration (same mission), dynamic state kept.
    pub fn reconfigure(&mut self, p: &SourceEcu) {
        self.p = p.clone();
    }

    pub fn new(p: &SourceEcu, reset_below_v: f64, reset_exit_v: f64, t_boot_ms: u64) -> EcuApps {
        EcuApps {
            p: p.clone(),
            reset_below_v,
            reset_exit_v,
            t_boot_ms,
            running: false,
            t_reset_exit: 0,
            lv_hist: Vec::new(),
            lv_avg: 0.0,
            lv_uv: false,
            ch: [ChannelState::new(), ChannelState::new()],
            sup_flag: false,
            sup_out_count: 0,
            sup_in_count: 0,
            flag_since: None,
            clear_since: None,
            fault: false,
            run_id: 0,
        }
    }

    fn reset_logic(&mut self) {
        self.ch = [ChannelState::new(), ChannelState::new()];
        self.sup_flag = false;
        self.sup_out_count = 0;
        self.sup_in_count = 0;
        self.flag_since = None;
        self.clear_since = None;
        self.fault = false;
        self.lv_uv = false;
    }

    /// Out of reset and past t_boot at `t_ms`: frames may be emitted.
    pub fn can_emit(&self, t_ms: u64, nominal_us: u64) -> bool {
        self.running && t_ms >= self.t_reset_exit && nominal_us >= (self.t_reset_exit + self.t_boot_ms) * 1000
    }

    fn code_to_v(&self, code: Option<u32>, divider: f64) -> f64 {
        match code {
            None => 0.0,
            Some(c) => {
                let full = (1u64 << self.p.adc_bits) as f64;
                let c = (c as f64).min(full - 1.0);
                divider * c * self.p.adc_vref_v / full
            }
        }
    }

    /// One 1-ms sample at `t_ms`.
    pub fn sample(&mut self, t_ms: u64, inp: &EcuInputs) {
        let lv = inp.lv.unwrap_or(0.0);
        self.lv_avg = push_avg(&mut self.lv_hist, self.p.lv_avg_samples, lv);
        if self.running {
            if lv < self.reset_below_v {
                self.running = false;
                self.reset_logic();
                return;
            }
        } else {
            if lv >= self.reset_exit_v {
                self.running = true;
                self.t_reset_exit = t_ms;
                self.run_id += 1;
                self.reset_logic();
            } else {
                return;
            }
        }
        if self.lv_uv {
            if self.lv_avg >= self.p.lv_uv_off_v {
                self.lv_uv = false;
            }
        } else if self.lv_avg < self.p.lv_uv_on_v {
            self.lv_uv = true;
        }
        let rails_on = self.t_reset_exit + self.t_boot_ms;
        if t_ms < rails_on {
            return;
        }
        let init_end = rails_on + self.p.init_after_rails_ms;
        let p = self.p.clone();
        let mut sup_out = false;
        for x in 0..2 {
            let v_sig = self.code_to_v(inp.adc[x], p.sig_divider);
            let v_sup = self.code_to_v(inp.adc[2 + x], p.sup_divider);
            let c = &mut self.ch[x];
            let cp = &p.channels[x];
            c.sup_now = v_sup;
            c.ratio_undefined = v_sup < p.ratio_undefined_below_v;
            let r = if c.ratio_undefined { 0.0 } else { 100.0 * v_sig / v_sup };
            c.ratio_avg = push_avg(&mut c.ratio_hist, p.filter_samples, r);
            c.sup_avg = push_avg(&mut c.sup_hist, p.filter_samples, v_sup);
            if (v_sup - p.sup_nominal_v).abs() > p.sup_tolerance_v {
                sup_out = true;
            }
            let side = if v_sup < p.supply_lo_v || c.ratio_undefined {
                Side::Lo
            } else if v_sup > p.supply_hi_v {
                Side::Hi
            } else if c.ratio_avg < cp.window_lo {
                Side::Lo
            } else if c.ratio_avg > cp.window_hi {
                Side::Hi
            } else {
                Side::In
            };
            if side == Side::In {
                c.in_count += 1;
                c.out_count = 0;
            } else {
                c.out_count += 1;
                c.in_count = 0;
                c.last_side = side;
            }
            let fault_status = if c.last_side == Side::Hi { ChStatus::Hi } else { ChStatus::Lo };
            if t_ms < init_end {
                c.status = ChStatus::Init;
            } else if c.status == ChStatus::Init {
                c.status = if c.in_count >= p.debounce_fault { ChStatus::Ok } else { fault_status };
            } else if c.status == ChStatus::Ok {
                if c.out_count >= p.debounce_fault {
                    c.status = fault_status;
                }
            } else if c.in_count >= p.debounce_recover {
                c.status = ChStatus::Ok;
            } else if side != Side::In {
                c.status = fault_status;
            }
            c.travel = (100.0 * (c.ratio_avg - cp.a) / cp.b).clamp(p.travel_lo, p.travel_hi);
        }
        // supply tolerance flag (debounce rise/fall) and FAULT persistence
        if sup_out {
            self.sup_out_count += 1;
            self.sup_in_count = 0;
            if self.sup_out_count >= p.sup_rise {
                self.sup_flag = true;
            }
        } else {
            self.sup_in_count += 1;
            self.sup_out_count = 0;
            if self.sup_in_count >= p.sup_fall {
                self.sup_flag = false;
            }
        }
        if self.sup_flag {
            self.clear_since = None;
            let since = *self.flag_since.get_or_insert(t_ms);
            if t_ms - since >= p.sup_fault_persist_ms {
                self.fault = true;
            }
        } else {
            self.flag_since = None;
            let since = *self.clear_since.get_or_insert(t_ms);
            if self.fault && t_ms - since >= p.sup_fault_persist_ms {
                self.fault = false;
            }
        }
    }

    fn status_code(&self, s: ChStatus) -> i64 {
        match s {
            ChStatus::Init => self.p.st_init,
            ChStatus::Ok => self.p.st_ok,
            ChStatus::Lo => self.p.st_lo,
            ChStatus::Hi => self.p.st_hi,
        }
    }

    pub fn status(&self, x: usize) -> f64 {
        self.status_code(self.ch[x].status) as f64
    }

    /// Travel to emit now: current value under OK, the last emitted value while pending, SNA otherwise.
    pub fn travel_for_emission(&mut self, x: usize) -> Option<f64> {
        let c = &mut self.ch[x];
        let v = match c.status {
            ChStatus::Ok if c.pending() => c.last_emitted,
            ChStatus::Ok => Some(c.travel),
            _ => None,
        };
        c.last_emitted = v;
        v
    }

    pub fn mode(&self) -> f64 {
        let st = [self.ch[0].status, self.ch[1].status];
        let m = if self.fault {
            self.p.mode_fault
        } else if st.iter().any(|s| matches!(s, ChStatus::Lo | ChStatus::Hi)) {
            self.p.mode_degraded
        } else if st.contains(&ChStatus::Init) {
            self.p.mode_init
        } else {
            self.p.mode_run
        };
        m as f64
    }

    pub fn lv_uv(&self) -> f64 {
        if self.lv_uv {
            1.0
        } else {
            0.0
        }
    }

    pub fn sup_flag(&self) -> f64 {
        if self.sup_flag {
            1.0
        } else {
            0.0
        }
    }

    pub fn v_lv(&self) -> f64 {
        self.lv_avg
    }

    pub fn ratio(&self, x: usize) -> Option<f64> {
        let c = &self.ch[x];
        if c.ratio_undefined {
            None
        } else {
            Some(c.ratio_avg)
        }
    }

    pub fn supply(&self, x: usize) -> f64 {
        self.ch[x].sup_avg
    }
}
