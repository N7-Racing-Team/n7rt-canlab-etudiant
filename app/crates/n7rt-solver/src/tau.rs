//! Step of an island (contract §5.5): capacitor forest, time constant of every tree capacitor, lower bound τ_lb and
//! sub-step count. Also the port resistance used by `unpowered_resistance`.

use crate::dsu::Dsu;
use crate::engine::{active, fix_rows, inject, slot, stamp, stamp_g, volt};
use crate::island::{IslandDef, LElem, GROUND, MODE_CV};
use crate::lu::Lu;
use crate::{DT_MIN_NS, FRAME_NS, SUBSTEP_COUNTS, TAU_QUASI_STATIC_S};

#[derive(Clone, Debug)]
pub(crate) struct TauInfo {
    /// Modes these values were computed for.
    pub modes: Vec<u8>,
    /// Per capacitor (island order): closes a loop of capacitors.
    pub mesh: Vec<bool>,
    /// Per tree capacitor: τ_k = C_k·R_k in s (+∞ without resistive path, 0 if undetermined; unused for mesh ones).
    pub tau: Vec<f64>,
    #[cfg_attr(not(test), allow(dead_code))] // read by the unit tests; the step only needs n_sub
    pub tau_lb: f64,
    pub n_sub: i64,
}

/// Result of a port resistance computation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Port {
    Finite(f64),
    /// No conductance path between the two ports.
    Open,
    /// Singular or non-finite system.
    Singular,
}

pub(crate) fn compute(def: &IslandDef, modes: &[u8]) -> TauInfo {
    let n = def.nodes.len();
    let mut forest = Dsu::new(n + 1);
    let mut mesh = vec![false; def.caps.len()];
    for &ci in &def.caps_by_key {
        let c = &def.caps[ci];
        if !forest.union(slot(c.a, n), slot(c.b, n)) {
            mesh[ci] = true;
        }
    }
    let mut tau = vec![f64::INFINITY; def.caps.len()];
    let mut inv_sum = 0.0;
    for &ci in &def.caps_by_key {
        if mesh[ci] {
            continue;
        }
        let t = match cap_resistance(def, modes, &mesh, ci) {
            Port::Finite(r) => def.caps[ci].c * r,
            Port::Open => f64::INFINITY,
            Port::Singular => 0.0,
        };
        // Undetermined (NaN) or negative (Vcvs) time constants are treated as quasi-static.
        let t = if t.is_nan() { 0.0 } else { t };
        tau[ci] = t;
        if t.is_finite() && t >= TAU_QUASI_STATIC_S {
            inv_sum += 1.0 / t;
        }
    }
    let tau_lb = if inv_sum > 0.0 { 1.0 / inv_sum } else { f64::INFINITY };
    TauInfo { modes: modes.to_vec(), mesh, tau, tau_lb, n_sub: substeps_for(tau_lb) }
}

/// N = first entry of SUBSTEP_COUNTS with FRAME_NS / N ≤ clamp(τ_lb / 20, DT_MIN_NS, FRAME_NS).
pub(crate) fn substeps_for(tau_lb: f64) -> i64 {
    let target_ns = (tau_lb / 20.0 * 1e9).clamp(DT_MIN_NS as f64, FRAME_NS as f64);
    for &n in SUBSTEP_COUNTS.iter() {
        if ((FRAME_NS / n) as f64) <= target_ns {
            return n;
        }
    }
    SUBSTEP_COUNTS[SUBSTEP_COUNTS.len() - 1]
}

