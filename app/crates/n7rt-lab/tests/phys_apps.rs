//! APPS bench kinds (docs/apps-mission-slice.md §5) with the NEUTRAL catalog values only: `ecu_apps` (black box:
//! reset gate, limited sensor rails, SIG and rail dividers, ADC channels) and `mlx90377` (ratiometric dies with a
//! start-up gate, pedal travel). Hidden parameters never reach a DTO, are never edited, and a mission bench is
//! never saved. The mission values and their check live in restricted/missions/apps/ (private).

#[path = "phys_kit.rs"]
mod kit;

use std::fs;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use kit::*;
use n7rt_circuit::{persist, Circuit, CompId, Edit, Interact, Kind, RefusalCode, Value};
use n7rt_lab::phys::{
    replay, AdcSample, CircuitProbe, Disclosure, EngineStats, FileError, OpState, PhysCommand, PhysConfig, PhysCore, PhysError,
    PhysLab, Reading, RunState,
};

struct Apps {
    circuit: Circuit,
    v1: CompId,
    u1: CompId,
    u2: CompId,
    sw: CompId,
}

/// V1 (12 V) on LV_IN / GND, U1 ecu_apps, U2 mlx90377 on the six harness nets, SW1 closed in series on SIG_1
/// (OUT1 side on its own net), the bus model on CAN_H / CAN_L. `vdd1`: the VDD1 wire is present.
fn apps(position: f64, vdd1: bool) -> Apps {
    let mut b = Bench::new();
    let v1 = b.add(Kind::DcSupply, &[("voltage", Value::Num(12.0))]);
    let g = b.add(Kind::Ground, &[]);
    let u1 = b.add(Kind::EcuApps, &[]);
    let u2 = b.add(Kind::Mlx90377, &[]);
    let sw = b.add(Kind::Switch, &[]);
    let bus = b.add(Kind::CanBus, &[("remote_nodes", Value::Int(8))]);
    b.join("lv", &[(v1, "pos"), (u1, "lv_in")]);
    b.join("gnd", &[(v1, "neg"), (g, "gnd"), (u1, "gnd"), (bus, "gnd")]);
    if vdd1 {
        b.join("vdd1", &[(u1, "v5_1"), (u2, "vdd1")]);
    }
    b.join("out1", &[(u2, "out1"), (sw, "p1")]);
    b.join("sig1", &[(sw, "p2"), (u1, "sig_1")]);
    b.join("vss1", &[(u1, "gnd_s1"), (u2, "vss1")]);
    b.join("vdd2", &[(u1, "v5_2"), (u2, "vdd2")]);
    b.join("sig2", &[(u1, "sig_2"), (u2, "out2")]);
    b.join("vss2", &[(u1, "gnd_s2"), (u2, "vss2")]);
    b.join("canh", &[(u1, "can_h"), (bus, "canh")]);
    b.join("canl", &[(u1, "can_l"), (bus, "canl")]);
    b.interact(Interact::SetSwitch { comp: sw, closed: true });
    b.interact(Interact::SetPot { comp: u2, position });
    Apps { circuit: b.build(), v1, u1, u2, sw }
}

fn between(core: &PhysCore, comp: CompId, a: &str, b: &str) -> f64 {
    valid(core.frame().voltage_between_pins(&pin(comp, a), &pin(comp, b)))
}

fn adc(core: &PhysCore, comp: CompId, ch: &str) -> Reading<AdcSample> {
    core.frame().adc(comp, ch)
}

fn sample(r: Reading<AdcSample>) -> AdcSample {
    match r {
        Reading::Valid { v } => v,
        other => panic!("expected a Valid sample, got {other:?}"),
    }
}

/// Neutral transfer function of both dies: TF = 10 + 0.5·θ %VDD.
fn neutral_tf(theta: f64) -> f64 {
    10.0 + 0.5 * theta
}

fn stats() -> EngineStats {
    EngineStats::default()
}

