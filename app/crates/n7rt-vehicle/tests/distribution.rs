//! Distribution tests (docs/distribution.md §3): group variants, student packs and run files. Their judgement and
//! the reference consumer are tested with the private oracle (tests/oracle.rs).
//! Generic tests use a synthetic pack built here; the tests that need the private mission pack read
//! `restricted/missions/apps/pack-v0.json` at run time (found through `N7RT_MISSIONS_DIR` or by walking up from
//! this crate) and pass with a printed note when it is absent. No mission value is written here, and failure
//! messages print neither frame names nor expected values.
#![recursion_limit = "512"]

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use n7rt_vehicle::{
    compare, judge_run, BusFrame, NominalScenario, Pack, RefConsumer, RunFrame, RunRecord, RunSlot, RunTick, Vehicle,
};
use serde_json::{json, Value};

// ------------------------------------------------------------------------------------------------- packs

fn synthetic() -> Value {
    let ids: Vec<String> =
        [0x9C, 0x3B, 0xE1, 0x47, 0x08, 0xD5, 0x62, 0xAF, 0x1E, 0x73, 0xC4, 0x5D, 0x90, 0x2A, 0xB7, 0x66]
            .iter()
            .map(|b: &u8| format!("0x{b:02X}"))
            .collect();
    json!({
        "format": "n7rt-mission-pack", "format_version": 1, "mission": "demo", "variant": "T1",
        "provenance": { "note": "synthetic" },
        "bench_file": "bench.json", "bench_ecu": "X1", "bench_sensor": "X2",
        "adc_channels": ["a", "b", "c", "d"], "tick_period_ms": 10,
        "bus": { "jitter_ms": 0.25, "hyperperiod_ms": 100, "min_spacing_ms": 1.0, "min_margin_ms": 2.0 },
        "power": { "lv_nominal_v": 13.5, "reset_below_v": 7.0, "reset_exit_v": 7.5, "t_boot_ms": 40 },
        "enums": {
            "s": { "OK": 1, "INIT": 0, "SRC_LO": 3, "SRC_HI": 2 },
            "m": { "INIT": 3, "RUN": 2, "DEGRADED": 1, "FAULT": 0 },
            "trq_mode": { "DISABLED": 3, "DRIVE": 2, "REGEN": 1, "SAFE_TORQUE": 0 },
            "inv_st": { "STANDBY": 3, "READY": 2, "RUN": 1, "FAULT": 0 },
            "veh_st": { "LV_ON": 4, "TS_PRECHARGE": 3, "TS_ACTIVE": 1, "R2D": 2, "FAULT": 0 },
            "bms_st": { "INIT": 4, "IDLE": 3, "PRECHARGE": 2, "ACTIVE": 1, "FAULT": 0 },
            "imu_st": { "OK": 2, "INIT": 1, "FAULT": 0 },
            "log_st": { "IDLE": 3, "RECORDING": 2, "FULL": 1, "ERROR": 0 },
            "st_ws": { "OK": 5, "INIT": 4 }
        },
        "frames": [
            { "id": "0x123", "name": "SYN_A", "emitter": "SRC", "period_ms": 10, "phase_ms": 2.0, "dlc": 8,
              "protection": { "kind": "p22", "data_ids": ids },
              "signals": [
                { "name": "C", "start": 0, "len": 8, "source": "e2e.crc" },
                { "name": "N", "start": 8, "len": 4, "source": "e2e.counter" },
                { "name": "S1", "start": 12, "len": 2, "enum": "s", "min": 0, "max": 3, "source": "apps.st1" },
                { "name": "S2", "start": 14, "len": 2, "enum": "s", "min": 0, "max": 3, "source": "apps.st2" },
                { "name": "T1", "start": 23, "len": 12, "order": "motorola", "factor": 0.05, "offset": -5.0, "min": 0,
                  "max": 2200, "sna": "0xFFF", "unit": "%", "qualifier": { "source": "apps.st1", "ok": 1 },
                  "source": "apps.travel1" },
                { "name": "T2", "start": 32, "len": 16, "signed": true, "factor": 0.02, "offset": 0, "min": -1000,
                  "max": 6000, "sna": "0x7FFF", "unit": "%", "qualifier": { "source": "apps.st2", "ok": 1 },
                  "source": "apps.travel2" }
              ] },
            { "id": "0x2C4", "name": "SYN_B", "emitter": "SRC", "period_ms": 50, "phase_ms": 9.0, "dlc": 4,
              "protection": { "kind": "alive", "bits": 8 },
              "signals": [
                { "name": "A", "start": 0, "len": 8, "source": "alive" },
                { "name": "U1", "start": 8, "len": 8, "factor": 0.03, "min": 0, "max": 250, "sna": "0xFF", "unit": "V",
                  "source": "apps.sup1" },
                { "name": "U2", "start": 23, "len": 8, "order": "motorola", "factor": 0.03, "min": 0, "max": 250,
                  "sna": "0xFF", "unit": "V", "source": "apps.sup2" },
                { "name": "MD", "start": 24, "len": 2, "enum": "m", "min": 0, "max": 3, "source": "apps.ecu_mode" }
              ] },
            { "id": "0x355", "name": "SYN_C", "emitter": "VCU", "period_ms": 100, "phase_ms": 31.0, "dlc": 2,
              "protection": { "kind": "none" },
              "signals": [ { "name": "V", "start": 0, "len": 4, "enum": "veh_st", "min": 0, "max": 4, "source": "vcu.veh_st" } ] }
        ],
        "source_ecu": {
            "emitter": "SRC", "adc_bits": 10, "adc_vref_v": 2.5, "sig_divider": 3.0, "sup_divider": 2.5, "filter_samples": 3,
            "ratio_undefined_below_v": 0.4, "supply_window_v": [4.3, 5.3],
            "channels": [ { "window_pct": [10.0, 85.0], "a_pct": 20.0, "b_pct": 50.0 },
                          { "window_pct": [4.0, 60.0], "a_pct": 10.0, "b_pct": 40.0 } ],
            "travel_range_pct": [0.0, 100.0], "debounce_fault_samples": 5, "debounce_recover_samples": 20,
            "init_after_rails_ms": 15,
            "sens_sup": { "nominal_v": 4.8, "tolerance_v": 0.2, "rise_samples": 5, "fall_samples": 30, "fault_persist_ms": 40 },
            "lv_uv_on_v": 10.0, "lv_uv_off_v": 10.5, "lv_avg_samples": 2, "status_enum": "s", "mode_enum": "m",
            "self_heating_k": 7.0
        },
        "background": {
            "world": { "veh_st": "R2D", "sdc_closed": 1, "r2d_sound": 0, "v_mps": 0.0, "delta_sw_deg": 0.0, "slip": 0.0,
                       "ts_btn": 0, "r2d_btn": 0, "drive_mode": 1, "trq_map": 4, "soc0": 0.5, "t_amb_c": 18.0 },
            "vehicle": {
                "r_eff_m": 0.3, "wheelbase_m": 2.0, "track_rear_m": 1.0, "steer_ratio": 4.0, "gear_ratio": 3.0,
                "c_max_nm": 150.0, "c_cons_nm": 80.0, "delta_max_deg": 90.0, "p_max_w": 50000.0, "f_map_base": 0.6,
                "f_map_step": 0.04, "c_regen_nm": 40.0, "p_ref_bar": 15.0, "v_min_mps": 1.5, "eta": 0.85, "n_tc_rpm": 40.0,
                "s_on": 0.2, "s_off": 0.1, "k_tc": 0.6, "n_s": 100, "r_i_ohm": 0.2, "q_nom_ah": 10.0, "ocv_base_v": 3.3,
                "ocv_span_v": 0.9, "dv_cell_v": 0.01, "dt_cell_k": 2.0, "tau_th_s": 30.0, "temp_step_c": 0.2,
                "t_motor_k_per_kw": 0.3, "t_igbt_k_per_kw": 0.2, "t_max_k_per_kw": 1.0, "tau_trq_ms": 15.0, "init_ms": 150,
                "inv_ready_v": 50.0, "ws_stop_ms": 150, "mem_free_pct": [40.0, 90.0], "mem_free_step_pct": 1.0,
                "mem_free_period_s": 30.0
            },
            "vcu_inputs": { "steering_angle": null, "wheels_front": null, "motor_speed": null, "brake_active": null,
                            "brake_pressure_front": null },
            "static_ecus": []
        },
        "consumer": {
            "p22": { "first_timeout_periods": 12, "timeout_periods": 6, "stale_periods": 4 },
            "alive": { "first_timeout_periods": 12, "timeout_periods": 6, "stale_periods": 4 },
            "unprotected": { "first_timeout_periods": 12, "timeout_periods": 6, "stale_periods": null },
            "max_delta_counter": 3, "life_counter_max_delta": 3,
            "state_machine": { "window_size_valid": 4, "window_size_init": 2, "window_size_invalid": 4,
                               "max_error_state_valid": 1, "max_error_state_init": 0, "max_error_state_invalid": 0,
                               "min_ok_state_valid": 2, "min_ok_state_init": 2, "min_ok_state_invalid": 3,
                               "clear_to_invalid": true },
            "apps": { "travel_sources": ["apps.travel1", "apps.travel2"], "supply_sources": ["apps.sup1", "apps.sup2"],
                      "max_diff_pct": 8.0, "persistence_ms": 40, "recovery_ticks": 4, "recovery_max_pct": 50.0,
                      "substitute_pct": 0.0 }
        },
        "slots": [
            { "slot": 0, "output": "travel1", "kind": "value", "lsb": 0.05 },
            { "slot": 1, "output": "travel2", "kind": "value", "lsb": 0.02 },
            { "slot": 2, "output": "diff", "kind": "value", "lsb": 0.05 },
            { "slot": 3, "output": "implausibility", "kind": "logic", "lsb": 1 },
            { "slot": 4, "output": "cut", "kind": "logic", "lsb": 1 },
            { "slot": 5, "output": "retained", "kind": "value", "lsb": 0.05 },
            { "slot": 6, "output": "supply1", "kind": "value", "lsb": 0.03 },
            { "slot": 7, "output": "supply2", "kind": "value", "lsb": 0.03 }
        ],
        "nominal": { "cold_start_ms": 500, "nominal_ms": 3000, "hold_ms": 200, "ramp_ms": [300, 600],
                     "segment_ms": [100, 300], "position_quantum": 0.001 },
        "compare": { "transition_tolerance_ms": 10, "value_tolerance_lsb": 0.5, "max_listed_mismatches": 5 }
    })
}

