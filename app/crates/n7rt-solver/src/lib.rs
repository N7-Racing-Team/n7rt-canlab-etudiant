//! n7rt-solver — deterministic piecewise-linear circuit solver of the N7RT Physical Lab.
//!
//! Contract: docs/physical-lab-v1.md §5 (architecture §10.1, §10.4, §14). Pure numerics: this crate knows nothing
//! about components, schematics or the UI. `n7rt-lab::phys` lowers a circuit into a [`Network`] (nodes = nets plus
//! component-internal nodes) and reads results back by node, element index and state key.
//!
//! Normative method (the contract gives the stamps, the derivations and the evidence):
//! - **Islands.** Graph whose vertices are all nodes except `ground`, with one edge per element branch (leakage
//!   resistors and gated elements included whatever their state), the edges (pos, cp) and (pos, cn) of every Vcvs,
//!   and for every [`Gate`] the edges from its control nodes to the nodes of the elements it gates. Each connected
//!   component is an island. Islands share only the ground node (0 V), so each is solved on its own, with its own
//!   step. An island whose canonical signature (elements in network order, nodes renumbered by rank inside the
//!   island, values bitwise, keys, gates) equals one of the previous load keeps its state and is not commuted.
//! - **Nodal analysis**, Norton equivalents only (no branch unknown), dense LU with partial pivoting written in this
//!   crate: pivot = first index of strictly maximal |a| in ascending order; the island is singular when
//!   |u_kk| ≤ [`PIVOT_TOL`] · max_j |a_kj| of the original row. The matrix is not symmetric (Vcvs).
//! - **Regularity without gmin.** In every solve, inside an island, each component of the conductance graph
//!   (resistors including leakage, capacitor companions, Supply in CV, VSource, output conductance of a Vcvs, gated
//!   elements only when on) that does not contain ground gets its smallest node fixed at 0 V (KCL row replaced by
//!   v = 0); a node without any conductance is fixed at 0 V. Those potentials are artefacts: they are classified
//!   [`NodeClass::Relative`] or [`NodeClass::Floating`] and never published as measurements.
//! - **Modes** (piecewise linear elements, resolved *inside* every sub-step, preview and static solve):
//!   [`SupplyMode`] of each Supply (one quadrant: CC < CV < REV along x = V(pos) − V(neg)) and the on/off state of
//!   each Gate (hysteresis comparator). Loop: start from the stored modes; a Supply in CC whose pos and neg are not
//!   linked by any other conductance path is set to CV; solve; check every mode element in ascending key order
//!   with the band [`MODE_BAND_V`]; move the first incoherent one by one position, switch the sub-step to backward
//!   Euler and solve again; at most 4·m + 4 solves (m = mode elements of the island), otherwise
//!   [`IslandStatus::ModeCycle`]. Any mode change is a commutation.
//! - **Transient.** Capacitor companions: trapezoidal (G = 2C/h, J = G·v + i) or backward Euler (G = C/h, J = G·v).
//!   Backward Euler is used by a capacitor that closes a loop of capacitors (mesh capacitor), by a capacitor whose
//!   time constant τ_k is below the island step, and by every capacitor of an island during the
//!   [`BE_AFTER_COMMUTATION`] sub-steps that follow a commutation (load of a changed island, reset, mode change).
//! - **Step of an island**, chosen at each frame boundary from the modes at that instant: capacitor forest by
//!   union-find in ascending key order; for each tree capacitor k, R_k = resistance between its terminals with the
//!   other tree capacitors shorted, mesh capacitors open, Supply in CV and VSource as 1/r_out, Supply in CC or REV
//!   and gated-off elements open, Vcvs active, leakage included; τ_k = C_k·R_k. τ_lb = 1 / Σ 1/τ_k over the tree
//!   capacitors with finite τ_k ≥ [`TAU_QUASI_STATIC_S`] (a lower bound of the fastest time constant of a passive
//!   RC network; a documented heuristic with Vcvs). dt_target = clamp(τ_lb / 20, [`DT_MIN_NS`], 1 ms) and
//!   dt = FRAME_NS / N with N the first entry of [`SUBSTEP_COUNTS`] such that FRAME_NS / N ≤ dt_target: every
//!   sub-step instant is an exact integer number of ns. No capacitor: N = 1 (static island, same mode loop).
//!   [écart] architecture §10.1 sets a 1 µs floor; 10 µs bounds the cost to 100 sub-steps per island and frame
//!   (architecture §14). Dynamics faster than 10 µs are quasi-static: exact steady state, error ≤ τ_k/dt on the
//!   first sub-step after a commutation.
//! - **Preview** (STOP, after an edit or an interaction): one backward Euler step of [`PREVIEW_H_S`] from the stored
//!   state, with the mode loop, published as preview; the stored state (t, capacitor v and i, modes, gates,
//!   commutation counters) is left unchanged. Capacitors are held to within h/τ_k and parallel capacitors at
//!   different voltages show their charge redistribution.
//! - **Classification** (graph K, on the accepted solution): vertices = all nodes including ground; edges = non
//!   leakage resistors, capacitors, VSource, Supply in CV, Vcvs outputs (gated elements only when on). Excluded:
//!   leakage, Supply in CC or REV, Vcvs control ports. A node is Referenced if its K group contains ground, Relative
//!   if its K group has at least 2 nodes, Floating otherwise. Gates are decided only when cp and cn are in the
//!   same K group; otherwise their coherent state is off.
//! - **Determinism.** Same network, same state, same calls ⇒ bit-identical results and [`Solver::state_digest`].
//!   Nodes of an island in ascending NodeId order, stamps in network order then branch order, scalar loops in a
//!   fixed order, no HashMap iteration, no threads, no BLAS / LAPACK / SIMD-dispatching crate, no `mul_add`,
//!   `powi` or transcendental function.

