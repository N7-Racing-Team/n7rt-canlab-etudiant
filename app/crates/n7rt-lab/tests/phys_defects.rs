//! Hidden wire defects of the mission engine (E7 of the vehicle specification) on synthetic circuits: an open or a
//! series resistance carried by a wire object, invisible in the document, following the object through delete,
//! undo and replacement, waiting for a wire when none joins the pins, and refused from the interface.

#[path = "phys_kit.rs"]
mod kit;

use kit::*;
use n7rt_circuit::{persist, Circuit, CompId, Edit, Endpoint, Kind, PinRef, RefusalCode, Value, WireId};
use n7rt_lab::phys::{replay, CircuitProbe, PhysCommand, PhysConfig, PhysCore, PhysError, PhysLab, SystemOp, WireDefect};

fn num(x: f64) -> Value {
    Value::Num(x)
}

/// V1 (5 V) → R1 (1 kΩ) → R2 (1 kΩ) → ground; with `third`, R3 (1 kΩ) from the midpoint to ground as well (the
/// midpoint net then holds a junction).
fn divider(third: bool) -> (Circuit, CompId, CompId, Option<CompId>) {
    let mut b = Bench::new();
    let v1 = b.add(Kind::DcSupply, &[]);
    let g = b.add(Kind::Ground, &[]);
    let r1 = b.add(Kind::Resistor, &[("resistance", num(1e3))]);
    let r2 = b.add(Kind::Resistor, &[("resistance", num(1e3))]);
    let r3 = third.then(|| b.add(Kind::Resistor, &[("resistance", num(1e3))]));
    b.join("p", &[(v1, "pos"), (r1, "p1")]);
    b.join("m", &[(r1, "p2"), (r2, "p1")]);
    let mut gnd = vec![(g, "gnd"), (v1, "neg"), (r2, "p2")];
    if let Some(r3) = r3 {
        b.join("m", &[(r3, "p1")]);
        gnd.push((r3, "p2"));
    }
    b.join("gnd", &gnd);
    (b.build(), r1, r2, r3)
}

fn wire_between(c: &Circuit, a: &PinRef, b: &PinRef) -> Option<WireId> {
    c.wires().values().find(|w| (&w.a == a && &w.b == b) || (&w.a == b && &w.b == a)).map(|w| w.id)
}

fn attach(core: &mut PhysCore, a: &PinRef, b: &PinRef, defect: WireDefect) {
    core.apply(&PhysCommand::System { op: SystemOp::AttachDefect { a: a.clone(), b: b.clone(), defect } }).expect("defect attached");
}

fn edit(core: &mut PhysCore, e: Edit) {
    let rev = core.document().rev();
    core.apply(&PhysCommand::Edit { base_rev: rev, edit: e }).expect("edit");
}

fn vm(core: &PhysCore, r2: CompId) -> f64 {
    valid(core.frame().voltage_at(&pin(r2, "p1")))
}

#[test]
fn e7_open_breaks_conduction_and_stays_invisible() {
    let (c, r1, r2, _) = divider(false);
    let mut core = PhysCore::new(c);
    assert_close("healthy midpoint", vm(&core, r2), 2.5, 1e-3);
    let text = persist::save(core.document().circuit());
    let nets = core.document().circuit().netlist();
    let digest = core.digest();
    attach(&mut core, &pin(r1, "p2"), &pin(r2, "p1"), WireDefect::Open);
    assert!(vm(&core, r2).abs() < 1e-9, "R2 side of the open wire at ground potential");
    assert_close("R1 side at the supply", valid(core.frame().voltage_at(&pin(r1, "p2"))), 5.0, 1e-3);
    assert_eq!(persist::save(core.document().circuit()), text, "document unchanged");
    assert_eq!(core.document().circuit().netlist(), nets, "nets of a healthy wire");
    assert_ne!(core.digest(), digest, "the defect is part of the core state");
    let (carriers, pending) = core.defects();
    assert_eq!(carriers.len(), 1);
    assert!(!pending);
    core.apply(&PhysCommand::System { op: SystemOp::ClearDefects }).unwrap();
    assert_close("cleared", vm(&core, r2), 2.5, 1e-3);
    assert!(core.defects().0.is_empty());
}

