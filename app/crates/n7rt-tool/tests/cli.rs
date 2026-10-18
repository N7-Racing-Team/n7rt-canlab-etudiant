//! n7rt-tool commands called as library functions on a temporary instructor directory (no terminal needed).
//! Packs: a synthetic pack built here (no mission value), or the restricted APPS pack read at run time (skipped when
//! absent). Tests marked "needs lot V2" call the distribution half of n7rt-vehicle (variant, student projection,
//! run files, judge_run).
#![recursion_limit = "512"]

use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use n7rt_seal::{group_seed, open_pack, read_pack_header, seal_pack_with, KdfParams, PackHeader};
use n7rt_tool::{
    check_instructor_code, gen_packs, inspect, judge, load_groups, run, set_code, test_pack, Cli, Command, Generated,
    LineCodes, EXIT_NO, EXIT_OK, MAX_PACK_BYTES, MAX_RUN_BYTES,
};
use n7rt_vehicle::{NominalScenario, Pack, RefConsumer, RunFrame, RunRecord, RunSlot, RunTick, SlotOut, Vehicle};
use serde_json::{json, Value};

const CODE: &str = "Code encadrant de test 42";
const OTHER: &str = "Un autre code encadrant";

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let p = std::env::temp_dir().join(format!("n7rt-tool-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn codes(lines: &[&str]) -> LineCodes<Cursor<Vec<u8>>> {
    let s: String = lines.iter().map(|l| format!("{l}\n")).collect();
    LineCodes(Cursor::new(s.into_bytes()))
}

/// Instructor directory with CODE set.
fn instructor(name: &str) -> TempDir {
    let d = TempDir::new(name);
    set_code(d.path(), &mut codes(&[CODE, CODE]), &mut Vec::new()).unwrap();
    d
}

fn text(out: &[u8]) -> String {
    String::from_utf8(out.to_vec()).unwrap()
}

const DATA_IDS: [u8; 16] = [0x9C, 0x3B, 0xE1, 0x47, 0x08, 0xD5, 0x62, 0xAF, 0x1E, 0x73, 0xC4, 0x5D, 0x90, 0x2A, 0xB7, 0x66];

/// Synthetic pack (same fixture as n7rt-vehicle's generic tests): public, invented values.
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

/// Writes the synthetic reference pack at missions/demo/pack-v0.json.
fn with_synthetic_pack(root: &Path) {
    let dir = root.join("missions").join("demo");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("pack-v0.json"), synthetic().to_string()).unwrap();
}

fn master(root: &Path) -> [u8; 32] {
    let s = fs::read_to_string(root.join("instructor").join("master.key")).unwrap();
    core::array::from_fn(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap())
}

// ------------------------------------------------------------------------------------------ code management

#[test]
fn set_code_writes_auth_and_master_key() {
    let d = TempDir::new("setcode");
    let mut out = Vec::new();
    set_code(d.path(), &mut codes(&[CODE, CODE]), &mut out).unwrap();
    assert!(text(&out).contains("Clé maîtresse créée"));

    let auth: Value = serde_json::from_str(&fs::read_to_string(d.path().join("instructor/auth.json")).unwrap()).unwrap();
    assert_eq!(auth["format"], "n7rt-instructor-auth");
    assert_eq!(auth["version"], 1);
    assert!(auth["code_phc"].as_str().unwrap().starts_with("$argon2id$v=19$m=65536,t=3,p=1$"));
    assert_eq!(auth.as_object().unwrap().len(), 3);
    let key = fs::read_to_string(d.path().join("instructor/master.key")).unwrap();
    assert_eq!(key.len(), 64);
    assert!(key.bytes().all(|c| c.is_ascii_hexdigit()));
    for f in ["instructor/auth.json", "instructor/master.key"] {
        let t = fs::read_to_string(d.path().join(f)).unwrap().to_uppercase();
        assert!(!t.contains("ENCADRANT") && !t.contains("CODEENCADRANT"), "{f}");
    }

    assert!(check_instructor_code(d.path(), &mut codes(&[CODE])).unwrap());
    assert!(check_instructor_code(d.path(), &mut codes(&["code-encadrant  DE TEST 42"])).unwrap());
    assert!(!check_instructor_code(d.path(), &mut codes(&[OTHER])).unwrap());

    // Changing the code needs the current one; the master key is never replaced.
    let e = set_code(d.path(), &mut codes(&[OTHER, OTHER, OTHER]), &mut Vec::new()).unwrap_err();
    assert!(e.0.contains("actuel incorrect"), "{e}");
    let mut out = Vec::new();
    set_code(d.path(), &mut codes(&[CODE, OTHER, OTHER]), &mut out).unwrap();
    assert!(text(&out).contains("existante conservée"));
    assert_eq!(fs::read_to_string(d.path().join("instructor/master.key")).unwrap(), key);
    assert!(check_instructor_code(d.path(), &mut codes(&[OTHER])).unwrap());
    assert!(!check_instructor_code(d.path(), &mut codes(&[CODE])).unwrap());
}

