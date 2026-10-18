//! Generic tests of the vehicle engine with a synthetic pack built here: no mission value is used or printed.
//! The receiver-side checks and the reference consumer chain are tested with the private oracle (tests/oracle.rs).
#![recursion_limit = "512"]

use std::collections::BTreeMap;

use n7rt_vehicle::e2e::{self, SmConfig};
use n7rt_vehicle::{BusFrame, EcuInputs, Pack, Vehicle};
use serde_json::{json, Value};

const DATA_IDS: [u8; 16] = [0x9C, 0x3B, 0xE1, 0x47, 0x08, 0xD5, 0x62, 0xAF, 0x1E, 0x73, 0xC4, 0x5D, 0x90, 0x2A, 0xB7, 0x66];

fn synthetic() -> Value {
    let ids: Vec<String> = DATA_IDS.iter().map(|b| format!("0x{b:02X}")).collect();
    json!({
        "format": "n7rt-mission-pack", "format_version": 1, "mission": "demo", "variant": "T1",
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
                { "name": "T1", "start": 23, "len": 12, "order": "motorola", "factor": 0.05, "offset": -5.0, "min": 0, "max": 4000,
                  "sna": "0xFFF", "qualifier": { "source": "apps.st1", "ok": 1 }, "source": "apps.travel1" },
                { "name": "T2", "start": 32, "len": 16, "signed": true, "factor": 0.02, "offset": 0, "min": -1000, "max": 6000,
                  "sna": "0x7FFF", "qualifier": { "source": "apps.st2", "ok": 1 }, "source": "apps.travel2" }
              ] },
            { "id": "0x2C4", "name": "SYN_B", "emitter": "SRC", "period_ms": 50, "phase_ms": 9.0, "dlc": 4,
              "protection": { "kind": "alive", "bits": 8 },
              "signals": [
                { "name": "A", "start": 0, "len": 8, "source": "alive" },
                { "name": "U1", "start": 8, "len": 8, "factor": 0.03, "min": 0, "max": 250, "sna": "0xFF", "source": "apps.sup1" },
                { "name": "U2", "start": 23, "len": 8, "order": "motorola", "factor": 0.03, "min": 0, "max": 250, "sna": "0xFF", "source": "apps.sup2" },
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
            { "slot": 2, "output": "diff", "kind": "value", "lsb": 0.02 },
            { "slot": 3, "output": "implausibility", "kind": "logic", "lsb": 1 },
            { "slot": 4, "output": "cut", "kind": "logic", "lsb": 1 },
            { "slot": 5, "output": "retained", "kind": "value", "lsb": 0.02 },
            { "slot": 6, "output": "supply1", "kind": "value", "lsb": 0.03 },
            { "slot": 7, "output": "supply2", "kind": "value", "lsb": 0.03 }
        ],
        "nominal": { "cold_start_ms": 500, "nominal_ms": 3000, "hold_ms": 200, "ramp_ms": [300, 600],
                     "segment_ms": [100, 300], "position_quantum": 0.001 },
        "compare": { "transition_tolerance_ms": 10, "value_tolerance_lsb": 0.5, "max_listed_mismatches": 5 }
    })
}

fn pack() -> Pack {
    Pack::from_json(&synthetic().to_string()).expect("synthetic pack")
}

fn values(pairs: &[(&str, Option<f64>)]) -> BTreeMap<String, Option<f64>> {
    pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()
}

// --------------------------------------------------------------------------------------------------- E2E

#[test]
fn crc8h2f_affinity_and_single_bit_detection() {
    for len in 1..256 {
        let a: Vec<u8> = (0..len).map(|i| (i * 31 + len * 7) as u8).collect();
        let b: Vec<u8> = (0..len).map(|i| (i * 17 + len * 11) as u8).collect();
        let xor: Vec<u8> = a.iter().zip(&b).map(|(x, y)| x ^ y).collect();
        assert_eq!(e2e::crc8h2f(&xor), e2e::crc8h2f(&a) ^ e2e::crc8h2f(&b) ^ e2e::crc8h2f(&vec![0; len]));
        let mut changed = a.clone();
        changed[len / 2] ^= 1;
        assert_ne!(e2e::crc8h2f(&changed), e2e::crc8h2f(&a));
    }
}