#[test]
fn neutral_bench_rails_ratiometric_outputs_and_adc_channels() {
    let a = apps(0.3, true);
    let core = core_after(a.circuit.clone(), 200);
    let f = core.frame();
    assert_eq!(f.op_state(a.u1), Some(OpState::Ecu { powered: true }), "out of reset at 12 V");
    assert_eq!(f.op_state(a.u2), Some(OpState::Pot { position: 0.3 }));
    for (rail, gs) in [("v5_1", "gnd_s1"), ("v5_2", "gnd_s2")] {
        let v = between(&core, a.u1, rail, "gnd");
        assert!((v - 5.0).abs() < 2e-3, "{rail}: {v}");
        assert!(between(&core, a.u1, gs, "gnd").abs() < 1e-4, "{gs} tied to GND inside");
    }
    for (die, out, vdd, vss) in [(1, "out1", "vdd1", "vss1"), (2, "out2", "vdd2", "vss2")] {
        let u = between(&core, a.u2, vdd, vss);
        let o = between(&core, a.u2, out, vss);
        assert_close(&format!("OUT{die} = TF·VDD"), o, neutral_tf(30.0) / 100.0 * u, 1e-3);
    }
    // ADC channels: midpoints of the dividers (neutral 15 k / 10 k and 20 k / 10 k), 12 bits, 3.3 V.
    for (ch, pin_a, ratio) in [("sig1", "sig_1", 10.0 / 25.0), ("sig2", "sig_2", 10.0 / 25.0), ("sup1", "v5_1", 1.0 / 3.0), ("sup2", "v5_2", 1.0 / 3.0)] {
        let s = sample(adc(&core, a.u1, ch));
        assert_close(ch, s.volts, between(&core, a.u1, pin_a, "gnd") * ratio, 1e-6);
        assert_eq!((s.bits, s.vref), (12, 3.3));
        assert_eq!(s.code, (s.volts * 4096.0 / 3.3).floor() as u32, "{ch}: law of adc_input");
        assert_eq!(s.sampled_at_ns, f.sim_time_ns(), "{ch} sampled every frame");
    }
    assert_eq!(unavailable(adc(&core, a.u1, "a0")), "unknown_pin");
    // A black box publishes no power; neither does the sensor.
    assert_eq!(unavailable(f.power(a.u1)), "ic_model");
    assert_eq!(unavailable(f.power(a.u2)), "ic_model");
    // The bus side of the ECU is recessive.
    assert!(between(&core, a.u1, "can_h", "can_l").abs() < 1e-3);
    assert!(!codes(&core).iter().any(|c| c == "ic_unpowered" || c == "supply_current_limit"), "{:?}", codes(&core));
}

#[test]
fn the_pedal_travel_moves_both_outputs_in_run_without_restart() {
    let a = apps(0.0, true);
    let mut core = core_after(a.circuit.clone(), 50);
    let ratio = |core: &PhysCore, die: &str| between(core, a.u2, &format!("out{die}"), &format!("vss{die}")) / between(core, a.u2, &format!("vdd{die}"), &format!("vss{die}"));
    assert_close("rest", ratio(&core, "1"), neutral_tf(0.0) / 100.0, 1e-3);
    let t0 = core.frame().sim_time_ns();
    interact(&mut core, Interact::SetPot { comp: a.u2, position: 1.0 });
    steps(&mut core, 20);
    assert_eq!(core.frame().sim_time_ns(), t0 + 20_000_000, "the simulated time goes on");
    assert_eq!(core.run_state(), RunState::Run);
    assert_close("full travel, die 1", ratio(&core, "1"), neutral_tf(100.0) / 100.0, 1e-3);
    assert_close("full travel, die 2", ratio(&core, "2"), neutral_tf(100.0) / 100.0, 1e-3);
    let code_full = sample(adc(&core, a.u1, "sig1")).code;
    interact(&mut core, Interact::SetPot { comp: a.u2, position: 0.5 });
    steps(&mut core, 20);
    assert_close("half travel", ratio(&core, "1"), neutral_tf(50.0) / 100.0, 1e-3);
    assert!(sample(adc(&core, a.u1, "sig1")).code < code_full);
    // The journal replays to the same state.
    let replayed = replay(&a.circuit, core.journal(), core.frame_index());
    assert_eq!(replayed.digest(), core.digest());
}

