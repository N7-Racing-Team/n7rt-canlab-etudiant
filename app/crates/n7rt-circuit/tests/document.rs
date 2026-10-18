//! Document semantics (contract §3.1–§3.3): revisions, preview, stale revisions, undo/redo with exact patches and
//! the history cap, interactions and their state rules.

mod common;

use common::*;
use n7rt_circuit::{
    persist, CompId, Document, Edit, Interact, ItemRef, Kind, Refusal, RefusalCode, Rot, Value, UNDO_CAP,
};

fn interact_err(d: &mut Document, i: Interact) -> Refusal {
    let before = d.circuit().clone();
    let rev = d.rev();
    let r = d.interact(&i).expect_err("interaction must be refused");
    assert_eq!((d.circuit(), d.rev()), (&before, rev), "a refused interaction changed something");
    r
}

fn state(d: &Document, id: CompId, name: &str) -> Value {
    d.circuit().state(id).expect("component exists")[name].clone()
}

#[test]
fn revisions_follow_edits_undo_redo_but_not_interactions() {
    let mut d = doc();
    assert_eq!((d.rev(), d.can_undo(), d.can_redo()), (0, false, false));
    let rv = add(&mut d, Kind::Potentiometer, 0, 0, Rot::R0);
    assert_eq!(d.rev(), 1);
    let s = d.interact(&Interact::SetPot { comp: rv, position: 0.3 }).unwrap();
    assert_eq!((s.rev, d.rev()), (1, 1));
    // A queued edit prepared before the interaction is still current.
    let a = d.edit(1, &Edit::Move { components: vec![rv], wires: vec![], delta: pt(5, 0) }).unwrap();
    assert_eq!(a.rev, 2);
    assert_eq!(d.undo().unwrap().rev, 3);
    assert_eq!(d.redo().unwrap().rev, 4);
    // Refusals and empty patches leave the revision alone.
    refused_code(&mut d, Edit::Delete { components: vec![CompId(99)], wires: vec![] }, RefusalCode::UnknownComponent);
    edit(&mut d, Edit::Move { components: vec![rv], wires: vec![], delta: pt(0, 0) });
    assert_eq!(d.rev(), 4);
}

#[test]
fn stale_revision_is_refused_for_preview_and_edit() {
    let mut d = doc();
    add(&mut d, Kind::Resistor, 0, 0, Rot::R0);
    let e = Edit::AddComponent { kind: Kind::Resistor, pos: pt(0, 10), rot: Rot::R0, params: Default::default(), label: None };
    let r = d.preview(0, &e).unwrap_err();
    assert_eq!(r.code, RefusalCode::StaleRevision);
    assert_eq!(r.message_fr, "Le sch\u{e9}ma a chang\u{e9} entre-temps (r\u{e9}vision 0, actuelle 1) : rechargement");
    let before = d.circuit().clone();
    assert_eq!(d.edit(5, &e).unwrap_err().code, RefusalCode::StaleRevision);
    assert_eq!(d.circuit(), &before);
}

#[test]
fn preview_is_the_same_function_and_consumes_no_id() {
    let (mut d, w1) = {
        let mut d = doc();
        let r1 = add(&mut d, Kind::Resistor, 0, 0, Rot::R0);
        let r2 = add(&mut d, Kind::Resistor, 12, 0, Rot::R0);
        let w = wire(&mut d, pin(r1, "p2"), pin(r2, "p1"), None);
        (d, w)
    };
    let r3 = add(&mut d, Kind::Resistor, 6, 8, Rot::R90);
    let e = Edit::AddWire { a: on_wire(w1, 6, 0), b: pin(r3, "p1"), bends: None };
    let before = d.circuit().clone();
    let rev = d.rev();
    let p1 = d.preview(rev, &e).unwrap();
    let p2 = d.preview(rev, &e).unwrap();
    // Nothing moved, no id consumed: two previews are identical and the commit gives the same patch.
    assert_eq!(p1, p2);
    assert_eq!((d.circuit(), d.rev()), (&before, rev));
    let a = d.edit(rev, &e).unwrap();
    assert_eq!(a.patch, p1);
    // A refused preview returns the refusal of the commit.
    let bad = Edit::AddWire { a: on_wire(w1, 5, 1), b: pin(r3, "p2"), bends: None };
    let rev = d.rev();
    assert_eq!(d.preview(rev, &bad).unwrap_err(), d.edit(rev, &bad).unwrap_err());
}

