//! Results of one computation and the readings of contract §5.7 and §6.
//!
//! - [`Results`]: the numbers of the solver, extracted right after a frame or a preview (nothing ever reads the
//!   solver lazily, so a frame stays consistent while the core moves on).
//! - [`Capture`]: results + the lowering they belong to + the per-frame state of the integrated circuits and the
//!   ADC sample-and-hold. Shared by `Arc`.
//! - [`Probe`]: readings by node or by pin. A reading is never a fictitious number: an invalid state is a
//!   [`Reading`] variant, and the nets of a transceiver asked for a dominant level read Unavailable.
//! - [`Frame`]: the immutable [`super::PhysFrame`] content, with its diagnostics.

use std::collections::{BTreeMap,VecDeque};
use std::sync::Arc;

use n7rt_circuit::{kind_def, Circuit, CompId, Component, Kind, Netlist, PinRef, Role};
use n7rt_solver::{
    unpowered_resistance, Element, IslandStatus, NodeClass, NodeId, Solver, SolverStats, StateKey, SupplyMode, FRAME_NS,
};

use super::lower::{contact_closed, ecu_powered, gate_state, state_bool, state_num, ElemInfo, Lowered, ECU_APPS_CHANNELS};
use super::util::{finite, recast, unavailable};
use super::{diag, AdcSample, Diagnostic, Disclosure, LogicLevel, OpState, Reading, RsMode, SupplyOutput, TxdState};

/// MCP2551 TXD thresholds against VSS (VIL, VIH).
pub(crate) const TXD_VIL: f64 = 0.8;
pub(crate) const TXD_VIH: f64 = 2.0;
/// MCP2551 RXD output levels as fractions of VDD (D31 VOH ≥ 0.7·VDD, D32 VOL ≤ 0.2·VDD).
pub(crate) const RXD_VOL: f64 = 0.2;
pub(crate) const RXD_VOH: f64 = 0.7;
/// STM32 CMOS input thresholds as fractions of V3V3 (DS12117 table 156).
pub(crate) const MCU_VIL: f64 = 0.3;
pub(crate) const MCU_VIH: f64 = 0.7;
/// Rs bands as fractions of VDD − VSS (high speed below, standby above).
pub(crate) const RS_HIGH_SPEED_BELOW: f64 = 0.3;
pub(crate) const RS_STANDBY_ABOVE: f64 = 0.75;
/// Permanent-dominant timer: 1.25 ms minimum of the datasheet (AC parameter 14), counted in frames.
pub(crate) const TXD_TIMEOUT_NS: i64 = 1_250_000;

/// What the frame results were computed for.
#[derive(Debug)]
pub(crate) struct Ctx {
    /// The circuit lowered (labels refreshed by geometry-only edits).
    pub circuit: Circuit,
    pub netlist: Netlist,
    pub low: Arc<Lowered>,
    /// Raw document revision of `circuit`.
    pub rev: u64,
}