#[test]
fn e7_series_adds_its_exact_resistance() {
    let (c, r1, r2, _) = divider(false);
    let mut core = PhysCore::new(c);
    attach(&mut core, &pin(r1, "p2"), &pin(r2, "p1"), WireDefect::Series { ohm: 68.0 });
    let i = 5.0 / (1e3 + 68.0 + 1e3 + 0.01);
    assert_close("R2 top", vm(&core, r2), i * 1e3, 1e-6);
    assert_close("drop across the wire", valid(core.frame().voltage_between_pins(&pin(r1, "p2"), &pin(r2, "p1"))), i * 68.0, 1e-6);
    for bad in [WireDefect::Series { ohm: 0.0 }, WireDefect::Series { ohm: f64::NAN }, WireDefect::Series { ohm: -1.0 }] {
        let op = SystemOp::AttachDefect { a: pin(r1, "p2"), b: pin(r2, "p1"), defect: bad };
        assert!(core.apply(&PhysCommand::System { op }).is_err(), "{bad:?}");
    }
    let op = SystemOp::AttachDefect { a: pin(r1, "p2"), b: pin(r2, "zz"), defect: WireDefect::Open };
    assert!(core.apply(&PhysCommand::System { op }).is_err(), "unknown pin");
}

#[test]
fn e7_follows_the_wire_object_through_delete_undo_and_replacement() {
    let (c, r1, r2, _) = divider(false);
    let (a, b) = (pin(r1, "p2"), pin(r2, "p1"));
    let original = wire_between(&c, &a, &b).expect("direct wire");
    let mut core = PhysCore::new(c);
    attach(&mut core, &a, &b, WireDefect::Open);
    // delete then undo: the same object comes back, and its defect with it
    edit(&mut core, Edit::Delete { components: vec![], wires: vec![original] });
    core.apply(&PhysCommand::Undo).unwrap();
    assert!(core.document().circuit().wires().contains_key(&original));
    assert!(vm(&core, r2).abs() < 1e-9, "undo is not a repair");
    // the repair: a new wire object between the same pins conducts
    edit(&mut core, Edit::Delete { components: vec![], wires: vec![original] });
    edit(&mut core, Edit::AddWire { a: Endpoint::Pin { comp: r1, pin: "p2".into() }, b: Endpoint::Pin { comp: r2, pin: "p1".into() }, bends: None });
    let new = wire_between(core.document().circuit(), &a, &b).expect("replacement wire");
    assert_ne!(new, original);
    assert_close("replaced", vm(&core, r2), 2.5, 1e-3);
    let (carriers, pending) = core.defects();
    assert_eq!(carriers.into_iter().collect::<Vec<_>>(), [original], "the inert defect stays on the deleted object");
    assert!(!pending);
}

#[test]
fn e7_waits_for_the_first_wire_created_between_the_pins() {
    let (c, r1, r2, _) = divider(false);
    let (a, b) = (pin(r1, "p2"), pin(r2, "p1"));
    let original = wire_between(&c, &a, &b).unwrap();
    let mut core = PhysCore::new(c);
    edit(&mut core, Edit::Delete { components: vec![], wires: vec![original] });
    attach(&mut core, &a, &b, WireDefect::Open);
    assert_eq!(core.defects(), (Default::default(), true), "no wire: pending");
    edit(&mut core, Edit::AddWire { a: Endpoint::Pin { comp: r2, pin: "p1".into() }, b: Endpoint::Pin { comp: r1, pin: "p2".into() }, bends: None });
    let new = wire_between(core.document().circuit(), &a, &b).unwrap();
    assert_eq!(core.defects(), ([new].into_iter().collect(), false), "attached to the first wire created");
    assert!(vm(&core, r2).abs() < 1e-9);
}

