//! Determinism (contract §5.8): same network, same calls ⇒ same state digest, bit for bit; one changed value
//! changes it.

use n7rt_solver::{Element, Gate, Network, Solver, SupplyMode};

/// Island 1: limited supply → 1 kΩ → 1 mF, with a power-on gate adding a 10 kΩ load above 3 V (CC, then gate,
/// then CV). Island 2: 2 V → switch → 2.2 kΩ → 4.7 µF, the switch toggled by reloads.
fn network(r12: f64, closed: bool) -> Network {
    let switch = if closed {
        Element::Resistor { a: 3, b: 4, r: 0.01, leakage: false, gate: None }
    } else {
        Element::Resistor { a: 3, b: 4, r: 1e8, leakage: true, gate: None }
    };
    Network {
        nodes: 6,
        ground: Some(0),
        elements: vec![
            Element::Supply { pos: 1, neg: 0, v: 5.0, r_out: 0.01, i_lim: Some(1e-3), key: 1, gate: None },
            Element::Resistor { a: 1, b: 2, r: r12, leakage: false, gate: None },
            Element::Capacitor { a: 2, b: 0, c: 1e-3, key: 2 },
            Element::Resistor { a: 2, b: 0, r: 1e4, leakage: false, gate: Some(0) },
            Element::VSource { pos: 3, neg: 0, v: 2.0, r_out: 0.01, gate: None },
            switch,
            Element::Resistor { a: 4, b: 5, r: 2.2e3, leakage: false, gate: None },
            Element::Capacitor { a: 5, b: 0, c: 4.7e-6, key: 4 },
        ],
        gates: vec![Gate { cp: 2, cn: 0, on_above: 3.0, off_below: 2.0, key: 3 }],
    }
}

/// Runs 10 000 frames with a reload every 2 500 frames and a preview every 1 000; digests every 1 000 frames.
fn run(r12: f64) -> (Vec<u64>, Solver) {
    let mut s = Solver::new();
    let mut digests = Vec::new();
    for frame in 0..10_000 {
        if frame % 2_500 == 0 {
            s.load(&network(r12, frame / 2_500 % 2 == 0)).unwrap();
        }
        s.step_frame().unwrap();
        if frame % 1_000 == 999 {
            s.preview().unwrap();
            digests.push(s.state_digest());
        }
        if frame == 999 {
            assert_eq!(s.supply_mode(0), Some(SupplyMode::Cc), "charging at the limit");
            assert_eq!(s.gate_on(0), Some(false));
        }
    }
    (digests, s)
}

#[test]
fn two_runs_give_the_same_digest_and_one_value_changes_it() {
    let (a, sa) = run(1e3);
    let (b, sb) = run(1e3);
    assert_eq!(a, b);
    assert_eq!(sa.state_digest(), sb.state_digest());
    assert_eq!(sa.time_ns(), 10_000 * 1_000_000);
    // The scenario went through its modes: gate on, supply back in CV near V·10k/(10k + 1k).
    assert_eq!(sa.gate_on(0), Some(true));
    assert_eq!(sa.supply_mode(0), Some(SupplyMode::Cv));
    let v2 = sa.capacitor_state(2).unwrap().0;
    assert!((v2 - 5.0 * 1e4 / (1e4 + 1e3 + 0.01)).abs() < 0.2, "{v2}");

    let (c, sc) = run(1e3 + 1e-7);
    assert_ne!(sc.state_digest(), sa.state_digest());
    assert_ne!(c.last(), a.last());
}
