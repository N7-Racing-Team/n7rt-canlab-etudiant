//! Private check of the APPS mission bench (docs/apps-mission-slice.md §5, §7). It reads, at run time,
//! `restricted/missions/apps/bench-v0.json` and the expectations `restricted/missions/apps/tools/bench-check.json`
//! (both private, written by `tools/bench_gen.py`), and is skipped with a note when they are absent. This file
//! holds no mission value: every number comes from those files, and failure messages name the quantity and the
//! case, never a value.

#[allow(unused_imports)]
#[path = "phys_kit.rs"]
mod kit;

use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use kit::*;
use n7rt_circuit::{persist, Circuit, CompId, Component, Interact, Kind, Value};
use n7rt_lab::phys::{CircuitProbe, Disclosure, EngineStats, OpState, PhysConfig, PhysCore, PhysLab, Reading};
use serde_json::Value as Json;

/// The private missions directory (parent of the APPS files), see `private_apps_dir`.
fn missions_dir() -> Option<PathBuf> {
    private_apps_dir().parent().map(Path::to_path_buf)
}

struct Private {
    bench: String,
    check: Option<Json>,
}

fn private_files() -> Option<Private> {
    let apps = missions_dir()?.join("apps");
    let bench = std::fs::read_to_string(apps.join("bench-v0.json")).ok()?;
    let check = std::fs::read_to_string(apps.join("tools").join("bench-check.json")).ok().map(|t| serde_json::from_str(&t).expect("bench-check.json is JSON"));
    Some(Private { bench, check })
}

fn num(v: &Json, what: &str) -> f64 {
    v.as_f64().unwrap_or_else(|| panic!("bench-check.json: {what} must be a number"))
}

fn param(c: &Component, name: &str) -> f64 {
    match c.params.get(name) {
        Some(Value::Num(x)) => *x,
        _ => panic!("bench: parameter {name} missing"),
    }
}

fn between(core: &PhysCore, comp: CompId, a: &str, b: &str) -> f64 {
    valid(core.frame().voltage_between_pins(&pin(comp, a), &pin(comp, b)))
}

fn within(what: &str, got: f64, expected: f64, tol: f64) {
    // No value in the message: the deviation in units of the tolerance only.
    assert!((got - expected).abs() <= tol, "{what}: outside its tolerance ({:.1} tolerances away)", (got - expected).abs() / tol);
}

