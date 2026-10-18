//! Canonical persistence (contract §7): generated documents and canonical round trips byte for byte on 1000 seeded random circuits,
//! r_in = ∞, transient states, and broken files refused with the complete list of their violations.

mod common;

use std::collections::BTreeSet;

use common::*;
use n7rt_circuit::{persist, Circuit, Edit, Interact, Kind, Rot, Value, Violation};



fn sample_circuit() -> Circuit {
    let mut d = doc();
    let r1 = add(&mut d, Kind::Resistor, 0, 0, Rot::R0);
    let bp = add(&mut d, Kind::PushButton, 0, 10, Rot::R90);
    let adc = add(&mut d, Kind::AdcInput, 12, 0, Rot::R0);
    wire(&mut d, pin(r1, "p2"), pin(adc, "in"), None);
    // Held down: latched toggles, pressed is transient and never written.
    d.interact(&Interact::SetButton { comp: bp, pressed: true }).unwrap();
    d.circuit().clone()
}

fn sample_text() -> String {
    persist::save(&sample_circuit())
}

fn codes(text: &str) -> BTreeSet<String> {
    match persist::load(text) {
        Ok(_) => panic!("broken file accepted"),
        Err(v) => v.into_iter().map(|v| v.code).collect(),
    }
}

fn violations(text: &str) -> Vec<Violation> {
    persist::load(text).expect_err("broken file accepted")
}

/// The generated canonical text with a JSON edit applied, pretty printed again.
fn mutated(f: impl FnOnce(&mut serde_json::Value)) -> String {
    let mut v: serde_json::Value = serde_json::from_str(&sample_text()).unwrap();
    f(&mut v);
    serde_json::to_string_pretty(&v).unwrap()
}

#[test]
fn canonical_text_round_trip_preserves_the_document() {
    let c = sample_circuit();
    let text = persist::save(&c);
    assert!(text.ends_with("}\n") && !text.contains('\r') && !text.starts_with('\u{feff}'));
    // save(load(t)) == t byte for byte; the transient state restarts from its default.
    let loaded = persist::load(&sample_text()).unwrap();
    assert_eq!(persist::save(&loaded), sample_text());
    assert_eq!(loaded.state(n7rt_circuit::CompId(2)).unwrap()["pressed"], Value::Bool(false));
    assert_eq!(loaded.state(n7rt_circuit::CompId(2)).unwrap()["latched"], Value::Bool(true));
    assert_eq!((loaded.components(), loaded.wires(), loaded.next_id(), loaded.name()), (c.components(), c.wires(), 5, "essai"));
    assert_eq!(loaded.netlist(), c.netlist());
}

#[test]
fn r_in_infinity_and_number_spellings_round_trip() {
    let loaded = persist::load(&sample_text()).unwrap();
    let adc = &loaded.components()[&n7rt_circuit::CompId(3)];
    assert_eq!(adc.params["r_in"], Value::Num(f64::INFINITY));
    // A finite r_in is written as the shortest float that reads back the same f64.
    let mut d = n7rt_circuit::Document::new(loaded);
    for text in ["1000G", "4k7", "1.5 k\u{3a9}", "inf"] {
        edit(&mut d, Edit::SetParam { comp: n7rt_circuit::CompId(3), name: "r_in".into(), value: Value::Choice(text.into()) });
        let t = persist::save(d.circuit());
        let back = persist::load(&t).unwrap();
        assert_eq!(back.components()[&n7rt_circuit::CompId(3)].params["r_in"], d.circuit().components()[&n7rt_circuit::CompId(3)].params["r_in"]);
        assert_eq!(persist::save(&persist::load(&t).unwrap()), t);
    }
    // Shortest representation of awkward values, read back bit for bit.
    let mut d = doc();
    let c = add(&mut d, Kind::Capacitor, 0, 0, Rot::R0);
    for text in ["1p", "2.2n", "4.7\u{b5}", "1", "0.1"] {
        edit(&mut d, Edit::SetParam { comp: c, name: "capacitance".into(), value: Value::Choice(text.into()) });
        let t = persist::save(d.circuit());

        let back = persist::load(&t).unwrap();
        assert_eq!(back.components()[&c].params["capacitance"], d.circuit().components()[&c].params["capacitance"]);
    }
    // A JSON integer for a Number parameter is read exactly (the canonical text then writes a float).
    let t = sample_text().replace("\"resistance\": 10000.0", "\"resistance\": 10000");
    let back = persist::load(&t).unwrap();
    assert_eq!(back.components()[&n7rt_circuit::CompId(1)].params["resistance"], Value::Num(1e4));
    assert_eq!(persist::save(&back), sample_text());
}