#[test]
fn set_code_refusals() {
    let d = TempDir::new("setcode-refusals");
    let e = set_code(d.path(), &mut codes(&[CODE, OTHER]), &mut Vec::new()).unwrap_err();
    assert!(e.0.contains("diffèrent"), "{e}");
    // 11 characters once spaces and dashes are removed.
    let short = " abc-def ghi-jk ";
    let e = set_code(d.path(), &mut codes(&[short, short]), &mut Vec::new()).unwrap_err();
    assert!(e.0.contains("12 caractères"), "{e}");
    assert!(!d.path().join("instructor/auth.json").exists());
    let e = set_code(d.path(), &mut codes(&[CODE]), &mut Vec::new()).unwrap_err();
    assert!(e.0.contains("entrée standard"), "{e}");
    for e in [
        set_code(&d.path().join("absent"), &mut codes(&[CODE, CODE]), &mut Vec::new()).unwrap_err(),
        check_instructor_code(d.path(), &mut codes(&[CODE])).unwrap_err(),
    ] {
        assert!(!e.0.contains(CODE), "{e}");
    }
    let e = check_instructor_code(d.path(), &mut codes(&[CODE])).unwrap_err();
    assert!(e.0.contains("set-code"), "{e}");
}

#[test]
fn verify_code_exit_codes() {
    let d = instructor("verify");
    let cli = Cli { command: Command::VerifyCode { dir: d.path().to_path_buf() }, code_stdin: true };
    let mut out = Vec::new();
    assert_eq!(run(&cli, &mut codes(&[CODE]), &mut out), Ok(EXIT_OK));
    assert!(text(&out).contains("correct"));
    let mut out = Vec::new();
    assert_eq!(run(&cli, &mut codes(&[OTHER]), &mut out), Ok(EXIT_NO));
    assert!(text(&out).contains("incorrect"));
    assert!(run(&cli, &mut codes(&[]), &mut Vec::new()).is_err());
}

// ------------------------------------------------------------------------------------------- pack generation

