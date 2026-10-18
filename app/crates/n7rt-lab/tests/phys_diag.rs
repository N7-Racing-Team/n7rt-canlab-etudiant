//! Diagnostics of contract §3.7: for every code, a minimal circuit that raises it and a neighbouring one that does
//! not, with the severity of the table. The solver statuses (`solver_singular`, `solver_mode_cycle`), which no
//! library circuit produces, and `internal_error` / `engine_restarted` are covered by the unit tests of the
//! engine (src/phys/diag.rs, kernel.rs, handle.rs).

#[path = "phys_kit.rs"]
mod kit;

use kit::*;
use n7rt_circuit::{CompId, Interact, ItemRef, Kind, Value};
use n7rt_lab::phys::{CircuitProbe, OpState, PhysCore, Reading, RsMode, Severity, SupplyOutput, TxdState};

fn num(x: f64) -> Value {
    Value::Num(x)
}

fn severity(core: &PhysCore, code: &str) -> Severity {
    diag(core, code).unwrap_or_else(|| panic!("{code} expected, got {:?}", codes(core))).severity
}

fn items(core: &PhysCore, code: &str) -> Vec<ItemRef> {
    diag(core, code).unwrap_or_else(|| panic!("{code} expected, got {:?}", codes(core))).items
}

fn pin_item(comp: CompId, pin: &str) -> ItemRef {
    ItemRef::Pin { comp, pin: pin.to_string() }
}

fn comp_item(id: CompId) -> ItemRef {
    ItemRef::Component { id }
}

/// A bench supply V1 with a ground symbol on its negative terminal; returns (bench, V1, ground).
fn grounded_supply(volts: f64, extra: &[(&str, Value)]) -> (Bench, CompId, CompId) {
    let mut b = Bench::new();
    let mut params = vec![("voltage", num(volts))];
    params.extend(extra.iter().cloned());
    let v1 = b.add(Kind::DcSupply, &params);
    let g = b.add(Kind::Ground, &[]);
    b.join("gnd", &[(g, "gnd"), (v1, "neg")]);
    (b, v1, g)
}

/// A transceiver U1 supplied by V1 (`vdd` volts) on VDD, VSS at ground; `wire` adds the rest.
fn transceiver(vdd: f64, wire: impl FnOnce(&mut Bench, CompId, CompId, CompId)) -> PhysCore {
    let (mut b, v1, g) = grounded_supply(vdd, &[]);
    let u1 = b.add(Kind::CanTransceiver, &[]);
    b.join("vdd", &[(v1, "pos"), (u1, "vdd")]);
    b.join("gnd", &[(u1, "vss")]);
    wire(&mut b, v1, g, u1);
    PhysCore::new(b.build())
}

/// An ECU U1 with its GND on the ground symbol; `wire` adds the rest.
fn ecu(params: &[(&str, Value)], wire: impl FnOnce(&mut Bench, CompId, CompId)) -> PhysCore {
    let mut b = Bench::new();
    let u1 = b.add(Kind::EcuStm32, params);
    let g = b.add(Kind::Ground, &[]);
    b.join("gnd", &[(g, "gnd"), (u1, "gnd")]);
    wire(&mut b, u1, g);
    PhysCore::new(b.build())
}

// ------------------------------------------------------------------------------------------------ topological

#[test]
fn no_ground_and_island_no_reference() {
    let build = |ground: bool| {
        let mut b = Bench::new();
        let v1 = b.add(Kind::DcSupply, &[]);
        let r1 = b.add(Kind::Resistor, &[("resistance", num(1e3))]);
        b.join("p", &[(v1, "pos"), (r1, "p1")]);
        b.join("n", &[(v1, "neg"), (r1, "p2")]);
        if ground {
            let g = b.add(Kind::Ground, &[]);
            b.join("n", &[(g, "gnd")]);
        }
        (PhysCore::new(b.build()), v1)
    };
    let (core, v1) = build(false);
    assert_eq!(severity(&core, "no_ground"), Severity::Warning);
    assert_eq!(items(&core, "no_ground"), vec![comp_item(v1)]);
    assert_eq!(severity(&core, "island_no_reference"), Severity::Warning);
    let d = diag(&core, "island_no_reference").expect("island");
    assert!(d.message_fr.contains("V1.+") && d.message_fr.contains("V1.\u{2212}"), "{}", d.message_fr);
    let (core, _) = build(true);
    assert!(!has(&core, "no_ground") && !has(&core, "island_no_reference"), "{:?}", codes(&core));
}

