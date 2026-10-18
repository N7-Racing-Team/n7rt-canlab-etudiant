//! The two embedded demonstrations against every expected value of contract §9.1 and §9.2: voltages within
//! 1e-6 relative, exact ADC codes, time constants checked against the discrete scheme of the solver (2 backward
//! Euler steps then trapezoidal, h = 1 ms) within 1e-9 relative. Every check also runs on a replica built
//! through the document API (`phys_bench.rs`), so that an engine failure is told apart from a demo file one.

#[path = "phys_kit.rs"]
mod kit;

use kit::*;
use kit::{ref_pot_rc as bench_ref_pot_rc, tp1_capteur_can as bench_tp1_capteur_can};
use n7rt_circuit::{Circuit, CompId, Interact, Value};
use n7rt_lab::phys::{CircuitProbe, LogicLevel, OpState, PhysCommand, PhysCore, Reading, RsMode, RunState, Severity, TxdState};

const H: f64 = 1e-3;
const V1: f64 = 5.0;
const R_POT: f64 = 1e4;
const R1: f64 = 1e4;
const C1: f64 = 1e-5;
/// r_out of V1 + r_on of S1.
const RS: f64 = 0.02;
const R_OFF: f64 = 1e9;
const R_OUT: f64 = 0.01;

struct RefPotRc {
    s1: CompId,
    rv1: CompId,
    r1: CompId,
    c1: CompId,
    adc1: CompId,
    adc2: CompId,
}

fn open_ref(c: Circuit) -> (PhysCore, RefPotRc) {
    let k = RefPotRc {
        s1: by_label(&c, "S1"),
        rv1: by_label(&c, "RV1"),
        r1: by_label(&c, "R1"),
        c1: by_label(&c, "C1"),
        adc1: by_label(&c, "ADC1"),
        adc2: by_label(&c, "ADC2"),
    };
    (PhysCore::new(c), k)
}

fn v(core: &PhysCore, comp: CompId, p: &str) -> f64 {
    valid(core.frame().voltage_at(&pin(comp, p)))
}

fn code(core: &PhysCore, comp: CompId, channel: &str) -> u32 {
    match core.frame().adc(comp, channel) {
        Reading::Valid { v } => v.code,
        other => panic!("ADC {channel}: {other:?}"),
    }
}

/// Code of an ADC input sampled by the STOP preview, at the frozen instant.
fn preview_code(core: &PhysCore, adc: CompId) -> u32 {
    let f = core.frame();
    match f.adc(adc, "in") {
        Reading::Valid { v } => {
            assert_eq!(v.sampled_at_ns, f.sim_time_ns(), "sampled at the frozen instant");
            assert_close("sample = V(in) − V(ref) of the preview", v.volts, valid(f.voltage_between_pins(&pin(adc, "in"), &pin(adc, "ref"))), 0.0);
            v.code
        }
        other => panic!("ADC: {other:?}"),
    }
}

/// Unrounded code q = volts · 2^bits / vref of the last sample of an ADC channel.
fn q(core: &PhysCore, comp: CompId, channel: &str) -> f64 {
    match core.frame().adc(comp, channel) {
        Reading::Valid { v } => v.volts * 2f64.powi(v.bits as i32) / v.vref,
        other => panic!("ADC {channel}: {other:?}"),
    }
}

/// The contract gives q to two decimals.
fn assert_q(what: &str, got: f64, expected: f64) {
    assert!((got - expected).abs() <= 0.005 + 1e-9, "{what}: q = {got} instead of {expected}");
}

/// V(W) at steady state, switch closed: 5·R_AW / (R + R_s).
fn w_closed(alpha: f64) -> f64 {
    V1 * alpha * R_POT / (R_POT + RS)
}

fn tau_closed(alpha: f64, r1: f64, c1: f64) -> f64 {
    c1 * (r1 + parallel(alpha * R_POT, (1.0 - alpha) * R_POT + RS))
}

