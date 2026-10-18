//! Library lookups, world geometry of components, pins and wires, and the working copy used by edits.

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    geom, kind_def, Circuit, CompId, Component, Dir, Kind, PinDef, PinRef, Pt, Rect, Refusal, RefusalCode, Rot, ValueMap,
    Wire, WireId, COORD_LIMIT,
};

pub(crate) type Comps = BTreeMap<CompId, Component>;
pub(crate) type Wires = BTreeMap<WireId, Wire>;

pub(crate) const JUNCTION_PIN: &str = "j";

pub(crate) fn pin_def(kind: Kind, pin: &str) -> Option<&'static PinDef> {
    kind_def(kind).pins.iter().find(|p| p.id == pin)
}

pub(crate) fn add(a: Pt, b: Pt) -> Pt {
    Pt { x: a.x.saturating_add(b.x), y: a.y.saturating_add(b.y) }
}

pub(crate) fn sub(a: Pt, b: Pt) -> Pt {
    Pt { x: a.x.saturating_sub(b.x), y: a.y.saturating_sub(b.y) }
}

pub(crate) fn dir_vec(d: Dir) -> Pt {
    match d {
        Dir::L => Pt { x: -1, y: 0 },
        Dir::R => Pt { x: 1, y: 0 },
        Dir::U => Pt { x: 0, y: -1 },
        Dir::D => Pt { x: 0, y: 1 },
    }
}

pub(crate) fn in_bounds(p: Pt) -> bool {
    p.x.unsigned_abs() <= COORD_LIMIT as u32 && p.y.unsigned_abs() <= COORD_LIMIT as u32
}

pub(crate) fn rot_plus_90(r: Rot) -> Rot {
    match r {
        Rot::R0 => Rot::R90,
        Rot::R90 => Rot::R180,
        Rot::R180 => Rot::R270,
        Rot::R270 => Rot::R0,
    }
}

pub(crate) fn pin_world(c: &Component, p: &PinDef) -> Pt {
    add(c.pos, geom::rotate(p.pos, c.rot))
}

pub(crate) fn body_rect(c: &Component) -> Rect {
    let b = kind_def(c.kind).body;
    let p = add(c.pos, geom::rotate(Pt { x: b.x0, y: b.y0 }, c.rot));
    let q = add(c.pos, geom::rotate(Pt { x: b.x1, y: b.y1 }, c.rot));
    Rect { x0: p.x.min(q.x), y0: p.y.min(q.y), x1: p.x.max(q.x), y1: p.y.max(q.y) }
}

/// First stored-coordinate point of a component outside the plane: position, pins, body corners.
pub(crate) fn out_of_bounds_point(c: &Component) -> Option<Pt> {
    let r = body_rect(c);
    std::iter::once(c.pos)
        .chain(kind_def(c.kind).pins.iter().map(|p| pin_world(c, p)))
        .chain([Pt { x: r.x0, y: r.y0 }, Pt { x: r.x1, y: r.y1 }])
        .find(|p| !in_bounds(*p))
}

pub(crate) fn pin_pos(comps: &Comps, pin: &PinRef) -> Option<Pt> {
    let c = comps.get(&pin.comp)?;
    Some(pin_world(c, pin_def(c.kind, &pin.pin)?))
}

pub(crate) fn pin_dir(comps: &Comps, pin: &PinRef) -> Option<Dir> {
    let c = comps.get(&pin.comp)?;
    Some(geom::rotate_dir(pin_def(c.kind, &pin.pin)?.dir, c.rot))
}

/// [pin a, bends…, pin b], None if an end does not exist.
pub(crate) fn path_of(comps: &Comps, w: &Wire) -> Option<Vec<Pt>> {
    let a = pin_pos(comps, &w.a)?;
    let b = pin_pos(comps, &w.b)?;
    let mut path = Vec::with_capacity(w.bends.len() + 2);
    path.push(a);
    path.extend_from_slice(&w.bends);
    path.push(b);
    Some(path)
}

pub(crate) fn is_junction(comps: &Comps, id: CompId) -> bool {
    comps.get(&id).is_some_and(|c| c.kind == Kind::Junction)
}

pub(crate) fn default_params(kind: Kind) -> ValueMap {
    kind_def(kind).params.iter().map(|p| (p.name.to_string(), p.default.to_value())).collect()
}

pub(crate) fn default_states(kind: Kind) -> ValueMap {
    kind_def(kind).states.iter().map(|s| (s.name.to_string(), s.default.to_value())).collect()
}

/// `[A-Za-z][A-Za-z0-9_]{0,15}`.
pub(crate) fn label_syntax_ok(label: &str) -> bool {
    let b = label.as_bytes();
    !b.is_empty() && b.len() <= 16 && b[0].is_ascii_alphabetic() && b[1..].iter().all(|c| c.is_ascii_alphanumeric() || *c == b'_')
}

