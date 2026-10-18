//! Student and instructor modes (docs/distribution.md §2): instructor directory discovery, the student's mission lock
//! (persisted, refused for another mission, cleared by the instructor reset only), pack import and opening, the
//! instructor login, the run record and the run file, the instructor tools. Every test uses temporary directories.
//! The instructor tools go through the console tool's library (n7rt-tool); the seal is n7rt-seal, run files and
//! variants are n7rt-vehicle. Tests that need the private pack pass with a note when it is absent.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use n7rt_lab::access::{InstructorLocator, Mode, StudentStore, AUTH_FILE, MSG_NO_INSTRUCTOR, MSG_WRONG_GROUP_CODE};
use n7rt_lab::engine::{CheckRecord, RecEntry, RecFrame, RecTick};
use n7rt_lab::fake::FakeHandle;
use n7rt_lab::mission::{feed_order, locked_message, run_record, Feed, MissionConfig, Missions, RunMeta};
use n7rt_lab::model::{LinkState, UiBatch};
use n7rt_lab::phys::{PhysConfig, PhysLab};
use n7rt_lab::sim::Stimuli;
use n7rt_lab::{Lab, LabConfig};
use n7rt_vehicle::{NominalScenario, Pack, RefConsumer, RunFrame, RunRecord, RunSlot, RunTick, SlotOut, Vehicle};

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> TempDir {
        static N: AtomicU64 = AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!("n7rt-modes-{tag}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn write(p: &Path, bytes: impl AsRef<[u8]>) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, bytes).unwrap();
}

struct Ctl {
    lab: Arc<Lab>,
    phys: Arc<PhysLab>,
    missions: Missions,
    _fake: FakeHandle,
}

impl Ctl {
    fn new(app_data: &Path, instructor: Option<&Path>) -> Ctl {
        Ctl::with_delay(app_data, instructor, Some(Duration::ZERO))
    }

    fn with_delay(app_data: &Path, instructor: Option<&Path>, delay: Option<Duration>) -> Ctl {
        let fake = FakeHandle::new();
        let cfg = LabConfig { record: true, stimuli: Stimuli { rate_hz: 0, ..Stimuli::default() }, ..LabConfig::default() };
        let lab = Arc::new(Lab::start(cfg, fake.connector()));
        let phys = Arc::new(PhysLab::start(PhysConfig::default()));
        let cfg = MissionConfig {
            fixed_seed: Some(7),
            app_data_dir: Some(app_data.to_path_buf()),
            documents_dir: Some(app_data.join("Documents")),
            instructor: InstructorLocator::fixed(instructor.map(Path::to_path_buf).unwrap_or_else(|| app_data.join("absent"))),
            failed_login_delay: delay,
            ..MissionConfig::default()
        };
        let missions = Missions::start(lab.clone(), phys.clone(), cfg);
        Ctl { lab, phys, missions, _fake: fake }
    }

    fn wait(&self, what: &str, timeout_ms: u64, cond: impl Fn(&UiBatch) -> bool) {
        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        while !cond(&self.lab.snapshot()) {
            assert!(Instant::now() < deadline, "timeout waiting for {what}");
            thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for Ctl {
    fn drop(&mut self) {
        self.missions.shutdown();
        self.lab.shutdown();
        self.phys.shutdown();
    }
}

// ----------------------------------------------------------------------------------------- instructor directory

#[test]
fn instructor_directory_is_found_in_order() {
    let t = TempDir::new("locate");
    let root = t.0.join("projets");
    let exe_dir = root.join("n7rt-canlab/app/target/release");
    std::fs::create_dir_all(&exe_dir).unwrap();
    let none = InstructorLocator { env: None, config_file: None, starts: vec![exe_dir.clone()] };
    assert_eq!(none.locate(), None);

    // A sibling n7rt-canlab-missions of an ancestor.
    let sibling = root.join("n7rt-canlab-missions");
    std::fs::create_dir_all(&sibling).unwrap();
    assert_eq!(none.locate(), Some(sibling.clone()));
    // A directory holding instructor/auth.json above the executable wins over the sibling.
    write(&root.join("n7rt-canlab").join(AUTH_FILE), "{}");
    assert_eq!(none.locate(), Some(root.join("n7rt-canlab")));
    // The path registered by the instructor install wins over both (absolute, existing).
    let registered = t.0.join("registered");
    std::fs::create_dir_all(&registered).unwrap();
    let cfg = t.0.join("instructor.json");
    write(&cfg, serde_json::json!({"instructor_dir": registered}).to_string());
    let with_cfg = InstructorLocator { config_file: Some(cfg.clone()), ..none.clone() };
    assert_eq!(with_cfg.locate(), Some(registered.clone()));
    write(&cfg, r#"{"instructor_dir": "relative/path"}"#);
    assert_eq!(with_cfg.locate(), Some(root.join("n7rt-canlab")), "a relative registered path is ignored");
    // The variable wins over everything, and names the directory itself (None if it does not exist).
    let env = InstructorLocator { env: Some(sibling.clone().into_os_string()), ..with_cfg.clone() };
    assert_eq!(env.locate(), Some(sibling));
    let gone = InstructorLocator { env: Some(t.0.join("gone").into_os_string()), ..with_cfg };
    assert_eq!(gone.locate(), None);
}

// ---------------------------------------------------------------------------------------------- student mode

#[test]
fn every_mission_choice_remains_exclusive_after_restart() {
    for chosen in ["apps", "wsf", "sas", "bps"] {
        let data = TempDir::new("four-locks");
        {
            let c = Ctl::new(&data.0,None);
            assert_eq!(c.missions.student_choose(chosen).unwrap().student.locked.as_deref(),Some(chosen));
            assert!(c.missions.reset().is_err());
        }
        let c = Ctl::new(&data.0,None);
        assert_eq!(c.missions.access().student.locked.as_deref(),Some(chosen));
        assert!(c.missions.student_choose(chosen).is_ok());
        for other in ["apps","wsf","sas","bps"].into_iter().filter(|id| *id != chosen) {
            assert_eq!(c.missions.student_choose(other).unwrap_err(),locked_message(chosen));
            assert_eq!(c.missions.open(other).unwrap_err(),locked_message(chosen));
        }
    }
}

#[test]
fn the_student_lock_persists_and_refuses_another_mission() {
    let data = TempDir::new("lock");
    {
        let c = Ctl::new(&data.0, None);
        let m = &c.missions;
        let a = m.access();
        assert_eq!((a.mode, a.student.locked.as_deref(), a.student.pack.as_ref()), (Mode::Student, None, None));
        let list = m.list();
        assert_eq!(list.iter().map(|e| (e.id.as_str(), e.available)).collect::<Vec<_>>(),
                   [("apps", true), ("wsf", true), ("sas", true), ("bps", true)]);
        assert!(list.iter().all(|e| e.reason.is_none()));
        assert!(m.student_choose("xyz").is_err());
        assert_eq!(m.access().student.locked, None, "a refused choice locks nothing");

        let a = m.student_choose("apps").expect("APPS chosen");
        assert_eq!(a.student.locked.as_deref(), Some("apps"));
        assert_eq!(a.student.locked_title.as_deref(), Some("APPS — pédale d'accélérateur"));
        assert!(data.0.join("student/state.json").is_file());
        assert!(m.student_choose("apps").is_ok(), "the same choice again is harmless");
        let locked = "La mission de cette session est déjà choisie : APPS.";
        assert_eq!(locked_message("apps"), locked);
        assert_eq!(m.student_choose("wsf").unwrap_err(), locked);
        assert_eq!(m.open("wsf").unwrap_err(), locked);
        assert_eq!(m.open("bps").unwrap_err(), locked);
        assert_eq!(m.open("apps").unwrap_err(), "Saisissez le code du groupe pour ouvrir la mission.");
        assert!(!m.view().open);
    }
    // A new controller (application restarted) reads the lock back.
    let c = Ctl::new(&data.0, None);
    assert_eq!(c.missions.access().student.locked.as_deref(), Some("apps"));
    assert_eq!(c.missions.student_choose("sas").unwrap_err(), locked_message("apps"));
    // Only an instructor clears it: every instructor action is refused in student mode.
    for err in [
        c.missions.instructor_reset().unwrap_err(),
        c.missions.reset().unwrap_err(),
        c.missions.instructor_gen_packs("apps", "c1", &["G01".into()], false).unwrap_err(),
        c.missions.instructor_judge("essai.n7rtrun", b"{}").unwrap_err(),
    ] {
        assert_eq!(err, "Réservé au mode encadrant.");
    }
    assert_eq!(c.missions.access().student.locked.as_deref(), Some("apps"));
    // Logging out of a mode that is not active changes nothing.
    assert_eq!(c.missions.instructor_logout().unwrap().mode, Mode::Student);
}

#[test]
fn a_pack_needs_the_lock_and_opening_needs_the_pack() {
    let data = TempDir::new("pack");
    let c = Ctl::new(&data.0, None);
    let m = &c.missions;
    let choose_first = "Choisissez d'abord la mission de la session.";
    assert_eq!(m.student_import_pack(b"anything").unwrap_err(), choose_first);
    assert_eq!(m.student_open("CODE").unwrap_err(), choose_first);
    m.student_choose("apps").unwrap();
    assert_eq!(m.student_open("CODE").unwrap_err(), "Importez d'abord le pack du groupe.");
    // Not a sealed pack: refused, nothing stored.
    assert_eq!(m.student_import_pack(b"{\"format\":\"something else\"}").unwrap_err(), "Ce fichier n'est pas un pack de groupe N7RT.");
    assert_eq!(m.student_import_pack(&[0xFF; 64]).unwrap_err(), "Ce fichier n'est pas un pack de groupe N7RT.");
    assert!(m.access().student.pack.is_none());
    let packs = data.0.join("student/packs");
    assert!(!packs.exists() || std::fs::read_dir(&packs).unwrap().next().is_none());
    // A state file written by hand cannot point outside the packs folder.
    let store = StudentStore { app_data_dir: Some(data.0.clone()) };
    assert!(data.0.join("student/state.json").is_file());
    assert!(store.read_pack("../state.json").is_none() && store.read_pack("../x.n7rtpack").is_none());
}

#[test]
fn instructor_login_fails_without_resources_and_waits_one_second() {
    let data = TempDir::new("login");
    let c = Ctl::with_delay(&data.0, None, None);
    let t0 = Instant::now();
    assert_eq!(c.missions.instructor_login("ABCD").unwrap_err(), MSG_NO_INSTRUCTOR);
    assert!(t0.elapsed() >= Duration::from_millis(990), "failed attempt answered after {:?}", t0.elapsed());
    assert_eq!(c.missions.mode(), Mode::Student);
    // A directory without a valid auth file: no instructor mode either.
    let dir = TempDir::new("noauth");
    let c = Ctl::new(&data.0, Some(&dir.0));
    assert_eq!(c.missions.instructor_login("ABCD").unwrap_err(), MSG_NO_INSTRUCTOR);
    write(&dir.0.join(AUTH_FILE), r#"{"format":"n7rt-instructor-auth","version":1,"code_phc":"plain"}"#);
    assert_eq!(c.missions.instructor_login("ABCD").unwrap_err(), MSG_NO_INSTRUCTOR);
    assert_eq!(c.missions.mode(), Mode::Student);
}

// --------------------------------------------------------------------------------------------- run record

fn check_record() -> CheckRecord {
    CheckRecord {
        session_id: Some(3),
        frames: vec![
            RecFrame { t_us: 1_500, id: 0x10, flags: 0, data: vec![1, 2] },
            RecFrame { t_us: 12_000, id: 0x20, flags: 4, data: vec![3] },
        ],
        ticks: vec![
            RecTick { t_us: 10_000, status: 0, entries: vec![RecEntry { slot: 0, kind: 0, value: 2.5 }, RecEntry { slot: 9, kind: 1, value: 0.0 }] },
            RecTick { t_us: 20_000, status: 1, entries: vec![RecEntry { slot: 0, kind: 0, value: 7.0 }] },
        ],
        overflow: false,
    }
}

fn meta() -> RunMeta {
    RunMeta {
        mission: "apps".into(),
        cohort: "c1".into(),
        group: "G01".into(),
        pack_id: "00112233445566778899aabbccddeeff".into(),
        app_version: "0.1.0".into(),
        board_uid: "UID".into(),
        runtime_build: "0.3.0 abc".into(),
        student_crc32: "0x12345678".into(),
        seed_vehicle: 7,
        seed_scenario: 8,
        duration_us: 20_000,
        tick_us: 10_000,
        started: Some(SystemTime::UNIX_EPOCH + Duration::from_secs(1_791_296_553)),
    }
}

#[test]
fn the_run_record_keeps_every_frame_and_tick_as_delivered() {
    let run = run_record(&meta(), &check_record());
    assert_eq!((run.mission.as_str(), run.cohort.as_str(), run.group.as_str()), ("apps", "c1", "G01"));
    assert_eq!((run.seed_vehicle.as_str(), run.seed_scenario.as_str()), ("7", "8"));
    assert_eq!((run.duration_us, run.tick_us, run.started_utc.as_str()), (20_000, 10_000, "20261006T142233Z"));
    assert_eq!(run.frames, [RunFrame { t_us: 1_500, id: 0x10, flags: 0, data: vec![1, 2] }, RunFrame { t_us: 12_000, id: 0x20, flags: 4, data: vec![3] }]);
    assert_eq!(run.ticks.len(), 2);
    assert_eq!(run.ticks[0].slots.len(), 10);
    assert_eq!((run.ticks[0].slots[0], run.ticks[0].slots[1], run.ticks[0].slots[9]), (Some(RunSlot::V(2.5)), None, Some(RunSlot::Reject)));
    assert!(run.ticks[1].slots.iter().all(Option::is_none), "a tick not delivered writes nothing");
}

#[test]
fn the_run_file_reads_back_identically() {
    let run = run_record(&meta(), &check_record());
    let bytes = run.to_file_bytes();
    assert_eq!(RunRecord::from_file_bytes(&bytes).expect("readable"), run);
    let mut altered = bytes.clone();
    let i = altered.len() / 2;
    altered[i] ^= 0x01;
    assert!(RunRecord::from_file_bytes(&altered).is_err(), "integrity checked");
}

// ------------------------------------------------------------------------------ with the seal (lots S and V2)

const CODE: &str = "TEST-INSTRUCTOR-CODE";

/// An instructor directory with `auth.json` for [`CODE`], a master key and, when present, the private pack.
fn instructor_dir(with_pack: bool) -> Option<TempDir> {
    let src = private_apps_dir();
    if with_pack && !src.join("pack-v0.json").is_file() {
        println!("note: restricted/missions/apps/pack-v0.json absent: test skipped");
        return None;
    }
    let t = TempDir::new("instructor");
    let phc = n7rt_seal::hash_code(CODE).expect("hash");
    write(&t.0.join(AUTH_FILE), serde_json::json!({"format": "n7rt-instructor-auth", "version": 1, "code_phc": phc}).to_string());
    write(&t.0.join("instructor/master.key"), "5a".repeat(32));
    if with_pack {
        std::fs::create_dir_all(t.0.join("missions/apps")).unwrap();
        for f in std::fs::read_dir(&src).unwrap().flatten().filter(|f| f.path().is_file()) {
            std::fs::copy(f.path(), t.0.join("missions/apps").join(f.file_name())).unwrap();
        }
    }
    Some(t)
}

#[test]
fn instructor_login_and_reset() {
    let data = TempDir::new("reset");
    let dir = instructor_dir(false).unwrap();
    let c = Ctl::new(&data.0, Some(&dir.0));
    let m = &c.missions;
    m.student_choose("apps").unwrap();
    assert_eq!(m.instructor_login("WRONG-CODE").unwrap_err(), "Code encadrant incorrect.");
    assert_eq!(m.instructor_login("").unwrap_err(), "Code encadrant incorrect.");
    assert_eq!(m.mode(), Mode::Student);
    // The typed code is normalized: case, spaces and dashes do not matter.
    let a = m.instructor_login(&CODE.to_lowercase().replace('-', " ")).expect("right code");
    assert_eq!(a.mode, Mode::Instructor);
    // Instructor list: the reference pack is missing here, all four are playable but their reference packs are absent.
    let list = m.list();
    assert_eq!(list[0].reason.as_deref(), Some("pack de mission introuvable"));
    assert!(m.open("apps").unwrap_err().contains("pack de mission introuvable"));
    assert_eq!(m.student_choose("wsf").unwrap_err(), "Quittez le mode encadrant pour agir sur la session étudiante.");
    // Reset: no lock, no pack; the mode stays.
    std::fs::create_dir_all(data.0.join("student/packs")).unwrap();
    write(&data.0.join("student/packs/apps-G01.n7rtpack"), "x");
    let a = m.instructor_reset().expect("reset");
    assert_eq!((a.mode, a.student.locked.as_deref()), (Mode::Instructor, None));
    assert!(!data.0.join("student/state.json").exists() && !data.0.join("student/packs/apps-G01.n7rtpack").exists());
    assert_eq!(m.instructor_logout().unwrap().mode, Mode::Student);
    assert_eq!(m.instructor_reset().unwrap_err(), "Réservé au mode encadrant.");
    drop(c);
    let c = Ctl::new(&data.0, Some(&dir.0));
    assert_eq!(c.missions.access().student.locked, None, "the reset is persistent");
    assert_eq!(c.missions.mode(), Mode::Student, "the instructor mode is never persisted");
    assert!(c.missions.student_choose("apps").is_ok());
}

fn active(s: &UiBatch) -> bool {
    s.status.session.as_ref().is_some_and(|x| x.phase == "active")
}

#[test]
fn the_student_opens_a_sealed_group_pack_and_records_a_run_judged_by_the_instructor() {
    let Some(dir) = instructor_dir(true) else { return };
    let data = TempDir::new("student");
    let c = Ctl::new(&data.0, Some(&dir.0));
    let m = &c.missions;
    c.wait("ready", 3000, |s| s.status.state == LinkState::Ready);
    // Instructor: the group pack of the cohort.
    m.instructor_login(CODE).unwrap();
    let rows = m.instructor_gen_packs("apps", "c1", &["G01".into(), "G02".into()], false).expect("packs");
    assert_eq!(rows.iter().map(|r| r.group.as_str()).collect::<Vec<_>>(), ["G01", "G02"]);
    assert_ne!(rows[0].code, rows[1].code);
    assert_eq!(rows[0].file, "cohorts/c1/apps/packs/G01.n7rtpack");
    let groups: serde_json::Value = serde_json::from_slice(&std::fs::read(dir.0.join("cohorts/c1/apps/groups.json")).unwrap()).unwrap();
    assert_eq!(groups.as_array().unwrap().len(), 2, "the tool's groups.json: one entry per group");
    m.instructor_logout().unwrap();
    // Student: lock, import, open with the group code.
    m.student_choose("apps").unwrap();
    let sealed = std::fs::read(dir.0.join(&rows[0].file)).unwrap();
    let a = m.student_import_pack(&sealed).expect("import");
    let pack = a.student.pack.unwrap();
    assert_eq!((pack.cohort.as_str(), pack.group.as_str()), ("c1", "G01"));
    assert_eq!(std::fs::read(data.0.join("student/packs/apps-G01.n7rtpack")).unwrap(), sealed, "copied as it is");
    assert_eq!(m.student_open(&rows[1].code).unwrap_err(), MSG_WRONG_GROUP_CODE);
    assert!(!m.view().open);
    let v = m.student_open(&rows[0].code).expect("opens");
    assert_eq!((v.open, v.kind, v.group.as_deref(), v.tick_period_ms), (true, Some("group"), Some("G01"), Some(10)));
    assert_eq!(m.open("wsf").unwrap_err(), locked_message("apps"));
    // The nominal check records a run file and gives no verdict.
    c.lab.command(n7rt_lab::Command::StartSession { tick_period_ms: 10 });
    c.wait("active session", 3000, active);
    m.check().expect("check starts");
    let deadline = Instant::now() + Duration::from_secs(45);
    let v = loop {
        let v = m.view();
        if v.check.state != "running" {
            break v;
        }
        assert!(Instant::now() < deadline, "check still running");
        thread::sleep(Duration::from_millis(100));
    };
    assert_eq!((v.check.state, v.check.verdict), ("recorded", None), "{:?}", v.check);
    let name = v.check.run_file.clone().unwrap();
    assert!(name.starts_with("essai-apps-G01-") && name.ends_with(".n7rtrun"), "{name}");
    assert_eq!(m.access().student.last_run.as_deref(), Some(name.as_str()));
    let path = data.0.join("Documents/N7RT CAN Lab/essais").join(&name);
    assert_eq!(m.last_run_path().as_deref(), Some(path.as_path()));
    let bytes = std::fs::read(&path).expect("run file written");
    let run = RunRecord::from_file_bytes(&bytes).expect("run file readable");
    assert_eq!((run.mission.as_str(), run.cohort.as_str(), run.group.as_str()), ("apps", "c1", "G01"));
    assert_eq!(run.duration_us, 9_000_000);
    assert!(run.ticks.len() >= 900 && run.frames.len() > 1000);
    assert!(!dir.0.join("missions/apps/reports").exists(), "no trainer report for a student run");
    // The instructor judges it: the fake board publishes nothing, the run fails.
    c.wait("normal session", 5000, active);
    m.close().unwrap();
    m.instructor_login(CODE).unwrap();
    if !n7rt_vehicle::ORACLE {
        // public edition: the run is recorded, its judgement is refused and says so
        assert_eq!(m.instructor_judge(&name, &bytes).unwrap_err(), n7rt_lab::mission::ORACLE_ABSENT);
        return;
    }
    let j = m.instructor_judge(&name, &bytes).expect("judged");
    assert_eq!((j.group.as_str(), j.cohort.as_str(), j.verdict), ("G01", "c1", "fail"));
    let stem = name.trim_end_matches(".n7rtrun");
    assert_eq!(j.judged_file.as_deref(), Some(format!("cohorts/c1/apps/judged/{stem}.json").as_str()));
    assert!(dir.0.join(j.judged_file.unwrap()).is_file());
    // A group already generated is refused unless replaced; the run of the replaced pack is still judged.
    assert!(m.instructor_gen_packs("apps", "c1", &["G01".into()], false).unwrap_err().contains("G01"));
    let again = m.instructor_gen_packs("apps", "c1", &["G01".into()], true).expect("replaced");
    assert_ne!(again[0].code, rows[0].code);
    assert_eq!(m.instructor_judge(&name, &bytes).expect("still judged").verdict, "fail");
    // Not a run file: the tool's sentence, without the instructor directory.
    let err = m.instructor_judge("x.n7rtrun", b"not a run").unwrap_err();
    assert!(!err.contains(&dir.0.display().to_string()), "{err}");
    // Wrong code at the tool: impossible here (the code was verified at login), and logout forgets it.
    m.instructor_logout().unwrap();
    assert_eq!(m.instructor_judge(&name, &bytes).unwrap_err(), "Réservé au mode encadrant.");
}

#[test]
fn a_perfect_run_of_the_group_variant_passes() {
    if !n7rt_vehicle::ORACLE {
        eprintln!("note : édition publique (sans oracle) — jugement non testé");
        return;
    }
    let Some(dir) = instructor_dir(true) else { return };
    let data = TempDir::new("judge");
    let c = Ctl::new(&data.0, Some(&dir.0));
    let m = &c.missions;
    m.instructor_login(CODE).unwrap();
    m.instructor_gen_packs("apps", "c2", &["G07".into()], false).unwrap();
    let groups: serde_json::Value = serde_json::from_slice(&std::fs::read(dir.0.join("cohorts/c2/apps/groups.json")).unwrap()).unwrap();
    let pack_id = groups[0]["pack_id"].as_str().unwrap().to_owned();
    // The group's full variant, as the instructor side derives it.
    let reference = Pack::load(&dir.0.join("missions/apps/pack-v0.json")).unwrap();
    let master = [0x5a; 32];
    let variant = reference.variant(&n7rt_seal::group_seed(&master, "apps", "c2", "G07")).unwrap();
    let scenario = NominalScenario::new(&variant, 11);
    let duration = scenario.duration_us();
    let tick_us = variant.tick_period_ms() as u64 * 1000;
    let frames = Vehicle::new(&variant, 5).advance(duration, &mut |t_ms| variant.ideal_inputs(scenario.position_at(t_ms * 1000)));
    let rec: Vec<RecFrame> = frames.iter().map(|f| RecFrame { t_us: f.t_us, id: f.id, flags: 0, data: f.data.clone() }).collect();
    // A board answering exactly what the reference consumer expects.
    let mut rc = RefConsumer::new(&variant);
    let mut ticks = Vec::new();
    for step in feed_order(&rec, duration, tick_us) {
        match step {
            Feed::Tick(t) => {
                let o = rc.on_tick(t);
                let slots = o.slots.iter().map(|s| s.map(|x| match x { SlotOut::Value(v) => RunSlot::V(v), SlotOut::Reject => RunSlot::Reject })).collect();
                ticks.push(RunTick { t_us: t, slots });
            }
            Feed::Frame(i) => rc.on_frame(rec[i].t_us, rec[i].id, 0, &rec[i].data),
        }
    }
    let mut run = RunRecord {
        mission: "apps".into(),
        cohort: "c2".into(),
        group: "G07".into(),
        pack_id,
        app_version: "test".into(),
        board_uid: "UID".into(),
        runtime_build: "rt".into(),
        student_crc32: "0".into(),
        seed_vehicle: "5".into(),
        seed_scenario: "11".into(),
        duration_us: duration,
        tick_us,
        started_utc: "20261006T120000Z".into(),
        frames: rec.iter().map(|f| RunFrame { t_us: f.t_us, id: f.id, flags: 0, data: f.data.clone() }).collect(),
        ticks,
    };
    let j = m.instructor_judge("essai-apps-G07-20261006-120000.n7rtrun", &run.to_file_bytes()).expect("judged");
    assert_eq!(j.verdict, "pass", "{}", j.detail);
    // One wrong published value late in the run: not passed.
    let k = run.ticks.len() - 10;
    run.ticks[k].slots[0] = Some(RunSlot::V(1.0e6));
    assert_eq!(m.instructor_judge("essai-apps-G07-20261006-120001.n7rtrun", &run.to_file_bytes()).expect("judged").verdict, "fail");
}

/// Private APPS mission files: `N7RT_MISSIONS_DIR/apps`, else `N7RT_INSTRUCTOR_DIR/missions/apps`, else, walking up from
/// this crate, `restricted/missions/apps` (single-folder layout) or `n7rt-canlab-missions/missions/apps` (instructor
/// repository checked out next to the student one). May not exist: callers check `pack-v0.json`.
fn private_apps_dir() -> std::path::PathBuf {
    use std::path::PathBuf;
    let has = |p: &PathBuf| p.join("pack-v0.json").is_file();
    for (var, sub) in [("N7RT_MISSIONS_DIR", "apps"), ("N7RT_INSTRUCTOR_DIR", "missions/apps")] {
        if let Ok(d) = std::env::var(var) {
            let p = PathBuf::from(d).join(sub);
            if has(&p) {
                return p;
            }
        }
    }
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    loop {
        for cand in [dir.join("restricted/missions/apps"), dir.join("n7rt-canlab-missions/missions/apps")] {
            if has(&cand) {
                return cand;
            }
        }
        if !dir.pop() {
            return PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("restricted-missions-apps-absent");
        }
    }
}
