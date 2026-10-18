//! Per-island numerics (contract §5.3 to §5.7): connectivity of a mode vector, cached factorizations, the mode
//! loop of one solve point, frame stepping, preview and publication of the accepted solution.

use crate::dsu::Dsu;
use crate::island::{IslandDef, LElem, ModeKind, GATE_OFF, GATE_ON, GROUND, MODE_CC, MODE_CV, MODE_REV};
use crate::lu::{Lu, LuFail};
use crate::tau::{self, TauInfo};
use crate::{IslandStatus, NodeClass, NodeId, BE_AFTER_COMMUTATION, FRAME_NS, MODE_BAND_V, PREVIEW_H_S};

/// Factorizations and connectivities kept per island (keyed by step, methods and modes).
const CACHE_LEN: usize = 4;

/// Index of a local node in a union-find of n + 1 slots, ground being slot n.
pub(crate) fn slot(x: usize, n: usize) -> usize {
    if x == GROUND {
        n
    } else {
        x
    }
}

/// An element is present unless its gate is off.
pub(crate) fn active(gate: Option<usize>, modes: &[u8]) -> bool {
    gate.map_or(true, |m| modes[m] == GATE_ON)
}

pub(crate) fn volt(x: &[f64], i: usize) -> f64 {
    if i == GROUND {
        0.0
    } else {
        x[i]
    }
}

pub(crate) fn stamp(y: &mut [f64], n: usize, r: usize, c: usize, v: f64) {
    if r != GROUND && c != GROUND {
        y[r * n + c] += v;
    }
}

/// Y[a,a] += g, Y[b,b] += g, Y[a,b] −= g, Y[b,a] −= g (ground rows and columns dropped).
pub(crate) fn stamp_g(y: &mut [f64], n: usize, a: usize, b: usize, g: f64) {
    stamp(y, n, a, a, g);
    stamp(y, n, b, b, g);
    stamp(y, n, a, b, -g);
    stamp(y, n, b, a, -g);
}

pub(crate) fn inject(rhs: &mut [f64], r: usize, v: f64) {
    if r != GROUND {
        rhs[r] += v;
    }
}

/// Replaces the rows of `fixed` by v = 0.
pub(crate) fn fix_rows(y: &mut [f64], n: usize, fixed: &[usize]) {
    for &f in fixed {
        y[f * n..(f + 1) * n].fill(0.0);
        y[f * n + f] = 1.0;
    }
}

#[derive(Debug, Default)]
pub(crate) struct Counters {
    pub factorizations: usize,
    pub mode_solves_max: usize,
}

/// Connectivity under one mode vector. Roots per local node, ground slot at index n.
#[derive(Clone, Debug)]
pub(crate) struct Topo {
    modes: Vec<u8>,
    /// Conductance graph: resistors (leakage included), capacitor companions, VSource, Supply in CV, Vcvs outputs.
    pub cond: Vec<usize>,
    /// Graph K: the same without leakage.
    pub k: Vec<usize>,
    /// Smallest node of every conductance component without ground: its row becomes v = 0.
    pub fixed: Vec<usize>,
}

fn build_topo(def: &IslandDef, modes: &[u8]) -> Topo {
    let n = def.nodes.len();
    let mut cond = Dsu::new(n + 1);
    let mut k = Dsu::new(n + 1);
    for e in &def.lelems {
        let (a, b, in_k) = match *e {
            LElem::R { a, b, leakage, gate, .. } => {
                if !active(gate, modes) {
                    continue;
                }
                (a, b, !leakage)
            }
            LElem::C { a, b, .. } => (a, b, true),
            LElem::V { pos, neg, gate, .. } | LElem::E { pos, neg, gate, .. } => {
                if !active(gate, modes) {
                    continue;
                }
                (pos, neg, true)
            }
            LElem::S { pos, neg, mode, gate, .. } => {
                if !active(gate, modes) || modes[mode] != MODE_CV {
                    continue;
                }
                (pos, neg, true)
            }
            LElem::B { pos, neg, mode, gate, .. } => {
                if !active(gate, modes) || modes[mode] != MODE_CV { continue; }
                (pos, neg, true)
            }
        };
        cond.union(slot(a, n), slot(b, n));
        if in_k {
            k.union(slot(a, n), slot(b, n));
        }
    }
    let cond = cond.roots();
    let k = k.roots();
    let fixed = (0..n).filter(|&i| cond[i] == i && cond[n] != i).collect();
    Topo { modes: modes.to_vec(), cond, k, fixed }
}

