//! Library versions 2/3: mission hardware, hidden parameters (kept in the document and in saved
//! files, never serialized for the UI, never edited) and older-file compatibility. Neutral catalog values
//! only: no mission value here.

mod common;

use common::*;
use n7rt_circuit::{
    kind_def, library, persist, Edit, Interact, Kind, RefusalCode, Rot, Value, ValueMap, LIBRARY_VERSION, LIBRARY_VERSIONS_READ,
};

fn hidden_names(kind: Kind) -> Vec<&'static str> {
    kind_def(kind).params.iter().filter(|p| p.hidden).map(|p| p.name).collect()
}

#[test]
fn version_two_sensor_migrates_only_the_new_current_limit() {
    let mut d = doc();
    let sensor = add(&mut d, Kind::Mlx90377, 0, 0, Rot::R0);
    let mut old: serde_json::Value = serde_json::from_str(&persist::save(d.circuit())).unwrap();
    old["library_version"] = 2.into();
    old["components"][0]["params"].as_object_mut().unwrap().remove("output_i_lim");
    let restored = persist::load(&old.to_string()).unwrap();
    assert_eq!(restored.components()[&sensor].params["output_i_lim"], Value::Num(0.0));
    let rewritten = persist::save(&restored);
    assert_eq!(persist::save(&persist::load(&rewritten).unwrap()), rewritten);
    old["library_version"] = 3.into();
    assert!(persist::load(&old.to_string()).unwrap_err().iter().any(|v| v.code == "param_missing"));
    old["library_version"] = 2.into();
    old["components"][0]["params"].as_object_mut().unwrap().remove("r_iso");
    assert!(persist::load(&old.to_string()).unwrap_err().iter().any(|v| v.code == "param_missing"));
}

#[test]
fn physical_models_hide_internal_parameters_and_keep_only_their_public_controls() {
    for def in library() {
        let all_hidden = def.params.iter().all(|p| p.hidden);
        match def.kind {
            Kind::EcuApps | Kind::Mlx90377 | Kind::EcuWsf | Kind::EcuSas | Kind::EcuBps
            | Kind::Tle5014 | Kind::Pss260 | Kind::PressureSwitch => {
                assert!(!def.params.is_empty() && all_hidden, "{:?}", def.kind);
                assert!(def.params.iter().all(|p| !p.editable), "{:?}: a hidden value is not editable", def.kind);
            }
            Kind::Ats682Lsh => {
                let visible: Vec<_> = def.params.iter().filter(|p| !p.hidden).map(|p| p.name).collect();
                assert_eq!(visible, ["air_gap_mm", "tolerance_corner"]);
                assert!(def.params.iter().filter(|p| p.hidden).all(|p| !p.editable));
            }
            _ => assert!(def.params.iter().all(|p| !p.hidden), "{:?}", def.kind),
        }
    }
    let ecu = kind_def(Kind::EcuApps);
    let pins: Vec<&str> = ecu.pins.iter().map(|p| p.id).collect();
    assert_eq!(pins, ["lv_in", "gnd", "v5_1", "sig_1", "gnd_s1", "v5_2", "sig_2", "gnd_s2", "can_h", "can_l"]);
    assert_eq!(ecu.prefix, "U");
    let mlx = kind_def(Kind::Mlx90377);
    let pins: Vec<&str> = mlx.pins.iter().map(|p| p.id).collect();
    assert_eq!(pins, ["vdd1", "vss1", "out1", "vdd2", "vss2", "out2"]);
    assert_eq!(mlx.states.len(), 1);
    assert_eq!((mlx.states[0].name, mlx.states[0].persistent), ("position", true));
    assert_eq!(Kind::EcuApps.since_library(), 2);
    assert_eq!(Kind::Resistor.since_library(), 1);
}

#[test]
fn the_library_json_never_carries_a_hidden_parameter() {
    let json = serde_json::to_string(library()).expect("library JSON");
    for name in ["r_ecu", "sig_r_high", "sup_r_low", "rail_i_lim", "tf1_offset", "tf2_clamp_high", "r_iso", "por_on"] {
        assert!(!json.contains(&format!("\"{name}\"")), "{name} in the library JSON");
    }
    let v: serde_json::Value = serde_json::from_str(&json).expect("JSON");
    for kind in ["ecu_apps", "mlx90377", "ecu_wsf", "ecu_sas", "ecu_bps", "tle5014", "pss260", "pressure_switch"] {
        let d = v.as_array().unwrap().iter().find(|d| d["kind"] == kind).expect("listed");
        assert_eq!(d["params"], serde_json::json!([]), "{kind}");
    }
    // The flag itself is not part of the UI contract.
    assert!(!json.contains("\"hidden\""));
    // Visible parameters are unchanged.
    let r = v.as_array().unwrap().iter().find(|d| d["kind"] == "resistor").unwrap();
    assert_eq!(r["params"].as_array().unwrap().len(), 2);
}

