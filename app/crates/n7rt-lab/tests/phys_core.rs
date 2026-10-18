//! The clock-free core (contract §5.7, §6, §8.1, §8.2): invalid readings, mission-mode filter, RUN / STOP /
//! RESET, edits in RUN without restart, interaction coalescing and deferred momentary release, ADC sampling,
//! and the replay property (a): 200 random edits and 500 interactions over 60 s of RUN, interleaved with undo /
//! redo, replay to the live digest.

#[path = "phys_kit.rs"]
mod kit;

use kit::*;
use n7rt_circuit::{Circuit, CompId, Edit, Interact, Kind, Pt, Rot, Value, ValueMap};
use n7rt_lab::phys::{
    replay, CircuitProbe, CommandReply, Disclosure, EngineStats, JournalEntry, LogicLevel, OpState, PhysCommand, PhysCore,
    Reading, RunReply, RunState, TxdState,
};

const H: f64 = 1e-3;

fn num(x: f64) -> Value {
    Value::Num(x)
}

fn v(core: &PhysCore, comp: CompId, p: &str) -> f64 {
    valid(core.frame().voltage_at(&pin(comp, p)))
}

fn run_reply(core: &mut PhysCore, c: PhysCommand) -> RunReply {
    match core.apply(&c) {
        Ok(CommandReply::Run(r)) => r,
        other => panic!("run reply expected, got {other:?}"),
    }
}

fn code(core: &PhysCore, comp: CompId, channel: &str) -> u32 {
    match core.frame().adc(comp, channel) {
        Reading::Valid { v } => v.code,
        other => panic!("ADC {channel}: {other:?}"),
    }
}

// ----------------------------------------------------------------------------------------------- readings

#[test]
fn no_reference_island_gives_differences_and_currents_only() {
    let mut b = Bench::new();
    let v1 = b.add(Kind::DcSupply, &[]);
    let r1 = b.add(Kind::Resistor, &[("resistance", num(1e3))]);
    b.join("p", &[(v1, "pos"), (r1, "p1")]);
    b.join("n", &[(v1, "neg"), (r1, "p2")]);
    let core = PhysCore::new(b.build());
    let f = core.frame();
    assert_eq!(f.voltage_at(&pin(r1, "p1")), Reading::NoReference);
    assert_eq!(f.voltage_at(&pin(r1, "p2")), Reading::NoReference);
    assert_close("V(R1)", valid(f.voltage_between_pins(&pin(r1, "p1"), &pin(r1, "p2"))), 5.0 * 1e3 / (1e3 + 0.01), 1e-12);
    assert_close("I(R1)", valid(f.pin_current(&pin(r1, "p1"))), 5.0 / (1e3 + 0.01), 1e-12);
    assert_close("P(V1) < 0: it delivers", valid(f.power(v1)), -25.0 * 1e3 / ((1e3 + 0.01) * (1e3 + 0.01)), 1e-9);
}

#[test]
fn ideal_inputs_float() {
    let mut b = Bench::new();
    let adc = b.add(Kind::AdcInput, &[]);
    let g = b.add(Kind::Ground, &[]);
    b.join("gnd", &[(g, "gnd"), (adc, "ref")]);
    let mut core = PhysCore::new(b.build());
    let f = core.frame();
    assert_eq!(f.voltage_at(&pin(adc, "in")), Reading::Floating, "never 0 V");
    assert_eq!(f.voltage_between_pins(&pin(adc, "in"), &pin(adc, "ref")), Reading::Floating);
    assert_eq!(f.adc(adc, "in"), Reading::Floating, "the STOP preview samples a floating input: no number");
    run(&mut core);
    core.step();
    assert_eq!(core.frame().adc(adc, "in"), Reading::Floating, "a floating sample, not a number");
}

#[test]
fn txd_between_thresholds_withholds_the_bus() {
    // TXD pulled to VSS by 10 kΩ against its 25 kΩ pull-up: 5·10/35 = 1.43 V, between VIL 0.8 V and VIH 2.0 V.
    let mut b = Bench::new();
    let v1 = b.add(Kind::DcSupply, &[]);
    let g = b.add(Kind::Ground, &[]);
    let u1 = b.add(Kind::CanTransceiver, &[]);
    let r1 = b.add(Kind::Resistor, &[]);
    b.join("vdd", &[(v1, "pos"), (u1, "vdd")]);
    b.join("gnd", &[(g, "gnd"), (v1, "neg"), (u1, "vss"), (u1, "rs"), (r1, "p2")]);
    b.join("tx", &[(u1, "txd"), (r1, "p1")]);
    let core = PhysCore::new(b.build());
    let f = core.frame();
    // VDD = 5 V behind r_out = 10 mΩ, loaded by R_DD (500 Ω) and the TXD divider (35 kΩ).
    let vdd = 5.0 / (1.0 + 0.01 * (1.0 / 500.0 + 1.0 / 3.5e4));
    assert_close("TXD", v(&core, u1, "txd"), vdd * 1e4 / 3.5e4, 1e-9);
    assert_eq!(f.logic(&pin(u1, "txd")), Reading::Undefined);
    assert!(matches!(f.op_state(u1), Some(OpState::Transceiver { powered: true, txd: TxdState::DominantRequested, .. })));
    for p in ["canh", "canl", "rxd"] {
        assert_eq!(unavailable(f.voltage_at(&pin(u1, p))), "txd_undefined", "{p}");
    }
    assert_eq!(unavailable(f.logic(&pin(u1, "rxd"))), "txd_undefined");
    assert!(has(&core, "logic_undefined") && has(&core, "dominant_requested"), "{:?}", codes(&core));
}