fn synthetic_pack() -> Pack {
    let mut v = synthetic();
    v["bench"] = json!("{\"synthetic\":true}");
    Pack::from_json(&v.to_string()).expect("synthetic pack")
}

fn mission_dir() -> Option<PathBuf> {
    let p = private_apps_dir();
    p.join("pack-v0.json").is_file().then_some(p)
}

/// The private pack, or None with a printed note.
fn private_pack() -> Option<Pack> {
    match mission_dir() {
        Some(d) => Some(Pack::load(&d.join("pack-v0.json")).expect("pack")),
        None => {
            eprintln!("note : pack de mission privé absent (restricted/missions/apps) — partie du test ignorée");
            None
        }
    }
}

/// Both packs the tests run on: the synthetic one, then the private one when present.
fn packs() -> Vec<(&'static str, Pack)> {
    let mut v = vec![("synthétique", synthetic_pack())];
    if let Some(p) = private_pack() {
        v.push(("privé", p));
    }
    v
}

fn seed(i: u64) -> [u8; 32] {
    let mut s = [0u8; 32];
    for (k, chunk) in s.chunks_mut(8).enumerate() {
        let x = (i ^ 0xA5A5_5A5A_0F0F_F0F0).wrapping_mul(0x9E37_79B9_7F4A_7C15).rotate_left(13 * k as u32 + 7);
        chunk.copy_from_slice(&x.to_le_bytes());
    }
    s
}

