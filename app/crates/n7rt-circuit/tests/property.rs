//! Property test of contract §10 (lot M): 1000 seeded random sequences mixing every edit kind, interactions, undo
//! and redo. After every step the invariants hold (check() empty), every committed patch is the exact difference of
//! the tables, a preview equals its commit, refusals change nothing; undoing everything returns the initial circuit
//! (canonical text equal except next_id; interaction states are outside undo), redoing everything the final one.

mod common;

use std::collections::BTreeMap;

use common::*;
use n7rt_circuit::{persist, Circuit, Document, Edit, RefusalCode};

const SEQUENCES: u64 = 1000;
const STEPS: usize = 30;

fn edit_name(e: &Edit) -> &'static str {
    match e {
        Edit::AddComponent { .. } => "add_component",
        Edit::Delete { .. } => "delete",
        Edit::Move { .. } => "move",
        Edit::Rotate { .. } => "rotate",
        Edit::SetParam { .. } => "set_param",
        Edit::SetLabel { .. } => "set_label",
        Edit::AddWire { .. } => "add_wire",
        Edit::MoveWireEnd { .. } => "move_wire_end",
        Edit::SetBends { .. } => "set_bends",
    }
}

#[derive(Default)]
struct Stats {
    committed: BTreeMap<&'static str, usize>,
    refused: BTreeMap<String, usize>,
    interactions: usize,
    undos: usize,
    redos: usize,
    junctions_seen: usize,
}

fn check_ok(c: &Circuit, seed: u64, step: usize, what: &str) {
    let v = c.check();
    assert!(v.is_empty(), "seed {seed} step {step} after {what}: {v:#?}");
}

fn run_sequence(seed: u64, steps: usize, stats: &mut Stats) {
    let mut rng = Rng::new(seed);
    // Initial circuit: a random document whose history is dropped.
    let initial = random_document(&mut rng, "propri\u{e9}t\u{e9}", 12).circuit().clone();
    check_ok(&initial, seed, 0, "set-up");
    let mut d = Document::new(initial.clone());
    for step in 1..=steps {
        let before = d.circuit().clone();
        let rev = d.rev();
        let roll = rng.below(100);
        if roll < 70 {
            let e = random_edit(&mut rng, d.circuit());
            let preview = d.preview(rev, &e);
            assert_eq!(d.circuit(), &before, "seed {seed} step {step}: a preview changed the document");
            match d.edit(rev, &e) {
                Ok(a) => {
                    assert_eq!(preview.as_ref(), Ok(&a.patch), "seed {seed} step {step}: preview ≠ commit for {e:?}");
                    assert_patch_exact(&before, d.circuit(), &a.patch);
                    if a.patch.is_empty() {
                        assert_eq!((d.rev(), d.circuit()), (rev, &before));
                    } else {
                        assert_eq!((a.rev, d.rev()), (rev + 1, rev + 1));
                        assert!(d.circuit().next_id() >= before.next_id());
                        *stats.committed.entry(edit_name(&e)).or_default() += 1;
                    }
                }
                Err(r) => {
                    assert_eq!(preview.as_ref(), Err(&r), "seed {seed} step {step}: preview ≠ commit for {e:?}");
                    assert_eq!((d.circuit(), d.rev()), (&before, rev), "seed {seed} step {step}: refusal changed things");
                    assert!(!r.message_fr.is_empty());
                    let key = match r.ambiguity {
                        Some(a) => format!("ambiguous/{}", serde_json::to_value(a).unwrap().as_str().unwrap()),
                        None => serde_json::to_value(r.code).unwrap().as_str().unwrap().to_string(),
                    };
                    *stats.refused.entry(key).or_default() += 1;
                }
            }
            check_ok(d.circuit(), seed, step, edit_name(&e));
        } else if roll < 82 {
            if let Some(i) = random_interact(&mut rng, d.circuit()) {
                if d.interact(&i).is_ok() {
                    stats.interactions += 1;
                }
                // Interactions never touch the tables nor the revision.
                assert_eq!((d.circuit().components(), d.circuit().wires(), d.rev()), (before.components(), before.wires(), rev));
            }
        } else {
            let undo = roll < 92;
            let can = if undo { d.can_undo() } else { d.can_redo() };
            match if undo { d.undo() } else { d.redo() } {
                Ok(a) => {
                    assert!(can);
                    assert_patch_exact(&before, d.circuit(), &a.patch);
                    assert_eq!(d.rev(), rev + 1);
                    assert_eq!(d.circuit().next_id(), before.next_id(), "undo/redo never touch next_id");
                    if undo {
                        stats.undos += 1;
                    } else {
                        stats.redos += 1;
                    }
                }
                Err(r) => {
                    assert!(!can);
                    assert_eq!(r.code, if undo { RefusalCode::NothingToUndo } else { RefusalCode::NothingToRedo });
                    assert_eq!(d.circuit(), &before);
                }
            }
            check_ok(d.circuit(), seed, step, if undo { "undo" } else { "redo" });
        }
        stats.junctions_seen += d.circuit().components().values().filter(|c| c.kind == n7rt_circuit::Kind::Junction).count();
    }

    // Undo everything: the initial tables, and the initial canonical text except next_id and interaction states.
    let last = d.circuit().clone();
    let mut undone = 0;
    while d.can_undo() {
        d.undo().unwrap();
        undone += 1;
        check_ok(d.circuit(), seed, steps + 1, "undo-all");
    }
    assert_eq!(d.circuit().components(), initial.components(), "seed {seed}: undo-all components");
    assert_eq!(d.circuit().wires(), initial.wires(), "seed {seed}: undo-all wires");
    assert_eq!(masked_text(d.circuit()), masked_text(&initial), "seed {seed}: undo-all text");
    assert_eq!(d.circuit().next_id(), last.next_id(), "seed {seed}: next_id is never restored");
    // Undo leaves the interaction states alone.
    for id in d.circuit().components().keys().filter(|id| last.components().contains_key(id)) {
        assert_eq!(d.circuit().state(*id), last.state(*id), "seed {seed}: state of {id:?} changed by undo");
    }
    // Redo everything undone: exactly the final circuit, canonical text included. Entries undone during the sequence
    // and never redone (no edit after them) are still on the stack beyond it and stay valid.
    for _ in 0..undone {
        d.redo().unwrap();
        check_ok(d.circuit(), seed, steps + 2, "redo-all");
    }
    assert_eq!(d.circuit(), &last, "seed {seed}: redo-all");
    assert_eq!(persist::save(d.circuit()), persist::save(&last));
    while d.can_redo() {
        d.redo().unwrap();
        check_ok(d.circuit(), seed, steps + 3, "redo beyond");
    }
}