#[test]
fn unconnected_pin_pair() {
    let build = |ref_wired: bool| {
        let (mut b, v1, g) = grounded_supply(5.0, &[]);
        let r1 = b.add(Kind::Resistor, &[]);
        let r2 = b.add(Kind::Resistor, &[]);
        let adc = b.add(Kind::AdcInput, &[]);
        b.join("p", &[(v1, "pos"), (r1, "p1")]);
        b.join("mid", &[(r1, "p2"), (r2, "p1"), (adc, "in")]);
        b.join("gnd", &[(r2, "p2")]);
        if ref_wired {
            b.join("gnd", &[(adc, "ref")]);
        }
        let _ = g;
        (PhysCore::new(b.build()), adc)
    };
    let (core, adc) = build(false);
    assert_eq!(severity(&core, "unconnected_pin"), Severity::Warning);
    assert_eq!(items(&core, "unconnected_pin"), vec![pin_item(adc, "ref")]);
    let (core, _) = build(true);
    assert!(!has(&core, "unconnected_pin"), "{:?}", codes(&core));
}

#[test]
fn unconnected_bus_ground_only_with_remote_nodes() {
    let build = |remote_nodes: i64| {
        let mut b = Bench::new();
        let rt = b.add(Kind::Termination, &[]);
        let bus = b.add(Kind::CanBus, &[("remote_nodes", Value::Int(remote_nodes))]);
        b.join("h", &[(rt, "p1"), (bus, "canh")]);
        b.join("l", &[(rt, "p2"), (bus, "canl")]);
        (PhysCore::new(b.build()), bus)
    };
    let (core, bus) = build(1);
    assert_eq!(items(&core, "unconnected_pin"), vec![pin_item(bus, "gnd")]);
    let (core, _) = build(0);
    assert!(!has(&core, "unconnected_pin"), "{:?}", codes(&core));
}

#[test]
fn open_pin_pair() {
    let build = |closed: bool| {
        let (mut b, v1, _) = grounded_supply(5.0, &[]);
        let r1 = b.add(Kind::Resistor, &[]);
        b.join("p", &[(v1, "pos"), (r1, "p1")]);
        if closed {
            b.join("gnd", &[(r1, "p2")]);
        }
        (PhysCore::new(b.build()), r1)
    };
    let (core, r1) = build(false);
    assert_eq!(severity(&core, "open_pin"), Severity::Info);
    assert_eq!(items(&core, "open_pin"), vec![pin_item(r1, "p2")]);
    // No current through an open pin.
    assert!(valid(core.frame().pin_current(&pin(r1, "p2"))).abs() < 1e-15);
    let (core, _) = build(true);
    assert!(!has(&core, "open_pin"), "{:?}", codes(&core));
}

#[test]
fn output_short_rxd_pair() {
    let core = transceiver(5.0, |b, _, _, u1| b.join("gnd", &[(u1, "rxd")]));
    assert_eq!(severity(&core, "output_short"), Severity::Error);
    let core = transceiver(5.0, |b, _, _, u1| {
        let r = b.add(Kind::Resistor, &[]);
        b.join("rx", &[(u1, "rxd"), (r, "p1")]);
        b.join("gnd", &[(r, "p2")]);
    });
    assert!(!has(&core, "output_short"), "{:?}", codes(&core));
}

#[test]
fn output_short_pd1_only_when_driven() {
    let build = |mode: &str| ecu(&[("can_mode", Value::Choice(mode.to_string()))], |b, u1, _| b.join("gnd", &[(u1, "can_tx")]));
    let core = build("can");
    assert_eq!(severity(&core, "output_short"), Severity::Error);
    assert!(items(&core, "output_short").contains(&ItemRef::Pin { comp: CompId(1), pin: "can_tx".into() }));
    assert!(!has(&build("hi_z"), "output_short"));
}

