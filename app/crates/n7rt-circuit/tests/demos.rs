//! Embedded demonstrations (contract §9) and the hidden performance fixture (§8.6).
//!
//! Each demonstration is built here through validated edits (every wire with explicit bends, so the layout does not
//! depend on the autoroute order) and saved canonically. `committed_demo_files_equal_their_generators` checks that
//! the files embedded by `src/demos.rs` are exactly this output; after a deliberate change, regenerate them with
//! `cargo test -p n7rt-circuit --test demos -- --ignored regenerate_demo_files`.

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::*;
use n7rt_circuit::{
    demos, kind_def, persist, Circuit, CompId, Document, Endpoint, Interact, Kind, Netlist, PinRef, Pt, Rot, Value,
    WireId,
};

// ------------------------------------------------------------------------------------------------ builder

/// A document driven by edits that must all be accepted, with an origin offset for tiled layouts.
struct Builder {
    d: Document,
    origin: Pt,
}

impl Builder {
    fn new(name: &str) -> Builder {
        Builder { d: Document::new(Circuit::new(name)), origin: pt(0, 0) }
    }

    fn at(&self, (x, y): (i32, i32)) -> Pt {
        pt(self.origin.x + x, self.origin.y + y)
    }

    fn add(&mut self, kind: Kind, xy: (i32, i32), rot: Rot, params: &[(&str, Value)]) -> CompId {
        let p = self.at(xy);
        add_with(&mut self.d, kind, p.x, p.y, rot, params)
    }

    fn bends(&self, bends: &[(i32, i32)]) -> Option<Vec<Pt>> {
        Some(bends.iter().map(|b| self.at(*b)).collect())
    }

    fn wire(&mut self, a: (CompId, &str), b: (CompId, &str), bends: &[(i32, i32)]) -> WireId {
        let bends = self.bends(bends);
        wire(&mut self.d, pin(a.0, a.1), pin(b.0, b.1), bends)
    }

    /// A wire from a new junction at `at`, inside an existing wire, to pin `b`.
    fn tap(&mut self, at: (i32, i32), b: (CompId, &str), bends: &[(i32, i32)]) -> WireId {
        let at = self.at(at);
        let c = self.d.circuit();
        let host = c
            .wires()
            .keys()
            .copied()
            .find(|w| {
                let p = c.wire_path(*w).unwrap();
                p[0] != at
                    && p[p.len() - 1] != at
                    && p.windows(2).any(|s| {
                        (s[0].x == at.x && s[1].x == at.x && s[0].y.min(s[1].y) <= at.y && at.y <= s[0].y.max(s[1].y))
                            || (s[0].y == at.y && s[1].y == at.y && s[0].x.min(s[1].x) <= at.x && at.x <= s[0].x.max(s[1].x))
                    })
            })
            .unwrap_or_else(|| panic!("no wire through {at:?}"));
        let bends = self.bends(bends);
        wire(&mut self.d, Endpoint::OnWire { wire: host, at }, pin(b.0, b.1), bends)
    }

    fn interact(&mut self, i: Interact) {
        self.d.interact(&i).unwrap();
    }

    fn finish(self) -> Circuit {
        let c = self.d.circuit().clone();
        let v = c.check();
        assert!(v.is_empty(), "{v:#?}");
        c
    }
}

const V5: (&str, Value) = ("vref", Value::Num(5.0));

// ------------------------------------------------------------------------------------------------ §9.1

