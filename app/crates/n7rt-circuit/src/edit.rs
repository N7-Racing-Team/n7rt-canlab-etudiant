//! Edits of contract §3.2 with the geometry of §3.4. An edit runs on a working copy of the committed circuit; the
//! patch is the exact difference over the ids it touched. Preview and commit use this same function.

use std::collections::BTreeSet;

use crate::check::degree_two_is_legal;
use crate::model::{
    add, comp_name, default_params, fmt_pt, in_bounds, is_junction, label_syntax_ok, out_of_bounds_point, path_of, pin_def,
    pin_dir, pin_name, pin_pos, refusal, rot_plus_90, State, JUNCTION_PIN,
};
use crate::validate::{self, Finding, Scope};
use crate::{
    geom, kind_def, values, Circuit, CompId, Component, Edit, End, Endpoint, ItemRef, Kind, KindDef, PinRef, Pt, Refusal,
    RefusalCode, Rot, Value, ValueMap, Wire, WireId,
};

pub(crate) fn run(base: &Circuit, edit: &Edit) -> Result<State, Refusal> {
    let mut st = State::of(base);
    match edit {
        Edit::AddComponent { kind, pos, rot, params, label } => add_component(&mut st, *kind, *pos, *rot, params, label.as_deref())?,
        Edit::Delete { components, wires } => delete(&mut st, components, wires)?,
        Edit::Move { components, wires, delta } => {
            transform(&mut st, components, wires, Motion::Translate(*delta))?;
        }
        Edit::Rotate { components, wires } => transform(&mut st, components, wires, Motion::Rotate)?,
        Edit::SetParam { comp, name, value } => set_param(&mut st, *comp, name, value)?,
        Edit::SetLabel { comp, label } => set_label(&mut st, *comp, label)?,
        Edit::AddWire { a, b, bends } => add_wire(&mut st, a, b, bends.as_deref())?,
        Edit::MoveWireEnd { wire, end, to, bends } => move_wire_end(&mut st, *wire, *end, to, bends.as_deref())?,
        Edit::SetBends { wire, bends } => set_bends(&mut st, *wire, bends)?,
    }
    Ok(st)
}

// ------------------------------------------------------------------------------------------------ validation

fn ambiguity_refusal(st: &State, f: &Finding, wire_edit: bool) -> Refusal {
    Refusal {
        code: RefusalCode::Ambiguous,
        ambiguity: Some(f.ambiguity(wire_edit)),
        message_fr: f.message(&st.comps, wire_edit),
        at: Some(f.at),
        items: f.items(),
    }
}

fn wire_bounds(st: &State, id: WireId) -> Result<(), Refusal> {
    let w = &st.wires[&id];
    match w.bends.iter().copied().find(|p| !in_bounds(*p)) {
        None => Ok(()),
        Some(p) => Err(Refusal {
            at: Some(p),
            items: vec![ItemRef::Wire { id }],
            ..refusal(RefusalCode::OutOfBounds, format!("Coude hors du plan en {}", fmt_pt(p)))
        }),
    }
}

fn comp_bounds(c: &Component) -> Result<(), Refusal> {
    match out_of_bounds_point(c) {
        None => Ok(()),
        Some(p) => Err(Refusal {
            at: Some(p),
            items: vec![ItemRef::Component { id: c.id }],
            ..refusal(
                RefusalCode::OutOfBounds,
                format!("{} sortirait du plan en {} (limite \u{b1}{})", comp_name(c), fmt_pt(p), crate::COORD_LIMIT),
            )
        }),
    }
}

/// One wire against everything else.
fn check_wire(st: &State, id: WireId, wire_edit: bool) -> Result<(), Refusal> {
    wire_bounds(st, id)?;
    let wires: BTreeSet<WireId> = [id].into();
    let none = BTreeSet::new();
    match validate::find(&st.comps, &st.wires, &Scope::Items { comps: &none, wires: &wires }).first() {
        None => Ok(()),
        Some(f) => Err(ambiguity_refusal(st, f, wire_edit)),
    }
}

