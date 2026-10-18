//! τ_lb against the eigenvalues of C⁻¹G (contract §5.5). τ_lb is internal, hence these in-crate tests:
//! lower bound of the fastest time constant on random passive RC networks with mesh capacitors (power iteration
//! oracle), trace identity Σ 1/τ_k = trace(C_tree⁻¹·G) on their capacitor tree, and the R–C–R–C ladder.

use crate::island::partition;
use crate::tau::{compute, substeps_for, TauInfo};
use crate::{Element, Network, DT_MIN_NS, FRAME_NS, SUBSTEP_COUNTS, TAU_QUASI_STATIC_S};

fn tau_info(net: &Network) -> TauInfo {
    let part = partition(net);
    assert_eq!(part.n_public, 1, "one island expected");
    let def = &part.defs[0];
    let modes: Vec<u8> = def.modes.iter().map(|m| m.initial()).collect();
    compute(def, &modes)
}

/// Deterministic xorshift64*.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// Gaussian elimination with partial pivoting: the test's own linear solver, independent of crate::lu.
fn gauss(a: &[Vec<f64>], b: &[f64]) -> Vec<f64> {
    let n = b.len();
    let mut m: Vec<Vec<f64>> = a.iter().zip(b).map(|(row, &bi)| row.iter().copied().chain([bi]).collect()).collect();
    for k in 0..n {
        let p = (k..n).max_by(|&i, &j| m[i][k].abs().total_cmp(&m[j][k].abs())).unwrap();
        m.swap(k, p);
        for i in (k + 1)..n {
            let l = m[i][k] / m[k][k];
            for j in k..=n {
                m[i][j] -= l * m[k][j];
            }
        }
    }
    let mut x = vec![0.0; n];
    for k in (0..n).rev() {
        let s: f64 = ((k + 1)..n).map(|j| m[k][j] * x[j]).sum();
        x[k] = (m[k][n] - s) / m[k][k];
    }
    x
}