/// RUN `n` frames from the current state and check V(X) against the exact oracle of the discrete scheme
/// (§9.1: tolerance 1e-12 of the voltage scale).
fn check_transient(core: &mut PhysCore, k: &RefPotRc, n: u32, tau: f64, v0: f64, v_inf: f64) -> f64 {
    let scale = v0.abs().max(v_inf.abs());
    let mut vx = v0;
    for i in 1..=n {
        core.step();
        vx = v(core, k.c1, "p1");
        let oracle = rc_scheme(i, H / tau, v0, v_inf);
        assert!((vx - oracle).abs() <= 1e-12 * scale, "V(X) after {i} frame(s), τ = {tau}: {vx} instead of {oracle}");
    }
    vx
}

#[test]
fn ref_pot_rc_charge_from_reset() {
    ref_pot_rc_charge_from_reset_on(demo("ref-pot-rc"));
}

#[test]
fn replica_ref_pot_rc_charge_from_reset() {
    ref_pot_rc_charge_from_reset_on(bench_ref_pot_rc());
}

fn ref_pot_rc_charge_from_reset_on(c: Circuit) {
    let (mut core, k) = open_ref(c);
    // Loaded = reset state: S1 closed, α = 0.3, C1 discharged; the STOP preview shows 0+.
    let (r_aw, r_wb) = (0.3 * R_POT, 0.7 * R_POT);
    let w0 = V1 * parallel(r_aw, R1) / (parallel(r_aw, R1) + r_wb + RS);
    assert_close("V(W) at 0+ (analytic)", v(&core, k.rv1, "w"), w0, 1e-7);
    assert_close("V(W) at 0+ (§9.1)", v(&core, k.rv1, "w"), 1.2396668, 1e-6);
    // The STOP preview samples at 0+: q = 1015.54 (§9.1), C1 still empty.
    assert_eq!(preview_code(&core, k.adc1), 1015);
    assert_q("ADC1 at 0+", q(&core, k.adc1, "in"), 1015.54);
    assert_eq!(preview_code(&core, k.adc2), 0);
    charge_from_0_plus(&mut core, &k);
}

/// From the 0+ state in STOP (C1 discharged, α = 0.3, S1 closed): RUN, the §9.1 transient and steady state.
fn charge_from_0_plus(core: &mut PhysCore, k: &RefPotRc) {
    let tau = tau_closed(0.3, R1, C1);
    assert_close("τ (§9.1)", tau, 0.121, 1e-6);
    let v_inf = w_closed(0.3);
    assert_close("V(W) final (§9.1)", v_inf, 1.4999970, 1e-6);
    run(core);
    let vx = check_transient(core, k, 121, tau, 0.0, v_inf);
    assert_close("V(X) at t = τ (§9.1, scheme)", vx, 0.9481445, 1e-6);
    let exact = v_inf * (1.0 - (-1.0f64).exp());
    assert_close("V(X) at t = τ (§9.1, exact)", exact, 0.948179, 1e-6);
    assert!((vx - exact).abs() / v_inf <= 1e-3, "scheme within 0.1 point of the exact charge (§9.3)");
    assert_eq!(code(core, k.adc2, "in"), 776);
    assert_q("ADC2 at t = τ", q(core, k.adc2, "in"), 776.72);
    match core.frame().adc(k.adc2, "in") {
        Reading::Valid { v } => {
            assert_eq!(v.sampled_at_ns, 121_000_000);
            assert_eq!((v.bits, v.vref), (12, 5.0));
        }
        other => panic!("{other:?}"),
    }

    steps(core, 3000);
    assert_close("V(X) final", v(core, k.c1, "p1"), 1.4999970, 1e-6);
    assert_close("V(W)", v(core, k.rv1, "w"), 1.4999970, 1e-6);
    assert_eq!(code(core, k.adc2, "in"), 1228);
    assert_eq!(code(core, k.adc1, "in"), 1228);
    assert_q("ADC1 at steady state", q(core, k.adc1, "in"), 1228.80);
    assert_q("ADC2 at steady state", q(core, k.adc2, "in"), 1228.80);
    let f = core.frame();
    assert_close("V(B)", valid(f.voltage_at(&pin(k.rv1, "b"))), 4.9999900, 1e-6);
    assert_close("I(RV1) entering by B", valid(f.pin_current(&pin(k.rv1, "b"))), 499.999e-6, 1e-6);
    assert_close("I(RV1) entering by A", valid(f.pin_current(&pin(k.rv1, "a"))), -499.999e-6, 1e-6);
    assert_close("P(RV1)", valid(f.power(k.rv1)), 2.49999e-3, 1e-6);
    assert!(valid(f.pin_current(&pin(k.rv1, "w"))).abs() < 1e-12, "no current in R1 at steady state");
    assert_eq!(f.op_state(k.s1), Some(OpState::Contact { closed: true }));
    assert_eq!(f.op_state(k.rv1), Some(OpState::Pot { position: 0.3 }));
    let bad: Vec<_> = f.diagnostics().iter().filter(|d| d.severity != Severity::Info).collect();
    assert!(bad.is_empty(), "no error nor warning: {bad:?}");
}