#[test]
fn e7_through_junctions_goes_on_the_wire_that_ends_on_the_ecu_side() {
    let (c, r1, r2, r3) = divider(true);
    let r3 = r3.unwrap();
    let (a, b) = (pin(r1, "p2"), pin(r2, "p1"));
    assert!(wire_between(&c, &a, &b).is_none(), "the midpoint net goes through a junction");
    let mut core = PhysCore::new(c);
    assert_close("three-resistor midpoint", vm(&core, r2), 5.0 * 500.0 / 1500.0, 1e-3);
    attach(&mut core, &a, &b, WireDefect::Open);
    let carrier = *core.defects().0.iter().next().expect("carrier");
    let w = &core.document().circuit().wires()[&carrier];
    assert!(w.a == b || w.b == b, "the chain wire incident to the ECU-side pin");
    assert!(vm(&core, r2).abs() < 1e-9, "R2 cut off");
    assert_close("R1 and R3 still joined", valid(core.frame().voltage_at(&pin(r3, "p1"))), 2.5, 1e-3);
}

#[test]
fn system_commands_journal_replay_to_the_same_digest() {
    let (c, r1, r2, _) = divider(true);
    let initial = c.clone();
    let mut core = PhysCore::new(c);
    core.apply(&PhysCommand::Run).unwrap();
    for _ in 0..5 {
        core.step();
    }
    attach(&mut core, &pin(r1, "p2"), &pin(r2, "p1"), WireDefect::Series { ohm: 120.0 });
    for _ in 0..5 {
        core.step();
    }
    core.apply(&PhysCommand::System { op: SystemOp::ClearDefects }).unwrap();
    attach(&mut core, &pin(r1, "p2"), &pin(r2, "p1"), WireDefect::Open);
    for _ in 0..5 {
        core.step();
    }
    let replayed = replay(&initial, core.journal(), core.frame_index());
    assert_eq!(replayed.digest(), core.digest());
    let line = serde_json::to_string(&core.journal().last().unwrap().command).unwrap();
    let back: PhysCommand = serde_json::from_str(&line).unwrap();
    assert_eq!(&back, &core.journal().last().unwrap().command);
}

#[test]
fn system_commands_are_refused_from_the_interface() {
    let lab = PhysLab::start(PhysConfig::default());
    let refused = lab.command(PhysCommand::System { op: SystemOp::ClearDefects });
    match refused {
        Err(PhysError::Refused { refusal }) => {
            assert_eq!(refusal.code, RefusalCode::InternalOnly);
            assert_eq!(refusal.message_fr, "Commande réservée au moteur de mission.");
        }
        other => panic!("refusal expected, got {other:?}"),
    }
    assert!(lab.command_internal(PhysCommand::System { op: SystemOp::ClearDefects }).is_ok());
    assert_eq!(lab.defects(), Some((Default::default(), false)));
}

#[test]
fn system_setting_is_applied_outside_the_undo_history() {
    let (c, r1, r2, _) = divider(false);
    let initial = c.clone();
    let mut core = PhysCore::new(c);
    // an ordinary edit first: it must not be able to revert the system setting afterwards
    edit(&mut core, Edit::SetParam { comp: r1, name: "resistance".into(), value: num(2e3) });
    assert!(core.document().can_undo());
    let op = SystemOp::SetParam { comp: r1, name: "resistance".into(), value: num(3e3) };
    core.apply(&PhysCommand::System { op }).expect("system setting");
    assert_eq!(core.document().circuit().components()[&r1].params["resistance"], num(3e3));
    assert!(!core.document().can_undo() && !core.document().can_redo(), "history cleared");
    assert!(core.apply(&PhysCommand::Undo).is_err());
    assert_close("new divider", vm(&core, r2), 5.0 * 1e3 / (4e3 + 0.01), 1e-6);
    // same validation as an edit
    let bad = SystemOp::SetParam { comp: r1, name: "resistance".into(), value: num(-5.0) };
    assert!(core.apply(&PhysCommand::System { op: bad }).is_err());
    let unknown = SystemOp::SetParam { comp: r1, name: "nope".into(), value: num(1.0) };
    assert!(core.apply(&PhysCommand::System { op: unknown }).is_err());
    // the student sets it back by an ordinary edit
    edit(&mut core, Edit::SetParam { comp: r1, name: "resistance".into(), value: num(1e3) });
    assert_close("set back", vm(&core, r2), 2.5, 1e-3);
    assert_eq!(replay(&initial, core.journal(), core.frame_index()).digest(), core.digest());
}

