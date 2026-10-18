//! The engine handle used by the Tauri commands (contract §7, §8.1, §8.3): circuit files and their name rules,
//! demonstrations, values publication, disclosure, refusals and the journal spill to the session file.

#[path = "phys_kit.rs"]
mod kit;

use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use kit::*;
use n7rt_circuit::{persist, CompId, Edit, Interact, RefusalCode, Value};
use n7rt_lab::phys::{
    CircuitProbe, CommandReply, Disclosure, FileError, JournalEntry, PhysCommand, PhysConfig, PhysError, PhysLab, Reading,
    RunState,
};

/// A fresh directory under the system temp directory, unique to this process and test.
fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("n7rt-phys-lab-{}-{tag}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    d
}

fn put_circuit(dir: &Path, name: &str, text: &str) {
    fs::create_dir_all(dir.join("circuits")).expect("circuits directory");
    fs::write(dir.join("circuits").join(format!("{name}.json")), text).expect("circuit file");
}

/// A handle on `dir` with the circuit of contract §9.1 loaded from `ref.json`.
fn lab_with_ref(dir: &Path) -> PhysLab {
    put_circuit(dir, "ref", &persist::save(&ref_pot_rc()));
    let lab = PhysLab::start(PhysConfig { data_dir: Some(dir.to_path_buf()) });
    lab.load("ref").expect("ref loaded");
    lab
}

fn label(lab: &PhysLab, l: &str) -> CompId {
    lab.snapshot().components.iter().find(|c| c.label == l).map(|c| c.id).unwrap_or_else(|| panic!("{l}"))
}

fn file_error(r: Result<impl std::fmt::Debug, PhysError>) -> FileError {
    match r {
        Err(PhysError::File { error }) => error,
        other => panic!("file error expected, got {other:?}"),
    }
}

fn refusal(r: Result<impl std::fmt::Debug, PhysError>) -> RefusalCode {
    match r {
        Err(PhysError::Refused { refusal }) => refusal.code,
        other => panic!("refusal expected, got {other:?}"),
    }
}