/// Supply on the left, high-side switch on the top rail, potentiometer (B on the left, A to ground on the right,
/// wiper down), wiper node to ADC1 and to the RC filter R1/C1 read by ADC2; a ground symbol under each return.
/// RV1.B stands 9 units from S1 so that the pin values of RV1 (RV1 selected) leave the switch visible; C1 sits
/// midway between R1 and ADC2, clear of the value text of ADC1.
fn ref_pot_rc() -> Circuit {
    let mut b = Builder::new("ref-pot-rc");
    let v1 = b.add(Kind::DcSupply, (0, 8), Rot::R0, &[]);
    let s1 = b.add(Kind::Switch, (9, 0), Rot::R0, &[]);
    let rv1 = b.add(Kind::Potentiometer, (24, 0), Rot::R180, &[]);
    let r1 = b.add(Kind::Resistor, (31, 10), Rot::R0, &[]);
    let c1 = b.add(Kind::Capacitor, (41, 15), Rot::R90, &[]);
    let adc1 = b.add(Kind::AdcInput, (31, 17), Rot::R0, &[V5]);
    let adc2 = b.add(Kind::AdcInput, (48, 10), Rot::R0, &[V5]);
    let g: Vec<CompId> =
        [(0, 16), (31, 4), (31, 24), (41, 22), (48, 17)].iter().map(|p| b.add(Kind::Ground, *p, Rot::R0, &[])).collect();

    b.wire((v1, "pos"), (s1, "p1"), &[(0, 0)]);
    b.wire((s1, "p2"), (rv1, "b"), &[]);
    b.wire((rv1, "a"), (g[1], "gnd"), &[(31, 0)]);
    b.wire((v1, "neg"), (g[0], "gnd"), &[]);
    b.wire((rv1, "w"), (adc1, "in"), &[(24, 17)]);
    b.tap((24, 10), (r1, "p1"), &[]);
    b.wire((r1, "p2"), (adc2, "in"), &[]);
    b.tap((41, 10), (c1, "p1"), &[]);
    b.wire((c1, "p2"), (g[3], "gnd"), &[]);
    b.wire((adc1, "ref"), (g[2], "gnd"), &[]);
    b.wire((adc2, "ref"), (g[4], "gnd"), &[]);

    b.interact(Interact::SetSwitch { comp: s1, closed: true });
    b.interact(Interact::SetPot { comp: rv1, position: 0.3 });
    b.finish()
}

// ------------------------------------------------------------------------------------------------ §9.2

/// Left to right: ECU, its analog front end, MCP2551, termination and bus. Straight CAN_TX/CAN_RX lines to TXD/RXD.
/// Front end above them, a horizontal RC low-pass: A0 rises to the wiper node (RV1 above it, B fed by 3V3, A to
/// ground), R1 to the node read by A1, C1 to ground. The 5V rail runs over everything down to VDD; the 3V3 line nests
/// inside it and crosses its stub once beside the ECU, a crossing no layout avoids (3V3 lies between GND and 5V on
/// the ECU, A0 inside the loop 5V → VDD, TXD → CAN_TX). CANH/CANL go right past the pin values of U2 (U2 selected),
/// then step down: RT1 is the rung between them, in line with the bus CANH. A ground symbol under each return.
fn tp1_capteur_can() -> Circuit {
    let mut b = Builder::new("tp1-capteur-can");
    let u1 = b.add(Kind::EcuStm32, (0, 0), Rot::R0, &[]);
    let u2 = b.add(Kind::CanTransceiver, (35, 6), Rot::R0, &[]);
    let rv1 = b.add(Kind::Potentiometer, (10, -18), Rot::R180, &[]);
    let r1 = b.add(Kind::Resistor, (15, -11), Rot::R0, &[]);
    let c1 = b.add(Kind::Capacitor, (25, -8), Rot::R90, &[]);
    let rt1 = b.add(Kind::Termination, (52, 15), Rot::R180, &[]);
    let bus1 = b.add(Kind::CanBus, (64, 17), Rot::R0, &[]);
    let g: Vec<CompId> = [(-10, 7), (17, -16), (25, -3), (26, 11), (35, 17), (58, 23)]
        .iter()
        .map(|p| b.add(Kind::Ground, *p, Rot::R0, &[]))
        .collect();

    // Supplies: 5V outside (x = −12, y = −22), 3V3 inside (x = −10, y = −18).
    b.wire((u1, "v5"), (u2, "vdd"), &[(-12, -4), (-12, -22), (35, -22)]);
    b.wire((u1, "v3v3"), (rv1, "b"), &[(-10, -2), (-10, -18)]);
    b.wire((u1, "gnd"), (g[0], "gnd"), &[(-10, 4)]);
    // Sensor and RC filter, on the row y = −11.
    b.wire((u1, "a0"), (r1, "p1"), &[(10, -4), (10, -11)]);
    b.tap((10, -11), (rv1, "w"), &[]);
    b.wire((rv1, "a"), (g[1], "gnd"), &[(17, -18)]);
    b.wire((r1, "p2"), (c1, "p1"), &[]);
    b.tap((20, -11), (u1, "a1"), &[(20, -2)]);
    b.wire((c1, "p2"), (g[2], "gnd"), &[]);
    // CAN.
    b.wire((u1, "can_tx"), (u2, "txd"), &[]);
    b.wire((u1, "can_rx"), (u2, "rxd"), &[]);
    b.wire((u2, "rs"), (g[3], "gnd"), &[(26, 8)]);
    b.wire((u2, "vss"), (g[4], "gnd"), &[]);
    b.wire((u2, "canh"), (bus1, "canh"), &[(56, 4), (56, 15)]);
    b.wire((u2, "canl"), (bus1, "canl"), &[(48, 8), (48, 19)]);
    b.tap((56, 15), (rt1, "p1"), &[]);
    b.tap((48, 15), (rt1, "p2"), &[]);
    b.wire((bus1, "gnd"), (g[5], "gnd"), &[(58, 21)]);

    b.interact(Interact::SetPot { comp: rv1, position: 0.3 });
    b.finish()
}