#[test]
fn sm_config_constraints() {
    let cfg = SmConfig {
        window_size_valid: 4,
        window_size_init: 2,
        window_size_invalid: 4,
        max_error_state_valid: 1,
        max_error_state_init: 0,
        max_error_state_invalid: 0,
        min_ok_state_valid: 2,
        min_ok_state_init: 2,
        min_ok_state_invalid: 3,
        clear_to_invalid: true,
    };
    assert!(cfg.check().is_ok());
    assert!(SmConfig { min_ok_state_valid: 3, ..cfg }.check().is_err());
}

// ------------------------------------------------------------------------------------------- pack and codec

#[test]
fn encoder_decoder_properties() {
    let p = pack();
    assert_eq!(p.mission(), "demo");
    assert_eq!(p.frame_ids(), vec![0x123, 0x2C4, 0x355]);
    assert_eq!(p.frame_of_source("apps.sup2"), Some(0x2C4));
    assert_eq!(p.bench_json(), "");
    // Motorola 12 bits from bit 23: byte 2 then the high nibble of byte 3; reserved bits stay at 1
    let v = values(&[
        ("apps.st1", Some(1.0)),
        ("apps.st2", Some(2.0)),
        ("apps.travel1", Some(0xABC as f64 * 0.05 - 5.0)),
        ("apps.travel2", Some(-12.34)),
    ]);
    let d = p.encode_frame(0x123, 7, &v).unwrap();
    assert_eq!(d[1], 0b1001_0111);
    assert_eq!(d[2], 0xAB);
    assert_eq!(d[3], 0xCF);
    assert_eq!(i16::from_le_bytes([d[4], d[5]]), -617);
    assert_eq!(&d[6..8], &[0xFF, 0xFF]);
    // counter 7 in the low nibble of byte 1, valid CRC over bytes 1…7 then DataIDList[7]
    assert_eq!(d[1] & 0x0F, 7);
    assert_eq!(e2e::p22_crc(&d, DATA_IDS[7]), d[0]);
    let back = p.decode_frame(0x123, &d).unwrap();
    assert!((back["apps.travel1"].unwrap() - (0xABC as f64 * 0.05 - 5.0)).abs() < 1e-9);
    assert!((back["apps.travel2"].unwrap() + 12.34).abs() < 1e-9);
    // SNA, rounding half up, saturation to the emitted range
    let v = values(&[("apps.st1", Some(0.0)), ("apps.st2", Some(1.0)), ("apps.travel1", None), ("apps.travel2", Some(500.0))]);
    let d = p.encode_frame(0x123, 0, &v).unwrap();
    let back = p.decode_frame(0x123, &d).unwrap();
    assert_eq!(back["apps.travel1"], None);
    assert!((back["apps.travel2"].unwrap() - 120.0).abs() < 1e-9);
    let v = values(&[("apps.st1", Some(0.0)), ("apps.st2", Some(1.0)), ("apps.travel1", Some(0.025)), ("apps.travel2", Some(0.01))]);
    let back = p.decode_frame(0x123, &p.encode_frame(0x123, 0, &v).unwrap()).unwrap();
    assert!((back["apps.travel1"].unwrap() - 0.05).abs() < 1e-9);
    assert!((back["apps.travel2"].unwrap() - 0.02).abs() < 1e-9);
    // alive counter frame, Motorola byte, round trip of every representable value
    for raw in [0u32, 1, 77, 160, 250] {
        let x = raw as f64 * 0.03;
        let v = values(&[("apps.sup1", Some(x)), ("apps.sup2", Some(x)), ("apps.ecu_mode", Some(2.0))]);
        let d = p.encode_frame(0x2C4, 300, &v).unwrap();
        assert_eq!(d[0], 44);
        assert_eq!(d[1], raw as u8);
        assert_eq!(d[2], raw as u8);
        assert_eq!(d[3], 0xFE);
        let back = p.decode_frame(0x2C4, &d).unwrap();
        assert!((back["apps.sup2"].unwrap() - x).abs() < 1e-9);
    }
    assert!(p.encode_frame(0x123, 0, &BTreeMap::new()).is_err());
}

