//! Lowering of every kind (contract §2.1), seen through the public probe: the switched-off resistance between pins
//! (§5.9) gives the resistive elements and their values, the live readings give the sources, their limits and the
//! gated outputs. The exact element lists, keys, gates and node order are checked by the unit tests of
//! src/phys/lower.rs.

#[path = "phys_kit.rs"]
mod kit;

use kit::*;
use n7rt_circuit::{CompId, Interact, Kind, Value};
use n7rt_lab::phys::{CircuitProbe, OpState, PhysCore, Reading, SupplyOutput};

fn num(x: f64) -> Value {
    Value::Num(x)
}

fn r_between(core: &PhysCore, comp: CompId, a: &str, b: &str) -> Reading<f64> {
    core.frame().unpowered_resistance(&pin(comp, a), &pin(comp, b))
}

fn between(core: &PhysCore, comp: CompId, a: &str, b: &str) -> Reading<f64> {
    core.frame().voltage_between_pins(&pin(comp, a), &pin(comp, b))
}

/// One component alone, every pin on a net of its own.
fn alone(kind: Kind, params: &[(&str, Value)]) -> (PhysCore, CompId) {
    let mut b = Bench::new();
    let id = b.add(kind, params);
    (PhysCore::new(b.build()), id)
}

#[test]
fn passive_kinds() {
    let (core, r1) = alone(Kind::Resistor, &[("resistance", num(4.7e3))]);
    assert_close("resistor", valid(r_between(&core, r1, "p1", "p2")), 4.7e3, 1e-12);
    let (core, rt1) = alone(Kind::Termination, &[]);
    assert_close("termination", valid(r_between(&core, rt1, "p1", "p2")), 120.0, 1e-12);
    let (core, c1) = alone(Kind::Capacitor, &[]);
    assert_eq!(r_between(&core, c1, "p1", "p2"), Reading::Open, "a capacitor is no resistive path");

    let (mut core, rv1) = alone(Kind::Potentiometer, &[]);
    interact(&mut core, Interact::SetPot { comp: rv1, position: 0.3 });
    assert_close("R_AW = α·R", valid(r_between(&core, rv1, "a", "w")), 3e3, 1e-12);
    assert_close("R_WB = (1 − α)·R", valid(r_between(&core, rv1, "w", "b")), 7e3, 1e-12);
    assert_close("R_AB", valid(r_between(&core, rv1, "a", "b")), 1e4, 1e-12);
    interact(&mut core, Interact::SetPot { comp: rv1, position: 0.0 });
    assert_close("R_AW floor", valid(r_between(&core, rv1, "a", "w")), 1e-3, 1e-9);
    interact(&mut core, Interact::SetPot { comp: rv1, position: 1.0 });
    assert_close("R_WB floor", valid(r_between(&core, rv1, "w", "b")), 1e-3, 1e-9);
}

#[test]
fn contacts_on_and_off() {
    let (mut core, s1) = alone(Kind::Switch, &[]);
    assert_close("switch open: r_off", valid(r_between(&core, s1, "p1", "p2")), 1e9, 1e-12);
    // r_off is a leakage: it conducts but never references a node.
    assert_eq!(between(&core, s1, "p1", "p2"), Reading::Floating);
    interact(&mut core, Interact::SetSwitch { comp: s1, closed: true });
    assert_close("switch closed: r_on", valid(r_between(&core, s1, "p1", "p2")), 0.01, 1e-12);
    assert!(matches!(between(&core, s1, "p1", "p2"), Reading::Valid { .. }), "a real conductance");

    let (mut core, bp1) = alone(Kind::PushButton, &[("r_on", num(0.2)), ("r_off", num(5e8))]);
    assert_close("NO button at rest", valid(r_between(&core, bp1, "p1", "p2")), 5e8, 1e-12);
    interact(&mut core, Interact::SetButton { comp: bp1, pressed: true });
    assert_close("NO button pressed", valid(r_between(&core, bp1, "p1", "p2")), 0.2, 1e-12);
    let (mut core, nc) = alone(Kind::PushButton, &[("contact", Value::Choice("nc".into()))]);
    assert_close("NC button at rest", valid(r_between(&core, nc, "p1", "p2")), 0.1, 1e-12);
    interact(&mut core, Interact::SetButton { comp: nc, pressed: true });
    assert_close("NC button pressed", valid(r_between(&core, nc, "p1", "p2")), 1e8, 1e-12);
}