/// Factored nodal matrix for (h, method per capacitor, modes).
#[derive(Clone, Debug)]
pub(crate) struct System {
    h_bits: u64,
    tr: Vec<bool>,
    modes: Vec<u8>,
    fixed: Vec<usize>,
    /// Companion conductance of each capacitor: C/h (BE) or 2C/h (trapezoid).
    cap_g: Vec<f64>,
    lu: Result<Lu, LuFail>,
}

fn build_system(def: &IslandDef, fixed: &[usize], h: f64, tr: &[bool], modes: &[u8]) -> System {
    let n = def.nodes.len();
    let cap_g: Vec<f64> = def.caps.iter().zip(tr).map(|(c, &t)| if t { 2.0 * c.c / h } else { c.c / h }).collect();
    let mut y = vec![0.0; n * n];
    for e in &def.lelems {
        match *e {
            LElem::R { a, b, g, gate, .. } => {
                if active(gate, modes) {
                    stamp_g(&mut y, n, a, b, g);
                }
            }
            LElem::C { a, b, cap } => stamp_g(&mut y, n, a, b, cap_g[cap]),
            LElem::V { pos, neg, g, gate, .. } => {
                if active(gate, modes) {
                    stamp_g(&mut y, n, pos, neg, g);
                }
            }
            LElem::S { pos, neg, g, mode, gate, .. } => {
                if active(gate, modes) && modes[mode] == MODE_CV {
                    stamp_g(&mut y, n, pos, neg, g);
                }
            }
            LElem::E { pos, neg, cp, cn, g, gm, gate, .. } => {
                if active(gate, modes) {
                    stamp_g(&mut y, n, pos, neg, g);
                    stamp(&mut y, n, pos, cp, -gm);
                    stamp(&mut y, n, pos, cn, gm);
                    stamp(&mut y, n, neg, cp, gm);
                    stamp(&mut y, n, neg, cn, -gm);
                }
            }
            LElem::B { pos, neg, cp, cn, g, gm, mode, gate, .. } => {
                if active(gate, modes) && modes[mode] == MODE_CV {
                    stamp_g(&mut y, n, pos, neg, g);
                    stamp(&mut y, n, pos, cp, -gm);
                    stamp(&mut y, n, pos, cn, gm);
                    stamp(&mut y, n, neg, cp, gm);
                    stamp(&mut y, n, neg, cn, -gm);
                }
            }
        }
    }
    fix_rows(&mut y, n, fixed);
    let lu = Lu::factor(y, n);
    System { h_bits: h.to_bits(), tr: tr.to_vec(), modes: modes.to_vec(), fixed: fixed.to_vec(), cap_g, lu }
}

/// Companion current source J of capacitor `ci`: G·v (BE) or G·v + i (trapezoid).
fn cap_j(sys: &System, ci: usize, (v, i): (f64, f64)) -> f64 {
    let g = sys.cap_g[ci];
    if sys.tr[ci] {
        g * v + i
    } else {
        g * v
    }
}