fn matvec(a: &[Vec<f64>], x: &[f64]) -> Vec<f64> {
    a.iter().map(|row| row.iter().zip(x).map(|(r, v)| r * v).sum()).collect()
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// Nodal capacitance (capacitors whose key satisfies `keep`) and conductance matrices, ground (node 0) removed.
fn matrices(net: &Network, keep: impl Fn(u64) -> bool) -> (Vec<Vec<f64>>, Vec<Vec<f64>>) {
    let n = net.nodes - 1;
    let mut c = vec![vec![0.0; n]; n];
    let mut g = vec![vec![0.0; n]; n];
    let add = |m: &mut Vec<Vec<f64>>, a: usize, b: usize, v: f64| {
        if a > 0 {
            m[a - 1][a - 1] += v;
        }
        if b > 0 {
            m[b - 1][b - 1] += v;
        }
        if a > 0 && b > 0 {
            m[a - 1][b - 1] -= v;
            m[b - 1][a - 1] -= v;
        }
    };
    for e in &net.elements {
        match *e {
            Element::Capacitor { a, b, c: cv, key } if keep(key) => add(&mut c, a, b, cv),
            Element::Resistor { a, b, r, .. } => add(&mut g, a, b, 1.0 / r),
            _ => {}
        }
    }
    (c, g)
}

/// Largest eigenvalue of C⁻¹G by power iteration. The Rayleigh quotient xᵀGx / xᵀCx (C symmetric positive
/// definite) never exceeds it and converges quadratically: the largest value seen is kept.
fn lambda_max(c: &[Vec<f64>], g: &[Vec<f64>]) -> f64 {
    let n = c.len();
    let mut x: Vec<f64> = (0..n).map(|i| 1.0 + 0.37 * i as f64).collect();
    let mut best = 0.0f64;
    let mut stable = 0;
    for _ in 0..200_000 {
        let y = gauss(c, &matvec(g, &x));
        let norm = dot(&y, &y).sqrt();
        if norm == 0.0 {
            return 0.0;
        }
        x = y.iter().map(|v| v / norm).collect();
        let next = dot(&x, &matvec(g, &x)) / dot(&x, &matvec(c, &x));
        stable = if (next - best).abs() <= 1e-13 * next { stable + 1 } else { 0 };
        best = best.max(next);
        if stable == 20 {
            return best;
        }
    }
    panic!("power iteration did not converge");
}

#[test]
fn tau_lb_bounds_the_fastest_time_constant_of_random_rc_networks() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for case in 0..300 {
        let k = 1 + rng.below(6);
        let mut elements = Vec::new();
        let mut key = 1u64;
        // Capacitor tree first (smallest keys): node 1 to ground, every other node to an earlier one.
        for i in 1..=k {
            let parent = if i == 1 { 0 } else { 1 + rng.below(i - 1) };
            elements.push(Element::Capacitor { a: i, b: parent, c: 1e-6 * (1.0 + 99.0 * rng.unit()), key });
            key += 1;
        }
        let tree_keys = key;
        // Mesh capacitors: the tree already spans every node, so each one closes a loop.
        for _ in 0..1 + rng.below(3) {
            let a = rng.below(k + 1);
            let b = (a + 1 + rng.below(k)) % (k + 1);
            elements.push(Element::Capacitor { a, b, c: 1e-6 * (1.0 + 99.0 * rng.unit()), key });
            key += 1;
        }
        for _ in 0..1 + rng.below(2 * k) {
            let a = rng.below(k + 1);
            let b = (a + 1 + rng.below(k)) % (k + 1);
            elements.push(Element::Resistor { a, b, r: 1e3 * (1.0 + 99.0 * rng.unit()), leakage: false, gate: None });
        }
        let net = Network { nodes: k + 1, ground: Some(0), elements, gates: vec![] };
        let info = tau_info(&net);

        for (ci, e) in net.elements.iter().filter(|e| matches!(e, Element::Capacitor { .. })).enumerate() {
            let Element::Capacitor { key, .. } = *e else { unreachable!() };
            assert_eq!(info.mesh[ci], key >= tree_keys, "case {case}: mesh flag of key {key}");
            if !info.mesh[ci] {
                let t = info.tau[ci];
                assert!(t == f64::INFINITY || t >= TAU_QUASI_STATIC_S, "case {case}: τ_k = {t} in range");
            }
        }

        // Trace identity on the tree: Σ 1/τ_k = trace(C_tree⁻¹·G).
        let (c_tree, g) = matrices(&net, |key| key < tree_keys);
        let trace: f64 = (0..k)
            .map(|j| {
                let col: Vec<f64> = g.iter().map(|row| row[j]).collect();
                gauss(&c_tree, &col)[j]
            })
            .sum();
        assert!(((1.0 / info.tau_lb) - trace).abs() <= 1e-9 * trace, "case {case}: 1/τ_lb = {} vs trace {trace}", 1.0 / info.tau_lb);

        // Lower bound of the true fastest time constant, mesh capacitors included.
        let (c_all, g) = matrices(&net, |_| true);
        let tau_min = 1.0 / lambda_max(&c_all, &g);
        assert!(info.tau_lb <= tau_min * (1.0 + 1e-9), "case {case}: τ_lb {} > τ_min {tau_min}", info.tau_lb);
    }
}