fn doc(p: &Pack) -> Value {
    serde_json::from_str(&p.to_json()).expect("pack JSON")
}

// ------------------------------------------------------------------------------------------ JSON helpers

fn num(v: &Value) -> i64 {
    match v {
        Value::Number(n) => n.as_i64().expect("integer"),
        Value::String(s) => match s.strip_prefix("0x") {
            Some(h) => i64::from_str_radix(h, 16).expect("hex"),
            None => s.parse().expect("integer"),
        },
        _ => panic!("integer expected"),
    }
}

fn us(v: &Value) -> u64 {
    (v.as_f64().expect("number") * 1000.0).round() as u64
}

/// Bit positions, least significant bit first (ch. 01 §0.4 convention, written independently of the crate).
fn positions(start: u64, len: u64, motorola: bool) -> Vec<u64> {
    if !motorola {
        return (start..start + len).collect();
    }
    let mut msb_first = Vec::new();
    let mut p = start as i64;
    for i in 0..len {
        msb_first.push(p as u64);
        if i + 1 < len {
            p = if p % 8 != 0 { p - 1 } else { p + 15 };
        }
    }
    msb_first.reverse();
    msb_first
}

const HEADER: [&str; 3] = ["e2e.crc", "e2e.counter", "alive"];

fn sig_positions(s: &Value) -> Vec<u64> {
    positions(s["start"].as_u64().unwrap(), s["len"].as_u64().unwrap(), s["order"].as_str() == Some("motorola"))
}

