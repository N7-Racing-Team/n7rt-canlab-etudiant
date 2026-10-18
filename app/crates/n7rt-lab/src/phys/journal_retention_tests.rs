use super::*;
use std::fs;

fn commands(host: &mut Host, count: usize, data: &Option<PathBuf>) {
    for _ in 0..count {
        host.core.as_mut().expect("live core").apply(&PhysCommand::Stop).expect("stop command");
        host.spill(data);
        assert!(host.core.as_ref().unwrap().journal().len() <= JOURNAL_MEMORY, "RAM journal must always remain bounded");
    }
}

fn test_path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!("n7rt-journal-{label}-{}", std::process::id()))
}

#[test]
fn no_storage_fifty_thousand_commands_are_bounded_and_visibly_incomplete() {
    let lab = Lab::start(PhysConfig { data_dir: None });
    {
        let mut host = lock(&lab.shared);
        commands(&mut host, 50_000, &None);
        assert_eq!(host.session.lost_entries, 45_005);
        assert_eq!(host.session.spilled_entries, 0);
    }
    let status = lab.journal_status().expect("public session");
    assert_eq!(status.retained_entries, 4_995);
    assert!(!status.lossless && !status.archive_corrupt);
    assert!(lab.require_lossless_session().is_err(), "no integral export/replay after data loss");
    let values = lab.values(0).expect("published warning");
    let notice = values.diagnostics.iter().find(|d| d.code == "journal_incomplete").expect("operator warning");
    assert_eq!(notice.severity, Severity::Warning);
    assert!(notice.message_fr.contains("45005") && notice.message_fr.contains("aucun dossier"));
    lab.new_circuit();
    assert!(lab.journal_status().unwrap().lossless, "a new document starts a new lossless live session");
    assert!(!lab.values(0).unwrap().diagnostics.iter().any(|d| d.code == "journal_incomplete"));
    lab.shutdown();
}

#[test]
fn persistent_io_failure_is_bounded_and_recovery_marks_the_gap_before_appending() {
    let base = test_path("recovery");
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&base).unwrap();
    let data = base.join("data");
    fs::write(&data, b"not a directory").unwrap();
    let mut host = Host::start();
    commands(&mut host, 50_000, &Some(data.clone()));
    assert_eq!(host.session.lost_entries, 45_005);
    assert_eq!(host.session.spilled_entries, 0);
    let session = host.session.started.clone();
    fs::remove_file(&data).unwrap();
    fs::create_dir_all(&data).unwrap();
    // The next spill occurs after 5,006 more entries; its 9,001 entries follow the known loss gap.
    commands(&mut host, 5_006, &Some(data.clone()));
    assert_eq!(host.session.lost_entries, 45_005, "loss must remain sticky after I/O recovers");
    assert_eq!(host.session.spilled_entries, 9_001);
    let folder = data.join(files::SESSIONS).join(session);
    let marker: serde_json::Value = serde_json::from_str(&fs::read_to_string(folder.join("incomplete.json")).unwrap()).unwrap();
    assert_eq!(marker["lost_entries"], 45_005);
    assert_eq!(marker["complete"], false);
    let journal = fs::read_to_string(folder.join("journal.ndjson")).unwrap();
    assert_eq!(journal.lines().count(), 9_001);
    assert!(journal.lines().all(|line| serde_json::from_str::<JournalEntry>(line).is_ok()));
    assert!(host.session.loss_notice().is_some());
    fs::remove_dir_all(&base).unwrap();
}

#[test]
fn good_storage_keeps_every_entry_across_spills_and_never_claims_a_finalized_file() {
    let data = test_path("good");
    let _ = fs::remove_dir_all(&data);
    let mut host = Host::start();
    commands(&mut host, 30_000, &Some(data.clone()));
    let path = data.join(files::SESSIONS).join(&host.session.started);
    let journal = fs::read_to_string(path.join("journal.ndjson")).unwrap();
    let tail = host.core.as_ref().unwrap().journal();
    assert_eq!(journal.lines().count() + tail.len(), 30_000, "spilled prefix plus RAM tail contains every command");
    assert_eq!(host.session.lost_entries, 0);
    assert!(!path.join("incomplete.json").exists());
    assert!(host.session.loss_notice().is_none());
    fs::remove_dir_all(data).unwrap();
}

#[test]
fn rollback_failure_quarantines_the_archive_until_a_new_session() {
    let data = test_path("corrupt");
    let _ = fs::remove_dir_all(&data);
    let mut host = Host::start();
    host.inject_journal_rollback_failure = true;
    commands(&mut host, 10_001, &Some(data.clone()));
    assert!(host.session.archive_corrupt && host.session.lost_entries == 9_001);
    let folder = data.join(files::SESSIONS).join(&host.session.started);
    fs::write(folder.join("journal.ndjson"), b"unknown-partial-prefix").unwrap();
    commands(&mut host, 9_001, &Some(data.clone()));
    assert_eq!(fs::read(folder.join("journal.ndjson")).unwrap(), b"unknown-partial-prefix", "never retry an append on an unknown prefix");
    assert_eq!(host.session.lost_entries, 18_002);
    let marker: serde_json::Value = serde_json::from_str(&fs::read_to_string(folder.join("incomplete.json")).unwrap()).unwrap();
    assert_eq!(marker["archive_corrupt"], true);
    assert_eq!(marker["lost_entries"], 18_002);
    fs::remove_dir_all(data).unwrap();
}

#[test]
fn sealed_journal_is_bounded_without_storage_and_exposes_no_loss_payload() {
    let lab = Lab::start(PhysConfig { data_dir: None });
    {
        let mut host = lock(&lab.shared);
        host.sealed = true;
        commands(&mut host, 50_000, &None);
        assert_eq!(host.session.lost_entries, 0, "intentional sealed withholding is not a public loss counter");
    }
    assert!(lab.journal_tail(usize::MAX).is_empty());
    assert!(lab.journal_status().is_err());
    assert!(lab.require_lossless_session().is_err());
    assert!(!lab.values(0).unwrap().diagnostics.iter().any(|d| d.code == "journal_incomplete"));
    lab.shutdown();
}