#[test]
fn rc_ladder_tau_lb_is_rc_over_three() {
    let (r, c) = (1e3, 1e-6);
    let net = Network {
        nodes: 3,
        ground: Some(0),
        elements: vec![
            Element::Resistor { a: 0, b: 1, r, leakage: false, gate: None },
            Element::Capacitor { a: 1, b: 0, c, key: 1 },
            Element::Resistor { a: 1, b: 2, r, leakage: false, gate: None },
            Element::Capacitor { a: 2, b: 0, c, key: 2 },
        ],
        gates: vec![],
    };
    let info = tau_info(&net);
    let rc = r * c;
    assert!((info.tau[0] - rc / 2.0).abs() <= 1e-15 * rc, "τ_1 = R∥R·C: {}", info.tau[0]);
    assert!((info.tau[1] - rc).abs() <= 1e-15 * rc, "τ_2 = R·C: {}", info.tau[1]);
    assert!((info.tau_lb - rc / 3.0).abs() <= 1e-12 * rc / 3.0, "τ_lb = {}", info.tau_lb);
    // The true fastest time constant, (3 − √5)/2 · RC ≈ 0.382 RC, is above the bound.
    let (cm, gm) = matrices(&net, |_| true);
    let tau_min = 1.0 / lambda_max(&cm, &gm);
    assert!((tau_min - (3.0 - 5f64.sqrt()) / 2.0 * rc).abs() <= 1e-9 * rc);
    assert!(info.tau_lb < tau_min);
}

#[test]
fn quasi_static_and_open_capacitors_leave_the_bound() {
    // C1 behind 10 Ω (τ = 1 µs, quasi-static), C2 behind 1 kΩ (τ = 1 ms), C3 with no resistive path (τ = ∞).
    let net = Network {
        nodes: 4,
        ground: Some(0),
        elements: vec![
            Element::Resistor { a: 1, b: 0, r: 10.0, leakage: false, gate: None },
            Element::Capacitor { a: 1, b: 0, c: 1e-7, key: 1 },
            Element::Resistor { a: 1, b: 2, r: 1e3, leakage: false, gate: None },
            Element::Capacitor { a: 2, b: 0, c: 1e-6, key: 2 },
            Element::Capacitor { a: 3, b: 2, c: 1e-6, key: 3 },
        ],
        gates: vec![],
    };
    let info = tau_info(&net);
    assert!((info.tau[0] - 10.0 * 1e-7 * (1e3 / (1e3 + 10.0))).abs() < 1e-18, "τ_1 = {}", info.tau[0]);
    assert_eq!(info.tau[2], f64::INFINITY);
    assert!((info.tau_lb - 1e-3).abs() <= 1e-15, "τ_lb = {}", info.tau_lb);
}

#[test]
fn step_table_gives_exact_integer_substeps() {
    // The table is every divisor N of one frame in ns whose step is at least DT_MIN_NS, ascending.
    let divisors: Vec<i64> = (1..=FRAME_NS).filter(|&n| FRAME_NS % n == 0 && FRAME_NS / n >= DT_MIN_NS).collect();
    assert_eq!(divisors, SUBSTEP_COUNTS.to_vec());

    // Sweep of τ_lb from 0.1 µs to 2 s: N is in the table, dt = 1 ms / N is an exact number of ns, at or below the
    // target clamp(τ_lb / 20, 10 µs, 1 ms), and the next larger step of the table would exceed the target.
    let mut tau = 1e-7;
    while tau < 2.0 {
        let n = substeps_for(tau);
        let k = SUBSTEP_COUNTS.iter().position(|&x| x == n).expect("N from the table");
        let dt = FRAME_NS / n;
        assert_eq!(dt * n, FRAME_NS, "τ_lb = {tau}");
        let target_ns = (tau / 20.0 * 1e9).clamp(DT_MIN_NS as f64, FRAME_NS as f64);
        assert!(dt as f64 <= target_ns, "τ_lb = {tau}: dt {dt} ns above the target {target_ns} ns");
        if k > 0 {
            assert!((FRAME_NS / SUBSTEP_COUNTS[k - 1]) as f64 > target_ns, "τ_lb = {tau}: N = {n} not the first");
        }
        tau *= 1.01;
    }
    // Anchors: the floor (100 sub-steps of 10 µs), the golden RC cases of the contract, no capacitor (static).
    assert_eq!(substeps_for(1e-6), 100);
    assert_eq!(substeps_for(1.00001e-3), 20);
    assert_eq!(substeps_for(100.0001e-3), 1);
    assert_eq!(substeps_for(f64::INFINITY), 1);
}