#[test]
fn gen_packs_refusals_before_any_write() {
    let d = instructor("gen-refusals");
    let root = d.path();
    let groups = |g: &[&str]| g.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let gen = |g: &[&str], c: &[&str], replace: bool| {
        gen_packs(root, "demo", "2026", &groups(g), replace, &mut codes(c), &mut Vec::new()).unwrap_err().0
    };
    // Argument errors come before the code is asked (empty code source).
    assert!(gen(&["G/1"], &[], false).contains("invalide"));
    assert!(gen(&["G01", "g01"], &[], false).contains("deux fois"));
    assert!(gen_packs(root, "demo", "../x", &groups(&["G01"]), false, &mut codes(&[]), &mut Vec::new())
        .unwrap_err()
        .0
        .contains("invalide"));
    assert!(gen(&["G01"], &[OTHER], false).contains("code encadrant incorrect"));
    assert!(gen(&["G01"], &[CODE], false).contains("pack de référence absent"));
    with_synthetic_pack(root);
    let gpath = root.join("cohorts/2026/demo/groups.json");
    fs::create_dir_all(gpath.parent().unwrap()).unwrap();
    fs::write(&gpath, r#"[{"group":"G01","pack_id":"00","code":"X","created_utc":"t","note":"kept"}]"#).unwrap();
    let e = gen(&["G02", "g01"], &[CODE], false);
    assert!(e.contains("déjà généré") && e.contains("--replace"), "{e}");
    assert!(!root.join("cohorts/2026/demo/packs").exists());
    let e = test_pack(root, "demo", "2026", "G01", false, &mut codes(&[CODE]), &mut Vec::new()).unwrap_err().0;
    assert!(e.contains("déjà généré"), "{e}");
    fs::remove_file(root.join("instructor/master.key")).unwrap();
    assert!(gen(&["G03"], &[CODE], false).contains("master.key"));
    assert_eq!(load_groups(&gpath).unwrap().len(), 1);
}

#[test]
fn inspect_pack_shows_header_only() {
    let d = TempDir::new("inspect");
    let h = PackHeader {
        mission: "demo".into(),
        cohort: "2026".into(),
        group: "G07".into(),
        pack_id: "0123456789abcdef0123456789abcdef".into(),
    };
    let sealed = seal_pack_with(&h, b"{\"student\":true}", "7K4Q-M2ZD-9XRT-4WBC", KdfParams { m_kib: 64, t: 1, p: 1 }).unwrap();
    let f = d.path().join("demo-G07.n7rtpack");
    fs::write(&f, &sealed).unwrap();
    let mut out = Vec::new();
    inspect(&f, &mut out).unwrap();
    let t = text(&out);
    for want in ["demo", "2026", "G07", "0123456789abcdef0123456789abcdef", "argon2id", "aes-256-gcm"] {
        assert!(t.contains(want), "{want} in {t}");
    }
    assert!(!t.contains("7K4Q") && !t.contains("student"));
    // Sniffed without the extension too.
    let g = d.path().join("pack.bin");
    fs::write(&g, &sealed).unwrap();
    inspect(&g, &mut Vec::new()).unwrap();
    assert!(inspect(&d.path().join("absent.n7rtpack"), &mut Vec::new()).is_err());
    fs::write(d.path().join("bad.n7rtpack"), b"{}").unwrap();
    assert!(inspect(&d.path().join("bad.n7rtpack"), &mut Vec::new()).is_err());
}

#[test]
fn gen_packs_and_test_pack_with_synthetic_pack() {
    let d = instructor("gen-synthetic");
    let root = d.path();
    with_synthetic_pack(root);
    let reference = Pack::load(&root.join("missions/demo/pack-v0.json")).unwrap();
    let groups = vec!["G01".to_string(), "G02".to_string()];
    let mut out = Vec::new();
    let gen = gen_packs(root, "demo", "2026", &groups, false, &mut codes(&[CODE]), &mut out).unwrap();
    let t = text(&out);
    assert!(t.contains("Groupe") && t.contains("G01") && t.contains("G02") && t.contains(&gen[0].code));

    let entries = load_groups(&root.join("cohorts/2026/demo/groups.json")).unwrap();
    assert_eq!(entries.len(), 2);
    let master = master(root);
    let mut plains = Vec::new();
    for (g, e) in gen.iter().zip(&entries) {
        assert_eq!(e.group, g.group);
        assert_eq!(e.code, g.code);
        assert_eq!(e.pack_id, g.pack_id);
        assert!(e.variant.is_none());
        assert_eq!(g.file, root.join(format!("cohorts/2026/demo/packs/{}.n7rtpack", g.group)));
        let file = fs::read(&g.file).unwrap();
        let h = read_pack_header(&file).unwrap();
        assert_eq!((h.mission.as_str(), h.cohort.as_str(), h.group.as_str()), ("demo", "2026", g.group.as_str()));
        let (_, plain) = open_pack(&file, &g.code).unwrap();
        let plain = String::from_utf8(plain).unwrap();
        let expected = reference.variant(&group_seed(&master, "demo", "2026", &g.group)).unwrap().student_json();
        assert_eq!(plain, expected, "student projection of the group variant");
        assert!(Pack::from_student_json(&plain).unwrap().is_student());
        plains.push(plain);
    }
    assert_ne!(plains[0], plains[1], "each group has its own variant");

    // Regeneration needs --replace: new code and pack_id, the old pack_id is kept for judging.
    assert!(gen_packs(root, "demo", "2026", &groups[..1], false, &mut codes(&[CODE]), &mut Vec::new()).is_err());
    let again = gen_packs(root, "demo", "2026", &groups[..1], true, &mut codes(&[CODE]), &mut Vec::new()).unwrap();
    assert_ne!(again[0].code, gen[0].code);
    let entries = load_groups(&root.join("cohorts/2026/demo/groups.json")).unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].previous_pack_ids, vec![gen[0].pack_id.clone()]);
    let (_, plain) = open_pack(&fs::read(&again[0].file).unwrap(), &again[0].code).unwrap();
    assert_eq!(String::from_utf8(plain).unwrap(), plains[0], "same seed, same variant");

    // Instructor test pack: the reference variant itself.
    let tp = test_pack(root, "demo", "essais", "TEST", false, &mut codes(&[CODE]), &mut Vec::new()).unwrap();
    let entries = load_groups(&root.join("cohorts/essais/demo/groups.json")).unwrap();
    assert_eq!(entries[0].variant.as_deref(), Some("v0"));
    let (_, plain) = open_pack(&fs::read(&tp.file).unwrap(), &tp.code).unwrap();
    assert_eq!(String::from_utf8(plain).unwrap(), reference.student_json());
}