#[test]
fn the_reset_gate_follows_lv_in_with_hysteresis() {
    let a = apps(0.3, true);
    let mut core = core_after(a.circuit.clone(), 20);
    let powered = |core: &PhysCore| core.frame().op_state(a.u1) == Some(OpState::Ecu { powered: true });
    assert!(powered(&core));
    let lv = |core: &mut PhysCore, v: f64| {
        set_param(core, a.v1, "voltage", Value::Num(v));
        steps(core, 10);
    };
    lv(&mut core, 6.2);
    assert!(powered(&core), "6.2 V: still out of reset (hysteresis)");
    lv(&mut core, 5.9);
    assert!(!powered(&core), "5.9 V: in reset");
    for ch in ["sig1", "sig2", "sup1", "sup2"] {
        assert_eq!(unavailable(adc(&core, a.u1, ch)), "ecu_unpowered");
    }
    assert!(between(&core, a.u1, "v5_1", "gnd").abs() < 1e-6, "rails off in reset");
    // The output capacitor and the ECU filter discharge into the SIG divider (slowest mode about 2.8 ms with the
    // neutral values).
    steps(&mut core, 30);
    let out1 = between(&core, a.u2, "out1", "vss1");
    assert!(out1.abs() < 1e-3, "sensor unpowered: OUT1 at {out1} V");
    assert!(has(&core, "ic_unpowered"));
    lv(&mut core, 6.3);
    assert!(!powered(&core), "6.3 V: still in reset");
    lv(&mut core, 6.6);
    assert!(powered(&core), "6.6 V: out of reset");
    assert!(matches!(adc(&core, a.u1, "sig1"), Reading::Valid { .. }));
}

#[test]
fn an_unpowered_die_leaves_its_output_to_the_pull_down_and_a_shorted_sig_saturates() {
    let a = apps(0.6, false);
    let core = core_after(a.circuit.clone(), 50);
    assert!(between(&core, a.u2, "out1", "vss1").abs() < 1e-3, "die 1 without VDD1: output in high impedance");
    let d = diag(&core, "ic_unpowered").expect("die 1 below its start threshold");
    assert!(d.message_fr.contains("puce 1"), "{}", d.message_fr);
    assert_eq!(sample(adc(&core, a.u1, "sig1")).code, 0);
    assert!(sample(adc(&core, a.u1, "sig2")).code > 0, "die 2 still on its own rail");

    // SIG_1 on LV (12 V) through the switch net: 12 V · 0.4 = 4.8 V at the midpoint, above 3.3 V.
    let mut b = Bench::new();
    let v1 = b.add(Kind::DcSupply, &[("voltage", Value::Num(12.0))]);
    let g = b.add(Kind::Ground, &[]);
    let u1 = b.add(Kind::EcuApps, &[]);
    b.join("lv", &[(v1, "pos"), (u1, "lv_in"), (u1, "sig_1")]);
    b.join("gnd", &[(v1, "neg"), (g, "gnd"), (u1, "gnd")]);
    let core = core_after(b.build(), 50);
    match adc(&core, u1, "sig1") {
        Reading::Saturated { v } => assert_eq!((v.code, v.bits), (4095, 12)),
        other => panic!("saturated sample expected, got {other:?}"),
    }
}

#[test]
fn hidden_parameters_are_refused_in_the_core_and_absent_from_every_dto() {
    let a = apps(0.3, true);
    let mut core = core_after(a.circuit.clone(), 5);
    for (comp, name) in [(a.u1, "r_ecu"), (a.u2, "tf1_slope")] {
        let base_rev = core_rev(&core);
        let r = core
            .apply(&PhysCommand::Edit { base_rev, edit: Edit::SetParam { comp, name: name.into(), value: Value::Num(1.0) } })
            .expect_err("hidden parameter");
        assert_eq!(r.code, RefusalCode::ParamHidden);
    }
    let names = ["r_ecu", "reset_on", "rail_i_lim", "sig_r_high", "sup_r_low", "sig_c", "tf1_offset", "tf2_clamp_low", "r_idd", "r_iso"];
    let check = |what: &str, json: String| {
        for n in names {
            assert!(!json.contains(&format!("\"{n}\"")), "{n} in {what}");
        }
    };
    check("components", serde_json::to_string(&core.document().circuit().components().values().collect::<Vec<_>>()).unwrap());
    check("values", serde_json::to_string(&core.values(1, Disclosure::Exploration, stats())).unwrap());
    let renamed = edit(&mut core, Edit::SetLabel { comp: a.u1, label: "U9".into() });
    assert_eq!(renamed.patch.components.len(), 1);
    check("edit reply", serde_json::to_string(&renamed).unwrap());
    let undone = core.apply(&PhysCommand::Undo).expect("undo");
    check("undo reply", serde_json::to_string(&undone).unwrap());
    let base_rev = core_rev(&core);
    let preview = core.preview(base_rev, &Edit::SetLabel { comp: a.u2, label: "U8".into() }).expect("preview");
    check("preview reply", serde_json::to_string(&preview).unwrap());
    // The core keeps them for the solver.
    assert!(core.document().circuit().components()[&a.u1].params.contains_key("r_ecu"));
}