#[test]
fn validation_errors_name_paths_not_names() {
    let mut j = synthetic();
    j["frames"][1]["id"] = json!("0x123");
    let e = Pack::from_json(&j.to_string()).unwrap_err();
    assert!(e.contains("frames[1].id"), "{e}");
    assert!(!e.contains("SYN_"));
    let mut j = synthetic();
    j["frames"][0]["signals"][5]["start"] = json!(56);
    let e = Pack::from_json(&j.to_string()).unwrap_err();
    assert!(e.contains("frames[0].signals[5]"), "{e}");
    let mut j = synthetic();
    j["frames"][0]["protection"]["data_ids"] = json!(["0x01", "0x02"]);
    let e = Pack::from_json(&j.to_string()).unwrap_err();
    assert!(e.contains("frames[0].protection.data_ids"), "{e}");
    let mut j = synthetic();
    j["frames"][2]["phase_ms"] = json!(32.5);
    let e = Pack::from_json(&j.to_string()).unwrap_err();
    assert!(e.contains("phase_ms"), "{e}");
    let mut j = synthetic();
    j["frames"][1]["signals"][3]["source"] = json!("apps.nothing");
    let e = Pack::from_json(&j.to_string()).unwrap_err();
    assert!(e.contains("frames[1].signals[3].source"), "{e}");
    let mut j = synthetic();
    j["frames"][0]["name"] = json!(12);
    let e = Pack::from_json(&j.to_string()).unwrap_err();
    assert!(!e.contains("SYN_"), "{e}");
}

// ------------------------------------------------------------------------------------------------- vehicle

/// Frames of the vehicle over `end_ms`, the ECU inputs held at an ideal `position`.
fn bus(p: &Pack, seed: u64, end_ms: u64, position: f64) -> Vec<BusFrame> {
    let mut v = Vehicle::new(p, seed);
    let inputs = p.ideal_inputs(position);
    let mut frames = Vec::new();
    for k in 1..=end_ms / 10 {
        frames.extend(v.advance(k * 10_000, &mut |_| inputs.clone()));
    }
    frames
}

#[test]
fn vehicle_schedule_counters_and_determinism() {
    let p = pack();
    let a = bus(&p, 11, 1000, 0.3);
    let b = bus(&p, 11, 1000, 0.3);
    let c = bus(&p, 12, 1000, 0.3);
    assert_eq!(a, b);
    assert_ne!(a, c);
    for id in p.frame_ids() {
        let t = p.frame_timing(id).unwrap();
        let fs: Vec<_> = a.iter().filter(|f| f.id == id).collect();
        assert!(!fs.is_empty());
        for f in &fs {
            assert_eq!(f.data.len(), t.dlc as usize);
            let k = ((f.t_us as f64 - t.phase_us as f64) / t.period_us as f64).round() as u64;
            let nominal = t.phase_us + k * t.period_us;
            assert!(f.t_us.abs_diff(nominal) <= 250, "jitter");
            assert!(nominal >= 40_000, "t_boot");
        }
    }
    // P22 counters consecutive, alive counters consecutive
    let p22: Vec<u8> = a.iter().filter(|f| f.id == 0x123).map(|f| f.data[1] & 0x0F).collect();
    assert_eq!(p22[0], 0);
    assert!(p22.windows(2).all(|w| w[1] == (w[0] + 1) % 16));
    let alive: Vec<u8> = a.iter().filter(|f| f.id == 0x2C4).map(|f| f.data[0]).collect();
    assert!(alive.windows(2).all(|w| w[1] == w[0].wrapping_add(1)));
    // frames sorted
    assert!(a.windows(2).all(|w| (w[0].t_us, w[0].id) <= (w[1].t_us, w[1].id)));
}