fn steady(core: &mut PhysCore) {
    run(core);
    steps(core, 3500);
}

#[test]
fn ref_pot_rc_reset_then_run() {
    ref_pot_rc_reset_then_run_on(demo("ref-pot-rc"));
}

#[test]
fn replica_ref_pot_rc_reset_then_run() {
    ref_pot_rc_reset_then_run_on(bench_ref_pot_rc());
}

/// §9.1 row « RESET puis RUN, α = 0,3 », from a charged C1: RESET in RUN, then STOP, RESET, RUN.
fn ref_pot_rc_reset_then_run_on(c: Circuit) {
    let (mut core, k) = open_ref(c);
    steady(&mut core);
    assert_eq!(code(&core, k.adc2, "in"), 1228);

    // RESET in RUN: still RUN, t = 0, C1 discharged; the next frame is the first one of the §9.1 transient.
    core.apply(&PhysCommand::Reset).expect("reset");
    assert_eq!((core.run_state(), core.frame_index()), (RunState::Run, 0));
    let tau = tau_closed(0.3, R1, C1);
    let vx = check_transient(&mut core, &k, 121, tau, 0.0, w_closed(0.3));
    assert_close("V(X) at t = τ after RESET in RUN (§9.1, scheme)", vx, 0.9481445, 1e-6);
    assert_eq!(code(&core, k.adc2, "in"), 776);
    steps(&mut core, 3000);

    // RESET in STOP: the preview shows 0+ (V(W) = 1.2396668 V, q = 1015.54), then RUN charges C1 again.
    stop(&mut core);
    core.apply(&PhysCommand::Reset).expect("reset");
    assert_eq!((core.run_state(), core.frame().sim_time_ns()), (RunState::Stop, 0));
    // Discharged: the 1 ns preview step (§5.6) only moves X by ~ V(W)·1 ns/(R1·C1) ≈ 12 nV.
    assert!(v(&core, k.c1, "p1").abs() < 1e-7, "C1 discharged");
    assert_close("V(W) at 0+ after RESET (§9.1)", v(&core, k.rv1, "w"), 1.2396668, 1e-6);
    assert_eq!(preview_code(&core, k.adc1), 1015);
    assert_q("ADC1 at 0+ after RESET", q(&core, k.adc1, "in"), 1015.54);
    assert_eq!(preview_code(&core, k.adc2), 0);
    assert_eq!(core.frame().op_state(k.s1), Some(OpState::Contact { closed: true }));
    assert_eq!(core.frame().op_state(k.rv1), Some(OpState::Pot { position: 0.3 }));
    charge_from_0_plus(&mut core, &k);
}

#[test]
fn ref_pot_rc_wiper_to_0_7_then_switch_open() {
    ref_pot_rc_wiper_to_0_7_then_switch_open_on(demo("ref-pot-rc"));
}

#[test]
fn replica_ref_pot_rc_wiper_to_0_7_then_switch_open() {
    ref_pot_rc_wiper_to_0_7_then_switch_open_on(bench_ref_pot_rc());
}