#[test]
fn a_component_serialized_for_the_ui_has_no_hidden_parameter_but_the_document_keeps_them() {
    let mut d = doc();
    let ecu = add(&mut d, Kind::EcuApps, 0, 0, Rot::R0);
    let mlx = add(&mut d, Kind::Mlx90377, -30, 0, Rot::R0);
    let mut components = vec![(ecu, Kind::EcuApps), (mlx, Kind::Mlx90377)];
    for (index, kind) in [Kind::EcuWsf, Kind::EcuSas, Kind::EcuBps, Kind::Tle5014, Kind::Pss260, Kind::PressureSwitch].into_iter().enumerate() {
        components.push((add(&mut d, kind, index as i32 * 40, 60, Rot::R0), kind));
    }
    for (id, kind) in components {
        let c = &d.circuit().components()[&id];
        assert_eq!(c.params.len(), hidden_names(kind).len(), "every hidden value is in the document");
        let json = serde_json::to_value(c).expect("component JSON");
        assert_eq!(json["params"], serde_json::json!({}), "{kind:?}");
        assert_eq!(json["kind"], serde_json::to_value(kind).unwrap());
    }
    // Patches too (edit, undo, preview replies).
    let a = edit(&mut d, Edit::Move { components: vec![ecu], wires: vec![], delta: pt(0, 30) });
    let json = serde_json::to_string(&a.patch).expect("patch JSON");
    assert!(!json.contains("r_ecu") && !json.contains("sig_r_high"), "{json}");
    // Visible parameters of the other kinds still travel.
    let r = add(&mut d, Kind::Resistor, 60, 0, Rot::R0);
    let json = serde_json::to_value(&d.circuit().components()[&r]).unwrap();
    assert_eq!(json["params"]["resistance"], serde_json::json!(10000.0));
}

#[test]
fn hidden_parameters_are_never_edited() {
    let mut d = doc();
    let ecu = add(&mut d, Kind::EcuApps, 0, 0, Rot::R0);
    let mlx = add(&mut d, Kind::Mlx90377, -30, 0, Rot::R0);
    for (comp, name) in [(ecu, "r_ecu"), (ecu, "sig_c"), (mlx, "tf1_offset"), (mlx, "r_out")] {
        let r = refused_code(&mut d, Edit::SetParam { comp, name: name.into(), value: Value::Num(1.0) }, RefusalCode::ParamHidden);
        assert!(!r.message_fr.contains('1'), "no value in the message: {}", r.message_fr);
    }
    // Even the current value, and as text.
    let current = d.circuit().components()[&ecu].params["r_ecu"].clone();
    refused_code(&mut d, Edit::SetParam { comp: ecu, name: "r_ecu".into(), value: current }, RefusalCode::ParamHidden);
    refused_code(&mut d, Edit::SetParam { comp: ecu, name: "rail_v".into(), value: Value::Choice("5 V".into()) }, RefusalCode::ParamHidden);
    // Nor given at placement.
    let params: ValueMap = [("tf2_slope".to_string(), Value::Num(0.25))].into_iter().collect();
    refused_code(&mut d, Edit::AddComponent { kind: Kind::Mlx90377, pos: pt(-60, 0), rot: Rot::R0, params, label: None }, RefusalCode::ParamHidden);
    assert_valid(&d);
}