#[test]
fn a_cohort_keeps_the_spelling_of_its_folder() {
    let d = instructor("cohort-case");
    let root = d.path();
    with_synthetic_pack(root);
    let reference = Pack::load(&root.join("missions/demo/pack-v0.json")).unwrap();
    let master = master(root);
    gen_packs(root, "demo", "Promo", &["G01".to_string()], false, &mut codes(&[CODE]), &mut Vec::new()).unwrap();
    // Typed in another case, the cohort is the existing folder (one folder on Windows): its spelling seeds the variant,
    // goes into the pack header and the paths, so the judge finds the same cohort.
    let second = gen_packs(root, "demo", "promo", &["G02".to_string()], false, &mut codes(&[CODE]), &mut Vec::new()).unwrap();
    assert_eq!(read_pack_header(&fs::read(&second[0].file).unwrap()).unwrap().cohort, "Promo");
    assert!(second[0].file.starts_with(root.join("cohorts").join("Promo")));
    let entries = load_groups(&root.join("cohorts/Promo/demo/groups.json")).unwrap();
    assert_eq!(entries.iter().map(|e| e.group.as_str()).collect::<Vec<_>>(), ["G01", "G02"]);
    let plain = String::from_utf8(open_pack(&fs::read(&second[0].file).unwrap(), &second[0].code).unwrap().1).unwrap();
    assert_eq!(plain, reference.variant(&group_seed(&master, "demo", "Promo", "G02")).unwrap().student_json());
}

#[test]
fn group_names_ignore_case_and_a_group_keeps_its_variant() {
    let d = instructor("case");
    let root = d.path();
    with_synthetic_pack(root);
    let reference = Pack::load(&root.join("missions/demo/pack-v0.json")).unwrap();
    let master = master(root);
    let variant_json = |g: &str| reference.variant(&group_seed(&master, "demo", "2026", g)).unwrap().student_json();
    let plain = |g: &Generated| String::from_utf8(open_pack(&fs::read(&g.file).unwrap(), &g.code).unwrap().1).unwrap();
    let gen = |g: &str, replace: bool| gen_packs(root, "demo", "2026", &[g.to_string()], replace, &mut codes(&[CODE]), &mut Vec::new());

    // A new group is generated in upper case, whatever the case typed.
    let first = gen("g05", false).unwrap();
    assert_eq!(first[0].group, "G05");
    assert_eq!(first[0].file, root.join("cohorts/2026/demo/packs/G05.n7rtpack"));
    assert_eq!(read_pack_header(&fs::read(&first[0].file).unwrap()).unwrap().group, "G05");
    assert_eq!(plain(&first[0]), variant_json("G05"));
    // Typed in another case, it is the same group: refused without --replace, regenerated with it under its recorded
    // name and with its variant, the earlier pack kept for judging.
    let e = gen("g05", false).unwrap_err().0;
    assert!(e.contains("déjà généré(s) : G05"), "{e}");
    let again = gen("g05", true).unwrap();
    assert_eq!(again[0].group, "G05");
    assert_eq!(plain(&again[0]), variant_json("G05"), "same variant");
    let gpath = root.join("cohorts/2026/demo/groups.json");
    let entries = load_groups(&gpath).unwrap();
    assert_eq!((entries.len(), entries[0].group.as_str()), (1, "G05"));
    assert_eq!(entries[0].previous_pack_ids, vec![first[0].pack_id.clone()]);

    // A group recorded in lower case earlier keeps its spelling, hence its variant.
    let legacy = "fedcba9876543210fedcba9876543210";
    let mut groups: Value = serde_json::from_str(&fs::read_to_string(&gpath).unwrap()).unwrap();
    groups.as_array_mut().unwrap().push(json!({ "group": "g06", "pack_id": legacy, "code": "X", "created_utc": "t" }));
    fs::write(&gpath, groups.to_string()).unwrap();
    let kept = gen("G06", true).unwrap();
    assert_eq!(kept[0].group, "g06");
    assert_ne!(variant_json("g06"), variant_json("G06"), "the recorded spelling seeds the variant");
    assert_eq!(plain(&kept[0]), variant_json("g06"));
    assert_eq!(load_groups(&gpath).unwrap()[1].previous_pack_ids, vec![legacy.to_string()]);

    // Instructor test packs follow the same rule.
    let tp = test_pack(root, "demo", "essais", "test", false, &mut codes(&[CODE]), &mut Vec::new()).unwrap();
    assert_eq!(tp.group, "TEST");

    // A run of the pack replaced under another case is judged against the group's variant.
    if n7rt_vehicle::ORACLE {
        let runs = TempDir::new("case-runs");
        let variant = reference.variant(&group_seed(&master, "demo", "2026", "G05")).unwrap();
        let p = runs.path().join("essai-demo-G05-20261006-080000.n7rtrun");
        fs::write(&p, perfect_run(&variant, "2026", "G05", &first[0].pack_id).to_file_bytes()).unwrap();
        let j = judge(root, &p, &mut codes(&[CODE]), &mut Vec::new()).unwrap();
        assert!(j.pass, "{}", j.detail);
        assert_eq!(j.group, "G05");
    }
}