#[test]
fn source_ecu_follows_its_supply() {
    let p = pack();
    let mut v = Vehicle::new(&p, 3);
    let on = p.ideal_inputs(0.5);
    let off = EcuInputs { lv: Some(7.2), ..on.clone() };
    let mut frames = Vec::new();
    // below the reset exit threshold: silent; then powered from 300 ms
    for k in 1..=60u64 {
        frames.extend(v.advance(k * 10_000, &mut |ms| if ms < 300 { off.clone() } else { on.clone() }));
    }
    let src: Vec<_> = frames.iter().filter(|f| f.id != 0x355).collect();
    assert!(src.iter().all(|f| f.t_us >= 340_000 - 250));
    let first = src.iter().find(|f| f.id == 0x123).unwrap();
    assert_eq!(first.data[1] & 0x0F, 0);
    // INIT status code during the start phase, then OK
    assert_eq!((first.data[1] >> 4) & 0x03, 0);
    let later = src.iter().filter(|f| f.id == 0x123).nth(3).unwrap();
    assert_eq!((later.data[1] >> 4) & 0x03, 1);
    // the background frame is there from its first slot after t_boot
    assert!(frames.iter().any(|f| f.id == 0x355 && f.t_us < 140_000));
    assert_eq!(v.time_us(), 600_000);
    assert!(v.advance(500_000, &mut |_| on.clone()).is_empty());
}

// ------------------------------------------------------------------------------------------- frame corruption

use n7rt_vehicle::FrameFault;

/// Frames over `end_ms` with `fault` installed at `from_ms` (rank 0 = first emission after it) and removed at `to_ms`.
fn faulted(p: &Pack, fault: FrameFault, from_ms: u64, to_ms: u64, end_ms: u64) -> Vec<BusFrame> {
    let mut v = Vehicle::new(p, 11);
    let inputs = p.ideal_inputs(0.3);
    let mut frames = Vec::new();
    for k in 1..=end_ms / 10 {
        if k * 10 == from_ms + 10 {
            v.set_frame_fault(Some(fault)).unwrap();
        }
        if k * 10 == to_ms + 10 {
            v.set_frame_fault(None).unwrap();
        }
        frames.extend(v.advance(k * 10_000, &mut |_| inputs.clone()));
    }
    frames
}

#[test]
fn frame_fault_corrupts_exactly_its_pattern_after_protection() {
    let p = pack();
    let clean = bus(&p, 11, 4000, 0.3);
    // a pattern of no mission (theirs are private): 2 corrupted frames every 7
    for phase in [0u16, 3, 6] {
        let fault = FrameFault { id: 0x123, byte: 3, mask: 0x01, period: 7, burst: 2, phase };
        let bad = faulted(&p, fault, 1000, 3000, 4000);
        assert_eq!(clean.len(), bad.len());
        let mut rank = 0u64;
        let mut hits = Vec::new();
        for (c, b) in clean.iter().zip(&bad) {
            assert_eq!((c.t_us, c.id, c.dlc), (b.t_us, b.id, b.dlc), "schedule unchanged");
            // installed before the advance to 1.010 s, removed before the advance to 3.010 s
            let installed = c.id == 0x123 && c.t_us > 1_000_000 && c.t_us <= 3_000_000;
            if !installed {
                assert_eq!(c.data, b.data, "other frames and other times unchanged");
                continue;
            }
            let hit = (rank + phase as u64) % 7 < 2;
            hits.push(hit);
            rank += 1;
            let mut expected = c.data.clone();
            if hit {
                expected[3] ^= 0x01;
            }
            assert_eq!(b.data, expected, "only the mask, on the pattern ranks");
            // the protection was computed on the clean payload: it fails exactly on the corrupted frames
            let id = DATA_IDS[(b.data[1] & 0x0F) as usize];
            assert_eq!(e2e::p22_crc(&b.data, id) == b.data[0], !hit);
            assert_eq!(b.data[1] & 0x0F, c.data[1] & 0x0F, "counter sequence kept");
        }
        assert!(hits.len() >= 150);
        assert!(hits.windows(98).all(|w| w.iter().filter(|h| **h).count() == 28), "28 corrupted in any 98 consecutive frames");
    }
}

