//! Cost of a sub-step and of a frame (contract §8.6) for RC ladders of 8, 16, 32 and 100 nodes at dt = 10 µs.
//! Run: cargo test -p n7rt-solver --test bench -- --ignored --nocapture

use std::time::Instant;

use n7rt_solver::{Element, Network, Solver, DT_MIN_NS};

/// Source node 1 and n − 1 stages of 100 Ω / 1 µF: one island of n nodes, τ_lb far below 200 µs.
fn ladder(n: usize) -> Network {
    let mut elements = vec![Element::VSource { pos: 1, neg: 0, v: 5.0, r_out: 0.01, gate: None }];
    for k in 1..n {
        elements.push(Element::Resistor { a: k, b: k + 1, r: 100.0, leakage: false, gate: None });
        elements.push(Element::Capacitor { a: k + 1, b: 0, c: 1e-6, key: k as u64 });
    }
    Network { nodes: n + 1, ground: Some(0), elements, gates: vec![] }
}

#[test]
#[ignore]
fn bench_substep() {
    for n in [8, 16, 32, 100] {
        let mut s = Solver::new();
        s.load(&ladder(n)).unwrap();
        let cold = Instant::now();
        s.step_frame().unwrap();
        let cold_us = cold.elapsed().as_secs_f64() * 1e6;
        assert_eq!(s.island_dt_ns(0), Some(DT_MIN_NS));
        let substeps = s.stats().max_island_substeps;
        for _ in 0..2 {
            s.step_frame().unwrap();
        }
        let frames = 20;
        let start = Instant::now();
        for _ in 0..frames {
            s.step_frame().unwrap();
        }
        let per_frame = start.elapsed().as_secs_f64() * 1e6 / frames as f64;
        println!(
            "bench_substep: {n:>3} nodes, {substeps} sub-steps of {} µs: {:>8.2} µs/sub-step, {:>9.1} µs/frame \
             (first frame with factorizations and τ: {cold_us:.1} µs)",
            DT_MIN_NS / 1000,
            per_frame / substeps as f64,
            per_frame
        );
    }
}
