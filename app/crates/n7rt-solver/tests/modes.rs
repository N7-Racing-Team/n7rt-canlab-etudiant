//! Piecewise-linear modes (contract §5.4, §9.3): one-quadrant supply CC / CV / REV and hysteresis gates.

use n7rt_solver::{Element, Gate, IslandStatus, Network, NodeClass, SolveError, Solver, SupplyMode};

fn r(a: usize, b: usize, r: f64) -> Element {
    Element::Resistor { a, b, r, leakage: false, gate: None }
}

fn supply(pos: usize, neg: usize, v: f64, r_out: f64, i_lim: Option<f64>, key: u64) -> Element {
    Element::Supply { pos, neg, v, r_out, i_lim, key, gate: None }
}

fn net(nodes: usize, elements: Vec<Element>) -> Network {
    Network { nodes, ground: Some(0), elements, gates: vec![] }
}

fn loaded(n: &Network) -> Solver {
    let mut s = Solver::new();
    s.load(n).expect("valid network");
    s
}

#[test]
fn constant_current_ramp_then_constant_voltage() {
    // 5 V / 0.1 A / 10 mΩ straight on 1 mF: v = i_lim·t/C = 0.1 V per frame until V − r_out·i_lim = 4.999 V.
    let (c, i_lim) = (1e-3, 0.1);
    let mut s = loaded(&net(2, vec![supply(1, 0, 5.0, 0.01, Some(i_lim), 1), Element::Capacitor { a: 1, b: 0, c, key: 2 }]));
    for frame in 1..=49 {
        s.step_frame().unwrap();
        let expected = i_lim * (frame as f64 * 1e-3) / c;
        let v = s.capacitor_state(2).unwrap().0;
        assert!((v - expected).abs() <= 1e-12 * 5.0, "frame {frame}: {v} vs {expected}");
        assert_eq!(s.supply_mode(0), Some(SupplyMode::Cc), "frame {frame}");
        assert!((s.element_current(0).unwrap() - i_lim).abs() <= 1e-15);
        // In CC the capacitor has no resistive path: static step of 1 ms after the first frame.
        if frame > 1 {
            assert_eq!(s.island_dt_ns(0), Some(1_000_000));
        }
    }
    s.step_frame().unwrap();
    assert_eq!(s.supply_mode(0), Some(SupplyMode::Cv), "4.9 V + 0.1 V crosses 4.999 V: CV");
    let v = s.capacitor_state(2).unwrap().0;
    assert!((4.999..=5.0).contains(&v), "{v}");
    assert!(s.element_current(0).unwrap() <= i_lim);
    for _ in 0..10 {
        s.step_frame().unwrap();
        assert_eq!(s.supply_mode(0), Some(SupplyMode::Cv));
    }
    assert!((s.capacitor_state(2).unwrap().0 - 5.0).abs() <= 1e-9);
}

#[test]
fn load_band_stays_in_constant_voltage() {
    let (v, r_out, i_lim) = (5.0, 0.01, 0.1);
    let with_load = |rl: f64| net(2, vec![supply(1, 0, v, r_out, Some(i_lim), 1), r(1, 0, rl)]);
    // Band R_L ∈ (V/i_lim − r_out ; V/i_lim] = (49.99 ; 50]: CV, from a fresh start and from CC.
    for rl in [49.990_001, 49.995, 50.0] {
        for from_cc in [false, true] {
            let mut s = Solver::new();
            if from_cc {
                s.load(&with_load(10.0)).unwrap();
                s.step_frame().unwrap();
                assert_eq!(s.supply_mode(0), Some(SupplyMode::Cc));
                assert!((s.node(1).volts - i_lim * 10.0).abs() <= 1e-12);
            }
            s.load(&with_load(rl)).unwrap();
            s.step_frame().unwrap();
            assert_eq!(s.supply_mode(0), Some(SupplyMode::Cv), "R_L = {rl}, from CC: {from_cc}");
            let x = v * rl / (rl + r_out);
            assert!((s.node(1).volts - x).abs() <= 1e-12 * x);
            assert!(s.element_current(1).unwrap() < i_lim);
        }
    }
    // Just below the band: limited.
    let mut s = loaded(&with_load(49.98));
    s.step_frame().unwrap();
    assert_eq!(s.supply_mode(0), Some(SupplyMode::Cc));
    assert!((s.node(1).volts - i_lim * 49.98).abs() <= 1e-12 * 5.0);
}