fn ref_pot_rc_wiper_to_0_7_then_switch_open_on(c: Circuit) {
    let (mut core, k) = open_ref(c);
    steady(&mut core);
    let vx = v(&core, k.c1, "p1");
    assert_close("X at steady state", vx, w_closed(0.3), 1e-9);

    // Wiper 0.3 → 0.7: the STOP preview shows 0+.
    stop(&mut core);
    interact(&mut core, Interact::SetPot { comp: k.rv1, position: 0.7 });
    let (r_aw, r_wb) = (7000.0, 3000.0);
    let w0 = (V1 / (r_wb + RS) + vx / R1) / (1.0 / r_aw + 1.0 / (r_wb + RS) + 1.0 / R1);
    assert_close("V(W) at 0+ (analytic)", v(&core, k.rv1, "w"), w0, 1e-7);
    assert_close("V(W) at 0+ (§9.1)", v(&core, k.rv1, "w"), 3.1528849, 1e-6);
    assert_eq!(preview_code(&core, k.adc1), 2582, "§9.1: ADC1 = 2582 at 0+ (q = 2582.84)");
    assert_q("ADC1 at 0+, α = 0.7", q(&core, k.adc1, "in"), 2582.84);
    run(&mut core);
    let tau = tau_closed(0.7, R1, C1);
    assert_close("τ", tau, 0.121, 1e-6);
    check_transient(&mut core, &k, 60, tau, vx, w_closed(0.7));
    steps(&mut core, 3500);
    assert_close("V(W) at α = 0.7 (§9.1)", v(&core, k.rv1, "w"), 3.4999930, 1e-6);
    assert_eq!(code(&core, k.adc1, "in"), 2867);
    assert_q("ADC1 at α = 0.7", q(&core, k.adc1, "in"), 2867.19);

    // S1 opened from α = 0.7: W keeps its path to the ground through R_AW.
    let vx = v(&core, k.c1, "p1");
    stop(&mut core);
    interact(&mut core, Interact::SetSwitch { comp: k.s1, closed: false });
    let g_supply = 1.0 / (r_wb + R_OFF + R_OUT);
    let w0 = (vx / R1 + V1 * g_supply) / (1.0 / r_aw + g_supply + 1.0 / R1);
    assert_close("V(W) after opening (analytic)", v(&core, k.rv1, "w"), w0, 1e-7);
    assert_close("V(W) after opening (§9.1)", v(&core, k.rv1, "w"), 1.4411882, 1e-6);
    assert_eq!(preview_code(&core, k.adc1), 1180, "§9.1: ADC1 = 1180 at 0+ (q = 1180.62)");
    assert_q("ADC1 after opening from α = 0.7", q(&core, k.adc1, "in"), 1180.62);
    run(&mut core);
    let tau = C1 * (R1 + parallel(r_aw, r_wb + R_OFF + R_OUT));
    assert_close("τ opened (§9.1)", tau, 0.1699995, 1e-6);
    let w_final = V1 * r_aw / (r_aw + r_wb + R_OFF + R_OUT);
    check_transient(&mut core, &k, 60, tau, vx, w_final);
    steps(&mut core, 9000);
    assert_close("V(W) final, opened (§9.1: 35.0 µV)", v(&core, k.rv1, "w"), w_final, 1e-6);
    assert_close("35.0 µV", w_final, 35.0e-6, 1e-3);
    assert_eq!(code(&core, k.adc1, "in"), 0);
    assert_eq!(core.frame().op_state(k.s1), Some(OpState::Contact { closed: false }));
}

#[test]
fn ref_pot_rc_switch_open_from_0_3() {
    ref_pot_rc_switch_open_from_0_3_on(demo("ref-pot-rc"));
}

#[test]
fn replica_ref_pot_rc_switch_open_from_0_3() {
    ref_pot_rc_switch_open_from_0_3_on(bench_ref_pot_rc());
}

fn ref_pot_rc_switch_open_from_0_3_on(c: Circuit) {
    let (mut core, k) = open_ref(c);
    steady(&mut core);
    let vx = v(&core, k.c1, "p1");
    stop(&mut core);
    interact(&mut core, Interact::SetSwitch { comp: k.s1, closed: false });
    let (r_aw, r_wb) = (3000.0, 7000.0);
    let g_supply = 1.0 / (r_wb + R_OFF + R_OUT);
    let w0 = (vx / R1 + V1 * g_supply) / (1.0 / r_aw + g_supply + 1.0 / R1);
    assert_close("V(W) after opening (analytic)", v(&core, k.rv1, "w"), w0, 1e-7);
    assert_close("V(W) after opening (§9.1)", v(&core, k.rv1, "w"), 0.3461639, 1e-6);
    assert_eq!(preview_code(&core, k.adc1), 283, "§9.1: ADC1 = 283 at 0+ (q = 283.58)");
    assert_q("ADC1 after opening from α = 0.3", q(&core, k.adc1, "in"), 283.58);
    run(&mut core);
    let tau = C1 * (R1 + parallel(r_aw, r_wb + R_OFF + R_OUT));
    assert_close("τ (§9.1)", tau, 0.1299999, 1e-6);
    let w_final = V1 * r_aw / (r_aw + r_wb + R_OFF + R_OUT);
    check_transient(&mut core, &k, 60, tau, vx, w_final);
    steps(&mut core, 8000);
    assert_close("V(W) final (§9.1: 15.0 µV)", v(&core, k.rv1, "w"), w_final, 1e-6);
    assert_close("15.0 µV", w_final, 15.0e-6, 1e-3);
    // W stays Referenced: its path to the ground is R_AW, not a leakage.
    assert!(matches!(core.frame().voltage_at(&pin(k.rv1, "w")), Reading::Valid { .. }));
}

