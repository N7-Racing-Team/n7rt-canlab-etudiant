//! Shared helpers of the Physical Lab engine tests (included with `#[path = "phys_kit.rs"] mod kit;`): the
//! circuit builder of `phys_bench.rs`, readings, diagnostics and commands on a [`PhysCore`].

#![allow(dead_code)]

#[path = "phys_bench.rs"]
mod bench;

use std::collections::BTreeMap;

pub use bench::*;
use n7rt_circuit::{persist, Circuit, CompId, Edit, Interact, PinRef, Value};
use n7rt_lab::phys::{CircuitProbe, CommandReply, Diagnostic, EditReply, PhysCommand, PhysCore, Reading};

// ------------------------------------------------------------------------------------------------ readings

pub fn pin(comp: CompId, pin: &str) -> PinRef {
    PinRef { comp, pin: pin.to_string() }
}

pub fn valid(r: Reading<f64>) -> f64 {
    match r {
        Reading::Valid { v } => v,
        other => panic!("expected a Valid reading, got {other:?}"),
    }
}

pub fn unavailable<T: std::fmt::Debug>(r: Reading<T>) -> String {
    match r {
        Reading::Unavailable { reason } => reason,
        other => panic!("expected Unavailable, got {other:?}"),
    }
}

/// |a − b| ≤ rel·|b| (plus a tiny absolute floor for values near zero).
pub fn assert_close(what: &str, got: f64, expected: f64, rel: f64) {
    let tol = rel * expected.abs() + 1e-15;
    assert!((got - expected).abs() <= tol, "{what}: {got} instead of {expected} (relative tolerance {rel})");
}

pub fn codes(core: &PhysCore) -> Vec<String> {
    core.frame().diagnostics().iter().map(|d| d.code.clone()).collect()
}

pub fn has(core: &PhysCore, code: &str) -> bool {
    core.frame().diagnostics().iter().any(|d| d.code == code)
}

pub fn diag(core: &PhysCore, code: &str) -> Option<Diagnostic> {
    core.frame().diagnostics().iter().find(|d| d.code == code).cloned()
}

// ------------------------------------------------------------------------------------------------ commands

pub fn edit(core: &mut PhysCore, edit: Edit) -> EditReply {
    let base_rev = core_rev(core);
    match core.apply(&PhysCommand::Edit { base_rev, edit }) {
        Ok(CommandReply::Edit(r)) => r,
        Ok(other) => panic!("edit reply expected, got {other:?}"),
        Err(r) => panic!("edit refused: {}", r.message_fr),
    }
}

/// Current reported revision (the document revision: tests use cores without offset).
pub fn core_rev(core: &PhysCore) -> u64 {
    core.document().rev()
}

pub fn set_param(core: &mut PhysCore, comp: CompId, name: &str, value: Value) -> EditReply {
    edit(core, Edit::SetParam { comp, name: name.to_string(), value })
}

pub fn interact(core: &mut PhysCore, i: Interact) {
    core.apply(&PhysCommand::Interact { interact: i }).unwrap_or_else(|r| panic!("interaction refused: {}", r.message_fr));
}

pub fn run(core: &mut PhysCore) {
    core.apply(&PhysCommand::Run).expect("run");
}

pub fn stop(core: &mut PhysCore) {
    core.apply(&PhysCommand::Stop).expect("stop");
}

pub fn steps(core: &mut PhysCore, n: u64) {
    for _ in 0..n {
        core.step();
    }
}

/// A core on `c`, in RUN for `frames` frames (STOP preview only when 0).
pub fn core_after(c: Circuit, frames: u64) -> PhysCore {
    let mut core = PhysCore::new(c);
    if frames > 0 {
        run(&mut core);
        steps(&mut core, frames);
    }
    core
}

// --------------------------------------------------------------------------------------------------- demos

pub fn demo(id: &str) -> Circuit {
    let d = n7rt_circuit::demos().iter().find(|d| d.id == id).unwrap_or_else(|| panic!("demo {id}"));
    persist::load(d.json).unwrap_or_else(|v| panic!("demo {id} loads: {v:?}"))
}

pub fn by_label(c: &Circuit, label: &str) -> CompId {
    c.components().values().find(|x| x.label == label).map(|x| x.id).unwrap_or_else(|| panic!("component {label}"))
}

pub fn labels(c: &Circuit) -> BTreeMap<String, CompId> {
    c.components().values().filter(|x| !x.label.is_empty()).map(|x| (x.label.clone(), x.id)).collect()
}

/// Discrete RC trajectory of the solver (contract §9.1): 2 backward Euler steps then trapezoidal, r = h/τ,
/// from v0 towards v_inf.
pub fn rc_scheme(n: u32, r: f64, v0: f64, v_inf: f64) -> f64 {
    rc_scheme_be(n, 2, r, v0, v_inf)
}

/// Same with `be` backward Euler steps first (a mode change in the first step adds one).
pub fn rc_scheme_be(n: u32, be: u32, r: f64, v0: f64, v_inf: f64) -> f64 {
    let mut e = 1.0;
    for k in 1..=n {
        e *= if k <= be { 1.0 / (1.0 + r) } else { (1.0 - r / 2.0) / (1.0 + r / 2.0) };
    }
    v_inf + (v0 - v_inf) * e
}

pub fn parallel(a: f64, b: f64) -> f64 {
    a * b / (a + b)
}