#[test]
fn closed_switch_on_a_limited_supply_publishes_the_limit() {
    // 5 V / 1 A / 10 mΩ shorted by a closed switch of 10 mΩ, static island: 1 A, never the unlimited 250 A.
    let network = net(2, vec![supply(1, 0, 5.0, 0.01, Some(1.0), 1), r(1, 0, 0.01)]);
    let mut s = loaded(&network);
    s.preview().unwrap();
    assert_eq!(s.supply_mode(0), Some(SupplyMode::Cc));
    assert_eq!(s.element_current(0), Some(1.0));
    s.step_frame().unwrap();
    assert_eq!(s.island_dt_ns(0), Some(1_000_000));
    assert_eq!(s.supply_mode(0), Some(SupplyMode::Cc));
    assert_eq!(s.element_current(0), Some(1.0));
    assert!((s.element_current(1).unwrap() - 1.0).abs() <= 1e-12);
    assert!((s.node(1).volts - 0.01).abs() <= 1e-15);
}

#[test]
fn opposed_supplies_reach_stable_modes() {
    // 5 V / 0.1 Ω limited 0.5 A against 3.3 V / 0.05 Ω limited 1.3 A on the same net, no load. Neither can sink
    // current: the 5 V supply holds the net in CV at no current and the 3.3 V one is back-driven (REV).
    let network = net(2, vec![supply(1, 0, 5.0, 0.1, Some(0.5), 1), supply(1, 0, 3.3, 0.05, Some(1.3), 2)]);
    let mut s = loaded(&network);
    for frame in 1..=1000 {
        s.step_frame().unwrap();
        assert_eq!(s.supply_mode(0), Some(SupplyMode::Cv), "frame {frame}");
        assert_eq!(s.supply_mode(1), Some(SupplyMode::Rev), "frame {frame}");
        assert_eq!(s.node(1).volts, 5.0);
        assert_eq!((s.element_current(0), s.element_current(1)), (Some(0.0), Some(0.0)));
    }
    assert!(s.stats().mode_solves_max <= 4 * 2 + 4);

    // With a 4 Ω load the 5 V supply limits at 0.5 A and the 3.3 V one supplies the rest in CV.
    let mut s = loaded(&net(2, vec![supply(1, 0, 5.0, 0.1, Some(0.5), 1), supply(1, 0, 3.3, 0.05, Some(1.3), 2), r(1, 0, 4.0)]));
    for _ in 0..1000 {
        s.step_frame().unwrap();
        assert_eq!((s.supply_mode(0), s.supply_mode(1)), (Some(SupplyMode::Cc), Some(SupplyMode::Cv)));
    }
    // x = (3.3/0.05 + 0.5) / (1/0.05 + 1/4)
    let x = (3.3 / 0.05 + 0.5) / (1.0 / 0.05 + 0.25);
    assert!((s.node(1).volts - x).abs() <= 1e-12 * x);
}

#[test]
fn opposed_supplies_settle_from_constant_current_whatever_the_key_order() {
    // Same pair, the 5 V one first limited by a 4 Ω load that is then removed: stored modes (CC, CV). With the
    // 3.3 V supply checked first, it goes REV, which unlinks the 5 V supply still in CC; that one must return to CV
    // before anything is decided on the then artificial potential of the net.
    for (k5, k33) in [(1, 2), (2, 1)] {
        let build = |load: bool| {
            let mut e = vec![supply(1, 0, 5.0, 0.1, Some(0.5), k5), supply(1, 0, 3.3, 0.05, Some(1.3), k33)];
            if load {
                e.push(r(1, 0, 4.0));
            }
            net(2, e)
        };
        let mut s = loaded(&build(true));
        s.step_frame().unwrap();
        assert_eq!((s.supply_mode(0), s.supply_mode(1)), (Some(SupplyMode::Cc), Some(SupplyMode::Cv)), "keys {k5} {k33}");
        s.load(&build(false)).unwrap();
        for frame in 1..=1000 {
            assert_eq!(s.step_frame(), Ok(()), "keys {k5} {k33}, frame {frame}");
            assert_eq!((s.supply_mode(0), s.supply_mode(1)), (Some(SupplyMode::Cv), Some(SupplyMode::Rev)));
            assert_eq!(s.node(1).volts, 5.0);
        }
        assert!(s.stats().mode_solves_max <= 4 * 2 + 4);
    }
}