// ------------------------------------------------------------------------------------------------ §8.6

/// Analog island: bench supply with its 10 µF directly across, top rail (high-side switch after the capacitor),
/// bottom ground rail. Branches: divider read by ADC1, potentiometer → RC → ADC2, push button pull-down read by
/// ADC3, second push button pull-down. 17 components, 17 junctions, 42 wires.
fn analog_tile(b: &mut Builder) {
    let v = b.add(Kind::DcSupply, (0, 10), Rot::R0, &[]);
    let s = b.add(Kind::Switch, (20, 2), Rot::R0, &[]);
    let c0 = b.add(Kind::Capacitor, (12, 9), Rot::R90, &[]);
    let r1 = b.add(Kind::Resistor, (28, 7), Rot::R90, &[]);
    let r2 = b.add(Kind::Resistor, (28, 16), Rot::R90, &[]);
    let adc1 = b.add(Kind::AdcInput, (35, 12), Rot::R0, &[]);
    let rv = b.add(Kind::Potentiometer, (44, 7), Rot::R90, &[]);
    let r3 = b.add(Kind::Resistor, (52, 7), Rot::R0, &[]);
    let adc2 = b.add(Kind::AdcInput, (63, 7), Rot::R0, &[]);
    let c2 = b.add(Kind::Capacitor, (57, 13), Rot::R90, &[("capacitance", Value::Choice("1\u{b5}".into()))]);
    let bp = b.add(Kind::PushButton, (70, 7), Rot::R90, &[]);
    let r4 = b.add(Kind::Resistor, (70, 16), Rot::R90, &[]);
    let adc3 = b.add(Kind::AdcInput, (77, 12), Rot::R0, &[]);
    let bp2 = b.add(Kind::PushButton, (84, 5), Rot::R90, &[("mode", Value::Choice("latching".into()))]);
    let r5 = b.add(Kind::Resistor, (84, 14), Rot::R90, &[]);
    let g1 = b.add(Kind::Ground, (6, 30), Rot::R0, &[]);
    let g2 = b.add(Kind::Ground, (80, 30), Rot::R0, &[]);

    // Rails.
    b.wire((v, "pos"), (s, "p1"), &[(0, 2)]);
    b.wire((s, "p2"), (bp2, "p1"), &[]);
    b.wire((v, "neg"), (r5, "p2"), &[(0, 26), (84, 26)]);
    // Series links.
    b.wire((r1, "p2"), (r2, "p1"), &[]);
    b.wire((rv, "w"), (r3, "p1"), &[]);
    b.wire((r3, "p2"), (adc2, "in"), &[]);
    b.wire((bp, "p2"), (r4, "p1"), &[]);
    b.wire((bp2, "p2"), (r5, "p1"), &[]);
    // Taps on the top rail, then inside the branches.
    b.tap((12, 2), (c0, "p1"), &[]);
    b.tap((28, 2), (r1, "p1"), &[]);
    b.tap((44, 2), (rv, "a"), &[]);
    b.tap((70, 2), (bp, "p1"), &[]);
    b.tap((28, 12), (adc1, "in"), &[]);
    b.tap((57, 7), (c2, "p1"), &[]);
    b.tap((70, 12), (adc3, "in"), &[]);
    // Returns to the ground rail, and two ground symbols on it.
    for (x, p) in [(12, (c0, "p2")), (28, (r2, "p2")), (35, (adc1, "ref")), (44, (rv, "b")), (57, (c2, "p2"))] {
        b.tap((x, 26), p, &[]);
    }
    for (x, p) in [(63, (adc2, "ref")), (70, (r4, "p2")), (77, (adc3, "ref")), (6, (g1, "gnd")), (80, (g2, "gnd"))] {
        b.tap((x, 26), p, &[]);
    }
    b.interact(Interact::SetPot { comp: rv, position: 0.25 });
    b.interact(Interact::SetSwitch { comp: s, closed: true });
}