/// R_k of tree capacitor k: other tree capacitors shorted, mesh capacitors open, VSource and Supply in CV as
/// 1/r_out, Supply in CC or REV and gated-off elements open, active Vcvs kept, leakage included.
fn cap_resistance(def: &IslandDef, modes: &[u8], mesh: &[bool], k: usize) -> Port {
    let n = def.nodes.len();
    let mut short = Dsu::new(n + 1);
    for (ci, c) in def.caps.iter().enumerate() {
        if ci != k && !mesh[ci] {
            short.union(slot(c.a, n), slot(c.b, n));
        }
    }
    let roots = short.roots();
    let ground_root = roots[n];
    let mut index = vec![usize::MAX; n + 1];
    let mut m = 0;
    for i in 0..n {
        let r = roots[i];
        if r != ground_root && index[r] == usize::MAX {
            index[r] = m;
            m += 1;
        }
    }
    let map = |x: usize| {
        let r = roots[slot(x, n)];
        if r == ground_root {
            GROUND
        } else {
            index[r]
        }
    };
    let mut cond = Vec::new();
    let mut trans = Vec::new();
    for e in &def.lelems {
        match *e {
            LElem::R { a, b, g, gate, .. } => {
                if active(gate, modes) {
                    cond.push((map(a), map(b), g));
                }
            }
            LElem::V { pos, neg, g, gate, .. } => {
                if active(gate, modes) {
                    cond.push((map(pos), map(neg), g));
                }
            }
            LElem::S { pos, neg, g, mode, gate, .. } => {
                if active(gate, modes) && modes[mode] == MODE_CV {
                    cond.push((map(pos), map(neg), g));
                }
            }
            LElem::E { pos, neg, cp, cn, g, gm, gate, .. } => {
                if active(gate, modes) {
                    cond.push((map(pos), map(neg), g));
                    trans.push((map(pos), map(neg), map(cp), map(cn), gm));
                }
            }
            LElem::B { pos, neg, cp, cn, g, gm, mode, gate, .. } => {
                if active(gate, modes) && modes[mode] == MODE_CV {
                    cond.push((map(pos), map(neg), g));
                    trans.push((map(pos), map(neg), map(cp), map(cn), gm));
                }
            }
            LElem::C { .. } => {}
        }
    }
    let c = &def.caps[k];
    port_resistance(m, &cond, &trans, map(c.a), map(c.b))
}

/// V(a) − V(b) when +1 A enters at `a` and leaves at `b`, for unknowns 0..m (GROUND = 0 V reference),
/// conductances (p, q, g) and transconductances (pos, neg, cp, cn, gm) stamped as a Vcvs. Components without
/// ground get their smallest node fixed at 0 V, as in the island solves.
pub(crate) fn port_resistance(
    m: usize,
    cond: &[(usize, usize, f64)],
    trans: &[(usize, usize, usize, usize, f64)],
    a: usize,
    b: usize,
) -> Port {
    if a == b {
        return Port::Finite(0.0);
    }
    let mut dsu = Dsu::new(m + 1);
    for &(p, q, _) in cond {
        dsu.union(slot(p, m), slot(q, m));
    }
    if dsu.find(slot(a, m)) != dsu.find(slot(b, m)) {
        return Port::Open;
    }
    let roots = dsu.roots();
    let fixed: Vec<usize> = (0..m).filter(|&i| roots[i] == i && roots[m] != i).collect();
    let mut y = vec![0.0; m * m];
    for &(p, q, g) in cond {
        stamp_g(&mut y, m, p, q, g);
    }
    for &(pos, neg, cp, cn, gm) in trans {
        stamp(&mut y, m, pos, cp, -gm);
        stamp(&mut y, m, pos, cn, gm);
        stamp(&mut y, m, neg, cp, gm);
        stamp(&mut y, m, neg, cn, -gm);
    }
    fix_rows(&mut y, m, &fixed);
    let mut rhs = vec![0.0; m];
    inject(&mut rhs, a, 1.0);
    inject(&mut rhs, b, -1.0);
    for &f in &fixed {
        rhs[f] = 0.0;
    }
    let Ok(lu) = Lu::factor(y, m) else { return Port::Singular };
    let mut x = vec![0.0; m];
    lu.solve(&rhs, &mut x);
    let r = volt(&x, a) - volt(&x, b);
    if r.is_finite() {
        Port::Finite(r)
    } else {
        Port::Singular
    }
}