#[test]
fn adc_input_ideal_or_finite() {
    let (core, adc) = alone(Kind::AdcInput, &[]);
    assert_eq!(r_between(&core, adc, "in", "ref"), Reading::Open, "r_in = ∞: no element");
    assert_eq!(between(&core, adc, "in", "ref"), Reading::Floating);
    let (core, adc) = alone(Kind::AdcInput, &[("r_in", num(1e6))]);
    assert_close("finite r_in", valid(r_between(&core, adc, "in", "ref")), 1e6, 1e-12);
    // A real conductance: in and ref share a K group, the difference is a valid 0 V.
    assert_eq!(between(&core, adc, "in", "ref"), Reading::Valid { v: 0.0 });
}

#[test]
fn dc_supply_one_quadrant_with_current_limit() {
    let build = |load: Option<f64>| {
        let mut b = Bench::new();
        let v1 = b.add(Kind::DcSupply, &[("voltage", num(12.0)), ("current_limit", num(0.2)), ("r_out", num(0.05))]);
        let g = b.add(Kind::Ground, &[]);
        b.join("gnd", &[(g, "gnd"), (v1, "neg")]);
        if let Some(r) = load {
            let r1 = b.add(Kind::Resistor, &[("resistance", num(r))]);
            b.join("p", &[(v1, "pos"), (r1, "p1")]);
            b.join("gnd", &[(r1, "p2")]);
        }
        (PhysCore::new(b.build()), v1)
    };
    let (mut core, v1) = build(None);
    assert_eq!(r_between(&core, v1, "pos", "neg"), Reading::Open, "a source is removed when switched off");
    assert_close("open circuit", valid(between(&core, v1, "pos", "neg")), 12.0, 1e-12);
    interact(&mut core, Interact::SetPower { comp: v1, on: false });
    assert_eq!(core.frame().voltage_at(&pin(v1, "pos")), Reading::Floating, "off: no element");
    assert_eq!(core.frame().op_state(v1), Some(OpState::Supply { output: SupplyOutput::Off }));

    let (core, v1) = build(Some(1e3));
    assert_close("CV behind r_out", valid(between(&core, v1, "pos", "neg")), 12.0 * 1e3 / (1e3 + 0.05), 1e-12);
    assert_eq!(core.frame().op_state(v1), Some(OpState::Supply { output: SupplyOutput::Cv }));
    let (core, v1) = build(Some(10.0));
    assert_close("CC at i_lim", -valid(core.frame().pin_current(&pin(v1, "pos"))), 0.2, 1e-12);
    assert_close("CC voltage", valid(between(&core, v1, "pos", "neg")), 2.0, 1e-12);
    assert_eq!(core.frame().op_state(v1), Some(OpState::Supply { output: SupplyOutput::Cc }));
}

/// ECU U1 with its ground; `loads` puts resistors from pins to the ground.
fn ecu(params: &[(&str, Value)], loads: &[(&str, f64)]) -> (PhysCore, CompId) {
    let mut b = Bench::new();
    let u1 = b.add(Kind::EcuStm32, params);
    let g = b.add(Kind::Ground, &[]);
    b.join("gnd", &[(g, "gnd"), (u1, "gnd")]);
    for (p, r) in loads {
        let r1 = b.add(Kind::Resistor, &[("resistance", num(*r))]);
        b.join(p, &[(u1, p), (r1, "p1")]);
        b.join("gnd", &[(r1, "p2")]);
    }
    (PhysCore::new(b.build()), u1)
}