impl Ctx {
    pub fn empty() -> Ctx {
        Ctx { circuit: Circuit::new(""), netlist: Netlist::default(), low: Arc::new(Lowered::default()), rev: 0 }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct NodeRes {
    pub volts: f64,
    pub class: NodeClass,
    pub k: Option<usize>,
    pub island: Option<usize>,
}

/// The numbers of one computation.
#[derive(Clone, Debug)]
pub(crate) struct Results {
    pub time_ns: i64,
    pub preview: bool,
    pub nodes: Vec<NodeRes>,
    pub islands: Vec<IslandStatus>,
    pub currents: Vec<Option<f64>>,
    pub modes: Vec<Option<SupplyMode>>,
    pub gates: Vec<Option<bool>>,
    pub stats: SolverStats,
}

impl Results {
    pub fn empty() -> Results {
        Results {
            time_ns: 0,
            preview: true,
            nodes: Vec::new(),
            islands: Vec::new(),
            currents: Vec::new(),
            modes: Vec::new(),
            gates: Vec::new(),
            stats: SolverStats::default(),
        }
    }

    pub fn extract(solver: &Solver, low: &Lowered) -> Results {
        let net = &low.network;
        let stats = solver.stats();
        let nodes = (0..net.nodes)
            .map(|n| {
                let v = solver.node(n);
                NodeRes { volts: v.volts, class: v.class, k: solver.k_group(n), island: solver.island_of(n) }
            })
            .collect();
        let islands = (0..stats.islands).map(|i| solver.island_status(i)).collect();
        let currents = (0..net.elements.len()).map(|e| solver.element_current(e)).collect();
        let modes = net
            .elements
            .iter()
            .enumerate()
            .map(|(e, el)| if matches!(el, Element::Supply { .. } | Element::LimitedVcvs { .. }) { solver.supply_mode(e) } else { None })
            .collect();
        let gates = (0..net.gates.len()).map(|g| solver.gate_on(g)).collect();
        Results { time_ns: solver.time_ns(), preview: solver.is_preview(), nodes, islands, currents, modes, gates, stats }
    }
}

/// How the Rs pin is wired, when it is not a datasheet mode.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum RsIssue {
    Floating,
    NotModelled(&'static str),
}

/// State of a transceiver in one computation.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Xcvr {
    pub powered: bool,
    /// The power-on reset gate is known (its island has results).
    pub gate_known: bool,
    pub standby: bool,
    pub rs: RsMode,
    pub rs_issue: Option<RsIssue>,
    pub txd: TxdState,
    pub txd_level: Reading<LogicLevel>,
    /// Reason the readings of its CANH, CANL and RXD nets are withheld.
    pub block: Option<&'static str>,
}

/// TXD low time, counted in frames, and the permanent-dominant timeout latch.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TxdTimer {
    pub low_frames: u32,
    pub timed_out: bool,
}

pub(crate) type AdcHolds = BTreeMap<(CompId, &'static str), Reading<AdcSample>>;
pub(crate) type MissionHistory = BTreeMap<CompId,VecDeque<(u64,Arc<n7rt_vehicle::EcuInputs>)>>;

/// Everything a frame needs, shared by `Arc` between the core and the frames.
#[derive(Clone, Debug)]
pub(crate) struct Capture {
    pub ctx: Arc<Ctx>,
    pub res: Arc<Results>,
    pub xcvr: Arc<BTreeMap<CompId, Xcvr>>,
    pub blocked: Arc<BTreeMap<NodeId, &'static str>>,
    pub adc: Arc<AdcHolds>,
    pub mission_inputs: Arc<BTreeMap<CompId, n7rt_vehicle::EcuInputs>>,
    pub mission_history: Arc<MissionHistory>,
    /// Actual mechanical steering position, bounded to the current document's steering columns.
    pub mechanical: Arc<BTreeMap<CompId, f64>>,
    /// Last published mode of each supply, shown while its island has no result.
    pub last_modes: Arc<BTreeMap<StateKey, SupplyMode>>,
    /// The last lowering was refused by the solver (diagnostic internal_error).
    pub load_error: bool,
    /// Raw document revision these results stand for.
    pub rev: u64,
}

impl Capture {
    pub fn empty() -> Capture {
        Capture {
            ctx: Arc::new(Ctx::empty()),
            res: Arc::new(Results::empty()),
            xcvr: Arc::new(BTreeMap::new()),
            blocked: Arc::new(BTreeMap::new()),
            adc: Arc::new(BTreeMap::new()),
            mission_inputs: Arc::new(BTreeMap::new()),
            mission_history:Arc::new(BTreeMap::new()),
            mechanical:Arc::new(BTreeMap::new()),
            last_modes: Arc::new(BTreeMap::new()),
            load_error: false,
            rev: 0,
        }
    }
}

fn island_reason(s: IslandStatus) -> &'static str {
    match s {
        IslandStatus::Solved { .. } => "",
        IslandStatus::Singular | IslandStatus::NonFinite => "solver_singular",
        IslandStatus::ModeCycle => "mode_unresolved",
        IslandStatus::Pending => "solver_pending",
    }
}

/// Va − Vb, 0 when the difference is within a few ulps of the larger potential: below the resolution of the
/// solved potentials, it is a cancellation residue, not a voltage (the counterpart of the current resolution
/// of contract §5.3).
pub(crate) fn difference(va: f64, vb: f64) -> f64 {
    let d = va - vb;
    if d.abs() <= 8.0 * f64::EPSILON * va.abs().max(vb.abs()) {
        0.0
    } else {
        d
    }
}

