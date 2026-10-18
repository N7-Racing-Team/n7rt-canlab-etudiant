//! Hidden wire defects (E7 of the vehicle specification): an open or a series resistance carried by a wire object.
//!
//! The core keeps them outside the document: the snapshot, the patches, the canonical text, the nets and the
//! interface stay those of a healthy wire, and only the lowering sees the defect (the wire does not conduct, or a
//! resistor replaces it). A defect is keyed by its wire object: deleting the wire makes it inert (the repair), and
//! since wire ids are never reused, an undo that brings the same object back brings the defect back too. Only the
//! mission engine sets or clears defects ([`SystemOp`], refused on the interface path).

use std::collections::{BTreeMap, BTreeSet};

use n7rt_circuit::{Circuit, CompId, Kind, Patch, PinRef, Value, Wire, WireId};
use serde::{Deserialize, Serialize};

use super::util::Fnv;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "form", rename_all = "snake_case")]
pub enum WireDefect {
    /// The wire does not conduct.
    Open,
    /// The wire is a resistance of `ohm`.
    Series { ohm: f64 },
}

impl WireDefect {
    fn valid(&self) -> bool {
        match self {
            WireDefect::Open => true,
            WireDefect::Series { ohm } => ohm.is_finite() && *ohm > 0.0,
        }
    }
}

/// A command of the mission engine (journaled like the others, refused when it comes from the interface).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum SystemOp {
    /// Puts `defect` on the wire object joining `a` and `b` (`b` on the ECU side, see [`carrier`]); when no wire
    /// joins them, on the first wire created between them.
    AttachDefect { a: PinRef, b: PinRef, defect: WireDefect },
    /// Puts `defect` on the existing wire object `wire` (the carrier found again after a reopening, or a virtual
    /// copy of the bench).
    DefectOn { wire: WireId, defect: WireDefect },
    /// Removes every defect, carried, inert or pending.
    ClearDefects,
    /// Sets a parameter as an edit would (same validation), outside the undo history, which it clears: the
    /// mechanical setting of an incident cannot be undone, only set back by the student.
    SetParam { comp: CompId, name: String, value: Value },
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Defects {
    /// Defects by wire object; the wires that no longer exist carry an inert defect.
    carried: BTreeMap<WireId, WireDefect>,
    /// A defect waiting for a wire between its two pins.
    pending: Option<(PinRef, PinRef, WireDefect)>,
    /// Defects copied by a junction merge: surviving wire → merged wire (see [`Defects::follow`]).
    copies: BTreeMap<WireId, WireId>,
}

impl Defects {
    /// Records `defect` between `a` and `b`. Err for an invalid resistance.
    pub fn attach(&mut self, circuit: &Circuit, a: &PinRef, b: &PinRef, defect: WireDefect) -> Result<(), ()> {
        if !defect.valid() || a == b {
            return Err(());
        }
        match carrier(circuit, a, b) {
            Some(w) => {
                self.carried.insert(w, defect);
            }
            None => self.pending = Some((a.clone(), b.clone(), defect)),
        }
        Ok(())
    }

    /// Puts `defect` on the existing wire `wire`. Err for an unknown wire or an invalid resistance.
    pub fn put(&mut self, circuit: &Circuit, wire: WireId, defect: WireDefect) -> Result<(), ()> {
        if !defect.valid() || !circuit.wires().contains_key(&wire) {
            return Err(());
        }
        self.carried.insert(wire, defect);
        Ok(())
    }

    /// After an edit: a pending defect goes to the wire that now joins its pins. True when it did.
    pub fn retry(&mut self, circuit: &Circuit) -> bool {
        let Some((a, b, defect)) = self.pending.clone() else { return false };
        match carrier(circuit, &a, &b) {
            Some(w) => {
                self.carried.insert(w, defect);
                self.pending = None;
                true
            }
            None => false,
        }
    }

