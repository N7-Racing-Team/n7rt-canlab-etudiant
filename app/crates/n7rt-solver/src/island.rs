//! Island partition (contract §5.2) and compilation of each island into local, index-based elements.
//!
//! Every element and gate belongs to exactly one island. Elements and gates whose nodes are all ground (a source
//! with both terminals on ground, for instance) form one extra, internal island without unknowns, placed after
//! the public ones: their modes are still resolved, with every potential at 0 V.

use crate::dsu::Dsu;
use crate::{Element, GateId, Network, NodeId, StateKey};

/// Local index standing for the ground node.
pub(crate) const GROUND: usize = usize::MAX;

pub(crate) const MODE_CC: u8 = 0;
pub(crate) const MODE_CV: u8 = 1;
pub(crate) const MODE_REV: u8 = 2;
pub(crate) const GATE_OFF: u8 = 0;
pub(crate) const GATE_ON: u8 = 1;

/// Element with local node indices; `gate` is the position of its gate in the island mode vector.
#[derive(Clone, Debug)]
pub(crate) enum LElem {
    R { a: usize, b: usize, g: f64, leakage: bool, gate: Option<usize> },
    C { a: usize, b: usize, cap: usize },
    V { pos: usize, neg: usize, v: f64, g: f64, vg: f64, gate: Option<usize> },
    S { pos: usize, neg: usize, v: f64, g: f64, vg: f64, r_out: f64, i_lim: Option<f64>, mode: usize, gate: Option<usize> },
    E { pos: usize, neg: usize, cp: usize, cn: usize, gain: f64, g: f64, gm: f64, gate: Option<usize> },
    B { pos: usize, neg: usize, cp: usize, cn: usize, gain: f64, g: f64, gm: f64, r_out: f64, i_lim: f64, mode: usize, gate: Option<usize> },
}

#[derive(Clone, Debug)]
pub(crate) struct CapDef {
    pub a: usize,
    pub b: usize,
    pub c: f64,
    pub key: StateKey,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum ModeKind {
    /// Index into `IslandDef::lelems`.
    Supply { elem: usize },
    Bipolar { elem: usize },
    Gate { cp: usize, cn: usize, on_above: f64, off_below: f64 },
}

/// A mode element (Supply or Gate); the island mode vector follows ascending keys.
#[derive(Clone, Debug)]
pub(crate) struct ModeDef {
    pub key: StateKey,
    pub kind: ModeKind,
}

impl ModeDef {
    pub(crate) fn is_gate(&self) -> bool {
        matches!(self.kind, ModeKind::Gate { .. })
    }

