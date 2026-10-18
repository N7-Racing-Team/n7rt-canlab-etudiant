//! Mission slice, core side (docs/apps-mission-slice.md §6): check recording through the link engine with a test
//! double of the vehicle, judgement helpers, pack and library discovery, the public statement, the controller without
//! a pack. Tests that need the private pack read it at run time and pass with a note when it is absent; those that
//! open it go through the instructor login and the run judgement (`n7rt_vehicle::judge_run`).
//! The student and instructor modes are tested in mission_modes.rs.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use n7rt_lab::engine::{RecEntry, RecFrame, RecTick};
use n7rt_lab::fake::FakeHandle;
use n7rt_lab::mission::{
    board_ticks, discover, feed_order, load_library, sort_key, statement, Catalog, Feed, MissionConfig, Missions, NO_PACK,
};
use n7rt_lab::InstructorLocator;
use n7rt_lab::model::{FrameRow, LinkState, UiBatch};
use n7rt_lab::phys::{PhysConfig, PhysLab};
use n7rt_lab::sim::{BusSource, SourceFactory, SourceKind, Stimuli};
use n7rt_lab::{Command, Lab, LabConfig};
use n7rt_vehicle::{BusFrame, SlotOut};

fn wait_for(lab: &Lab, what: &str, timeout_ms: u64, cond: impl Fn(&UiBatch) -> bool) -> UiBatch {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    loop {
        let s = lab.snapshot();
        if cond(&s) {
            return s;
        }
        assert!(Instant::now() < deadline, "timeout waiting for {what}: state {:?} « {} »", s.status.state, s.status.message);
        thread::sleep(Duration::from_millis(10));
    }
}

fn start_lab(fake: &FakeHandle) -> Lab {
    let cfg = LabConfig { record: true, stimuli: Stimuli { rate_hz: 0, ..Stimuli::default() }, ..LabConfig::default() };
    Lab::start(cfg, fake.connector())
}

fn active(s: &UiBatch) -> bool {
    s.status.session.as_ref().is_some_and(|x| x.phase == "active")
}

/// Stand-in for the mission vehicle: two identifiers, every 2 ms and every 7 ms.
struct TwoIds {
    next: [u64; 2],
}

impl BusSource for TwoIds {
    fn advance(&mut self, t_us: u64) -> Vec<BusFrame> {
        let mut out = Vec::new();
        loop {
            let (i, t) = if self.next[0] <= self.next[1] { (0, self.next[0]) } else { (1, self.next[1]) };
            if t > t_us {
                return out;
            }
            out.push(BusFrame { t_us: t, id: [0x10, 0x20][i], flags: 0, dlc: 8, data: vec![i as u8; 8] });
            self.next[i] += [2_000, 7_000][i];
        }
    }
}

struct TwoIdsFactory(Arc<AtomicU64>);

impl SourceFactory for TwoIdsFactory {
    fn start(&mut self) -> Box<dyn BusSource> {
        self.0.fetch_add(1, Ordering::Relaxed);
        Box::new(TwoIds { next: [2_000, 7_000] })
    }
}