#[test]
fn undo_redo_apply_exact_patches_and_keep_next_id() {
    let mut d = doc();
    let r1 = add(&mut d, Kind::Resistor, 0, 0, Rot::R0);
    let r2 = add(&mut d, Kind::Resistor, 12, 0, Rot::R0);
    let w1 = wire(&mut d, pin(r1, "p2"), pin(r2, "p1"), None);
    let r3 = add(&mut d, Kind::Resistor, 6, 8, Rot::R90);
    let mut snapshots = vec![d.circuit().clone()];
    let edits = vec![
        Edit::AddWire { a: on_wire(w1, 6, 0), b: pin(r3, "p1"), bends: None },
        Edit::Move { components: vec![r2], wires: vec![], delta: pt(0, 3) },
        Edit::SetParam { comp: r1, name: "resistance".into(), value: Value::Choice("4k7".into()) },
        Edit::Rotate { components: vec![r3], wires: vec![] },
        Edit::Delete { components: vec![r3], wires: vec![] },
    ];
    for e in &edits {
        let before = d.circuit().clone();
        let a = edit(&mut d, e.clone());
        assert!(!a.patch.is_empty());
        for (id, b, _) in &a.patch.components {
            assert_eq!(b.as_ref(), before.components().get(id));
        }
        assert_valid(&d);
        snapshots.push(d.circuit().clone());
    }
    let next_id = d.circuit().next_id();
    // Undo walks back through every snapshot, components and wires exactly; next_id is never restored.
    for i in (0..edits.len()).rev() {
        let a = d.undo().unwrap();
        assert_eq!(d.circuit().components(), snapshots[i].components(), "undo {i}");
        assert_eq!(d.circuit().wires(), snapshots[i].wires(), "undo {i}");
        assert_eq!(d.circuit().next_id(), next_id);
        assert!(a.patch.components.windows(2).all(|p| p[0].0 < p[1].0));
        assert_valid(&d);
    }
    // The four set-up edits (R1, R2, W1, R3) come back off too, down to the empty circuit.
    for _ in 0..4 {
        d.undo().unwrap();
    }
    assert!(d.circuit().components().is_empty() && d.circuit().wires().is_empty());
    assert_eq!(d.undo().unwrap_err().code, RefusalCode::NothingToUndo);
    assert_eq!(d.undo().unwrap_err().message_fr, "Rien \u{e0} annuler");
    for _ in 0..4 {
        d.redo().unwrap();
    }
    assert_eq!(d.circuit().components(), snapshots[0].components());
    for (i, snap) in snapshots.iter().enumerate().skip(1) {
        d.redo().unwrap();
        assert_eq!(d.circuit().components(), snap.components(), "redo {i}");
        assert_eq!(d.circuit().wires(), snap.wires(), "redo {i}");
    }
    let r = d.redo().unwrap_err();
    assert_eq!((r.code, r.message_fr.as_str()), (RefusalCode::NothingToRedo, "Rien \u{e0} r\u{e9}tablir"));
}