// ---------------------------------------------------------------------------------------------------- judge

/// A run whose board answers exactly like the reference consumer (feed rule of the instructor's local check).
fn perfect_run(pack: &Pack, cohort: &str, group: &str, pack_id: &str) -> RunRecord {
    let (seed_v, seed_s) = (7u64, 11u64);
    let scenario = NominalScenario::new(pack, seed_s);
    let duration = scenario.duration_us();
    let tick = u64::from(pack.tick_period_ms()) * 1000;
    let mut vehicle = Vehicle::new(pack, seed_v);
    let frames = vehicle.advance(duration, &mut |t_ms| pack.ideal_inputs(scenario.position_at(t_ms * 1000)));
    let mut rc = RefConsumer::new(pack);
    let mut ticks = Vec::new();
    let mut fi = 0;
    for k in 1..=duration / tick {
        let t = k * tick;
        while fi < frames.len() && frames[fi].t_us < t {
            rc.on_frame(frames[fi].t_us, frames[fi].id, 0, &frames[fi].data);
            fi += 1;
        }
        let slots = rc
            .on_tick(t)
            .slots
            .iter()
            .map(|s| s.map(|s| match s {
                SlotOut::Value(v) => RunSlot::V(v),
                SlotOut::Reject => RunSlot::Reject,
            }))
            .collect();
        ticks.push(RunTick { t_us: t, slots });
    }
    RunRecord {
        mission: pack.mission().into(),
        cohort: cohort.into(),
        group: group.into(),
        pack_id: pack_id.into(),
        app_version: "test".into(),
        board_uid: "TESTBOARD".into(),
        runtime_build: "test".into(),
        student_crc32: "00000000".into(),
        seed_vehicle: format!("{seed_v:016x}"),
        seed_scenario: format!("{seed_s:016x}"),
        duration_us: duration,
        tick_us: tick,
        started_utc: "2026-10-06T08:00:00Z".into(),
        frames: frames.iter().map(|f| RunFrame { t_us: f.t_us, id: f.id, flags: 0, data: f.data.clone() }).collect(),
        ticks,
    }
}

