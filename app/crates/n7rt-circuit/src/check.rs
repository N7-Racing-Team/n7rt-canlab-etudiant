//! Invariants of contract §3.5 on a whole circuit (load, property tests): the complete list of violations.

use std::collections::BTreeMap;

use crate::model::{
    comp_name, fmt_pt, in_bounds, out_of_bounds_point, path_of, pin_def, pin_name, Comps, Wires, JUNCTION_PIN,
};
use crate::validate::{self, Scope};
use crate::{geom, kind_def, values, Circuit, CompId, ItemRef, Kind, PinRef, Pt, RefusalCode, Violation, WireId};

pub(crate) fn code_name(code: RefusalCode) -> String {
    serde_json::to_value(code).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default()
}

fn violation(code: &str, message: String, items: Vec<ItemRef>, at: Option<Pt>) -> Violation {
    Violation { code: code.to_string(), message_fr: message, items, at }
}

fn comp_item(id: CompId) -> Vec<ItemRef> {
    vec![ItemRef::Component { id }]
}

fn wire_item(id: WireId) -> Vec<ItemRef> {
    vec![ItemRef::Wire { id }]
}

/// Far end of `w` seen from the junction pin `j`.
fn far_end<'a>(w: &'a crate::Wire, j: &PinRef) -> &'a PinRef {
    if &w.a == j {
        &w.b
    } else {
        &w.a
    }
}

/// A junction of degree 2 is legal only when merging its two wires would recreate a duplicate (contract §3.4.7).
pub(crate) fn degree_two_is_legal(wires: &Wires, j: &PinRef, w1: WireId, w2: WireId) -> bool {
    let (p, q) = (far_end(&wires[&w1], j), far_end(&wires[&w2], j));
    p == q
        || wires.values().any(|w| w.id != w1 && w.id != w2 && ((&w.a == p && &w.b == q) || (&w.a == q && &w.b == p)))
}

pub(crate) fn check(c: &Circuit) -> Vec<Violation> {
    let mut out = Vec::new();
    check_ids(c, &mut out);
    check_components(c, &mut out);
    check_wires(&c.components, &c.wires, &mut out);
    check_junctions(&c.components, &c.wires, &mut out);
    for f in validate::find(&c.components, &c.wires, &Scope::All) {
        let code = serde_json::to_value(f.ambiguity(false)).ok().and_then(|v| v.as_str().map(str::to_string));
        out.push(violation(&code.unwrap_or_default(), f.message(&c.components, false), f.items(), Some(f.at)));
    }
    out
}

fn check_ids(c: &Circuit, out: &mut Vec<Violation>) {
    for (id, comp) in &c.components {
        if comp.id != *id {
            out.push(violation("duplicate_id", format!("Composant rang\u{e9} sous l'id {} mais d'id {}", id.0, comp.id.0), comp_item(*id), None));
        }
        if id.0 >= c.next_id {
            out.push(violation(
                "id_not_below_next_id",
                format!("Identifiant {} \u{e0} partir de next_id ({})", id.0, c.next_id),
                comp_item(*id),
                None,
            ));
        }
        if c.wires.contains_key(&WireId(id.0)) {
            out.push(violation(
                "duplicate_id",
                format!("L'identifiant {} d\u{e9}signe \u{e0} la fois un composant et un fil", id.0),
                vec![ItemRef::Component { id: *id }, ItemRef::Wire { id: WireId(id.0) }],
                None,
            ));
        }
    }
    for (id, w) in &c.wires {
        if w.id != *id {
            out.push(violation("duplicate_id", format!("Fil rang\u{e9} sous l'id {} mais d'id {}", id.0, w.id.0), wire_item(*id), None));
        }
        if id.0 >= c.next_id {
            out.push(violation(
                "id_not_below_next_id",
                format!("Identifiant {} \u{e0} partir de next_id ({})", id.0, c.next_id),
                wire_item(*id),
                None,
            ));
        }
    }
}