// ------------------------------------------------------------------------------------------------ multimeter

use n7rt_circuit::Interact;
use n7rt_lab::phys::{Disclosure, EngineStats, MeterFn};

fn meter(core: &mut PhysCore, com: PinRef, vohm: PinRef, function: MeterFn) {
    core.apply(&PhysCommand::Meter { com: Some(com), vohm: Some(vohm), function }).expect("meter placed");
}

fn display(core: &PhysCore) -> String {
    core.values(1, Disclosure::Mission, EngineStats::default()).meter.display
}

#[test]
fn meter_reads_volts_in_mission_mode_averaged_over_100_frames() {
    let (c, r1, r2, _) = divider(false);
    let mut core = PhysCore::new(c);
    assert_eq!(display(&core), "—", "no probe placed");
    meter(&mut core, pin(r2, "p2"), pin(r2, "p1"), MeterFn::VDc);
    assert_eq!(display(&core), "2.500 V", "STOP preview");
    let values = core.values(1, Disclosure::Mission, EngineStats::default());
    assert!(values.nets.is_empty() && values.comps.iter().all(|c| c.pins.is_empty()), "nets and pins stay hidden");
    core.apply(&PhysCommand::Run).unwrap();
    for _ in 0..50 {
        core.step();
    }
    // the drop changes half-way through the window: the display is the mean
    attach(&mut core, &pin(r1, "p2"), &pin(r2, "p1"), WireDefect::Open);
    for _ in 0..50 {
        core.step();
    }
    let shown: f64 = display(&core).trim_end_matches(" V").parse().unwrap();
    assert!((shown - 1.25).abs() < 0.03, "mean of 2.5 V and 0 V halves: {shown}");
    for _ in 0..100 {
        core.step();
    }
    assert_eq!(display(&core), "0.000 V");
    assert!(core.apply(&PhysCommand::Meter { com: Some(pin(r2, "p2")), vohm: Some(pin(r2, "zz")), function: MeterFn::VDc }).is_err());
}

#[test]
fn meter_reads_ohms_only_on_a_switched_off_bench_and_sees_hidden_defects() {
    let (c, r1, r2, _) = divider(false);
    let v1 = c.components().values().find(|x| x.kind == Kind::DcSupply).unwrap().id;
    let mut core = PhysCore::new(c);
    meter(&mut core, pin(r1, "p2"), pin(r2, "p1"), MeterFn::Ohm);
    assert_eq!(display(&core), "—", "live bench");
    core.apply(&PhysCommand::Interact { interact: Interact::SetPower { comp: v1, on: false } }).unwrap();
    assert_eq!(display(&core), "0.0 Ω", "healthy wire");
    attach(&mut core, &pin(r1, "p2"), &pin(r2, "p1"), WireDefect::Series { ohm: 68.0 });
    assert_eq!(display(&core), "68.0 Ω");
    core.apply(&PhysCommand::System { op: SystemOp::ClearDefects }).unwrap();
    attach(&mut core, &pin(r1, "p2"), &pin(r2, "p1"), WireDefect::Open);
    assert_eq!(display(&core), "OL");
    meter(&mut core, pin(r2, "p2"), pin(r2, "p1"), MeterFn::Ohm);
    assert_eq!(display(&core), "1.000 kΩ");
}