#[test]
fn source_short_pair() {
    // Without ground: both terminals on one floating node, a current-limited output (contract §5.4).
    let mut b = Bench::new();
    let v1 = b.add(Kind::DcSupply, &[]);
    b.join("s", &[(v1, "pos"), (v1, "neg")]);
    let core = PhysCore::new(b.build());
    assert_eq!(severity(&core, "source_short"), Severity::Error);
    assert_eq!(items(&core, "source_short"), vec![comp_item(v1)]);
    assert_eq!(core.frame().op_state(v1), Some(OpState::Supply { output: SupplyOutput::Cc }));
    assert_close("short-circuit current", -valid(core.frame().pin_current(&pin(v1, "pos"))), 0.1, 1e-12);
    // Both terminals on the ground net.
    let (mut b, v1, _) = grounded_supply(5.0, &[]);
    b.join("gnd", &[(v1, "pos")]);
    assert!(has(&PhysCore::new(b.build()), "source_short"));
    // A load between the terminals: no short.
    let (mut b, v1, _) = grounded_supply(5.0, &[]);
    let r1 = b.add(Kind::Resistor, &[]);
    b.join("p", &[(v1, "pos"), (r1, "p1")]);
    b.join("gnd", &[(r1, "p2")]);
    let core = PhysCore::new(b.build());
    assert!(!has(&core, "source_short"), "{:?}", codes(&core));
}

// ------------------------------------------------------------------------------------------------- electrical

#[test]
fn floating_net_through_leakage_only() {
    // V1 → S1 → X → ADC1.in: with S1 open, X is linked by the leakage r_off only.
    let build = |closed: bool| {
        let (mut b, v1, _) = grounded_supply(5.0, &[]);
        let s1 = b.add(Kind::Switch, &[]);
        let adc = b.add(Kind::AdcInput, &[("vref", num(5.0))]);
        b.join("p", &[(v1, "pos"), (s1, "p1")]);
        b.join("x", &[(s1, "p2"), (adc, "in")]);
        b.join("gnd", &[(adc, "ref")]);
        b.interact(Interact::SetSwitch { comp: s1, closed });
        (PhysCore::new(b.build()), adc)
    };
    let (core, adc) = build(false);
    assert_eq!(severity(&core, "floating_net"), Severity::Warning);
    assert_eq!(core.frame().voltage_at(&pin(adc, "in")), Reading::Floating);
    assert_eq!(severity(&core, "adc_floating"), Severity::Warning);
    let (core, adc) = build(true);
    assert!(!has(&core, "floating_net") && !has(&core, "adc_floating"), "{:?}", codes(&core));
    assert_close("V(X)", valid(core.frame().voltage_at(&pin(adc, "in"))), 5.0, 1e-6);
}

#[test]
fn adc_floating_pair() {
    let build = |wired: bool| {
        let (mut b, v1, _) = grounded_supply(5.0, &[]);
        let r1 = b.add(Kind::Resistor, &[]);
        let r2 = b.add(Kind::Resistor, &[]);
        let adc = b.add(Kind::AdcInput, &[]);
        b.join("p", &[(v1, "pos"), (r1, "p1")]);
        b.join("mid", &[(r1, "p2"), (r2, "p1")]);
        b.join("gnd", &[(r2, "p2"), (adc, "ref")]);
        if wired {
            b.join("mid", &[(adc, "in")]);
        }
        (PhysCore::new(b.build()), adc)
    };
    let (core, adc) = build(false);
    assert_eq!(items(&core, "adc_floating"), vec![pin_item(adc, "in")]);
    let (core, _) = build(true);
    assert!(!has(&core, "adc_floating"), "{:?}", codes(&core));
}