/// CAN island: bench supply (10 µF directly across) feeding the MCP2551, ECU on USB driving TXD and reading RXD,
/// 120 Ω termination and the bus model. 12 components, 1 junction, 15 wires.
fn can_tile(b: &mut Builder) {
    let v = b.add(Kind::DcSupply, (-24, -2), Rot::R0, &[]);
    let c0 = b.add(Kind::Capacitor, (-16, -3), Rot::R90, &[]);
    let u1 = b.add(Kind::EcuStm32, (0, 0), Rot::R0, &[]);
    let u2 = b.add(Kind::CanTransceiver, (40, 20), Rot::R0, &[]);
    let rt = b.add(Kind::Termination, (52, 19), Rot::R90, &[]);
    let bus = b.add(Kind::CanBus, (62, 20), Rot::R0, &[("remote_nodes", Value::Int(2))]);
    let g: Vec<CompId> = [(-24, 6), (-16, 4), (-10, 8), (30, 26), (40, 32), (56, 28)]
        .iter()
        .map(|p| b.add(Kind::Ground, *p, Rot::R0, &[]))
        .collect();
    b.wire((v, "pos"), (u2, "vdd"), &[(-24, -10), (40, -10)]);
    b.tap((-16, -10), (c0, "p1"), &[]);
    b.wire((v, "neg"), (g[0], "gnd"), &[]);
    b.wire((c0, "p2"), (g[1], "gnd"), &[]);
    b.wire((u1, "gnd"), (g[2], "gnd"), &[(-10, 4)]);
    b.wire((u1, "can_tx"), (u2, "txd"), &[(11, 2), (11, 16)]);
    b.wire((u1, "can_rx"), (u2, "rxd"), &[(10, 4), (10, 18)]);
    b.wire((u2, "rs"), (g[3], "gnd"), &[(30, 22)]);
    b.wire((u2, "vss"), (g[4], "gnd"), &[]);
    b.wire((u2, "canh"), (rt, "p1"), &[(49, 18), (49, 16)]);
    b.wire((u2, "canl"), (rt, "p2"), &[]);
    b.wire((rt, "p1"), (bus, "canh"), &[(55, 16), (55, 18)]);
    b.wire((rt, "p2"), (bus, "canl"), &[]);
    b.wire((bus, "gnd"), (g[5], "gnd"), &[(56, 24)]);
}