#[test]
fn dominant_request_withholds_pd0_too() {
    // ECU U1 (PD1 high impedance) reads RXD on PD0; TXD of U2 forced low by S1.
    let mut b = Bench::new();
    let u1 = b.add(Kind::EcuStm32, &[("can_mode", Value::Choice("hi_z".into()))]);
    let g = b.add(Kind::Ground, &[]);
    let u2 = b.add(Kind::CanTransceiver, &[]);
    let s1 = b.add(Kind::Switch, &[]);
    let rt = b.add(Kind::Termination, &[]);
    b.join("gnd", &[(g, "gnd"), (u1, "gnd"), (u2, "vss"), (u2, "rs"), (s1, "p2")]);
    b.join("5v", &[(u1, "v5"), (u2, "vdd")]);
    b.join("rx", &[(u2, "rxd"), (u1, "can_rx")]);
    b.join("tx", &[(u2, "txd"), (s1, "p1")]);
    b.join("h", &[(u2, "canh"), (rt, "p1")]);
    b.join("l", &[(u2, "canl"), (rt, "p2")]);
    b.interact(Interact::SetSwitch { comp: s1, closed: true });
    let mut core = PhysCore::new(b.build());
    let f = core.frame();
    assert_eq!(unavailable(f.logic(&pin(u1, "can_rx"))), "dominant_non_modelise_v1");
    assert_eq!(unavailable(f.voltage_at(&pin(u1, "can_rx"))), "dominant_non_modelise_v1");
    assert_eq!(unavailable(f.voltage_between_pins(&pin(u2, "canh"), &pin(u2, "canl"))), "dominant_non_modelise_v1");
    assert_eq!(f.logic(&pin(u2, "txd")), Reading::Valid { v: LogicLevel::Low });
    // Released: recessive, RXD high, PD0 high.
    interact(&mut core, Interact::SetSwitch { comp: s1, closed: false });
    let f = core.frame();
    assert_eq!(f.logic(&pin(u1, "can_rx")), Reading::Valid { v: LogicLevel::High });
    assert_eq!(f.logic(&pin(u2, "rxd")), Reading::Valid { v: LogicLevel::High });
    assert!(valid(f.voltage_between_pins(&pin(u2, "canh"), &pin(u2, "canl"))).abs() < 1e-9);
}

#[test]
fn saturated_samples_are_clamped() {
    for (pos, code) in [("pos", 4095), ("neg", 0)] {
        // ADC1 (3.3 V, 12 bits) on +5 V, or on −5 V with the supply reversed.
        let mut b = Bench::new();
        let v1 = b.add(Kind::DcSupply, &[]);
        let g = b.add(Kind::Ground, &[]);
        let adc = b.add(Kind::AdcInput, &[]);
        let other = if pos == "pos" { "neg" } else { "pos" };
        b.join("gnd", &[(g, "gnd"), (v1, other), (adc, "ref")]);
        b.join("in", &[(v1, pos), (adc, "in")]);
        let core = core_after(b.build(), 1);
        match core.frame().adc(adc, "in") {
            Reading::Saturated { v } => {
                assert_eq!(v.code, code);
                assert_close("sampled volts", v.volts, if code == 0 { -5.0 } else { 5.0 }, 1e-12);
                assert_eq!((v.bits, v.vref, v.sampled_at_ns), (12, 3.3, 1_000_000));
            }
            other => panic!("{other:?}"),
        }
    }
}

#[test]
fn unavailable_reasons() {
    let c = tp1_capteur_can();
    let (u1, u2, bus1, r1) = (by_label(&c, "U1"), by_label(&c, "U2"), by_label(&c, "BUS1"), by_label(&c, "R1"));
    let mut core = core_after(c, 3);
    let f = core.frame();
    for comp in [u1, u2, bus1] {
        assert_eq!(unavailable(f.power(comp)), "ic_model");
    }
    assert!(matches!(f.power(r1), Reading::Valid { .. }));
    assert_eq!(unavailable(f.voltage_at(&pin(u1, "no_such_pin"))), "unknown_pin");
    assert_eq!(unavailable(f.voltage_at(&pin(CompId(999), "p1"))), "unknown_pin");
    assert_eq!(unavailable(f.adc(u1, "a2")), "unknown_pin");
    assert_eq!(unavailable(f.logic(&pin(r1, "p1"))), "not_logic_pin");
    // Internal nodes (bias, rbias) are never addressable: only library pins.
    assert_eq!(unavailable(f.voltage_at(&pin(u2, "bias"))), "unknown_pin");

    // A deleted component: unknown pin, no net.
    stop(&mut core);
    edit(&mut core, Edit::Delete { components: vec![r1], wires: vec![] });
    let f = core.frame();
    assert_eq!(unavailable(f.voltage_at(&pin(r1, "p1"))), "unknown_pin");
    assert_eq!(f.net_of(&pin(r1, "p1")), None);
    assert!(f.net_of(&pin(u1, "gnd")).is_some());

    // ECU unpowered: no rails, no ADC, no logic on PD0.
    interact(&mut core, Interact::SetPower { comp: u1, on: false });
    let f = core.frame();
    assert_eq!(unavailable(f.adc(u1, "a0")), "ecu_unpowered");
    assert_eq!(unavailable(f.logic(&pin(u1, "can_rx"))), "ecu_unpowered");
    assert_eq!(f.op_state(u1), Some(OpState::Ecu { powered: false }));
    assert!(matches!(f.op_state(u2), Some(OpState::Transceiver { powered: false, .. })));
}