#[test]
fn mission_values_show_only_what_the_parts_show() {
    let a = apps(0.42, true);
    let core = core_after(a.circuit.clone(), 20);
    let v = core.values(3, Disclosure::Mission, stats());
    assert!(v.nets.is_empty() && v.diagnostics.is_empty());
    for c in &v.comps {
        assert!(c.pins.is_empty() && c.measures.is_empty(), "{:?}", c.id);
    }
    let op = |id: CompId| v.comps.iter().find(|c| c.id == id).map(|c| c.op.clone());
    assert_eq!(op(a.u1), Some(OpState::Ecu { powered: true }));
    assert_eq!(op(a.u2), Some(OpState::Pot { position: 0.42 }));
    assert_eq!(op(a.sw), Some(OpState::Contact { closed: true }));
    // Exploration publishes the connector of the black box, never its inside.
    let e = core.values(4, Disclosure::Exploration, stats());
    let u1 = e.comps.iter().find(|c| c.id == a.u1).unwrap();
    let names: Vec<&str> = u1.measures.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, ["u_lv", "v_diff"]);
    assert_eq!(u1.pins.len(), 10);
}

// ------------------------------------------------------------------------------------------------- the handle

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("n7rt-phys-apps-{}-{tag}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).expect("temp dir");
    d
}

/// The neutral bench as a bench file, with distinctive (neutral) hidden values.
fn bench_json() -> String {
    let a = apps(0.25, true);
    let mut v: serde_json::Value = serde_json::from_str(&persist::save(&a.circuit)).unwrap();
    for c in v["components"].as_array_mut().unwrap() {
        if c["kind"] == "ecu_apps" {
            c["params"]["r_ecu"] = serde_json::json!(777.0);
        }
        if c["kind"] == "mlx90377" {
            c["params"]["tf1_offset"] = serde_json::json!(12.5);
        }
    }
    persist::save(&persist::load(&serde_json::to_string(&v).unwrap()).expect("bench file valid"))
}