/// Every touched item still present, against everything.
fn check_touched(st: &State, wire_edit: bool) -> Result<(), Refusal> {
    for id in st.touched_comps() {
        if let Some(c) = st.comps.get(id) {
            comp_bounds(c)?;
        }
    }
    for id in st.touched_wires() {
        if st.wires.contains_key(id) {
            wire_bounds(st, *id)?;
        }
    }
    let comps: BTreeSet<CompId> = st.touched_comps().iter().copied().filter(|id| st.comps.contains_key(id)).collect();
    let wires: BTreeSet<WireId> = st.touched_wires().iter().copied().filter(|id| st.wires.contains_key(id)).collect();
    if let Some(f) = validate::find(&st.comps, &st.wires, &Scope::Items { comps: &comps, wires: &wires }).first() {
        return Err(ambiguity_refusal(st, f, wire_edit));
    }
    // Paths the ambiguity rules cannot judge (two ends on one point after a repair).
    for id in &wires {
        if !st.path(*id).is_some_and(|p| geom::is_orthogonal(&p)) {
            return Err(Refusal {
                items: vec![ItemRef::Wire { id: *id }],
                ..refusal(RefusalCode::NonOrthogonal, "Ce fil n'aurait plus de trac\u{e9} orthogonal".to_string())
            });
        }
    }
    Ok(())
}

// ------------------------------------------------------------------------------------------------ references

fn comp_of<'a>(st: &'a State, id: CompId) -> Result<&'a Component, Refusal> {
    st.comps.get(&id).ok_or_else(|| Refusal {
        items: vec![ItemRef::Component { id }],
        ..refusal(RefusalCode::UnknownComponent, format!("Composant {} inexistant", id.0))
    })
}

fn wire_of(st: &State, id: WireId) -> Result<&Wire, Refusal> {
    st.wires.get(&id).ok_or_else(|| Refusal {
        items: vec![ItemRef::Wire { id }],
        ..refusal(RefusalCode::UnknownWire, format!("Fil {} inexistant", id.0))
    })
}

fn junction_pin(id: CompId) -> PinRef {
    PinRef { comp: id, pin: JUNCTION_PIN.to_string() }
}

/// An endpoint checked against the current tables: an existing pin, or a split of a wire at a grid point.
enum Target {
    Pin(PinRef),
    Split { wire: WireId, at: Pt },
}

fn resolve(st: &State, ep: &Endpoint) -> Result<Target, Refusal> {
    match ep {
        Endpoint::Pin { comp, pin } => {
            let c = comp_of(st, *comp)?;
            if pin_def(c.kind, pin).is_none() {
                return Err(Refusal {
                    items: vec![ItemRef::Component { id: *comp }],
                    ..refusal(
                        RefusalCode::UnknownPin,
                        format!("{} n'a pas de broche \u{ab}\u{a0}{pin}\u{a0}\u{bb}", comp_name(c)),
                    )
                });
            }
            Ok(Target::Pin(PinRef { comp: *comp, pin: pin.clone() }))
        }
        Endpoint::OnWire { wire, at } => {
            let w = wire_of(st, *wire)?;
            let path = path_of(&st.comps, w).unwrap_or_default();
            if path.first() == Some(at) {
                return Ok(Target::Pin(w.a.clone()));
            }
            if path.last() == Some(at) {
                return Ok(Target::Pin(w.b.clone()));
            }
            if !path.windows(2).any(|s| on_segment(s[0], s[1], *at)) {
                return Err(Refusal {
                    at: Some(*at),
                    items: vec![ItemRef::Wire { id: *wire }],
                    ..refusal(RefusalCode::EndpointNotOnWire, format!("Le point {} n'est pas sur ce fil", fmt_pt(*at)))
                });
            }
            // A junction where another wire crosses would join four branches.
            let crossing = st.wires.values().find(|o| {
                o.id != *wire && path_of(&st.comps, o).is_some_and(|p| p.windows(2).any(|s| on_segment(s[0], s[1], *at)))
            });
            if let Some(o) = crossing {
                return Err(Refusal {
                    at: Some(*at),
                    items: vec![ItemRef::Wire { id: *wire }, ItemRef::Wire { id: o.id }],
                    ..refusal(RefusalCode::FourWayJunction, FOUR_WAY)
                });
            }
            Ok(Target::Split { wire: *wire, at: *at })
        }
    }
}

const FOUR_WAY: &str = "Jonction \u{e0} 4 branches interdite : d\u{e9}calez en deux T";