/// Readings over one computation. `blocked` is empty for the raw evaluation of the integrated circuits.
pub(crate) struct Probe<'a> {
    pub ctx: &'a Ctx,
    pub res: &'a Results,
    pub blocked: &'a BTreeMap<NodeId, &'static str>,
}

impl<'a> Probe<'a> {
    pub fn ground(&self) -> Option<NodeId> {
        self.ctx.low.network.ground
    }

    pub fn island_issue(&self, n: NodeId) -> Option<&'static str> {
        let i = self.res.nodes.get(n)?.island?;
        match self.res.islands.get(i) {
            Some(IslandStatus::Solved { .. }) => None,
            Some(s) => Some(island_reason(*s)),
            None => Some("solver_pending"),
        }
    }

    /// Solver potential (an artefact for Relative and Floating nodes; only differences inside a K group or a
    /// conductance component are meaningful).
    pub fn volts(&self, n: NodeId) -> f64 {
        if Some(n) == self.ground() {
            0.0
        } else {
            self.res.nodes.get(n).map_or(0.0, |r| r.volts)
        }
    }

    fn k_of(&self, n: NodeId) -> Option<usize> {
        self.res.nodes.get(n).and_then(|r| r.k)
    }

    /// Alone in its K group or used by no element: an undetermined potential.
    pub fn is_floating(&self, n: NodeId) -> bool {
        Some(n) != self.ground() && self.res.nodes.get(n).map_or(true, |r| matches!(r.class, NodeClass::Floating | NodeClass::Unused))
    }

    /// V(node) against the ground net.
    pub fn node_reading(&self, n: NodeId) -> Reading<f64> {
        if Some(n) == self.ground() {
            return Reading::Valid { v: 0.0 };
        }
        let Some(r) = self.res.nodes.get(n) else { return unavailable("unknown_pin") };
        if let Some(why) = self.island_issue(n) {
            return unavailable(why);
        }
        match r.class {
            NodeClass::Referenced => finite(r.volts),
            NodeClass::Relative => Reading::NoReference,
            NodeClass::Floating | NodeClass::Unused => Reading::Floating,
        }
    }

    /// V(a) − V(b): Valid iff both nodes share a K group.
    pub fn node_between(&self, a: NodeId, b: NodeId) -> Reading<f64> {
        if let Some(why) = self.island_issue(a).or_else(|| self.island_issue(b)) {
            return unavailable(why);
        }
        if a == b {
            return Reading::Valid { v: 0.0 };
        }
        let ka = self.k_of(a);
        if ka.is_some() && ka == self.k_of(b) {
            return finite(difference(self.volts(a), self.volts(b)));
        }
        if self.is_floating(a) || self.is_floating(b) {
            Reading::Floating
        } else {
            Reading::NoReference
        }
    }

    pub fn comp(&self, id: CompId) -> Option<&'a Component> {
        self.ctx.circuit.components().get(&id)
    }

    /// Node of a pin of the computed circuit, or Unavailable(`unknown_pin`).
    pub fn resolve(&self, pin: &PinRef) -> Result<NodeId, Reading<f64>> {
        let known = self.comp(pin.comp).is_some_and(|c| kind_def(c.kind).pins.iter().any(|p| p.id == pin.pin));
        match self.ctx.low.node_of.get(pin) {
            Some(n) if known => Ok(*n),
            _ => Err(unavailable("unknown_pin")),
        }
    }

    pub fn pin_node(&self, comp: CompId, pin: &str) -> Option<NodeId> {
        self.ctx.low.node_of.get(&PinRef { comp, pin: pin.to_string() }).copied()
    }

    fn blocked(&self, n: NodeId) -> Option<Reading<f64>> {
        self.blocked.get(&n).map(|why| unavailable(why))
    }

    pub fn voltage_at(&self, pin: &PinRef) -> Reading<f64> {
        let n = match self.resolve(pin) {
            Ok(n) => n,
            Err(r) => return r,
        };
        self.blocked(n).unwrap_or_else(|| self.node_reading(n))
    }

    pub fn between(&self, a: &PinRef, b: &PinRef) -> Reading<f64> {
        let (na, nb) = match (self.resolve(a), self.resolve(b)) {
            (Ok(na), Ok(nb)) => (na, nb),
            (Err(r), _) | (_, Err(r)) => return r,
        };
        if let Some(r) = self.blocked(na).or_else(|| self.blocked(nb)) {
            return r;
        }
        self.node_between(na, nb)
    }

    pub fn pins_between(&self, comp: CompId, a: &str, b: &str) -> Reading<f64> {
        self.between(&PinRef { comp, pin: a.to_string() }, &PinRef { comp, pin: b.to_string() })
    }

    /// Current of an element in its solver reference direction.
    pub fn elem_current(&self, e: &ElemInfo) -> Result<f64, Reading<f64>> {
        match self.res.currents.get(e.index).copied().flatten() {
            Some(i) if i.is_finite() => Ok(i),
            Some(_) => Err(unavailable("solver_singular")),
            None => {
                let ground = self.ground();
                let node = e.terms.iter().map(|t| t.node).find(|n| Some(*n) != ground);
                if let Some(why) = node.and_then(|n| self.island_issue(n)) {
                    return Err(unavailable(why));
                }
                // An element with both ends on the ground node belongs to no island: no voltage, no current.
                let shorted = e.terms[0].node == e.terms[1].node;
                let passive = matches!(
                    self.ctx.low.network.elements.get(e.index),
                    Some(Element::Resistor { .. } | Element::Capacitor { .. })
                );
                if shorted && passive {
                    Ok(0.0)
                } else {
                    Err(unavailable("solver_singular"))
                }
            }
        }
    }

    /// Current entering the component through this pin (0 for a pin without element: ideal input, ground
    /// symbol, junction, source switched off).
    pub fn pin_current(&self, pin: &PinRef) -> Reading<f64> {
        let n = match self.resolve(pin) {
            Ok(n) => n,
            Err(r) => return r,
        };
        if let Some(r) = self.blocked(n) {
            return r;
        }
        let Some(low) = self.ctx.low.comps.get(&pin.comp) else { return Reading::Valid { v: 0.0 } };
        let mut sum = 0.0;
        for e in &low.elems {
            for t in &e.terms {
                if t.pin == Some(pin.pin.as_str()) {
                    match self.elem_current(e) {
                        Ok(i) => sum += t.sign * i,
                        Err(r) => return r,
                    }
                }
            }
        }
        finite(sum)
    }

    /// Power absorbed by the component, Σ V·I entering (negative when it delivers power).
    pub fn power(&self, comp: CompId) -> Reading<f64> {
        let Some(c) = self.comp(comp) else { return unavailable("unknown_pin") };
        if matches!(c.kind, Kind::EcuStm32 | Kind::CanTransceiver | Kind::CanBus | Kind::EcuApps | Kind::Mlx90377
            | Kind::EcuWsf | Kind::EcuSas | Kind::EcuBps | Kind::Tle5014 | Kind::Pss260) {
            return unavailable("ic_model");
        }
        for p in kind_def(c.kind).pins {
            if let Some(r) = self.pin_node(comp, p.id).and_then(|n| self.blocked(n)) {
                return r;
            }
        }
        let Some(low) = self.ctx.low.comps.get(&comp) else { return Reading::Valid { v: 0.0 } };
        let mut p = 0.0;
        for e in &low.elems {
            match self.elem_current(e) {
                // Two terminals with opposite signs: the reference offset of a relative island cancels.
                Ok(i) => p += i * e.terms[0].sign * (self.volts(e.terms[0].node) - self.volts(e.terms[1].node)),
                Err(r) => return r,
            }
        }
        finite(p)
    }

    /// Logic level against the component's thresholds and local ground.
    pub fn logic(&self, pin: &PinRef) -> Reading<LogicLevel> {
        let n = match self.resolve(pin) {
            Ok(n) => n,
            Err(r) => return recast(&r),
        };
        let Some(c) = self.comp(pin.comp) else { return unavailable("unknown_pin") };
        let id = c.id;
        match (c.kind, pin.pin.as_str()) {
            (Kind::EcuStm32, "can_rx" | "can_tx") => {
                if !ecu_powered(&self.ctx.circuit, c) {
                    return unavailable("ecu_unpowered");
                }
                if let Some(r) = self.blocked(n) {
                    return recast(&r);
                }
                match self.pins_between(id, "v3v3", "gnd") {
                    Reading::Valid { v: vdd } => classify(&self.pins_between(id, &pin.pin, "gnd"), MCU_VIL * vdd, MCU_VIH * vdd),
                    other => recast(&other),
                }
            }
            (Kind::CanTransceiver, "txd") => classify(&self.pins_between(id, "txd", "vss"), TXD_VIL, TXD_VIH),
            (Kind::CanTransceiver, "rxd") => {
                if let Some(r) = self.blocked(n) {
                    return recast(&r);
                }
                match self.pins_between(id, "vdd", "vss") {
                    Reading::Valid { v: u } => classify(&self.pins_between(id, "rxd", "vss"), RXD_VOL * u, RXD_VOH * u),
                    other => recast(&other),
                }
            }
            _ => unavailable("not_logic_pin"),
        }
    }

    /// Reset gate of an ECU APPS, or start-up gate of die `die` (0-based) of a position sensor, in this
    /// computation: None when undecided (island without result) or not such a part.
    pub fn part_gate(&self, comp: CompId, die: Option<usize>) -> Option<bool> {
        let low = self.ctx.low.comps.get(&comp)?;
        let which = match die {
            None => low.gate,
            Some(d) => low.dies.get(d).copied(),
        };
        gate_state(&self.res.gates, which)
    }

    /// V(internal node `name` of `comp`) − V(pin `against` of `comp`), e.g. an ADC midpoint of an ECU APPS.
    pub fn internal_between(&self, comp: CompId, name: &str, against: &str) -> Reading<f64> {
        let node = self.ctx.low.comps.get(&comp).and_then(|l| l.node(name));
        match (node, self.pin_node(comp, against)) {
            (Some(n), Some(g)) => self.node_between(n, g),
            _ => unavailable("unknown_pin"),
        }
    }

    /// Mode of a supply element: the published one, else the last published, else CV (reset state).
    pub fn supply_mode(&self, e: &ElemInfo, last: &BTreeMap<StateKey, SupplyMode>) -> SupplyMode {
        if let Some(Some(m)) = self.res.modes.get(e.index) {
            return *m;
        }
        match self.ctx.low.network.elements.get(e.index) {
            Some(Element::Supply { key, .. } | Element::LimitedVcvs { key, .. }) => last.get(key).copied().unwrap_or(SupplyMode::Cv),
            _ => SupplyMode::Cv,
        }
    }
}

