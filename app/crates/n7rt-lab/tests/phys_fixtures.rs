//! Rust ↔ TypeScript agreement (contract §11): writes JSON fixtures of real DTOs, produced by the engine on real
//! circuits, to `app/ui/src/lab/__fixtures__/`. `fixtures.test.ts` (vitest) decodes them and checks every field
//! against `types.ts`. The output is deterministic (fixed engine statistics and file dates), and a file is
//! rewritten only when its content changes.

#[path = "phys_kit.rs"]
mod kit;

use std::fs;
use std::path::{Path, PathBuf};

use kit::*;
use n7rt_circuit::{persist, Circuit, CompId, Edit, Interact, Kind, Pt, Rot, Value, ValueMap};
use n7rt_lab::phys::{
    CircuitProbe, CommandReply, Disclosure, EngineStats, FileError, PhysCommand, PhysConfig, PhysCore, PhysError, PhysLab, Reading,
};
use serde::Serialize;

fn out_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/src/lab/__fixtures__")
}

fn write<T: Serialize>(name: &str, value: &T) {
    let dir = out_dir();
    fs::create_dir_all(&dir).expect("fixtures directory");
    let text = serde_json::to_string_pretty(value).expect("serializable DTO") + "\n";
    let path = dir.join(format!("{name}.json"));
    if fs::read_to_string(&path).ok().as_deref() != Some(text.as_str()) {
        fs::write(&path, text).expect("fixture written");
    }
}

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("n7rt-phys-fixtures-{}-{tag}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    d
}

fn stats() -> EngineStats {
    EngineStats { lag_frames: 2, slow: true, frame_us_p95: 41.5, substeps_last: 3, min_dt_ns: 50_000, islands: 4, restarts: 1 }
}

/// Every kind of reading in one circuit: a loaded supply, an ADC saturated by it, an ADC left open, an island
/// without ground, an ADC on a charging RC, an ECU and a transceiver whose TXD is held low by a switch (bus readings withheld), a pot and
/// a capacitor, a latching button left open.
fn showcase() -> Circuit {
    let mut b = Bench::new();
    let v1 = b.add(Kind::DcSupply, &[]);
    let g = b.add(Kind::Ground, &[]);
    let rv1 = b.add(Kind::Potentiometer, &[]);
    let c1 = b.add(Kind::Capacitor, &[]);
    let adc1 = b.add(Kind::AdcInput, &[]);
    let adc2 = b.add(Kind::AdcInput, &[]);
    let adc3 = b.add(Kind::AdcInput, &[]);
    let v2 = b.add(Kind::DcSupply, &[("voltage", Value::Num(12.0))]);
    let r2 = b.add(Kind::Resistor, &[("resistance", Value::Num(4.7e3))]);
    let u1 = b.add(Kind::EcuStm32, &[]);
    let u2 = b.add(Kind::CanTransceiver, &[]);
    let s1 = b.add(Kind::Switch, &[]);
    let rt1 = b.add(Kind::Termination, &[]);
    let bus1 = b.add(Kind::CanBus, &[]);
    let bp1 = b.add(Kind::PushButton, &[("mode", Value::Choice("latching".into()))]);
    b.join("gnd", &[(g, "gnd"), (v1, "neg"), (rv1, "a"), (c1, "p2"), (adc1, "ref"), (adc2, "ref"), (adc3, "ref"), (u1, "gnd")]);
    b.join("gnd", &[(u2, "vss"), (u2, "rs"), (s1, "p2"), (bus1, "gnd")]);
    b.join("v", &[(v1, "pos"), (rv1, "b"), (adc1, "in")]);
    b.join("w", &[(rv1, "w"), (c1, "p1"), (adc3, "in"), (bp1, "p1")]);
    b.join("p2", &[(v2, "pos"), (r2, "p1")]);
    b.join("n2", &[(v2, "neg"), (r2, "p2")]);
    b.join("5v", &[(u1, "v5"), (u2, "vdd")]);
    b.join("rx", &[(u2, "rxd"), (u1, "can_rx")]);
    b.join("tx", &[(u2, "txd"), (s1, "p1")]);
    b.join("h", &[(u2, "canh"), (rt1, "p1"), (bus1, "canh")]);
    b.join("l", &[(u2, "canl"), (rt1, "p2"), (bus1, "canl")]);
    b.interact(Interact::SetSwitch { comp: s1, closed: true });
    b.interact(Interact::SetPot { comp: rv1, position: 0.3 });
    b.build()
}