#[test]
fn save_load_list_and_name_rules() {
    let dir = temp_dir("files");
    put_circuit(&dir, "Ref", &persist::save(&ref_pot_rc()));
    let lab = PhysLab::start(PhysConfig { data_dir: Some(dir.clone()) });
    let empty_rev = lab.snapshot().rev;

    // Case-insensitive lookup; the stored name is kept; a load is STOP, clean, without history.
    let s = lab.load("ref").expect("loaded");
    assert_eq!((s.name.as_str(), s.run, s.dirty, s.can_undo, s.can_redo), ("Ref", RunState::Stop, false, false, false));
    assert!(s.rev > empty_rev);
    assert_eq!(s.components.len(), ref_pot_rc().components().len());

    let r1 = label(&lab, "R1");
    let rev = lab.snapshot().rev;
    lab.command(PhysCommand::Edit { base_rev: rev, edit: Edit::SetParam { comp: r1, name: "resistance".into(), value: Value::Num(22e3) } })
        .expect("edit");
    assert!(lab.snapshot().dirty);

    for bad in ["a b", "-x", "_x", "CON", "nul", "Com1", "x.json", "é", &"x".repeat(65)] {
        assert_eq!(file_error(lab.save(bad, true)), FileError::BadName, "{bad}");
    }
    assert_eq!(file_error(lab.save("REF", false)), FileError::NameExists);
    let s = lab.save("ref", true).expect("overwritten");
    assert_eq!((s.name.as_str(), s.dirty), ("ref", false));
    let names: Vec<String> = lab.list().expect("list").into_iter().map(|e| e.name).collect();
    assert_eq!(names, vec!["ref".to_string()], "one file, under its new case");

    // The file is canonical and reloads to the same circuit.
    let text = fs::read_to_string(dir.join("circuits").join("ref.json")).expect("saved file");
    let loaded = persist::load(&text).expect("valid file");
    assert_eq!(persist::save(&loaded), text, "save(load(t)) = t");
    assert_eq!(loaded.name(), "ref");
    assert_eq!(loaded.components()[&r1].params["resistance"], Value::Num(22e3));
    let before = lab.snapshot().rev;
    let s = lab.load("REF").expect("reloaded");
    assert!(s.rev > before, "revisions keep increasing across loads");
    assert_eq!(s.components, lab.snapshot().components);

    lab.save("second-circuit_2", false).expect("saved");
    let names: Vec<String> = lab.list().expect("list").into_iter().map(|e| e.name).collect();
    assert_eq!(names, vec!["ref".to_string(), "second-circuit_2".to_string()]);

    assert_eq!(file_error(lab.load("absent")), FileError::NotFound);
    // An invalid file is refused with its violations, and nothing is loaded.
    put_circuit(&dir, "broken", "{\"format\": \"autre\"}\n");
    let current = lab.snapshot();
    match file_error(lab.load("broken")) {
        FileError::Invalid { violations } => assert!(!violations.is_empty()),
        other => panic!("{other:?}"),
    }
    assert_eq!(lab.snapshot(), current);
    lab.shutdown();
    lab.shutdown();
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn no_storage_without_data_directory() {
    let lab = PhysLab::start(PhysConfig::default());
    assert_eq!(file_error(lab.save("x", false)), FileError::NoStorage);
    assert_eq!(file_error(lab.load("x")), FileError::NoStorage);
    assert_eq!(file_error(lab.list()), FileError::NoStorage);
    assert_eq!(file_error(lab.save("a b", false)), FileError::BadName);
    let s = lab.snapshot();
    assert_eq!((s.name.as_str(), s.run, s.disclosure), ("sans titre", RunState::Stop, Disclosure::Exploration));
    assert!(s.components.is_empty());
}

#[test]
fn demonstrations_and_new_circuit() {
    let lab = PhysLab::start(PhysConfig::default());
    let ids: Vec<String> = lab.demos().into_iter().map(|d| d.id).collect();
    assert_eq!(ids, vec!["ref-pot-rc".to_string(), "tp1-capteur-can".to_string()], "the performance fixture is hidden");
    assert!(lab.demos().iter().all(|d| !d.title_fr.is_empty() && !d.summary_fr.is_empty()));
    assert_eq!(file_error(lab.load_demo("absent")), FileError::NotFound);
    let rev = lab.snapshot().rev;
    let s = lab.new_circuit();
    assert!(s.rev > rev && s.components.is_empty() && s.name == "sans titre");
}

#[test]
fn preview_commands_and_refusals() {
    let dir = temp_dir("refusals");
    let lab = lab_with_ref(&dir);
    let (r1, rv1) = (label(&lab, "R1"), label(&lab, "RV1"));
    let rev = lab.snapshot().rev;
    let edit = Edit::SetParam { comp: r1, name: "resistance".into(), value: Value::Choice("4k7".into()) };
    let p = lab.preview(rev, &edit).expect("preview");
    assert_eq!(p.base_rev, rev);
    assert_eq!(p.patch.components.len(), 1);
    assert_eq!(lab.snapshot().rev, rev, "a preview commits nothing");
    assert_eq!(refusal(lab.preview(rev + 1, &edit)), RefusalCode::StaleRevision);
    assert_eq!(refusal(lab.command(PhysCommand::Edit { base_rev: rev + 1, edit: edit.clone() })), RefusalCode::StaleRevision);
    let bad = Edit::SetParam { comp: r1, name: "resistance".into(), value: Value::Num(0.0) };
    assert_eq!(refusal(lab.command(PhysCommand::Edit { base_rev: rev, edit: bad })), RefusalCode::ParamOutOfRange);
    assert_eq!(refusal(lab.command(PhysCommand::Undo)), RefusalCode::NothingToUndo);
    let unknown = PhysCommand::Interact { interact: Interact::SetPot { comp: CompId(999), position: 0.5 } };
    assert_eq!(refusal(lab.command(unknown)), RefusalCode::UnknownComponent);
    assert!(lab.journal_tail(10).is_empty(), "refusals are not journaled");

    match lab.command(PhysCommand::Edit { base_rev: rev, edit }).expect("edit") {
        CommandReply::Edit(r) => {
            assert!(r.rev > rev && r.can_undo && !r.can_redo && r.dirty);
            assert_eq!(r.patch, p.patch, "the preview is the same function as the commit");
        }
        other => panic!("{other:?}"),
    }
    // Interactions do not change the revision.
    let rev = lab.snapshot().rev;
    match lab.command(PhysCommand::Interact { interact: Interact::SetPot { comp: rv1, position: 0.25 } }).expect("pot") {
        CommandReply::Interact(r) => assert_eq!((r.rev, r.comp, r.state["position"].clone()), (rev, rv1, Value::Num(0.25))),
        other => panic!("{other:?}"),
    }
    let tail = lab.journal_tail(10);
    assert_eq!(tail.len(), 2);
    assert!(matches!(tail[1].command, PhysCommand::Interact { .. }));
    lab.shutdown();
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn values_are_pulled_and_filtered_by_disclosure() {
    let dir = temp_dir("values");
    let lab = lab_with_ref(&dir);
    let (rv1, adc1) = (label(&lab, "RV1"), label(&lab, "ADC1"));
    let v0 = lab.values(0).expect("first values");
    assert!(lab.values(v0.seq).is_none(), "nothing new");
    assert_eq!((v0.run, v0.preview, v0.disclosure), (RunState::Stop, true, Disclosure::Exploration));
    assert!(!v0.nets.is_empty());

    lab.command(PhysCommand::Run).expect("run");
    let v1 = lab.values(v0.seq).expect("published after the command");
    assert_eq!(v1.run, RunState::Run);
    thread::sleep(Duration::from_millis(150));
    let v2 = lab.values(v1.seq).expect("published while running");
    assert!(v2.sim_time_ns > 0 && v2.seq > v1.seq);
    let probe = lab.probe();
    assert!(matches!(probe.adc(adc1, "in"), Reading::Valid { .. }), "sampled while running");
    lab.command(PhysCommand::Stop).expect("stop");
    let t = lab.snapshot().sim_time_ns;
    thread::sleep(Duration::from_millis(60));
    assert_eq!(lab.snapshot().sim_time_ns, t, "STOP freezes the time");

    lab.set_disclosure(Disclosure::Mission);
    let m = lab.values(0).expect("values");
    assert_eq!(m.disclosure, Disclosure::Mission);
    assert!(m.nets.is_empty() && m.diagnostics.is_empty() && m.comps.iter().all(|c| c.pins.is_empty() && c.measures.is_empty()));
    assert_eq!(lab.snapshot().disclosure, Disclosure::Mission);
    let w = pin(rv1, "w");
    assert_eq!(unavailable(lab.probe().unpowered_resistance(&w, &pin(rv1, "a"))), "hidden_mission");
    lab.set_disclosure(Disclosure::Exploration);
    assert_close("R_AW unpowered", valid(lab.probe().unpowered_resistance(&w, &pin(rv1, "a"))), 3e3, 1e-9);
    lab.shutdown();
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn journal_spills_to_the_session_file_beyond_10000_entries() {
    let dir = temp_dir("spill");
    let lab = PhysLab::start(PhysConfig { data_dir: Some(dir.clone()) });
    for _ in 0..10_001 {
        lab.command(PhysCommand::Stop).expect("stop");
    }
    let sessions: Vec<PathBuf> = fs::read_dir(dir.join("sessions")).expect("sessions").flatten().map(|e| e.path()).collect();
    assert_eq!(sessions.len(), 1, "{sessions:?}");
    let initial = fs::read_to_string(sessions[0].join("initial.json")).expect("initial circuit");
    assert!(persist::load(&initial).expect("valid").components().is_empty());
    let spilled: Vec<JournalEntry> = fs::read_to_string(sessions[0].join("journal.ndjson"))
        .expect("journal")
        .lines()
        .map(|l| serde_json::from_str(l).expect("journal line"))
        .collect();
    let kept = lab.journal_tail(usize::MAX);
    assert_eq!(spilled.len() + kept.len(), 10_001);
    assert!(kept.len() <= 10_000 && spilled.len() >= 9_000);
    assert!(spilled.iter().chain(&kept).all(|e| e == &JournalEntry { frame: 0, command: PhysCommand::Stop }));
    assert_eq!(lab.journal_tail(3).len(), 3);
    lab.shutdown();
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn journal_without_data_directory_is_bounded_and_reports_loss() {
    let lab = PhysLab::start(PhysConfig::default());
    for _ in 0..10_050 {
        lab.command(PhysCommand::Stop).expect("stop");
    }
    let status = lab.journal_status().expect("journal status");
    assert!(status.retained_entries <= 10_000);
    assert_eq!(status.retained_entries as u64 + status.lost_entries, 10_050);
    assert_eq!(status.spilled_entries, 0);
    assert!(!status.lossless && status.lost_entries > 0);
    assert!(lab.require_lossless_session().is_err());
    assert!(lab.values(0).expect("values").diagnostics.iter().any(|d| d.code == "journal_incomplete"));
    lab.shutdown();
}