#[test]
fn judge_pass_fail_and_refusals() {
    if !n7rt_vehicle::ORACLE {
        // public build: `judge` refuses before asking for the code
        let e = judge(Path::new("."), Path::new("absent.n7rtrun"), &mut codes(&[]), &mut Vec::new()).unwrap_err();
        assert_eq!(e.0, n7rt_tool::ORACLE_ABSENT);
        return;
    }
    let d = instructor("judge");
    let root = d.path();
    with_synthetic_pack(root);
    let reference = Pack::load(&root.join("missions/demo/pack-v0.json")).unwrap();
    let gen = gen_packs(root, "demo", "2026", &["G01".to_string()], false, &mut codes(&[CODE]), &mut Vec::new()).unwrap();
    let variant = reference.variant(&group_seed(&master(root), "demo", "2026", "G01")).unwrap();
    let runs = TempDir::new("judge-runs");

    // Passing run, through the CLI entry (exit 0) and the judged record.
    let good = perfect_run(&variant, "2026", "G01", &gen[0].pack_id);
    let good_path = runs.path().join("essai-demo-G01-20261006-080000.n7rtrun");
    fs::write(&good_path, good.to_file_bytes()).unwrap();
    let cli = Cli { command: Command::Judge { dir: root.to_path_buf(), run: good_path.clone() }, code_stdin: true };
    let mut out = Vec::new();
    assert_eq!(run(&cli, &mut codes(&[CODE]), &mut out), Ok(EXIT_OK), "{}", text(&out));
    assert!(text(&out).contains("VERDICT : réussi"));
    let judged = root.join("cohorts/2026/demo/judged/essai-demo-G01-20261006-080000.json");
    let first = fs::read_to_string(&judged).unwrap();
    let j: Value = serde_json::from_str(&first).unwrap();
    assert_eq!(j["pass"], true);
    assert_eq!(j["verdict"], "réussi");
    assert_eq!(j["group"], "G01");
    assert_eq!(j["started_utc"], "2026-10-06T08:00:00Z");
    let integrity: Value = serde_json::from_slice::<Value>(&good.to_file_bytes()).unwrap()["integrity"].clone();
    assert!(integrity.is_string() && j["run_integrity"] == integrity);
    assert!(j["judged_utc"].is_string() && j["detail"].is_string());

    // A later judgement under the same name never replaces it: next free name, which the tool names.
    let mut out = Vec::new();
    let again = judge(root, &good_path, &mut codes(&[CODE]), &mut out).unwrap();
    assert_eq!(again.judged_file, root.join("cohorts/2026/demo/judged/essai-demo-G01-20261006-080000-2.json"));
    assert!(text(&out).contains("essai-demo-G01-20261006-080000-2.json"), "{}", text(&out));
    assert_eq!(again.started_utc, "2026-10-06T08:00:00Z");
    assert_eq!(fs::read_to_string(&judged).unwrap(), first, "first judgement untouched");
    let other_runs = TempDir::new("judge-runs-other");
    let same_name = other_runs.path().join("essai-demo-G01-20261006-080000.n7rtrun");
    fs::write(&same_name, good.to_file_bytes()).unwrap();
    let third = judge(root, &same_name, &mut codes(&[CODE]), &mut Vec::new()).unwrap();
    assert_eq!(third.judged_file, root.join("cohorts/2026/demo/judged/essai-demo-G01-20261006-080000-3.json"));
    assert!(again.judged_file.is_file() && third.judged_file.is_file());

    // The same run judged against the reference variant fails: the variant matters.
    let mut wrong = good.clone();
    for t in &mut wrong.ticks {
        for s in t.slots.iter_mut().flatten() {
            if let RunSlot::V(v) = s {
                *v += 1000.0;
            }
        }
    }
    let wrong_path = runs.path().join("essai-demo-G01-20261006-080100.n7rtrun");
    fs::write(&wrong_path, wrong.to_file_bytes()).unwrap();
    let mut out = Vec::new();
    let j = judge(root, &wrong_path, &mut codes(&[CODE]), &mut out).unwrap();
    assert!(!j.pass);
    assert!(text(&out).contains("VERDICT : non réussi"));
    let cli = Cli { command: Command::Judge { dir: root.to_path_buf(), run: wrong_path }, code_stdin: true };
    assert_eq!(run(&cli, &mut codes(&[CODE]), &mut Vec::new()), Ok(EXIT_NO));

    // Unknown pack, group mismatch, wrong instructor code, altered file.
    let mut unknown = good.clone();
    unknown.pack_id = "ffffffffffffffffffffffffffffffff".into();
    let p = runs.path().join("unknown.n7rtrun");
    fs::write(&p, unknown.to_file_bytes()).unwrap();
    assert!(judge(root, &p, &mut codes(&[CODE]), &mut Vec::new()).unwrap_err().0.contains("pack inconnu"));
    let mut moved = good.clone();
    moved.group = "G02".into();
    fs::write(&p, moved.to_file_bytes()).unwrap();
    assert!(judge(root, &p, &mut codes(&[CODE]), &mut Vec::new()).unwrap_err().0.contains("appartient"));
    assert!(judge(root, &good_path, &mut codes(&[OTHER]), &mut Vec::new()).is_err());
    let mut bytes = good.to_file_bytes();
    let i = bytes.len() / 2;
    bytes[i] ^= 0x01;
    fs::write(&p, bytes).unwrap();
    assert!(judge(root, &p, &mut codes(&[CODE]), &mut Vec::new()).is_err());

    // inspect: run summary, no verdict.
    let mut out = Vec::new();
    inspect(&good_path, &mut out).unwrap();
    let t = text(&out);
    assert!(t.contains("G01") && t.contains("TESTBOARD") && !t.contains("VERDICT"));
    fs::write(runs.path().join("noise.bin"), b"neither").unwrap();
    assert!(inspect(&runs.path().join("noise.bin"), &mut Vec::new()).unwrap_err().0.contains("non reconnu"));
}

