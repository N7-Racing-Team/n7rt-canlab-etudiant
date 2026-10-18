//! Ambiguity rules of contract §3.4.2: "what looks connected is; nothing looks connected without being". Checked
//! on the final geometry of an edit (only pairs that involve a changed item: the rest was valid) and on a whole
//! circuit (check, load).

use std::collections::{BTreeMap, BTreeSet};

use crate::model::{body_rect, comp_name, path_of, pin_name, pin_world, Comps, Wires};
use crate::{geom, kind_def, Ambiguity, CompId, ItemRef, Kind, PinRef, Pt, Rect, WireId};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Seg {
    a: Pt,
    b: Pt,
}

impl Seg {
    fn horizontal(&self) -> bool {
        self.a.y == self.b.y
    }

    /// Fixed coordinate (y of a horizontal segment, x of a vertical one) and the range along the axis.
    fn line(&self) -> (i32, i32, i32) {
        if self.horizontal() {
            (self.a.y, self.a.x.min(self.b.x), self.a.x.max(self.b.x))
        } else {
            (self.a.x, self.a.y.min(self.b.y), self.a.y.max(self.b.y))
        }
    }

    fn contains(&self, p: Pt) -> bool {
        let (c, lo, hi) = self.line();
        if self.horizontal() {
            p.y == c && lo <= p.x && p.x <= hi
        } else {
            p.x == c && lo <= p.y && p.y <= hi
        }
    }

    fn point(&self, along: i32) -> Pt {
        let (c, _, _) = self.line();
        if self.horizontal() {
            Pt { x: along, y: c }
        } else {
            Pt { x: c, y: along }
        }
    }
}

/// Start of the collinear overlap of positive length, if any.
fn collinear_overlap(s: Seg, t: Seg) -> Option<Pt> {
    if s.horizontal() != t.horizontal() {
        return None;
    }
    let (cs, slo, shi) = s.line();
    let (ct, tlo, thi) = t.line();
    let (lo, hi) = (slo.max(tlo), shi.min(thi));
    (cs == ct && lo < hi).then(|| s.point(lo))
}

/// Any common point of two axis-aligned segments, and whether it is a proper perpendicular crossing of interiors.
fn touch(s: Seg, t: Seg) -> Option<(Pt, bool)> {
    if s.horizontal() == t.horizontal() {
        let (cs, slo, shi) = s.line();
        let (ct, tlo, thi) = t.line();
        let lo = slo.max(tlo);
        return (cs == ct && lo <= shi.min(thi)).then(|| (s.point(lo), false));
    }
    let (h, v) = if s.horizontal() { (s, t) } else { (t, s) };
    let (y, hlo, hhi) = h.line();
    let (x, vlo, vhi) = v.line();
    if hlo <= x && x <= hhi && vlo <= y && y <= vhi {
        let proper = hlo < x && x < hhi && vlo < y && y < vhi;
        Some((Pt { x, y }, proper))
    } else {
        None
    }
}

/// A point where the open segment meets the closed rectangle.
fn through_rect(s: Seg, r: &Rect) -> Option<Pt> {
    let (c, lo, hi) = s.line();
    let (clo, chi, alo, ahi) = if s.horizontal() { (r.y0, r.y1, r.x0, r.x1) } else { (r.x0, r.x1, r.y0, r.y1) };
    (clo <= c && c <= chi && lo < ahi && hi > alo).then(|| s.point(lo.clamp(alo, ahi)))
}

fn rects_meet(a: &Rect, b: &Rect) -> bool {
    a.x0 <= b.x1 && b.x0 <= a.x1 && a.y0 <= b.y1 && b.y0 <= a.y1
}

fn rect_has(r: &Rect, p: Pt) -> bool {
    r.x0 <= p.x && p.x <= r.x1 && r.y0 <= p.y && p.y <= r.y1
}

fn bbox(points: &[Pt]) -> Rect {
    let mut r = Rect { x0: i32::MAX, y0: i32::MAX, x1: i32::MIN, y1: i32::MIN };
    for p in points {
        r.x0 = r.x0.min(p.x);
        r.y0 = r.y0.min(p.y);
        r.x1 = r.x1.max(p.x);
        r.y1 = r.y1.max(p.y);
    }
    r
}

struct PinGeo {
    pin: PinRef,
    pos: Pt,
    lead: Option<Seg>,
}

struct CompGeo {
    id: CompId,
    /// None for the junction (no body, no lead).
    body: Option<Rect>,
    pins: Vec<PinGeo>,
    extent: Rect,
}

struct WireGeo {
    id: WireId,
    path: Vec<Pt>,
    segs: Vec<Seg>,
    extent: Rect,
}

impl WireGeo {
    fn on_path(&self, p: Pt) -> bool {
        rect_has(&self.extent, p) && self.segs.iter().any(|s| s.contains(p))
    }

    fn is_end(&self, p: Pt) -> bool {
        p == self.path[0] || p == self.path[self.path.len() - 1]
    }
}