#[test]
fn random_circuits_round_trip_byte_for_byte() {
    let mut junctions = 0;
    let mut wires = 0;
    for seed in 0..1000u64 {
        let mut rng = Rng::new(0x5EED_0000 + seed);
        let d = random_document(&mut rng, "al\u{e9}atoire", 40);
        let c = d.circuit();
        assert!(c.check().is_empty());
        let text = persist::save(c);
        let back = persist::load(&text).unwrap_or_else(|v| panic!("seed {seed}: {v:#?}\n{text}"));
        assert_eq!(back.components(), c.components(), "seed {seed}");
        assert_eq!(back.wires(), c.wires(), "seed {seed}");
        assert_eq!((back.next_id(), back.name()), (c.next_id(), c.name()), "seed {seed}");
        assert_eq!(back.netlist(), c.netlist(), "seed {seed}");
        for (id, comp) in c.components() {
            // Persistent states are kept; the transient `pressed` restarts released.
            for sd in n7rt_circuit::kind_def(comp.kind).states {
                let want = if sd.persistent { c.state(*id).unwrap()[sd.name].clone() } else { Value::Bool(false) };
                assert_eq!(back.state(*id).unwrap()[sd.name], want, "seed {seed} {id:?}.{}", sd.name);
            }
        }
        assert_eq!(persist::save(&back), text, "seed {seed}: save(load(t)) ≠ t");
        junctions += c.components().values().filter(|k| k.kind == Kind::Junction).count();
        wires += c.wires().len();
    }
    assert!(junctions > 200 && wires > 1000, "junctions {junctions}, wires {wires}");
}

#[test]
fn identity_of_the_file_is_checked_first() {
    let t = sample_text()
        .replace("\"format\": \"n7rt-physlab-circuit\"", "\"format\": \"autre\"")
        .replace("\"version\": 1", "\"version\": 2")
        .replace("\"library_version\": 3", "\"library_version\": 7");
    let v = violations(&t);
    let got: Vec<(&str, &str)> = v.iter().map(|v| (v.code.as_str(), v.message_fr.as_str())).collect();
    assert_eq!(
        got,
        vec![
            ("format_unknown", "Format \u{ab}\u{a0}autre\u{a0}\u{bb} inconnu : n7rt-physlab-circuit attendu"),
            ("version_unsupported", "Version 2 non prise en charge (version 1 attendue)"),
            ("library_mismatch", "Biblioth\u{e8}que version 7 : ce logiciel utilise la version 3, sans migration"),
        ]
    );
    assert_eq!(codes("{ pas du json"), ["json_invalid".to_string()].into());
    assert_eq!(codes("[1, 2]"), ["json_invalid".to_string()].into());
    assert_eq!(codes(&format!("\u{feff}{}", sample_text())), ["json_invalid".to_string()].into());
    // Out of the f64 range: not a JSON number we accept.
    assert_eq!(codes(&sample_text().replace("\"vref\": 3.3", "\"vref\": 1e999")), ["json_invalid".to_string()].into());
}