#[test]
fn adc_saturated_pair() {
    // ADC1 (vref 3.3 V) on the supply output; sampled at the end of the first frame.
    let build = |volts: f64| {
        let (mut b, v1, _) = grounded_supply(volts, &[]);
        let adc = b.add(Kind::AdcInput, &[]);
        b.join("p", &[(v1, "pos"), (adc, "in")]);
        b.join("gnd", &[(adc, "ref")]);
        (core_after(b.build(), 1), adc)
    };
    let (core, adc) = build(5.0);
    assert_eq!(severity(&core, "adc_saturated"), Severity::Warning);
    match core.frame().adc(adc, "in") {
        Reading::Saturated { v } => assert_eq!((v.code, v.bits), (4095, 12)),
        other => panic!("{other:?}"),
    }
    let (core, _) = build(3.0);
    assert!(!has(&core, "adc_saturated"), "{:?}", codes(&core));
}

#[test]
fn logic_floating_pair() {
    let core = ecu(&[], |_, _, _| {});
    assert_eq!(items(&core, "logic_floating"), vec![pin_item(CompId(1), "can_rx")]);
    assert_eq!(severity(&core, "logic_floating"), Severity::Warning);
    let core = ecu(&[], |b, u1, _| b.join("3v3", &[(u1, "v3v3"), (u1, "can_rx")]));
    assert!(!has(&core, "logic_floating"), "{:?}", codes(&core));
}

#[test]
fn logic_undefined_pair() {
    // PD0 on a divider of the 3V3 rail: 1.65 V lies between VIL = 0.99 V and VIH = 2.31 V; 0.595 V is low.
    let build = |r2: f64| {
        ecu(&[], |b, u1, _| {
            let r1 = b.add(Kind::Resistor, &[]);
            let r2 = b.add(Kind::Resistor, &[("resistance", num(r2))]);
            b.join("3v3", &[(u1, "v3v3"), (r1, "p1")]);
            b.join("rx", &[(r1, "p2"), (r2, "p1"), (u1, "can_rx")]);
            b.join("gnd", &[(r2, "p2")]);
        })
    };
    let core = build(1e4);
    assert_eq!(severity(&core, "logic_undefined"), Severity::Warning);
    assert_eq!(core.frame().logic(&pin(CompId(1), "can_rx")), Reading::Undefined);
    let core = build(2.2e3);
    assert!(!has(&core, "logic_undefined"), "{:?}", codes(&core));
}

#[test]
fn supply_current_limit_pair() {
    let build = |load: f64| {
        let (mut b, v1, _) = grounded_supply(5.0, &[]);
        let r1 = b.add(Kind::Resistor, &[("resistance", num(load)), ("power_rating", num(10.0))]);
        b.join("p", &[(v1, "pos"), (r1, "p1")]);
        b.join("gnd", &[(r1, "p2")]);
        (PhysCore::new(b.build()), v1, r1)
    };
    let (core, v1, r1) = build(10.0);
    assert_eq!(severity(&core, "supply_current_limit"), Severity::Warning);
    assert_eq!(core.frame().op_state(v1), Some(OpState::Supply { output: SupplyOutput::Cc }));
    assert_close("CC: V = R·i_lim", valid(core.frame().voltage_at(&pin(r1, "p1"))), 1.0, 1e-9);
    let (core, v1, _) = build(100.0);
    assert!(!has(&core, "supply_current_limit"), "{:?}", codes(&core));
    assert_eq!(core.frame().op_state(v1), Some(OpState::Supply { output: SupplyOutput::Cv }));
}

#[test]
fn supply_backfeed_pair() {
    // V2 (6 V) pushes the output of V1 (5 V) above its setting: V1 delivers nothing.
    let build = |v2_on: bool| {
        let (mut b, v1, g) = grounded_supply(5.0, &[]);
        let v2 = b.add(Kind::DcSupply, &[("voltage", num(6.0))]);
        let r1 = b.add(Kind::Resistor, &[("resistance", num(1e3))]);
        b.join("p", &[(v1, "pos"), (v2, "pos"), (r1, "p1")]);
        b.join("gnd", &[(v2, "neg"), (r1, "p2")]);
        b.interact(Interact::SetPower { comp: v2, on: v2_on });
        let _ = g;
        (PhysCore::new(b.build()), v1)
    };
    let (core, v1) = build(true);
    assert_eq!(severity(&core, "supply_backfeed"), Severity::Warning);
    assert_eq!(items(&core, "supply_backfeed"), vec![comp_item(v1)]);
    assert_eq!(core.frame().op_state(v1), Some(OpState::Supply { output: SupplyOutput::Rev }));
    assert_eq!(valid(core.frame().pin_current(&pin(v1, "pos"))), 0.0, "REV delivers nothing");
    let (core, v1) = build(false);
    assert!(!has(&core, "supply_backfeed"), "{:?}", codes(&core));
    assert_eq!(core.frame().op_state(v1), Some(OpState::Supply { output: SupplyOutput::Cv }));
}

