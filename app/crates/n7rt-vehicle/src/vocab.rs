//! Names of the engine quantities a pack signal can carry (`source` key). The pack maps them to frames and
//! layouts; the engine computes them. Quantities of the static background ECUs are defined by the pack itself
//! (`background.static_ecus[].phases`), plus `<prefix>.ecu_temp` computed by the thermal law.

pub(crate) const HDR_CRC: &str = "e2e.crc";
pub(crate) const HDR_COUNTER: &str = "e2e.counter";
pub(crate) const HDR_ALIVE: &str = "alive";

/// Emitter names of the background ECUs whose quantities the engine computes.
pub(crate) const VCU: &str = "VCU";
pub(crate) const INV: &str = "INV";
pub(crate) const BMS: &str = "BMS";
pub(crate) const DASH: &str = "DASH";
pub(crate) const LOG: &str = "LOG";

const DYNAMIC_PREFIXES: [&str; 6] = ["apps", "vcu", "inv", "bms", "dash", "log"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Dynamic {
    Mission(&'static str),
    // source ECU (mission ECU, model driven by the Physical Lab)
    AppsStatus(usize),
    AppsTravel(usize),
    AppsMode,
    AppsLvUv,
    AppsSupFlt,
    AppsVlv,
    AppsTemp,
    AppsRatio(usize),
    AppsSupply(usize),
    // VCU
    VcuTrqMode,
    VcuInvEn,
    VcuTrqReq,
    VcuTrqLim,
    VcuWsStill,
    VcuWs(usize),
    VcuWsSt(usize),
    VcuImuSt,
    VcuYawRate,
    VcuAccLat,
    VcuAccLong,
    VcuVehSt,
    VcuBrakeLight,
    VcuR2dSound,
    VcuSdcClosed,
    VcuTcActive,
    VcuVehSpeed,
    // inverter
    InvSt,
    InvMotSpeed,
    InvTrqAct,
    InvIdc,
    InvTMotor,
    InvTIgbt,
    InvUdc,
    // BMS
    BmsSt,
    BmsUpack,
    BmsIpack,
    BmsSoc,
    BmsTmax,
    BmsVcellMin,
    BmsVcellMax,
    BmsTcellMin,
    BmsTcellMax,
    BmsIdxVmin,
    BmsIdxVmax,
    // dashboard
    DashTsBtn,
    DashR2dBtn,
    DashDriveMode,
    DashTrqMap,
    // logger
    LogSt,
    LogMemFree,
    LogUptime,
}

/// Every dynamic quantity name, in a fixed order (documentation and tests).
pub(crate) const BASE: [&str; 57] = [
    "apps.st1",
    "apps.st2",
    "apps.travel1",
    "apps.travel2",
    "apps.ecu_mode",
    "apps.lv_uv",
    "apps.sens_sup_flt",
    "apps.v_lv",
    "apps.ecu_temp",
    "apps.ratio1",
    "apps.ratio2",
    "apps.sup1",
    "apps.sup2",
    "vcu.trq_mode",
    "vcu.inv_en",
    "vcu.trq_req",
    "vcu.trq_lim",
    "vcu.ws_still",
    "vcu.ws_l",
    "vcu.ws_r",
    "vcu.ws_l_st",
    "vcu.ws_r_st",
    "vcu.imu_st",
    "vcu.yaw_rate",
    "vcu.acc_lat",
    "vcu.acc_long",
    "vcu.veh_st",
    "vcu.brake_light",
    "vcu.r2d_sound",
    "vcu.sdc_closed",
    "vcu.tc_active",
    "vcu.veh_speed",
    "inv.st",
    "inv.mot_speed",
    "inv.trq_act",
    "inv.i_dc",
    "inv.t_motor",
    "inv.t_igbt",
    "inv.u_dc",
    "bms.st",
    "bms.u_pack",
    "bms.i_pack",
    "bms.soc",
    "bms.t_max",
    "bms.v_cell_min",
    "bms.v_cell_max",
    "bms.t_cell_min",
    "bms.t_cell_max",
    "bms.idx_v_min",
    "bms.idx_v_max",
    "dash.ts_btn",
    "dash.r2d_btn",
    "dash.drive_mode",
    "dash.trq_map",
    "log.st",
    "log.mem_free",
    "log.uptime",
];

impl Dynamic {
    pub(crate) fn parse(name: &str) -> Option<Dynamic> {
        use Dynamic::*;
        if let Some(source) = crate::ecu_wsf::SOURCES.iter().chain(crate::ecu_sas::SOURCES.iter())
            .chain(crate::ecu_bps::SOURCES.iter()).chain(["wsf.ecu_temp", "sas.ecu_temp"].iter())
            .find(|s| **s == name) { return Some(Mission(source)); }
        Some(match name {
            "apps.st1" => AppsStatus(0),
            "apps.st2" => AppsStatus(1),
            "apps.travel1" => AppsTravel(0),
            "apps.travel2" => AppsTravel(1),
            "apps.ecu_mode" => AppsMode,
            "apps.lv_uv" => AppsLvUv,
            "apps.sens_sup_flt" => AppsSupFlt,
            "apps.v_lv" => AppsVlv,
            "apps.ecu_temp" => AppsTemp,
            "apps.ratio1" => AppsRatio(0),
            "apps.ratio2" => AppsRatio(1),
            "apps.sup1" => AppsSupply(0),
            "apps.sup2" => AppsSupply(1),
            "vcu.trq_mode" => VcuTrqMode,
            "vcu.inv_en" => VcuInvEn,
            "vcu.trq_req" => VcuTrqReq,
            "vcu.trq_lim" => VcuTrqLim,
            "vcu.ws_still" => VcuWsStill,
            "vcu.ws_l" => VcuWs(0),
            "vcu.ws_r" => VcuWs(1),
            "vcu.ws_l_st" => VcuWsSt(0),
            "vcu.ws_r_st" => VcuWsSt(1),
            "vcu.imu_st" => VcuImuSt,
            "vcu.yaw_rate" => VcuYawRate,
            "vcu.acc_lat" => VcuAccLat,
            "vcu.acc_long" => VcuAccLong,
            "vcu.veh_st" => VcuVehSt,
            "vcu.brake_light" => VcuBrakeLight,
            "vcu.r2d_sound" => VcuR2dSound,
            "vcu.sdc_closed" => VcuSdcClosed,
            "vcu.tc_active" => VcuTcActive,
            "vcu.veh_speed" => VcuVehSpeed,
            "inv.st" => InvSt,
            "inv.mot_speed" => InvMotSpeed,
            "inv.trq_act" => InvTrqAct,
            "inv.i_dc" => InvIdc,
            "inv.t_motor" => InvTMotor,
            "inv.t_igbt" => InvTIgbt,
            "inv.u_dc" => InvUdc,
            "bms.st" => BmsSt,
            "bms.u_pack" => BmsUpack,
            "bms.i_pack" => BmsIpack,
            "bms.soc" => BmsSoc,
            "bms.t_max" => BmsTmax,
            "bms.v_cell_min" => BmsVcellMin,
            "bms.v_cell_max" => BmsVcellMax,
            "bms.t_cell_min" => BmsTcellMin,
            "bms.t_cell_max" => BmsTcellMax,
            "bms.idx_v_min" => BmsIdxVmin,
            "bms.idx_v_max" => BmsIdxVmax,
            "dash.ts_btn" => DashTsBtn,
            "dash.r2d_btn" => DashR2dBtn,
            "dash.drive_mode" => DashDriveMode,
            "dash.trq_map" => DashTrqMap,
            "log.st" => LogSt,
            "log.mem_free" => LogMemFree,
            "log.uptime" => LogUptime,
            _ => return None,
        })
    }

    /// Emitter that owns the quantity (`source_emitter` for the mission ECU).
    pub(crate) fn owner<'a>(&self, source_emitter: &'a str) -> &'a str {
        use Dynamic::*;
        match self {
            Mission(_) => source_emitter,
            AppsStatus(_) | AppsTravel(_) | AppsMode | AppsLvUv | AppsSupFlt | AppsVlv | AppsTemp | AppsRatio(_)
            | AppsSupply(_) => source_emitter,
            VcuTrqMode | VcuInvEn | VcuTrqReq | VcuTrqLim | VcuWsStill | VcuWs(_) | VcuWsSt(_) | VcuImuSt
            | VcuYawRate | VcuAccLat | VcuAccLong | VcuVehSt | VcuBrakeLight | VcuR2dSound | VcuSdcClosed
            | VcuTcActive | VcuVehSpeed => VCU,
            InvSt | InvMotSpeed | InvTrqAct | InvIdc | InvTMotor | InvTIgbt | InvUdc => INV,
            BmsSt | BmsUpack | BmsIpack | BmsSoc | BmsTmax | BmsVcellMin | BmsVcellMax | BmsTcellMin
            | BmsTcellMax | BmsIdxVmin | BmsIdxVmax => BMS,
            DashTsBtn | DashR2dBtn | DashDriveMode | DashTrqMap => DASH,
            LogSt | LogMemFree | LogUptime => LOG,
        }
    }
}