/// Name of a component in messages: its designator, "la masse", "la jonction J12".
pub(crate) fn comp_name(c: &Component) -> String {
    match c.kind {
        Kind::Ground => "la masse".to_string(),
        Kind::Junction => format!("la jonction J{}", c.id.0),
        _ if c.label.is_empty() => kind_def(c.kind).name_fr.to_string(),
        _ => c.label.clone(),
    }
}

/// Name of a pin in messages: "R2.1", "U1.PD0", "GND", "J12".
pub(crate) fn pin_name(comps: &Comps, pin: &PinRef) -> String {
    match comps.get(&pin.comp) {
        None => format!("{}.{}", pin.comp.0, pin.pin),
        Some(c) => match c.kind {
            Kind::Ground => "GND".to_string(),
            Kind::Junction => format!("J{}", c.id.0),
            _ => {
                let short = pin_def(c.kind, &pin.pin).map_or(pin.pin.as_str(), |d| d.short);
                format!("{}.{}", c.label, short)
            }
        },
    }
}

pub(crate) fn fmt_pt(p: Pt) -> String {
    format!("({}, {})", p.x, p.y)
}

pub(crate) fn refusal(code: RefusalCode, message: impl Into<String>) -> Refusal {
    Refusal { code, ambiguity: None, message_fr: message.into(), at: None, items: Vec::new() }
}

// --------------------------------------------------------------------------------------------- working copy

/// Mutable copy of the committed tables; every id written or removed is recorded so that the patch is the exact
/// difference over those ids.
#[derive(Clone, Debug)]
pub(crate) struct State {
    pub next_id: u32,
    pub comps: Comps,
    pub wires: Wires,
    touched_comps: BTreeSet<CompId>,
    touched_wires: BTreeSet<WireId>,
}

impl State {
    pub fn of(c: &Circuit) -> State {
        State {
            next_id: c.next_id,
            comps: c.components.clone(),
            wires: c.wires.clone(),
            touched_comps: BTreeSet::new(),
            touched_wires: BTreeSet::new(),
        }
    }

    pub fn alloc(&mut self) -> Result<u32, Refusal> {
        let id = self.next_id;
        self.next_id = id
            .checked_add(1)
            .ok_or_else(|| refusal(RefusalCode::OutOfBounds, "Plus aucun identifiant disponible dans ce sch\u{e9}ma"))?;
        Ok(id)
    }

    pub fn put_comp(&mut self, c: Component) {
        self.touched_comps.insert(c.id);
        self.comps.insert(c.id, c);
    }

    pub fn remove_comp(&mut self, id: CompId) {
        self.touched_comps.insert(id);
        self.comps.remove(&id);
    }

    pub fn put_wire(&mut self, w: Wire) {
        self.touched_wires.insert(w.id);
        self.wires.insert(w.id, w);
    }

    pub fn remove_wire(&mut self, id: WireId) {
        self.touched_wires.insert(id);
        self.wires.remove(&id);
    }

    pub fn touched_comps(&self) -> &BTreeSet<CompId> {
        &self.touched_comps
    }

    pub fn touched_wires(&self) -> &BTreeSet<WireId> {
        &self.touched_wires
    }

    /// Wires with an end on `pin`, ascending.
    pub fn wires_at(&self, pin: &PinRef) -> Vec<WireId> {
        self.wires.values().filter(|w| &w.a == pin || &w.b == pin).map(|w| w.id).collect()
    }

    /// Wires with an end on any pin of `comp`, ascending.
    pub fn wires_of(&self, comp: CompId) -> Vec<WireId> {
        self.wires.values().filter(|w| w.a.comp == comp || w.b.comp == comp).map(|w| w.id).collect()
    }

    pub fn path(&self, id: WireId) -> Option<Vec<Pt>> {
        path_of(&self.comps, self.wires.get(&id)?)
    }

    /// Exact difference with `base` over the touched ids, ascending.
    pub fn patch(&self, base: &Circuit) -> crate::Patch {
        let components = self
            .touched_comps
            .iter()
            .filter_map(|id| {
                let before = base.components.get(id);
                let after = self.comps.get(id);
                (before != after).then(|| (*id, before.cloned(), after.cloned()))
            })
            .collect();
        let wires = self
            .touched_wires
            .iter()
            .filter_map(|id| {
                let before = base.wires.get(id);
                let after = self.wires.get(id);
                (before != after).then(|| (*id, before.cloned(), after.cloned()))
            })
            .collect();
        crate::Patch { components, wires }
    }
}