/// Which pairs are checked: all of them, or only those that involve a changed item.
pub(crate) enum Scope<'a> {
    All,
    Items { comps: &'a BTreeSet<CompId>, wires: &'a BTreeSet<WireId> },
}

impl Scope<'_> {
    fn comp(&self, id: CompId) -> bool {
        match self {
            Scope::All => true,
            Scope::Items { comps, .. } => comps.contains(&id),
        }
    }

    fn wire(&self, id: WireId) -> bool {
        match self {
            Scope::All => true,
            Scope::Items { wires, .. } => wires.contains(&id),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) enum Detail {
    PinOnPin(PinRef, PinRef),
    PinOnWire(PinRef, WireId),
    WireOnWire(WireId, WireId),
    WireOnLead(WireId, PinRef),
    SelfOverlap(WireId),
    BendOnWire(WireId, WireId),
    ThroughBody(WireId, CompId),
}

#[derive(Clone, Debug)]
pub(crate) struct Finding {
    pub detail: Detail,
    pub at: Pt,
}

impl Finding {
    fn rank(&self) -> u8 {
        match self.detail {
            Detail::PinOnPin(..) => 0,
            Detail::PinOnWire(..) => 1,
            Detail::WireOnWire(..) | Detail::WireOnLead(..) => 2,
            Detail::SelfOverlap(..) => 3,
            Detail::BendOnWire(..) => 4,
            Detail::ThroughBody(..) => 5,
        }
    }

    /// Sub-code; `wire_edit` names the pin/wire conflict after what was edited (`wire_over_pin` for a wire edit).
    pub fn ambiguity(&self, wire_edit: bool) -> Ambiguity {
        match self.detail {
            Detail::PinOnPin(..) => Ambiguity::PinOnPin,
            Detail::PinOnWire(..) if wire_edit => Ambiguity::WireOverPin,
            Detail::PinOnWire(..) => Ambiguity::PinOnWire,
            Detail::WireOnWire(..) | Detail::WireOnLead(..) => Ambiguity::CollinearOverlap,
            Detail::SelfOverlap(..) => Ambiguity::SelfOverlap,
            Detail::BendOnWire(..) => Ambiguity::BendOnWire,
            Detail::ThroughBody(..) => Ambiguity::WireThroughBody,
        }
    }

    pub fn items(&self) -> Vec<ItemRef> {
        let pin = |p: &PinRef| ItemRef::Pin { comp: p.comp, pin: p.pin.clone() };
        let wire = |w: &WireId| ItemRef::Wire { id: *w };
        match &self.detail {
            Detail::PinOnPin(a, b) => vec![pin(a), pin(b)],
            Detail::PinOnWire(p, w) => vec![pin(p), wire(w)],
            Detail::WireOnWire(a, b) | Detail::BendOnWire(a, b) => vec![wire(a), wire(b)],
            Detail::WireOnLead(w, p) => vec![wire(w), pin(p)],
            Detail::SelfOverlap(w) => vec![wire(w)],
            Detail::ThroughBody(w, c) => vec![wire(w), ItemRef::Component { id: *c }],
        }
    }

    pub fn message(&self, comps: &Comps, wire_edit: bool) -> String {
        match &self.detail {
            Detail::PinOnPin(a, b) => format!(
                "Les broches {} et {} se superposent sans \u{ea}tre reli\u{e9}es : d\u{e9}calez le composant",
                pin_name(comps, a),
                pin_name(comps, b)
            ),
            Detail::PinOnWire(p, _) if wire_edit => {
                format!("Ce fil passerait sur la broche {} sans s'y relier", pin_name(comps, p))
            }
            Detail::PinOnWire(p, _) => {
                format!("La broche {} toucherait un fil sans y \u{ea}tre reli\u{e9}e", pin_name(comps, p))
            }
            Detail::WireOnWire(..) => {
                "Deux fils se superposeraient : on ne verrait plus o\u{f9} ils vont".to_string()
            }
            Detail::WireOnLead(_, p) => {
                format!("Un fil recouvrirait la patte de la broche {}", pin_name(comps, p))
            }
            Detail::SelfOverlap(_) => "Ce fil se chevaucherait ou se toucherait lui-m\u{ea}me".to_string(),
            Detail::BendOnWire(..) => {
                "Un coude toucherait un autre fil : ils sembleraient reli\u{e9}s sans l'\u{ea}tre".to_string()
            }
            Detail::ThroughBody(_, c) => format!(
                "Ce fil traverserait le corps de {}",
                comps.get(c).map_or_else(|| format!("{}", c.0), comp_name)
            ),
        }
    }

    fn sort_key(&self) -> (u8, Vec<(u8, u32, String)>, Pt) {
        let key = self
            .items()
            .into_iter()
            .map(|i| match i {
                ItemRef::Component { id } => (0, id.0, String::new()),
                ItemRef::Wire { id } => (1, id.0, String::new()),
                ItemRef::Pin { comp, pin } => (2, comp.0, pin),
            })
            .collect();
        (self.rank(), key, self.at)
    }
}

/// Every ambiguity in scope, sorted by rule (table order of §3.4.2), then items. Wires with a missing end or a
/// path that is not orthogonal are skipped (reported elsewhere).
pub(crate) fn find(comps: &Comps, wires: &Wires, scope: &Scope) -> Vec<Finding> {
    let cg: Vec<CompGeo> = comps
        .values()
        .map(|c| {
            let def = kind_def(c.kind);
            let junction = c.kind == Kind::Junction;
            let pins: Vec<PinGeo> = def
                .pins
                .iter()
                .map(|p| {
                    let pos = pin_world(c, p);
                    let dir = crate::model::dir_vec(geom::rotate_dir(p.dir, c.rot));
                    let lead = (!junction).then(|| Seg { a: crate::model::sub(pos, dir), b: pos });
                    PinGeo { pin: PinRef { comp: c.id, pin: p.id.to_string() }, pos, lead }
                })
                .collect();
            let body = (!junction).then(|| body_rect(c));
            let mut pts: Vec<Pt> = pins.iter().map(|p| p.pos).collect();
            if let Some(b) = body {
                pts.extend([Pt { x: b.x0, y: b.y0 }, Pt { x: b.x1, y: b.y1 }]);
            }
            CompGeo { id: c.id, body, pins, extent: bbox(&pts) }
        })
        .collect();
    let wg: Vec<WireGeo> = wires
        .values()
        .filter_map(|w| {
            let path = path_of(comps, w)?;
            if !geom::is_orthogonal(&path) {
                return None;
            }
            let segs = path.windows(2).map(|p| Seg { a: p[0], b: p[1] }).collect();
            Some(WireGeo { id: w.id, extent: bbox(&path), path, segs })
        })
        .collect();

    let mut out: Vec<Finding> = Vec::new();

    // pin_on_pin
    let mut at_point: BTreeMap<Pt, Vec<&PinGeo>> = BTreeMap::new();
    for c in &cg {
        for p in &c.pins {
            at_point.entry(p.pos).or_default().push(p);
        }
    }
    for (pos, pins) in &at_point {
        for (i, a) in pins.iter().enumerate() {
            for b in &pins[i + 1..] {
                if a.pin.comp != b.pin.comp && (scope.comp(a.pin.comp) || scope.comp(b.pin.comp)) {
                    out.push(Finding { detail: Detail::PinOnPin(a.pin.clone(), b.pin.clone()), at: *pos });
                }
            }
        }
    }

    for w in &wg {
        let w_in = scope.wire(w.id);
        for c in &cg {
            if !(w_in || scope.comp(c.id)) || !rects_meet(&w.extent, &c.extent) {
                continue;
            }
            // pin_on_wire / wire_over_pin: a wire touches a pin only at its two ends.
            for p in &c.pins {
                if w.on_path(p.pos) && !w.is_end(p.pos) {
                    out.push(Finding { detail: Detail::PinOnWire(p.pin.clone(), w.id), at: p.pos });
                }
            }
            // collinear_overlap with a lead
            for p in &c.pins {
                if let Some(lead) = p.lead {
                    if let Some(at) = w.segs.iter().find_map(|s| collinear_overlap(*s, lead)) {
                        out.push(Finding { detail: Detail::WireOnLead(w.id, p.pin.clone()), at });
                    }
                }
            }
            // wire_through_body
            if let Some(body) = c.body {
                if let Some(at) = w.segs.iter().find_map(|s| through_rect(*s, &body)) {
                    out.push(Finding { detail: Detail::ThroughBody(w.id, c.id), at });
                }
            }
        }

        // self_overlap
        if w_in {
            'pairs: for (i, s) in w.segs.iter().enumerate() {
                for (j, t) in w.segs.iter().enumerate().skip(i + 1) {
                    let hit = if j == i + 1 {
                        collinear_overlap(*s, *t)
                    } else {
                        touch(*s, *t).and_then(|(at, proper)| (!proper).then_some(at))
                    };
                    if let Some(at) = hit {
                        out.push(Finding { detail: Detail::SelfOverlap(w.id), at });
                        break 'pairs;
                    }
                }
            }
        }
    }

    // wire / wire: collinear_overlap, bend_on_wire
    for (i, w) in wg.iter().enumerate() {
        for v in &wg[i + 1..] {
            if !(scope.wire(w.id) || scope.wire(v.id)) || !rects_meet(&w.extent, &v.extent) {
                continue;
            }
            if let Some(at) = w.segs.iter().find_map(|s| v.segs.iter().find_map(|t| collinear_overlap(*s, *t))) {
                out.push(Finding { detail: Detail::WireOnWire(w.id, v.id), at });
            }
            for (x, y) in [(w, v), (v, w)] {
                let bends = &x.path[1..x.path.len() - 1];
                if let Some(at) = bends.iter().copied().find(|b| y.on_path(*b) && !y.is_end(*b)) {
                    out.push(Finding { detail: Detail::BendOnWire(x.id, y.id), at });
                }
            }
        }
    }

    out.sort_by_cached_key(Finding::sort_key);
    out
}