#[test]
fn ecu_rails_and_pd1() {
    let (core, u1) = ecu(&[], &[]);
    assert_close("5V unloaded", valid(between(&core, u1, "v5", "gnd")), 5.0, 1e-12);
    assert_close("3V3 unloaded", valid(between(&core, u1, "v3v3", "gnd")), 3.3, 1e-12);
    assert_close("PD1 recessive = V(3V3)", valid(between(&core, u1, "can_tx", "gnd")), 3.3, 1e-12);
    assert_eq!(between(&core, u1, "can_rx", "gnd"), Reading::Floating, "PD0 is an ideal input");
    assert_eq!(between(&core, u1, "a0", "gnd"), Reading::Floating, "A0 is an ideal input");
    assert_eq!(r_between(&core, u1, "v5", "gnd"), Reading::Open, "the rails are sources");

    let (core, u1) = ecu(&[("r_out_5v", num(0.2)), ("r_out_3v3", num(0.1))], &[("v5", 100.0), ("v3v3", 100.0), ("can_tx", 1e3)]);
    let v3 = 3.3 * 100.0 / (100.0 + 0.1);
    assert_close("5V behind r_out_5v", valid(between(&core, u1, "v5", "gnd")), 5.0 * 100.0 / (100.0 + 0.2), 1e-12);
    assert_close("3V3 behind r_out_3v3", valid(between(&core, u1, "v3v3", "gnd")), v3, 1e-12);
    assert_close("PD1 behind 50 Ω", valid(between(&core, u1, "can_tx", "gnd")), v3 * 1e3 / (1e3 + 50.0), 1e-12);

    // Current limits: 0.5 A on 5V, 1.3 A on 3V3.
    let (core, u1) = ecu(&[], &[("v5", 1.0), ("v3v3", 1.0)]);
    assert_close("5V limited", -valid(core.frame().pin_current(&pin(u1, "v5"))), 0.5, 1e-12);
    assert_close("3V3 limited", -valid(core.frame().pin_current(&pin(u1, "v3v3"))), 1.3, 1e-12);

    let (core, u1) = ecu(&[("can_mode", Value::Choice("hi_z".into()))], &[]);
    assert_eq!(between(&core, u1, "can_tx", "gnd"), Reading::Floating, "hi_z: no PD1 driver");
    let (mut core, u1) = ecu(&[], &[]);
    interact(&mut core, Interact::SetPower { comp: u1, on: false });
    assert_eq!(between(&core, u1, "v5", "gnd"), Reading::Floating, "usb false: no rail");
    assert_eq!(between(&core, u1, "can_tx", "gnd"), Reading::Floating);
}

/// Transceiver U1 with VSS and Rs at ground and VDD on a supply of `vdd` volts.
fn xcvr(vdd: f64) -> (PhysCore, CompId, CompId) {
    let mut b = Bench::new();
    let v1 = b.add(Kind::DcSupply, &[("voltage", num(vdd))]);
    let g = b.add(Kind::Ground, &[]);
    let u1 = b.add(Kind::CanTransceiver, &[("r_in", num(30e3))]);
    b.join("gnd", &[(g, "gnd"), (v1, "neg"), (u1, "vss"), (u1, "rs")]);
    b.join("vdd", &[(v1, "pos"), (u1, "vdd")]);
    (PhysCore::new(b.build()), v1, u1)
}

#[test]
fn transceiver_permanent_network_and_gated_outputs() {
    let (mut core, v1, u1) = xcvr(5.0);
    // Switched off: input T network, TXD pull-up and R_DD (the bias node is internal).
    assert_close("R(CANH, CANL) = 2·r_in", valid(r_between(&core, u1, "canh", "canl")), 60e3, 1e-12);
    assert_close("TXD pull-up", valid(r_between(&core, u1, "txd", "vdd")), 25e3, 1e-12);
    assert_close("R(TXD, VSS)", valid(r_between(&core, u1, "txd", "vss")), 25e3 + 500.0, 1e-12);
    assert_close("R_DD", valid(r_between(&core, u1, "vdd", "vss")), 500.0, 1e-12);
    assert_eq!(r_between(&core, u1, "rxd", "vss"), Reading::Open, "RXD is a controlled source only");

    // Powered (the POR gate switches on in the first frame): unloaded outputs.
    run(&mut core);
    core.step();
    let u = valid(between(&core, u1, "vdd", "vss"));
    // TXD is left open: its pull-up carries no current, R_DD is the only load.
    assert_close("VDD behind 10 mΩ and R_DD", u, 5.0 * 500.0 / (500.0 + 0.01), 1e-12);
    assert_close("RXD = VDD", valid(between(&core, u1, "rxd", "vss")), u, 1e-12);
    assert_close("VREF = VDD/2", valid(between(&core, u1, "vref", "vss")), u / 2.0, 1e-12);
    assert_close("CANH = VDD/2", valid(between(&core, u1, "canh", "vss")), u / 2.0, 1e-12);
    assert_close("CANL = VDD/2", valid(between(&core, u1, "canl", "vss")), u / 2.0, 1e-12);

    // POR hysteresis: on above 4.05 V, off below 3.70 V (frames: the gate state is solver state).
    let powered = |core: &PhysCore| matches!(core.frame().op_state(u1), Some(OpState::Transceiver { powered: true, .. }));
    for (volts, expected) in [(3.9, true), (3.75, true), (3.65, false), (4.0, false), (4.1, true)] {
        set_param(&mut core, v1, "voltage", num(volts));
        core.step();
        assert_eq!(powered(&core), expected, "VDD set to {volts} V");
    }
    set_param(&mut core, v1, "voltage", num(3.65));
    core.step();
    // Unpowered: bias, RXD and VREF removed; the bus side keeps its T network without reference.
    assert_eq!(between(&core, u1, "rxd", "vss"), Reading::Floating);
    assert_eq!(between(&core, u1, "vref", "vss"), Reading::Floating);
    assert_eq!(core.frame().voltage_at(&pin(u1, "canh")), Reading::NoReference);
    assert!(matches!(between(&core, u1, "canh", "canl"), Reading::Valid { .. }));
}