#[test]
fn check_record_keeps_what_crossed_the_link_and_the_check_session_stays_hidden() {
    let fake = FakeHandle::new();
    let lab = start_lab(&fake);
    wait_for(&lab, "ready", 3000, |s| s.status.state == LinkState::Ready);
    let starts = Arc::new(AtomicU64::new(0));
    lab.set_bus_source(SourceKind::Mission(Box::new(TwoIdsFactory(starts.clone()))));
    lab.check_record_start();
    lab.command(Command::StartSession { tick_period_ms: 10 });
    let s = wait_for(&lab, "active session", 3000, active);
    let sid = s.status.session.as_ref().unwrap().id;

    let mut shown: Vec<FrameRow> = Vec::new();
    let mut slots_seen = 0;
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_millis(700) {
        if let Some(b) = lab.drain() {
            slots_seen += b.slots.len();
            shown.extend(b.rows);
        }
        thread::sleep(Duration::from_millis(20));
    }
    let progress = lab.check_record_progress();
    let rec = lab.check_record_stop();
    lab.command(Command::StopSession);
    wait_for(&lab, "session closed", 3000, |s| s.status.session.is_none());
    while let Some(b) = lab.drain() {
        shown.extend(b.rows);
    }

    assert_eq!(starts.load(Ordering::Relaxed), 1);
    assert_eq!(rec.session_id, Some(sid));
    assert!(!rec.overflow);
    assert!(progress.frames > 0 && progress.ticks > 0 && progress.frames <= rec.frames.len());
    assert!(rec.frames.len() >= 200, "{} frames in ~700 ms", rec.frames.len());
    assert!(rec.frames.windows(2).all(|w| w[0].t_us <= w[1].t_us), "non-decreasing bus time");
    assert!(rec.frames.iter().all(|f| (f.id == 0x10 || f.id == 0x20) && f.flags == 0 && f.data.len() == 8));
    assert!(rec.ticks.len() >= 40, "{} ticks", rec.ticks.len());
    assert!(rec.ticks.iter().enumerate().all(|(i, t)| t.t_us == (i as u64 + 1) * 10_000 && t.status == 0), "every tick once, in order");
    // The check session is never shown: neither its frames nor its slots.
    assert!(shown.iter().all(|r| r.session_id != sid), "a row of the check session reached the UI");
    assert_eq!(slots_seen, 0);
    let st = lab.snapshot().stats;
    assert_eq!((st.lost, st.nacks), (0, 0), "{st:?}");

    // The next session (not recorded) is shown again, with the same source restarted at t = 0.
    lab.command(Command::StartSession { tick_period_ms: 10 });
    let s = wait_for(&lab, "second session", 3000, active);
    let second = s.status.session.as_ref().unwrap().id;
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut rows = Vec::new();
    while rows.is_empty() && Instant::now() < deadline {
        rows.extend(lab.drain().map(|b| b.rows).unwrap_or_default().into_iter().filter(|r| r.session_id == second));
        thread::sleep(Duration::from_millis(20));
    }
    assert!(!rows.is_empty(), "rows of a normal session are shown");
    assert_eq!(starts.load(Ordering::Relaxed), 2);
    assert!(lab.check_record_stop().frames.is_empty(), "nothing recorded after the stop");
    lab.shutdown();
}

fn frame(t_us: u64) -> RecFrame {
    RecFrame { t_us, id: 0x55, flags: 0, data: vec![1] }
}

fn tick(t_us: u64, entries: Vec<RecEntry>) -> RecTick {
    RecTick { t_us, status: 0, entries }
}

#[test]
fn reference_feed_puts_ticks_before_frames_of_the_same_time() {
    let frames = [frame(3_000), frame(10_000), frame(10_000), frame(25_000), frame(31_000)];
    let order = feed_order(&frames, 30_000, 10_000);
    assert_eq!(
        order,
        [Feed::Frame(0), Feed::Tick(10_000), Feed::Frame(1), Feed::Frame(2), Feed::Tick(20_000), Feed::Frame(3), Feed::Tick(30_000)]
    );
}

#[test]
fn board_ticks_must_be_complete() {
    let v = |slot, value| RecEntry { slot, kind: 0, value };
    let full: Vec<RecTick> = (1..=4).map(|k| tick(k * 10_000, vec![v(0, k as f32), RecEntry { slot: 2, kind: 1, value: 0.0 }])).collect();
    let b = board_ticks(&full, 30_000, 10_000).expect("complete up to the duration");
    assert_eq!(b.len(), 3);
    assert_eq!(b[1].t_us, 20_000);
    assert_eq!(b[1].slots.len(), 8);
    assert_eq!(b[1].slots[0], Some(SlotOut::Value(2.0)));
    assert_eq!(b[1].slots[1], None);
    assert_eq!(b[1].slots[2], Some(SlotOut::Reject));
    let mut missing = full.clone();
    missing.remove(1);
    assert!(board_ticks(&missing, 30_000, 10_000).is_none(), "a lost tick cancels the check");
    let mut doubled = full.clone();
    doubled.insert(1, tick(10_000, vec![]));
    assert!(board_ticks(&doubled, 30_000, 10_000).is_none());
    let not_delivered = vec![RecTick { t_us: 10_000, status: 1, entries: vec![v(0, 5.0)] }];
    assert!(board_ticks(&not_delivered, 10_000, 10_000).unwrap()[0].slots.iter().all(Option::is_none));
}