fn factor(s: &Value) -> f64 {
    s.get("factor").and_then(Value::as_f64).unwrap_or(1.0)
}

fn offset(s: &Value) -> f64 {
    s.get("offset").and_then(Value::as_f64).unwrap_or(0.0)
}

fn enum_codes(d: &Value) -> BTreeMap<String, BTreeSet<i64>> {
    d["enums"].as_object().unwrap().iter().map(|(k, m)| (k.clone(), m.as_object().unwrap().values().map(num).collect())).collect()
}

/// Every bound of a group variant (ch. 01 §2.2, §2.3.3, §2.5, §2.8), checked on the JSON of the variant.
fn check_variant(base: &Value, var: &Value) {
    let bus = &base["bus"];
    let (hyper, spacing, margin) = (us(&bus["hyperperiod_ms"]), us(&bus["min_spacing_ms"]), us(&bus["min_margin_ms"]));
    let (bf, vf) = (base["frames"].as_array().unwrap(), var["frames"].as_array().unwrap());
    assert_eq!(bf.len(), vf.len());
    let mut times = Vec::new();
    let mut data_ids = Vec::new();
    for (fi, (b, f)) in bf.iter().zip(vf).enumerate() {
        for k in ["id", "period_ms", "dlc", "emitter"] {
            assert_eq!(b[k], f[k], "frames[{fi}].{k} modifié");
        }
        assert_eq!(b["protection"]["kind"], f["protection"]["kind"], "frames[{fi}] : protection modifiée");
        let (period, phase) = (us(&f["period_ms"]), us(&f["phase_ms"]));
        assert!(phase < period && period - phase >= margin, "frames[{fi}] : marge insuffisante");
        assert!(phase >= us(&bus["jitter_ms"]) + 500, "frames[{fi}] : arrivée possible dans la fenêtre précédente");
        times.extend((0..hyper / period).map(|k| phase + k * period));
        let nbits = f["dlc"].as_u64().unwrap() * 8;
        let p22 = f["protection"]["kind"] == "p22";
        if p22 {
            data_ids.extend(f["protection"]["data_ids"].as_array().unwrap().iter().map(num));
        }
        let mut used = BTreeSet::new();
        let (bs, vs) = (b["signals"].as_array().unwrap(), f["signals"].as_array().unwrap());
        assert_eq!(bs.len(), vs.len());
        for (si, (bsig, s)) in bs.iter().zip(vs).enumerate() {
            let at = format!("frames[{fi}].signals[{si}]");
            assert_eq!(bsig["source"], s["source"], "{at}");
            assert_eq!(bsig["len"], s["len"], "{at} : longueur modifiée");
            let pos = sig_positions(s);
            assert!(pos.iter().all(|&p| p < nbits), "{at} : hors de la trame");
            for &p in &pos {
                assert!(used.insert(p), "{at} : chevauchement");
            }
            let header = HEADER.contains(&s["source"].as_str().unwrap());
            if header {
                assert_eq!(pos, sig_positions(bsig), "{at} : en-tête déplacé");
                assert_ne!(s["order"], "motorola", "{at}");
                continue;
            }
            if p22 {
                assert!(pos.iter().all(|&p| p >= 12), "{at} : signal dans l'en-tête P22");
            }
            let len = pos.len();
            if len >= 8 {
                assert!(pos[0] % 8 == 0 || pos[len - 1] % 8 == 7, "{at} : règle de mise en page");
            } else {
                assert!(pos.iter().all(|p| p / 8 == pos[0] / 8), "{at} : signal court sur deux octets");
                assert_ne!(s["order"], "motorola", "{at}");
            }
            if s.get("min").is_none() {
                continue;
            }
            let (min, max) = (num(&s["min"]), num(&s["max"]));
            let signed = s["signed"].as_bool().unwrap_or(false);
            let mag = min.unsigned_abs().max(max.unsigned_abs());
            assert!(64 - mag.leading_zeros() + signed as u32 <= 16, "{at} : plus de 16 bits utiles");
            if signed && (factor(s) != factor(bsig) || offset(s) != offset(bsig)) {
                assert!(min <= 0 && max >= 0, "{at} : plage brute signée sans zéro");
            }
            assert!(factor(s) <= factor(bsig) * (1.0 + 1e-9), "{at} : résolution dégradée");
            let tol = 1e-9 * 1e4;
            let (bmin, bmax) = (num(&bsig["min"]) as f64 * factor(bsig) + offset(bsig), num(&bsig["max"]) as f64 * factor(bsig) + offset(bsig));
            assert!((min as f64 * factor(s) + offset(s) - bmin).abs() < tol, "{at} : plage physique modifiée");
            assert!((max as f64 * factor(s) + offset(s) - bmax).abs() < tol, "{at} : plage physique modifiée");
            assert_eq!(bsig.get("sna"), s.get("sna"), "{at} : SNA modifié");
            if let Some(sna) = s.get("sna").map(num) {
                assert!(!(min..=max).contains(&sna), "{at} : SNA dans la plage émise");
            }
            assert_eq!(bsig.get("enum"), s.get("enum"), "{at}");
        }
    }
    times.sort_unstable();
    for w in 0..times.len() {
        let next = if w + 1 < times.len() { times[w + 1] } else { times[0] + hyper };
        assert!(next - times[w] >= spacing, "deux émissions planifiées trop proches");
    }
    let distinct: BTreeSet<i64> = data_ids.iter().copied().collect();
    assert_eq!(distinct.len(), data_ids.len(), "octets de Data ID en double");
    assert!(data_ids.iter().all(|b| (0..=255).contains(b)));
    let n_p22 = vf.iter().filter(|f| f["protection"]["kind"] == "p22").count();
    assert_eq!(data_ids.len(), 16 * n_p22);
    assert_eq!(enum_codes(base), enum_codes(var), "ensembles de codes modifiés");
}