/// Nine analog islands on a 3 × 3 grid, the CAN island below: about 300 × 180 units.
fn perf_150_300() -> Circuit {
    let mut b = Builder::new("perf-150-300");
    for row in 0..3 {
        for col in 0..3 {
            b.origin = pt(col * 100, row * 40);
            analog_tile(&mut b);
        }
    }
    b.origin = pt(30, 140);
    can_tile(&mut b);
    b.finish()
}

fn generators() -> [(&'static str, fn() -> Circuit); 3] {
    [("ref-pot-rc", ref_pot_rc), ("tp1-capteur-can", tp1_capteur_can), ("perf-150-300", perf_150_300)]
}

// ------------------------------------------------------------------------------------------------ files

#[test]
fn committed_demo_files_equal_their_generators() {
    for (id, build) in generators() {
        let demo = demos().iter().find(|d| d.id == id).expect("demo listed");
        assert!(
            demo.json == persist::save(&build()),
            "demos/{id}.json differs from its generator: run `cargo test -p n7rt-circuit --test demos -- --ignored regenerate_demo_files`"
        );
    }
}

#[test]
#[ignore = "writes demos/*.json; run after a deliberate change of a generator"]
fn regenerate_demo_files() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("demos");
    for (id, build) in generators() {
        std::fs::write(dir.join(format!("{id}.json")), persist::save(&build())).unwrap();
    }
}

#[test]
fn demos_are_listed_valid_and_canonical() {
    let ids: Vec<(&str, bool)> = demos().iter().map(|d| (d.id, d.hidden)).collect();
    assert_eq!(ids, vec![("ref-pot-rc", false), ("tp1-capteur-can", false), ("perf-150-300", true)]);
    for d in demos() {
        assert!(!d.title_fr.is_empty() && !d.summary_fr.is_empty());
        let c = persist::load(d.json).unwrap_or_else(|v| panic!("{}: {v:#?}", d.id));
        assert!(c.check().is_empty(), "{}", d.id);
        assert_eq!(c.name(), d.id);
        assert_eq!(persist::save(&c), d.json, "{} is not canonical", d.id);
        // Clean drawing: no junction of degree 2 (only real T's), every junction joins exactly three wires.
        for j in c.components().values().filter(|k| k.kind == Kind::Junction) {
            let jp = PinRef { comp: j.id, pin: "j".into() };
            let degree = c.wires().values().filter(|w| w.a == jp || w.b == jp).count();
            assert_eq!(degree, 3, "{}: junction J{}", d.id, j.id.0);
        }
    }
}

// ------------------------------------------------------------------------------------------------ §9 netlists

fn load(id: &str) -> Circuit {
    persist::load(demos().iter().find(|d| d.id == id).unwrap().json).unwrap()
}

fn by_label(c: &Circuit, label: &str) -> CompId {
    c.components().values().find(|k| k.label == label).unwrap_or_else(|| panic!("no {label}")).id
}

fn param(c: &Circuit, label: &str, name: &str) -> Value {
    c.components()[&by_label(c, label)].params[name].clone()
}

fn state(c: &Circuit, label: &str, name: &str) -> Value {
    c.state(by_label(c, label)).unwrap()[name].clone()
}

/// Nets as sets of "label.pin" of the designated components (junctions and ground symbols left out); nets made only
/// of those are dropped. The ground net is marked "GND".
fn nets(c: &Circuit, nl: &Netlist) -> BTreeSet<BTreeSet<String>> {
    nl.nets
        .iter()
        .filter_map(|n| {
            let mut s: BTreeSet<String> = n
                .pins
                .iter()
                .filter_map(|p| {
                    let comp = &c.components()[&p.comp];
                    (!comp.label.is_empty()).then(|| format!("{}.{}", comp.label, p.pin))
                })
                .collect();
            if n.ground {
                s.insert("GND".into());
            }
            (!s.is_empty()).then_some(s)
        })
        .collect()
}

fn expected(list: &[&[&str]]) -> BTreeSet<BTreeSet<String>> {
    list.iter().map(|n| n.iter().map(|s| s.to_string()).collect()).collect()
}