fn wait_until(what: &str, mut f: impl FnMut() -> bool) {
    let t0 = Instant::now();
    while !f() {
        assert!(t0.elapsed() < Duration::from_secs(5), "{what}");
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn load_circuit_json_replaces_the_document_in_mission_and_runs() {
    let lab = PhysLab::start(PhysConfig { data_dir: None });
    let before = lab.snapshot();
    let json = bench_json();
    let s = lab.load_circuit_json("banc-apps", &json, Disclosure::Mission).expect("bench loaded");
    assert_eq!((s.name.as_str(), s.run, s.disclosure), ("banc-apps", RunState::Run, Disclosure::Mission));
    assert!(s.doc_id > before.doc_id, "a new document identity");
    assert!(s.rev > before.rev);
    assert!(!s.can_undo && !s.can_redo);
    let text = serde_json::to_string(&s).unwrap();
    assert!(!text.contains("r_ecu") && !text.contains("777") && !text.contains("tf1_offset") && !text.contains("12.5"), "{text}");
    assert!(lab.journal_tail(10).is_empty(), "the journal of a mission bench is never shown");

    let u1 = lab.find_designator("U1").expect("ECU");
    let u2 = lab.find_designator("U2").expect("sensor");
    assert_ne!(u1, u2);
    assert_eq!(lab.find_designator("X9"), None);
    assert_eq!(lab.find_designator(""), None, "parts without designator are never found");
    // The thread runs the bench: the ECU samples its channels.
    wait_until("ECU channels sampled", || matches!(lab.probe().adc(u1, "sig1"), Reading::Valid { .. }));
    assert_eq!(lab.probe().op_state(u1), Some(OpState::Ecu { powered: true }));
    // The hidden values are the solver's: r_ecu = 777 Ω on 12 V.
    let p = lab.probe();
    let i = valid(p.pin_current(&pin(u1, "lv_in")));
    let u = valid(p.voltage_between_pins(&pin(u1, "lv_in"), &pin(u1, "gnd")));
    assert_close("LV current through the hidden R_ECU", u / i, 777.0, 1e-6);

    let v = lab.values(0).expect("values");
    assert_eq!((v.doc_id, v.disclosure), (s.doc_id, Disclosure::Mission));
    assert!(v.nets.is_empty() && v.diagnostics.is_empty());
    let vt = serde_json::to_string(&v).unwrap();
    assert!(!vt.contains("777") && !vt.contains("r_ecu"));
    // Pedal travel from the core's caller (mission controller) or the UI.
    lab.command(PhysCommand::Interact { interact: Interact::SetPot { comp: u2, position: 0.8 } }).expect("travel");

    let broken = json.replace("\"r_ecu\": 777.0", "\"r_ecu\": -1.0");
    match lab.load_circuit_json("x", &broken, Disclosure::Mission) {
        Err(PhysError::File { error: FileError::Invalid { violations } }) => assert!(!violations.is_empty()),
        other => panic!("invalid bench refused, got {other:?}"),
    }
    assert_eq!(lab.find_designator("U1"), Some(u1), "nothing loaded partially");
    lab.shutdown();
}

#[test]
fn a_mission_bench_is_never_saved() {
    let dir = temp_dir("save");
    let lab = PhysLab::start(PhysConfig { data_dir: Some(dir.clone()) });
    let json = bench_json();
    let locked = |r: Result<_, PhysError>| match r {
        Err(PhysError::Refused { refusal }) => {
            assert_eq!(refusal.code, RefusalCode::MissionLocked);
            assert_eq!(refusal.message_fr, "Enregistrement indisponible en mission.");
        }
        other => panic!("save refused expected, got {other:?}"),
    };
    lab.load_circuit_json("banc", &json, Disclosure::Mission).expect("bench");
    locked(lab.save("copie", false));
    // Even once the disclosure is relaxed: the bench stays sealed until another document replaces it.
    lab.set_disclosure(Disclosure::Exploration);
    locked(lab.save("copie", false));
    lab.load_circuit_json("banc", &json, Disclosure::Exploration).expect("bench in exploration");
    locked(lab.save("copie", false));
    assert!(!dir.join("circuits").join("copie.json").exists());
    // A new circuit leaves the mission: exploration, saving allowed.
    lab.load_circuit_json("banc", &json, Disclosure::Mission).expect("bench");
    let s = lab.new_circuit();
    assert_eq!(s.disclosure, Disclosure::Exploration);
    assert_eq!(lab.values(0).expect("values").disclosure, Disclosure::Exploration);
    lab.save("vide", false).expect("saved");
    // So does a demonstration or a saved file; version-1 files load.
    lab.load_circuit_json("banc", &json, Disclosure::Mission).expect("bench");
    lab.load_demo("ref-pot-rc").expect("demo");
    lab.save("demo", false).expect("saved");
    let v1 = fs::read_to_string(dir.join("circuits").join("demo.json")).unwrap().replace("\"library_version\": 3", "\"library_version\": 1");
    fs::write(dir.join("circuits").join("ancien.json"), v1).unwrap();
    lab.load_circuit_json("banc", &json, Disclosure::Mission).expect("bench");
    let s = lab.load("ancien").expect("a version-1 file loads");
    assert_eq!(s.disclosure, Disclosure::Exploration);
    lab.save("ancien", true).expect("saved again, as version 3");
    assert!(fs::read_to_string(dir.join("circuits").join("ancien.json")).unwrap().contains("\"library_version\": 3"));
    lab.shutdown();
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_mission_bench_never_reaches_the_session_files() {
    let dir = temp_dir("spill");
    let lab = PhysLab::start(PhysConfig { data_dir: Some(dir.clone()) });
    lab.load_circuit_json("banc", &bench_json(), Disclosure::Mission).expect("bench");
    for _ in 0..10_001 {
        lab.command(PhysCommand::Stop).expect("stop");
    }
    assert!(!dir.join("sessions").exists(), "no session file with the bench as its initial circuit");
    let kept = lab.journal_tail(usize::MAX).len();
    assert!(kept <= 10_000, "the oldest entries are dropped: {kept}");
    lab.shutdown();
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn the_check_lock_makes_the_bench_read_only_and_hides_the_driven_travel() {
    let lab = PhysLab::start(PhysConfig { data_dir: None });
    let s0 = lab.load_circuit_json("banc", &bench_json(), Disclosure::Mission).expect("bench");
    let u2 = lab.find_designator("U2").expect("sensor");
    let travel = |v: &n7rt_lab::phys::ValuesDto| v.comps.iter().find(|c| c.id == u2).map(|c| (c.op.clone(), c.state["position"].clone()));
    // the multimeter on the sensor output: its reading is frozen with the states
    let (out, vss) = (n7rt_circuit::PinRef { comp: u2, pin: "out1".into() }, n7rt_circuit::PinRef { comp: u2, pin: "vss1".into() });
    lab.command(PhysCommand::Meter { com: Some(vss), vohm: Some(out), function: n7rt_lab::phys::MeterFn::VDc }).expect("meter placed");
    let _ = lab.command(PhysCommand::Run);
    wait_until("meter reading", || lab.values(0).is_some_and(|v| v.meter.display.ends_with(" V")));
    thread::sleep(Duration::from_millis(300));
    assert!(!lab.check_locked());
    lab.set_check_lock(true);
    assert!(lab.check_locked());
    let meter_at_lock = lab.values(0).expect("values").meter;
    assert!(meter_at_lock.display.ends_with(" V"), "{meter_at_lock:?}");
    let locked = |r: PhysError| match r {
        PhysError::Refused { refusal } => {
            assert_eq!(refusal.code, RefusalCode::BenchLocked);
            assert_eq!(refusal.message_fr, "Banc en lecture seule pendant l'essai.");
        }
        other => panic!("bench_locked expected, got {other:?}"),
    };
    // Every UI-side call is refused.
    locked(lab.command(PhysCommand::Interact { interact: Interact::SetPot { comp: u2, position: 0.5 } }).unwrap_err());
    locked(lab.command(PhysCommand::Stop).unwrap_err());
    locked(lab.command(PhysCommand::Undo).unwrap_err());
    locked(lab.preview(s0.rev, &Edit::SetLabel { comp: u2, label: "X".into() }).unwrap_err());
    locked(lab.load_demo("ref-pot-rc").unwrap_err());
    assert_eq!(lab.new_circuit().doc_id, s0.doc_id, "the bench stays");
    // The mission controller drives the travel; the probe reads it, the UI does not see it.
    lab.command_internal(PhysCommand::Interact { interact: Interact::SetPot { comp: u2, position: 0.9 } }).expect("driven");
    // In RUN the probe follows at the next frame.
    wait_until("travel seen by the probe", || lab.probe().op_state(u2) == Some(OpState::Pot { position: 0.9 }));
    thread::sleep(Duration::from_millis(300));
    let v = lab.values(0).expect("values");
    assert_eq!(travel(&v), Some((OpState::Pot { position: 0.25 }, Value::Num(0.25))), "frozen at the lock");
    assert_eq!(v.meter, meter_at_lock, "the multimeter does not follow the driven travel");
    let snap = lab.snapshot();
    assert_eq!(snap.states.iter().find(|(id, _)| *id == u2).map(|(_, s)| s["position"].clone()), Some(Value::Num(0.25)));
    assert!(lab.journal_tail(usize::MAX).is_empty());
    // Released: the UI sees the circuit again and may act on it.
    lab.set_check_lock(false);
    let v = lab.values(0).expect("values");
    assert_eq!(travel(&v), Some((OpState::Pot { position: 0.9 }, Value::Num(0.9))));
    wait_until("the multimeter follows again", || lab.values(0).is_some_and(|v| v.meter.display != meter_at_lock.display));
    lab.command(PhysCommand::Interact { interact: Interact::SetPot { comp: u2, position: 0.25 } }).expect("the student again");
    assert!(lab.journal_tail(usize::MAX).is_empty(), "still a mission bench");
    lab.shutdown();
}