// ------------------------------------------------------------------------------------------------- variants

#[test]
fn variant_is_deterministic_and_round_trips() {
    for (which, p) in packs() {
        let a = p.variant(&seed(1)).expect("variant");
        let b = p.variant(&seed(1)).expect("variant");
        let c = p.variant(&seed(2)).expect("variant");
        assert_eq!(a.to_json(), b.to_json(), "{which} : même seed, variantes différentes");
        assert_ne!(a.to_json(), c.to_json(), "{which} : seeds différents, même variante");
        assert_ne!(a.to_json(), p.to_json(), "{which} : variante identique à sa base");
        // the full JSON reads back to the same pack, bench included
        let back = Pack::from_json(&a.to_json()).expect("full JSON");
        assert_eq!(back.to_json(), a.to_json(), "{which}");
        assert_eq!(back.bench_json(), p.bench_json(), "{which}");
        assert!(!a.is_student());
        // a variant of a variant is still bounded by its own base
        let aa = a.variant(&seed(3)).expect("variant of a variant");
        check_variant(&doc(&a), &doc(&aa));
    }
}

#[test]
fn variant_bounds_on_200_seeds() {
    for (which, p) in packs() {
        let base = doc(&p);
        let mut phases = BTreeSet::new();
        let mut layouts = BTreeSet::new();
        let mut motorola = 0usize;
        let mut rescaled = 0usize;
        for i in 0..200 {
            let v = p.variant(&seed(1000 + i)).unwrap_or_else(|e| panic!("{which} : variante {i} : {e}"));
            let d = doc(&v);
            check_variant(&base, &d);
            let frames = d["frames"].as_array().unwrap();
            phases.insert(frames.iter().map(|f| us(&f["phase_ms"])).collect::<Vec<_>>());
            layouts.insert(frames.iter().map(|f| f["signals"].to_string()).collect::<Vec<_>>());
            for (f, bfr) in frames.iter().zip(base["frames"].as_array().unwrap()) {
                for (s, b) in f["signals"].as_array().unwrap().iter().zip(bfr["signals"].as_array().unwrap()) {
                    motorola += (s["order"] == "motorola" && b["order"] != "motorola") as usize;
                    rescaled += (factor(s) != factor(b) || offset(s) != offset(b)) as usize;
                }
            }
        }
        // the draws really vary
        assert!(phases.len() > 150, "{which} : phases trop peu variées");
        assert!(layouts.len() > 150, "{which} : dispositions trop peu variées");
        assert!(motorola > 0 && rescaled > 0, "{which} : ni Motorola ni nouvelle échelle");
    }
}