fn build_rhs(def: &IslandDef, sys: &System, caps: &[(f64, f64)], modes: &[u8], rhs: &mut [f64]) {
    rhs.fill(0.0);
    for e in &def.lelems {
        match *e {
            LElem::C { a, b, cap } => {
                let j = cap_j(sys, cap, caps[cap]);
                inject(rhs, a, j);
                inject(rhs, b, -j);
            }
            LElem::V { pos, neg, vg, gate, .. } => {
                if active(gate, modes) {
                    inject(rhs, pos, vg);
                    inject(rhs, neg, -vg);
                }
            }
            LElem::S { pos, neg, vg, i_lim, mode, gate, .. } => {
                if active(gate, modes) {
                    match (modes[mode], i_lim) {
                        (MODE_CV, _) => {
                            inject(rhs, pos, vg);
                            inject(rhs, neg, -vg);
                        }
                        (MODE_CC, Some(lim)) => {
                            inject(rhs, pos, lim);
                            inject(rhs, neg, -lim);
                        }
                        _ => {}
                    }
                }
            }
            LElem::R { .. } | LElem::E { .. } => {}
            LElem::B { pos, neg, i_lim, mode, gate, .. } => {
                if active(gate, modes) && modes[mode] != MODE_CV {
                    let i = if modes[mode] == MODE_CC { i_lim } else { -i_lim };
                    inject(rhs, pos, i);
                    inject(rhs, neg, -i);
                }
            }
        }
    }
    for &f in &sys.fixed {
        rhs[f] = 0.0;
    }
}