#[test]
fn back_driven_supply_left_alone_returns_to_constant_voltage() {
    // A capacitor (2 → 1) is charged to 5 V while a 3.3 V supply on node 2 is back-driven (REV) by a 5 V one.
    let charge = net(
        3,
        vec![
            supply(2, 0, 3.3, 0.05, None, 1),
            supply(2, 0, 5.0, 0.1, None, 2),
            Element::Capacitor { a: 2, b: 1, c: 1e-6, key: 3 },
            r(1, 0, 100.0),
        ],
    );
    let mut s = loaded(&charge);
    for _ in 0..20 {
        s.step_frame().unwrap();
    }
    assert_eq!(s.supply_mode(0), Some(SupplyMode::Rev));
    let (vc, _) = s.capacitor_state(3).unwrap();
    assert!((vc - 5.0).abs() < 1e-9, "{vc}");

    // The 5 V supply and the resistor are removed: the capacitor floats on the REV supply. Its 5 V would put the
    // supply output 5 V above node 1, the artefact 0 V node: no decision on that. Nothing can flow through the
    // supply, which holds node 2 at 3.3 V in CV, and the far plate sits 5 V below, both measurable.
    s.load(&net(3, vec![supply(2, 0, 3.3, 0.05, None, 1), Element::Capacitor { a: 2, b: 1, c: 1e-6, key: 3 }])).unwrap();
    for publish in ["preview", "frame"] {
        if publish == "preview" {
            s.preview().unwrap();
        } else {
            s.step_frame().unwrap();
        }
        assert_eq!(s.supply_mode(0), Some(SupplyMode::Cv), "{publish}");
        assert_eq!((s.node(1).class, s.node(2).class), (NodeClass::Referenced, NodeClass::Referenced));
        assert!((s.node(2).volts - 3.3).abs() <= 1e-12, "{publish}: {}", s.node(2).volts);
        assert!((s.node(1).volts - (3.3 - vc)).abs() <= 1e-12, "{publish}: {}", s.node(1).volts);
        assert!(s.element_current(0).unwrap().abs() <= 1e-9);
    }
}

#[test]
fn supply_with_both_terminals_on_one_node_is_limited() {
    // On an internal node, and with both terminals on ground (internal island without unknowns).
    for (pos, neg, nodes) in [(1, 1, 2), (0, 0, 1)] {
        let mut s = loaded(&net(nodes, vec![supply(pos, neg, 5.0, 0.01, Some(0.25), 9)]));
        s.step_frame().unwrap();
        assert_eq!(s.supply_mode(0), Some(SupplyMode::Cc), "pos = neg = {pos}");
        assert_eq!(s.element_current(0), Some(0.25));
    }
    // Without a limit it stays CV and the short-circuit current flows.
    let mut s = loaded(&net(1, vec![supply(0, 0, 5.0, 0.01, None, 9)]));
    s.preview().unwrap();
    assert_eq!(s.supply_mode(0), Some(SupplyMode::Cv));
    assert!((s.element_current(0).unwrap() - 500.0).abs() <= 1e-12 * 500.0);
}

/// Control source u on node 1; gate (on above 2 V, off below 1 V) on a 100 Ω load of a 5 V / 100 Ω source on node 2.
fn gate_network(u: f64) -> Network {
    Network {
        nodes: 3,
        ground: Some(0),
        elements: vec![
            Element::VSource { pos: 1, neg: 0, v: u, r_out: 1.0, gate: None },
            Element::VSource { pos: 2, neg: 0, v: 5.0, r_out: 100.0, gate: None },
            Element::Resistor { a: 2, b: 0, r: 100.0, leakage: false, gate: Some(0) },
        ],
        gates: vec![Gate { cp: 1, cn: 0, on_above: 2.0, off_below: 1.0, key: 10 }],
    }
}

#[test]
fn gate_hysteresis() {
    let mut s = Solver::new();
    let sequence = [
        (0.0, false),
        (1.5, false),
        (2.0, false),
        (2.5, true),
        (1.5, true),
        (1.0, true),
        (0.999, false),
        (1.999_999, false),
        (2.000_001, true),
    ];
    for (u, on) in sequence {
        s.load(&gate_network(u)).unwrap();
        // The preview decides from the stored state without storing; the frame then commits the same decision.
        let digest = s.state_digest();
        s.preview().unwrap();
        assert_eq!(s.gate_on(0), Some(on), "preview, u = {u}");
        assert_eq!(s.state_digest(), digest);
        s.step_frame().unwrap();
        assert_eq!(s.gate_on(0), Some(on), "frame, u = {u}");
        let (load_current, v2) = if on { (0.025, 2.5) } else { (0.0, 5.0) };
        assert!((s.element_current(2).unwrap() - load_current).abs() <= 1e-15, "u = {u}");
        assert!((s.node(2).volts - v2).abs() <= 1e-14, "u = {u}");
    }
}

