use super::*;
use n7rt_circuit::{Pt, Rot, ValueMap};

fn add(core: &mut Core, kind: Kind) -> CompId {
    let reply = core.apply(&PhysCommand::Edit { base_rev: core.rev(), edit: Edit::AddComponent {
        kind, pos: Pt { x: core.document().circuit().components().len() as i32 * 30, y: 0 },
        rot: Rot::R0, params: ValueMap::new(), label: None,
    }}).expect("placed");
    let CommandReply::Edit(reply) = reply else { panic!("edit reply") };
    reply.patch.components.iter().find(|(_, before, after)| before.is_none() && after.is_some()).unwrap().0
}

fn delete(core: &mut Core, id: CompId) {
    core.apply(&PhysCommand::Edit { base_rev: core.rev(), edit: Edit::Delete { components: vec![id], wires: vec![] } }).unwrap();
}

fn pot(core: &mut Core, id: CompId, position: f64) {
    core.apply(&PhysCommand::Interact { interact: Interact::SetPot { comp: id, position } }).unwrap();
}

#[test]
fn pot_prior_is_frame_local_and_preserves_same_frame_round_trip_coalescing() {
    let mut core = Core::new(Circuit::new("retention"));
    let id = add(&mut core, Kind::Potentiometer);
    core.apply(&PhysCommand::Run).unwrap();
    let initial = core.journal.len();
    pot(&mut core, id, 0.6);
    pot(&mut core, id, 0.8);
    pot(&mut core, id, 0.5);
    assert_eq!(core.journal.len(), initial);
    assert!(core.pot_prior.is_empty());
    pot(&mut core, id, 0.7);
    assert_eq!(core.pot_prior.len(), 1);
    core.step();
    assert!(core.pot_prior.is_empty());
    pot(&mut core, id, 0.9);
    core.apply(&PhysCommand::Stop).unwrap();
    assert!(core.pot_prior.is_empty());
    core.apply(&PhysCommand::Run).unwrap();
    pot(&mut core, id, 0.8);
    core.apply(&PhysCommand::Reset).unwrap();
    assert!(core.pot_prior.is_empty());
}

#[test]
fn deleted_drag_and_button_ids_do_not_accumulate_across_topology_edits() {
    let mut core = Core::new(Circuit::new("retention"));
    for _ in 0..300 {
        let button = add(&mut core, Kind::PushButton);
        core.apply(&PhysCommand::Interact { interact: Interact::SetButton { comp: button, pressed: true } }).unwrap();
        assert!(core.unseen_presses.contains(&button));
        delete(&mut core, button);
        assert!(core.unseen_presses.is_empty());
    }
    core.apply(&PhysCommand::Run).unwrap();
    for _ in 0..300 {
        let id = add(&mut core, Kind::Potentiometer);
        pot(&mut core, id, 0.7);
        assert_eq!(core.pot_prior.len(), 1);
        delete(&mut core, id);
        assert!(core.pot_prior.is_empty());
    }
    let button = add(&mut core, Kind::PushButton);
    core.apply(&PhysCommand::Interact { interact: Interact::SetButton { comp: button, pressed: true } }).unwrap();
    core.apply(&PhysCommand::Interact { interact: Interact::SetButton { comp: button, pressed: false } }).unwrap();
    assert!(core.unseen_presses.contains(&button));
    assert!(core.deferred_releases.contains(&button));
    delete(&mut core, button);
    assert!(core.unseen_presses.is_empty());
    assert!(core.deferred_releases.is_empty());
}

#[test]
fn a_button_held_in_stop_then_released_at_run_start_is_seen_by_one_frame() {
    let mut core = Core::new(Circuit::new("retention"));
    let button = add(&mut core, Kind::PushButton);
    core.apply(&PhysCommand::Interact { interact: Interact::SetButton { comp: button, pressed: true } }).unwrap();
    core.apply(&PhysCommand::Run).unwrap();
    core.apply(&PhysCommand::Interact { interact: Interact::SetButton { comp: button, pressed: false } }).unwrap();
    assert_eq!(core.document().circuit().state(button).unwrap()["pressed"], Value::Bool(true));
    assert!(core.deferred_releases.contains(&button));
    core.step();
    assert_eq!(core.capture.ctx.circuit.state(button).unwrap()["pressed"], Value::Bool(true));
    assert_eq!(core.document().circuit().state(button).unwrap()["pressed"], Value::Bool(false));
    assert!(core.unseen_presses.is_empty());
    assert!(core.deferred_releases.is_empty());
}

#[test]
fn accepted_network_releases_vanished_supply_keys_while_failed_load_preserves_fallback() {
    let mut core = Core::new(Circuit::new("retention"));
    for _ in 0..300 {
        let supply = add(&mut core, Kind::DcSupply);
        assert_eq!(core.last_modes.len(), 1);
        delete(&mut core, supply);
        assert!(core.last_modes.is_empty());
    }
    let _anchor = add(&mut core, Kind::Resistor);
    let supply = add(&mut core, Kind::DcSupply);
    let key = super::super::lower::state_key(supply, "supply");
    core.last_modes.insert(key, SupplyMode::Cc);
    core.corrupt_next_load = true;
    // A bad network must keep the previous context and its fallback modes.
    let resistor = add(&mut core, Kind::Resistor);
    assert!(core.load_error);
    assert!(core.last_modes.contains_key(&key));
    delete(&mut core, resistor);
    assert!(!core.load_error);
    assert!(core.last_modes.contains_key(&key));
    delete(&mut core, supply);
    assert!(core.last_modes.is_empty());
}
