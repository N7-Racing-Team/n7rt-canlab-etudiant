//! Dispatch of source ECUs. Every operating parameter is supplied by the loaded pack.
use crate::{EcuInputs, WsfInputs, SasInput};
use crate::pack::PackData;
use crate::ecu_apps::EcuApps;
use crate::ecu_wsf::EcuWsf;
use crate::ecu_sas::EcuSas;
use crate::ecu_bps::EcuBps;

pub(crate) enum SourceModel { Apps(EcuApps), Wsf(EcuWsf), Sas(EcuSas), Bps(EcuBps) }
impl SourceModel {
    pub fn new(p: &PackData) -> Self {
        let s = &p.source_ecu;
        let pow = &p.power;
        match p.mission.as_str() {
            "wsf" => Self::Wsf(EcuWsf::new(s.wsf.as_ref().expect("validated WSF"), pow.reset_below_v, pow.reset_exit_v, pow.t_boot_ms)),
            "sas" => Self::Sas(EcuSas::new(s.sas.as_ref().expect("validated SAS"), pow.reset_below_v, pow.reset_exit_v, pow.t_boot_ms)),
            "bps" => Self::Bps(EcuBps::new(s, pow.reset_below_v, pow.reset_exit_v, pow.t_boot_ms)),
            _ => Self::Apps(EcuApps::new(s, pow.reset_below_v, pow.reset_exit_v, pow.t_boot_ms)),
        }
    }
    /// Static configuration of `p` (same mission as at construction), dynamic state kept.
    pub fn reconfigure(&mut self, p: &PackData) {
        let s = &p.source_ecu;
        match self {
            Self::Apps(e) => e.reconfigure(s),
            Self::Wsf(e) => {
                if let Some(w) = &s.wsf {
                    e.reconfigure(w)
                }
            }
            Self::Sas(e) => {
                if let Some(c) = &s.sas {
                    e.reconfigure(c)
                }
            }
            Self::Bps(e) => e.reconfigure(s),
        }
    }
    pub fn sample(&mut self, ms: u64, input: &EcuInputs) {
        match self {
            Self::Apps(e) => e.sample(ms, input),
            Self::Bps(e) => e.sample(ms, input),
            Self::Wsf(e) => e.sample(ms, input.wsf.as_ref().unwrap_or(&WsfInputs { lv: input.lv, ..Default::default() })),
            Self::Sas(e) => e.sample(ms, input.sas.as_ref().unwrap_or(&SasInput { lv: input.lv, supply_adc: input.adc[0], ..Default::default() })),
        }
    }
    pub fn can_emit(&self, ms: u64, us: u64) -> bool {
        match self { Self::Apps(e) => e.can_emit(ms,us), Self::Wsf(e) => e.can_emit(ms,us),
            Self::Sas(e) => e.can_emit(ms,us), Self::Bps(e) => e.can_emit(ms,us) }
    }
    pub fn run_id(&self) -> u64 {
        match self { Self::Apps(e) => e.run_id, Self::Wsf(e) => e.run_id,
            Self::Sas(e) => e.run_id, Self::Bps(e) => e.run_id }
    }
    pub fn prepare_emission(&mut self, nominal_us: u64) {
        if let Self::Wsf(e) = self { e.prepare_emission(nominal_us); }
    }
    pub fn value(&mut self, source: &str) -> Option<f64> {
        match self {
            Self::Wsf(e) => e.value(source), Self::Sas(e) => e.value(source), Self::Bps(e) => e.value(source),
            Self::Apps(e) => match source {
                "apps.st1" => Some(e.status(0)), "apps.st2" => Some(e.status(1)),
                "apps.travel1" => e.travel_for_emission(0), "apps.travel2" => e.travel_for_emission(1),
                "apps.ecu_mode" => Some(e.mode()), "apps.lv_uv" => Some(e.lv_uv()),
                "apps.sens_sup_flt" => Some(e.sup_flag()), "apps.v_lv" => Some(e.v_lv()),
                "apps.ratio1" => e.ratio(0), "apps.ratio2" => e.ratio(1),
                "apps.sup1" => Some(e.supply(0)), "apps.sup2" => Some(e.supply(1)), _ => None,
            }
        }
    }
}