fn check_components(c: &Circuit, out: &mut Vec<Violation>) {
    let mut labels: BTreeMap<&str, CompId> = BTreeMap::new();
    for comp in c.components.values() {
        let def = kind_def(comp.kind);
        let item = comp_item(comp.id);
        let name = comp_name(comp);

        for pd in def.params {
            match comp.params.get(pd.name) {
                None => out.push(violation("param_missing", format!("{name} : param\u{e8}tre {} absent", pd.name), item.clone(), None)),
                Some(v) => {
                    if let Err((code, msg)) = values::check_stored(pd, v) {
                        out.push(violation(&code_name(code), format!("{name} : {msg}"), item.clone(), None));
                    }
                }
            }
        }
        for key in comp.params.keys() {
            if !def.params.iter().any(|p| p.name == key) {
                out.push(violation("param_unknown", format!("{name} : param\u{e8}tre {key} inconnu pour {}", def.name_fr), item.clone(), None));
            }
        }

        if def.prefix.is_empty() {
            if !comp.label.is_empty() {
                out.push(violation("label_invalid", format!("{} sans d\u{e9}signateur : \u{ab}\u{a0}{}\u{a0}\u{bb} refus\u{e9}", def.name_fr, comp.label), item.clone(), None));
            }
        } else if !crate::model::label_syntax_ok(&comp.label) {
            out.push(violation(
                "label_invalid",
                format!("D\u{e9}signateur \u{ab}\u{a0}{}\u{a0}\u{bb} invalide : une lettre, puis au plus 15 lettres, chiffres ou _", comp.label),
                item.clone(),
                None,
            ));
        } else if let Some(other) = labels.insert(&comp.label, comp.id) {
            out.push(violation(
                "label_duplicate",
                format!("Le d\u{e9}signateur {} est utilis\u{e9} deux fois", comp.label),
                vec![ItemRef::Component { id: other }, ItemRef::Component { id: comp.id }],
                None,
            ));
        }

        if let Some(p) = out_of_bounds_point(comp) {
            out.push(violation("out_of_bounds", format!("{name} hors du plan en {}", fmt_pt(p)), item.clone(), Some(p)));
        }

        match c.states.get(&comp.id) {
            None if !def.states.is_empty() => {
                out.push(violation("state_missing", format!("{name} : \u{e9}tats d'interaction absents"), item.clone(), None))
            }
            None => {}
            Some(states) => {
                for sd in def.states {
                    match states.get(sd.name) {
                        None => out.push(violation("state_missing", format!("{name} : \u{e9}tat {} absent", sd.name), item.clone(), None)),
                        Some(v) => {
                            if let Err((code, msg)) = values::check_state(sd, v) {
                                out.push(violation(&code_name(code), format!("{name} : {msg}"), item.clone(), None));
                            }
                        }
                    }
                }
                for key in states.keys() {
                    if !def.states.iter().any(|s| s.name == key) {
                        out.push(violation("state_unknown", format!("{name} : \u{e9}tat {key} inconnu"), item.clone(), None));
                    }
                }
            }
        }
    }
}

fn check_wires(comps: &Comps, wires: &Wires, out: &mut Vec<Violation>) {
    let mut pairs: BTreeMap<(PinRef, PinRef), WireId> = BTreeMap::new();
    for w in wires.values() {
        let item = wire_item(w.id);
        let mut ends_ok = true;
        for end in [&w.a, &w.b] {
            match comps.get(&end.comp) {
                None => {
                    ends_ok = false;
                    out.push(violation("unknown_component", format!("Fil reli\u{e9} au composant {} inexistant", end.comp.0), item.clone(), None));
                }
                Some(comp) if pin_def(comp.kind, &end.pin).is_none() => {
                    ends_ok = false;
                    out.push(violation(
                        "unknown_pin",
                        format!("Fil reli\u{e9} \u{e0} une broche {} inexistante sur {}", end.pin, comp_name(comp)),
                        item.clone(),
                        None,
                    ));
                }
                Some(_) => {}
            }
        }
        if w.a == w.b {
            out.push(violation("same_pin", format!("Fil reliant {} \u{e0} elle-m\u{ea}me", pin_name(comps, &w.a)), item.clone(), None));
        }
        let key = if w.a <= w.b { (w.a.clone(), w.b.clone()) } else { (w.b.clone(), w.a.clone()) };
        if let Some(other) = pairs.insert(key, w.id) {
            out.push(violation(
                "duplicate_wire",
                format!("{} et {} reli\u{e9}es par deux fils", pin_name(comps, &w.a), pin_name(comps, &w.b)),
                vec![ItemRef::Wire { id: other }, ItemRef::Wire { id: w.id }],
                None,
            ));
        }
        if let Some(p) = w.bends.iter().copied().find(|p| !in_bounds(*p)) {
            out.push(violation("out_of_bounds", format!("Coude hors du plan en {}", fmt_pt(p)), item.clone(), Some(p)));
        }
        if !ends_ok {
            continue;
        }
        let Some(path) = path_of(comps, w) else { continue };
        if !geom::is_orthogonal(&path) {
            let at = path.windows(2).find(|s| !geom::is_orthogonal(s)).map(|s| s[0]);
            out.push(violation("non_orthogonal", "Trac\u{e9} non orthogonal ou segment de longueur nulle".to_string(), item.clone(), at));
        } else if geom::canonical(&path) != path {
            out.push(violation("non_canonical", "Trac\u{e9} non canonique (coude align\u{e9} ou aller-retour)".to_string(), item.clone(), None));
        }
    }
}

fn check_junctions(comps: &Comps, wires: &Wires, out: &mut Vec<Violation>) {
    for comp in comps.values().filter(|c| c.kind == Kind::Junction) {
        let j = PinRef { comp: comp.id, pin: JUNCTION_PIN.to_string() };
        let at: Vec<WireId> = wires.values().filter(|w| w.a == j || w.b == j).map(|w| w.id).collect();
        let legal = match at.len() {
            3 => true,
            2 => degree_two_is_legal(wires, &j, at[0], at[1]),
            _ => false,
        };
        if !legal {
            out.push(violation(
                "junction_degree",
                format!("Jonction J{} de degr\u{e9} {} (3 fils, ou 2 si leur fusion cr\u{e9}erait un doublon)", comp.id.0, at.len()),
                comp_item(comp.id),
                Some(comp.pos),
            ));
        }
    }
}