// ------------------------------------------------------------------------------------------- student pack

fn keys_anywhere(v: &Value, out: &mut BTreeSet<String>) {
    match v {
        Value::Object(m) => {
            for (k, x) in m {
                out.insert(k.clone());
                keys_anywhere(x, out);
            }
        }
        Value::Array(a) => a.iter().for_each(|x| keys_anywhere(x, out)),
        _ => {}
    }
}

#[test]
fn student_pack_has_no_oracle_and_simulates_the_mission_identically() {
    for (which, p) in packs() {
        let v = p.variant(&seed(21)).expect("variant");
        let text = v.student_json();
        let sj: Value = serde_json::from_str(&text).unwrap();
        let mut keys = BTreeSet::new();
        keys_anywhere(&sj, &mut keys);
        for k in ["slots", "compare", "consumer", "lsb", "provenance", "name", "unit", "variant", "bench_file", "golden"] {
            assert!(!keys.contains(k), "{which} : clé `{k}` dans le pack étudiant");
        }
        assert_eq!(sj["format"], "n7rt-student-pack");

        let s = Pack::from_student_json(&text).unwrap_or_else(|e| panic!("{which} : {e}"));
        assert!(s.is_student() && !v.is_student());
        assert_eq!(s.student_json(), text, "{which} : projection non stable");
        assert_eq!(s.to_json(), text, "{which}");
        assert_eq!(s.bench_json(), v.bench_json());
        assert_eq!(s.slot_count(), v.slot_count());
        assert_eq!(s.slot_lsb(0), None);
        assert_eq!(s.frame_ids(), v.frame_ids());
        for id in v.frame_ids() {
            assert_eq!(s.frame_timing(id), v.frame_timing(id));
        }
        assert_eq!((s.mission(), s.tick_period_ms(), s.adc_channels()), (v.mission(), v.tick_period_ms(), v.adc_channels()));
        assert_eq!((s.bench_ecu(), s.bench_sensor()), (v.bench_ecu(), v.bench_sensor()));
        // formats are not interchangeable
        assert!(Pack::from_json(&text).is_err());
        assert!(Pack::from_student_json(&v.to_json()).is_err());
        let mut extra = sj.clone();
        extra["slots"] = json!([]);
        assert!(Pack::from_student_json(&extra.to_string()).is_err(), "{which} : clé d'oracle acceptée");
        assert!(s.variant(&seed(1)).is_err());

        // the simulation is the same: nominal scenario and vehicle output. A public build's background VCU reads only
        // the source names of its consumer parameters, so the whole bus is identical; an instructor build runs the
        // student pack's background VCU on neutral parameters, so there only the mission source ECU's frames are.
        let (sa, sb) = (NominalScenario::new(&v, 9), NominalScenario::new(&s, 9));
        assert_eq!(sa.duration_us(), sb.duration_us());
        let end = sa.duration_us();
        assert!((0..=end).step_by(1000).all(|t| sa.position_at(t) == sb.position_at(t)));
        let mut va = Vehicle::new(&v, 77);
        let mut vb = Vehicle::new(&s, 77);
        let fa = va.advance(end, &mut |m| v.ideal_inputs(sa.position_at(m * 1000)));
        let fb = vb.advance(end, &mut |m| s.ideal_inputs(sb.position_at(m * 1000)));
        assert!(!fa.is_empty());
        if n7rt_vehicle::ORACLE {
            let source: BTreeSet<u32> = Pack::source_vocabulary()
                .iter()
                .filter(|s| s.starts_with("apps."))
                .filter_map(|s| v.frame_of_source(s))
                .collect();
            let ma: Vec<&BusFrame> = fa.iter().filter(|f| source.contains(&f.id)).collect();
            let mb: Vec<&BusFrame> = fb.iter().filter(|f| source.contains(&f.id)).collect();
            assert!(!ma.is_empty());
            assert_eq!(ma, mb, "{which} : trames de l'ECU source différentes avec le pack étudiant");
        } else {
            assert_eq!(fa, fb, "{which} : sortie du véhicule différente avec le pack étudiant");
        }

        // the background VCU's consumer parameters are neutral: none of the oracle's reaches a student
        let vcu = &sj["background"]["vcu"];
        let full = doc(&v);
        assert_eq!(vcu["apps"]["travel_sources"], full["consumer"]["apps"]["travel_sources"]);
        assert_eq!(vcu["apps"]["max_diff_pct"], json!(100.0), "{which}");
        assert_eq!(vcu["apps"]["persistence_ms"], json!(0), "{which}");
        assert_eq!(vcu["apps"]["substitute_pct"], json!(0.0), "{which}");
        assert_eq!(vcu["p22"]["timeout_periods"], json!(1_000_000), "{which}");
        assert_eq!(vcu["state_machine"]["window_size_valid"], json!(1), "{which}");

        // a public build has no oracle: nothing can be judged
        if !n7rt_vehicle::ORACLE {
            assert!(RefConsumer::try_new(&v).is_err());
            assert!(judge_run(&v, &small_run()).is_err());
            assert!(!compare(&v, &[], &[]).pass);
        }
    }
}