    /// After an edit: when the junction cleanup merged a wire that carries a defect into another (in `patch`, the
    /// junction and that wire removed, the other wire now joining both far ends), the surviving wire carries the
    /// defect too, the faulty segment being inside it; an undo that brings the merged wire back takes the copy away.
    /// True when a defect moved.
    pub fn follow(&mut self, patch: &Patch) -> bool {
        let mut moved = false;
        for (id, before, after) in &patch.wires {
            if before.is_none() && after.is_some() {
                let back: Vec<WireId> = self.copies.iter().filter(|(_, o)| **o == *id).map(|(s, _)| *s).collect();
                for s in back {
                    self.copies.remove(&s);
                    self.carried.remove(&s);
                    moved = true;
                }
            }
        }
        let gone_junction = |p: &PinRef| {
            patch.components.iter().any(|(c, b, a)| *c == p.comp && a.is_none() && b.as_ref().is_some_and(|b| b.kind == Kind::Junction))
        };
        for (merged, before, after) in &patch.wires {
            let (Some(old), None) = (before, after) else { continue };
            let Some(defect) = self.carried.get(merged).copied() else { continue };
            for j in [&old.a, &old.b] {
                let Some(q) = far_end(old, j).filter(|_| gone_junction(j)) else { continue };
                for (survivor, prev, now) in &patch.wires {
                    let (Some(prev), Some(now)) = (prev, now) else { continue };
                    let Some(p) = far_end(prev, j) else { continue };
                    let joins = (&now.a == p && &now.b == q) || (&now.a == q && &now.b == p);
                    if survivor != merged && joins && !self.carried.contains_key(survivor) {
                        self.carried.insert(*survivor, defect);
                        self.copies.insert(*survivor, *merged);
                        moved = true;
                    }
                }
            }
        }
        moved
    }

    pub fn clear(&mut self) {
        self.carried.clear();
        self.pending = None;
        self.copies.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.carried.is_empty() && self.pending.is_none()
    }

    /// Defects of the wires that exist in `circuit` (the ones the lowering sees).
    pub fn live(&self, circuit: &Circuit) -> BTreeMap<WireId, WireDefect> {
        self.carried.iter().filter(|(w, _)| circuit.wires().contains_key(w)).map(|(w, d)| (*w, *d)).collect()
    }

    /// Wire objects that carry or carried a defect, existing or not.
    pub fn carriers(&self) -> BTreeSet<WireId> {
        self.carried.keys().copied().collect()
    }

    pub fn pending(&self) -> bool {
        self.pending.is_some()
    }

    pub fn digest(&self, h: &mut Fnv) {
        h.u64(self.carried.len() as u64);
        for (w, d) in &self.carried {
            h.u32(w.0);
            match d {
                WireDefect::Open => h.u8(0),
                WireDefect::Series { ohm } => {
                    h.u8(1);
                    h.u64(ohm.to_bits());
                }
            }
        }
        match &self.pending {
            None => h.u8(0),
            Some((a, b, d)) => {
                h.u8(1);
                for p in [a, b] {
                    h.u32(p.comp.0);
                    h.str(&p.pin);
                }
                match d {
                    WireDefect::Open => h.u8(0),
                    WireDefect::Series { ohm } => {
                        h.u8(1);
                        h.u64(ohm.to_bits());
                    }
                }
            }
        }
        h.u64(self.copies.len() as u64);
        for (s, o) in &self.copies {
            h.u32(s.0);
            h.u32(o.0);
        }
    }
}

fn far_end<'a>(w: &'a Wire, p: &PinRef) -> Option<&'a PinRef> {
    if &w.a == p {
        Some(&w.b)
    } else if &w.b == p {
        Some(&w.a)
    } else {
        None
    }
}

/// The wire object that carries a defect between `a` and `b`: the wire joining them; else, when they are joined
/// only through junctions, the wire of the shortest such chain that ends on `b` (the ECU side, where the key
/// measure is taken). Wires in ascending id order: the choice is deterministic.
pub(crate) fn carrier(circuit: &Circuit, a: &PinRef, b: &PinRef) -> Option<WireId> {
    let wires = circuit.wires();
    if let Some(w) = wires.values().find(|w| far_end(w, b) == Some(a)) {
        return Some(w.id);
    }
    let junction = |p: &PinRef| circuit.components().get(&p.comp).is_some_and(|c| c.kind == Kind::Junction);
    let mut seen = BTreeSet::from([b.clone()]);
    let mut frontier: Vec<(PinRef, WireId)> = wires
        .values()
        .filter_map(|w| far_end(w, b).filter(|q| junction(q)).map(|q| (q.clone(), w.id)))
        .filter(|(q, _)| seen.insert(q.clone()))
        .collect();
    while !frontier.is_empty() {
        let mut next = Vec::new();
        for (p, first) in &frontier {
            for w in wires.values() {
                if let Some(q) = far_end(w, p) {
                    if q == a {
                        return Some(*first);
                    }
                    if junction(q) && seen.insert(q.clone()) {
                        next.push((q.clone(), *first));
                    }
                }
            }
        }
        frontier = next;
    }
    None
}