mod dsu;
mod engine;
mod island;
mod lu;
mod tau;
#[cfg(test)]
mod tau_tests;

use std::collections::{BTreeMap, BTreeSet};

use engine::{Cache, Counters, Island};
use island::{GATE_ON, MODE_CC, MODE_CV};

pub type NodeId = usize;
/// Stable identity of a stateful element (capacitor charge, supply mode, gate state) across reloads. Unique in a
/// network across capacitors, supplies and gates.
pub type StateKey = u64;
/// Index into [`Network::gates`].
pub type GateId = usize;

/// One logical step of the simulation (architecture §10.1).
pub const FRAME_NS: i64 = 1_000_000;
/// Admissible sub-step counts per frame: the divisors N of 10^6 with FRAME_NS / N ≥ DT_MIN_NS.
pub const SUBSTEP_COUNTS: [i64; 15] = [1, 2, 4, 5, 8, 10, 16, 20, 25, 32, 40, 50, 64, 80, 100];
/// Smallest island step ([écart] architecture §10.1: 1 µs there).
pub const DT_MIN_NS: i64 = 10_000;
/// Capacitors with τ_k below this bound are quasi-static: backward Euler, excluded from τ_lb.
pub const TAU_QUASI_STATIC_S: f64 = 1e-5;
/// Duration of the preview step.
pub const PREVIEW_H_S: f64 = 1e-9;
/// Hysteresis band of the Supply mode boundaries, in volts.
pub const MODE_BAND_V: f64 = 1e-9;
/// Relative pivot threshold of the LU factorization.
pub const PIVOT_TOL: f64 = 1e-13;
/// max|u_kk| / min|u_kk| above which an island is reported ill-conditioned (results still published).
pub const ILL_CONDITIONED_RATIO: f64 = 1e14;
/// Backward Euler sub-steps after a commutation.
pub const BE_AFTER_COMMUTATION: u32 = 2;