#[test]
fn rail_overload_pair() {
    let build = |load: f64| {
        ecu(&[], |b, u1, _| {
            let r1 = b.add(Kind::Resistor, &[("resistance", num(load)), ("power_rating", num(10.0))]);
            b.join("5v", &[(u1, "v5"), (r1, "p1")]);
            b.join("gnd", &[(r1, "p2")]);
        })
    };
    let core = build(10.0);
    assert_eq!(severity(&core, "rail_overload"), Severity::Warning);
    assert!(!has(&core, "supply_current_limit"), "0.495 A stays under the 0.5 A limit: {:?}", codes(&core));
    let core = build(100.0);
    assert!(!has(&core, "rail_overload"), "{:?}", codes(&core));
}

#[test]
fn resistor_overload_pair() {
    let build = |load: f64| {
        let (mut b, v1, _) = grounded_supply(5.0, &[("current_limit", num(5.0))]);
        let r1 = b.add(Kind::Resistor, &[("resistance", num(load))]);
        b.join("p", &[(v1, "pos"), (r1, "p1")]);
        b.join("gnd", &[(r1, "p2")]);
        (PhysCore::new(b.build()), r1)
    };
    let (core, r1) = build(10.0);
    assert_eq!(severity(&core, "resistor_overload"), Severity::Error);
    assert_eq!(items(&core, "resistor_overload"), vec![comp_item(r1)]);
    let (core, _) = build(1e3);
    assert!(!has(&core, "resistor_overload"), "{:?}", codes(&core));
}

#[test]
fn potentiometer_overload_per_segment() {
    // 100 Ω, 0.25 W: a segment may carry √(P/R) = 50 mA. 10 V across A–B: 100 mA; 4 V: 40 mA.
    let build = |volts: f64| {
        let (mut b, v1, _) = grounded_supply(volts, &[("current_limit", num(1.0))]);
        let rv1 = b.add(Kind::Potentiometer, &[("resistance", num(100.0))]);
        b.join("p", &[(v1, "pos"), (rv1, "b")]);
        b.join("gnd", &[(rv1, "a")]);
        (PhysCore::new(b.build()), rv1)
    };
    let (core, rv1) = build(10.0);
    assert_eq!(items(&core, "resistor_overload"), vec![comp_item(rv1)]);
    let (core, _) = build(4.0);
    assert!(!has(&core, "resistor_overload"), "{:?}", codes(&core));
}

#[test]
fn cap_overvoltage_pair() {
    // 5 V through 100 Ω on 10 µF (τ = 1 ms): charged after 50 frames.
    let build = |rating: f64| {
        let (mut b, v1, _) = grounded_supply(5.0, &[]);
        let r1 = b.add(Kind::Resistor, &[("resistance", num(100.0))]);
        let c1 = b.add(Kind::Capacitor, &[("voltage_rating", num(rating))]);
        b.join("p", &[(v1, "pos"), (r1, "p1")]);
        b.join("x", &[(r1, "p2"), (c1, "p1")]);
        b.join("gnd", &[(c1, "p2")]);
        core_after(b.build(), 50)
    };
    let core = build(4.0);
    assert_eq!(severity(&core, "cap_overvoltage"), Severity::Error);
    assert!(!has(&build(25.0), "cap_overvoltage"));
}