#[test]
fn ref_pot_rc_wiper_ends() {
    ref_pot_rc_wiper_ends_on(demo("ref-pot-rc"));
}

#[test]
fn replica_ref_pot_rc_wiper_ends() {
    ref_pot_rc_wiper_ends_on(bench_ref_pot_rc());
}

fn ref_pot_rc_wiper_ends_on(c: Circuit) {
    let (mut core, k) = open_ref(c);
    interact(&mut core, Interact::SetPot { comp: k.rv1, position: 0.0 });
    steady(&mut core);
    assert_eq!(code(&core, k.adc1, "in"), 0);
    assert_eq!(code(&core, k.adc2, "in"), 0);
    interact(&mut core, Interact::SetPot { comp: k.rv1, position: 1.0 });
    steps(&mut core, 3500);
    assert_eq!(code(&core, k.adc1, "in"), 4095);
    assert_eq!(code(&core, k.adc2, "in"), 4095);
}

#[test]
fn ref_pot_rc_r1_then_c1_edited_in_run() {
    ref_pot_rc_r1_then_c1_edited_in_run_on(demo("ref-pot-rc"));
}

#[test]
fn replica_ref_pot_rc_r1_then_c1_edited_in_run() {
    ref_pot_rc_r1_then_c1_edited_in_run_on(bench_ref_pot_rc());
}

fn ref_pot_rc_r1_then_c1_edited_in_run_on(c: Circuit) {
    let (mut core, k) = open_ref(c);
    set_param(&mut core, k.r1, "resistance", Value::Num(22e3));
    run(&mut core);
    let tau = tau_closed(0.3, 22e3, C1);
    assert_close("τ with R1 = 22 kΩ (§9.1)", tau, 0.241, 1e-6);
    let vx = check_transient(&mut core, &k, 100, tau, 0.0, w_closed(0.3));
    let code_before = code(&core, k.adc2, "in");
    let t_before = core.frame().sim_time_ns();

    // C1 = 4.7 µF in RUN, mid-transient: no restart, the capacitor keeps its voltage (not its charge).
    set_param(&mut core, k.c1, "capacitance", Value::Num(4.7e-6));
    let tau = tau_closed(0.3, 22e3, 4.7e-6);
    assert_close("τ with C1 = 4.7 µF (§9.1)", tau, 0.11327, 1e-6);
    core.step();
    let v1 = rc_scheme(1, H / tau, vx, w_closed(0.3));
    assert_close("V(X) one frame after the edit", v(&core, k.c1, "p1"), v1, 1e-12);
    let first_code_after = code(&core, k.adc2, "in");
    assert_eq!(first_code_after, (v1 * 4096.0 / 5.0).floor() as u32);
    // No jump: one frame of the new transient (about 7 codes here), nothing like the charge kept (×10/4.7).
    let natural = ((w_closed(0.3) - vx) * (H / tau) * 4096.0 / 5.0).ceil() as u32 + 1;
    assert!(first_code_after.abs_diff(code_before) <= natural, "no jump of ADC2: {code_before} then {first_code_after}");
    let charge_kept = (vx * 1e-5 / 4.7e-6 * 4096.0 / 5.0).floor() as u32;
    assert!(charge_kept > first_code_after + 100);
    for i in 2..=60 {
        core.step();
        assert_close(&format!("V(X) {i} frames after the edit"), v(&core, k.c1, "p1"), rc_scheme(i, H / tau, vx, w_closed(0.3)), 1e-12);
    }
    assert_eq!(core.frame().sim_time_ns(), t_before + 60_000_000, "time goes on");
    steps(&mut core, 3000);
    assert_close("final value unchanged", v(&core, k.c1, "p1"), 1.4999970, 1e-6);
    assert_eq!(code(&core, k.adc2, "in"), 1228);
}