fn on_segment(a: Pt, b: Pt, p: Pt) -> bool {
    (a.x == b.x && p.x == a.x && a.y.min(b.y) <= p.y && p.y <= a.y.max(b.y))
        || (a.y == b.y && p.y == a.y && a.x.min(b.x) <= p.x && p.x <= a.x.max(b.x))
}

/// Pin of a target; a split creates the junction J (id first), then the second half (next id). The half that keeps
/// the original end `a` keeps the wire id.
fn materialize(st: &mut State, t: Target) -> Result<PinRef, Refusal> {
    let (wire, at) = match t {
        Target::Pin(p) => return Ok(p),
        Target::Split { wire, at } => (wire, at),
    };
    let w = st.wires[&wire].clone();
    let path = st.path(wire).unwrap_or_default();
    let k = path.windows(2).position(|s| on_segment(s[0], s[1], at)).unwrap_or(0);
    let mut first: Vec<Pt> = path[..=k].to_vec();
    if first.last() != Some(&at) {
        first.push(at);
    }
    let mut second = vec![at];
    second.extend_from_slice(&path[if path[k + 1] == at { k + 2 } else { k + 1 }..]);

    let jid = CompId(st.alloc()?);
    st.put_comp(Component {
        id: jid,
        kind: Kind::Junction,
        label: String::new(),
        pos: at,
        rot: Rot::R0,
        params: ValueMap::new(),
    });
    let half = WireId(st.alloc()?);
    let j = junction_pin(jid);
    let first = geom::canonical(&first);
    let second = geom::canonical(&second);
    st.put_wire(Wire { id: wire, a: w.a.clone(), b: j.clone(), bends: first[1..first.len() - 1].to_vec() });
    st.put_wire(Wire { id: half, a: j.clone(), b: w.b, bends: second[1..second.len() - 1].to_vec() });
    Ok(j)
}

/// same_pin, duplicate_wire, four_way_junction for a wire `moving` (None: a new wire) between `a` and `b`; only the
/// pins in `attached` gain a wire end.
fn check_ends(st: &State, a: &PinRef, b: &PinRef, moving: Option<WireId>, attached: &[&PinRef]) -> Result<(), Refusal> {
    if a == b {
        return Err(Refusal {
            items: vec![ItemRef::Pin { comp: a.comp, pin: a.pin.clone() }],
            ..refusal(RefusalCode::SamePin, format!("Un fil ne peut pas relier {} \u{e0} elle-m\u{ea}me", pin_name(&st.comps, a)))
        });
    }
    if let Some(d) = st.wires.values().find(|w| Some(w.id) != moving && ((&w.a == a && &w.b == b) || (&w.a == b && &w.b == a))) {
        return Err(Refusal {
            items: vec![ItemRef::Wire { id: d.id }],
            ..refusal(
                RefusalCode::DuplicateWire,
                format!("{} et {} sont d\u{e9}j\u{e0} reli\u{e9}es par un fil", pin_name(&st.comps, a), pin_name(&st.comps, b)),
            )
        });
    }
    for p in attached {
        if is_junction(&st.comps, p.comp) {
            let degree = st.wires_at(p).into_iter().filter(|w| Some(*w) != moving).count();
            if degree >= 3 {
                return Err(Refusal {
                    at: pin_pos(&st.comps, p),
                    items: vec![ItemRef::Component { id: p.comp }],
                    ..refusal(RefusalCode::FourWayJunction, FOUR_WAY)
                });
            }
        }
    }
    Ok(())
}