#[test]
fn pin_currents_of_every_component_sum_to_zero() {
    let core = core_after(tp1_capteur_can(), 200);
    let f = core.frame();
    for (id, c) in core.document().circuit().components() {
        let def = n7rt_circuit::kind_def(c.kind);
        let currents: Vec<f64> = def.pins.iter().map(|p| valid(f.pin_current(&pin(*id, p.id)))).collect();
        let sum: f64 = currents.iter().sum();
        let scale = currents.iter().fold(0.0f64, |m, i| m.max(i.abs()));
        // Rounding: relative to the largest current, plus the current resolution of the 1 mΩ bias source of
        // the transceiver, ε·V/r_out ≈ 1e-12 A.
        assert!(sum.abs() <= 1e-12 * scale + 1e-11, "{} {:?}: Σ = {sum} over {currents:?}", c.label, c.kind);
    }
}

#[test]
fn unpowered_resistance_open_zero_and_value() {
    let c = tp1_capteur_can();
    let (u1, u2, r1, rv1) = (by_label(&c, "U1"), by_label(&c, "U2"), by_label(&c, "R1"), by_label(&c, "RV1"));
    let core = PhysCore::new(c);
    let f = core.frame();
    assert_eq!(valid(f.unpowered_resistance(&pin(u1, "gnd"), &pin(u2, "vss"))), 0.0, "same net");
    // Switched off, C1 and the sources are gone: X hangs on R1 only, W reaches GND through R_AW only (the 3V3 rail
    // is a dead end), VDD reaches VSS through R_DD only (the TXD pull-up ends on PD1, a removed source).
    assert_close("R(R1.1, R1.2)", valid(f.unpowered_resistance(&pin(r1, "p1"), &pin(r1, "p2"))), 1e4, 1e-9);
    assert_close("R(W, GND)", valid(f.unpowered_resistance(&pin(rv1, "w"), &pin(u1, "gnd"))), 3e3, 1e-9);
    assert_close("R(VDD, VSS)", valid(f.unpowered_resistance(&pin(u2, "vdd"), &pin(u2, "vss"))), 500.0, 1e-9);
    assert_eq!(f.unpowered_resistance(&pin(u2, "canh"), &pin(u1, "gnd")), Reading::Open);
    assert_eq!(unavailable(f.unpowered_resistance(&pin(u2, "canh"), &pin(CompId(999), "p1"))), "unknown_pin");
}

// ------------------------------------------------------------------------------------------ mission mode

#[test]
fn mission_mode_withholds_omniscient_values() {
    let c = tp1_capteur_can();
    let (u1, u2, rv1) = (by_label(&c, "U1"), by_label(&c, "U2"), by_label(&c, "RV1"));
    let core = core_after(c, 10);
    let open = core.values(5, Disclosure::Exploration, EngineStats::default());
    assert!(!open.nets.is_empty());
    assert!(open.nets.iter().all(|n| matches!(n.voltage, Reading::Valid { .. })), "tp1 is fully referenced");
    assert!(open.comps.iter().all(|c| !c.pins.is_empty()), "every kind has pins");
    assert!(open.comps.iter().any(|c| !c.measures.is_empty()));
    assert_eq!(open.comps.iter().find(|c| c.id == u2).map(|c| c.op.clone()), core.frame().op_state(u2));

    let hidden = core.values(6, Disclosure::Mission, EngineStats::default());
    assert_eq!((hidden.seq, hidden.disclosure), (6, Disclosure::Mission));
    assert!(hidden.nets.is_empty() && hidden.diagnostics.is_empty());
    assert_eq!(hidden.comps.len(), open.comps.len());
    for c in &hidden.comps {
        assert!(c.pins.is_empty() && c.measures.is_empty(), "{:?}", c.id);
    }
    let op = |id: CompId| hidden.comps.iter().find(|c| c.id == id).map(|c| c.op.clone());
    assert_eq!(op(u2), Some(OpState::Passive), "the transceiver shows nothing");
    assert_eq!(op(u1), Some(OpState::Ecu { powered: true }), "the board LED");
    assert_eq!(op(rv1), Some(OpState::Pot { position: 0.3 }), "the wiper");
    let state = |v: &n7rt_lab::phys::ValuesDto, id: CompId| v.comps.iter().find(|c| c.id == id).map(|c| c.state.clone());
    assert_eq!(state(&hidden, rv1), state(&open, rv1), "interaction states stay");
}

// ------------------------------------------------------------------------------------- RUN / STOP / RESET