#[derive(Clone, Debug, PartialEq)]
pub enum Element {
    /// Linear resistor, r > 0. `leakage` marks unintended high-value model paths (open contact r_off, ADC input
    /// resistance ≥ 1 MΩ): they conduct in the solution but never make a node Referenced or Relative. Absent from
    /// the matrix while its gate is off (input network of an unpowered integrated circuit).
    Resistor { a: NodeId, b: NodeId, r: f64, leakage: bool, gate: Option<GateId> },
    /// Capacitor, c > 0. v = V(a) − V(b) and i (current entering by `a`) are state, kept across reloads under
    /// `key`; a changed `c` keeps v (not the charge). Discharged by [`Solver::reset`] and when its key disappears.
    Capacitor { a: NodeId, b: NodeId, c: f64, key: StateKey },
    /// Linear Thevenin source, two quadrants: ideal `v` = V(pos) − V(neg) in series with r_out > 0. Used for model
    /// biases (remote CAN nodes). Absent from the matrix while its gate is off.
    VSource { pos: NodeId, neg: NodeId, v: f64, r_out: f64, gate: Option<GateId> },
    /// One-quadrant supply (bench supply, regulator rail): v ≥ 0 behind r_out > 0, output current
    /// I = (v − x) / r_out out of `pos` limited to [0 ; i_lim] (x = V(pos) − V(neg)). Modes: CC (I = i_lim, no
    /// conductance), CV (Norton), REV (open: the supply cannot sink current). Without `i_lim`: CV and REV only.
    /// `pos == neg` needs no special case: CC at i_lim if limited.
    Supply { pos: NodeId, neg: NodeId, v: f64, r_out: f64, i_lim: Option<f64>, key: StateKey, gate: Option<GateId> },
    /// Voltage-controlled voltage source with series resistance: open-circuit output
    /// V(pos) − V(neg) = gain · (V(cp) − V(cn)), output resistance r_out > 0, control inputs draw no current.
    /// Absent from the matrix while its gate is off.
    Vcvs { pos: NodeId, neg: NodeId, cp: NodeId, cn: NodeId, gain: f64, r_out: f64, gate: Option<GateId> },
    /// Ratiometric Thevenin driver with a symmetric output-current limit. The control port draws no current;
    /// output modes are CC+ / CV / CC-. Used for externally powered sensor output stages.
    LimitedVcvs { pos: NodeId, neg: NodeId, cp: NodeId, cn: NodeId, gain: f64, r_out: f64, i_lim: f64, key: StateKey, gate: Option<GateId> },
}

/// Hysteresis comparator on u = V(cp) − V(cn) (integrated circuit power-on reset). Off → on when u > on_above,
/// on → off when u < off_below; requires on_above > off_below. Off after [`Solver::reset`].
#[derive(Clone, Debug, PartialEq)]
pub struct Gate {
    pub cp: NodeId,
    pub cn: NodeId,
    pub on_above: f64,
    pub off_below: f64,
    pub key: StateKey,
}

/// A circuit lowered to numbers. Node indices are `0..nodes`; `ground` is the 0 V reference if the circuit has one.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Network {
    pub nodes: usize,
    pub ground: Option<NodeId>,
    pub elements: Vec<Element>,
    pub gates: Vec<Gate>,
}