// ------------------------------------------------------------------------------------------- discovery, library

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> TempDir {
        static N: AtomicU64 = AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!("n7rt-mission-{tag}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
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

#[test]
fn packs_and_library_are_found_walking_up() {
    let t = TempDir::new("discover");
    let missions = t.0.join("restricted/missions");
    std::fs::create_dir_all(missions.join("apps")).unwrap();
    std::fs::create_dir_all(t.0.join("library")).unwrap();
    let deep = t.0.join("app/target/debug");
    std::fs::create_dir_all(&deep).unwrap();
    let elsewhere = TempDir::new("elsewhere");

    assert_eq!(discover(None, "restricted/missions", &[deep.clone()]), Some(missions.clone()));
    assert_eq!(discover(None, "library", &[elsewhere.0.clone(), deep.clone()]), Some(t.0.join("library")), "second start");
    assert_eq!(discover(None, "restricted/missions", &[elsewhere.0.clone()]), None);
    // The variable wins, and names the directory itself.
    let other = elsewhere.0.join("packs");
    std::fs::create_dir_all(&other).unwrap();
    assert_eq!(discover(Some(other.clone().into_os_string()), "restricted/missions", &[deep.clone()]), Some(other));
    assert_eq!(discover(Some(elsewhere.0.join("absent").into_os_string()), "restricted/missions", &[deep.clone()]), None);
    assert_eq!(discover(Some("".into()), "restricted/missions", &[deep]), Some(missions));
}

#[test]
fn mission_list_says_why_a_mission_is_unavailable() {
    let none = Catalog::default().list();
    assert_eq!(none.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["apps", "wsf", "sas", "bps"]);
    let apps = &none[0];
    assert_eq!(apps.title, "APPS — pédale d'accélérateur");
    assert!(!apps.available);
    assert_eq!(apps.reason.as_deref(), Some(NO_PACK));
    assert!(none[1..].iter().all(|m| !m.available && m.reason.is_some()));

    let t = TempDir::new("list");
    std::fs::create_dir_all(t.0.join("apps")).unwrap();
    let catalog = Catalog { missions_dir: Some(t.0.clone()), library_dir: None };
    assert_eq!(catalog.list()[0].reason.as_deref(), Some(NO_PACK), "directory without a pack file");
    std::fs::write(t.0.join("apps/pack-v0.json"), "{}").unwrap();
    let entry = &catalog.list()[0];
    assert!(!entry.available);
    assert!(entry.reason.as_deref().unwrap().starts_with("pack de mission illisible"), "{entry:?}");
}

fn write(p: &Path, text: &str) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

#[test]
fn library_is_one_flat_list_sorted_by_title() {
    let t = TempDir::new("library");
    let entry = |title: &str, file: &str, missions: &str| {
        format!(
            r#"{{"title":"{title}","publisher":"P","reference":"R","revision":"1","date":null,"primary_url":"https://example.org/{file}","sha256":"{}","missions":[{missions}],"local_file":"{file}"}}"#,
            "0".repeat(64)
        )
    };
    write(&t.0.join("apps/manifest.json"), &format!("[{}]", entry("Zeta datasheet", "z.pdf", r#""APPS""#)));
    write(
        &t.0.join("common/manifest.json"),
        &format!("[{},{}]", entry("Écrou manual", "e.pdf", r#""APPS","WSF""#), entry("Only WSF", "w.pdf", r#""WSF""#)),
    );
    write(&t.0.join("sas/manifest.json"), &format!("[{}]", entry("Other mission", "s.pdf", r#""SAS""#)));
    write(
        &t.0.join("links.json"),
        r#"[{"title":"alpha rules","publisher":"FSG","reference":"X","revision":"v1","date":"2025-11-25","primary_url":"https://example.org/a","missions":["APPS","BPS"]},
            {"title":"Beta","publisher":"Q","reference":"Y","revision":"v2","date":null,"primary_url":"https://example.org/b","missions":["SAS"]}]"#,
    );
    let lib = load_library(&t.0, "apps");
    let titles: Vec<&str> = lib.iter().map(|e| e.item.title.as_str()).collect();
    assert_eq!(titles, ["alpha rules", "Écrou manual", "Zeta datasheet"]);
    assert!(lib[0].local.is_none() && lib[0].url.as_deref() == Some("https://example.org/a"));
    assert_eq!(lib[1].local.as_ref().unwrap().0, t.0.join("common/e.pdf"));
    // What the UI receives: bibliographic fields only.
    let json = serde_json::to_value(&lib[1].item).unwrap();
    let keys: BTreeSet<&str> = json.as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(keys, BTreeSet::from(["title", "publisher", "reference", "revision", "date"]));
    assert!(load_library(&t.0.join("absent"), "apps").is_empty());
    assert!(sort_key("Écart") < sort_key("ecrou") && sort_key("Œuvre") == "oeuvre");
}

#[test]
fn the_public_statement_names_the_slots_and_nothing_of_the_encoding() {
    let s = statement("apps").expect("shared/missions/apps/enonce.json");
    assert_eq!((s.format.as_str(), s.version, s.mission.as_str()), ("n7rt-mission-enonce", 1, "apps"));
    assert_eq!(s.title, "APPS — pédale d'accélérateur");
    assert_eq!(s.slots.len(), 8);
    for (i, slot) in s.slots.iter().enumerate() {
        assert_eq!(slot.slot as usize, i);
        assert_eq!(slot.code, format!("A{}", i + 1));
        assert!(!slot.label.is_empty() && !slot.unit.is_empty());
    }
    assert!(!s.summary.is_empty() && s.requirements.len() >= 4);
    let all = format!("{} {} {:?}", s.summary, s.requirements.join(" "), s.slots).to_lowercase();
    for banned in ["0x", "e2e", "p22", "data id", "crc", "facteur", "offset", "octet", "bit ", "identifiant", "sna", "profil"] {
        assert!(!all.contains(banned), "the public statement mentions « {banned} »");
    }
    for (mission, count) in [("wsf", 8), ("sas", 5), ("bps", 6)] {
        let s = statement(mission).expect("public mission statement");
        assert_eq!(s.mission, mission);
        assert_eq!(s.slots.len(), count);
        assert!(s.slots.iter().enumerate().all(|(n, s)| s.slot as usize == n));
        let all = format!("{} {} {:?}", s.summary, s.requirements.join(" "), s.slots).to_lowercase();
        for banned in ["0x", "data id", "golden", "octet", "facteur", "offset", "oracle"] {
            assert!(!all.contains(banned), "public statement leaks encoding or reference: {mission}/{banned}");
        }
    }
}

// ------------------------------------------------------------------------------------------------- controller

/// A controller in student mode; `instructor` is the instructor directory found at login (none: login fails).
fn controller(app_data: &Path, instructor: Option<&Path>) -> (Arc<Lab>, Arc<PhysLab>, Missions, FakeHandle) {
    let fake = FakeHandle::new();
    let lab = Arc::new(start_lab(&fake));
    let phys = Arc::new(PhysLab::start(PhysConfig::default()));
    let cfg = MissionConfig {
        fixed_seed: Some(7),
        app_data_dir: Some(app_data.to_path_buf()),
        documents_dir: Some(app_data.join("Documents")),
        instructor: InstructorLocator::fixed(instructor.map(Path::to_path_buf).unwrap_or_else(|| app_data.join("absent"))),
        failed_login_delay: Some(Duration::ZERO),
        ..MissionConfig::default()
    };
    let missions = Missions::start(lab.clone(), phys.clone(), cfg);
    (lab, phys, missions, fake)
}

#[test]
fn without_a_pack_the_mission_does_not_open_and_nothing_changes() {
    let data = TempDir::new("nopack");
    let (lab, phys, missions, _fake) = controller(&data.0, None);
    // Student mode, nothing chosen: no mission opens directly, whatever the identifier.
    assert_eq!(missions.open("apps").unwrap_err(), "Choisissez d'abord la mission de la session.");
    assert!(missions.open("xyz").is_err());
    assert!(missions.open("wsf").is_err());
    let v = missions.view();
    assert!(!v.open && v.id.is_none() && v.statement.is_none() && v.library.is_empty() && v.tick_period_ms.is_none());
    assert!(v.kind.is_none() && v.group.is_none());
    assert_eq!((v.check.state, v.check.verdict, v.check.reason, v.check.run_file.as_deref()), ("idle", None, None, None));
    assert_eq!(missions.check().unwrap_err(), "Ouvrez d'abord une mission.");
    assert!(missions.library_open(0).is_err());
    assert!(!missions.close().unwrap().open);
    // The view serialises with the field names the UI reads.
    let json = serde_json::to_value(&v).unwrap();
    for k in ["id", "title", "open", "kind", "group", "statement", "library", "check", "tick_period_ms"] {
        assert!(json.get(k).is_some(), "{k}");
    }
    for k in ["state", "elapsed_s", "total_s", "verdict", "reason", "run_file"] {
        assert!(json["check"].get(k).is_some(), "check.{k}");
    }
    missions.shutdown();
    lab.shutdown();
    phys.shutdown();
}

// ------------------------------------------------------------------------- with the private pack (lots V and B)

const CODE: &str = "TEST-INSTRUCTOR-CODE";

/// A temporary instructor directory: `instructor/auth.json` for [`CODE`] and the private pack (its files, bench
/// included) under `missions/apps/`, where the reports are written; None with a note when the pack is absent.
fn instructor_dir() -> Option<TempDir> {
    let src = private_apps_dir();
    if !src.join("pack-v0.json").is_file() {
        println!("note: restricted/missions/apps/pack-v0.json absent: test skipped");
        return None;
    }
    let t = TempDir::new("instructor");
    std::fs::create_dir_all(t.0.join("missions/apps")).unwrap();
    for f in std::fs::read_dir(&src).unwrap().flatten().filter(|f| f.path().is_file()) {
        std::fs::copy(f.path(), t.0.join("missions/apps").join(f.file_name())).unwrap();
    }
    let phc = n7rt_seal::hash_code(CODE).expect("hash");
    let auth = serde_json::json!({"format": "n7rt-instructor-auth", "version": 1, "code_phc": phc});
    write(&t.0.join("instructor/auth.json"), &auth.to_string());
    Some(t)
}

#[test]
fn opened_mission_puts_the_vehicle_on_the_bus() {
    let Some(dir) = instructor_dir() else { return };
    let data = TempDir::new("data");
    let (lab, phys, missions, _fake) = controller(&data.0, Some(&dir.0));
    wait_for(&lab, "ready", 3000, |s| s.status.state == LinkState::Ready);
    missions.instructor_login(CODE).expect("instructor login");
    assert!(missions.list()[0].available, "reference pack listed");
    let v = missions.open("apps").expect("mission opens");
    assert_eq!((v.kind, v.group.as_deref()), (Some("reference"), None));
    assert!(v.open && v.tick_period_ms == Some(10) && v.statement.as_ref().unwrap().slots.len() == 8);
    lab.command(Command::StartSession { tick_period_ms: 10 });
    wait_for(&lab, "active session", 3000, active);
    let mut ids = BTreeSet::new();
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_secs(3) {
        if let Some(b) = lab.drain() {
            ids.extend(b.rows.iter().filter(|r| r.outcome == "result").map(|r| r.can_id));
        }
        thread::sleep(Duration::from_millis(20));
    }
    assert!(ids.len() >= 17, "{} identifiers on the bus", ids.len());
    let st = lab.snapshot().stats;
    assert_eq!((st.lost, st.nacks), (0, 0));
    assert!(!missions.close().unwrap().open);
    missions.shutdown();
    lab.shutdown();
    phys.shutdown();
}

#[test]
fn nominal_check_against_the_fake_board_is_judged_and_reported() {
    let Some(dir) = instructor_dir() else { return };
    let data = TempDir::new("data");
    let (lab, phys, missions, _fake) = controller(&data.0, Some(&dir.0));
    wait_for(&lab, "ready", 3000, |s| s.status.state == LinkState::Ready);
    missions.instructor_login(CODE).expect("instructor login");
    missions.open("apps").expect("mission opens");
    if !n7rt_vehicle::ORACLE {
        // public edition: the instructor's nominal check gives no verdict, and says so
        assert_eq!(missions.check().unwrap_err(), n7rt_lab::mission::ORACLE_ABSENT);
        return;
    }
    let v = missions.check().expect("check starts");
    assert_eq!(v.check.state, "running");
    let total = v.check.total_s;
    assert!((total - 9.0).abs() < 1e-9, "ch. 24 §2.9.6: 9.0 s, got {total}");
    let deadline = Instant::now() + Duration::from_secs_f64(total + 30.0);
    let v = loop {
        let v = missions.view();
        if v.check.state != "running" {
            break v;
        }
        assert!(Instant::now() < deadline, "check still running");
        thread::sleep(Duration::from_millis(100));
    };
    // The fake board publishes nothing at its ticks: the example decoder fails the nominal check.
    assert_eq!((v.check.state, v.check.verdict), ("done", Some("fail")), "{:?}", v.check);
    assert!(v.check.run_file.is_none(), "the instructor gets a verdict, no run file");
    let reports: Vec<_> = std::fs::read_dir(dir.0.join("missions/apps/reports")).unwrap().flatten().collect();
    assert_eq!(reports.len(), 1);
    let report: serde_json::Value = serde_json::from_slice(&std::fs::read(reports[0].path()).unwrap()).unwrap();
    assert_eq!(report["result"], "fail");
    assert!(report["frames"].as_u64().unwrap() > 1000 && report["ticks"] == 900);
    // Back to a normal session with the live slider.
    wait_for(&lab, "normal session", 5000, active);
    missions.shutdown();
    lab.shutdown();
    phys.shutdown();
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