/// Low at or below `vil`, high at or above `vih`, undefined between.
pub(crate) fn classify(v: &Reading<f64>, vil: f64, vih: f64) -> Reading<LogicLevel> {
    match v {
        Reading::Valid { v } if *v <= vil => Reading::Valid { v: LogicLevel::Low },
        Reading::Valid { v } if *v >= vih => Reading::Valid { v: LogicLevel::High },
        Reading::Valid { .. } => Reading::Undefined,
        other => recast(other),
    }
}

/// Rs mode (contract §2.1): same net as VSS → high speed; a net driven by a source or a logic output → bands
/// of VDD − VSS; anything else (a slope resistor included) is undefined, never high speed.
fn rs_mode(p: &Probe, c: &Component, u: &Reading<f64>) -> (RsMode, Option<RsIssue>) {
    let id = c.id;
    let (Some(rs), Some(vss)) = (p.pin_node(id, "rs"), p.pin_node(id, "vss")) else { return (RsMode::Undefined, None) };
    if rs == vss {
        return (RsMode::HighSpeed, None);
    }
    let rs_pin = PinRef { comp: id, pin: "rs".to_string() };
    let driven = p.ctx.netlist.net_of(&rs_pin).is_some_and(|net| {
        net.pins.iter().any(|q| {
            p.comp(q.comp)
                .and_then(|qc| kind_def(qc.kind).pins.iter().find(|d| d.id == q.pin))
                .is_some_and(|d| matches!(d.role, Role::Source | Role::LogicOut))
        })
    });
    let v = p.node_between(rs, vss);
    match (&v, driven) {
        (Reading::Floating | Reading::NoReference, _) => (RsMode::Floating, Some(RsIssue::Floating)),
        (Reading::Valid { v }, true) => match u {
            Reading::Valid { v: u } if *v < RS_HIGH_SPEED_BELOW * u => (RsMode::HighSpeed, None),
            Reading::Valid { v: u } if *v > RS_STANDBY_ABOVE * u => (RsMode::Standby, None),
            Reading::Valid { .. } => (RsMode::Undefined, Some(RsIssue::NotModelled("tension entre 0.3·VDD et 0.75·VDD"))),
            _ => (RsMode::Undefined, None),
        },
        (Reading::Valid { .. }, false) => {
            (RsMode::Undefined, Some(RsIssue::NotModelled("résistance de pente ou broche non pilotée, courant de Rs non modélisé")))
        }
        _ => (RsMode::Undefined, None),
    }
}

