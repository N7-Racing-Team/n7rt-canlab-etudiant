//! Physical checks of a verification (private spec ch. 24 §2.9.2), comparing the student's live circuit with the
//! reference bench of the pack. Positions, bends, wire identifiers and interaction states are ignored.
//!
//! - P-1 connectivity equivalence: the same components (same designator, same kind), none added or removed, ground
//!   symbols and junctions excepted; for every pair of pins of these components, « same net » in the circuit ⇔
//!   « same net » in the reference.
//! - P-2 parameters: every parameter equal to the reference (hidden ones included), except the settings an incident
//!   declares as its repair setting.
//! - P-3 (no hidden defect on an existing wire) and P-4 (the incident's target repaired) are judged by the mission
//!   controller on the live core's defects.

use std::collections::BTreeMap;

use n7rt_circuit::{kind_def, Circuit, Component, Kind, PinRef, Value};

fn named(c: &Circuit) -> BTreeMap<&str, &Component> {
    c.components().values().filter(|x| !matches!(x.kind, Kind::Ground | Kind::Junction)).map(|x| (x.label.as_str(), x)).collect()
}

/// For each pin `(designator, pin)` of the named components, a representative of its net (its smallest named pin).
fn partition(c: &Circuit) -> BTreeMap<(String, String), (String, String)> {
    let labels: BTreeMap<_, _> = c.components().values().filter(|x| !matches!(x.kind, Kind::Ground | Kind::Junction)).map(|x| (x.id, x.label.clone())).collect();
    let mut out = BTreeMap::new();
    for net in c.netlist().nets {
        let mut pins: Vec<(String, String)> =
            net.pins.iter().filter_map(|p| labels.get(&p.comp).map(|l| (l.clone(), p.pin.clone()))).collect();
        pins.sort();
        if let Some(first) = pins.first().cloned() {
            for p in pins {
                out.insert(p, first.clone());
            }
        }
    }
    out
}

/// The same designated components of the same kinds (ground symbols and junctions excepted).
pub fn same_parts(live: &Circuit, reference: &Circuit) -> bool {
    let (a, b) = (named(live), named(reference));
    a.len() == b.len() && a.iter().all(|(label, x)| b.get(label).is_some_and(|y| y.kind == x.kind))
}

/// P-1: same designated components of the same kinds, and the same connectivity between their pins.
pub fn same_connectivity(live: &Circuit, reference: &Circuit) -> bool {
    same_parts(live, reference) && partition(live) == partition(reference)
}

/// P-2: every parameter of every designated component equal to the reference, except `exempt` (designator,
/// parameter) pairs. Call after [`same_connectivity`].
pub fn same_parameters(live: &Circuit, reference: &Circuit, exempt: &[(&str, &str)]) -> bool {
    let b = named(reference);
    named(live).iter().all(|(label, x)| {
        b.get(label).is_some_and(|y| {
            let keys: std::collections::BTreeSet<&String> = x.params.keys().chain(y.params.keys()).collect();
            keys.into_iter().all(|k| exempt.contains(&(*label, k.as_str())) || x.params.get(k) == y.params.get(k))
        })
    })
}

/// P-1 ∧ P-2 (∧ P-3, trivially true for now).
pub fn bench_matches(live: &Circuit, reference: &Circuit, exempt: &[(&str, &str)]) -> bool {
    same_connectivity(live, reference) && same_parameters(live, reference, exempt)
}