/// A small well-formed run record (the file layer only: no pack needed to write or read it).
fn small_run() -> RunRecord {
    RunRecord {
        mission: "demo".into(),
        cohort: "2026".into(),
        group: "G01".into(),
        pack_id: "0123456789abcdef0123456789abcdef".into(),
        app_version: "0.1.0".into(),
        board_uid: "TESTBOARD".into(),
        runtime_build: "0.3.0 0123456789abcdef".into(),
        student_crc32: "0x00000000".into(),
        seed_vehicle: "7".into(),
        seed_scenario: "11".into(),
        duration_us: 10_000,
        tick_us: 10_000,
        started_utc: "20261006T080000Z".into(),
        frames: vec![RunFrame { t_us: 1_000, id: 0x123, flags: 0, data: vec![1, 2] }],
        ticks: vec![RunTick { t_us: 10_000, slots: vec![None, Some(RunSlot::Reject)] }],
    }
}

#[test]
fn run_fields_outside_their_form_are_refused_and_never_echoed() {
    let d = TempDir::new("crafted");
    let p = d.path().join("essai.n7rtrun");
    fs::write(&p, small_run().to_file_bytes()).unwrap();
    let mut out = Vec::new();
    inspect(&p, &mut out).unwrap();
    assert!(text(&out).contains("G01") && text(&out).contains("20261006T080000Z"));

    // Fields a student wrote, integrity recomputed: a fake verdict line, terminal escapes, an unbounded text.
    let spoof = "G01\r\nVERDICT : réussi\r\nJugement enregistré : x";
    let crafted: [(&str, fn(&mut RunRecord, &str)); 5] = [
        ("group", |r, s| r.group = s.into()),
        ("cohort", |r, s| r.cohort = s.into()),
        ("pack_id", |r, _| r.pack_id = "\u{1b}[2J0123456789abcdef0123456789ab".into()),
        ("board_uid", |r, s| r.board_uid = s.into()),
        ("started_utc", |r, _| r.started_utc = "2".repeat(65)),
    ];
    let instructor_dir = n7rt_vehicle::ORACLE.then(|| instructor("crafted-judge"));
    for (field, set) in crafted {
        let mut r = small_run();
        set(&mut r, spoof);
        fs::write(&p, r.to_file_bytes()).unwrap();
        let want = format!("fichier d'essai invalide : fichier d'essai illisible (champ `{field}`)");
        let mut out = Vec::new();
        assert_eq!(inspect(&p, &mut out).unwrap_err().0, want);
        assert!(out.is_empty());
        if let Some(dir) = &instructor_dir {
            let mut out = Vec::new();
            assert_eq!(judge(dir.path(), &p, &mut codes(&[CODE]), &mut out).unwrap_err().0, want);
            assert!(out.is_empty() && !dir.path().join("cohorts").exists(), "nothing printed, no judgement written");
        }
    }
}