#[test]
fn frame_fault_refuses_inconsistent_settings() {
    let p = pack();
    let mut v = Vehicle::new(&p, 1);
    let ok = FrameFault { id: 0x123, byte: 3, mask: 0x01, period: 7, burst: 2, phase: 0 };
    assert!(v.set_frame_fault(Some(ok)).is_ok());
    for bad in [
        FrameFault { id: 0x124, ..ok },
        FrameFault { byte: 8, ..ok },
        FrameFault { mask: 0, ..ok },
        FrameFault { period: 0, burst: 0, ..ok },
        FrameFault { burst: 8, ..ok },
        FrameFault { phase: 7, ..ok },
    ] {
        assert!(v.set_frame_fault(Some(bad)).is_err(), "{bad:?}");
    }
    assert!(v.set_frame_fault(None).is_ok());
}

// -------------------------------------------------------------------------------------------- source overrides

#[test]
fn source_overrides_are_validated_and_keep_the_pack_otherwise() {
    let p = pack();
    let same = p.with_source_overrides(&[]).unwrap();
    assert_eq!(bus(&p, 11, 1000, 0.3), bus(&same, 11, 1000, 0.3), "no override: same bus");
    let changed = p.with_source_overrides(&[("sig_divider".into(), json!(2.0))]).unwrap();
    assert_ne!(bus(&p, 11, 1000, 0.3), bus(&changed, 11, 1000, 0.3), "the source ECU follows its new parameter");
    for (path, value) in [("nope", json!(1)), ("sig_divider.x", json!(1)), ("sig_divider", json!("2")), ("adc_bits", json!(0))] {
        assert!(p.with_source_overrides(&[(path.into(), value.clone())]).is_err(), "{path} = {value}");
    }
    let student = Pack::from_student_json(&p.student_json()).unwrap();
    let s2 = student.with_source_overrides(&[("sig_divider".into(), json!(2.0))]).unwrap();
    assert!(s2.is_student());
    assert_eq!(bus(&changed, 11, 1000, 0.3), bus(&s2, 11, 1000, 0.3), "student projection overridden alike");
}

#[test]
fn reconfiguring_the_source_keeps_every_dynamic_state() {
    let p = pack();
    let changed = p.with_source_overrides(&[("sig_divider".into(), json!(2.0))]).unwrap();
    let inputs = p.ideal_inputs(0.3);
    let run = |swap: Option<(&Pack, u64)>| {
        let mut v = Vehicle::new(&p, 11);
        let mut frames = Vec::new();
        for k in 1..=100u64 {
            if let Some((other, at)) = swap {
                if k == at {
                    v.reconfigure_source(other).unwrap();
                }
            }
            frames.extend(v.advance(k * 10_000, &mut |_| inputs.clone()));
        }
        frames
    };
    let plain = run(None);
    assert_eq!(run(Some((&p, 50))), plain, "an identical configuration changes nothing");
    let swapped = run(Some((&changed, 50)));
    let before = |f: &&BusFrame| f.t_us <= 490_000;
    assert_eq!(swapped.iter().filter(before).collect::<Vec<_>>(), plain.iter().filter(before).collect::<Vec<_>>());
    assert_ne!(swapped, plain, "the new configuration acts from the swap on");
    // counters continue across the swap
    let p22: Vec<u8> = swapped.iter().filter(|f| f.id == 0x123).map(|f| f.data[1] & 0x0F).collect();
    assert!(p22.windows(2).all(|w| w[1] == (w[0] + 1) % 16));
    let mut v = Vehicle::new(&p, 1);
    let mut j = synthetic();
    j["frames"][2]["period_ms"] = json!(50);
    let other_matrix = Pack::from_json(&j.to_string()).unwrap();
    assert!(v.reconfigure_source(&other_matrix).is_err());
}