#[test]
fn random_edit_sequences_keep_every_invariant_and_undo_exactly() {
    let mut stats = Stats::default();
    for seed in 0..SEQUENCES {
        run_sequence(seed, STEPS, &mut stats);
    }
    eprintln!(
        "committed {:?}\nrefused {:?}\ninteractions {} undos {} redos {} junction-steps {}",
        stats.committed, stats.refused, stats.interactions, stats.undos, stats.redos, stats.junctions_seen
    );
    // The generator really exercises everything: every edit kind commits, the main refusals and ambiguities occur.
    for kind in ["add_component", "delete", "move", "rotate", "set_param", "set_label", "add_wire", "move_wire_end", "set_bends"] {
        let n = stats.committed.get(kind).copied().unwrap_or(0);
        assert!(n >= 100, "{kind} committed only {n} times: {:?}", stats.committed);
    }
    for code in [
        "ambiguous/pin_on_pin",
        "ambiguous/pin_on_wire",
        "ambiguous/wire_over_pin",
        "ambiguous/collinear_overlap",
        "ambiguous/wire_through_body",
        "duplicate_wire",
        "same_pin",
        "non_orthogonal",
        "no_route",
        "param_parse",
        "param_unit",
        "param_out_of_range",
        "param_locked",
        "label_invalid",
        "label_duplicate",
        "endpoint_not_on_wire",
        "four_way_junction",
        "unknown_pin",
    ] {
        assert!(stats.refused.contains_key(code), "refusal {code} never produced: {:?}", stats.refused);
    }
    assert!(stats.interactions > 500 && stats.undos > 1000 && stats.redos > 200, "{} {} {}", stats.interactions, stats.undos, stats.redos);
    assert!(stats.junctions_seen > 1000, "junctions seen {}", stats.junctions_seen);
}

/// Longer sequences (denser circuits, rarer cascades); run on demand: `-- --ignored soak`.
#[test]
#[ignore = "about a minute in debug"]
fn soak_long_random_sequences() {
    let mut stats = Stats::default();
    for seed in 0..300 {
        run_sequence(0xA5A5_0000 + seed, 150, &mut stats);
    }
    eprintln!("committed {:?}
refused {:?}", stats.committed, stats.refused);
}