#[test]
fn ref_pot_rc_follows_9_1() {
    let c = load("ref-pot-rc");
    let nl = c.netlist();
    assert_eq!(
        nets(&c, &nl),
        expected(&[
            &["V1.pos", "S1.p1"],
            &["S1.p2", "RV1.b"],
            &["GND", "V1.neg", "RV1.a", "C1.p2", "ADC1.ref", "ADC2.ref"],
            &["RV1.w", "ADC1.in", "R1.p1"],
            &["R1.p2", "C1.p1", "ADC2.in"],
        ])
    );
    assert_eq!(nl.net_of(&PinRef { comp: by_label(&c, "RV1"), pin: "w".into() }).unwrap().name, "RV1.W");
    assert_eq!(nl.net_of(&PinRef { comp: by_label(&c, "R1"), pin: "p2".into() }).unwrap().name, "R1.2");
    // Parameters (catalog defaults except the ADC reference) and initial states.
    for (label, name, v) in [
        ("V1", "voltage", Value::Num(5.0)),
        ("V1", "current_limit", Value::Num(0.1)),
        ("V1", "r_out", Value::Num(0.01)),
        ("S1", "r_on", Value::Num(0.01)),
        ("S1", "r_off", Value::Num(1e9)),
        ("RV1", "resistance", Value::Num(1e4)),
        ("R1", "resistance", Value::Num(1e4)),
        ("C1", "capacitance", Value::Num(1e-5)),
        ("C1", "polarized", Value::Bool(false)),
    ] {
        assert_eq!(param(&c, label, name), v, "{label}.{name}");
    }
    for adc in ["ADC1", "ADC2"] {
        assert_eq!(param(&c, adc, "vref"), Value::Num(5.0));
        assert_eq!(param(&c, adc, "bits"), Value::Int(12));
        assert_eq!(param(&c, adc, "r_in"), Value::Num(f64::INFINITY));
        assert_eq!(param(&c, adc, "sample_rate"), Value::Int(1000));
    }
    assert_eq!(state(&c, "V1", "on"), Value::Bool(true));
    assert_eq!(state(&c, "S1", "closed"), Value::Bool(true));
    assert_eq!(state(&c, "RV1", "position"), Value::Num(0.3));
    assert!(c.components().values().any(|k| k.kind == Kind::Ground));
}

#[test]
fn tp1_capteur_can_follows_9_2() {
    let c = load("tp1-capteur-can");
    let nl = c.netlist();
    assert_eq!(
        nets(&c, &nl),
        expected(&[
            &["U1.v3v3", "RV1.b"],
            &["U1.v5", "U2.vdd"],
            &["GND", "U1.gnd", "RV1.a", "C1.p2", "U2.vss", "U2.rs", "BUS1.gnd"],
            &["RV1.w", "U1.a0", "R1.p1"],
            &["R1.p2", "C1.p1", "U1.a1"],
            &["U1.can_tx", "U2.txd"],
            &["U2.rxd", "U1.can_rx"],
            &["U2.canh", "RT1.p1", "BUS1.canh"],
            &["U2.canl", "RT1.p2", "BUS1.canl"],
            &["U2.vref"],
        ])
    );
    // A ground symbol on U1.gnd (directly wired to it).
    let u1 = by_label(&c, "U1");
    assert!(c.wires().values().any(|w| {
        let (a, b) = (&c.components()[&w.a.comp], &c.components()[&w.b.comp]);
        (w.a == PinRef { comp: u1, pin: "gnd".into() } && b.kind == Kind::Ground)
            || (w.b == PinRef { comp: u1, pin: "gnd".into() } && a.kind == Kind::Ground)
    }));
    assert_eq!((c.components()[&u1].kind, c.components()[&by_label(&c, "U2")].kind), (Kind::EcuStm32, Kind::CanTransceiver));
    for (label, name, v) in [
        ("U1", "adc_bits", Value::Choice("12".into())),
        ("U1", "can_mode", Value::Choice("can".into())),
        ("RV1", "resistance", Value::Num(1e4)),
        ("R1", "resistance", Value::Num(1e4)),
        ("C1", "capacitance", Value::Num(1e-5)),
        ("RT1", "resistance", Value::Num(120.0)),
        ("BUS1", "remote_nodes", Value::Int(1)),
        ("BUS1", "remote_termination", Value::Bool(true)),
    ] {
        assert_eq!(param(&c, label, name), v, "{label}.{name}");
    }
    assert_eq!(state(&c, "U1", "usb"), Value::Bool(true));
    assert_eq!(state(&c, "RV1", "position"), Value::Num(0.3));
}