#[test]
fn cap_reverse_pair() {
    // Polarized: V(p1) − V(p2) < −0.5 V is reverse; the same capacitor the right way round is not.
    let build = |reversed: bool| {
        let (mut b, v1, _) = grounded_supply(5.0, &[]);
        let r1 = b.add(Kind::Resistor, &[("resistance", num(100.0))]);
        let c1 = b.add(Kind::Capacitor, &[("polarized", Value::Bool(true))]);
        b.join("p", &[(v1, "pos"), (r1, "p1")]);
        let (hot, cold) = if reversed { ("p2", "p1") } else { ("p1", "p2") };
        b.join("x", &[(r1, "p2"), (c1, hot)]);
        b.join("gnd", &[(c1, cold)]);
        core_after(b.build(), 50)
    };
    let core = build(true);
    assert_eq!(severity(&core, "cap_reverse"), Severity::Error);
    assert!(!has(&core, "cap_overvoltage"), "5 V is within the 25 V rating");
    assert!(!has(&build(false), "cap_reverse"));
}

#[test]
fn output_overcurrent_pd1_is_an_error() {
    // PD1 (3.3 V behind 50 Ω) into 100 Ω: 22 mA > 20 mA; into 1 kΩ: 3.1 mA.
    let build = |load: f64| {
        ecu(&[], |b, u1, _| {
            let r1 = b.add(Kind::Resistor, &[("resistance", num(load))]);
            b.join("tx", &[(u1, "can_tx"), (r1, "p1")]);
            b.join("gnd", &[(r1, "p2")]);
        })
    };
    let core = build(100.0);
    assert_eq!(severity(&core, "output_overcurrent"), Severity::Error);
    assert_eq!(items(&core, "output_overcurrent"), vec![pin_item(CompId(1), "can_tx")]);
    assert_close("I(PD1)", -valid(core.frame().pin_current(&pin(CompId(1), "can_tx"))), 3.3 / 150.0, 1e-9);
    assert!(!has(&build(1e3), "output_overcurrent"));
}

#[test]
fn output_overcurrent_rxd_is_a_warning() {
    // RXD (VDD behind 187.5 Ω) into 100 Ω: 17 mA > 8 mA; into 10 kΩ: 0.5 mA.
    let build = |load: f64| {
        transceiver(5.0, |b, _, _, u1| {
            let r1 = b.add(Kind::Resistor, &[("resistance", num(load))]);
            b.join("rx", &[(u1, "rxd"), (r1, "p1")]);
            b.join("gnd", &[(r1, "p2")]);
            b.join("gnd", &[(u1, "rs")]);
        })
    };
    let core = build(100.0);
    assert_eq!(severity(&core, "output_overcurrent"), Severity::Warning);
    assert!(!has(&build(1e4), "output_overcurrent"));
}

#[test]
fn ft_pin_abs_max_powered_and_unpowered() {
    // PD0 driven by V1: limit VDD + 4.0 V = 7.3 V powered, 4.0 V unpowered.
    let build = |volts: f64, usb: bool| {
        let mut b = Bench::new();
        let u1 = b.add(Kind::EcuStm32, &[]);
        let g = b.add(Kind::Ground, &[]);
        let v1 = b.add(Kind::DcSupply, &[("voltage", num(volts))]);
        b.join("gnd", &[(g, "gnd"), (u1, "gnd"), (v1, "neg")]);
        b.join("rx", &[(v1, "pos"), (u1, "can_rx")]);
        b.interact(Interact::SetPower { comp: u1, on: usb });
        PhysCore::new(b.build())
    };
    let core = build(9.0, true);
    assert_eq!(severity(&core, "ft_pin_abs_max"), Severity::Error);
    assert_eq!(items(&core, "ft_pin_abs_max"), vec![pin_item(CompId(1), "can_rx")]);
    assert!(!has(&build(5.0, true), "ft_pin_abs_max"));
    assert!(has(&build(5.0, false), "ft_pin_abs_max"));
    assert!(!has(&build(3.0, false), "ft_pin_abs_max"));
}

#[test]
fn ft_pin_abs_max_below_ground() {
    // −1 V on PD0: V1 reversed, its + at ground.
    let mut b = Bench::new();
    let u1 = b.add(Kind::EcuStm32, &[]);
    let g = b.add(Kind::Ground, &[]);
    let v1 = b.add(Kind::DcSupply, &[("voltage", num(1.0))]);
    b.join("gnd", &[(g, "gnd"), (u1, "gnd"), (v1, "pos")]);
    b.join("rx", &[(v1, "neg"), (u1, "can_rx")]);
    let core = PhysCore::new(b.build());
    assert_close("PD0", valid(core.frame().voltage_at(&pin(u1, "can_rx"))), -1.0, 1e-6);
    assert!(has(&core, "ft_pin_abs_max"), "{:?}", codes(&core));
}