/// Evaluates a transceiver on raw readings; in a frame (`advance`) the permanent-dominant timer counts one more
/// frame, in a preview it is read only.
pub(crate) fn eval_xcvr(p: &Probe, c: &Component, timer: &mut TxdTimer, advance: bool) -> Xcvr {
    let gate = p.ctx.low.comps.get(&c.id).and_then(|l| l.gate).and_then(|g| p.res.gates.get(g).copied().flatten());
    let powered = gate == Some(true);
    let u = p.pins_between(c.id, "vdd", "vss");
    let (rs, rs_issue) = rs_mode(p, c, &u);
    let standby = rs == RsMode::Standby;
    let txd_level = classify(&p.pins_between(c.id, "txd", "vss"), TXD_VIL, TXD_VIH);
    let mut x = Xcvr { powered, gate_known: gate.is_some(), standby, rs, rs_issue, txd: TxdState::Recessive, txd_level, block: None };
    if !powered || standby {
        if advance {
            *timer = TxdTimer::default();
        }
        return x;
    }
    match x.txd_level {
        Reading::Valid { v: LogicLevel::High } => {
            // Rising edge (or still high): the timer and its latch are cleared.
            if advance {
                *timer = TxdTimer::default();
            }
        }
        Reading::Valid { v: LogicLevel::Low } => {
            if advance {
                timer.low_frames = timer.low_frames.saturating_add(1);
                if i64::from(timer.low_frames) * FRAME_NS > TXD_TIMEOUT_NS {
                    timer.timed_out = true;
                }
            }
            if timer.timed_out {
                x.txd = TxdState::DominantTimeout;
            } else {
                x.txd = TxdState::DominantRequested;
                x.block = Some("dominant_non_modelise_v1");
            }
        }
        Reading::Undefined => {
            if advance {
                timer.low_frames = 0;
            }
            if timer.timed_out {
                x.txd = TxdState::DominantTimeout;
            } else {
                x.txd = TxdState::DominantRequested;
                x.block = Some("txd_undefined");
            }
        }
        // TXD has a pull-up to VDD: unreadable only when its island has no result, reported elsewhere.
        _ => {}
    }
    x
}