/// Refused network: the lowering produced something the solver cannot represent. Always a bug of the caller;
/// the solver keeps its previous network and state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NetworkError {
    NodeOutOfRange { element: usize },
    /// Non-finite value, r / c / r_out ≤ 0, i_lim ≤ 0, Supply v < 0.
    BadValue { element: usize, field: &'static str },
    /// Unknown gate index, or a gate with on_above ≤ off_below or a node out of range.
    BadGate { gate: usize },
    DuplicateKey { key: StateKey },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeClass {
    /// K group contains ground: the potential is a valid measurement.
    Referenced,
    /// K group without ground, at least 2 nodes: differences inside the group are valid, absolute values are not.
    Relative,
    /// Alone in its K group (leakage paths or nothing): undetermined, never shown as a measurement.
    Floating,
    /// The node belongs to no element.
    Unused,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SupplyMode {
    /// Current limited at i_lim.
    Cc,
    /// Constant voltage behind r_out.
    Cv,
    /// Back-driven above v: open, delivers no current.
    Rev,
    /// A bidirectional controlled driver sinks its negative current limit.
    CcNeg,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NodeValue {
    pub class: NodeClass,
    /// Volts relative to ground (Referenced) or to an artefact 0 V node (Relative, Floating).
    pub volts: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IslandStatus {
    /// Results published. `ill_conditioned`: max|u_kk| / min|u_kk| > [`ILL_CONDITIONED_RATIO`] in the last solve.
    Solved { ill_conditioned: bool },
    /// Singular matrix: nothing published, state not advanced.
    Singular,
    /// A non-finite value in the solution: nothing published, state not advanced.
    NonFinite,
    /// The mode loop did not converge: nothing published, state not advanced.
    ModeCycle,
    /// Loaded but neither previewed nor stepped since.
    Pending,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SolveError {
    pub island: usize,
    pub status: IslandStatus,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct SolverStats {
    pub islands: usize,
    /// Islands commuted by the last `load` (changed signature).
    pub changed_islands: usize,
    /// Sum over islands of the sub-steps of the last `step_frame`.
    pub substeps_last_frame: i64,
    /// Largest sub-step count of one island in the last `step_frame`.
    pub max_island_substeps: i64,
    /// Smallest island step of the last frame, in ns (FRAME_NS if every island is static).
    pub min_dt_ns: i64,
    /// LU factorizations performed by the last `step_frame` or `preview`.
    pub factorizations_last: usize,
    /// Largest number of solves of one mode loop since the last `load`.
    pub mode_solves_max: usize,
}

/// The solver: owns the per-island systems and the state of capacitors, supplies and gates.
#[derive(Clone, Debug, Default)]
pub struct Solver {
    net: Network,
    /// Public islands, then the internal island of ground-only elements if any.
    islands: Vec<Island>,
    n_public: usize,
    /// Terminal of at least one element; a node used only by a gate is Unused (it still sits in the gate's island).
    node_in_element: Vec<bool>,
    node_island: Vec<Option<usize>>,
    node_local: Vec<usize>,
    elem_island: Vec<usize>,
    elem_pos: Vec<usize>,
    elem_mode: Vec<Option<usize>>,
    gate_island: Vec<usize>,
    gate_mode: Vec<usize>,
    /// Stateful elements by ascending key: (island, position in its capacitors or mode vector).
    cap_index: BTreeMap<StateKey, (usize, usize)>,
    mode_index: BTreeMap<StateKey, (usize, usize)>,
    t_ns: i64,
    preview: bool,
    stats: SolverStats,
}

impl Solver {
    pub fn new() -> Self {
        Self::default()
    }

    /// Load a network (after an edit or a discrete-state change of the circuit, between two frames). Elements whose
    /// key survives keep their state; states of vanished keys are dropped. Islands with a changed signature are
    /// commuted. Results become [`IslandStatus::Pending`] until the next [`Solver::preview`] or
    /// [`Solver::step_frame`]. On error nothing changes.
    pub fn load(&mut self, net: &Network) -> Result<(), NetworkError> {
        validate(net)?;
        let part = island::partition(net);

        let mut old_caps: BTreeMap<StateKey, (f64, f64)> = BTreeMap::new();
        let mut old_modes: BTreeMap<StateKey, (bool, u8)> = BTreeMap::new();
        for isl in &self.islands {
            for (c, &s) in isl.def.caps.iter().zip(&isl.caps) {
                old_caps.insert(c.key, s);
            }
            for (m, &v) in isl.def.modes.iter().zip(&isl.modes) {
                old_modes.insert(m.key, (m.is_gate(), v));
            }
        }
        let mut old: Vec<Option<Island>> = std::mem::take(&mut self.islands).into_iter().map(Some).collect();
        let mut islands = Vec::with_capacity(part.defs.len());
        let mut changed = 0;
        for (i, def) in part.defs.into_iter().enumerate() {
            let caps = def.caps.iter().map(|c| old_caps.get(&c.key).copied().unwrap_or((0.0, 0.0))).collect();
            let modes = def
                .modes
                .iter()
                .map(|m| match old_modes.get(&m.key) {
                    Some(&(is_gate, v)) if is_gate == m.is_gate() => v,
                    _ => m.initial(),
                })
                .collect();
            // An island of unchanged signature keeps its commutation counter and its factorizations.
            let same = old.iter().position(|o| o.as_ref().map_or(false, |o| o.def.signature == def.signature));
            let island = match same.and_then(|j| old[j].take()) {
                Some(o) => Island::new(def, caps, modes, o.be_remaining, o.cache),
                None => {
                    if i < part.n_public {
                        changed += 1;
                    }
                    Island::new(def, caps, modes, BE_AFTER_COMMUTATION, Cache::default())
                }
            };
            islands.push(island);
        }

        self.cap_index.clear();
        self.mode_index.clear();
        for (ii, isl) in islands.iter().enumerate() {
            for (ci, c) in isl.def.caps.iter().enumerate() {
                self.cap_index.insert(c.key, (ii, ci));
            }
            for (mi, m) in isl.def.modes.iter().enumerate() {
                self.mode_index.insert(m.key, (ii, mi));
            }
        }
        self.islands = islands;
        self.n_public = part.n_public;
        self.node_in_element = part.node_in_element;
        self.node_island = part.node_island;
        self.node_local = part.node_local;
        self.elem_island = part.elem_island;
        self.elem_pos = part.elem_pos;
        self.elem_mode = part.elem_mode;
        self.gate_island = part.gate_island;
        self.gate_mode = part.gate_mode;
        self.net = net.clone();
        self.preview = false;
        self.stats.islands = part.n_public;
        self.stats.changed_islands = changed;
        self.stats.mode_solves_max = 0;
        Ok(())
    }

    /// RESET: t = 0, capacitors discharged (v = 0, i = 0), supplies in CV, gates off, every island commuted,
    /// results Pending.
    pub fn reset(&mut self) {
        self.t_ns = 0;
        self.preview = false;
        for isl in &mut self.islands {
            isl.reset();
        }
    }

    /// STOP preview: one backward Euler step of [`PREVIEW_H_S`] on every island from the stored state, with the
    /// mode loop; results published with [`Solver::is_preview`] = true, stored state unchanged. Returns the first
    /// island error after all islands were processed.
    pub fn preview(&mut self) -> Result<(), SolveError> {
        self.preview_impl(false)
    }

    /// Static point preview with two bounded LU refinements from electrical branch laws.
    /// Transient stepping and stored capacitor/mode state remain unchanged.
    pub fn preview_precise(&mut self)->Result<(),SolveError> {
        self.preview_impl(true)
    }

    fn preview_impl(&mut self,precise:bool)->Result<(),SolveError> {
        let mut cnt = Counters::default();
        let ground = self.net.ground;
        for isl in &mut self.islands {
            isl.preview(ground, &mut cnt, precise);
        }
        self.preview = true;
        self.stats.factorizations_last = cnt.factorizations;
        self.stats.mode_solves_max = self.stats.mode_solves_max.max(cnt.mode_solves_max);
        self.first_error()
    }

    /// Advance exactly [`FRAME_NS`] on every island (in island order). The error of one island does not stop the
    /// others; the first one is returned after all islands were processed.
    pub fn step_frame(&mut self) -> Result<(), SolveError> {
        let mut cnt = Counters::default();
        let ground = self.net.ground;
        let (mut sum, mut max, mut min_dt) = (0, 0, FRAME_NS);
        for (i, isl) in self.islands.iter_mut().enumerate() {
            let n_sub = isl.step_frame(ground, &mut cnt);
            if i < self.n_public {
                sum += n_sub;
                max = max.max(n_sub);
                min_dt = min_dt.min(FRAME_NS / n_sub);
            }
        }
        self.t_ns += FRAME_NS;
        self.preview = false;
        self.stats.substeps_last_frame = sum;
        self.stats.max_island_substeps = max;
        self.stats.min_dt_ns = min_dt;
        self.stats.factorizations_last = cnt.factorizations;
        self.stats.mode_solves_max = self.stats.mode_solves_max.max(cnt.mode_solves_max);
        self.first_error()
    }

    /// Simulated time in ns (multiple of FRAME_NS).
    pub fn time_ns(&self) -> i64 {
        self.t_ns
    }

    /// True if the published results come from [`Solver::preview`].
    pub fn is_preview(&self) -> bool {
        self.preview
    }

    /// Value and class of a node in the published results (class Unused also for a node of an island without
    /// published results: check [`Solver::island_status`]).
    pub fn node(&self, n: NodeId) -> NodeValue {
        if self.net.ground == Some(n) && n < self.net.nodes {
            return NodeValue { class: NodeClass::Referenced, volts: 0.0 };
        }
        match self.published_node(n) {
            Some((p, l)) => NodeValue { class: p.classes[l], volts: p.volts[l] },
            None => NodeValue { class: NodeClass::Unused, volts: 0.0 },
        }
    }

    /// K group of a node (classification graph), `None` if unused. Two nodes have a valid voltage difference iff
    /// they share a K group.
    pub fn k_group(&self, n: NodeId) -> Option<usize> {
        if self.net.ground == Some(n) && n < self.net.nodes {
            return Some(n);
        }
        self.published_node(n).map(|(p, l)| p.groups[l])
    }

    /// Island of a node, `None` for ground and unused nodes.
    pub fn island_of(&self, n: NodeId) -> Option<usize> {
        if !self.node_in_element.get(n).copied().unwrap_or(false) {
            return None;
        }
        self.node_island.get(n).copied().flatten()
    }

    pub fn island_status(&self, island: usize) -> IslandStatus {
        if island < self.n_public {
            self.islands[island].status
        } else {
            IslandStatus::Pending
        }
    }

    /// Current through element `index` in the published results, in its reference direction: Resistor and
    /// Capacitor from `a` to `b` through the element; VSource, Supply and Vcvs out of `pos` into the external
    /// circuit (0 while gated off). `None` if the island has no published results.
    pub fn element_current(&self, index: usize) -> Option<f64> {
        let island = *self.elem_island.get(index)?;
        let p = self.islands[island].published.as_ref()?;
        Some(p.currents[self.elem_pos[index]])
    }

    /// Mode of the Supply at `index` in the published results (None for other elements).
    pub fn supply_mode(&self, index: usize) -> Option<SupplyMode> {
        let mi = (*self.elem_mode.get(index)?)?;
        let p = self.islands[self.elem_island[index]].published.as_ref()?;
        Some(match p.modes[mi] {
            MODE_CC => SupplyMode::Cc,
            MODE_CV => SupplyMode::Cv,
            _ if matches!(self.net.elements[index], Element::LimitedVcvs { .. }) => SupplyMode::CcNeg,
            _ => SupplyMode::Rev,
        })
    }

    /// State of gate `g` in the published results.
    pub fn gate_on(&self, g: GateId) -> Option<bool> {
        let island = *self.gate_island.get(g)?;
        let p = self.islands[island].published.as_ref()?;
        Some(p.modes[self.gate_mode[g]] == GATE_ON)
    }

    /// Stored (v, i) of the capacitor with this key: v = V(a) − V(b), i entering by `a`.
    pub fn capacitor_state(&self, key: StateKey) -> Option<(f64, f64)> {
        let &(island, ci) = self.cap_index.get(&key)?;
        Some(self.islands[island].caps[ci])
    }

    /// Step of an island in the last frame, in ns.
    pub fn island_dt_ns(&self, island: usize) -> Option<i64> {
        if island < self.n_public {
            self.islands[island].dt_ns
        } else {
            None
        }
    }

    /// FNV-1a 64 over the `to_bits` of the stored state: t, then capacitors (v, i) by ascending key, supply modes
    /// and gate states by ascending key, commutation counters by island. Used by the determinism and replay tests.
    pub fn state_digest(&self) -> u64 {
        let mut h = Fnv1a::default();
        h.word(self.t_ns as u64);
        for &(island, ci) in self.cap_index.values() {
            let (v, i) = self.islands[island].caps[ci];
            h.word(v.to_bits());
            h.word(i.to_bits());
        }
        for &(island, mi) in self.mode_index.values() {
            h.word(u64::from(self.islands[island].modes[mi]));
        }
        for isl in &self.islands {
            h.word(u64::from(isl.be_remaining));
        }
        h.0
    }

    pub fn stats(&self) -> SolverStats {
        self.stats.clone()
    }

    /// Published results of the island holding node `n`, with its local index.
    fn published_node(&self, n: NodeId) -> Option<(&engine::Published, usize)> {
        let island = self.island_of(n)?;
        let p = self.islands[island].published.as_ref()?;
        Some((p, self.node_local[n]))
    }

    /// First error among the public islands, after a preview or a frame.
    fn first_error(&self) -> Result<(), SolveError> {
        match self.islands[..self.n_public].iter().enumerate().find_map(|(i, isl)| isl.error().map(|s| (i, s))) {
            Some((island, status)) => Err(SolveError { island, status }),
            None => Ok(()),
        }
    }
}

/// FNV-1a 64 over the little-endian bytes of each word.
struct Fnv1a(u64);

impl Default for Fnv1a {
    fn default() -> Self {
        Fnv1a(0xcbf2_9ce4_8422_2325)
    }
}

impl Fnv1a {
    fn word(&mut self, w: u64) {
        for byte in w.to_le_bytes() {
            self.0 ^= u64::from(byte);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
}

/// Validation of a network (contract §5.1): elements in order (nodes, values, gate), then gates; keys unique across
/// capacitors, supplies and gates.
fn validate(net: &Network) -> Result<(), NetworkError> {
    let n = net.nodes;
    if net.ground.map_or(false, |g| g >= n) {
        // No variant names the ground: reported as an element index past every element.
        return Err(NetworkError::NodeOutOfRange { element: usize::MAX });
    }
    let positive = |x: f64| x.is_finite() && x > 0.0;
    let mut keys = BTreeSet::new();
    for (i, e) in net.elements.iter().enumerate() {
        let (nodes, count) = island::element_nodes(e);
        if nodes[..count].iter().any(|&x| x >= n) {
            return Err(NetworkError::NodeOutOfRange { element: i });
        }
        let bad = |field: &'static str| Err(NetworkError::BadValue { element: i, field });
        let key = match *e {
            Element::Resistor { r, .. } => {
                if !positive(r) {
                    return bad("r");
                }
                None
            }
            Element::Capacitor { c, key, .. } => {
                if !positive(c) {
                    return bad("c");
                }
                Some(key)
            }
            Element::VSource { v, r_out, .. } => {
                if !v.is_finite() {
                    return bad("v");
                }
                if !positive(r_out) {
                    return bad("r_out");
                }
                None
            }
            Element::Supply { v, r_out, i_lim, key, .. } => {
                if !(v.is_finite() && v >= 0.0) {
                    return bad("v");
                }
                if !positive(r_out) {
                    return bad("r_out");
                }
                if i_lim.map_or(false, |l| !positive(l)) {
                    return bad("i_lim");
                }
                Some(key)
            }
            Element::Vcvs { gain, r_out, .. } => {
                if !gain.is_finite() {
                    return bad("gain");
                }
                if !positive(r_out) {
                    return bad("r_out");
                }
                None
            }
            Element::LimitedVcvs { gain, r_out, i_lim, key, .. } => {
                if !gain.is_finite() { return bad("gain"); }
                if !positive(r_out) { return bad("r_out"); }
                if !positive(i_lim) { return bad("i_lim"); }
                Some(key)
            }
        };
        if let Some(g) = island::element_gate(e) {
            if g >= net.gates.len() {
                return Err(NetworkError::BadGate { gate: g });
            }
        }
        if let Some(key) = key {
            if !keys.insert(key) {
                return Err(NetworkError::DuplicateKey { key });
            }
        }
    }
    for (gi, g) in net.gates.iter().enumerate() {
        let thresholds = g.on_above.is_finite() && g.off_below.is_finite() && g.on_above > g.off_below;
        if g.cp >= n || g.cn >= n || !thresholds {
            return Err(NetworkError::BadGate { gate: gi });
        }
        if !keys.insert(g.key) {
            return Err(NetworkError::DuplicateKey { key: g.key });
        }
    }
    Ok(())
}

/// Resistance between `a` and `b` of the circuit switched off, as an ohmmeter on an unpowered bench sees it (tests,
/// exploration mode, and the bench multimeter's Ω function, which reads only when every supply of the bench is
/// off): only the ungated resistors are kept (leakage included); sources, Vcvs, gated elements and capacitors
/// are removed; whole network including ground; +1 A injected at `a`, −1 A at `b`, R = V(a) − V(b).
/// `Some(0.0)` if a == b, `Some(f64::INFINITY)` if no resistive path, `None` if a node is out of range or the system
/// is singular.
pub fn unpowered_resistance(net: &Network, a: NodeId, b: NodeId) -> Option<f64> {
    let n = net.nodes;
    if a >= n || b >= n {
        return None;
    }
    if a == b {
        return Some(0.0);
    }
    let mut resistors = Vec::new();
    for e in &net.elements {
        if let Element::Resistor { a: p, b: q, r, gate: None, .. } = *e {
            if p >= n || q >= n || !(r.is_finite() && r > 0.0) {
                return None;
            }
            resistors.push((p, q, 1.0 / r));
        }
    }
    // Only the component of `a` matters (no controlled source here): solve it alone.
    let mut dsu = dsu::Dsu::new(n);
    for &(p, q, _) in &resistors {
        dsu.union(p, q);
    }
    let root = dsu.find(a);
    if dsu.find(b) != root {
        return Some(f64::INFINITY);
    }
    let mut local = vec![usize::MAX; n];
    let mut m = 0;
    for x in 0..n {
        if dsu.find(x) == root {
            local[x] = m;
            m += 1;
        }
    }
    let cond: Vec<(usize, usize, f64)> =
        resistors.iter().filter(|&&(p, _, _)| local[p] != usize::MAX).map(|&(p, q, g)| (local[p], local[q], g)).collect();
    match tau::port_resistance(m, &cond, &[], local[a], local[b]) {
        tau::Port::Finite(r) => Some(r),
        tau::Port::Open => Some(f64::INFINITY),
        tau::Port::Singular => None,
    }
}