    /// State after RESET or for a new key: supplies in CV, gates off.
    pub(crate) fn initial(&self) -> u8 {
        if self.is_gate() {
            GATE_OFF
        } else {
            MODE_CV
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct IslandDef {
    /// Global node ids, ascending; local index = rank.
    pub nodes: Vec<NodeId>,
    /// Global element indices in network order, and their compiled form.
    pub elems: Vec<usize>,
    pub lelems: Vec<LElem>,
    /// Capacitors in network order, and the same sorted by ascending key.
    pub caps: Vec<CapDef>,
    pub caps_by_key: Vec<usize>,
    pub modes: Vec<ModeDef>,
    /// Global gate ids, ascending.
    pub gates: Vec<GateId>,
    pub signature: Vec<u64>,
}

pub(crate) struct Partition {
    pub defs: Vec<IslandDef>,
    pub n_public: usize,
    /// The node is a terminal of at least one element (gate control nodes alone do not count).
    pub node_in_element: Vec<bool>,
    pub node_island: Vec<Option<usize>>,
    pub node_local: Vec<usize>,
    pub elem_island: Vec<usize>,
    pub elem_pos: Vec<usize>,
    pub elem_mode: Vec<Option<usize>>,
    pub gate_island: Vec<usize>,
    pub gate_mode: Vec<usize>,
}

/// Terminals of an element: (pos|a, neg|b, cp, cn) and their count.
pub(crate) fn element_nodes(e: &Element) -> ([NodeId; 4], usize) {
    match *e {
        Element::Resistor { a, b, .. } | Element::Capacitor { a, b, .. } => ([a, b, 0, 0], 2),
        Element::VSource { pos, neg, .. } | Element::Supply { pos, neg, .. } => ([pos, neg, 0, 0], 2),
        Element::Vcvs { pos, neg, cp, cn, .. } | Element::LimitedVcvs { pos, neg, cp, cn, .. } => ([pos, neg, cp, cn], 4),
    }
}

pub(crate) fn element_gate(e: &Element) -> Option<GateId> {
    match *e {
        Element::Resistor { gate, .. }
        | Element::VSource { gate, .. }
        | Element::Supply { gate, .. }
        | Element::Vcvs { gate, .. }
        | Element::LimitedVcvs { gate, .. } => gate,
        Element::Capacitor { .. } => None,
    }
}

/// Splits a validated network into islands. Graph: vertices = nodes except ground; every element joins all its
/// non-ground terminals (branch, and the control inputs of a Vcvs); every gate joins its control nodes with the
/// nodes of the elements it gates. Islands are numbered by ascending smallest node.
pub(crate) fn partition(net: &Network) -> Partition {
    let n = net.nodes;
    let is_ground = |x: NodeId| net.ground == Some(x);
    let mut dsu = Dsu::new(n);
    let mut node_used = vec![false; n];
    let mut gated: Vec<Vec<usize>> = vec![Vec::new(); net.gates.len()];
    let join = |dsu: &mut Dsu, nodes: &mut dyn Iterator<Item = NodeId>| {
        let mut first: Option<NodeId> = None;
        for x in nodes {
            if is_ground(x) {
                continue;
            }
            match first {
                None => first = Some(x),
                Some(f) => {
                    dsu.union(f, x);
                }
            }
        }
        first
    };
    for (i, e) in net.elements.iter().enumerate() {
        let (ns, k) = element_nodes(e);
        for &x in &ns[..k] {
            node_used[x] = true;
        }
        join(&mut dsu, &mut ns[..k].iter().copied());
        if let Some(g) = element_gate(e) {
            gated[g].push(i);
        }
    }
    let node_in_element = node_used.clone();
    let gate_nodes = |gi: usize| -> Vec<NodeId> {
        let g = &net.gates[gi];
        let mut v = vec![g.cp, g.cn];
        for &ei in &gated[gi] {
            let (ns, k) = element_nodes(&net.elements[ei]);
            v.extend_from_slice(&ns[..k]);
        }
        v
    };
    for (gi, g) in net.gates.iter().enumerate() {
        node_used[g.cp] = true;
        node_used[g.cn] = true;
        join(&mut dsu, &mut gate_nodes(gi).into_iter());
    }

    let roots = dsu.roots();
    let mut root_island = vec![usize::MAX; n];
    let mut n_public = 0;
    for x in 0..n {
        if node_used[x] && !is_ground(x) && roots[x] == x {
            root_island[x] = n_public;
            n_public += 1;
        }
    }
    let hidden = n_public;
    let node_island: Vec<Option<usize>> =
        (0..n).map(|x| if node_used[x] && !is_ground(x) { Some(root_island[roots[x]]) } else { None }).collect();
    let first_island = |nodes: &mut dyn Iterator<Item = NodeId>| nodes.filter_map(|x| node_island[x]).next();
    let gate_island: Vec<usize> =
        (0..net.gates.len()).map(|gi| first_island(&mut gate_nodes(gi).into_iter()).unwrap_or(hidden)).collect();
    let elem_island: Vec<usize> = net
        .elements
        .iter()
        .map(|e| {
            let (ns, k) = element_nodes(e);
            first_island(&mut ns[..k].iter().copied())
                .or_else(|| element_gate(e).map(|g| gate_island[g]))
                .unwrap_or(hidden)
        })
        .collect();
    let has_hidden = elem_island.contains(&hidden) || gate_island.contains(&hidden);
    let n_islands = n_public + usize::from(has_hidden);

    let mut defs: Vec<IslandDef> = (0..n_islands).map(|_| IslandDef::default()).collect();
    let mut node_local = vec![usize::MAX; n];
    for x in 0..n {
        if let Some(i) = node_island[x] {
            node_local[x] = defs[i].nodes.len();
            defs[i].nodes.push(x);
        }
    }
    for (ei, &i) in elem_island.iter().enumerate() {
        defs[i].elems.push(ei);
    }
    for (gi, &i) in gate_island.iter().enumerate() {
        defs[i].gates.push(gi);
    }

    let mut elem_pos = vec![0; net.elements.len()];
    let mut elem_mode = vec![None; net.elements.len()];
    let mut gate_mode = vec![0; net.gates.len()];
    for def in &mut defs {
        compile(net, def, &node_local, &mut elem_pos, &mut elem_mode, &mut gate_mode);
    }
    Partition {
        defs,
        n_public,
        node_in_element,
        node_island,
        node_local,
        elem_island,
        elem_pos,
        elem_mode,
        gate_island,
        gate_mode,
    }
}

enum ModeSrc {
    Supply(usize),
    Gate(GateId),
}

fn compile(
    net: &Network,
    def: &mut IslandDef,
    node_local: &[usize],
    elem_pos: &mut [usize],
    elem_mode: &mut [Option<usize>],
    gate_mode: &mut [usize],
) {
    let loc = |x: NodeId| if net.ground == Some(x) { GROUND } else { node_local[x] };

    let mut modes: Vec<(StateKey, ModeSrc)> = Vec::new();
    for (p, &ei) in def.elems.iter().enumerate() {
        elem_pos[ei] = p;
        if let Element::Supply { key, .. } | Element::LimitedVcvs { key, .. } = net.elements[ei] {
            modes.push((key, ModeSrc::Supply(p)));
        }
    }
    for &gi in &def.gates {
        modes.push((net.gates[gi].key, ModeSrc::Gate(gi)));
    }
    modes.sort_by_key(|m| m.0);
    let mut supply_mode = vec![usize::MAX; def.elems.len()];
    for (mi, (key, src)) in modes.iter().enumerate() {
        let kind = match *src {
            ModeSrc::Supply(p) => {
                supply_mode[p] = mi;
                elem_mode[def.elems[p]] = Some(mi);
                if matches!(net.elements[def.elems[p]], Element::LimitedVcvs { .. }) { ModeKind::Bipolar { elem: p } }
                else { ModeKind::Supply { elem: p } }
            }
            ModeSrc::Gate(gi) => {
                gate_mode[gi] = mi;
                let g = &net.gates[gi];
                ModeKind::Gate { cp: loc(g.cp), cn: loc(g.cn), on_above: g.on_above, off_below: g.off_below }
            }
        };
        def.modes.push(ModeDef { key: *key, kind });
    }

    let rank = |x: NodeId| if net.ground == Some(x) { u64::MAX } else { node_local[x] as u64 };
    let gates = def.gates.clone();
    let gate_rank = |g: Option<GateId>| match g {
        None => u64::MAX,
        Some(g) => gates.binary_search(&g).map_or(u64::MAX - 1, |r| r as u64),
    };
    let mut sig = vec![def.nodes.len() as u64, def.elems.len() as u64, def.gates.len() as u64];
    for (p, &ei) in def.elems.iter().enumerate() {
        let gate_pos = |g: Option<GateId>| g.map(|g| gate_mode[g]);
        let le = match net.elements[ei] {
            Element::Resistor { a, b, r, leakage, gate } => {
                sig.extend_from_slice(&[1, rank(a), rank(b), r.to_bits(), u64::from(leakage), gate_rank(gate)]);
                LElem::R { a: loc(a), b: loc(b), g: 1.0 / r, leakage, gate: gate_pos(gate) }
            }
            Element::Capacitor { a, b, c, key } => {
                sig.extend_from_slice(&[2, rank(a), rank(b), c.to_bits(), key]);
                let cap = def.caps.len();
                def.caps.push(CapDef { a: loc(a), b: loc(b), c, key });
                LElem::C { a: loc(a), b: loc(b), cap }
            }
            Element::VSource { pos, neg, v, r_out, gate } => {
                sig.extend_from_slice(&[3, rank(pos), rank(neg), v.to_bits(), r_out.to_bits(), gate_rank(gate)]);
                let g = 1.0 / r_out;
                LElem::V { pos: loc(pos), neg: loc(neg), v, g, vg: v * g, gate: gate_pos(gate) }
            }
            Element::Supply { pos, neg, v, r_out, i_lim, key, gate } => {
                let (has, lim) = i_lim.map_or((0, 0), |l| (1, l.to_bits()));
                sig.extend_from_slice(&[4, rank(pos), rank(neg), v.to_bits(), r_out.to_bits(), has, lim, key]);
                sig.push(gate_rank(gate));
                let g = 1.0 / r_out;
                LElem::S {
                    pos: loc(pos),
                    neg: loc(neg),
                    v,
                    g,
                    vg: v * g,
                    r_out,
                    i_lim,
                    mode: supply_mode[p],
                    gate: gate_pos(gate),
                }
            }
            Element::Vcvs { pos, neg, cp, cn, gain, r_out, gate } => {
                sig.extend_from_slice(&[5, rank(pos), rank(neg), rank(cp), rank(cn), gain.to_bits(), r_out.to_bits()]);
                sig.push(gate_rank(gate));
                let g = 1.0 / r_out;
                LElem::E { pos: loc(pos), neg: loc(neg), cp: loc(cp), cn: loc(cn), gain, g, gm: gain * g, gate: gate_pos(gate) }
            }
            Element::LimitedVcvs { pos, neg, cp, cn, gain, r_out, i_lim, key, gate } => {
                sig.extend_from_slice(&[7, rank(pos), rank(neg), rank(cp), rank(cn), gain.to_bits(), r_out.to_bits(), i_lim.to_bits(), key, gate_rank(gate)]);
                let g = 1.0 / r_out;
                LElem::B { pos: loc(pos), neg: loc(neg), cp: loc(cp), cn: loc(cn), gain, g, gm: gain * g,
                    r_out, i_lim, mode: supply_mode[p], gate: gate_pos(gate) }
            }
        };
        def.lelems.push(le);
    }
    for &gi in &def.gates {
        let g = &net.gates[gi];
        sig.extend_from_slice(&[6, rank(g.cp), rank(g.cn), g.on_above.to_bits(), g.off_below.to_bits(), g.key]);
    }
    def.signature = sig;

    let mut by_key: Vec<usize> = (0..def.caps.len()).collect();
    by_key.sort_by_key(|&i| def.caps[i].key);
    def.caps_by_key = by_key;
}
