//! Classification, islands, invalid states, load() and unpowered resistance (contract §5.1, §5.2, §5.3, §5.7, §5.9).

use n7rt_solver::{
    unpowered_resistance, Element, Gate, IslandStatus, Network, NetworkError, NodeClass, SolveError, Solver,
};

fn r(a: usize, b: usize, r: f64) -> Element {
    Element::Resistor { a, b, r, leakage: false, gate: None }
}

fn leak(a: usize, b: usize, r: f64) -> Element {
    Element::Resistor { a, b, r, leakage: true, gate: None }
}

fn vsrc(pos: usize, neg: usize, v: f64, r_out: f64) -> Element {
    Element::VSource { pos, neg, v, r_out, gate: None }
}

fn cap(a: usize, c: f64, key: u64) -> Element {
    Element::Capacitor { a, b: 0, c, key }
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
fn node_classes() {
    let network = net(
        7,
        vec![
            vsrc(1, 0, 5.0, 0.01),
            r(1, 2, 1e3),
            r(2, 0, 1e3),
            // Source floating between 3 and 4: differences valid, absolute values not.
            vsrc(3, 4, 2.0, 0.01),
            r(3, 4, 1e3),
            // Node 5 only reached by a leakage path.
            leak(1, 5, 1e8),
            // Node 6: no element.
        ],
    );
    let mut s = loaded(&network);
    assert_eq!(s.node(2).class, NodeClass::Unused, "nothing published before the first solve");
    s.step_frame().unwrap();
    let class = |n| s.node(n).class;
    assert_eq!([class(0), class(1), class(2)], [NodeClass::Referenced; 3]);
    assert_eq!([class(3), class(4)], [NodeClass::Relative; 2]);
    assert_eq!(class(5), NodeClass::Floating);
    assert_eq!(class(6), NodeClass::Unused);
    assert!((s.node(2).volts - 2.5 * 2e3 / (2e3 + 0.01)).abs() <= 1e-12);
    let diff = s.node(3).volts - s.node(4).volts;
    assert!((diff - 2.0 * 1e3 / (1e3 + 0.01)).abs() <= 1e-12, "{diff}");
    assert_eq!(s.k_group(1), s.k_group(0));
    assert_eq!(s.k_group(2), s.k_group(0));
    assert_eq!(s.k_group(3), s.k_group(4));
    assert_ne!(s.k_group(3), s.k_group(0));
    assert_ne!(s.k_group(5), s.k_group(1));
    assert_eq!(s.k_group(6), None);
    assert_eq!((s.island_of(0), s.island_of(6)), (None, None));
    // The leakage path conducts: node 5 sits at V(1) through 100 MΩ without current.
    assert!((s.node(5).volts - s.node(1).volts).abs() <= 1e-12);
}

#[test]
fn node_of_a_gate_alone_is_unused() {
    // Node 2 only controls the gate: it belongs to no element, so it is Unused and the gate, whose control cannot
    // be referred to ground, stays off.
    let network = Network {
        nodes: 3,
        ground: Some(0),
        elements: vec![vsrc(1, 0, 5.0, 100.0), Element::Resistor { a: 1, b: 0, r: 100.0, leakage: false, gate: Some(0) }],
        gates: vec![Gate { cp: 2, cn: 0, on_above: 2.0, off_below: 1.0, key: 1 }],
    };
    let mut s = loaded(&network);
    s.step_frame().unwrap();
    assert_eq!(s.node(2).class, NodeClass::Unused);
    assert_eq!((s.k_group(2), s.island_of(2)), (None, None));
    assert_eq!(s.gate_on(0), Some(false));
    assert_eq!(s.node(1), n7rt_solver::NodeValue { class: NodeClass::Referenced, volts: 5.0 });
}

#[test]
fn network_without_ground_gives_exact_differences() {
    // No ground at all: a 2 V source across 1 kΩ between nodes 0 and 1, and a lone resistor between 2 and 3.
    let network = Network {
        nodes: 4,
        ground: None,
        elements: vec![vsrc(1, 0, 2.0, 0.01), r(1, 0, 1e3), r(2, 3, 1e3)],
        gates: vec![],
    };
    let mut s = loaded(&network);
    s.step_frame().unwrap();
    for n in 0..4 {
        assert_eq!(s.node(n).class, NodeClass::Relative, "node {n}");
    }
    let diff = s.node(1).volts - s.node(0).volts;
    assert!((diff - 2.0 * 1e3 / (1e3 + 0.01)).abs() <= 1e-12, "{diff}");
    assert_eq!(s.node(2).volts - s.node(3).volts, 0.0);
    assert_eq!(s.k_group(0), s.k_group(1));
    assert_ne!(s.k_group(0), s.k_group(2));
    assert_eq!(s.stats().islands, 2);
    let i = 2.0 / (1e3 + 0.01);
    assert!((s.element_current(1).unwrap() - i).abs() <= 1e-12 * i);
}

#[test]
fn leakage_conducts_but_does_not_reference() {
    // 5 V → open switch (r_off 100 MΩ, leakage) → B → 1 kΩ → ground.
    let mut s = loaded(&net(3, vec![vsrc(1, 0, 5.0, 0.01), leak(1, 2, 1e8), r(2, 0, 1e3)]));
    s.step_frame().unwrap();
    let b = s.node(2);
    let expected = 5.0 * 1e3 / (0.01 + 1e8 + 1e3);
    assert_eq!(b.class, NodeClass::Referenced);
    assert!((b.volts - expected).abs() <= 1e-12 * expected, "{}", b.volts);
    assert!((b.volts - 49.9995e-6).abs() < 1e-10);
    assert!((s.element_current(1).unwrap() - expected / 1e3).abs() <= 1e-12 * expected / 1e3);
    // Open switch and nothing else behind it: floating.
    let mut s = loaded(&net(3, vec![vsrc(1, 0, 5.0, 0.01), leak(1, 2, 1e8)]));
    s.step_frame().unwrap();
    assert_eq!(s.node(2).class, NodeClass::Floating);
    assert!(s.element_current(1).unwrap().abs() <= 1e-20);
}

/// V → R → C → ground on nodes (p, p + 1).
fn rc(p: usize, res: f64, c: f64, key: u64) -> Vec<Element> {
    vec![vsrc(p, 0, 5.0, 0.01), r(p, p + 1, res), cap(p + 1, c, key)]
}

#[test]
fn islands_share_only_ground() {
    // τ ≈ 1 ms (dt 50 µs) and τ ≈ 100 ms (dt 1 ms): two islands, independent steps.
    let mut elements = rc(1, 1e3, 1e-6, 1);
    elements.extend(rc(3, 1e4, 1e-5, 2));
    let mut s = loaded(&net(5, elements.clone()));
    s.step_frame().unwrap();
    let st = s.stats();
    assert_eq!(st.islands, 2);
    let (fast, slow) = (s.island_of(2).unwrap(), s.island_of(4).unwrap());
    assert_ne!(fast, slow);
    assert_eq!((s.island_dt_ns(fast), s.island_dt_ns(slow)), (Some(50_000), Some(1_000_000)));
    assert_eq!((st.substeps_last_frame, st.max_island_substeps, st.min_dt_ns), (21, 20, 50_000));

    // A leakage resistor between them merges the islands: one step for both, from τ_lb = 1/(1/τ1 + 1/τ2)
    // ≈ 0.990 ms, so 25 sub-steps of 40 µs.
    let mut merged = elements.clone();
    merged.push(leak(2, 4, 1e9));
    let mut s = loaded(&net(5, merged));
    s.step_frame().unwrap();
    assert_eq!(s.stats().islands, 1);
    assert_eq!(s.island_of(2), s.island_of(4));
    assert_eq!(s.island_dt_ns(0), Some(40_000));

    // So does a Vcvs controlled from one circuit and driving the other.
    let mut merged = elements;
    merged.push(Element::Vcvs { pos: 4, neg: 0, cp: 2, cn: 0, gain: 0.5, r_out: 1e6, gate: None });
    let mut s = loaded(&net(5, merged));
    s.step_frame().unwrap();
    assert_eq!(s.stats().islands, 1);
}

#[test]
fn rc_ladder_step_matches_tau_lb_of_rc_over_three() {
    // R–C–R–C ladder: τ_lb = RC/3. RC/3 = 1.0001 ms gives dt_target = 50.005 µs → 20 sub-steps;
    // RC/3 = 0.9999 ms gives 49.995 µs → 25 sub-steps of 40 µs.
    for (c, n_sub) in [(3.0003e-6, 20), (2.9997e-6, 25)] {
        let ladder = net(3, vec![r(0, 1, 1e3), cap(1, c, 1), r(1, 2, 1e3), cap(2, c, 2)]);
        let mut s = loaded(&ladder);
        s.step_frame().unwrap();
        assert_eq!(s.stats().max_island_substeps, n_sub, "C = {c}");
        assert_eq!(s.island_dt_ns(0), Some(1_000_000 / n_sub));
    }
}

#[test]
fn singular_island_publishes_nothing_and_keeps_its_state() {
    // Vcvs 1 drives node 1 from node 1 itself; Vcvs 2 copies node 1 onto an RC charged by a 5 V source.
    let build = |gain: f64| {
        net(
            4,
            vec![
                Element::Vcvs { pos: 1, neg: 0, cp: 1, cn: 0, gain, r_out: 1.0, gate: None },
                Element::Vcvs { pos: 3, neg: 0, cp: 1, cn: 0, gain: 1.0, r_out: 1e3, gate: None },
                vsrc(3, 0, 5.0, 1e3),
                cap(3, 1e-6, 1),
            ],
        )
    };
    let mut s = loaded(&build(0.5));
    for _ in 0..10 {
        s.step_frame().unwrap();
    }
    let stored = s.capacitor_state(1).unwrap();
    assert!((stored.0 - 2.5).abs() < 1e-6, "{stored:?}");

    // Gain 1 with nothing else on node 1: Y[1,1] = g − gain·g = 0.
    s.load(&build(1.0)).unwrap();
    assert_eq!(s.island_status(0), IslandStatus::Pending);
    let t = s.time_ns();
    assert_eq!(s.step_frame(), Err(SolveError { island: 0, status: IslandStatus::Singular }));
    assert_eq!(s.island_status(0), IslandStatus::Singular);
    assert_eq!(s.capacitor_state(1), Some(stored));
    assert_eq!(s.node(3).class, NodeClass::Unused);
    assert_eq!(s.k_group(3), None);
    assert_eq!(s.element_current(2), None);
    assert_eq!(s.time_ns(), t + 1_000_000);
    assert_eq!(s.preview(), Err(SolveError { island: 0, status: IslandStatus::Singular }));
    assert_eq!(s.capacitor_state(1), Some(stored));
}

#[test]
fn non_finite_solution_is_refused() {
    // Finite values whose Norton current overflows: 1e300 V behind 1e-300 Ω.
    let mut s = loaded(&net(3, vec![vsrc(1, 0, 1e300, 1e-300), r(1, 2, 1.0), cap(2, 1e-6, 1)]));
    assert_eq!(s.step_frame(), Err(SolveError { island: 0, status: IslandStatus::NonFinite }));
    assert_eq!(s.capacitor_state(1), Some((0.0, 0.0)));
    assert_eq!(s.element_current(1), None);
}

#[test]
fn unpowered_resistance_of_the_can_bus() {
    // CANH = 1, CANL = 2; three transceivers modelled as 25 kΩ + 25 kΩ through a mid node biased by a source,
    // which is removed when unpowered. Gated elements, capacitors and other sources are ignored too.
    let mut elements = vec![r(1, 2, 120.0), r(1, 2, 120.0)];
    for mid in [3, 4, 5] {
        elements.push(r(1, mid, 25e3));
        elements.push(r(mid, 2, 25e3));
        elements.push(vsrc(mid, 0, 2.5, 1.0));
    }
    elements.push(Element::Resistor { a: 1, b: 0, r: 10.0, leakage: false, gate: Some(0) });
    elements.push(cap(1, 1e-9, 1));
    elements.push(Element::Supply { pos: 2, neg: 0, v: 5.0, r_out: 0.01, i_lim: None, key: 2, gate: None });
    elements.push(Element::Vcvs { pos: 1, neg: 0, cp: 2, cn: 0, gain: 2.0, r_out: 1.0, gate: None });
    let network = Network {
        nodes: 7,
        ground: Some(0),
        elements,
        gates: vec![Gate { cp: 1, cn: 0, on_above: 2.0, off_below: 1.0, key: 3 }],
    };
    let expected = 1.0 / (2.0 / 120.0 + 3.0 / 50e3);
    let r12 = unpowered_resistance(&network, 1, 2).unwrap();
    assert!((r12 - 59.7848).abs() <= 1e-4 && (r12 - expected).abs() <= 1e-9 * expected, "{r12}");
    assert_eq!(unpowered_resistance(&network, 2, 1), Some(r12));
    assert_eq!(unpowered_resistance(&network, 3, 3), Some(0.0));
    assert_eq!(unpowered_resistance(&network, 1, 0), Some(f64::INFINITY), "CANH–GND open when unpowered");
    assert_eq!(unpowered_resistance(&network, 1, 6), Some(f64::INFINITY));
    assert_eq!(unpowered_resistance(&network, 1, 7), None);
    // Through a mid node: 25 kΩ ∥ (25 kΩ + bus) towards the others.
    let rm = unpowered_resistance(&network, 3, 1).unwrap();
    let rest = 1.0 / (2.0 / 120.0 + 2.0 / 50e3);
    let exact = 1.0 / (1.0 / 25e3 + 1.0 / (25e3 + rest));
    assert!((rm - exact).abs() <= 1e-9 * exact, "{rm} vs {exact}");
}

#[test]
fn load_refuses_every_network_error_and_changes_nothing() {
    let good = net(3, vec![vsrc(1, 0, 5.0, 0.01), r(1, 2, 1e3), cap(2, 1e-6, 1)]);
    let mut s = loaded(&good);
    s.step_frame().unwrap();
    let (digest, v2) = (s.state_digest(), s.node(2));
    let gate = |on_above: f64, off_below: f64, cp: usize| Gate { cp, cn: 0, on_above, off_below, key: 50 };
    let bad_value = |e: Element, field: &'static str| (net(3, vec![e]), NetworkError::BadValue { element: 0, field });
    let supply = |v: f64, r_out: f64, i_lim: Option<f64>| Element::Supply { pos: 1, neg: 0, v, r_out, i_lim, key: 9, gate: None };
    let vcvs = |gain: f64, r_out: f64| Element::Vcvs { pos: 1, neg: 0, cp: 2, cn: 0, gain, r_out, gate: None };
    let mut cases = vec![
        (net(3, vec![r(1, 2, 1.0), r(1, 3, 1.0)]), NetworkError::NodeOutOfRange { element: 1 }),
        (net(3, vec![vcvs(1.0, 1.0), Element::Vcvs { pos: 1, neg: 0, cp: 2, cn: 9, gain: 1.0, r_out: 1.0, gate: None }]), NetworkError::NodeOutOfRange { element: 1 }),
        (Network { nodes: 3, ground: Some(3), elements: vec![], gates: vec![] }, NetworkError::NodeOutOfRange { element: usize::MAX }),
        bad_value(r(1, 2, 0.0), "r"),
        bad_value(r(1, 2, f64::NAN), "r"),
        bad_value(cap(1, -1e-6, 1), "c"),
        bad_value(cap(1, f64::INFINITY, 1), "c"),
        bad_value(vsrc(1, 0, f64::INFINITY, 1.0), "v"),
        bad_value(vsrc(1, 0, -5.0, 0.0), "r_out"),
        bad_value(supply(-1.0, 1.0, None), "v"),
        bad_value(supply(5.0, f64::NAN, None), "r_out"),
        bad_value(supply(5.0, 1.0, Some(0.0)), "i_lim"),
        bad_value(supply(5.0, 1.0, Some(f64::INFINITY)), "i_lim"),
        bad_value(vcvs(f64::NAN, 1.0), "gain"),
        bad_value(vcvs(1.0, -1.0), "r_out"),
        (net(3, vec![Element::Resistor { a: 1, b: 2, r: 1.0, leakage: false, gate: Some(1) }]), NetworkError::BadGate { gate: 1 }),
        (net(3, vec![cap(1, 1e-6, 7), cap(2, 1e-6, 7)]), NetworkError::DuplicateKey { key: 7 }),
    ];
    for (g, err) in [
        (gate(1.0, 1.0, 1), NetworkError::BadGate { gate: 0 }),
        (gate(f64::NAN, 1.0, 1), NetworkError::BadGate { gate: 0 }),
        (gate(2.0, 1.0, 3), NetworkError::BadGate { gate: 0 }),
        (Gate { key: 9, ..gate(2.0, 1.0, 1) }, NetworkError::DuplicateKey { key: 9 }),
    ] {
        cases.push((Network { nodes: 3, ground: Some(0), elements: vec![supply(5.0, 1.0, None)], gates: vec![g] }, err));
    }
    for (network, err) in cases {
        assert_eq!(s.load(&network), Err(err.clone()), "{err:?}");
        assert_eq!(s.state_digest(), digest, "{err:?}: state unchanged");
        assert_eq!(s.node(2), v2, "{err:?}: results unchanged");
        assert_eq!(s.stats().islands, 1);
    }
    // The previous network is still the one stepped.
    s.step_frame().unwrap();
    assert!(s.capacitor_state(1).unwrap().0 > v2.volts);
}

#[test]
fn state_follows_keys_across_reloads() {
    let base = || {
        let mut e = rc(1, 1e3, 1e-6, 1);
        e.extend(rc(3, 2e3, 1e-6, 2));
        e
    };
    let mut s = loaded(&net(6, base()));
    for _ in 0..2 {
        s.step_frame().unwrap();
    }
    let (c1, c2) = (s.capacitor_state(1).unwrap(), s.capacitor_state(2).unwrap());

    // Identical network: nothing commuted, states kept bit for bit.
    s.load(&net(6, base())).unwrap();
    assert_eq!(s.stats().changed_islands, 0);
    assert_eq!((s.capacitor_state(1), s.capacitor_state(2)), (Some(c1), Some(c2)));

    // A new independent island: only it is commuted.
    let mut extended = base();
    extended.push(vsrc(5, 0, 1.0, 1.0));
    s.load(&net(6, extended)).unwrap();
    assert_eq!((s.stats().islands, s.stats().changed_islands), (3, 1));
    assert_eq!((s.capacitor_state(1), s.capacitor_state(2)), (Some(c1), Some(c2)));

    // Changed capacitance: the voltage is kept, not the charge.
    let mut changed = base();
    changed[2] = cap(2, 4.7e-6, 1);
    s.load(&net(6, changed)).unwrap();
    assert_eq!(s.stats().changed_islands, 1);
    assert_eq!(s.capacitor_state(1).unwrap().0, c1.0);

    // Vanished key: state dropped; restored: discharged.
    let without: Vec<Element> = base().into_iter().filter(|e| !matches!(e, Element::Capacitor { key: 1, .. })).collect();
    s.load(&net(6, without)).unwrap();
    assert_eq!(s.capacitor_state(1), None);
    s.load(&net(6, base())).unwrap();
    assert_eq!(s.capacitor_state(1), Some((0.0, 0.0)));
    assert_eq!(s.capacitor_state(2), Some(c2));
}

/// v_n / V∞ after a commutation: 2 backward Euler sub-steps, then trapezoids.
fn scheme(n: u32, r: f64) -> f64 {
    let q = (1.0 - r / 2.0) / (1.0 + r / 2.0);
    1.0 - q.powi(n as i32 - 2) / ((1.0 + r) * (1.0 + r))
}

#[test]
fn only_the_changed_island_is_commuted() {
    // Island A: 1 kΩ / 1 µF (20 sub-steps of 50 µs); island B: 1.5 kΩ / 1 µF (τ/20 = 75 µs: 16 of 62.5 µs).
    let build = |ra: f64| {
        let mut e = rc(1, ra, 1e-6, 1);
        e.extend(rc(3, 1.5e3, 1e-6, 2));
        net(5, e)
    };
    let mut s = loaded(&build(1e3));
    let rb = 62.5e-6 / ((1.5e3 + 0.01) * 1e-6);
    for _ in 0..2 {
        s.step_frame().unwrap();
    }
    let va = s.capacitor_state(1).unwrap().0;
    // A changes (1 kΩ → 1.2 kΩ): commuted, its discharge restarts with 2 backward Euler sub-steps from va.
    s.load(&build(1.2e3)).unwrap();
    assert_eq!(s.stats().changed_islands, 1);
    let ra = 50e-6 / ((1.2e3 + 0.01) * 1e-6);
    for frame in 3..=5u32 {
        s.step_frame().unwrap();
        // B is not commuted: its trapezoid sequence continues as if nothing happened.
        assert_eq!(s.island_dt_ns(s.island_of(4).unwrap()), Some(62_500));
        let vb = s.capacitor_state(2).unwrap().0;
        let expected_b = 5.0 * scheme(frame * 16, rb);
        assert!((vb - expected_b).abs() <= 1e-12 * 5.0, "B frame {frame}: {vb} vs {expected_b}");
        let n = (frame - 2) * 20;
        let expected_a = 5.0 - (5.0 - va) * (1.0 - scheme(n, ra));
        let va_now = s.capacitor_state(1).unwrap().0;
        assert!((va_now - expected_a).abs() <= 1e-12 * 5.0, "A frame {frame}: {va_now} vs {expected_a}");
    }
}

#[test]
fn reset_discharges_and_commutes() {
    let mut s = loaded(&net(3, rc(1, 1e3, 1e-6, 1)));
    let fresh = s.state_digest();
    for _ in 0..3 {
        s.step_frame().unwrap();
    }
    s.reset();
    assert_eq!(s.time_ns(), 0);
    assert_eq!(s.capacitor_state(1), Some((0.0, 0.0)));
    assert_eq!(s.island_status(0), IslandStatus::Pending);
    assert_eq!(s.state_digest(), fresh);
    s.step_frame().unwrap();
    let ratio = 50e-6 / ((1e3 + 0.01) * 1e-6);
    assert!((s.capacitor_state(1).unwrap().0 - 5.0 * scheme(20, ratio)).abs() <= 1e-12 * 5.0);
}