#[test]
fn ic_abs_max_and_vdd_out_of_spec() {
    let core = transceiver(9.0, |b, _, _, u1| b.join("gnd", &[(u1, "rs")]));
    assert_eq!(severity(&core, "ic_abs_max"), Severity::Error);
    assert_eq!(severity(&core, "vdd_out_of_spec"), Severity::Warning);
    let core = transceiver(4.2, |b, _, _, u1| b.join("gnd", &[(u1, "rs")]));
    assert!(!has(&core, "ic_abs_max"));
    assert_eq!(severity(&core, "vdd_out_of_spec"), Severity::Warning);
    assert!(matches!(core.frame().op_state(CompId(3)), Some(OpState::Transceiver { powered: true, .. })), "4.2 V is above the POR");
    let core = transceiver(5.0, |b, _, _, u1| b.join("gnd", &[(u1, "rs")]));
    assert!(!has(&core, "ic_abs_max") && !has(&core, "vdd_out_of_spec"), "{:?}", codes(&core));
}

#[test]
fn ic_unpowered_transceiver_and_ecu() {
    let core = transceiver(3.0, |b, _, _, u1| b.join("gnd", &[(u1, "rs")]));
    assert_eq!(severity(&core, "ic_unpowered"), Severity::Warning);
    assert_eq!(
        core.frame().op_state(CompId(3)),
        Some(OpState::Transceiver { powered: false, rs: RsMode::HighSpeed, txd: TxdState::Recessive })
    );
    let core = transceiver(5.0, |b, _, _, u1| b.join("gnd", &[(u1, "rs")]));
    assert!(!has(&core, "ic_unpowered"), "{:?}", codes(&core));

    let mut b = Bench::new();
    let u1 = b.add(Kind::EcuStm32, &[]);
    let g = b.add(Kind::Ground, &[]);
    b.join("gnd", &[(g, "gnd"), (u1, "gnd")]);
    b.interact(Interact::SetPower { comp: u1, on: false });
    let core = PhysCore::new(b.build());
    assert_eq!(items(&core, "ic_unpowered"), vec![comp_item(u1)]);
    assert!(!has(&ecu(&[], |_, _, _| {}), "ic_unpowered"));
}

#[test]
fn rs_modes() {
    let op = |core: &PhysCore| core.frame().op_state(CompId(3));
    let rs_of = |core: &PhysCore| match op(core) {
        Some(OpState::Transceiver { rs, .. }) => rs,
        other => panic!("{other:?}"),
    };
    // Rs on the VSS net: high speed, nothing to report.
    let core = transceiver(5.0, |b, _, _, u1| b.join("gnd", &[(u1, "rs")]));
    assert_eq!(rs_of(&core), RsMode::HighSpeed);
    for code in ["rs_floating", "rs_non_modelise", "ic_standby"] {
        assert!(!has(&core, code), "{code}");
    }
    // Rs open: floating.
    let core = transceiver(5.0, |_, _, _, _| {});
    assert_eq!(rs_of(&core), RsMode::Floating);
    assert_eq!(severity(&core, "rs_floating"), Severity::Warning);
    // A slope resistor: not modelled in V1, never high speed.
    let core = transceiver(5.0, |b, _, _, u1| {
        let r1 = b.add(Kind::Resistor, &[]);
        b.join("rs", &[(u1, "rs"), (r1, "p1")]);
        b.join("gnd", &[(r1, "p2")]);
    });
    assert_eq!(rs_of(&core), RsMode::Undefined);
    assert_eq!(severity(&core, "rs_non_modelise"), Severity::Warning);
    // Driven by a source at VDD/2 (VREF): between the bands.
    let core = transceiver(5.0, |b, _, _, u1| b.join("vref", &[(u1, "vref"), (u1, "rs")]));
    assert_eq!(rs_of(&core), RsMode::Undefined);
    assert!(has(&core, "rs_non_modelise"));
    // Driven at VDD: standby.
    let core = transceiver(5.0, |b, _, _, u1| b.join("vdd", &[(u1, "rs")]));
    assert_eq!(rs_of(&core), RsMode::Standby);
    assert_eq!(severity(&core, "ic_standby"), Severity::Info);
    assert!(!has(&core, "rs_non_modelise") && !has(&core, "rs_floating"));
}