impl Dynamic {
    pub(crate) fn for_mission(name: &str, mission: &str) -> Option<Self> {
        let mission = if ["wsf", "sas", "bps"].contains(&mission) { mission } else { "apps" };
        let d = Self::parse(name)?;
        let prefix = name.split('.').next()?;
        if ["apps", "wsf", "sas", "bps"].contains(&prefix) && prefix != mission { None } else { Some(d) }
    }
}

pub(crate) fn all() -> &'static [&'static str] {
    static ALL: std::sync::LazyLock<Vec<&'static str>> = std::sync::LazyLock::new(||
        BASE.iter().chain(crate::ecu_wsf::SOURCES.iter()).chain(crate::ecu_sas::SOURCES.iter())
            .chain(crate::ecu_bps::SOURCES.iter()).chain(["wsf.ecu_temp", "sas.ecu_temp"].iter()).copied().collect());
    &ALL
}

pub(crate) fn is_dynamic_prefix(prefix: &str) -> bool {
    DYNAMIC_PREFIXES.contains(&prefix)
}

pub(crate) fn valid_prefix(prefix: &str) -> bool {
    !prefix.is_empty() && prefix.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_listed_name_parses() {
        for n in all() {
            assert!(Dynamic::parse(n).is_some(), "{n}");
        }
        assert!(Dynamic::parse("apps.unknown").is_none());
    }
}