/// What changed in the circuit from `before` to `after`, in the bench's own words: wires by their two ends (a junction
/// is « jonction »), components retired or added, visible parameters changed (hidden ones never appear). At most
/// [`crate::workflow::progress::MAX_CHANGES`] lines.
pub fn changes(before: &Circuit, after: &Circuit) -> Vec<String> {
    let end = |c: &Circuit, p: &PinRef| -> String {
        match c.components().get(&p.comp) {
            Some(x) if x.kind == Kind::Junction => "jonction".to_owned(),
            Some(x) if x.label.is_empty() => kind_def(x.kind).name_fr.to_owned(),
            Some(x) => format!("{}.{}", x.label, p.pin),
            None => "?".to_owned(),
        }
    };
    let name = |x: &Component| {
        if x.label.is_empty() {
            kind_def(x.kind).name_fr.to_owned()
        } else {
            format!("{} ({})", x.label, kind_def(x.kind).name_fr)
        }
    };
    let show = |v: Option<&Value>| match v {
        Some(Value::Num(x)) => format!("{x}"),
        Some(Value::Int(i)) => i.to_string(),
        Some(Value::Bool(b)) => (if *b { "oui" } else { "non" }).to_owned(),
        Some(Value::Choice(c)) => c.clone(),
        None => "—".to_owned(),
    };
    let mut out = Vec::new();
    for (id, w) in before.wires() {
        if !after.wires().contains_key(id) {
            out.push(format!("Fil retiré : {} – {}", end(before, &w.a), end(before, &w.b)));
        }
    }
    for (id, w) in after.wires() {
        match before.wires().get(id) {
            None => out.push(format!("Fil ajouté : {} – {}", end(after, &w.a), end(after, &w.b))),
            Some(b) if b.a != w.a || b.b != w.b => out.push(format!(
                "Fil rebranché : {} – {} → {} – {}",
                end(before, &b.a),
                end(before, &b.b),
                end(after, &w.a),
                end(after, &w.b)
            )),
            Some(_) => {}
        }
    }
    for (id, x) in before.components() {
        if x.kind != Kind::Junction && !after.components().contains_key(id) {
            out.push(format!("Composant retiré : {}", name(x)));
        }
    }
    for (id, x) in after.components() {
        if x.kind == Kind::Junction {
            continue;
        }
        match before.components().get(id) {
            None => out.push(format!("Composant ajouté : {}", name(x))),
            Some(b) => {
                for def in kind_def(x.kind).params.iter().filter(|d| !d.hidden) {
                    let (old, new) = (b.params.get(def.name), x.params.get(def.name));
                    if old != new {
                        out.push(format!("Paramètre modifié : {}.{} : {} → {}", x.label, def.name, show(old), show(new)));
                    }
                }
            }
        }
    }
    out.truncate(crate::workflow::progress::MAX_CHANGES);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use n7rt_circuit::{CompId, Document, Edit, Endpoint, Pt, Rot, Value, ValueMap};

    fn add(d: &mut Document, kind: Kind, x: i32, label: &str) -> CompId {
        let rev = d.rev();
        let a = d
            .edit(rev, &Edit::AddComponent { kind, pos: Pt { x, y: 0 }, rot: Rot::R0, params: ValueMap::new(), label: Some(label.into()) })
            .unwrap();
        a.patch.components.iter().find(|(_, before, _)| before.is_none()).map(|(id, _, _)| *id).unwrap()
    }

    fn wire(d: &mut Document, a: CompId, pa: &str, b: CompId, pb: &str) {
        let rev = d.rev();
        d.edit(rev, &Edit::AddWire {
            a: Endpoint::Pin { comp: a, pin: pa.into() },
            b: Endpoint::Pin { comp: b, pin: pb.into() },
            bends: None,
        })
        .unwrap();
    }

    /// R1 — R2 in series, R3 alone.
    fn bench() -> (Document, [CompId; 3]) {
        let mut d = Document::new(Circuit::new("banc"));
        let r1 = add(&mut d, Kind::Resistor, 0, "R1");
        let r2 = add(&mut d, Kind::Resistor, 20, "R2");
        let r3 = add(&mut d, Kind::Resistor, 40, "R3");
        wire(&mut d, r1, "p2", r2, "p1");
        (d, [r1, r2, r3])
    }

    #[test]
    fn the_reference_matches_itself_and_moves_do_not_matter() {
        let (reference, [r1, ..]) = bench();
        let mut live = reference.clone();
        let rev = live.rev();
        live.edit(rev, &Edit::Move { components: vec![r1], wires: vec![], delta: Pt { x: 0, y: 10 } }).unwrap();
        assert!(bench_matches(live.circuit(), reference.circuit(), &[]));
    }

    #[test]
    fn a_rewired_equivalent_connection_still_matches() {
        let (reference, [r1, r2, _]) = bench();
        let mut live = reference.clone();
        let wire_id = *live.circuit().wires().keys().next().unwrap();
        let rev = live.rev();
        live.edit(rev, &Edit::Delete { components: vec![], wires: vec![wire_id] }).unwrap();
        assert!(!same_connectivity(live.circuit(), reference.circuit()), "an open connection");
        wire(&mut live, r2, "p1", r1, "p2");
        assert!(bench_matches(live.circuit(), reference.circuit(), &[]), "the same connection drawn again");
    }

    #[test]
    fn other_connections_or_components_fail() {
        let (reference, [_, r2, r3]) = bench();
        let mut extra_wire = reference.clone();
        wire(&mut extra_wire, r2, "p2", r3, "p1");
        assert!(!same_connectivity(extra_wire.circuit(), reference.circuit()));
        let mut extra_part = reference.clone();
        add(&mut extra_part, Kind::Resistor, 60, "R4");
        assert!(!same_connectivity(extra_part.circuit(), reference.circuit()));
        let mut other_kind = Document::new(Circuit::new("banc"));
        let r1 = add(&mut other_kind, Kind::Resistor, 0, "R1");
        let r2 = add(&mut other_kind, Kind::Capacitor, 20, "R2");
        add(&mut other_kind, Kind::Resistor, 40, "R3");
        wire(&mut other_kind, r1, "p2", r2, "p1");
        assert!(!same_connectivity(other_kind.circuit(), reference.circuit()));
    }

    #[test]
    fn parameters_must_be_equal_unless_exempt() {
        let (reference, [_, r2, _]) = bench();
        let mut live = reference.clone();
        let rev = live.rev();
        live.edit(rev, &Edit::SetParam { comp: r2, name: "resistance".into(), value: Value::Num(4700.0) }).unwrap();
        assert!(same_connectivity(live.circuit(), reference.circuit()));
        assert!(!same_parameters(live.circuit(), reference.circuit(), &[]));
        assert!(same_parameters(live.circuit(), reference.circuit(), &[("R2", "resistance")]));
        assert!(!same_parameters(live.circuit(), reference.circuit(), &[("R1", "resistance")]));
    }

    #[test]
    fn changes_name_wires_components_and_visible_parameters_only() {
        let (mut d, [r1, r2, r3]) = bench();
        let ecu = add(&mut d, Kind::EcuApps, 80, "U1");
        let before = d.circuit().clone();
        assert!(changes(&before, &before).is_empty());
        // the R1–R2 wire replaced, R3 changed, R4 added
        let w = *d.circuit().wires().keys().next().unwrap();
        let rev = d.rev();
        d.edit(rev, &Edit::Delete { components: vec![], wires: vec![w] }).unwrap();
        wire(&mut d, r1, "p2", r2, "p1");
        let rev = d.rev();
        d.edit(rev, &Edit::SetParam { comp: r3, name: "resistance".into(), value: Value::Num(2200.0) }).unwrap();
        add(&mut d, Kind::Resistor, 120, "R4");
        let lines = changes(&before, d.circuit());
        assert!(lines.contains(&"Fil retiré : R1.p2 – R2.p1".to_owned()), "{lines:?}");
        assert!(lines.contains(&"Fil ajouté : R1.p2 – R2.p1".to_owned()), "{lines:?}");
        assert!(lines.iter().any(|l| l.starts_with("Paramètre modifié : R3.resistance : ") && l.ends_with("→ 2200")), "{lines:?}");
        assert!(lines.iter().any(|l| l.starts_with("Composant ajouté : R4 (")), "{lines:?}");
        // a hidden parameter never appears, even when it differs
        let mut text: serde_json::Value = serde_json::from_str(&n7rt_circuit::persist::save(&before)).unwrap();
        for c in text["components"].as_array_mut().unwrap() {
            if c["id"] == serde_json::json!(ecu.0) {
                c["params"]["r_ecu"] = serde_json::json!(777.0);
            }
        }
        let hidden = n7rt_circuit::persist::load(&text.to_string()).unwrap();
        assert!(changes(&before, &hidden).is_empty(), "{:?}", changes(&before, &hidden));
    }
}