#[test]
fn writes_dto_fixtures() {
    let c = showcase();
    let ids = labels(&c);
    let id = |l: &str| ids[l];
    let mut core = core_after(c.clone(), 3);

    // Values, exploration and mission (the multimeter placed: its probes and display are in both).
    let rv1 = id("RV1");
    core.apply(&PhysCommand::Meter {
        com: Some(n7rt_circuit::PinRef { comp: rv1, pin: "a".into() }),
        vohm: Some(n7rt_circuit::PinRef { comp: rv1, pin: "w".into() }),
        function: n7rt_lab::phys::MeterFn::VDc,
    })
    .expect("meter placed");
    let values = core.values(7, Disclosure::Exploration, stats());
    assert!(values.diagnostics.len() >= 3 && values.nets.iter().any(|n| n.voltage == Reading::NoReference));
    write("values", &values);
    write("values_mission", &core.values(8, Disclosure::Mission, stats()));

    // Readings of every variant, by quantity.
    let f = core.frame();
    let p = |l: &str, q: &str| pin(id(l), q);
    let volts: Vec<Reading<f64>> = vec![
        f.voltage_at(&p("V1", "pos")),
        f.voltage_at(&p("ADC2", "in")),
        f.voltage_at(&p("R1", "p1")),
        f.voltage_at(&p("U2", "canh")),
        f.voltage_at(&pin(CompId(999), "p1")),
        f.unpowered_resistance(&p("U2", "canh"), &p("V1", "pos")),
        f.unpowered_resistance(&p("RT1", "p1"), &p("RT1", "p2")),
        values.comps.iter().find(|c| c.id == id("ADC1")).and_then(|c| c.measures.iter().find(|m| m.name == "code")).expect("code").reading.clone(),
    ];
    let kinds: Vec<&str> = ["valid", "floating", "no_reference", "unavailable", "open", "saturated"].to_vec();
    let json = serde_json::to_string(&volts).expect("json");
    for k in kinds {
        assert!(json.contains(&format!("\"k\":\"{k}\"")), "{k} in {json}");
    }
    write("readings_f64", &volts);
    let not_sampled = {
        // ADC3 at 100 Hz, RESET while running: no sample before frame 10.
        let mut core = PhysCore::new(c.clone());
        set_param(&mut core, id("ADC3"), "sample_rate", Value::Int(100));
        run(&mut core);
        steps(&mut core, 2);
        core.apply(&PhysCommand::Reset).expect("reset");
        core.step();
        core.frame().adc(id("ADC3"), "in")
    };
    let samples = vec![f.adc(id("ADC3"), "in"), f.adc(id("ADC1"), "in"), f.adc(id("ADC2"), "in"), f.adc(id("U1"), "a0"), not_sampled];
    assert!(matches!(samples[0], Reading::Valid { .. }) && matches!(samples[1], Reading::Saturated { .. }));
    assert_eq!(samples[4], Reading::Unavailable { reason: "not_sampled_yet".into() });
    write("readings_adc", &samples);
    write("readings_logic", &vec![f.logic(&p("U2", "txd")), f.logic(&p("U1", "can_rx")), f.logic(&p("U1", "can_tx")), f.logic(&p("R1", "p1"))]);
    let undefined = {
        // TXD at 1.43 V (10 kΩ to VSS against the 25 kΩ pull-up).
        let mut b = Bench::new();
        let v1 = b.add(Kind::DcSupply, &[]);
        let g = b.add(Kind::Ground, &[]);
        let u1 = b.add(Kind::CanTransceiver, &[]);
        let r1 = b.add(Kind::Resistor, &[]);
        b.join("vdd", &[(v1, "pos"), (u1, "vdd")]);
        b.join("gnd", &[(g, "gnd"), (v1, "neg"), (u1, "vss"), (u1, "rs"), (r1, "p2")]);
        b.join("tx", &[(u1, "txd"), (r1, "p1")]);
        let core = PhysCore::new(b.build());
        let r = core.frame().logic(&pin(u1, "txd"));
        assert_eq!(r, Reading::Undefined);
        r
    };
    write("readings_undefined", &vec![undefined]);
    let ops: Vec<_> = ["V1", "V2", "S1", "BP1", "RV1", "U1", "U2", "C1"].iter().map(|l| f.op_state(id(l))).collect();
    write("op_states", &ops);

    // Commands and their replies.
    stop(&mut core);
    let rev = core.document().rev();
    let add = Edit::AddComponent { kind: Kind::Resistor, pos: Pt { x: 0, y: 200 }, rot: Rot::R90, params: ValueMap::new(), label: None };
    write("preview", &core.preview(rev, &add).expect("preview"));
    let added = core.apply(&PhysCommand::Edit { base_rev: rev, edit: add }).expect("edit");
    assert!(matches!(&added, CommandReply::Edit(r) if r.nets.is_some()));
    write("command_edit", &added);
    let new_r = match &added {
        CommandReply::Edit(r) => r.patch.components[0].0,
        _ => unreachable!(),
    };
    let moved = core
        .apply(&PhysCommand::Edit { base_rev: core.document().rev(), edit: Edit::Move { components: vec![new_r], wires: vec![], delta: Pt { x: 4, y: 0 } } })
        .expect("move");
    assert!(matches!(&moved, CommandReply::Edit(r) if r.nets.is_none()));
    write("command_edit_geometry", &moved);
    write("command_interact", &core.apply(&PhysCommand::Interact { interact: Interact::SetPot { comp: id("RV1"), position: 0.7 } }).expect("pot"));
    write("command_run", &core.apply(&PhysCommand::Run).expect("run"));
    for cmd in [
        PhysCommand::Undo,
        PhysCommand::Redo,
        PhysCommand::Interact { interact: Interact::SetSwitch { comp: id("S1"), closed: false } },
        PhysCommand::Interact { interact: Interact::SetPower { comp: id("V2"), on: false } },
        PhysCommand::Interact { interact: Interact::SetButton { comp: id("BP1"), pressed: true } },
        PhysCommand::Reset,
        PhysCommand::Stop,
    ] {
        core.apply(&cmd).expect("command");
    }
    core.apply(&PhysCommand::Edit {
        base_rev: core.document().rev(),
        edit: Edit::SetParam { comp: id("R1"), name: "resistance".into(), value: Value::Choice("2k2".into()) },
    })
    .expect("text parameter");
    write("journal", &core.journal());

    // The handle: library, snapshot, files, demonstrations, errors.
    let dir = temp_dir("lab");
    fs::create_dir_all(dir.join("circuits")).expect("circuits");
    fs::write(dir.join("circuits").join("tp1.json"), persist::save(&tp1_capteur_can())).expect("tp1");
    fs::write(dir.join("circuits").join("broken.json"), "{\"format\": \"autre\", \"version\": 1}\n").expect("broken");
    let lab = PhysLab::start(PhysConfig { data_dir: Some(dir.clone()) });
    write("library", &lab.library());
    let snapshot = lab.load("tp1").expect("tp1");
    assert!(!snapshot.states.is_empty() && !snapshot.nets.is_empty() && !snapshot.wires.is_empty());
    write("snapshot", &snapshot);
    let mut files = lab.list().expect("list");
    for (i, e) in files.iter_mut().enumerate() {
        e.modified_unix_ms = 1_790_000_000_000 + i as u64;
    }
    write("files", &files);
    write("demos", &lab.demos());
    let stale = lab.preview(snapshot.rev + 5, &Edit::Delete { components: vec![], wires: vec![] }).expect_err("stale");
    let errors: Vec<PhysError> = vec![
        stale,
        lab.save("a b", false).expect_err("bad name"),
        lab.save("tp1", false).expect_err("exists"),
        lab.load("absent").expect_err("not found"),
        lab.load("broken").expect_err("invalid"),
        PhysError::File { error: FileError::Io { message: "Accès refusé. (os error 5)".into() } },
        PhysLab::start(PhysConfig::default()).list().expect_err("no storage"),
        PhysError::EngineUnavailable,
    ];
    assert!(matches!(&errors[4], PhysError::File { error: FileError::Invalid { violations } } if !violations.is_empty()));
    write("errors", &errors);
    lab.shutdown();
    let _ = fs::remove_dir_all(&dir);
}
