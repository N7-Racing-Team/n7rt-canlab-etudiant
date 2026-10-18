use super::*;

fn edit(document: &mut Document, edit: Edit) {
    document.edit(document.rev(), &edit).expect("valid edit");
}

fn add(document: &mut Document, kind: Kind) -> CompId {
    let applied = document.edit(document.rev(), &Edit::AddComponent {
        kind, pos: Pt { x: 0, y: 0 }, rot: Rot::R0, params: ValueMap::new(), label: None,
    }).expect("placed");
    applied.patch.components.iter().find(|(_, before, after)| before.is_none() && after.is_some()).unwrap().0
}

#[test]
fn history_eviction_bounds_deleted_states_without_losing_undo_redo_state() {
    let mut document = Document::new(Circuit::new("retention"));
    for _ in 0..UNDO_CAP * 4 {
        let id = add(&mut document, Kind::Potentiometer);
        document.interact(&Interact::SetPot { comp: id, position: 0.7 }).unwrap();
        edit(&mut document, Edit::Delete { components: vec![id], wires: vec![] });
        assert!(document.circuit.states.len() <= UNDO_CAP);
    }
    assert!(document.circuit.components.is_empty());
    assert_eq!(document.circuit.states.len(), UNDO_CAP / 2);
    for _ in 0..UNDO_CAP {
        document.undo().unwrap();
        for id in document.circuit.components.keys() {
            assert_eq!(document.circuit.state(*id).unwrap()["position"], Value::Num(0.7));
        }
    }
    assert!(!document.can_undo());
    for _ in 0..UNDO_CAP {
        document.redo().unwrap();
        for id in document.circuit.components.keys() {
            assert_eq!(document.circuit.state(*id).unwrap()["position"], Value::Num(0.7));
        }
    }
    assert!(!document.can_redo());
    assert_eq!(document.circuit.states.len(), UNDO_CAP / 2);
}

#[test]
fn invalidated_redo_releases_states_but_empty_edits_preserve_redo() {
    let mut document = Document::new(Circuit::new("retention"));
    let id = add(&mut document, Kind::Potentiometer);
    document.interact(&Interact::SetPot { comp: id, position: 0.8 }).unwrap();
    document.undo().unwrap();
    assert_eq!(document.circuit.states.len(), 1);
    edit(&mut document, Edit::Delete { components: vec![], wires: vec![] });
    assert!(document.can_redo());
    document.redo().unwrap();
    assert_eq!(document.circuit.state(id).unwrap()["position"], Value::Num(0.8));
    document.undo().unwrap();
    let live = add(&mut document, Kind::Resistor);
    assert!(!document.can_redo());
    assert_eq!(document.circuit.states.keys().copied().collect::<Vec<_>>(), vec![live]);
}

#[test]
fn fresh_document_keeps_only_live_component_states() {
    let mut document = Document::new(Circuit::new("retention"));
    let id = add(&mut document, Kind::Potentiometer);
    document.interact(&Interact::SetPot { comp: id, position: 0.6 }).unwrap();
    edit(&mut document, Edit::Delete { components: vec![id], wires: vec![] });
    assert_eq!(document.circuit.states.len(), 1); // undo can still restore it
    let fresh = Document::new(document.circuit.clone());
    assert!(fresh.circuit.states.is_empty());
    assert!(!fresh.can_undo());
    document.undo().unwrap();
    let fresh = Document::new(document.circuit.clone());
    assert_eq!(fresh.circuit.state(id).unwrap()["position"], Value::Num(0.6));
}