#[test]
fn an_edit_clears_redo_an_interaction_does_not() {
    let mut d = doc();
    let s = add(&mut d, Kind::Switch, 0, 0, Rot::R0);
    edit(&mut d, Edit::Move { components: vec![s], wires: vec![], delta: pt(1, 0) });
    d.undo().unwrap();
    assert!(d.can_redo());
    d.interact(&Interact::SetSwitch { comp: s, closed: true }).unwrap();
    assert!(d.can_redo());
    // Undo and redo leave interaction states alone.
    d.redo().unwrap();
    assert_eq!(state(&d, s, "closed"), Value::Bool(true));
    d.undo().unwrap();
    assert_eq!(state(&d, s, "closed"), Value::Bool(true));
    edit(&mut d, Edit::SetLabel { comp: s, label: "Smain".into() });
    assert!(!d.can_redo());
    // An empty patch is not an edit: redo survives it.
    d.undo().unwrap();
    edit(&mut d, Edit::SetLabel { comp: s, label: "S1".into() });
    assert!(d.can_redo());
}

#[test]
fn history_keeps_the_last_undo_cap_entries() {
    let mut d = doc();
    let r = add(&mut d, Kind::Resistor, 0, 0, Rot::R0);
    for i in 0..UNDO_CAP + 5 {
        let x = if i % 2 == 0 { 1 } else { -1 };
        edit(&mut d, Edit::Move { components: vec![r], wires: vec![], delta: pt(x, 0) });
    }
    let mut undone = 0;
    while d.undo().is_ok() {
        undone += 1;
    }
    assert_eq!(undone, UNDO_CAP);
    // The add itself and the first moves were dropped: the resistor is still there.
    assert!(d.circuit().components().contains_key(&r));
    let mut redone = 0;
    while d.redo().is_ok() {
        redone += 1;
    }
    assert_eq!(redone, UNDO_CAP);
    assert_eq!(d.circuit().components()[&r].pos, pt(if (UNDO_CAP + 5) % 2 == 0 { 0 } else { 1 }, 0));
}

#[test]
fn a_component_recreated_by_undo_gets_its_last_state() {
    let mut d = doc();
    let rv = add(&mut d, Kind::Potentiometer, 0, 0, Rot::R0);
    d.interact(&Interact::SetPot { comp: rv, position: 0.7 }).unwrap();
    edit(&mut d, Edit::Delete { components: vec![rv], wires: vec![] });
    assert!(d.circuit().state(rv).is_none());
    assert_eq!(interact_err(&mut d, Interact::SetPot { comp: rv, position: 0.2 }).code, RefusalCode::UnknownComponent);
    d.undo().unwrap();
    assert_eq!(state(&d, rv, "position"), Value::Num(0.7));
    // Only the states of present components are saved.
    let text = persist::save(d.circuit());
    assert!(text.contains("\"position\": 0.7"));
}

#[test]
fn pot_position_is_quantized_and_range_checked() {
    let mut d = doc();
    let rv = add(&mut d, Kind::Potentiometer, 0, 0, Rot::R0);
    assert_eq!(state(&d, rv, "position"), Value::Num(0.5));
    for (given, stored) in [(0.3, 0.3), (0.12345, 0.123), (0.0006, 0.001), (0.0004, 0.0), (1.0, 1.0), (0.9999, 1.0)] {
        let s = d.interact(&Interact::SetPot { comp: rv, position: given }).unwrap();
        // α = k/1000 computed as k as f64 / 1000.0 (contract §2.1).
        assert_eq!(s.state["position"], Value::Num(stored), "{given}");
        assert_eq!(state(&d, rv, "position"), Value::Num(stored));
    }
    for bad in [-0.001, 1.001, f64::NAN, f64::INFINITY] {
        let r = interact_err(&mut d, Interact::SetPot { comp: rv, position: bad });
        assert_eq!(r.code, RefusalCode::StateOutOfRange, "{bad}");
        assert_eq!(r.items, vec![ItemRef::Component { id: rv }]);
    }
    let r = interact_err(&mut d, Interact::SetPot { comp: rv, position: 1.5 });
    assert_eq!(r.message_fr, "Position 1.5 hors plage [0 ; 1]");
}

