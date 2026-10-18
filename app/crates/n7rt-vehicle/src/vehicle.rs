//! Vehicle engine: scheduler (t = φ + k·T + jitter), protection, counters, mission source ECU, background ECUs
//! (VCU, inverter, BMS, dashboard, logger, static student ECUs) and the world laws of the background frames.
//!
//! Time advances in 1-ms samples. A frame emitted at t carries the state after the sample ⌊t / 1 ms⌋. The VCU and
//! the inverter apply the consumer model to the bus at their 10-ms ticks (k ≥ 1), before the frames of the same
//! millisecond.

use std::f64::consts::PI;
use std::sync::Arc;

use crate::codec;
use crate::consumer::AppsCore;
use crate::e2e;
use crate::source_model::SourceModel;
use crate::pack::{PackData, Protection, Role};
use crate::prng;
use crate::supervision::Monitor;
use crate::vocab::Dynamic;
use crate::{BusFrame, EcuInputs};

const VCU_TICK_MS: u64 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Src {
    Header,
    Dyn(Dynamic),
    Static { ecu: usize, key: usize },
    StaticTemp(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ThermalKind {
    Source,
    Static(usize),
    Motor,
    Igbt,
    PackMax,
}

#[derive(Debug, Clone)]
struct Thermal {
    kind: ThermalKind,
    t: f64,
    steps: u64,
}

#[derive(Debug, Clone)]
struct Sched {
    k: u64,
    counter: u64,
    run_id: u64,
    source: bool,
}

/// Index pair (monitor, signal) of a VCU input.
type InputRef = (usize, usize);

/// Payload corruption after protection (message incidents): `mask` is XORed into byte `byte` of the frames `id` on
/// the emission ranks r where `(r + phase) mod period < burst`, r = 0 for the first emission after installation.
/// The bytes are changed before any receiver sees them, so the bus carries the corrupted payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameFault {
    pub id: u32,
    pub byte: u8,
    pub mask: u8,
    pub period: u16,
    pub burst: u16,
    pub phase: u16,
}

impl FrameFault {
    /// Whether the emission of rank `r` is corrupted.
    pub fn hits(&self, r: u64) -> bool {
        (r + u64::from(self.phase)) % u64::from(self.period) < u64::from(self.burst)
    }
}

pub(crate) struct Engine {
    pack: Arc<PackData>,
    seed: u64,
    now_us: u64,
    sampled_ms: Option<u64>,
    srcs: Vec<Vec<Src>>,
    sched: Vec<Sched>,
    /// The installed corruption and the emission rank of its next frame.
    fault: Option<(FrameFault, u64)>,
    ecu: SourceModel,
    thermals: Vec<Thermal>,
    // world (constant in this slice) and derived kinematics
    n_rear: [f64; 2],
    yaw_rad_s: f64,
    // VCU
    vcu_apps: AppsCore,
    vcu_mons: Vec<Monitor>,
    in_angle: Option<InputRef>,
    in_wheels: Option<[InputRef; 2]>,
    in_motor: Option<InputRef>,
    in_brake: Option<InputRef>,
    in_pfront: Option<InputRef>,
    last_motor_rpm: Option<f64>,
    tc_active: bool,
    brake_ever: bool,
    brake_last: f64,
    brake_loss_since: Option<u64>,
    trq_mode: i64,
    inv_en: bool,
    trq_req: f64,
    trq_lim: f64,
    veh_speed: Option<f64>,
    brake_light: bool,
    // inverter
    inv_mon: Option<(Monitor, usize, usize)>,
    inv_fault: bool,
    inv_last_en: bool,
    inv_req: f64,
    trq_act: f64,
    i_dc: f64,
    // BMS
    soc: f64,
    i_pack: f64,
    u_pack: f64,
    idx_cells: [f64; 2],
    // logger
    mem_free0: f64,
    log_first_ms: Option<u64>,
}

fn locate(pack: &PackData, mons: &mut Vec<Monitor>, source: &str) -> Option<InputRef> {
    let &(fi, si) = pack.sources.get(source)?;
    let mi = match mons.iter().position(|m| m.frame == fi) {
        Some(i) => i,
        None => {
            mons.push(Monitor::new(pack, fi));
            mons.len() - 1
        }
    };
    Some((mi, si))
}

impl Engine {
    pub fn new(pack: Arc<PackData>, seed: u64) -> Engine {
        // the variant's jitter and drift seed (0 for a pack without one)
        let seed = seed ^ pack.jitter_seed;
        let p = &*pack;
        let bg = &p.background;
        let mut thermals = vec![Thermal { kind: ThermalKind::Source, t: 0.0, steps: 0 }];
        for i in 0..bg.static_ecus.len() {
            thermals.push(Thermal { kind: ThermalKind::Static(i), t: 0.0, steps: 0 });
        }
        for kind in [ThermalKind::Motor, ThermalKind::Igbt, ThermalKind::PackMax] {
            thermals.push(Thermal { kind, t: 0.0, steps: 0 });
        }
        let srcs = p
            .frames
            .iter()
            .map(|f| {
                f.signals
                    .iter()
                    .map(|s| {
                        if s.role != Role::Value {
                            return Src::Header;
                        }
                        if let Some(d) = Dynamic::for_mission(&s.source, &p.mission) {
                            return Src::Dyn(d);
                        }
                        for (ei, e) in bg.static_ecus.iter().enumerate() {
                            if let Some(k) = s.source.strip_prefix(e.prefix.as_str()).and_then(|r| r.strip_prefix('.')) {
                                if k == "ecu_temp" {
                                    return Src::StaticTemp(ei);
                                }
                                if let Some(ki) = e.keys.iter().position(|x| x == k) {
                                    return Src::Static { ecu: ei, key: ki };
                                }
                            }
                        }
                        Src::Header
                    })
                    .collect()
            })
            .collect();
        let sched = p
            .frames
            .iter()
            .map(|f| Sched { k: 0, counter: 0, run_id: 0, source: f.emitter == p.source_ecu.emitter })
            .collect();
        let vi = &bg.vcu_inputs;
        let mut vcu_mons = Vec::new();
        let in_angle = vi.steering_angle.as_deref().and_then(|s| locate(p, &mut vcu_mons, s));
        let in_wheels = vi.wheels_front.as_ref().and_then(|w| {
            Some([locate(p, &mut vcu_mons, &w[0])?, locate(p, &mut vcu_mons, &w[1])?])
        });
        let in_motor = vi.motor_speed.as_deref().and_then(|s| locate(p, &mut vcu_mons, s));
        let in_brake = vi.brake_active.as_deref().and_then(|s| locate(p, &mut vcu_mons, s));
        let in_pfront = vi.brake_pressure_front.as_deref().and_then(|s| locate(p, &mut vcu_mons, s));
        let inv_mon = match (p.sources.get("vcu.trq_req"), p.sources.get("vcu.inv_en")) {
            (Some(&(f1, s1)), Some(&(f2, s2))) if f1 == f2 => Some((Monitor::new(p, f1), s1, s2)),
            _ => None,
        };
        let v = &bg.vehicle;
        let steps = ((v.mem_free_hi - v.mem_free_lo) / v.mem_free_step).round().max(0.0) as i64;
        let mem_free0 =
            v.mem_free_lo + v.mem_free_step * prng::uniform_int(seed, prng::STREAM_LOG_MEMORY, 0, 0, steps) as f64;
        let n_cells = v.n_s.round().max(2.0) as i64;
        let idx_min = prng::uniform_int(seed, prng::STREAM_CELL_INDEX, 0, 0, n_cells - 1);
        let mut idx_max = prng::uniform_int(seed, prng::STREAM_CELL_INDEX, 1, 0, n_cells - 2);
        if idx_max >= idx_min {
            idx_max += 1;
        }
        let w = &bg.world;
        let yaw_rad_s = if w.v_mps == 0.0 || w.delta_sw_deg == 0.0 {
            0.0
        } else {
            w.v_mps * (w.delta_sw_deg * PI / 180.0 / v.steer_ratio).tan() / v.wheelbase_m
        };
        let rear = |sign: f64| {
            (w.v_mps + sign * yaw_rad_s * v.track_rear_m / 2.0) / v.r_eff_m * (1.0 + w.slip) * 30.0 / PI
        };
        let u_pack = v.n_s * (v.ocv_base_v + v.ocv_span_v * w.soc0);
        let mut e = Engine {
            ecu: SourceModel::new(p),
            seed,
            now_us: 0,
            sampled_ms: None,
            srcs,
            sched,
            fault: None,
            thermals,
            n_rear: [rear(-1.0), rear(1.0)],
            yaw_rad_s,
            vcu_apps: AppsCore::new(pack.clone()),
            vcu_mons,
            in_angle,
            in_wheels,
            in_motor,
            in_brake,
            in_pfront,
            last_motor_rpm: None,
            tc_active: false,
            brake_ever: false,
            brake_last: 0.0,
            brake_loss_since: None,
            trq_mode: bg.codes.trq_disabled,
            inv_en: false,
            trq_req: 0.0,
            trq_lim: v.c_cons_nm,
            veh_speed: None,
            brake_light: false,
            inv_mon,
            inv_fault: false,
            inv_last_en: false,
            inv_req: 0.0,
            trq_act: 0.0,
            i_dc: 0.0,
            soc: w.soc0,
            i_pack: 0.0,
            u_pack,
            idx_cells: [idx_min as f64, idx_max as f64],
            mem_free0,
            log_first_ms: None,
            pack,
        };
        for i in 0..e.thermals.len() {
            e.thermals[i].t = e.thermal_target(e.thermals[i].kind);
        }
        e.vcu_decide(0, None);
        e
    }

    pub fn now_us(&self) -> u64 {
        self.now_us
    }

    /// Swaps the source ECU's static configuration for that of `pack` (same mission and matrix: a source override of
    /// the same variant); every dynamic state is kept.
    pub fn reconfigure_source(&mut self, pack: Arc<PackData>) -> Result<(), String> {
        // the schedule (source frames included) is built once: the emitters stay those of the running vehicle
        let same = pack.mission == self.pack.mission
            && pack.source_ecu.emitter == self.pack.source_ecu.emitter
            && pack.frames.len() == self.pack.frames.len()
            && pack.frames.iter().zip(&self.pack.frames).all(|(a, b)| a.id == b.id && a.period_us == b.period_us && a.dlc == b.dlc && a.emitter == b.emitter);
        if !same {
            return Err("reconfiguration : autre mission ou autre matrice".into());
        }
        self.ecu.reconfigure(&pack);
        self.pack = pack;
        Ok(())
    }

    /// Installs (rank restarted at 0) or removes the payload corruption. Refuses an identifier the vehicle does not
    /// emit, a byte outside its payload, an empty mask or an inconsistent pattern.
    pub fn set_frame_fault(&mut self, fault: Option<FrameFault>) -> Result<(), String> {
        if let Some(f) = &fault {
            let frame = self.pack.frames.iter().find(|x| x.id == f.id).ok_or("corruption : identifiant non émis")?;
            if f.byte >= frame.dlc || f.mask == 0 || f.period == 0 || f.burst > f.period || f.phase >= f.period {
                return Err("corruption : paramètres incohérents".into());
            }
        }
        self.fault = fault.map(|f| (f, 0));
        Ok(())
    }

    // ------------------------------------------------------------------------------------------- time stepping

    pub fn advance(&mut self, t_us: u64, inputs: &mut dyn FnMut(u64) -> EcuInputs) -> Vec<BusFrame> {
        if t_us <= self.now_us && self.sampled_ms.is_some() {
            return Vec::new();
        }
        if t_us == 0 {
            return Vec::new();
        }
        let mut out = Vec::new();
        if let Some(m) = self.sampled_ms {
            self.emit(m, t_us, &mut out);
        }
        let first = self.sampled_ms.map_or(0, |m| m + 1);
        for m in first..=t_us / 1000 {
            let inp = inputs(m);
            self.sample(m, &inp);
            self.sampled_ms = Some(m);
            self.emit(m, t_us, &mut out);
        }
        self.now_us = t_us;
        out.sort_by_key(|f| (f.t_us, f.id));
        out
    }

    fn sample(&mut self, m: u64, inp: &EcuInputs) {
        self.ecu.sample(m, inp);
        self.world_step();
        if m > 0 && m % VCU_TICK_MS == 0 {
            let t = m * 1000;
            self.inv_tick(t);
            self.vcu_tick(t);
        }
    }

    fn emission_time(&self, fi: usize) -> (u64, u64) {
        let f = &self.pack.frames[fi];
        let k = self.sched[fi].k;
        let nominal = f.phase_us + k * f.period_us;
        let j = self.pack.jitter_us as i64;
        let jitter = prng::uniform_int(self.seed, prng::STREAM_JITTER + fi as u64, k, -j, j);
        (nominal, (nominal as i64 + jitter).max(0) as u64)
    }

    fn emit(&mut self, m: u64, t_us: u64, out: &mut Vec<BusFrame>) {
        let lo = m * 1000;
        let hi = (m + 1) * 1000;
        for fi in 0..self.pack.frames.len() {
            loop {
                let (nominal, te) = self.emission_time(fi);
                if te >= hi || te > t_us {
                    break;
                }
                self.sched[fi].k += 1;
                if te < lo {
                    continue;
                }
                let alive = if self.sched[fi].source {
                    let ok = self.ecu.can_emit(m, nominal);
                    if ok && self.sched[fi].run_id != self.ecu.run_id() {
                        self.sched[fi].run_id = self.ecu.run_id();
                        self.sched[fi].counter = 0;
                    }
                    ok
                } else {
                    nominal >= self.pack.power.t_boot_ms * 1000
                };
                if !alive {
                    continue;
                }
                // The source ECU computes with what it has sampled when the frame leaves: the nominal instant (whole
                // ms) when the jitter delays the frame, the last sampled millisecond when it advances it.
                if self.sched[fi].source { self.ecu.prepare_emission(nominal.min(lo)); }
                let mut data = self.build(fi, m);
                let id = self.pack.frames[fi].id;
                if let Some((f, rank)) = &mut self.fault {
                    if f.id == id {
                        if f.hits(*rank) {
                            data[f.byte as usize] ^= f.mask;
                        }
                        *rank += 1;
                    }
                }
                self.vcu_apps.on_frame(te, id, 0, &data);
                for mon in &mut self.vcu_mons {
                    mon.on_frame(te, id, 0, &data);
                }
                if let Some((mon, _, _)) = &mut self.inv_mon {
                    mon.on_frame(te, id, 0, &data);
                }
                out.push(BusFrame { t_us: te, id, flags: 0, dlc: data.len() as u8, data });
            }
        }
    }

    fn build(&mut self, fi: usize, m: u64) -> Vec<u8> {
        let pack = self.pack.clone();
        let f = &pack.frames[fi];
        let mut data = vec![0xFFu8; f.dlc as usize];
        for (si, s) in f.signals.iter().enumerate() {
            let src = self.srcs[fi][si];
            if src == Src::Header {
                continue;
            }
            let v = self.value(src, m);
            codec::write_value(s, &mut data, v);
        }
        let counter = self.sched[fi].counter;
        self.sched[fi].counter = counter.wrapping_add(1);
        match &f.protection {
            Protection::P22 { data_ids } => e2e::p22_protect(&mut data, (counter & 0x0F) as u8, data_ids),
            Protection::Alive { bits, signal } => {
                codec::write_raw(&f.signals[*signal], &mut data, (counter % (1u64 << bits)) as i64)
            }
            Protection::None => {}
        }
        data
    }

    // --------------------------------------------------------------------------------------------- quantities

    fn thermal_index(&self, kind: ThermalKind) -> usize {
        self.thermals.iter().position(|t| t.kind == kind).unwrap_or(0)
    }

    fn thermal_target(&self, kind: ThermalKind) -> f64 {
        let bg = &self.pack.background;
        let v = &bg.vehicle;
        let t_amb = bg.world.t_amb_c;
        let mech_kw = (self.trq_act * self.motor_rpm() * PI / 30.0).abs() / 1000.0;
        match kind {
            ThermalKind::Source => t_amb + self.pack.source_ecu.self_heating_k,
            ThermalKind::Static(i) => t_amb + bg.static_ecus[i].self_heating_k,
            ThermalKind::Motor => t_amb + v.t_motor_k_per_kw * mech_kw,
            ThermalKind::Igbt => t_amb + v.t_igbt_k_per_kw * mech_kw,
            ThermalKind::PackMax => t_amb + v.t_max_k_per_kw * v.r_i_ohm * self.i_pack * self.i_pack / 1000.0,
        }
    }

    /// Temperature carried by a frame: one PRNG step in [−step ; +step] per emission, then the value.
    fn thermal_emit(&mut self, kind: ThermalKind) -> f64 {
        let i = self.thermal_index(kind);
        let step = self.pack.background.vehicle.temp_step_c;
        let th = &mut self.thermals[i];
        let u = prng::uniform_unit(self.seed, prng::STREAM_THERMAL + i as u64, th.steps);
        th.steps += 1;
        th.t += (2.0 * u - 1.0) * step;
        th.t
    }

    fn motor_rpm(&self) -> f64 {
        self.pack.background.vehicle.gear_ratio * (self.n_rear[0] + self.n_rear[1]) / 2.0
    }

    fn inv_state(&self) -> i64 {
        let c = &self.pack.background.codes;
        if self.inv_fault {
            c.inv_fault
        } else if self.inv_last_en {
            c.inv_run
        } else if self.u_dc() >= self.pack.background.vehicle.inv_ready_v {
            c.inv_ready
        } else {
            c.inv_standby
        }
    }

    fn u_dc(&self) -> f64 {
        if self.pack.background.world.air_closed {
            self.u_pack
        } else {
            0.0
        }
    }

    fn bool(b: bool) -> Option<f64> {
        Some(if b { 1.0 } else { 0.0 })
    }

    fn value(&mut self, src: Src, m: u64) -> Option<f64> {
        use Dynamic::*;
        let pack = self.pack.clone();
        let bg = &pack.background;
        let w = &bg.world;
        let v = &bg.vehicle;
        let c = &bg.codes;
        let d = match src {
            Src::Header => return None,
            Src::Static { ecu, key } => {
                let e = &bg.static_ecus[ecu];
                let phase = e.phases.iter().find(|(until, _)| match until {
                    None => true,
                    Some(u) => m < *u,
                });
                return phase.and_then(|(_, vals)| vals[key]);
            }
            Src::StaticTemp(i) => return Some(self.thermal_emit(ThermalKind::Static(i))),
            Src::Dyn(d) => d,
        };
        match d {
            Mission(name) if name.ends_with(".ecu_temp") => Some(self.thermal_emit(ThermalKind::Source)),
            Mission(name) => self.ecu.value(name),
            AppsStatus(x) => self.ecu.value(if x == 0 { "apps.st1" } else { "apps.st2" }),
            AppsTravel(x) => self.ecu.value(if x == 0 { "apps.travel1" } else { "apps.travel2" }),
            AppsMode => self.ecu.value("apps.ecu_mode"),
            AppsLvUv => self.ecu.value("apps.lv_uv"),
            AppsSupFlt => self.ecu.value("apps.sens_sup_flt"),
            AppsVlv => self.ecu.value("apps.v_lv"),
            AppsTemp => Some(self.thermal_emit(ThermalKind::Source)),
            AppsRatio(x) => self.ecu.value(if x == 0 { "apps.ratio1" } else { "apps.ratio2" }),
            AppsSupply(x) => self.ecu.value(if x == 0 { "apps.sup1" } else { "apps.sup2" }),
            VcuTrqMode => Some(self.trq_mode as f64),
            VcuInvEn => Self::bool(self.inv_en),
            VcuTrqReq => Some(self.trq_req),
            VcuTrqLim => Some(self.trq_lim),
            VcuWsStill => {
                Self::bool(self.n_rear[0] == 0.0 && self.n_rear[1] == 0.0 && m >= v.ws_stop_ms)
            }
            VcuWs(x) => Some(self.n_rear[x]),
            VcuWsSt(_) => Some(c.ws_ok as f64),
            VcuImuSt => Some(if m < v.init_ms { c.imu_init } else { c.imu_ok } as f64),
            VcuYawRate => Some(self.yaw_rad_s * 180.0 / PI),
            VcuAccLat => Some(w.v_mps * self.yaw_rad_s),
            VcuAccLong => Some(0.0),
            VcuVehSt => Some(w.veh_st as f64),
            VcuBrakeLight => Self::bool(self.brake_light),
            VcuR2dSound => Some(w.r2d_sound as f64),
            VcuSdcClosed => Some(w.sdc_closed as f64),
            VcuTcActive => Self::bool(self.tc_active),
            VcuVehSpeed => self.veh_speed,
            InvSt => Some(self.inv_state() as f64),
            InvMotSpeed => Some(self.motor_rpm()),
            InvTrqAct => Some(self.trq_act),
            InvIdc => Some(self.i_dc),
            InvTMotor => Some(self.thermal_emit(ThermalKind::Motor)),
            InvTIgbt => Some(self.thermal_emit(ThermalKind::Igbt)),
            InvUdc => Some(self.u_dc()),
            BmsSt => Some(if m < v.init_ms {
                c.bms_init
            } else if w.air_closed {
                c.bms_active
            } else {
                c.bms_idle
            } as f64),
            BmsUpack => Some(self.u_pack),
            BmsIpack => Some(self.i_pack),
            BmsSoc => Some(self.soc * 100.0),
            BmsTmax => Some(self.thermal_emit(ThermalKind::PackMax)),
            BmsVcellMin => Some(self.u_pack / v.n_s - v.dv_cell_v),
            BmsVcellMax => Some(self.u_pack / v.n_s + v.dv_cell_v),
            BmsTcellMax => Some(self.thermals[self.thermal_index(ThermalKind::PackMax)].t),
            BmsTcellMin => Some(self.thermals[self.thermal_index(ThermalKind::PackMax)].t - v.dt_cell_k),
            BmsIdxVmin => Some(self.idx_cells[0]),
            BmsIdxVmax => Some(self.idx_cells[1]),
            DashTsBtn => Some(w.ts_btn as f64),
            DashR2dBtn => Some(w.r2d_btn as f64),
            DashDriveMode => Some(w.drive_mode as f64),
            DashTrqMap => Some(w.trq_map as f64),
            LogSt | LogMemFree => {
                let first = *self.log_first_ms.get_or_insert(m);
                let drops = (m - first) / v.mem_free_period_ms;
                let mem = (self.mem_free0 - v.mem_free_step * drops as f64).max(0.0);
                if d == LogSt {
                    Some(if mem <= 0.0 { c.log_full } else { c.log_recording } as f64)
                } else {
                    Some(mem)
                }
            }
            LogUptime => Some(((m / 1000) % 65_536) as f64),
        }
    }

    // ------------------------------------------------------------------------------------------ world and ECUs

    /// 1-ms step of the continuous laws: realised torque (first order), DC current, pack, temperatures.
    fn world_step(&mut self) {
        let bg = &self.pack.background;
        let v = &bg.vehicle;
        let target = if self.inv_state() == bg.codes.inv_run { self.inv_req } else { 0.0 };
        self.trq_act += (target - self.trq_act) / v.tau_trq_ms;
        let omega = self.motor_rpm() * PI / 30.0;
        let u_prev = self.u_pack.max(1.0);
        self.i_dc = if self.trq_act >= 0.0 {
            self.trq_act * omega / (v.eta * u_prev)
        } else {
            v.eta * self.trq_act * omega / u_prev
        };
        self.i_pack = if bg.world.air_closed { self.i_dc } else { 0.0 };
        self.soc = (self.soc - self.i_pack * 0.001 / (3600.0 * v.q_nom_ah)).clamp(0.0, 1.0);
        self.u_pack = v.n_s * (v.ocv_base_v + v.ocv_span_v * self.soc) - v.r_i_ohm * self.i_pack;
        let tau_ms = v.tau_th_s * 1000.0;
        for i in 0..self.thermals.len() {
            let target = self.thermal_target(self.thermals[i].kind);
            let th = &mut self.thermals[i];
            th.t += (target - th.t) / tau_ms;
        }
    }

    /// Inverter consumer of the torque command: loss → FAULT (latched), RUN on the last qualified enable.
    fn inv_tick(&mut self, t: u64) {
        let pack = self.pack.clone();
        let Some((mon, s_req, s_en)) = &mut self.inv_mon else { return };
        let frame = &pack.frames[mon.frame];
        mon.on_tick(frame, t);
        if mon.rx_timeout {
            self.inv_fault = true;
        }
        if let Some(data) = mon.fresh_data() {
            self.inv_last_en = codec::read_raw(&frame.signals[*s_en], data) == 1;
        }
        self.inv_req = mon.value(frame, *s_req).unwrap_or(frame.signals[*s_req].substitute.unwrap_or(0.0));
    }

    fn input(&self, r: Option<InputRef>) -> Option<f64> {
        let (mi, si) = r?;
        let m = &self.vcu_mons[mi];
        m.value(&self.pack.frames[m.frame], si)
    }

    fn vcu_tick(&mut self, t: u64) {
        let apps = self.vcu_apps.on_tick(t);
        let pack = self.pack.clone();
        for m in &mut self.vcu_mons {
            m.on_tick(&pack.frames[m.frame], t);
        }
        self.vcu_decide(t, Some((apps.latched, apps.retained)));
    }

    fn vcu_decide(&mut self, t: u64, apps: Option<(bool, f64)>) {
        let pack = self.pack.clone();
        let bg = &pack.background;
        let v = &bg.vehicle;
        let c = &bg.codes;
        let w = &bg.world;
        let angle = self.input(self.in_angle);
        let wheels = self.in_wheels.and_then(|r| Some([self.input(Some(r[0]))?, self.input(Some(r[1]))?]));
        let motor = self.input(self.in_motor);
        if motor.is_some() {
            self.last_motor_rpm = motor;
        }
        let brake = self.input(self.in_brake);
        let p_front = self.input(self.in_pfront);
        // brake light fallback (ch. 01 §1.7)
        if let Some(b) = brake {
            self.brake_ever = true;
            self.brake_last = b;
            self.brake_loss_since = None;
        } else if self.brake_ever && self.brake_loss_since.is_none() {
            self.brake_loss_since = Some(t);
        }
        let b_vcu = if !self.brake_ever {
            0.0
        } else if let Some(b) = brake {
            b
        } else if t - self.brake_loss_since.unwrap_or(t) < 100_000 {
            self.brake_last
        } else {
            1.0
        };
        // speed estimate
        let n_ref = match (wheels, motor) {
            (Some(n), _) => Some((n[0] + n[1]) / 2.0),
            (None, Some(m)) => Some(m / v.gear_ratio),
            _ => None,
        };
        let v_hat = n_ref.map_or(0.0, |n| v.r_eff_m * PI / 30.0 * n);
        self.veh_speed = n_ref.map(|_| 3.6 * v_hat);
        // torque limit
        let lim_delta = match (angle, wheels) {
            (Some(a), Some(_)) => v.c_max_nm - (v.c_max_nm - v.c_cons_nm) * (a.abs() / v.delta_max_deg).min(1.0),
            _ => v.c_cons_nm,
        };
        let omega_hat = self.last_motor_rpm.unwrap_or(0.0) * PI / 30.0;
        let lim_p = if omega_hat >= 1.0 { v.eta * v.p_max_w / omega_hat } else { v.c_max_nm };
        self.tc_active = match wheels {
            Some(n) if w.r2d => {
                let front = n[0] + n[1];
                let s_hat =
                    if front / 2.0 >= v.n_tc_rpm { (self.n_rear[0] + self.n_rear[1]) / front - 1.0 } else { 0.0 };
                if self.tc_active {
                    s_hat >= v.s_off
                } else {
                    s_hat > v.s_on
                }
            }
            _ => false,
        };
        let k = if self.tc_active { v.k_tc } else { 1.0 };
        self.trq_lim = k * lim_delta.min(lim_p);
        let (latched, retained) = apps.unwrap_or((false, pack.consumer.apps.substitute_pct));
        let f_map = v.f_map_base + v.f_map_step * w.trq_map as f64;
        let (mode, req) = if !w.r2d {
            (c.trq_disabled, 0.0)
        } else if latched {
            (c.trq_safe, 0.0)
        } else if v_hat > v.v_min_mps && brake == Some(1.0) && p_front.is_some() {
            (c.trq_regen, -v.c_regen_nm * (p_front.unwrap_or(0.0) / v.p_ref_bar).min(1.0))
        } else {
            (c.trq_drive, (f_map * v.c_max_nm * retained / 100.0).min(self.trq_lim))
        };
        self.trq_mode = mode;
        self.trq_req = req;
        self.inv_en = w.r2d && (mode == c.trq_drive || mode == c.trq_regen);
        self.brake_light = b_vcu == 1.0 || mode == c.trq_regen;
    }
}