// ------------------------------------------------------------------------------------------------ tp1

#[test]
fn tp1_capteur_can_expected_values() {
    tp1_expected_values_on(demo("tp1-capteur-can"));
}

#[test]
fn replica_tp1_capteur_can_expected_values() {
    tp1_expected_values_on(bench_tp1_capteur_can());
}

fn tp1_expected_values_on(c: Circuit) {
    let (u1, u2, bus1) = (by_label(&c, "U1"), by_label(&c, "U2"), by_label(&c, "BUS1"));
    let mut core = PhysCore::new(c);
    // The STOP preview of the loaded demo (inspector of U2 and BUS1).
    let f = core.frame();
    assert_eq!(f.voltage_between_pins(&pin(u2, "canh"), &pin(u2, "canl")), Reading::Valid { v: 0.0 });
    assert_eq!(f.voltage_between_pins(&pin(bus1, "canh"), &pin(bus1, "canl")), Reading::Valid { v: 0.0 });
    run(&mut core);

    // A1 behind R1-C1: τ = C1·(R1 + R_AW ∥ (R_WB + r_out_3v3)), final α·V3V3 (ratiometric).
    let r_out_3v3 = 0.05;
    let v3v3 = 3.3 * R_POT / (R_POT + r_out_3v3);
    let a_inf = 0.3 * v3v3;
    let tau = C1 * (R1 + parallel(0.3 * R_POT, 0.7 * R_POT + r_out_3v3));
    assert_close("τ of A1 (§9.2)", tau, 0.121, 1e-6);
    // The power-on reset gate of U2 switches on in the first sub-step: a commutation of the island, hence
    // backward Euler for the frame that sees it and the 2 following ones (contract §5.5), then trapezoidal.
    for n in 1..=121 {
        core.step();
        assert_close(&format!("A1 after {n} frame(s)"), v(&core, u1, "a1"), rc_scheme_be(n, 3, H / tau, 0.0, a_inf), 1e-12);
    }
    steps(&mut core, 3500);
    let f = core.frame();
    let at = |comp: CompId, p: &str| valid(f.voltage_at(&pin(comp, p)));

    assert_close("rail 5V", at(u1, "v5"), 4.9989934, 1e-6);
    assert_close("rail 5V current (R_DD and TXD pull-up)", -valid(f.pin_current(&pin(u1, "v5"))), 10.066e-3, 1e-4);
    assert_close("rail 3V3", at(u1, "v3v3"), 3.2999835, 1e-6);
    assert_close("A0", at(u1, "a0"), 0.9899951, 1e-6);
    match f.adc(u1, "a0") {
        Reading::Valid { v } => {
            assert_eq!(v.code, 1228);
            assert_close("Vref = V(3V3)", v.vref, 3.2999835, 1e-6);
        }
        other => panic!("A0: {other:?}"),
    }
    assert_q("A0, ratiometric α·4096", q(&core, u1, "a0"), 1228.8);
    assert_eq!(code(&core, u1, "a1"), 1228);
    // Transceiver: powered (VDD > 4.05 V), Rs on the net of VSS (high speed), TXD high (recessive).
    let vdd = at(u2, "vdd");
    assert_close("VDD of U2", vdd, 4.9989934, 1e-6);
    assert!(vdd > 4.05);
    assert!(f.net_of(&pin(u2, "rs")).is_some());
    assert_eq!(f.net_of(&pin(u2, "rs")), f.net_of(&pin(u2, "vss")));
    assert_eq!(f.op_state(u2), Some(OpState::Transceiver { powered: true, rs: RsMode::HighSpeed, txd: TxdState::Recessive }));
    assert_eq!(f.op_state(u1), Some(OpState::Ecu { powered: true }));
    assert_close("TXD", at(u2, "txd"), 3.3033747, 1e-6);
    assert_eq!(f.logic(&pin(u2, "txd")), Reading::Valid { v: LogicLevel::High });
    assert_close("RXD", at(u2, "rxd"), 4.9989934, 1e-6);
    assert_close("PD0", at(u1, "can_rx"), 4.9989934, 1e-6);
    assert!(at(u1, "can_rx") < 7.3, "under the FT limit of PD0");
    assert_eq!(f.logic(&pin(u1, "can_rx")), Reading::Valid { v: LogicLevel::High });
    assert_eq!(f.logic(&pin(u2, "rxd")), Reading::Valid { v: LogicLevel::High });
    assert_close("CANH", at(u2, "canh"), 2.4997483, 1e-6);
    assert_close("CANL", at(u2, "canl"), 2.4997483, 1e-6);
    // Two equal potentials: exactly 0, never a cancellation residue of the solve (contract §6).
    assert_eq!(f.voltage_between_pins(&pin(u2, "canh"), &pin(u2, "canl")), Reading::Valid { v: 0.0 });
    assert_eq!(f.voltage_between_pins(&pin(bus1, "canh"), &pin(bus1, "canl")), Reading::Valid { v: 0.0 });
    assert_eq!(f.power(u2), Reading::Unavailable { reason: "ic_model".into() });

    // Switched off, as an ohmmeter sees it (M1, M2 of hardware-wiring).
    let r = valid(f.unpowered_resistance(&pin(u2, "canh"), &pin(u2, "canl")));
    assert_close("R(CANH, CANL) unpowered", r, 1.0 / (2.0 / 120.0 + 2.0 / 50e3), 1e-9);
    assert_close("R(CANH, CANL) unpowered (§9.2)", r, 59.8563, 1e-6);
    assert_eq!(f.unpowered_resistance(&pin(u2, "canh"), &pin(u1, "gnd")), Reading::Open);
    let bad: Vec<_> = f.diagnostics().iter().filter(|d| d.severity != Severity::Info).collect();
    assert!(bad.is_empty(), "no error nor warning (§9.2): {bad:?}");

    // Architecture §10.5: two remote nodes.
    stop(&mut core);
    set_param(&mut core, bus1, "remote_nodes", Value::Int(2));
    let r = valid(core.frame().unpowered_resistance(&pin(u2, "canh"), &pin(u2, "canl")));
    assert_close("R(CANH, CANL), 2 remote nodes", r, 1.0 / (2.0 / 120.0 + 1.0 / 50e3 + 2.0 / 50e3), 1e-9);
    assert_close("R(CANH, CANL), 2 remote nodes (§9.2)", r, 59.7848, 1e-6);
    // Recessive bus: common mode 2.5 V ± 0.5 V, |ΔV| < 50 mV (§9.3).
    let f = core.frame();
    let cm = (valid(f.voltage_at(&pin(u2, "canh"))) + valid(f.voltage_at(&pin(u2, "canl")))) / 2.0;
    assert!((cm - 2.5).abs() <= 0.5, "common mode {cm}");
    assert_eq!(f.voltage_between_pins(&pin(u2, "canh"), &pin(u2, "canl")), Reading::Valid { v: 0.0 });
}

// ------------------------------------------------------------------------------------------------ perf

#[test]
fn perf_fixture_runs_with_every_island_solved() {
    let c = demo("perf-150-300");
    let comps = c.components().len();
    let mut core = PhysCore::new(c);
    run(&mut core);
    steps(&mut core, 200);
    let f = core.frame();
    assert_eq!(f.sim_time_ns(), 200_000_000);
    for code in ["solver_singular", "solver_mode_cycle", "internal_error"] {
        assert!(!has(&core, code), "{code}: {:?}", diag(&core, code));
    }
    let v = core.values(1, n7rt_lab::phys::Disclosure::Exploration, Default::default());
    assert_eq!(v.comps.len(), comps);
    for c in &v.comps {
        for p in &c.pins {
            for r in [&p.voltage, &p.current] {
                if let Reading::Unavailable { reason } = r {
                    assert!(reason == "dominant_non_modelise_v1" || reason == "txd_undefined", "{:?}.{}: {reason}", c.id, p.pin);
                }
            }
        }
    }
}
