//! Physical laws on generated circuit parameters; exact numerical conformance is restricted.
use n7rt_solver::{Element, IslandStatus, Network, NodeClass, Solver, SupplyMode, FRAME_NS};

fn r(a: usize, b: usize, r: f64) -> Element {
    Element::Resistor { a, b, r, leakage: false, gate: None }
}

fn vsrc(pos: usize, v: f64, r_out: f64) -> Element {
    Element::VSource { pos, neg: 0, v, r_out, gate: None }
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


fn rc_charge_laws(res: f64, capacitance: f64, frames: u32) {
    let supply = 5.0;
    let output_resistance = 0.01;
    let mut solver = loaded(&net(3, vec![vsrc(1, supply, output_resistance), r(1, 2, res), cap(2, capacitance, 1)]));
    let tau = (res + output_resistance) * capacitance;
    let mut previous = 0.0;
    let mut at_tau = None;
    for frame in 1..=frames {
        solver.step_frame().unwrap();
        let dt = solver.island_dt_ns(0).unwrap();
        assert!(dt > 0 && FRAME_NS % dt == 0);
        assert!((dt as f64 * 1e-9) <= tau / 20.0 * (1.0 + 1e-12));
        let substeps = FRAME_NS / dt;
        let stats = solver.stats();
        assert_eq!((stats.max_island_substeps, stats.substeps_last_frame, stats.min_dt_ns), (substeps, substeps, dt));
        let (volts, current) = solver.capacitor_state(1).unwrap();
        assert!(volts >= previous && volts >= 0.0 && volts <= supply);
        assert_eq!(solver.node(2).volts, volts);
        assert_eq!(solver.node(2).class, NodeClass::Referenced);
        let (source_current, resistor_current) = (solver.element_current(0).unwrap(), solver.element_current(1).unwrap());
        let tolerance = 1e-8 * supply / res;
        assert!((source_current - resistor_current).abs() <= tolerance && (resistor_current - current).abs() <= tolerance);
        let time = solver.time_ns() as f64 * 1e-9;
        let continuous = supply * (1.0 - (-time / tau).exp());
        // The two startup backward-Euler steps contribute O((dt/tau)^2) error.
        let error_bound = 2.0 * (dt as f64 * 1e-9 / tau).powi(2) * supply;
        assert!((volts - continuous).abs() <= error_bound + supply * 1e-10, "frame {frame}");
        if at_tau.is_none() && time >= tau * (1.0 - 1e-3) { at_tau = Some((time, volts / supply)); }
        previous = volts;
    }
    let (time, fraction) = at_tau.unwrap();
    assert!((fraction - (1.0 - (-time / tau).exp())).abs() <= 1e-3);
}

#[test]
fn fast_rc_charge_obeys_monotonicity_kcl_and_exponential_law() {
    for scale in 1..=3 { rc_charge_laws(scale as f64 * 1e3, 1e-6, scale * 3); }
}

#[test]
fn slow_rc_charge_obeys_monotonicity_kcl_and_exponential_law() {
    for scale in 1..=3 { rc_charge_laws(scale as f64 * 1e4, 1e-5, scale * 300); }
}

#[test]
fn rc_discharge_is_monotone_and_conserves_loop_current() {
    let (resistance, capacitance) = (1.2e3, 1e-6);
    let mut solver = loaded(&net(3, vec![vsrc(1, 5.0, 0.01), r(1, 2, resistance), cap(2, capacitance, 7)]));
    for _ in 0..40 { solver.step_frame().unwrap(); }
    let initial = solver.capacitor_state(7).unwrap().0;
    let start = solver.time_ns();
    solver.load(&net(3, vec![r(2, 0, resistance), cap(2, capacitance, 7)])).unwrap();
    let mut previous = initial;
    for _ in 0..5 {
        solver.step_frame().unwrap();
        let (volts, current) = solver.capacitor_state(7).unwrap();
        assert!(volts >= 0.0 && volts <= previous);
        let tau = resistance * capacitance;
        let time = (solver.time_ns() - start) as f64 * 1e-9;
        let dt = solver.island_dt_ns(0).unwrap() as f64 * 1e-9;
        assert!((volts - initial * (-time / tau).exp()).abs() <= 2.0 * (dt / tau).powi(2) * initial);
        assert!((solver.element_current(0).unwrap() + current).abs() <= 1e-10 * initial / resistance);
        previous = volts;
    }
}

/// Bench supply 5 V / 0.1 A / 10 mΩ → closed switch (10 mΩ) → potentiometer 10 kΩ (B at the switch, A at ground).
fn pot_network(alpha: f64) -> Network {
    let rp = 1e4;
    net(
        4,
        vec![
            Element::Supply { pos: 1, neg: 0, v: 5.0, r_out: 0.01, i_lim: Some(0.1), key: 1, gate: None },
            r(1, 2, 0.01),
            r(2, 3, (1.0 - alpha) * rp),
            r(3, 0, alpha * rp),
        ],
    )
}

#[test]
fn divider_and_potentiometer_values() {
    for setting in 1..10 {
        let alpha = setting as f64 / 10.0;
        let mut s = loaded(&pot_network(alpha));
        s.step_frame().unwrap();
        let (r_aw, r_wb) = (alpha * 1e4, (1.0 - alpha) * 1e4);
        let exact = 5.0 * r_aw / (r_aw + r_wb + 0.01 + 0.01);
        let w = s.node(3);
        assert_eq!(w.class, NodeClass::Referenced);
        assert!((w.volts - exact).abs() <= 1e-12 * exact, "V(W) = {} vs {exact}", w.volts);
        // Through a resistor the current is exact to rounding; out of the supply it is (V − V(pos))/r_out, a
        // difference of nearly equal potentials (≈ 1e−10 relative here).
        let i = 5.0 / (1e4 + 0.02);
        assert!((s.element_current(2).unwrap() - i).abs() <= 1e-12 * i);
        assert!((s.element_current(0).unwrap() - i).abs() <= 1e-9 * i);
        assert_eq!(s.supply_mode(0), Some(SupplyMode::Cv));
        assert_eq!(s.island_status(0), IslandStatus::Solved { ill_conditioned: false });
    }

    let mut s = loaded(&pot_network(0.3));
    s.preview().unwrap();
    assert!((s.node(2).volts - 5.0 * 1e4 / (1e4 + 0.02)).abs() <= 1e-12 * 5.0);
    assert!((s.element_current(2).unwrap() - 5.0 / (1e4 + 0.02)).abs() < 1e-12);
}

#[test]
fn vcvs_amplifies_without_loading_its_control() {
    // Divider 4 V (r_out 10 mΩ) → 1 kΩ → node 2 → 3 kΩ → ground controls a gain-3 Vcvs (r_out 10 Ω) whose output
    // (3, 4) drives a 1 kΩ load; node 4 goes to ground through 100 Ω, which carries no current (no return path).
    let network = net(
        5,
        vec![
            vsrc(1, 4.0, 0.01),
            r(1, 2, 1e3),
            r(2, 0, 3e3),
            Element::Vcvs { pos: 3, neg: 4, cp: 2, cn: 0, gain: 3.0, r_out: 10.0, gate: None },
            r(3, 4, 1e3),
            r(4, 0, 100.0),
        ],
    );
    let mut s = loaded(&network);
    s.step_frame().unwrap();
    let v2 = 4.0 * 3e3 / (1e3 + 3e3 + 0.01);
    assert!((s.node(2).volts - v2).abs() <= 1e-12 * v2);
    // The control port draws nothing: same current in both divider resistors.
    let i_div = v2 / 3e3;
    assert!((s.element_current(1).unwrap() - i_div).abs() <= 1e-12 * i_div);
    assert!((s.element_current(2).unwrap() - i_div).abs() <= 1e-12 * i_div);
    let emf = 3.0 * v2;
    let i_out = emf / (10.0 + 1e3);
    assert!((s.node(3).volts - s.node(4).volts - emf * 1e3 / 1010.0).abs() <= 1e-12 * emf);
    assert!(s.node(4).volts.abs() <= 1e-12 * emf, "{}", s.node(4).volts);
    assert!((s.element_current(3).unwrap() - i_out).abs() <= 1e-12 * i_out, "out of pos");
    assert!((s.element_current(4).unwrap() - i_out).abs() <= 1e-12 * i_out);
    assert_eq!((s.node(3).class, s.node(4).class), (NodeClass::Referenced, NodeClass::Referenced));
}

#[test]
fn preview_redistributes_charge_and_leaves_the_state() {
    let (c1, c2) = (1e-6, 2e-6);
    // C1 charged to 5 V through 100 Ω; C2 held at 0 V by 1 kΩ.
    let charging = net(4, vec![vsrc(1, 5.0, 0.01), r(1, 2, 100.0), cap(2, c1, 1), r(3, 0, 1e3), cap(3, c2, 2)]);
    let mut s = loaded(&charging);
    for _ in 0..30 {
        s.step_frame().unwrap();
    }
    let (v1, v2) = (s.capacitor_state(1).unwrap(), s.capacitor_state(2).unwrap());
    assert!((v1.0 - 5.0).abs() < 1e-12 && v2.0 == 0.0, "{v1:?} {v2:?}");

    // Both capacitors now on the same node, nothing else: preview shows the redistribution.
    s.load(&net(2, vec![cap(1, c1, 1), cap(1, c2, 2)])).unwrap();
    let (digest, t) = (s.state_digest(), s.time_ns());
    s.preview().unwrap();
    assert!(s.is_preview());
    let shared = (c1 * 5.0) / (c1 + c2);
    let exact = (c1 * v1.0 + c2 * v2.0) / (c1 + c2);
    let v = s.node(1);
    assert_eq!(v.class, NodeClass::Referenced);
    assert!((v.volts - shared).abs() <= 1e-6 * shared && (v.volts - exact).abs() <= 1e-12 * exact, "{}", v.volts);
    // Stored state untouched, bit for bit; a second preview gives the same result.
    assert_eq!(s.capacitor_state(1), Some(v1));
    assert_eq!(s.capacitor_state(2), Some(v2));
    assert_eq!((s.state_digest(), s.time_ns()), (digest, t));
    s.preview().unwrap();
    assert_eq!(s.node(1).volts, v.volts);

    // RUN resumes from the frozen state: the frame lands on the same shared voltage.
    s.step_frame().unwrap();
    assert!(!s.is_preview());
    assert!((s.capacitor_state(1).unwrap().0 - exact).abs() <= 1e-12 * exact);
    assert!((s.capacitor_state(2).unwrap().0 - exact).abs() <= 1e-12 * exact);
    assert_ne!(s.state_digest(), digest);
}