#[test]
fn push_button_latches_on_each_rising_edge() {
    let mut d = doc();
    let bp = add(&mut d, Kind::PushButton, 0, 0, Rot::R0);
    let press = |d: &mut Document, p: bool| d.interact(&Interact::SetButton { comp: bp, pressed: p }).unwrap().state;
    assert_eq!((state(&d, bp, "pressed"), state(&d, bp, "latched")), (Value::Bool(false), Value::Bool(false)));
    // Edges: press toggles, holding or releasing does not.
    let seq = [(true, true), (true, true), (false, true), (false, true), (true, false), (false, false), (true, true)];
    for (i, (pressed, latched)) in seq.into_iter().enumerate() {
        let s = press(&mut d, pressed);
        assert_eq!((s["pressed"].clone(), s["latched"].clone()), (Value::Bool(pressed), Value::Bool(latched)), "step {i}");
    }
    // The same rule in latching mode; the mode decides only how the contact is computed (n7rt-lab).
    edit(&mut d, Edit::SetParam { comp: bp, name: "mode".into(), value: Value::Choice("latching".into()) });
    press(&mut d, false);
    assert_eq!(press(&mut d, true)["latched"], Value::Bool(false));
    // `pressed` is transient: never saved, false after a load.
    let loaded = persist::load(&persist::save(d.circuit())).unwrap();
    assert_eq!(loaded.state(bp).unwrap()["pressed"], Value::Bool(false));
    assert_eq!(loaded.state(bp).unwrap()["latched"], Value::Bool(false));
}

#[test]
fn switch_and_power_interactions() {
    let mut d = doc();
    let s = add(&mut d, Kind::Switch, 0, 0, Rot::R0);
    let v = add(&mut d, Kind::DcSupply, 20, 0, Rot::R0);
    let u = add(&mut d, Kind::EcuStm32, 50, 0, Rot::R0);
    assert_eq!(state(&d, s, "closed"), Value::Bool(false));
    assert_eq!(state(&d, v, "on"), Value::Bool(true));
    assert_eq!(state(&d, u, "usb"), Value::Bool(true));
    d.interact(&Interact::SetSwitch { comp: s, closed: true }).unwrap();
    assert_eq!(state(&d, s, "closed"), Value::Bool(true));
    let st = d.interact(&Interact::SetPower { comp: v, on: false }).unwrap();
    assert_eq!((st.comp, st.state.clone()), (v, [("on".to_string(), Value::Bool(false))].into()));
    d.interact(&Interact::SetPower { comp: u, on: false }).unwrap();
    assert_eq!(state(&d, u, "usb"), Value::Bool(false));
    // Same value again: accepted, nothing else changes.
    d.interact(&Interact::SetPower { comp: u, on: false }).unwrap();
    assert_eq!(d.rev(), 3);
}

#[test]
fn interactions_on_the_wrong_kind_are_refused() {
    let mut d = doc();
    let r = add(&mut d, Kind::Resistor, 0, 0, Rot::R0);
    let s = add(&mut d, Kind::Switch, 20, 0, Rot::R0);
    let g = add(&mut d, Kind::Ground, 40, 0, Rot::R0);
    let cases = [
        (Interact::SetPot { comp: r, position: 0.5 }, "R1 (R\u{e9}sistance) n'a pas de curseur"),
        (Interact::SetButton { comp: s, pressed: true }, "S1 (Interrupteur) n'est pas un bouton poussoir"),
        (Interact::SetSwitch { comp: r, closed: true }, "R1 (R\u{e9}sistance) n'est pas un interrupteur"),
        (Interact::SetPower { comp: g, on: true }, "la masse (Masse) n'a pas d'alimentation \u{e0} commuter"),
    ];
    for (i, msg) in cases {
        let refusal = interact_err(&mut d, i);
        assert_eq!((refusal.code, refusal.message_fr.as_str()), (RefusalCode::NotInteractive, msg));
    }
    let refusal = interact_err(&mut d, Interact::SetSwitch { comp: CompId(77), closed: true });
    assert_eq!((refusal.code, refusal.message_fr.as_str()), (RefusalCode::UnknownComponent, "Composant 77 inexistant"));
}