// ------------------------------------------------------------------------------------------------ run files

fn small_run() -> RunRecord {
    RunRecord {
        mission: "demo".into(),
        cohort: "2026-A".into(),
        group: "G07".into(),
        pack_id: "ffeeddccbbaa99887766554433221100".into(),
        app_version: "0.1.0".into(),
        board_uid: "0035001A3438510A".into(),
        runtime_build: "r1".into(),
        student_crc32: "1234abcd".into(),
        seed_vehicle: "00000000deadbeef".into(),
        seed_scenario: "0000000012345678".into(),
        duration_us: 30_000,
        tick_us: 10_000,
        started_utc: "2026-10-06T09:30:00Z".into(),
        frames: vec![
            RunFrame { t_us: 2_120, id: 0x123, flags: 0, data: vec![0x00, 0x10, 0xFF, 0x7F, 0x80, 0x01, 0xAB, 0xCD] },
            RunFrame { t_us: 9_000, id: 0x2C4, flags: 2, data: vec![0x01, 0x02, 0x03, 0x04] },
            RunFrame { t_us: 12_500, id: 0x355, flags: 0, data: vec![] },
        ],
        ticks: vec![
            RunTick { t_us: 10_000, slots: vec![Some(RunSlot::V(0.1)), Some(RunSlot::Reject), None] },
            RunTick { t_us: 20_000, slots: vec![Some(RunSlot::V(25.03f32 as f64)), Some(RunSlot::V(-1e-7)), Some(RunSlot::V(1e300))] },
            RunTick { t_us: 30_000, slots: vec![] },
        ],
    }
}