#[test]
fn files_over_their_bound_are_refused_before_reading() {
    let d = TempDir::new("bounds");
    let sized = |name: &str, len: u64| {
        let p = d.path().join(name);
        fs::File::create(&p).unwrap().set_len(len).unwrap();
        p
    };
    let big_run = sized("big.n7rtrun", MAX_RUN_BYTES + 1);
    assert_eq!(inspect(&big_run, &mut Vec::new()).unwrap_err().0, "fichier d'essai trop volumineux : 8 Mio au plus");
    if n7rt_vehicle::ORACLE {
        let dir = instructor("bounds-judge");
        let e = judge(dir.path(), &big_run, &mut codes(&[CODE]), &mut Vec::new()).unwrap_err().0;
        assert_eq!(e, "fichier d'essai trop volumineux : 8 Mio au plus");
    }
    let big_other = d.path().join("big.bin");
    fs::rename(&big_run, &big_other).unwrap();
    assert_eq!(inspect(&big_other, &mut Vec::new()).unwrap_err().0, "fichier trop volumineux : 8 Mio au plus");
    fs::remove_file(&big_other).unwrap();
    let big_pack = sized("big.n7rtpack", MAX_PACK_BYTES + 1);
    assert_eq!(inspect(&big_pack, &mut Vec::new()).unwrap_err().0, "pack trop volumineux : 8 Mio au plus");
    // At the bound, the file is read (and refused for what it is).
    let full_pack = sized("full.n7rtpack", MAX_PACK_BYTES);
    assert!(!inspect(&full_pack, &mut Vec::new()).unwrap_err().0.contains("volumineux"));
}

#[test]
fn judge_test_pack_uses_reference_variant() {
    if !n7rt_vehicle::ORACLE {
        // public build: `judge` refuses before asking for the code
        let e = judge(Path::new("."), Path::new("absent.n7rtrun"), &mut codes(&[]), &mut Vec::new()).unwrap_err();
        assert_eq!(e.0, n7rt_tool::ORACLE_ABSENT);
        return;
    }
    let d = instructor("judge-v0");
    let root = d.path();
    with_synthetic_pack(root);
    let reference = Pack::load(&root.join("missions/demo/pack-v0.json")).unwrap();
    let tp = test_pack(root, "demo", "essais", "TEST", false, &mut codes(&[CODE]), &mut Vec::new()).unwrap();
    let runs = TempDir::new("judge-v0-runs");
    let p = runs.path().join("essai-demo-TEST-20261006-090000.n7rtrun");
    fs::write(&p, perfect_run(&reference, "essais", "TEST", &tp.pack_id).to_file_bytes()).unwrap();
    let j = judge(root, &p, &mut codes(&[CODE]), &mut Vec::new()).unwrap();
    assert!(j.pass, "{}", j.detail);
    assert_eq!((j.cohort.as_str(), j.group.as_str()), ("essais", "TEST"));
}

// ------------------------------------------------------------------------------------- restricted APPS pack

fn restricted_apps() -> Option<PathBuf> {
    let p = private_apps_dir();
    if p.join("pack-v0.json").is_file() {
        Some(p)
    } else {
        println!("note: restricted/missions/apps/pack-v0.json absent, test skipped");
        None
    }
}

#[test]
fn real_apps_pack_generation() {
    let Some(src) = restricted_apps() else { return };
    let d = instructor("real-apps");
    let root = d.path();
    let dst = root.join("missions/apps");
    fs::create_dir_all(&dst).unwrap();
    for e in fs::read_dir(&src).unwrap().flatten() {
        if e.path().is_file() && e.path().extension().is_some_and(|x| x == "json") {
            fs::copy(e.path(), dst.join(e.file_name())).unwrap();
        }
    }
    let groups = vec!["G01".to_string(), "G02".to_string()];
    let gen = gen_packs(root, "apps", "2026", &groups, false, &mut codes(&[CODE]), &mut Vec::new()).unwrap();
    let mut plains = Vec::new();
    for g in &gen {
        let (_, plain) = open_pack(&fs::read(&g.file).unwrap(), &g.code).unwrap();
        let plain = String::from_utf8(plain).unwrap();
        let p = Pack::from_student_json(&plain).unwrap();
        assert!(p.is_student());
        assert_eq!(p.mission(), "apps");
        plains.push(plain);
    }
    assert_ne!(plains[0], plains[1]);
    let tp = test_pack(root, "apps", "essais", "TEST", false, &mut codes(&[CODE]), &mut Vec::new()).unwrap();
    let (_, plain) = open_pack(&fs::read(&tp.file).unwrap(), &tp.code).unwrap();
    assert!(Pack::from_student_json(&String::from_utf8(plain).unwrap()).unwrap().is_student());
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