/// U1 powered, Rs at VSS, TXD pulled low by the switch S1 to ground (TXD high through its pull-up when open).
fn txd_switch(closed: bool) -> (PhysCore, CompId, CompId) {
    let (mut b, v1, _) = grounded_supply(5.0, &[]);
    let u1 = b.add(Kind::CanTransceiver, &[]);
    let s1 = b.add(Kind::Switch, &[]);
    let rt = b.add(Kind::Termination, &[]);
    b.join("vdd", &[(v1, "pos"), (u1, "vdd")]);
    b.join("gnd", &[(u1, "vss"), (u1, "rs"), (s1, "p2")]);
    b.join("tx", &[(u1, "txd"), (s1, "p1")]);
    b.join("h", &[(u1, "canh"), (rt, "p1")]);
    b.join("l", &[(u1, "canl"), (rt, "p2")]);
    b.interact(Interact::SetSwitch { comp: s1, closed });
    (PhysCore::new(b.build()), u1, s1)
}

#[test]
fn dominant_requested_then_timeout() {
    let (mut core, u1, s1) = txd_switch(true);
    let txd = |core: &PhysCore| match core.frame().op_state(u1) {
        Some(OpState::Transceiver { txd, .. }) => txd,
        other => panic!("{other:?}"),
    };
    // STOP preview: dominant requested, the bus readings withheld.
    assert_eq!(severity(&core, "dominant_requested"), Severity::Warning);
    assert_eq!(unavailable(core.frame().voltage_at(&pin(u1, "canh"))), "dominant_non_modelise_v1");
    run(&mut core);
    core.step();
    assert_eq!(txd(&core), TxdState::DominantRequested, "1 ms low: below the 1.25 ms timer");
    assert!(!has(&core, "dominant_timeout"));
    core.step();
    assert_eq!(txd(&core), TxdState::DominantTimeout, "2 ms low: drivers disabled");
    assert_eq!(severity(&core, "dominant_timeout"), Severity::Warning);
    assert!(!has(&core, "dominant_requested"));
    // Bus recessive again: readings available.
    assert_close("CANH", valid(core.frame().voltage_at(&pin(u1, "canh"))), 2.5, 1e-3);
    steps(&mut core, 5);
    assert_eq!(txd(&core), TxdState::DominantTimeout, "latched until a rising edge of TXD");
    // Rising edge: the timer and its latch are cleared; low again: a new request.
    interact(&mut core, Interact::SetSwitch { comp: s1, closed: false });
    core.step();
    assert_eq!(txd(&core), TxdState::Recessive);
    interact(&mut core, Interact::SetSwitch { comp: s1, closed: true });
    core.step();
    assert_eq!(txd(&core), TxdState::DominantRequested);

    let (core, _, _) = txd_switch(false);
    assert!(!has(&core, "dominant_requested") && !has(&core, "dominant_timeout"), "{:?}", codes(&core));
}

#[test]
fn diagnostics_are_sorted_by_severity_code_item() {
    // Several codes at once: errors, then warnings, then infos; codes ascending inside a severity.
    let core = transceiver(9.0, |b, _, _, u1| {
        let r1 = b.add(Kind::Resistor, &[]);
        b.join("rx", &[(u1, "rxd"), (r1, "p1")]);
        let _ = r1;
    });
    let d = core.frame().diagnostics().to_vec();
    assert!(d.len() >= 4, "{d:?}");
    let rank = |s: Severity| match s {
        Severity::Error => 0,
        Severity::Warning => 1,
        Severity::Info => 2,
    };
    for w in d.windows(2) {
        assert!((rank(w[0].severity), &w[0].code) <= (rank(w[1].severity), &w[1].code), "{d:?}");
    }
}