/// Writes wire `id` from `a` to `b`: the given bends (canonicalized), or the first valid autoroute candidate.
fn route(st: &mut State, id: WireId, a: PinRef, b: PinRef, bends: Option<&[Pt]>) -> Result<(), Refusal> {
    let pa = pin_pos(&st.comps, &a).unwrap_or_default();
    let pb = pin_pos(&st.comps, &b).unwrap_or_default();
    match bends {
        Some(bends) => {
            let mut path = Vec::with_capacity(bends.len() + 2);
            path.push(pa);
            path.extend_from_slice(bends);
            path.push(pb);
            let path = geom::canonical(&path);
            if !geom::is_orthogonal(&path) {
                let at = path.windows(2).find(|s| !geom::is_orthogonal(s)).map_or(pa, |s| s[0]);
                return Err(Refusal {
                    at: Some(at),
                    items: vec![ItemRef::Wire { id }],
                    ..refusal(RefusalCode::NonOrthogonal, format!("Trac\u{e9} non orthogonal \u{e0} partir de {}", fmt_pt(at)))
                });
            }
            st.put_wire(Wire { id, a, b, bends: path[1..path.len() - 1].to_vec() });
            Ok(())
        }
        None => {
            let da = pin_dir(&st.comps, &a).unwrap_or(crate::Dir::R);
            let db = pin_dir(&st.comps, &b).unwrap_or(crate::Dir::R);
            let mut first: Option<Refusal> = None;
            for cand in geom::autoroute(pa, da, pb, db) {
                st.put_wire(Wire { id, a: a.clone(), b: b.clone(), bends: cand });
                match check_wire(st, id, true) {
                    Ok(()) => return Ok(()),
                    Err(r) => {
                        first.get_or_insert(r);
                    }
                }
            }
            let (at, items) = first.map_or((None, Vec::new()), |r| (r.at, r.items));
            Err(Refusal {
                at,
                items,
                ..refusal(
                    RefusalCode::NoRoute,
                    format!("Aucun trac\u{e9} libre entre {} et {} : dessinez les coudes \u{e0} la main", pin_name(&st.comps, &a), pin_name(&st.comps, &b)),
                )
            })
        }
    }
}

// ------------------------------------------------------------------------------------------------ junctions

/// Contract §3.4.7: until stable, the smallest junction of degree ≤ 2 that is not skipped is removed (degree 0),
/// removed with its wire (degree 1) or merged into its smallest wire (degree 2).
pub(crate) fn cleanup(st: &mut State) {
    loop {
        let junctions: Vec<CompId> = st.comps.values().filter(|c| c.kind == Kind::Junction).map(|c| c.id).collect();
        let mut acted = false;
        for jid in junctions {
            let j = junction_pin(jid);
            let at = st.wires_at(&j);
            match at.len() {
                0 => st.remove_comp(jid),
                1 => {
                    st.remove_wire(at[0]);
                    st.remove_comp(jid);
                }
                2 if !degree_two_is_legal(&st.wires, &j, at[0], at[1]) => merge(st, &j, at[0], at[1]),
                _ => continue,
            }
            acted = true;
            break;
        }
        if !acted {
            return;
        }
    }
}

fn merge(st: &mut State, j: &PinRef, w1: WireId, w2: WireId) {
    let mut p1 = st.path(w1).unwrap_or_default();
    let mut p2 = st.path(w2).unwrap_or_default();
    let (a1, b1) = (st.wires[&w1].a.clone(), st.wires[&w1].b.clone());
    let (a2, b2) = (st.wires[&w2].a.clone(), st.wires[&w2].b.clone());
    // P → J along w1, then J → Q along w2.
    let p = if &a1 == j {
        p1.reverse();
        b1
    } else {
        a1
    };
    let q = if &a2 == j {
        b2
    } else {
        p2.reverse();
        a2
    };
    p1.extend_from_slice(&p2[1..]);
    let path = geom::canonical(&p1);
    st.put_wire(Wire { id: w1, a: p, b: q, bends: path[1..path.len() - 1].to_vec() });
    st.remove_wire(w2);
    st.remove_comp(j.comp);
}

// ------------------------------------------------------------------------------------------------ components

fn validate_label(st: &State, def: &KindDef, label: &str, own: Option<CompId>) -> Result<(), Refusal> {
    if def.prefix.is_empty() {
        if label.is_empty() {
            return Ok(());
        }
        return Err(refusal(RefusalCode::LabelInvalid, format!("{} : pas de d\u{e9}signateur", def.name_fr)));
    }
    if !label_syntax_ok(label) {
        return Err(refusal(
            RefusalCode::LabelInvalid,
            format!("D\u{e9}signateur \u{ab}\u{a0}{label}\u{a0}\u{bb} invalide : une lettre, puis au plus 15 lettres, chiffres ou _"),
        ));
    }
    if let Some(other) = st.comps.values().find(|c| c.label == label && Some(c.id) != own) {
        return Err(Refusal {
            items: vec![ItemRef::Component { id: other.id }],
            ..refusal(RefusalCode::LabelDuplicate, format!("Le d\u{e9}signateur {label} est d\u{e9}j\u{e0} utilis\u{e9}"))
        });
    }
    Ok(())
}