#[test]
fn gate_is_not_decided_on_artificial_potentials() {
    // A 5 V source floating between nodes 3 (fixed at 0 V, artefact) and 4 (5 V above it).
    let floating = |cn: usize| Network {
        nodes: 5,
        ground: Some(0),
        elements: vec![
            Element::VSource { pos: 4, neg: 3, v: 5.0, r_out: 1.0, gate: None },
            Element::VSource { pos: 2, neg: 0, v: 5.0, r_out: 100.0, gate: None },
            Element::Resistor { a: 2, b: 0, r: 100.0, leakage: false, gate: Some(0) },
        ],
        gates: vec![Gate { cp: 4, cn, on_above: 2.0, off_below: 1.0, key: 10 }],
    };
    // Control referred to ground: different K groups, the gate stays off.
    let mut s = loaded(&floating(0));
    s.step_frame().unwrap();
    assert_eq!(s.node(4).class, NodeClass::Relative);
    assert_ne!(s.k_group(4), s.k_group(0));
    assert_eq!(s.gate_on(0), Some(false));
    assert_eq!(s.island_status(0), IslandStatus::Solved { ill_conditioned: false });
    // Control between the two floating nodes: a valid difference of 5 V, the gate turns on.
    let mut s = loaded(&floating(3));
    s.step_frame().unwrap();
    assert_eq!(s.k_group(4), s.k_group(3));
    assert_eq!(s.gate_on(0), Some(true));
    assert!((s.node(2).volts - 2.5).abs() <= 1e-14);
}

/// 5 V / 100 Ω on node 1, a 1 Ω load gated by V(1) (on above `on_above`, off below 1 V), and 1 MΩ → 1 µF on node 2.
fn relaxation_network(on_above: f64) -> Network {
    Network {
        nodes: 3,
        ground: Some(0),
        elements: vec![
            Element::VSource { pos: 1, neg: 0, v: 5.0, r_out: 100.0, gate: None },
            Element::Resistor { a: 1, b: 0, r: 1.0, leakage: false, gate: Some(0) },
            r(1, 2, 1e6),
            Element::Capacitor { a: 2, b: 0, c: 1e-6, key: 1 },
        ],
        gates: vec![Gate { cp: 1, cn: 0, on_above, off_below: 1.0, key: 2 }],
    }
}

#[test]
fn mode_cycle_publishes_nothing_and_keeps_the_state() {
    // Thresholds out of reach: the capacitor charges with the gate off.
    let mut s = loaded(&relaxation_network(100.0));
    for _ in 0..20 {
        s.step_frame().unwrap();
    }
    assert_eq!(s.gate_on(0), Some(false));
    let stored = s.capacitor_state(1).unwrap();
    assert!(stored.0 > 0.05, "{stored:?}");

    // Off, V(1) ≈ 5 V turns the gate on; on, V(1) ≈ 5/101 V turns it off: no coherent mode vector exists. The loop
    // stops after 4·m + 4 = 8 solves (m = 1).
    s.load(&relaxation_network(2.0)).unwrap();
    let t = s.time_ns();
    assert_eq!(s.step_frame(), Err(SolveError { island: 0, status: IslandStatus::ModeCycle }));
    assert_eq!(s.island_status(0), IslandStatus::ModeCycle);
    assert_eq!(s.stats().mode_solves_max, 8);
    assert_eq!(s.capacitor_state(1), Some(stored));
    assert_eq!((s.node(1).class, s.k_group(1)), (NodeClass::Unused, None));
    assert_eq!((s.element_current(1), s.gate_on(0)), (None, None));
    assert_eq!(s.time_ns(), t + 1_000_000);
    assert_eq!(s.preview(), Err(SolveError { island: 0, status: IslandStatus::ModeCycle }));
    assert_eq!(s.capacitor_state(1), Some(stored));

    // Back to the first network: the charge resumes from the kept state.
    s.load(&relaxation_network(100.0)).unwrap();
    s.step_frame().unwrap();
    assert_eq!(s.gate_on(0), Some(false));
    assert!(s.capacitor_state(1).unwrap().0 > stored.0);
}