#[test]
fn bus_remote_nodes_and_termination() {
    let build = |params: &[(&str, Value)]| {
        let mut b = Bench::new();
        let bus = b.add(Kind::CanBus, params);
        let g = b.add(Kind::Ground, &[]);
        b.join("gnd", &[(g, "gnd"), (bus, "gnd")]);
        (PhysCore::new(b.build()), bus)
    };
    let (core, bus) = build(&[("remote_nodes", Value::Int(2)), ("remote_r_in", num(25e3))]);
    assert_close("120 Ω ∥ 2·r_in/n", valid(r_between(&core, bus, "canh", "canl")), 120.0 * 25e3 / (120.0 + 25e3), 1e-12);
    assert_eq!(r_between(&core, bus, "canh", "gnd"), Reading::Open, "the 2.5 V bias is a source");
    assert_close("CANH recessive", valid(between(&core, bus, "canh", "gnd")), 2.5, 1e-12);
    assert_close("CANL recessive", valid(between(&core, bus, "canl", "gnd")), 2.5, 1e-12);

    let (core, bus) = build(&[("remote_nodes", Value::Int(1)), ("remote_termination", Value::Bool(false))]);
    assert_close("2·r_in, no termination", valid(r_between(&core, bus, "canh", "canl")), 50e3, 1e-12);
    let (core, bus) = build(&[("remote_nodes", Value::Int(0))]);
    assert_close("termination only", valid(r_between(&core, bus, "canh", "canl")), 120.0, 1e-12);
    assert_eq!(core.frame().voltage_at(&pin(bus, "canh")), Reading::NoReference, "n = 0: no bias");
    let (core, bus) = build(&[("remote_nodes", Value::Int(0)), ("remote_termination", Value::Bool(false))]);
    assert_eq!(r_between(&core, bus, "canh", "canl"), Reading::Open, "nothing at all");
}

#[test]
fn ground_symbols_form_the_reference_net() {
    let mut b = Bench::new();
    let g1 = b.add(Kind::Ground, &[]);
    let r1 = b.add(Kind::Resistor, &[]);
    let g2 = b.add(Kind::Ground, &[]);
    b.join("a", &[(g1, "gnd"), (r1, "p1")]);
    b.join("b", &[(g2, "gnd"), (r1, "p2")]);
    let core = PhysCore::new(b.build());
    let f = core.frame();
    // Every ground pin is one net "GND" at 0 V, without a wire between the symbols.
    assert_eq!(f.net_of(&pin(g1, "gnd")), f.net_of(&pin(g2, "gnd")));
    assert_eq!(f.voltage_at(&pin(r1, "p2")), Reading::Valid { v: 0.0 });
    assert_eq!(valid(f.unpowered_resistance(&pin(r1, "p1"), &pin(r1, "p2"))), 0.0, "R1 shorted by the ground net");
}