#[test]
fn run_file_round_trip_and_canonical_form() {
    let r = small_run();
    let bytes = r.to_file_bytes();
    assert_eq!(bytes, r.to_file_bytes(), "sérialisation non déterministe");
    let text = String::from_utf8(bytes.clone()).unwrap();
    assert!(text.starts_with("{\"app_version\":"), "clés non triées");
    assert!(text.contains("\"format\":\"n7rt-run\"") && text.contains("\"version\":1"));
    assert!(!text.contains(' ') && !text.contains('\n'), "espaces dans la forme canonique");
    let v: Value = serde_json::from_str(&text).unwrap();
    let integrity = v["integrity"].as_str().unwrap();
    assert!(integrity.len() == 64 && integrity.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    assert_eq!(RunRecord::from_file_bytes(&bytes).unwrap(), r);
    // any JSON layout of the same document is accepted
    let pretty = serde_json::to_string_pretty(&v).unwrap();
    assert_eq!(RunRecord::from_file_bytes(pretty.as_bytes()).unwrap(), r);
    // non-finite values survive
    let mut nf = small_run();
    nf.ticks[2].slots = vec![Some(RunSlot::V(f64::NAN)), Some(RunSlot::V(f64::INFINITY)), Some(RunSlot::V(f64::NEG_INFINITY))];
    let back = RunRecord::from_file_bytes(&nf.to_file_bytes()).unwrap();
    match back.ticks[2].slots[..] {
        [Some(RunSlot::V(a)), Some(RunSlot::V(b)), Some(RunSlot::V(c))] => {
            assert!(a.is_nan() && b == f64::INFINITY && c == f64::NEG_INFINITY)
        }
        _ => panic!("valeurs non finies perdues"),
    }
}

#[test]
fn run_file_tamper_detection() {
    let bytes = small_run().to_file_bytes();
    for i in 0..bytes.len() {
        let mut b = bytes.clone();
        b[i] ^= 0x01;
        assert!(RunRecord::from_file_bytes(&b).is_err(), "octet {i} modifié non détecté");
        let mut b = bytes.clone();
        b[i] = if b[i] == b'7' { b'3' } else { b'7' };
        assert!(RunRecord::from_file_bytes(&b).is_err(), "octet {i} remplacé non détecté");
    }
    let text = String::from_utf8(bytes.clone()).unwrap();
    let e = RunRecord::from_file_bytes(text.replace("\"G07\"", "\"G08\"").as_bytes()).unwrap_err();
    assert_eq!(e, "empreinte invalide : fichier altéré");
    let e = RunRecord::from_file_bytes(text.replace("\"version\":1", "\"version\":2").as_bytes()).unwrap_err();
    assert_eq!(e, "version non prise en charge");
    let e = RunRecord::from_file_bytes(&bytes[..bytes.len() / 2]).unwrap_err();
    assert!(e.starts_with("fichier d'essai illisible"), "{e}");
    let mut v: Value = serde_json::from_str(&text).unwrap();
    v.as_object_mut().unwrap().remove("integrity");
    assert_eq!(RunRecord::from_file_bytes(v.to_string().as_bytes()).unwrap_err(), "empreinte invalide : fichier altéré");
    let e = RunRecord::from_file_bytes(b"{\"format\":\"autre\",\"version\":1}").unwrap_err();
    assert!(e.starts_with("fichier d'essai illisible"), "{e}");
}

/// Private APPS mission files: `N7RT_MISSIONS_DIR/apps`, else `N7RT_INSTRUCTOR_DIR/missions/apps`, else, walking up from
/// this crate, `restricted/missions/apps` (single-folder layout) or `n7rt-canlab-missions/missions/apps` (instructor
/// repository checked out next to the student one). May not exist: callers check `pack-v0.json`.
fn private_apps_dir() -> std::path::PathBuf {
    use std::path::PathBuf;
    let has = |p: &PathBuf| p.join("pack-v0.json").is_file();
    for (var, sub) in [("N7RT_MISSIONS_DIR", "apps"), ("N7RT_INSTRUCTOR_DIR", "missions/apps")] {
        if let Ok(d) = std::env::var(var) {
            let p = PathBuf::from(d).join(sub);
            if has(&p) {
                return p;
            }
        }
    }
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    loop {
        for cand in [dir.join("restricted/missions/apps"), dir.join("n7rt-canlab-missions/missions/apps")] {
            if has(&cand) {
                return cand;
            }
        }
        if !dir.pop() {
            return PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("restricted-missions-apps-absent");
        }
    }
}