#[test]
fn private_apps_bench_follows_the_spec() {
    let Some(p) = private_files() else {
        eprintln!("note: restricted/missions/apps/bench-v0.json absent: private APPS bench check skipped");
        return;
    };
    let circuit: Circuit = match persist::load(&p.bench) {
        Ok(c) => c,
        Err(v) => panic!("bench-v0.json refused: {:?}", v.iter().map(|x| x.code.as_str()).collect::<Vec<_>>()),
    };
    assert_eq!(persist::save(&circuit), p.bench, "bench-v0.json is not in the canonical form of persist::save");
    let Some(check) = p.check else {
        eprintln!("note: restricted/missions/apps/tools/bench-check.json absent: expectations skipped");
        return;
    };
    let label = |k: &str| check[k].as_str().unwrap_or_else(|| panic!("bench-check.json: {k}"));
    let (ecu, sensor, supply) = (by_label(&circuit, label("ecu")), by_label(&circuit, label("sensor")), by_label(&circuit, label("supply")));
    let comps = circuit.components();
    assert_eq!((comps[&ecu].kind, comps[&sensor].kind, comps[&supply].kind), (Kind::EcuApps, Kind::Mlx90377, Kind::DcSupply));
    for c in comps.values().filter(|c| c.kind == Kind::Switch) {
        assert_eq!(circuit.state(c.id).and_then(|s| s.get("closed")), Some(&Value::Bool(true)), "{} closed", c.label);
    }
    let e = comps[&ecu].clone();
    let s = comps[&sensor].clone();
    let sig_ratio = param(&e, "sig_r_low") / (param(&e, "sig_r_high") + param(&e, "sig_r_low"));
    let sup_ratio = param(&e, "sup_r_low") / (param(&e, "sup_r_high") + param(&e, "sup_r_low"));
    let tf = |die: u8, theta: f64| {
        let p = |k: &str| param(&s, &format!("tf{die}_{k}"));
        (p("offset") + p("slope") * theta).max(p("clamp_low")).min(p("clamp_high"))
    };
    let settle = check["settle_frames"].as_u64().expect("settle_frames");

    let mut core = PhysCore::new(circuit.clone());
    run(&mut core);
    for (k, case) in check["travel"].as_array().expect("travel").iter().enumerate() {
        let position = num(&case["position"], "position");
        interact(&mut core, Interact::SetPot { comp: sensor, position });
        steps(&mut core, settle);
        let f = core.frame();
        assert_eq!(f.op_state(ecu), Some(OpState::Ecu { powered: true }), "case {k}: ECU out of reset");
        for rail in ["v5_1", "v5_2"] {
            within(&format!("case {k}: rail {rail}"), between(&core, ecu, rail, "gnd"), num(&check["rail"]["v"], "rail.v"), num(&check["rail"]["tol"], "rail.tol"));
        }
        let tol = num(&case["tol"], "tol");
        let theta = (position * 1000.0).round() / 10.0;
        for die in [1u8, 2] {
            let (out, vdd, vss) = (format!("out{die}"), format!("vdd{die}"), format!("vss{die}"));
            let v_out = between(&core, sensor, &out, &vss);
            within(&format!("case {k}: OUT{die}"), v_out, num(&case[out.as_str()], "out"), tol);
            // OUT = TF(θ)·V(VDD)/100, with the bench's own transfer function.
            let ratio = v_out / between(&core, sensor, &vdd, &vss);
            within(&format!("case {k}: OUT{die}/VDD{die}"), ratio, tf(die, theta) / 100.0, 1e-3 * tf(die, theta) / 100.0);
        }
        let code_tol = case["code_tol"].as_i64().expect("code_tol");
        for (ch, pin_a, ratio) in [("sig1", "sig_1", sig_ratio), ("sig2", "sig_2", sig_ratio), ("sup1", "v5_1", sup_ratio), ("sup2", "v5_2", sup_ratio)] {
            let smp = match f.adc(ecu, ch) {
                Reading::Valid { v } => v,
                other => panic!("case {k}: channel {ch} not valid ({})", match other {
                    Reading::Unavailable { reason } => reason,
                    _ => "invalid".to_string(),
                }),
            };
            assert_eq!(smp.code, (smp.volts * f64::from(1u32 << smp.bits) / smp.vref).floor() as u32, "case {k}: {ch} code law");
            within(&format!("case {k}: {ch} divider"), smp.volts, between(&core, ecu, pin_a, "gnd") * ratio, 1e-6 * smp.volts.abs().max(1e-3));
            let expected = case["codes"][ch].as_i64().expect("codes");
            assert!((i64::from(smp.code) - expected).abs() <= code_tol, "case {k}: {ch} code outside its tolerance");
        }
    }
    // Reset of the ECU with LV_IN (ch. 01 §1.3), with its hysteresis.
    for (k, step) in check["lv_sequence"].as_array().expect("lv_sequence").iter().enumerate() {
        let v = num(&step[0], "lv");
        let powered = step[1].as_bool().expect("lv_sequence powered");
        set_param(&mut core, supply, "voltage", Value::Num(v));
        steps(&mut core, 20);
        assert_eq!(core.frame().op_state(ecu), Some(OpState::Ecu { powered }), "LV step {k}: reset state");
        if !powered {
            assert!(matches!(core.frame().adc(ecu, "sig1"), Reading::Unavailable { .. }), "LV step {k}: ADC stopped in reset");
            assert!(between(&core, ecu, "v5_1", "gnd").abs() < 1e-3, "LV step {k}: rails off in reset");
        }
    }
    // Mission mode publishes no internal value.
    let v = core.values(1, Disclosure::Mission, EngineStats::default());
    assert!(v.nets.is_empty() && v.diagnostics.is_empty() && v.comps.iter().all(|c| c.pins.is_empty() && c.measures.is_empty()));

    // The path of the mission controller: load_circuit_json in mission mode, then the probe by designator.
    let lab = PhysLab::start(PhysConfig { data_dir: None });
    let snap = lab.load_circuit_json("banc-apps", &p.bench, Disclosure::Mission).expect("bench loaded by the handle");
    let json = serde_json::to_value(&snap).expect("snapshot JSON");
    for c in json["components"].as_array().expect("components") {
        if c["kind"] == "ecu_apps" || c["kind"] == "mlx90377" {
            assert_eq!(c["params"], serde_json::json!({}), "hidden parameters in the snapshot");
        }
    }
    let u1 = lab.find_designator(label("ecu")).expect("ECU by designator");
    let t0 = Instant::now();
    while !matches!(lab.probe().adc(u1, "sig1"), Reading::Valid { .. }) {
        assert!(t0.elapsed() < Duration::from_secs(5), "ECU channels sampled by the running bench");
        thread::sleep(Duration::from_millis(10));
    }
    assert!(lab.save("copie", false).is_err(), "a mission bench is never saved");
    lab.shutdown();
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