/// b-Yx from branch laws, without subtracting large nodal matrix coefficients.
/// Kahan accumulation keeps small sense currents next to low-resistance connections.
fn branch_residual(def:&IslandDef,sys:&System,caps:&[(f64,f64)],modes:&[u8],x:&[f64])->Vec<f64> {
    let mut residual=vec![0.0;x.len()];let mut correction=vec![0.0;x.len()];
    let mut add=|node:usize,value:f64| {
        if node==GROUND {return;}
        let y=value-correction[node];let next=residual[node]+y;
        correction[node]=(next-residual[node])-y;residual[node]=next;
    };
    for e in &def.lelems {
        let (pos,neg,current)=match *e {
            LElem::R{a,b,g,gate,..} if active(gate,modes)=>(a,b,g*(volt(x,a)-volt(x,b))),
            LElem::C{a,b,cap}=>(a,b,sys.cap_g[cap]*(volt(x,a)-volt(x,b))-cap_j(sys,cap,caps[cap])),
            LElem::V{pos,neg,g,vg,gate,..} if active(gate,modes)=>
                (pos,neg,g*(volt(x,pos)-volt(x,neg))-vg),
            LElem::S{pos,neg,g,vg,i_lim,mode,gate,..} if active(gate,modes)=>
                (pos,neg,match (modes[mode],i_lim) {
                    (MODE_CV,_)=>g*(volt(x,pos)-volt(x,neg))-vg,
                    (MODE_CC,Some(lim))=>-lim,_=>0.0,
                }),
            LElem::E{pos,neg,cp,cn,g,gm,gate,..} if active(gate,modes)=>
                (pos,neg,g*(volt(x,pos)-volt(x,neg))-gm*(volt(x,cp)-volt(x,cn))),
            LElem::B{pos,neg,cp,cn,g,gm,i_lim,mode,gate,..} if active(gate,modes)=>
                (pos,neg,match modes[mode] {
                    MODE_CV=>g*(volt(x,pos)-volt(x,neg))-gm*(volt(x,cp)-volt(x,cn)),
                    MODE_CC=>-i_lim,_=>i_lim,
                }),
            _=>continue,
        };
        add(pos,-current);add(neg,current);
    }
    for &fixed in &sys.fixed {residual[fixed]=-x[fixed];}
    residual
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Cache {
    topos: Vec<Topo>,
    systems: Vec<System>,
    tau: Option<TauInfo>,
}

impl Cache {
    fn topo(&mut self, def: &IslandDef, modes: &[u8]) -> usize {
        if let Some(i) = self.topos.iter().position(|t| t.modes == modes) {
            return i;
        }
        if self.topos.len() == CACHE_LEN {
            self.topos.remove(0);
        }
        self.topos.push(build_topo(def, modes));
        self.topos.len() - 1
    }

    fn system(&mut self, def: &IslandDef, h: f64, tr: &[bool], modes: &[u8], cnt: &mut Counters) -> usize {
        let h_bits = h.to_bits();
        if let Some(i) = self.systems.iter().position(|s| s.h_bits == h_bits && s.tr == tr && s.modes == modes) {
            return i;
        }
        let ti = self.topo(def, modes);
        let sys = build_system(def, &self.topos[ti].fixed, h, tr, modes);
        cnt.factorizations += 1;
        if self.systems.len() == CACHE_LEN {
            self.systems.remove(0);
        }
        self.systems.push(sys);
        self.systems.len() - 1
    }

    /// Capacitor forest, time constants and sub-step count for these modes.
    pub(crate) fn tau(&mut self, def: &IslandDef, modes: &[u8]) -> &TauInfo {
        if self.tau.as_ref().map_or(false, |t| t.modes != modes) {
            self.tau = None;
        }
        self.tau.get_or_insert_with(|| tau::compute(def, modes))
    }
}

/// Accepted solution of one solve point (sub-step or preview).
pub(crate) struct Point {
    x: Vec<f64>,
    modes: Vec<u8>,
    caps: Vec<(f64, f64)>,
    /// A mode differs from the stored one: commutation.
    changed: bool,
    ill: bool,
}

/// Step 1 of the mode loop, in ascending key order: a Supply in CC whose terminals are not linked by any other
/// conductance path (or that has no limit) is set to CV. One pass suffices: CV only adds links. Returns true if a
/// mode changed.
fn normalize(def: &IslandDef, cache: &mut Cache, modes: &mut [u8]) -> bool {
    let n = def.nodes.len();
    let mut changed = false;
    for mi in 0..def.modes.len() {
        let (pos, neg, gate, has_limit, constrained) = match def.modes[mi].kind {
            ModeKind::Supply { elem } => match def.lelems[elem] {
                LElem::S { pos, neg, i_lim, gate, .. } => (pos, neg, gate, i_lim.is_some(), modes[mi] == MODE_CC),
                _ => continue,
            },
            ModeKind::Bipolar { elem } => match def.lelems[elem] {
                LElem::B { pos, neg, gate, .. } => (pos, neg, gate, true, modes[mi] != MODE_CV),
                _ => continue,
            },
            _ => continue,
        };
        if !constrained || !active(gate, modes) { continue; }
        let linked = has_limit && {
            let ti = cache.topo(def, modes);
            let t = &cache.topos[ti];
            t.cond[slot(pos, n)] == t.cond[slot(neg, n)]
        };
        if !linked {
            modes[mi] = MODE_CV;
            changed = true;
        }
    }
    changed
}

/// First incoherent mode element in ascending key order, with its next position (one step towards coherence).
fn first_incoherent(def: &IslandDef, x: &[f64], modes: &[u8], topo: &Topo) -> Option<(usize, u8)> {
    let n = def.nodes.len();
    for (mi, md) in def.modes.iter().enumerate() {
        let next = match md.kind {
            ModeKind::Supply { elem } => {
                let LElem::S { pos, neg, v, r_out, i_lim, gate, .. } = def.lelems[elem] else { continue };
                if !active(gate, modes) {
                    continue;
                }
                let xs = volt(x, pos) - volt(x, neg);
                // Terminals not linked by any other conductance: x is an artefact and no current can flow through
                // the supply, so CV (x = v at zero current) is the only coherent mode.
                let unlinked = topo.cond[slot(pos, n)] != topo.cond[slot(neg, n)];
                match (modes[mi], i_lim) {
                    (MODE_CC, None) => Some(MODE_CV),
                    (MODE_CC, Some(lim)) => (unlinked || xs > v - r_out * lim + MODE_BAND_V).then_some(MODE_CV),
                    (MODE_CV, _) => {
                        if i_lim.map_or(false, |lim| xs < v - r_out * lim - MODE_BAND_V) {
                            Some(MODE_CC)
                        } else if xs > v + MODE_BAND_V {
                            Some(MODE_REV)
                        } else {
                            None
                        }
                    }
                    _ => (unlinked || xs < v - MODE_BAND_V).then_some(MODE_CV),
                }
            }
            ModeKind::Gate { cp, cn, on_above, off_below } => {
                let on = modes[mi] == GATE_ON;
                if topo.k[slot(cp, n)] != topo.k[slot(cn, n)] {
                    // Not decidable on artificial potentials: the coherent state is off.
                    on.then_some(GATE_OFF)
                } else {
                    let u = volt(x, cp) - volt(x, cn);
                    if !on && u > on_above {
                        Some(GATE_ON)
                    } else if on && u < off_below {
                        Some(GATE_OFF)
                    } else {
                        None
                    }
                }
            }
            ModeKind::Bipolar { elem } => {
                let LElem::B { pos, neg, cp, cn, gain, r_out, i_lim, gate, .. } = def.lelems[elem] else { continue };
                if !active(gate, modes) { continue; }
                let xs = volt(x, pos) - volt(x, neg);
                let target = gain * (volt(x, cp) - volt(x, cn));
                let unlinked = topo.cond[slot(pos, n)] != topo.cond[slot(neg, n)];
                match modes[mi] {
                    MODE_CC => (unlinked || xs > target - r_out * i_lim + MODE_BAND_V).then_some(MODE_CV),
                    MODE_CV => {
                        if xs < target - r_out * i_lim - MODE_BAND_V { Some(MODE_CC) }
                        else if xs > target + r_out * i_lim + MODE_BAND_V { Some(MODE_REV) }
                        else { None }
                    }
                    _ => (unlinked || xs < target + r_out * i_lim - MODE_BAND_V).then_some(MODE_CV),
                }
            }
        };
        if let Some(m) = next {
            return Some((mi, m));
        }
    }
    None
}

/// One solve point with the mode loop (contract §5.4). `base_tr[c]`: capacitor c may use the trapezoid in this
/// frame; `force_be`: every capacitor in backward Euler (commutation counter, preview).
#[allow(clippy::too_many_arguments)]
fn solve_point(
    def: &IslandDef,
    cache: &mut Cache,
    h: f64,
    base_tr: &[bool],
    force_be: bool,
    caps_in: &[(f64, f64)],
    modes_in: &[u8],
    cnt: &mut Counters,
    precise: bool,
) -> Result<Point, IslandStatus> {
    let n = def.nodes.len();
    let mut modes = modes_in.to_vec();
    let mut changed = false;
    let mut be = force_be;
    let limit = 4 * def.modes.len() + 4;
    let mut solves = 0usize;
    let mut rhs = vec![0.0; n];
    let mut x = vec![0.0; n];
    loop {
        // Step 1 before every solve, not only the first: a move (another supply going REV, a gate turning off) can
        // unlink a supply in CC, whose current would then only reach a fixed artefact row. Checking it in key order
        // with the others instead lets a lower-keyed supply decide on that artefact and cycle.
        if normalize(def, cache, &mut modes) {
            changed = true;
            be = true;
        }
        let tr: Vec<bool> = base_tr.iter().map(|&t| t && !be).collect();
        let si = cache.system(def, h, &tr, &modes, cnt);
        let ti = cache.topo(def, &modes);
        let sys = &cache.systems[si];
        let lu = match &sys.lu {
            Ok(lu) => lu,
            Err(LuFail::Singular) => return Err(IslandStatus::Singular),
            Err(LuFail::NonFinite) => return Err(IslandStatus::NonFinite),
        };
        build_rhs(def, sys, caps_in, &modes, &mut rhs);
        lu.solve(&rhs, &mut x);
        if precise {
            // Static probes only: bounded corrections, existing LU, no retained state.
            let mut delta=vec![0.0;n];
            for _ in 0..2 {
                let residual=branch_residual(def,sys,caps_in,&modes,&x);
                lu.solve(&residual,&mut delta);
                for (value,change) in x.iter_mut().zip(&delta) {*value+=change;}
            }
        }
        solves += 1;
        cnt.mode_solves_max = cnt.mode_solves_max.max(solves);
        if x.iter().any(|v| !v.is_finite()) {
            return Err(IslandStatus::NonFinite);
        }
        match first_incoherent(def, &x, &modes, &cache.topos[ti]) {
            None => {
                let mut caps = Vec::with_capacity(def.caps.len());
                for (ci, c) in def.caps.iter().enumerate() {
                    let j = cap_j(sys, ci, caps_in[ci]);
                    let v = volt(&x, c.a) - volt(&x, c.b);
                    let i = sys.cap_g[ci] * v - j;
                    if !(v.is_finite() && i.is_finite()) {
                        return Err(IslandStatus::NonFinite);
                    }
                    caps.push((v, i));
                }
                return Ok(Point { x, modes, caps, changed, ill: lu.ill_conditioned });
            }
            Some((mi, m)) => {
                if solves >= limit {
                    return Err(IslandStatus::ModeCycle);
                }
                modes[mi] = m;
                changed = true;
                be = true;
            }
        }
    }
}

/// Results of an island, as published after a frame or a preview.
#[derive(Clone, Debug)]
pub(crate) struct Published {
    pub volts: Vec<f64>,
    pub classes: Vec<NodeClass>,
    /// K group id per local node: the ground node for Referenced nodes, else the smallest node of the group.
    pub groups: Vec<NodeId>,
    pub modes: Vec<u8>,
    /// Per island element, in its reference direction.
    pub currents: Vec<f64>,
}

fn publish(def: &IslandDef, cache: &mut Cache, p: &Point, ground: Option<NodeId>) -> Result<Published, IslandStatus> {
    let n = def.nodes.len();
    let x = &p.x;
    let modes = &p.modes;
    let currents: Vec<f64> = def
        .lelems
        .iter()
        .map(|e| match *e {
            LElem::R { a, b, g, gate, .. } => {
                if active(gate, modes) {
                    (volt(x, a) - volt(x, b)) * g
                } else {
                    0.0
                }
            }
            LElem::C { cap, .. } => p.caps[cap].1,
            LElem::V { pos, neg, v, g, gate, .. } => {
                if active(gate, modes) {
                    (v - (volt(x, pos) - volt(x, neg))) * g
                } else {
                    0.0
                }
            }
            LElem::S { pos, neg, v, g, i_lim, mode, gate, .. } => match (active(gate, modes), modes[mode], i_lim) {
                (true, MODE_CV, _) => (v - (volt(x, pos) - volt(x, neg))) * g,
                (true, MODE_CC, Some(lim)) => lim,
                _ => 0.0,
            },
            LElem::E { pos, neg, cp, cn, gain, g, gate, .. } => {
                if active(gate, modes) {
                    (gain * (volt(x, cp) - volt(x, cn)) - (volt(x, pos) - volt(x, neg))) * g
                } else {
                    0.0
                }
            }
            LElem::B { pos, neg, cp, cn, gain, g, i_lim, mode, gate, .. } => {
                if !active(gate, modes) { 0.0 }
                else if modes[mode] == MODE_CV { (gain * (volt(x, cp) - volt(x, cn)) - (volt(x, pos) - volt(x, neg))) * g }
                else if modes[mode] == MODE_CC { i_lim }
                else { -i_lim }
            }
        })
        .collect();
    if currents.iter().any(|c| !c.is_finite()) {
        return Err(IslandStatus::NonFinite);
    }
    let ti = cache.topo(def, modes);
    let topo = &cache.topos[ti];
    let mut size = vec![0usize; n + 1];
    for i in 0..n {
        size[topo.k[i]] += 1;
    }
    let mut classes = Vec::with_capacity(n);
    let mut groups = Vec::with_capacity(n);
    for i in 0..n {
        let r = topo.k[i];
        match ground {
            Some(gnd) if r == topo.k[n] => {
                classes.push(NodeClass::Referenced);
                groups.push(gnd);
            }
            _ => {
                classes.push(if size[r] >= 2 { NodeClass::Relative } else { NodeClass::Floating });
                groups.push(def.nodes[r]);
            }
        }
    }
    Ok(Published { volts: x.clone(), classes, groups, modes: modes.clone(), currents })
}

#[derive(Clone, Debug)]
pub(crate) struct Island {
    pub def: IslandDef,
    /// Stored (v, i) per capacitor and modes per mode element.
    pub caps: Vec<(f64, f64)>,
    pub modes: Vec<u8>,
    /// Backward Euler sub-steps still due after a commutation.
    pub be_remaining: u32,
    pub status: IslandStatus,
    pub dt_ns: Option<i64>,
    pub published: Option<Published>,
    pub cache: Cache,
}

impl Island {
    pub(crate) fn new(def: IslandDef, caps: Vec<(f64, f64)>, modes: Vec<u8>, be_remaining: u32, cache: Cache) -> Self {
        Island { def, caps, modes, be_remaining, status: IslandStatus::Pending, dt_ns: None, published: None, cache }
    }

    /// RESET: discharged, supplies CV, gates off, commuted, nothing published.
    pub(crate) fn reset(&mut self) {
        self.caps.fill((0.0, 0.0));
        self.modes = self.def.modes.iter().map(|m| m.initial()).collect();
        self.be_remaining = BE_AFTER_COMMUTATION;
        self.status = IslandStatus::Pending;
        self.dt_ns = None;
        self.published = None;
    }

    fn fail(&mut self, status: IslandStatus) {
        self.status = status;
        self.published = None;
    }

    /// Error status after a preview or a frame, if any.
    pub(crate) fn error(&self) -> Option<IslandStatus> {
        match self.status {
            IslandStatus::Singular | IslandStatus::NonFinite | IslandStatus::ModeCycle => Some(self.status),
            IslandStatus::Solved { .. } | IslandStatus::Pending => None,
        }
    }

    /// Advances one frame. The stored state changes only if every sub-step succeeded. Returns the sub-step count.
    pub(crate) fn step_frame(&mut self, ground: Option<NodeId>, cnt: &mut Counters) -> i64 {
        let (n_sub, base_tr) = {
            let t = self.cache.tau(&self.def, &self.modes);
            let h = (FRAME_NS / t.n_sub) as f64 / 1e9;
            let base_tr: Vec<bool> = t.mesh.iter().zip(&t.tau).map(|(&mesh, &tk)| !mesh && tk >= h).collect();
            (t.n_sub, base_tr)
        };
        let dt_ns = FRAME_NS / n_sub;
        let h = dt_ns as f64 / 1e9;
        self.dt_ns = Some(dt_ns);
        let mut caps = self.caps.clone();
        let mut modes = self.modes.clone();
        let mut be_remaining = self.be_remaining;
        let mut last: Option<Point> = None;
        for _ in 0..n_sub {
            match solve_point(&self.def, &mut self.cache, h, &base_tr, be_remaining > 0, &caps, &modes, cnt, false) {
                Ok(p) => {
                    be_remaining = if p.changed { BE_AFTER_COMMUTATION } else { be_remaining.saturating_sub(1) };
                    caps.clone_from(&p.caps);
                    modes.clone_from(&p.modes);
                    last = Some(p);
                }
                Err(status) => {
                    self.fail(status);
                    return n_sub;
                }
            }
        }
        let Some(p) = last else {
            self.fail(IslandStatus::Pending);
            return n_sub;
        };
        match publish(&self.def, &mut self.cache, &p, ground) {
            Ok(published) => {
                self.caps = caps;
                self.modes = modes;
                self.be_remaining = be_remaining;
                self.status = IslandStatus::Solved { ill_conditioned: p.ill };
                self.published = Some(published);
            }
            Err(status) => self.fail(status),
        }
        n_sub
    }

    /// One backward Euler step of PREVIEW_H_S from the stored state, published; the stored state is untouched.
    pub(crate) fn preview(&mut self, ground: Option<NodeId>, cnt: &mut Counters, precise:bool) {
        let all_be = vec![false; self.def.caps.len()];
        let point = solve_point(&self.def, &mut self.cache, PREVIEW_H_S, &all_be, true, &self.caps, &self.modes, cnt, precise);
        match point.and_then(|p| publish(&self.def, &mut self.cache, &p, ground).map(|pb| (pb, p.ill))) {
            Ok((published, ill)) => {
                self.status = IslandStatus::Solved { ill_conditioned: ill };
                self.published = Some(published);
            }
            Err(status) => self.fail(status),
        }
    }
}