#[test]
fn perf_fixture_has_the_size_and_islands_of_8_6() {
    let c = load("perf-150-300");
    let comps = c.components().values().filter(|k| k.kind != Kind::Junction).count();
    let junctions = c.components().len() - comps;
    assert!(comps >= 150 && c.wires().len() >= 300 && junctions >= 50, "{comps} {} {junctions}", c.wires().len());
    // About 300 × 200 units.
    let pins: Vec<Pt> = c
        .components()
        .values()
        .flat_map(|k| kind_def(k.kind).pins.iter().map(|p| c.pin_pos(&PinRef { comp: k.id, pin: p.id.into() }).unwrap()))
        .collect();
    let (x0, x1) = (pins.iter().map(|p| p.x).min().unwrap(), pins.iter().map(|p| p.x).max().unwrap());
    let (y0, y1) = (pins.iter().map(|p| p.y).min().unwrap(), pins.iter().map(|p| p.y).max().unwrap());
    assert!(x1 - x0 <= 320 && y1 - y0 <= 220, "extent {}×{}", x1 - x0, y1 - y0);

    // Islands: nets other than GND joined through the components that touch them (the solver's islands share
    // only the ground). Each holds one 10 mΩ supply with a 10 µF capacitor directly across its terminals.
    let nl = c.netlist();
    let net_index = |p: &PinRef| nl.nets.iter().position(|n| n.pins.binary_search(p).is_ok()).unwrap();
    let mut parent: Vec<usize> = (0..nl.nets.len()).collect();
    fn find(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            i = parent[i];
        }
        i
    }
    for k in c.components().values() {
        let ids: Vec<usize> = kind_def(k.kind)
            .pins
            .iter()
            .map(|p| net_index(&PinRef { comp: k.id, pin: p.id.into() }))
            .filter(|i| !nl.nets[*i].ground)
            .collect();
        for w in ids.windows(2) {
            let (a, b) = (find(&mut parent, w[0]), find(&mut parent, w[1]));
            parent[a.max(b)] = a.min(b);
        }
    }
    let mut islands: BTreeMap<usize, Vec<CompId>> = BTreeMap::new();
    for k in c.components().values().filter(|k| k.kind == Kind::DcSupply) {
        let pos = net_index(&PinRef { comp: k.id, pin: "pos".into() });
        islands.entry(find(&mut parent, pos)).or_default().push(k.id);
        assert_eq!(k.params["r_out"], Value::Num(0.01));
        assert!(nl.nets[net_index(&PinRef { comp: k.id, pin: "neg".into() })].ground);
        let across = c.components().values().filter(|cap| {
            cap.kind == Kind::Capacitor
                && cap.params["capacitance"] == Value::Num(1e-5)
                && net_index(&PinRef { comp: cap.id, pin: "p1".into() }) == pos
                && nl.nets[net_index(&PinRef { comp: cap.id, pin: "p2".into() })].ground
        });
        assert_eq!(across.count(), 1, "supply {} without its 10 µF", k.label);
    }
    assert_eq!(islands.len(), 10);
    assert!(islands.values().all(|s| s.len() == 1));
    // The fixture also carries the heavy parts: ECU, transceiver, bus, termination.
    for kind in [Kind::EcuStm32, Kind::CanTransceiver, Kind::CanBus, Kind::Termination] {
        assert!(c.components().values().any(|k| k.kind == kind), "{kind:?}");
    }
}