#[test]
fn run_stop_reset() {
    let c = ref_pot_rc();
    let (rv1, c1, adc1) = (by_label(&c, "RV1"), by_label(&c, "C1"), by_label(&c, "ADC1"));
    let mut core = PhysCore::new(c);
    assert_eq!(core.run_state(), RunState::Stop);
    core.step();
    assert_eq!((core.frame_index(), core.frame().sim_time_ns()), (0, 0), "nothing in STOP");
    assert!(core.values(1, Disclosure::Exploration, EngineStats::default()).preview);

    assert_eq!(run_reply(&mut core, PhysCommand::Run), RunReply { run: RunState::Run, sim_time_ns: 0 });
    steps(&mut core, 50);
    assert_eq!((core.frame_index(), core.frame().sim_time_ns()), (50, 50_000_000));
    let tau = 1e-5 * (1e4 + parallel(3e3, 7e3 + 0.02));
    let v_inf = 5.0 * 3e3 / (1e4 + 0.02);
    let vx = v(&core, c1, "p1");
    assert_close("V(X) after 50 frames", vx, rc_scheme(50, H / tau, 0.0, v_inf), 1e-9);
    assert!(!core.values(2, Disclosure::Exploration, EngineStats::default()).preview);

    // STOP freezes time and state; an interaction is previewed, the stored state untouched.
    assert_eq!(run_reply(&mut core, PhysCommand::Stop), RunReply { run: RunState::Stop, sim_time_ns: 50_000_000 });
    let digest = core.digest();
    core.step();
    assert_eq!(core.digest(), digest);
    interact(&mut core, Interact::SetPot { comp: rv1, position: 0.7 });
    let f = core.frame();
    assert_eq!(f.sim_time_ns(), 50_000_000);
    assert!(core.values(3, Disclosure::Exploration, EngineStats::default()).preview);
    assert_close("C1 held during the 1 ns preview", v(&core, c1, "p1"), vx, 1e-4);

    // RESET in STOP: t = 0, C1 discharged, the wiper stays at 0.7, still STOP.
    assert_eq!(run_reply(&mut core, PhysCommand::Reset), RunReply { run: RunState::Stop, sim_time_ns: 0 });
    assert_eq!((core.run_state(), core.frame_index()), (RunState::Stop, 0));
    assert!(v(&core, c1, "p1").abs() < 1e-6, "discharged");
    let (r_aw, r_wb) = (7e3, 3e3);
    let w0 = 5.0 * parallel(r_aw, 1e4) / (parallel(r_aw, 1e4) + r_wb + 0.02);
    assert_close("V(W) at 0+ with α = 0.7", v(&core, rv1, "w"), w0, 1e-7);
    // The preview samples at 0+ (t = 0), C1 empty.
    match core.frame().adc(adc1, "in") {
        Reading::Valid { v: s } => {
            assert_eq!((s.sampled_at_ns, s.code), (0, (w0 * 4096.0 / 5.0).floor() as u32));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(core.frame().op_state(rv1), Some(OpState::Pot { position: 0.7 }));

    // RESET in RUN: stays RUN, starts again from t = 0 with C1 discharged.
    run(&mut core);
    steps(&mut core, 10);
    assert_eq!(run_reply(&mut core, PhysCommand::Reset), RunReply { run: RunState::Run, sim_time_ns: 0 });
    core.step();
    assert_eq!((core.frame_index(), core.frame().sim_time_ns()), (1, 1_000_000));
    let tau = 1e-5 * (1e4 + parallel(r_aw, r_wb + 0.02));
    assert_close("V(X) one frame after RESET", v(&core, c1, "p1"), rc_scheme(1, H / tau, 0.0, 5.0 * r_aw / (1e4 + 0.02)), 1e-9);
}

#[test]
fn electrical_edit_in_run_keeps_capacitor_and_time() {
    let c = ref_pot_rc();
    let (r1, c1) = (by_label(&c, "R1"), by_label(&c, "C1"));
    let mut core = core_after(c, 100);
    let vx = v(&core, c1, "p1");
    let reply = set_param(&mut core, r1, "resistance", num(22e3));
    assert!(reply.nets.is_none(), "a parameter does not change connectivity");
    let v_inf = 5.0 * 3e3 / (1e4 + 0.02);
    let tau = 1e-5 * (22e3 + parallel(3e3, 7e3 + 0.02));
    for n in 1..=20 {
        core.step();
        assert_close(&format!("V(X) {n} frames after R1 = 22 kΩ"), v(&core, c1, "p1"), rc_scheme(n, H / tau, vx, v_inf), 1e-9);
    }
    assert_eq!(core.frame().sim_time_ns(), 120_000_000, "no restart");
}

#[test]
fn geometry_edit_in_run_changes_no_value() {
    let c = ref_pot_rc();
    let labels = labels(&c);
    let (mut a, mut b) = (core_after(c.clone(), 30), core_after(c, 30));
    let adc2 = labels["ADC2"];
    let reply = edit(&mut a, Edit::Move { components: vec![adc2], wires: vec![], delta: Pt { x: 2, y: 0 } });
    assert!(reply.nets.is_none() && !reply.patch.components.is_empty());
    let reply = edit(&mut a, Edit::SetLabel { comp: labels["R1"], label: "RA".into() });
    assert!(reply.nets.is_some(), "net names follow the designator");
    steps(&mut a, 30);
    steps(&mut b, 30);
    let (fa, fb) = (a.frame(), b.frame());
    for (id, comp) in a.document().circuit().components() {
        for p in n7rt_circuit::kind_def(comp.kind).pins {
            let q = pin(*id, p.id);
            assert_eq!(fa.voltage_at(&q), fb.voltage_at(&q), "{id:?}.{}", p.id);
            assert_eq!(fa.pin_current(&q), fb.pin_current(&q), "{id:?}.{}", p.id);
        }
    }
    assert_eq!(fa.adc(adc2, "in"), fb.adc(adc2, "in"));
    // The net of R1.2 is renamed at once, in RUN.
    let renamed = a.values(1, Disclosure::Exploration, EngineStats::default());
    assert!(renamed.nets.iter().any(|n| n.name.starts_with("RA.")), "{:?}", renamed.nets.iter().map(|n| &n.name).collect::<Vec<_>>());
}

// ---------------------------------------------------------------------------------------------- journal

fn journal_tail(core: &PhysCore, from: usize) -> Vec<JournalEntry> {
    core.journal()[from..].to_vec()
}

fn set_pot(comp: CompId, position: f64) -> PhysCommand {
    PhysCommand::Interact { interact: Interact::SetPot { comp, position } }
}

#[test]
fn set_pot_is_coalesced_per_component_and_frame() {
    let c = ref_pot_rc();
    let (s1, rv1) = (by_label(&c, "S1"), by_label(&c, "RV1"));
    let initial = c.clone();
    let mut core = PhysCore::new(c);
    run(&mut core);
    steps(&mut core, 5);
    let from = core.journal().len();
    for cmd in [
        set_pot(rv1, 0.4),
        PhysCommand::Interact { interact: Interact::SetSwitch { comp: s1, closed: false } },
        set_pot(rv1, 0.5),
        set_pot(rv1, 0.6),
    ] {
        core.apply(&cmd).expect("interaction");
    }
    assert_eq!(
        journal_tail(&core, from),
        vec![
            JournalEntry { frame: 5, command: PhysCommand::Interact { interact: Interact::SetSwitch { comp: s1, closed: false } } },
            JournalEntry { frame: 5, command: set_pot(rv1, 0.6) },
        ]
    );
    core.step();
    core.apply(&set_pot(rv1, 0.7)).expect("pot");
    assert_eq!(core.journal().last(), Some(&JournalEntry { frame: 6, command: set_pot(rv1, 0.7) }));
    assert_eq!(core.journal()[core.journal().len() - 2], JournalEntry { frame: 5, command: set_pot(rv1, 0.6) }, "another frame: kept");
    // Unchanged state: not journaled.
    let n = core.journal().len();
    core.apply(&set_pot(rv1, 0.7)).expect("pot");
    assert_eq!(core.journal().len(), n);
    steps(&mut core, 3);
    // Moved and back to its position within one frame: nothing happened in that frame, nothing journaled.
    let n = core.journal().len();
    core.apply(&set_pot(rv1, 0.2)).expect("pot");
    core.apply(&set_pot(rv1, 0.25)).expect("pot");
    assert_eq!(core.journal().len(), n + 1);
    core.apply(&set_pot(rv1, 0.7)).expect("pot");
    assert_eq!(core.journal().len(), n, "back to 0.7 within frame 9");
    core.step();

    // In STOP every change is previewed at once (solver reloaded, ADCs sampled): each one is journaled.
    stop(&mut core);
    let from = core.journal().len();
    core.apply(&set_pot(rv1, 0.1)).expect("pot");
    core.apply(&set_pot(rv1, 0.7)).expect("pot");
    core.apply(&set_pot(rv1, 0.4)).expect("pot");
    let tail: Vec<PhysCommand> = journal_tail(&core, from).into_iter().map(|e| e.command).collect();
    assert_eq!(tail, vec![set_pot(rv1, 0.1), set_pot(rv1, 0.7), set_pot(rv1, 0.4)]);
    // RUN then a move in the same frame: not merged with the STOP ones.
    run(&mut core);
    core.apply(&set_pot(rv1, 0.5)).expect("pot");
    assert_eq!(core.journal().len(), from + 5);
    steps(&mut core, 3);

    let replayed = replay(&initial, core.journal(), core.frame_index());
    assert_eq!(replayed.journal(), core.journal(), "the replay journals the same entries");
    assert_eq!(replayed.digest(), core.digest());
    let adcs: Vec<CompId> = ["ADC1", "ADC2"].iter().map(|l| by_label(core.document().circuit(), l)).collect();
    for adc in adcs {
        assert_eq!(replayed.frame().adc(adc, "in"), core.frame().adc(adc, "in"), "same held samples");
    }
}

/// V1 → BP1 (momentary, normally open) → R1 (1 kΩ) → ground.
fn button_circuit(mode: &str) -> (Circuit, CompId, CompId) {
    let mut b = Bench::new();
    let v1 = b.add(Kind::DcSupply, &[]);
    let g = b.add(Kind::Ground, &[]);
    let bp1 = b.add(Kind::PushButton, &[("mode", Value::Choice(mode.into()))]);
    let r1 = b.add(Kind::Resistor, &[("resistance", num(1e3))]);
    b.join("p", &[(v1, "pos"), (bp1, "p1")]);
    b.join("q", &[(bp1, "p2"), (r1, "p1")]);
    b.join("gnd", &[(g, "gnd"), (v1, "neg"), (r1, "p2")]);
    (b.build(), bp1, r1)
}

#[test]
fn momentary_release_is_deferred_after_the_frame_that_saw_the_press() {
    let (c, bp1, r1) = button_circuit("momentary");
    let initial = c.clone();
    let mut core = PhysCore::new(c);
    let pressed = |core: &PhysCore| core.document().circuit().state(bp1).expect("state")["pressed"] == Value::Bool(true);
    let i_r1 = |core: &PhysCore| valid(core.frame().pin_current(&pin(r1, "p1")));
    run(&mut core);
    steps(&mut core, 3);
    core.apply(&PhysCommand::Interact { interact: Interact::SetButton { comp: bp1, pressed: true } }).expect("press");
    let reply = core.apply(&PhysCommand::Interact { interact: Interact::SetButton { comp: bp1, pressed: false } }).expect("release");
    match reply {
        CommandReply::Interact(r) => assert_eq!(r.state["pressed"], Value::Bool(false), "the reply shows the release"),
        other => panic!("{other:?}"),
    }
    assert!(pressed(&core), "held until a frame sees the press");
    core.step();
    assert_close("the frame saw the contact closed", i_r1(&core), 5.0 / (1e3 + 0.1 + 0.01), 1e-12);
    assert!(!pressed(&core), "released after that frame");
    core.step();
    assert!(i_r1(&core) < 1e-7, "open again: leakage only");
    let tail: Vec<(u64, bool)> = core.journal()[1..]
        .iter()
        .map(|e| match &e.command {
            PhysCommand::Interact { interact: Interact::SetButton { pressed, .. } } => (e.frame, *pressed),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(tail, vec![(3, true), (4, false)], "the release is journaled where it takes effect");
    assert_eq!(replay(&initial, core.journal(), core.frame_index()).digest(), core.digest());

    // In STOP the release takes effect at once.
    stop(&mut core);
    interact(&mut core, Interact::SetButton { comp: bp1, pressed: true });
    assert_close("pressed (preview)", i_r1(&core), 5.0 / (1e3 + 0.1 + 0.01), 1e-9);
    interact(&mut core, Interact::SetButton { comp: bp1, pressed: false });
    assert!(!pressed(&core) && i_r1(&core) < 1e-7);
}

#[test]
fn latching_button_toggles_on_presses() {
    let (c, bp1, r1) = button_circuit("latching");
    let mut core = PhysCore::new(c);
    let closed = |core: &PhysCore| core.frame().op_state(bp1) == Some(OpState::Contact { closed: true });
    let press = |core: &mut PhysCore, p: bool| interact(core, Interact::SetButton { comp: bp1, pressed: p });
    assert!(!closed(&core));
    press(&mut core, true);
    assert!(closed(&core));
    press(&mut core, false);
    assert!(closed(&core), "latched");
    assert_close("I(R1) latched", valid(core.frame().pin_current(&pin(r1, "p1"))), 5.0 / (1e3 + 0.1 + 0.01), 1e-9);
    press(&mut core, true);
    press(&mut core, false);
    assert!(!closed(&core), "unlatched by the second press");
}

// ------------------------------------------------------------------------------------------------- ADC

#[test]
fn adc_samples_at_its_rate_and_holds() {
    let c = ref_pot_rc();
    let (rv1, adc1, adc2) = (by_label(&c, "RV1"), by_label(&c, "ADC1"), by_label(&c, "ADC2"));
    let mut core = PhysCore::new(c);
    set_param(&mut core, adc1, "sample_rate", Value::Int(100));
    // The STOP preview converts every channel once, whatever its rate, at the frozen instant.
    let at_stop = match core.frame().adc(adc1, "in") {
        Reading::Valid { v } => v,
        other => panic!("{other:?}"),
    };
    assert_eq!(at_stop.sampled_at_ns, 0);
    run(&mut core);
    for n in 1..=9 {
        core.step();
        assert_eq!(core.frame().adc(adc1, "in"), Reading::Valid { v: at_stop }, "held at frame {n} (100 Hz: frames 10, 20…)");
        match core.frame().adc(adc2, "in") {
            Reading::Valid { v } => assert_eq!(v.sampled_at_ns, n * 1_000_000, "1000 Hz: every frame"),
            other => panic!("{other:?}"),
        }
    }
    core.step();
    let first = match core.frame().adc(adc1, "in") {
        Reading::Valid { v } => v,
        other => panic!("{other:?}"),
    };
    assert_eq!(first.sampled_at_ns, 10_000_000);
    assert_close("sample = V(W) at the end of frame 10", first.volts, v(&core, rv1, "w"), 0.0);
    steps(&mut core, 2);
    interact(&mut core, Interact::SetPot { comp: rv1, position: 0.7 });
    for _ in 13..20 {
        core.step();
        assert_eq!(core.frame().adc(adc1, "in"), Reading::Valid { v: first }, "held");
    }
    core.step();
    assert!(code(&core, adc1, "in") > first.code + 1000, "new sample at frame 20");

    // RESET in RUN: the holds are emptied, nothing is previewed; not sampled until the first due frame.
    core.apply(&PhysCommand::Reset).expect("reset");
    for n in 1..=9 {
        core.step();
        assert_eq!(unavailable(core.frame().adc(adc1, "in")), "not_sampled_yet", "frame {n} after RESET");
        assert!(matches!(core.frame().adc(adc2, "in"), Reading::Valid { .. }));
    }
    core.step();
    assert!(matches!(core.frame().adc(adc1, "in"), Reading::Valid { v } if v.sampled_at_ns == 10_000_000));
}

#[test]
fn adc_reads_against_its_local_reference() {
    // ADC1.ref lifted to 1 V by V2, ADC1.in at 2.5 V by V1: the sample is V(in) − V(ref) = 1.5 V.
    let mut b = Bench::new();
    let v1 = b.add(Kind::DcSupply, &[("voltage", num(2.5))]);
    let v2 = b.add(Kind::DcSupply, &[("voltage", num(1.0))]);
    let g = b.add(Kind::Ground, &[]);
    let adc = b.add(Kind::AdcInput, &[]);
    b.join("gnd", &[(g, "gnd"), (v1, "neg"), (v2, "neg")]);
    b.join("in", &[(v1, "pos"), (adc, "in")]);
    b.join("ref", &[(v2, "pos"), (adc, "ref")]);
    let core = core_after(b.build(), 1);
    match core.frame().adc(adc, "in") {
        Reading::Valid { v } => {
            assert_close("V(in) − V(ref)", v.volts, 1.5, 1e-12);
            assert_eq!(v.code, (1.5f64 * 4096.0 / 3.3).floor() as u32);
        }
        other => panic!("{other:?}"),
    }
    assert!(!has(&core, "adc_floating"), "{:?}", codes(&core));
}

#[test]
fn ecu_adc_is_ratiometric_with_its_resolution() {
    let c = tp1_capteur_can();
    let (u1, rv1) = (by_label(&c, "U1"), by_label(&c, "RV1"));
    let mut core = PhysCore::new(c);
    set_param(&mut core, u1, "adc_bits", Value::Choice("8".into()));
    run(&mut core);
    steps(&mut core, 3000);
    match core.frame().adc(u1, "a0") {
        Reading::Valid { v } => {
            assert_eq!(v.bits, 8);
            // At steady state A0 = α·V(3V3): α·256 = 76.8 whatever the rail, the code depends on the ratio only.
            assert_eq!(v.code, 76);
            assert_close("vref = V(3V3)", v.vref, 3.3 * 1e4 / (1e4 + 0.05), 1e-9);
        }
        other => panic!("{other:?}"),
    }
    let _ = rv1;
}

// ---------------------------------------------------------------------------------------------- replay

/// xorshift64*: deterministic choices of the property test.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    fn pick<'a, T>(&mut self, v: &'a [T]) -> &'a T {
        &v[self.below(v.len() as u64) as usize]
    }
}

/// ref-pot-rc plus a momentary button BP1 across C1.
fn replay_circuit() -> Circuit {
    let mut b = Bench::new();
    let v1 = b.add(Kind::DcSupply, &[]);
    let g = b.add(Kind::Ground, &[]);
    let s1 = b.add(Kind::Switch, &[]);
    let rv1 = b.add(Kind::Potentiometer, &[]);
    let r1 = b.add(Kind::Resistor, &[]);
    let c1 = b.add(Kind::Capacitor, &[]);
    let adc1 = b.add(Kind::AdcInput, &[("vref", num(5.0))]);
    let adc2 = b.add(Kind::AdcInput, &[("vref", num(5.0))]);
    let bp1 = b.add(Kind::PushButton, &[]);
    b.join("v", &[(v1, "pos"), (s1, "p1")]);
    b.join("b", &[(s1, "p2"), (rv1, "b")]);
    b.join("gnd", &[(g, "gnd"), (v1, "neg"), (rv1, "a"), (c1, "p2"), (adc1, "ref"), (adc2, "ref"), (bp1, "p2")]);
    b.join("w", &[(rv1, "w"), (adc1, "in"), (r1, "p1")]);
    b.join("x", &[(r1, "p2"), (c1, "p1"), (adc2, "in"), (bp1, "p1")]);
    b.interact(Interact::SetSwitch { comp: s1, closed: true });
    b.interact(Interact::SetPot { comp: rv1, position: 0.3 });
    b.build()
}

struct Driver {
    core: PhysCore,
    rng: Rng,
    ids: std::collections::BTreeMap<String, CompId>,
    edits: u32,
    interactions: u32,
    added: i32,
}

impl Driver {
    fn apply(&mut self, cmd: PhysCommand) -> bool {
        let ok = self.core.apply(&cmd).is_ok();
        let violations = self.core.document().circuit().check();
        assert!(violations.is_empty(), "invariants after {cmd:?}: {violations:?}");
        ok
    }

    fn interaction(&mut self) {
        let i = match self.rng.below(4) {
            0 => Interact::SetPot { comp: self.ids["RV1"], position: self.rng.below(1001) as f64 / 1000.0 },
            1 => Interact::SetSwitch { comp: self.ids["S1"], closed: self.rng.below(4) != 0 },
            2 => Interact::SetButton { comp: self.ids["BP1"], pressed: self.rng.below(2) == 0 },
            _ => Interact::SetPower { comp: self.ids["V1"], on: self.rng.below(5) != 0 },
        };
        if self.apply(PhysCommand::Interact { interact: i }) {
            self.interactions += 1;
        }
    }

    fn edit(&mut self) {
        let comps: Vec<CompId> = self.ids.values().copied().collect();
        let e = match self.rng.below(10) {
            0..=3 => {
                let (label, name, values): (&str, &str, Vec<Value>) = match self.rng.below(7) {
                    0 => ("R1", "resistance", vec![num(4.7e3), num(1e4), num(2.2e4)]),
                    1 => ("C1", "capacitance", vec![num(4.7e-6), num(1e-5), num(2.2e-5)]),
                    2 => ("RV1", "resistance", vec![num(5e3), num(1e4)]),
                    3 => ("V1", "voltage", vec![num(3.3), num(5.0), num(9.0)]),
                    4 => ("V1", "current_limit", vec![num(0.05), num(0.1), num(1.0)]),
                    5 => ("ADC2", "sample_rate", vec![Value::Int(100), Value::Int(500), Value::Int(1000)]),
                    _ => ("BP1", "mode", vec![Value::Choice("momentary".into()), Value::Choice("latching".into())]),
                };
                Edit::SetParam { comp: self.ids[label], name: name.into(), value: self.rng.pick(&values).clone() }
            }
            4..=5 => {
                let delta = Pt { x: self.rng.below(5) as i32 - 2, y: self.rng.below(5) as i32 - 2 };
                Edit::Move { components: vec![*self.rng.pick(&comps)], wires: vec![], delta }
            }
            6 => Edit::Rotate { components: vec![*self.rng.pick(&comps)], wires: vec![] },
            // A few spare parts far from the circuit (each one is an island of its own).
            7 if self.added < 8 => {
                self.added += 1;
                let kind = *self.rng.pick(&[Kind::Resistor, Kind::Capacitor, Kind::Switch]);
                Edit::AddComponent { kind, pos: Pt { x: 40 * self.added, y: 400 }, rot: Rot::R0, params: ValueMap::new(), label: None }
            }
            8 => Edit::Delete { components: vec![*self.rng.pick(&[self.ids["C1"], self.ids["R1"], self.ids["S1"]])], wires: vec![] },
            _ => Edit::SetLabel { comp: self.ids["R1"], label: self.rng.pick(&["R1", "RA", "R9"]).to_string() },
        };
        if self.apply(PhysCommand::Edit { base_rev: self.core.document().rev(), edit: e }) {
            self.edits += 1;
        }
    }

    fn action(&mut self) {
        match self.rng.below(100) {
            0..=44 => self.interaction(),
            45..=69 => self.edit(),
            70..=79 => {
                self.apply(PhysCommand::Undo);
            }
            80..=87 => {
                self.apply(PhysCommand::Redo);
            }
            88..=95 => {
                // A pause: edits and interactions previewed in STOP, then RUN again.
                self.apply(PhysCommand::Stop);
                for _ in 0..1 + self.rng.below(3) {
                    if self.rng.below(2) == 0 {
                        self.edit()
                    } else {
                        self.interaction()
                    }
                }
                self.apply(PhysCommand::Run);
            }
            _ => {
                self.apply(PhysCommand::Reset);
            }
        }
    }
}

#[test]
fn replay_of_random_edits_and_interactions_gives_the_live_digest() {
    let initial = replay_circuit();
    let ids = labels(&initial);
    let mut d = Driver { core: PhysCore::new(initial.clone()), rng: Rng(0x9e37_79b9_7f4a_7c15), ids, edits: 0, interactions: 0, added: 0 };
    d.apply(PhysCommand::Run);
    let mut run_frames = 0u64;
    while run_frames < 60_000 || d.edits < 200 || d.interactions < 500 {
        d.core.step();
        run_frames += 1;
        if d.rng.below(40) == 0 {
            // One to three commands within the same frame (coalescing, deferred releases).
            for _ in 0..1 + d.rng.below(3) {
                d.action();
            }
        }
    }
    assert!(d.edits >= 200 && d.interactions >= 500, "{} edits, {} interactions", d.edits, d.interactions);
    d.apply(PhysCommand::Stop);
    let live = d.core.digest();
    let mut replayed = replay(&initial, d.core.journal(), d.core.frame_index());
    assert_eq!(replayed.journal(), d.core.journal(), "the replay journals the same entries");
    assert_eq!(replayed.digest(), live);
    assert_eq!(replayed.document().circuit(), d.core.document().circuit());
    for l in ["ADC1", "ADC2"] {
        assert_eq!(replayed.frame().adc(d.ids[l], "in"), d.core.frame().adc(d.ids[l], "in"), "{l}: same held sample");
    }
    // The digest sees the run state and the solver state.
    replayed.apply(&PhysCommand::Run).expect("run");
    let running = replayed.digest();
    assert_ne!(running, live);
    replayed.step();
    assert_ne!(replayed.digest(), running);
}