fn auto_label(st: &State, def: &KindDef) -> String {
    if def.prefix.is_empty() {
        return String::new();
    }
    let used: BTreeSet<&str> = st.comps.values().map(|c| c.label.as_str()).collect();
    (1u32..).map(|n| format!("{}{n}", def.prefix)).find(|l| !used.contains(l.as_str())).unwrap_or_default()
}

fn param_def(def: &'static KindDef, name: &str, c: &Component) -> Result<&'static crate::ParamDef, Refusal> {
    def.params.iter().find(|p| p.name == name).ok_or_else(|| Refusal {
        items: vec![ItemRef::Component { id: c.id }],
        ..refusal(RefusalCode::ParamUnknown, format!("Param\u{e8}tre \u{ab}\u{a0}{name}\u{a0}\u{bb} inconnu pour {}", def.name_fr))
    })
}

fn value_refusal(c: &Component, (code, message): values::Fail) -> Refusal {
    Refusal { items: vec![ItemRef::Component { id: c.id }], ..refusal(code, message) }
}

fn locked_refusal(c: &Component, pd: &crate::ParamDef) -> Refusal {
    let value = match pd.default {
        crate::Lit::Num(x) => crate::units::quantity(x, pd.unit),
        crate::Lit::Int(i) => crate::units::integer(i, pd.unit),
        _ => String::new(),
    };
    value_refusal(c, (RefusalCode::ParamLocked, format!("{} fix\u{e9}e par le composant ({value})", pd.label_fr)))
}

/// A hidden parameter is never edited, whatever the value: the message names it without any value.
fn hidden_refusal(c: &Component, def: &KindDef) -> Refusal {
    value_refusal(c, (RefusalCode::ParamHidden, format!("Param\u{e8}tre interne de {} : non modifiable", def.name_fr)))
}

fn add_component(
    st: &mut State,
    kind: Kind,
    pos: Pt,
    rot: Rot,
    params: &ValueMap,
    label: Option<&str>,
) -> Result<(), Refusal> {
    let def = kind_def(kind);
    if !def.placeable {
        return Err(refusal(
            RefusalCode::UnknownKind,
            "Une jonction ne se place pas seule : reliez un fil au milieu d'un autre fil".to_string(),
        ));
    }
    let id = CompId(st.next_id);
    let mut comp = Component { id, kind, label: String::new(), pos, rot, params: default_params(kind) };
    for (name, value) in params {
        let pd = param_def(def, name, &comp)?;
        if pd.hidden {
            return Err(hidden_refusal(&comp, def));
        }
        // A locked parameter may only be given with its fixed value: anything else is `param_locked`, not a range.
        let v = match values::coerce(pd, value) {
            Ok(v) if pd.editable || Some(&v) == comp.params.get(pd.name) => v,
            Ok(_) => return Err(locked_refusal(&comp, pd)),
            Err(_) if !pd.editable => return Err(locked_refusal(&comp, pd)),
            Err(f) => return Err(value_refusal(&comp, f)),
        };
        comp.params.insert(pd.name.to_string(), v);
    }
    comp.label = match label {
        Some(l) => {
            validate_label(st, def, l, None)?;
            l.to_string()
        }
        None => auto_label(st, def),
    };
    comp_bounds(&comp)?;
    st.alloc()?;
    st.put_comp(comp);
    check_touched(st, false)
}

fn delete(st: &mut State, components: &[CompId], wires: &[WireId]) -> Result<(), Refusal> {
    for id in components {
        comp_of(st, *id)?;
    }
    for id in wires {
        wire_of(st, *id)?;
    }
    let comps: BTreeSet<CompId> = components.iter().copied().filter(|id| !is_junction(&st.comps, *id)).collect();
    for id in comps {
        for w in st.wires_of(id) {
            st.remove_wire(w);
        }
        st.remove_comp(id);
    }
    for id in wires {
        if st.wires.contains_key(id) {
            st.remove_wire(*id);
        }
    }
    cleanup(st);
    check_touched(st, false)
}

fn set_param(st: &mut State, id: CompId, name: &str, value: &Value) -> Result<(), Refusal> {
    let c = comp_of(st, id)?.clone();
    let def = kind_def(c.kind);
    let pd = param_def(def, name, &c)?;
    if pd.hidden {
        return Err(hidden_refusal(&c, def));
    }
    if !pd.editable {
        return Err(locked_refusal(&c, pd));
    }
    let v = values::coerce(pd, value).map_err(|f| value_refusal(&c, f))?;
    if c.params.get(pd.name) != Some(&v) {
        let mut c = c;
        c.params.insert(pd.name.to_string(), v);
        st.put_comp(c);
    }
    Ok(())
}

fn set_label(st: &mut State, id: CompId, label: &str) -> Result<(), Refusal> {
    let c = comp_of(st, id)?.clone();
    validate_label(st, kind_def(c.kind), label, Some(id)).map_err(|r| Refusal { items: vec![ItemRef::Component { id }], ..r })?;
    if c.label != label {
        let mut c = c;
        c.label = label.to_string();
        st.put_comp(c);
    }
    Ok(())
}

// ------------------------------------------------------------------------------------------------ wires

fn add_wire(st: &mut State, a: &Endpoint, b: &Endpoint, bends: Option<&[Pt]>) -> Result<(), Refusal> {
    let ta = resolve(st, a)?;
    let tb = resolve(st, b)?;
    if let (Endpoint::OnWire { wire: wa, at }, Endpoint::OnWire { wire: wb, .. }) = (a, b) {
        if wa == wb {
            return Err(Refusal {
                at: Some(*at),
                items: vec![ItemRef::Wire { id: *wa }],
                ..refusal(RefusalCode::SameWire, "Les deux extr\u{e9}mit\u{e9}s sont sur le m\u{ea}me fil".to_string())
            });
        }
    }
    let pa = materialize(st, ta)?;
    let pb = materialize(st, tb)?;
    check_ends(st, &pa, &pb, None, &[&pa, &pb])?;
    let id = WireId(st.alloc()?);
    route(st, id, pa, pb, bends)?;
    // A split wire may have been the duplicate that kept a degree-2 junction (§3.4.7): it can merge now.
    cleanup(st);
    check_touched(st, true)
}

fn move_wire_end(st: &mut State, id: WireId, end: End, to: &Endpoint, bends: Option<&[Pt]>) -> Result<(), Refusal> {
    let w = wire_of(st, id)?.clone();
    if let Endpoint::OnWire { wire, at } = to {
        if *wire == id {
            return Err(Refusal {
                at: Some(*at),
                items: vec![ItemRef::Wire { id }],
                ..refusal(RefusalCode::SameWire, "Un fil ne peut pas se rattacher \u{e0} lui-m\u{ea}me".to_string())
            });
        }
    }
    let target = resolve(st, to)?;
    let p = materialize(st, target)?;
    let (a, b) = match end {
        End::A => (p.clone(), w.b.clone()),
        End::B => (w.a.clone(), p.clone()),
    };
    check_ends(st, &a, &b, Some(id), &[&p])?;
    route(st, id, a, b, bends)?;
    cleanup(st);
    check_touched(st, true)
}

fn set_bends(st: &mut State, id: WireId, bends: &[Pt]) -> Result<(), Refusal> {
    let w = wire_of(st, id)?.clone();
    route(st, id, w.a.clone(), w.b.clone(), Some(bends))?;
    if st.wires.get(&id) == Some(&w) {
        return Ok(());
    }
    check_touched(st, true)
}

// ------------------------------------------------------------------------------------------------ move, rotate

#[derive(Clone, Copy)]
enum Motion {
    Translate(Pt),
    /// +90° about a centre chosen from the selection.
    Rotate,
}

/// Selected components, then every junction whose wires all lead into the set (contract §3.4.3).
fn moved_set(st: &State, selected: &BTreeSet<CompId>) -> BTreeSet<CompId> {
    let mut m = selected.clone();
    loop {
        let add: Vec<CompId> = st
            .comps
            .values()
            .filter(|c| c.kind == Kind::Junction && !m.contains(&c.id))
            .filter(|c| {
                let j = junction_pin(c.id);
                let at = st.wires_at(&j);
                !at.is_empty()
                    && at.iter().all(|w| {
                        let w = &st.wires[w];
                        m.contains(&if w.a == j { w.b.comp } else { w.a.comp })
                    })
            })
            .map(|c| c.id)
            .collect();
        if add.is_empty() {
            return m;
        }
        m.extend(add);
    }
}

fn rotate_about(p: Pt, c: Pt) -> Pt {
    let (dx, dy) = (i64::from(p.x) - i64::from(c.x), i64::from(p.y) - i64::from(c.y));
    let clamp = |v: i64| v.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
    Pt { x: clamp(i64::from(c.x) - dy), y: clamp(i64::from(c.y) + dx) }
}

/// Centre of a selection rotation: ⌊centre + 0.5⌋ of the box around the selected bodies and pins (the selected
/// wires when no component is selected).
fn rotation_centre(st: &State, comps: &BTreeSet<CompId>, wires: &BTreeSet<WireId>) -> Pt {
    let mut pts: Vec<Pt> = Vec::new();
    for id in comps {
        let c = &st.comps[id];
        let def = kind_def(c.kind);
        if c.kind != Kind::Junction {
            let r = crate::model::body_rect(c);
            pts.extend([Pt { x: r.x0, y: r.y0 }, Pt { x: r.x1, y: r.y1 }]);
        }
        pts.extend(def.pins.iter().map(|p| crate::model::pin_world(c, p)));
    }
    if pts.is_empty() {
        for id in wires {
            pts.extend(st.path(*id).unwrap_or_default());
        }
    }
    let (x0, x1) = (pts.iter().map(|p| p.x).min().unwrap_or(0), pts.iter().map(|p| p.x).max().unwrap_or(0));
    let (y0, y1) = (pts.iter().map(|p| p.y).min().unwrap_or(0), pts.iter().map(|p| p.y).max().unwrap_or(0));
    let half = |a: i32, b: i32| ((i64::from(a) + i64::from(b) + 1).div_euclid(2)) as i32;
    Pt { x: half(x0, x1), y: half(y0, y1) }
}

/// Repair of a canonical path whose ends become `a` and `b` (contract §3.4.3), then canonicalized.
pub(crate) fn repair(q: &[Pt], a: Pt, b: Pt) -> Vec<Pt> {
    let m = q.len() - 1;
    let (moved_a, moved_b) = (a != q[0], b != q[m]);
    if !moved_a && !moved_b {
        return q.to_vec();
    }
    let r = if m == 1 {
        if q[0].y == q[1].y {
            if a.y == b.y {
                vec![a, b]
            } else {
                let xm = (i64::from(a.x) + (i64::from(b.x) - i64::from(a.x)).div_euclid(2)) as i32;
                vec![a, Pt { x: xm, y: a.y }, Pt { x: xm, y: b.y }, b]
            }
        } else if a.x == b.x {
            vec![a, b]
        } else {
            let ym = (i64::from(a.y) + (i64::from(b.y) - i64::from(a.y)).div_euclid(2)) as i32;
            vec![a, Pt { x: a.x, y: ym }, Pt { x: b.x, y: ym }, b]
        }
    } else if m == 2 && moved_a && moved_b {
        let corner = if q[0].y == q[1].y { Pt { x: b.x, y: a.y } } else { Pt { x: a.x, y: b.y } };
        vec![a, corner, b]
    } else {
        let mut r = q.to_vec();
        if moved_a {
            if q[0].y == q[1].y {
                r[1].y = a.y;
            } else {
                r[1].x = a.x;
            }
            r[0] = a;
        }
        if moved_b {
            if q[m].y == q[m - 1].y {
                r[m - 1].y = b.y;
            } else {
                r[m - 1].x = b.x;
            }
            r[m] = b;
        }
        r
    };
    geom::canonical(&r)
}

fn transform(st: &mut State, components: &[CompId], wires: &[WireId], motion: Motion) -> Result<(), Refusal> {
    for id in components {
        comp_of(st, *id)?;
    }
    for id in wires {
        wire_of(st, *id)?;
    }
    let selected: BTreeSet<CompId> = components.iter().copied().collect();
    let selected_wires: BTreeSet<WireId> = wires.iter().copied().collect();
    if selected.is_empty() && selected_wires.is_empty() {
        return Ok(());
    }
    if let Motion::Translate(d) = motion {
        if d == (Pt { x: 0, y: 0 }) {
            return Ok(());
        }
    }
    let comp_edit = !selected.is_empty();
    let single = selected.len() == 1 && selected_wires.is_empty();
    let centre = match motion {
        Motion::Translate(_) => Pt::default(),
        Motion::Rotate if single => selected.iter().next().and_then(|id| st.comps.get(id)).map(|c| c.pos).unwrap_or_default(),
        Motion::Rotate => rotation_centre(st, &selected, &selected_wires),
    };
    let map = |p: Pt| match motion {
        Motion::Translate(d) => add(p, d),
        Motion::Rotate => rotate_about(p, centre),
    };

    let moved = moved_set(st, &selected);
    let old_paths: Vec<(WireId, Vec<Pt>)> =
        st.wires.keys().filter_map(|id| st.path(*id).map(|p| (*id, p))).collect();

    for id in &moved {
        let mut c = st.comps[id].clone();
        c.pos = map(c.pos);
        if matches!(motion, Motion::Rotate) && c.kind != Kind::Junction {
            c.rot = rot_plus_90(c.rot);
        }
        comp_bounds(&c)?;
        st.put_comp(c);
    }

    let mut repaired: Vec<WireId> = Vec::new();
    for (id, old) in old_paths {
        let w = st.wires[&id].clone();
        let (in_a, in_b) = (moved.contains(&w.a.comp), moved.contains(&w.b.comp));
        let chosen = selected_wires.contains(&id);
        if !(in_a || in_b || chosen) {
            continue;
        }
        let new_a = pin_pos(&st.comps, &w.a).unwrap_or(old[0]);
        let new_b = pin_pos(&st.comps, &w.b).unwrap_or(old[old.len() - 1]);
        let path = if in_a && in_b {
            // Rigid: the bends follow the components.
            let mut p: Vec<Pt> = old.iter().map(|p| map(*p)).collect();
            p[0] = new_a;
            let last = p.len() - 1;
            p[last] = new_b;
            p
        } else {
            let base: Vec<Pt> = if chosen { old.iter().map(|p| map(*p)).collect() } else { old };
            repaired.push(id);
            repair(&base, new_a, new_b)
        };
        if !geom::is_orthogonal(&path) {
            // A repair that degenerates (both ends on the same point) is left to the validation below.
            st.put_wire(Wire { bends: Vec::new(), ..w });
            continue;
        }
        st.put_wire(Wire { bends: path[1..path.len() - 1].to_vec(), ..w });
    }
    settle(st, &repaired, comp_edit)
}

/// Repaired wires that are invalid take the first valid autoroute candidate (contract §3.4.3); otherwise the edit
/// is refused with the reason of the first candidate (of the repaired path when no candidate exists).
fn settle(st: &mut State, repaired: &[WireId], comp_edit: bool) -> Result<(), Refusal> {
    let wire_edit = !comp_edit;
    if check_touched(st, wire_edit).is_ok() {
        return Ok(());
    }
    for id in repaired {
        let mut reason = match check_wire(st, *id, wire_edit) {
            Ok(()) => continue,
            Err(r) => r,
        };
        let w = st.wires[id].clone();
        let (pa, pb) = (pin_pos(&st.comps, &w.a).unwrap_or_default(), pin_pos(&st.comps, &w.b).unwrap_or_default());
        let (da, db) = (pin_dir(&st.comps, &w.a).unwrap_or(crate::Dir::R), pin_dir(&st.comps, &w.b).unwrap_or(crate::Dir::R));
        let mut found = false;
        for (i, cand) in geom::autoroute(pa, da, pb, db).into_iter().enumerate() {
            st.put_wire(Wire { bends: cand, ..w.clone() });
            match check_wire(st, *id, wire_edit) {
                Ok(()) => {
                    found = true;
                    break;
                }
                Err(r) if i == 0 => reason = r,
                Err(_) => {}
            }
        }
        if !found {
            return Err(reason);
        }
    }
    check_touched(st, wire_edit)
}