#[test]
fn e7_a_junction_merge_keeps_the_defect_in_the_surviving_wire_and_undo_takes_the_copy_back() {
    let (c, r1, r2, r3) = divider(true);
    let r3 = r3.unwrap();
    // the merge keeps the smaller wire id: the defect goes on the chain wire with the larger one
    let chain = |p: &PinRef| c.wires().values().find(|w| &w.a == p || &w.b == p).map(|w| w.id).unwrap();
    let (a, b) = if chain(&pin(r2, "p1")) > chain(&pin(r1, "p2")) { (pin(r1, "p2"), pin(r2, "p1")) } else { (pin(r2, "p1"), pin(r1, "p2")) };
    let branch = chain(&pin(r3, "p1"));
    let mut core = PhysCore::new(c);
    attach(&mut core, &a, &b, WireDefect::Open);
    let carrier = *core.defects().0.iter().next().unwrap();
    // deleting the third branch leaves the junction with two wires: the cleanup merges them into one
    edit(&mut core, Edit::Delete { components: vec![], wires: vec![branch] });
    let survivor = wire_between(core.document().circuit(), &a, &b).expect("one wire from R1 to R2");
    assert!(!core.document().circuit().wires().contains_key(&carrier), "the carrier was merged into the other wire");
    assert!(vm(&core, r2).abs() < 1e-9, "the faulty segment is inside the merged wire: no repair");
    assert!(core.defects().0.contains(&survivor), "P-3 still sees a defective wire");
    let digest = core.digest();
    // undo: the carrier and the junction come back, the copy goes
    core.apply(&PhysCommand::Undo).unwrap();
    assert!(core.document().circuit().wires().contains_key(&carrier));
    assert!(vm(&core, r2).abs() < 1e-9, "the carrier is back with its defect");
    assert_close("the other wire conducts again", valid(core.frame().voltage_at(&pin(r3, "p1"))), 2.5, 1e-3);
    assert_eq!(core.defects().0.into_iter().collect::<Vec<_>>(), [carrier]);
    // redo merges again
    core.apply(&PhysCommand::Redo).unwrap();
    assert_eq!(core.digest(), digest, "redo: same state as the merge");
}

#[test]
fn e7_the_form_of_a_pending_defect_is_part_of_the_digest() {
    let (c, r1, r2, _) = divider(false);
    let (a, b) = (pin(r1, "p2"), pin(r2, "p1"));
    let original = wire_between(&c, &a, &b).unwrap();
    let digests: Vec<u64> = [WireDefect::Open, WireDefect::Series { ohm: 68.0 }]
        .into_iter()
        .map(|d| {
            let mut core = PhysCore::new(c.clone());
            edit(&mut core, Edit::Delete { components: vec![], wires: vec![original] });
            attach(&mut core, &a, &b, d);
            assert!(core.defects().1, "pending");
            core.digest()
        })
        .collect();
    assert_ne!(digests[0], digests[1]);
}

#[test]
fn a_defect_on_a_mission_bench_leaves_no_trace_in_snapshot_journal_or_values() {
    let (c, r1, r2, _) = divider(false);
    let lab = PhysLab::start(PhysConfig { data_dir: None });
    lab.load_circuit_json("banc", &persist::save(&c), Disclosure::Mission).expect("mission bench");
    let before = serde_json::to_string(&lab.snapshot()).unwrap();
    let values_before = lab.values(0).expect("values");
    for defect in [WireDefect::Open, WireDefect::Series { ohm: 68.0 }] {
        lab.command_internal(PhysCommand::System { op: SystemOp::ClearDefects }).unwrap();
        lab.command_internal(PhysCommand::System { op: SystemOp::AttachDefect { a: pin(r1, "p2"), b: pin(r2, "p1"), defect } })
            .expect("attached");
        let after = serde_json::to_string(&lab.snapshot()).unwrap();
        assert_eq!(after, before, "{defect:?}: the snapshot is that of a healthy bench");
        for word in ["defect", "open", "series", "ohm"] {
            assert!(!after.to_lowercase().contains(word), "{defect:?}: « {word} » in the snapshot");
        }
        assert!(lab.journal_tail(usize::MAX).is_empty(), "{defect:?}: the mission bench's journal is never shown");
        let v = lab.values(0).expect("values");
        assert!(v.nets.is_empty() && v.diagnostics.is_empty(), "{defect:?}: no net voltage, no diagnostic in mission mode");
        assert_eq!(v.comps.iter().map(|x| (x.id, x.state.clone())).collect::<Vec<_>>(),
                   values_before.comps.iter().map(|x| (x.id, x.state.clone())).collect::<Vec<_>>(), "{defect:?}: same states");
        assert!(!serde_json::to_string(&v).unwrap().to_lowercase().contains("defect"));
    }
    // the interface path is refused, whatever it asks
    let refused = lab.command(PhysCommand::System { op: SystemOp::ClearDefects }).unwrap_err();
    assert!(!format!("{refused:?}").to_lowercase().contains("defect"), "the refusal names no defect: {refused:?}");
    lab.shutdown();
}