/// Nets whose readings are withheld: CANH, CANL and RXD of a transceiver asked for a dominant level, and the
/// RXD of every powered transceiver on a withheld bus (it would follow the dominant level).
pub(crate) fn blocked_nodes(p: &Probe, xcvr: &BTreeMap<CompId, Xcvr>) -> BTreeMap<NodeId, &'static str> {
    let mut map = BTreeMap::new();
    for (id, x) in xcvr {
        if let Some(why) = x.block {
            for pin in ["canh", "canl", "rxd"] {
                if let Some(n) = p.pin_node(*id, pin) {
                    map.entry(n).or_insert(why);
                }
            }
        }
    }
    let followers: Vec<(CompId, &'static str)> = xcvr
        .iter()
        .filter(|(_, x)| x.powered)
        .filter_map(|(id, _)| {
            ["canh", "canl"].iter().find_map(|pin| p.pin_node(*id, pin).and_then(|n| map.get(&n).copied())).map(|why| (*id, why))
        })
        .collect();
    for (id, why) in followers {
        if let Some(n) = p.pin_node(id, "rxd") {
            map.entry(n).or_insert(why);
        }
    }
    map
}

/// Sample-and-hold of an ADC channel: code = floor(V·2^bits / vref), Valid iff 0 ≤ V < vref, else Saturated
/// with the code clamped to [0, 2^bits − 1].
pub(crate) fn sample(v: &Reading<f64>, vref: f64, bits: u32, t_ns: i64) -> Reading<AdcSample> {
    let bits = bits.clamp(1, 16);
    match v {
        Reading::Valid { v } => {
            let v = *v;
            let full = (1u32 << bits) as f64;
            let max = (1u32 << bits) - 1;
            if vref > 0.0 && v >= 0.0 && v < vref {
                let q = (v * full / vref).floor();
                let code = if q >= full { max } else { q as u32 };
                Reading::Valid { v: AdcSample { volts: v, code, bits, vref, sampled_at_ns: t_ns } }
            } else {
                // Below 0 V (or a reference that is not positive, e.g. a collapsed 3V3 rail): 0; above Vref: full scale.
                let code = if v <= 0.0 { 0 } else { max };
                Reading::Saturated { v: AdcSample { volts: v, code, bits, vref, sampled_at_ns: t_ns } }
            }
        }
        other => recast(other),
    }
}