#[test]
fn the_pedal_travel_is_a_quantized_set_pot() {
    let mut d = doc();
    let mlx = add(&mut d, Kind::Mlx90377, 0, 0, Rot::R0);
    let ecu = add(&mut d, Kind::EcuApps, 40, 0, Rot::R0);
    assert_eq!(d.circuit().state(mlx).unwrap()["position"], Value::Num(0.0), "at rest by default");
    let rev = d.rev();
    let s = d.interact(&Interact::SetPot { comp: mlx, position: 0.4567 }).expect("travel");
    assert_eq!(s.state["position"], Value::Num(0.457));
    assert_eq!(d.rev(), rev, "an interaction is not an edit");
    assert_eq!(d.interact(&Interact::SetPot { comp: mlx, position: 1.5 }).unwrap_err().code, RefusalCode::StateOutOfRange);
    assert_eq!(d.interact(&Interact::SetPot { comp: ecu, position: 0.5 }).unwrap_err().code, RefusalCode::NotInteractive);
    assert_eq!(d.interact(&Interact::SetPower { comp: ecu, on: false }).unwrap_err().code, RefusalCode::NotInteractive);
}

#[test]
fn saved_files_keep_hidden_values_and_reload_them() {
    let mut d = doc();
    let ecu = add(&mut d, Kind::EcuApps, 0, 0, Rot::R0);
    let mlx = add(&mut d, Kind::Mlx90377, -30, 0, Rot::R0);
    d.interact(&Interact::SetPot { comp: mlx, position: 0.25 }).unwrap();
    let text = persist::save(d.circuit());
    assert!(text.contains(&format!("\"library_version\": {LIBRARY_VERSION}")));
    assert!(text.contains("\"r_ecu\"") && text.contains("\"tf2_clamp_high\""));
    // A bench file sets its own hidden values (neutral ones here).
    let mut v: serde_json::Value = serde_json::from_str(&text).unwrap();
    for c in v["components"].as_array_mut().unwrap() {
        if c["kind"] == "ecu_apps" {
            c["params"]["r_ecu"] = serde_json::json!(777.0);
        } else {
            c["params"]["tf1_offset"] = serde_json::json!(12.5);
        }
    }
    let edited = serde_json::to_string_pretty(&v).unwrap() + "\n";
    let loaded = persist::load(&edited).expect("bench with hidden values loads");
    assert_eq!(loaded.components()[&ecu].params["r_ecu"], Value::Num(777.0));
    assert_eq!(loaded.components()[&mlx].params["tf1_offset"], Value::Num(12.5));
    assert_eq!(loaded.state(mlx).unwrap()["position"], Value::Num(0.25));
    let canonical = persist::save(&loaded);
    assert!(canonical.contains("\"r_ecu\": 777.0") && canonical.contains("\"tf1_offset\": 12.5"));
    assert_eq!(&persist::load(&canonical).expect("reloads"), &loaded, "hidden values make the round trip");
    let json = serde_json::to_string(&loaded.components()[&ecu]).unwrap();
    assert!(!json.contains("777"), "{json}");
    // Out of range: refused like any parameter.
    let broken = canonical.replace("\"r_ecu\": 777.0", "\"r_ecu\": 0.0");
    assert!(persist::load(&broken).unwrap_err().iter().any(|x| x.code == "param_out_of_range"));
}

#[test]
fn version_1_files_still_load_and_cannot_hold_the_new_kinds() {
    assert_eq!(LIBRARY_VERSION, 3);
    assert_eq!(LIBRARY_VERSIONS_READ, [1, 2, 3]);
    let mut d = doc();
    let r = add(&mut d, Kind::Resistor, 0, 0, Rot::R0);
    let adc = add(&mut d, Kind::AdcInput, 12, 0, Rot::R0);
    wire(&mut d, pin(r, "p2"), pin(adc, "in"), None);
    let v2 = persist::save(d.circuit());
    let v1 = v2.replace("\"library_version\": 3", "\"library_version\": 1");
    assert_ne!(v1, v2);
    let loaded = persist::load(&v1).expect("a version-1 file loads");
    assert_eq!(loaded.components(), d.circuit().components());
    assert_eq!(persist::save(&loaded), v2, "saved again as version 3");

    let mut d = doc();
    add(&mut d, Kind::Mlx90377, 0, 0, Rot::R0);
    let v1 = persist::save(d.circuit()).replace("\"library_version\": 3", "\"library_version\": 1");
    let errs = persist::load(&v1).unwrap_err();
    assert!(errs.iter().any(|x| x.code == "library_mismatch"), "{errs:?}");
    let v3 = persist::save(d.circuit()).replace("\"library_version\": 3", "\"library_version\": 4");
    assert!(persist::load(&v3).unwrap_err().iter().any(|x| x.code == "library_mismatch"));
}