#[test]
fn every_violation_of_a_broken_file_is_listed() {
    let t = mutated(|v| {
        v["next_id"] = 4.into();
        let comps = v["components"].as_array_mut().unwrap();
        comps[0]["params"]["resistance"] = 0.0.into();
        comps[0]["pos"] = serde_json::json!([20000, 0]);
        comps[1]["label"] = "BP 1".into();
        comps[1]["state"]["pressed"] = true.into();
        comps[2]["params"]["vref"] = "inf".into();
        comps[2]["params"]["sample_rate"] = 3.into();
        comps[2]["params"]["bits"] = 12.5.into();
        comps.push(serde_json::json!({
            "id": 9, "kind": "flux_capacitor", "label": "F1", "pos": [50, 50], "rot": 0, "params": {}, "state": {}
        }));
        v["wires"][0]["bends"] = serde_json::json!([[5, 0], [5, 0]]);
    });
    let got = codes(&t);
    for code in [
        "id_not_below_next_id",
        "param_out_of_range",
        "out_of_bounds",
        "label_invalid",
        "state_unknown",
        "json_invalid",
        "param_type",
        "unknown_kind",
        "non_orthogonal",
    ] {
        assert!(got.contains(code), "{code} missing from {got:?}");
    }
    // The sample rate message is the one of the edits.
    assert!(violations(&t).iter().any(|v| v.message_fr.contains("doit diviser 1000\u{a0}Hz")));
}

#[test]
fn structural_violations_of_wires_and_junctions() {
    // Non-canonical path: a bend aligned with its neighbours.
    let t = mutated(|v| v["wires"][0]["bends"] = serde_json::json!([[6, 0]]));
    assert_eq!(codes(&t), ["non_canonical".to_string()].into());
    // A second wire on the same pair, overlapping the first one.
    let t = mutated(|v| {
        let w = &v["wires"][0];
        let w2 = serde_json::json!({"id": 5, "a": w["b"].clone(), "b": w["a"].clone(), "bends": []});
        v["wires"].as_array_mut().unwrap().push(w2);
        v["next_id"] = 6.into();
    });
    let got = codes(&t);
    assert!(got.contains("duplicate_wire") && got.contains("collinear_overlap"), "{got:?}");
    // A junction with one wire, a wire to a missing pin and to a missing component.
    let t = mutated(|v| {
        v["components"].as_array_mut().unwrap().push(serde_json::json!({
            "id": 5, "kind": "junction", "label": "", "pos": [6, 6], "rot": 0, "params": {}, "state": {}
        }));
        v["wires"].as_array_mut().unwrap().push(serde_json::json!({
            "id": 6, "a": {"comp": 5, "pin": "j"}, "b": {"comp": 1, "pin": "p1"}, "bends": [[-3, 6]]
        }));
        v["wires"].as_array_mut().unwrap().push(serde_json::json!({
            "id": 7, "a": {"comp": 1, "pin": "p9"}, "b": {"comp": 42, "pin": "p1"}, "bends": []
        }));
        v["next_id"] = 8.into();
    });
    let got = codes(&t);
    for code in ["junction_degree", "unknown_pin", "unknown_component"] {
        assert!(got.contains(code), "{code} missing from {got:?}");
    }
    // A persistent state missing, an unknown field.
    let t = mutated(|v| {
        v["components"][1]["state"] = serde_json::json!({});
        v["components"][0]["color"] = "red".into();
    });
    let got = codes(&t);
    assert!(got.contains("state_missing") && got.contains("json_invalid"), "{got:?}");
    // A pin on another pin.
    let t = mutated(|v| v["components"][2]["pos"] = serde_json::json!([6, 0]));
    assert!(codes(&t).contains("pin_on_pin"));
}

#[test]
fn a_refused_file_loads_nothing() {
    // Even one violation refuses the whole file; nothing is corrected silently.
    let t = sample_text().replace("\"power_rating\": 0.25", "\"power_rating\": 0.01");
    let v = violations(&t);
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].code, "param_out_of_range");
    assert_eq!(v[0].message_fr, "R1 : 10\u{a0}mW hors plage [62.5\u{a0}mW ; 10\u{a0}W]");
    assert_eq!(v[0].items, vec![n7rt_circuit::ItemRef::Component { id: n7rt_circuit::CompId(1) }]);
}