/// Frames between two samples for a rate dividing 1000 Hz.
pub(crate) fn sample_period(rate_hz: i64) -> u64 {
    if rate_hz <= 0 {
        1
    } else {
        (1000 / rate_hz).max(1) as u64
    }
}

// ------------------------------------------------------------------------------------------------------ frame

/// Content of [`super::PhysFrame`].
#[derive(Debug)]
pub(crate) struct Frame {
    /// Reported revision (offset applied).
    pub rev: u64,
    pub cap: Arc<Capture>,
    pub disclosure: Disclosure,
    pub diagnostics: Vec<Diagnostic>,
}

impl Frame {
    pub fn build(cap: Arc<Capture>, rev: u64, disclosure: Disclosure, notices: &[Diagnostic]) -> Frame {
        let mut f = Frame { rev, cap, disclosure, diagnostics: Vec::new() };
        let diagnostics = diag::compute(&f.probe(), &f.cap, notices);
        f.diagnostics = diagnostics;
        f
    }

    pub fn probe(&self) -> Probe<'_> {
        Probe { ctx: &self.cap.ctx, res: &self.cap.res, blocked: &self.cap.blocked }
    }

    pub fn adc(&self, comp: CompId, channel: &str) -> Reading<AdcSample> {
        let Some(c) = self.cap.ctx.circuit.components().get(&comp) else { return unavailable("unknown_pin") };
        let ch: &'static str = match (c.kind, channel) {
            (Kind::AdcInput, "in") => "in",
            (Kind::EcuStm32, "a0") => "a0",
            (Kind::EcuStm32, "a1") => "a1",
            (Kind::EcuApps, ch) => match ECU_APPS_CHANNELS.iter().find(|n| **n == ch) {
                Some(n) => n,
                None => return unavailable("unknown_pin"),
            },
            (Kind::EcuBps, "sig_f") => "sig_f",
            (Kind::EcuBps, "sig_r") => "sig_r",
            (Kind::EcuBps | Kind::EcuSas, "sup5v") => "sup5v",
            (Kind::EcuSas, "s5v") => "sup5v",
            _ => return unavailable("unknown_pin"),
        };
        if c.kind == Kind::EcuStm32 && !ecu_powered(&self.cap.ctx.circuit, c) {
            return unavailable("ecu_unpowered");
        }
        self.cap.adc.get(&(comp, ch)).cloned().unwrap_or_else(|| unavailable("not_sampled_yet"))
    }

    pub fn op_state(&self, comp: CompId) -> Option<OpState> {
        let circuit = &self.cap.ctx.circuit;
        let c = circuit.components().get(&comp)?;
        Some(match c.kind {
            Kind::DcSupply => {
                let output = if !state_bool(circuit, c, "on") {
                    SupplyOutput::Off
                } else {
                    match self.cap.ctx.low.comps.get(&comp).and_then(|l| l.elem("supply")) {
                        Some(e) => match self.probe().supply_mode(e, &self.cap.last_modes) {
                            SupplyMode::Cc => SupplyOutput::Cc,
                            SupplyMode::Cv => SupplyOutput::Cv,
                            SupplyMode::Rev => SupplyOutput::Rev,
                            SupplyMode::CcNeg => SupplyOutput::Cc,
                        },
                        None => SupplyOutput::Cv,
                    }
                };
                OpState::Supply { output }
            }
            Kind::Switch | Kind::PushButton => OpState::Contact { closed: contact_closed(circuit, c) },
            Kind::PressureSwitch => OpState::Contact {closed:self.cap.ctx.low.comps.get(&comp).and_then(|l|l.elem("contact"))
                .and_then(|e|self.cap.ctx.low.network.elements.get(e.index)).is_some_and(|e|matches!(e,Element::Resistor {leakage:false,..}))},
            Kind::Potentiometer => OpState::Pot { position: state_num(circuit, c, "position") },
            Kind::EcuStm32 => OpState::Ecu { powered: ecu_powered(circuit, c) },
            // Out of reset or in reset: all a black box shows.
            Kind::EcuApps | Kind::EcuWsf | Kind::EcuSas | Kind::EcuBps => OpState::Ecu { powered: self.probe().part_gate(comp, None) == Some(true) },
            // The pedal travel only: the state of the dies is not visible on the part.
            Kind::Mlx90377 => OpState::Pot { position: state_num(circuit, c, "position") },
            Kind::CanTransceiver => match self.cap.xcvr.get(&comp) {
                Some(x) => OpState::Transceiver { powered: x.powered, rs: x.rs, txd: x.txd },
                None => OpState::Transceiver { powered: false, rs: RsMode::Undefined, txd: TxdState::Recessive },
            },
            _ => OpState::Passive,
        })
    }

    pub fn unpowered_resistance(&self, a: &PinRef, b: &PinRef) -> Reading<f64> {
        if self.disclosure == Disclosure::Mission {
            return unavailable("hidden_mission");
        }
        let p = self.probe();
        let (na, nb) = match (p.resolve(a), p.resolve(b)) {
            (Ok(na), Ok(nb)) => (na, nb),
            (Err(r), _) | (_, Err(r)) => return r,
        };
        match unpowered_resistance(&self.cap.ctx.low.network, na, nb) {
            Some(r) if r == f64::INFINITY => Reading::Open,
            Some(r) => finite(r),
            None => unavailable("solver_singular"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::difference;

    fn ulps(v: f64, n: i64) -> f64 {
        f64::from_bits((v.to_bits() as i64 + n) as u64)
    }

    #[test]
    fn a_difference_within_the_resolution_of_the_potentials_is_zero() {
        // The recessive CAN bus of tp1: CANH and CANL one ulp apart after the solve.
        let v = 2.4997483;
        // 8ε·2.4997483 is just under 10 ulps of 2.4997483 (ulp = 2ε on [2, 4)).
        for n in [-9, -1, 1, 9] {
            assert_eq!(difference(v, ulps(v, n)), 0.0, "{n} ulp(s)");
            assert_eq!(difference(-v, -ulps(v, n)), 0.0, "{n} ulp(s), negative");
        }
        assert_eq!(difference(v, v), 0.0);
        assert_eq!(difference(0.0, 0.0), 0.0);
    }

    #[test]
    fn a_resolved_difference_is_kept_exactly() {
        let v = 2.4997483;
        assert_eq!(difference(v, ulps(v, 12)), v - ulps(v, 12));
        assert_eq!(difference(v, v - 1e-9), v - (v - 1e-9));
        // Against the ground (0 V) every non-zero potential is resolved, however small.
        assert_eq!(difference(1e-300, 0.0), 1e-300);
        assert_eq!(difference(0.0, 4.4e-16), -4.4e-16);
        assert!(difference(f64::NAN, 1.0).is_nan());
    }
}
