//! Mission controller (docs/apps-mission-slice.md §2, §6; docs/distribution.md §2): mission list, the student and
//! instructor modes, opening (group pack or reference pack, Physical Lab bench, bus source switched to the mission
//! vehicle), closing (back to the training ECU), the hidden nominal check (ch. 24 §2.9.6) and the public library.
//!
//! Modes. Student (default): the mission of the session is chosen once and locked in `<app data>/student/state.json`;
//! the group pack (`.n7rtpack`, sealed) is imported as it is, opened with the group code (key and plaintext in memory
//! only); the nominal check records a run file (`Documents\N7RT CAN Lab\essais\*.n7rtrun`) and gives no verdict.
//! Instructor (after the instructor code, verified against `instructor/auth.json` of the instructor directory; memory
//! only): every mission with its reference pack, the nominal check with its verdict, the reset of the student lock,
//! the group packs of a cohort and the judgement of a run file. Student mode never reads the instructor directory.
//!
//! Secrecy (architecture §9.3): packs are read at run time, never compiled in; what leaves this module for the UI is
//! the public statement (`shared/missions/<id>/enonce.json`), the bibliographic fields of the library, the clear
//! header fields of a group pack (cohort, group) and a check state (« réussi / non réussi » for the instructor, a run
//! file name for the student). The detailed verdict goes to the trainer's report (`<instructor>/missions/<id>/reports/`)
//! only. Nothing here names a frame or a signal.
//!
//! The nominal check: stop the session, new vehicle with a fresh seed, the scenario drives the sensor position, new
//! session at t = 0, record what crosses the link (`Lab::check_record_*`), then build the run record (frames as
//! delivered, the board's `TICK_RESULT`s) and judge it with `n7rt_vehicle::judge_run` (instructor, the same judgement
//! as the instructor tool) or write it as a run file (student). A lost board, a mute board (`STALLED`), a NACK, a lost
//! frame or tick, or a link error cancels the check (« annulé »), never a fail.

use std::collections::HashMap;
use std::ffi::OsString;
use std::io::{self, Read};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use n7rt_circuit::{Circuit, CompId, Interact, PinRef, Value, WireId};
use n7rt_seal::{normalize_code, open_pack, read_pack_header, verify_code, PackHeader, SealError};
use n7rt_vehicle::{
    judge_run, BoardTick, BusFrame, EcuInputs, FrameFault, NominalScenario, Pack, RunFrame, RunRecord, RunSlot, RunTick, SlotOut,
    Vehicle, Verdict,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::access::{
    read_auth, run_file_name, safe_name, unique_path, wipe, InstructorLocator, Mode, PackRef, StudentState, StudentStore,
    MAX_FILE_BYTES, MSG_NO_INSTRUCTOR, MSG_WRONG_GROUP_CODE, MSG_WRONG_INSTRUCTOR_CODE, PACK_EXT, RUNS_SUBDIR,
};
use crate::cohort::{GenRow, Judgement};
use crate::engine::{CheckRecord, Command, Lab, RecFrame, RecTick};
use crate::model::{LinkState, UiBatch};
use crate::phys::{CircuitProbe, Disclosure, MeterFn, PhysCommand, PhysLab, Reading, SystemOp};
use crate::sim::{BusSource, SourceFactory, SourceKind};
use crate::workflow::compare::{compare_competences, compare_ticks};
use crate::workflow::fiche::NominalPart;
use crate::workflow::incident::{self, Apply, Conditions, IncidentContent, IncidentPart, MeterRecord, PinName, P4};
use crate::workflow::measure::{self, Anomaly, Preuve, PreuveFrame};
use crate::workflow::pcheck;
use crate::workflow::progress::{Active, AttemptVerdict, Outcome as StepOutcome, Owner};
use crate::workflow::replay::ReplayFactory;
use crate::workflow::sealed;
use crate::workflow::session::{self, DocumentChoice, HiddenTestRef, ProgressionView, Session};

pub const LIBRARY_ENV: &str = "N7RT_LIBRARY_DIR";
pub const PACK_FILE: &str = "pack-v0.json";
const STATEMENT_WSF: &str = include_str!("../../../../shared/missions/wsf/enonce.json");
const STATEMENT_SAS: &str = include_str!("../../../../shared/missions/sas/enonce.json");
const STATEMENT_BPS: &str = include_str!("../../../../shared/missions/bps/enonce.json");
const STATEMENT_APPS: &str = include_str!("../../../../shared/missions/apps/enonce.json");

/// Reason of an unavailable APPS mission (UI title of the disabled entry).
pub const NO_PACK: &str = "pack de mission introuvable";
const UNREADABLE: &str = "pack de mission illisible";
pub const NOT_YET: &str = "mission pas encore disponible dans cette version";
/// ch. 24 §7.3 M-06.
pub const BOARD_NOT_READY: &str = "Connectez la carte et vérifiez que votre programme est chargé.";
/// Instructor check or judgement in a build without the oracle (docs/distribution.md §1).
pub const ORACLE_ABSENT: &str = "Verdict réservé à l'édition encadrant : cette version de l'application ne contient pas \
                                 l'oracle (archive des outils encadrant, ou n7rt-tool judge de cette archive).";
/// ch. 24 §7.3 M-11 reasons.
pub const CANCEL_BOARD: &str = "carte déconnectée";
pub const CANCEL_APP: &str = "application fermée";
pub const CANCEL_INCOMPLETE: &str = "essai incomplet";
/// The student's run file could not be written.
pub const CANCEL_WRITE: &str = "fichier d'essai non enregistré";
const RUNNING: &str = "Vérification en cours : attendez la fin de l'essai.";
const INSTRUCTOR_ONLY: &str = "Réservé au mode encadrant.";
const STUDENT_ONLY: &str = "Quittez le mode encadrant pour agir sur la session étudiante.";
const CHOOSE_FIRST: &str = "Choisissez d'abord la mission de la session.";
const IMPORT_FIRST: &str = "Importez d'abord le pack du groupe.";
const NOT_A_PACK: &str = "Ce fichier n'est pas un pack de groupe N7RT.";
const OTHER_MISSION_PACK: &str = "Ce pack est destiné à une autre mission.";
const PACK_UNREADABLE: &str = "Pack du groupe illisible : demandez un nouveau fichier à votre encadrant.";
const NO_PROGRESSION: &str = "Ce pack de groupe n'a pas de progression de mission.";
/// ch. 24 §7.3 M-08.
const BENCH_NOT_READY: &str = "Le banc n'est pas prêt : attendez la fin du chargement du véhicule.";
/// Another document replaced the mission bench in the Physical Lab (« Nouveau », « Ouvrir », « Démonstration »).
const BENCH_LEFT: &str = "Le banc de la mission n'est plus ouvert dans le Physical Lab : rouvrez la mission avec le code \
                          du groupe, votre banc revient tel que vous l'aviez laissé.";
const FAILED_LOGIN_DELAY: Duration = Duration::from_secs(1);

/// « La mission de cette session est déjà choisie : APPS. »
pub fn locked_message(mission: &str) -> String {
    format!("La mission de cette session est déjà choisie : {}.", mission.to_uppercase())
}

/// The ECU pins read for LV_IN (docs/apps-mission-slice.md §5).
const PIN_LV_IN: &str = "lv_in";
const PIN_GND: &str = "gnd";
const POLL: Duration = Duration::from_millis(50);
const SESSION_WAIT: Duration = Duration::from_secs(5);
/// Real-time budget of a check beyond its bus duration (backpressure, in-flight answers).
const CHECK_SLACK: Duration = Duration::from_secs(30);
/// After the last expected tick: longer than the engine's grace before an overtaken frame is counted lost (300 ms), so
/// that the last loss check sees every frame of the check.
const CHECK_GRACE: Duration = Duration::from_millis(600);

struct Known {
    id: &'static str,
    title: &'static str,
    playable: bool,
}

const KNOWN: [Known; 4] = [
    Known { id: "apps", title: "APPS — pédale d'accélérateur", playable: true },
    Known { id: "wsf", title: "WSF — vitesse des roues", playable: true },
    Known { id: "sas", title: "SAS — angle du volant", playable: true },
    Known { id: "bps", title: "BPS — pression de freinage", playable: true },
];

fn known(id: &str) -> Option<&'static Known> {
    KNOWN.iter().find(|k| k.id == id)
}

// --------------------------------------------------------------------------------------------------------- DTOs

/// An entry of the mission selector.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct MissionEntry {
    pub id: String,
    pub title: String,
    pub available: bool,
    /// Why the mission cannot be opened (French), when unavailable.
    pub reason: Option<String>,
}

/// A public slot of the statement (ch. 10 §7.2): `slot` is the index published by the student code.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SlotLabel {
    pub slot: u8,
    pub code: String,
    pub label: String,
    pub unit: String,
}

/// `shared/missions/<id>/enonce.json`.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Statement {
    pub format: String,
    pub version: u32,
    pub mission: String,
    pub title: String,
    pub summary: String,
    pub requirements: Vec<String>,
    pub slots: Vec<SlotLabel>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct StatementView {
    pub summary: String,
    pub requirements: Vec<String>,
    pub slots: Vec<SlotLabel>,
}

/// Bibliographic fields only (ch. 24 §7.6): no role, no file, no address.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct LibraryItem {
    pub title: String,
    pub publisher: String,
    pub reference: String,
    pub revision: String,
    pub date: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CheckView {
    /// "idle" | "running" | "done" (instructor verdict) | "recorded" (student run file) | "cancelled"
    pub state: &'static str,
    pub elapsed_s: f64,
    pub total_s: f64,
    /// "pass" | "fail" once done (instructor only).
    pub verdict: Option<&'static str>,
    /// Why a check was cancelled (ch. 24 §7.3 M-11): « carte déconnectée », « application fermée », « essai incomplet ».
    pub reason: Option<&'static str>,
    /// Name of the run file once recorded (student).
    pub run_file: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct MissionView {
    pub id: Option<String>,
    pub title: Option<String>,
    pub open: bool,
    /// "reference" (instructor) | "group" (student pack), when open.
    pub kind: Option<&'static str>,
    /// Group of the open student pack.
    pub group: Option<String>,
    pub statement: Option<StatementView>,
    /// Sorted by title; `library_open(index)` opens an entry.
    pub library: Vec<LibraryItem>,
    pub check: CheckView,
    /// Tick period of the mission's sessions (public: ch. 10 §7.2), when open.
    pub tick_period_ms: Option<u16>,
    /// Student progression of a release 0.3 group pack (steps, work order, messages); `None` for a 0.2 pack or the
    /// instructor's reference pack.
    pub progression: Option<ProgressionView>,
}

/// The imported group pack, as the student sees it (clear header fields).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PackInfo {
    pub cohort: String,
    pub group: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct StudentView {
    /// Mission of the session, once chosen (locked).
    pub locked: Option<String>,
    pub locked_title: Option<String>,
    pub pack: Option<PackInfo>,
    /// Name of the last run file written by this application.
    pub last_run: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AccessView {
    pub mode: Mode,
    pub student: StudentView,
    /// Edition of the build (`n7rt_vehicle::EDITION`): the public edition gives no verdict.
    pub edition: &'static str,
    /// The build contains the oracle: instructor checks and judgements give a verdict.
    pub oracle: bool,
}

// ---------------------------------------------------------------------------------------------------- discovery

/// `env` when set (the directory itself, None if it does not exist), else the first `<ancestor>/<rel>` directory
/// walking up from each start in turn.
pub fn discover(env: Option<OsString>, rel: &str, starts: &[PathBuf]) -> Option<PathBuf> {
    if let Some(v) = env.filter(|v| !v.is_empty()) {
        let p = PathBuf::from(v);
        return p.is_dir().then_some(p);
    }
    starts.iter().flat_map(|s| s.ancestors()).map(|a| a.join(rel)).find(|p| p.is_dir())
}

/// The executable's directory, then the current directory.
pub fn default_starts() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
        v.push(dir);
    }
    if let Ok(cwd) = std::env::current_dir() {
        v.push(cwd);
    }
    v
}

/// Calls into the mission engine (lot V), the seal and the Physical Lab are guarded: a panic is an error, never a
/// crash.
pub(crate) fn guarded<T>(f: impl FnOnce() -> T) -> Result<T, ()> {
    catch_unwind(AssertUnwindSafe(f)).map_err(|_| ())
}

fn clip(s: &str, n: usize) -> String {
    match s.char_indices().nth(n) {
        Some((i, _)) => format!("{}…", &s[..i]),
        None => s.to_owned(),
    }
}

/// Where the reference packs (instructor directory, instructor mode only) and the public library are.
#[derive(Debug, Clone, Default)]
pub struct Catalog {
    pub missions_dir: Option<PathBuf>,
    pub library_dir: Option<PathBuf>,
}

impl Catalog {
    /// The public library: `N7RT_LIBRARY_DIR`, else the first `library` walking up from the executable, then from the
    /// current directory. Reference packs are never discovered: they come from the instructor directory after login.
    pub fn discover() -> Catalog {
        Catalog { missions_dir: None, library_dir: discover(std::env::var_os(LIBRARY_ENV), "library", &default_starts()) }
    }

    fn pack_path(&self, id: &str) -> Option<PathBuf> {
        self.missions_dir.as_ref().map(|d| d.join(id).join(PACK_FILE)).filter(|p| p.is_file())
    }

    /// The pack of a mission, or why it cannot be used (French, no mission value).
    pub fn load_pack(&self, id: &str) -> Result<Pack, String> {
        let path = self.pack_path(id).ok_or_else(|| NO_PACK.to_owned())?;
        let pack = match guarded(|| Pack::load(&path)) {
            Ok(Ok(p)) => p,
            Ok(Err(e)) => return Err(format!("{UNREADABLE} : {}", clip(&e, 200))),
            Err(()) => return Err(format!("{UNREADABLE} : erreur interne du moteur véhicule")),
        };
        match guarded(|| (pack.mission().eq_ignore_ascii_case(id), pack.bench_json().trim().is_empty())) {
            Ok((true, false)) => Ok(pack),
            Ok((false, _)) => Err(format!("{UNREADABLE} : pack d'une autre mission")),
            Ok((true, true)) => Err(format!("{UNREADABLE} : banc de la mission absent")),
            Err(()) => Err(format!("{UNREADABLE} : erreur interne du moteur véhicule")),
        }
    }

    /// Every known mission; only those whose pack loads are available (instructor list).
    pub fn list(&self) -> Vec<MissionEntry> {
        KNOWN
            .iter()
            .map(|k| {
                let reason = if k.playable { self.load_pack(k.id).err() } else { Some(NOT_YET.to_owned()) };
                MissionEntry { id: k.id.into(), title: k.title.into(), available: reason.is_none(), reason }
            })
            .collect()
    }

    /// The mission's library, sorted by title (empty without a library directory).
    pub fn library(&self, id: &str) -> Vec<LibEntry> {
        self.library_dir.as_deref().map(|d| load_library(d, id)).unwrap_or_default()
    }
}

/// The student list: every known mission, the playable ones can be chosen (their pack comes from the instructor).
pub fn student_list() -> Vec<MissionEntry> {
    KNOWN
        .iter()
        .map(|k| MissionEntry {
            id: k.id.into(),
            title: k.title.into(),
            available: k.playable,
            reason: (!k.playable).then(|| NOT_YET.to_owned()),
        })
        .collect()
}

/// The public statement of a mission, from `shared/missions/<id>/enonce.json` (embedded: public data).
pub fn statement(id: &str) -> Option<&'static Statement> {
    static WSF: OnceLock<Option<Statement>> = OnceLock::new();
    static SAS: OnceLock<Option<Statement>> = OnceLock::new();
    static BPS: OnceLock<Option<Statement>> = OnceLock::new();
    static APPS: OnceLock<Option<Statement>> = OnceLock::new();
    match id {
        "apps" => APPS.get_or_init(|| serde_json::from_str(STATEMENT_APPS).ok()).as_ref(),
        "wsf" => WSF.get_or_init(|| serde_json::from_str(STATEMENT_WSF).ok()).as_ref(),
        "sas" => SAS.get_or_init(|| serde_json::from_str(STATEMENT_SAS).ok()).as_ref(),
        "bps" => BPS.get_or_init(|| serde_json::from_str(STATEMENT_BPS).ok()).as_ref(),
        _ => None,
    }
}

// ------------------------------------------------------------------------------------------------------ library

#[derive(Deserialize)]
struct ManifestEntry {
    title: String,
    publisher: String,
    reference: String,
    revision: String,
    date: Option<String>,
    primary_url: Option<String>,
    sha256: String,
    missions: Vec<String>,
    local_file: String,
}

#[derive(Deserialize)]
struct LinkEntry {
    title: String,
    publisher: String,
    reference: String,
    revision: String,
    date: Option<String>,
    primary_url: String,
    missions: Vec<String>,
}

/// A library document: what the UI shows, and how to open it (never sent to the UI).
#[derive(Debug, Clone, PartialEq)]
pub struct LibEntry {
    pub item: LibraryItem,
    /// Local copy and its manifest SHA-256.
    pub local: Option<(PathBuf, String)>,
    pub url: Option<String>,
}

/// French order of titles, ignoring case and accents (ch. 24 §7.6).
pub fn sort_key(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars().flat_map(char::to_lowercase) {
        match c {
            'à' | 'á' | 'â' | 'ä' | 'ã' | 'å' => out.push('a'),
            'ç' => out.push('c'),
            'è' | 'é' | 'ê' | 'ë' => out.push('e'),
            'ì' | 'í' | 'î' | 'ï' => out.push('i'),
            'ñ' => out.push('n'),
            'ò' | 'ó' | 'ô' | 'ö' | 'õ' => out.push('o'),
            'ù' | 'ú' | 'û' | 'ü' => out.push('u'),
            'ý' | 'ÿ' => out.push('y'),
            'œ' => out.push_str("oe"),
            'æ' => out.push_str("ae"),
            c => out.push(c),
        }
    }
    out
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

/// `<dir>/<id>/manifest.json`, the entries of `<dir>/common/manifest.json` for the mission, and the documents cited by
/// link (`<dir>/links.json`) for the mission: one flat list sorted by title. Unreadable files are skipped.
pub fn load_library(dir: &Path, id: &str) -> Vec<LibEntry> {
    let mission = id.to_ascii_uppercase();
    let item = |title: String, publisher: String, reference: String, revision: String, date: Option<String>| LibraryItem {
        title,
        publisher,
        reference,
        revision,
        date,
    };
    let mut out = Vec::new();
    for (folder, all) in [(id, true), ("common", false)] {
        let path = dir.join(folder).join("manifest.json");
        for m in read_json::<Vec<ManifestEntry>>(&path).unwrap_or_default() {
            if !all && !m.missions.contains(&mission) {
                continue;
            }
            out.push(LibEntry {
                local: Some((dir.join(folder).join(&m.local_file), m.sha256)),
                url: m.primary_url,
                item: item(m.title, m.publisher, m.reference, m.revision, m.date),
            });
        }
    }
    for l in read_json::<Vec<LinkEntry>>(&dir.join("links.json")).unwrap_or_default() {
        if l.missions.contains(&mission) {
            out.push(LibEntry { local: None, url: Some(l.primary_url), item: item(l.title, l.publisher, l.reference, l.revision, l.date) });
        }
    }
    out.sort_by(|a, b| sort_key(&a.item.title).cmp(&sort_key(&b.item.title)).then_with(|| a.item.title.cmp(&b.item.title)));
    out
}

fn sha256_file(path: &Path) -> Option<String> {
    let mut f = std::fs::File::open(path).ok()?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Some(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Opens a local file or folder with the default viewer, or an address with the default browser.
fn open_external(target: &str) -> io::Result<()> {
    if target.contains('"') || target.chars().any(char::is_control) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "target"));
    }
    #[cfg(windows)]
    let mut child = {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        std::process::Command::new("cmd").raw_arg(format!("/C start \"\" \"{target}\"")).creation_flags(CREATE_NO_WINDOW).spawn()?
    };
    #[cfg(target_os = "macos")]
    let mut child = std::process::Command::new("open").arg(target).spawn()?;
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut child = std::process::Command::new("xdg-open").arg(target).spawn()?;
    thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

// ------------------------------------------------------------------------------------------- bus source (vehicle)

/// What the source ECU samples, read from the latest Physical Lab frame.
#[derive(Clone)]
struct LiveInputs {
    phys: Arc<PhysLab>,
    ecu: CompId,
    channels: Vec<String>,
}

/// Sets the sensor position imposed by the nominal scenario, only when its quantized value (1e-3) changes.
struct PositionDriver {
    scenario: NominalScenario,
    phys: Arc<PhysLab>,
    sensor: CompId,
    last: Option<i64>,
    ranges: std::collections::BTreeMap<String, [f64; 2]>,
}

impl PositionDriver {
    fn drive(&mut self, t_us: u64) {
        let p = self.scenario.position_at(t_us);
        let q = if p.is_finite() { (p.clamp(0.0, 1.0) * 1000.0).round() as i64 } else { 0 };
        if self.last != Some(q) {
            self.last = Some(q);
            set_position(&self.phys, self.sensor, &self.ranges, q as f64 / 1000.0).expect("physical trajectory refused");
        }
    }
}

/// Internal command: applies under the check lock, never shown in the student's journal.
fn set_position(phys: &PhysLab, sensor: CompId, ranges: &std::collections::BTreeMap<String, [f64; 2]>, position: f64) -> Result<(), ()> {
    if ranges.is_empty() {
        guarded(|| phys.command_internal(PhysCommand::Interact { interact: Interact::SetPot { comp: sensor, position } }))
            .map_err(|_| ())?.map_err(|_| ())?;
    } else {
        for (quantity, [lo, hi]) in ranges {
            let value = lo + (hi - lo) * position;
            guarded(|| phys.command_internal(PhysCommand::Interact { interact: Interact::SetWorld {
                comp: sensor, quantity: quantity.clone(), value,
            } })).map_err(|_| ())?.map_err(|_| ())?;
        }
    }
    Ok(())
}
fn sensor_position(phys: &PhysLab, sensor: CompId) -> std::collections::BTreeMap<String, f64> {
    guarded(|| phys.snapshot()).ok().and_then(|s| s.states.into_iter().find(|(id,_)| *id == sensor))
        .map(|(_,states)| states.into_iter().filter_map(|(name,value)| match value {
            n7rt_circuit::Value::Num(v) => Some((name,v)), _ => None,
        }).collect()).unwrap_or_default()
}

/// Release the clock even when source creation or a trajectory panics.
struct SourceClock { phys: Arc<PhysLab>, token: u64 }
impl SourceClock {
    fn acquire(phys: Arc<PhysLab>) -> Self {
        let token = phys.acquire_source_clock().expect("physical source clock unavailable");
        Self { phys, token }
    }
}
impl Drop for SourceClock {
    fn drop(&mut self) { let _ = self.phys.release_source_clock(self.token); }
}

/// The bus owns the Physical Lab clock for a mission session. Every ECU sample
/// follows exactly one real 1 ms solver step, even when a poll advances several ms.
struct VehicleSource {
    vehicle: Vehicle,
    inputs: LiveInputs,
    driver: Option<PositionDriver>,
    clock: SourceClock,
}

impl BusSource for VehicleSource {
    fn advance(&mut self, t_us: u64) -> Vec<BusFrame> {
        let inputs = &self.inputs;
        let clock = self.clock.token;
        let driver = &mut self.driver;
        self.vehicle.advance(t_us, &mut |ms| {
            if let Some(driver) = driver.as_mut() { driver.drive(ms * 1000); }
            let frame = inputs.phys.advance_source_clock(clock, ms)
                .expect("physical source clock unavailable");
            // A typed mission input is never reconstructed from a missing event history.
            if let Some(input) = frame.mission_source_inputs(inputs.ecu) { return input; }
            let pin = |p: &str| PinRef { comp: inputs.ecu, pin: p.to_owned() };
            let lv = match frame.voltage_between_pins(&pin(PIN_LV_IN), &pin(PIN_GND)) {
                Reading::Valid { v } => Some(v), _ => None,
            };
            let mut adc = [None; 4];
            for (value, channel) in adc.iter_mut().zip(&inputs.channels) {
                *value = match frame.adc(inputs.ecu, channel) {
                    Reading::Valid { v } => Some(v.code), _ => None,
                };
            }
            EcuInputs { lv, adc, ..Default::default() }
        })
    }
}

fn now_s() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Offset of the local civil time from UTC, seconds (dates shown to the student; UTC when unknown).
#[cfg(windows)]
fn local_offset_s() -> i64 {
    use windows_sys::Win32::System::Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION};
    // SAFETY: plain C structure filled by the call; all-zero bytes are a valid initial value.
    let mut tz: TIME_ZONE_INFORMATION = unsafe { std::mem::zeroed() };
    let id = unsafe { GetTimeZoneInformation(&mut tz) };
    let bias = match id {
        0 => tz.Bias,
        1 => tz.Bias + tz.StandardBias,
        2 => tz.Bias + tz.DaylightBias,
        _ => return 0,
    };
    -(bias as i64) * 60
}
#[cfg(not(windows))]
fn local_offset_s() -> i64 {
    0
}

fn attempt_id() -> String {
    let mut b = [0u8; 16];
    if getrandom::fill(&mut b).is_err() {
        b[..8].copy_from_slice(&os_seed().to_le_bytes());
    }
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// How the fiche cites a library entry: its manifest SHA-256 (local copy) or its primary address (ch. 24 §3.3).
fn library_id(e: &LibEntry) -> String {
    e.local.as_ref().map(|(_, sha)| sha.clone()).or_else(|| e.url.clone()).unwrap_or_else(|| e.item.title.clone())
}

fn os_seed() -> u64 {
    let mut b = [0u8; 8];
    if getrandom::fill(&mut b).is_ok() {
        return u64::from_le_bytes(b);
    }
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0x9E37_79B9_7F4A_7C15)
}

/// Live slider: a new vehicle at each session start, fresh seed; with the anomaly in force when it acts on the
/// vehicle (frame corruption in `fault`, source parameters in `pack`), from the first frame on.
struct LiveFactory {
    pack: Arc<Pack>,
    inputs: LiveInputs,
    seed: Option<u64>,
    fault: Option<FrameFault>,
}

impl SourceFactory for LiveFactory {
    fn start(&mut self) -> Box<dyn BusSource> {
        let seed = self.seed.unwrap_or_else(os_seed);
        let clock = SourceClock::acquire(self.inputs.phys.clone());
        self.inputs.phys.set_sensor_seed(seed);
        self.inputs.phys.command_internal(PhysCommand::Run).expect("physical source cannot run");
        let mut vehicle = Vehicle::new(&self.pack, seed);
        // a corruption the vehicle refuses would be a pack defect: the bus then stays healthy
        let _ = vehicle.set_frame_fault(self.fault);
        Box::new(VehicleSource { vehicle, inputs: self.inputs.clone(), driver: None, clock })
    }
}

/// Nominal check: the vehicle and the scenario of the check's seeds; the sensor is set to its initial position at t = 0.
struct CheckFactory {
    pack: Arc<Pack>,
    inputs: LiveInputs,
    sensor: CompId,
    seeds: Seeds,
}

impl SourceFactory for CheckFactory {
    fn start(&mut self) -> Box<dyn BusSource> {
        let clock = SourceClock::acquire(self.inputs.phys.clone());
        self.inputs.phys.set_sensor_seed(self.seeds.scenario);
        let mut driver = PositionDriver {
            scenario: NominalScenario::new(&self.pack, self.seeds.scenario),
            phys: self.inputs.phys.clone(),
            sensor: self.sensor,
            last: None,
            ranges: self.pack.world_ranges().clone(),
        };
        driver.drive(0);
        self.inputs.phys.command_internal(PhysCommand::Run).expect("physical source cannot run");
        Box::new(VehicleSource {
            vehicle: Vehicle::new(&self.pack, self.seeds.vehicle),
            inputs: self.inputs.clone(),
            driver: Some(driver),
            clock,
        })
    }
}

#[derive(Debug, Clone, Copy)]
struct Seeds {
    vehicle: u64,
    scenario: u64,
}

// ------------------------------------------------------------------------------------------------------ judgement

/// Order in which the reference consumer is fed: every tick `k·P` (k ≥ 1, ≤ `duration_us`) before the frames of the
/// same or a later timestamp (N7RT-Link v0 §9.4), frames after `duration_us` left out. `Frame(i)` indexes `frames`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Feed {
    Tick(u64),
    Frame(usize),
}

pub fn feed_order(frames: &[RecFrame], duration_us: u64, tick_us: u64) -> Vec<Feed> {
    let n = if tick_us == 0 { 0 } else { duration_us / tick_us };
    let mut out = Vec::with_capacity(frames.len() + n as usize);
    let mut k = 1u64;
    for (i, f) in frames.iter().enumerate().filter(|(_, f)| f.t_us <= duration_us) {
        while k <= n && k * tick_us <= f.t_us {
            out.push(Feed::Tick(k * tick_us));
            k += 1;
        }
        out.push(Feed::Frame(i));
    }
    while k <= n {
        out.push(Feed::Tick(k * tick_us));
        k += 1;
    }
    out
}

/// The board's answer to one tick: slot → value or reject (`None` when not written, or not delivered).
pub fn board_tick(t: &RecTick) -> BoardTick {
    let len = t.entries.iter().map(|e| e.slot as usize + 1).max().unwrap_or(0).max(8);
    let mut slots = vec![None; len];
    if t.status == 0 {
        for e in &t.entries {
            slots[e.slot as usize] = Some(if e.kind == 1 { SlotOut::Reject } else { SlotOut::Value(e.value as f64) });
        }
    }
    BoardTick { t_us: t.t_us, slots }
}

/// The board's ticks up to `duration_us`, if every tick `k·P` is there exactly once and in order (a lost tick cancels
/// the check: ch. 24 §2.9.5).
pub fn board_ticks(ticks: &[RecTick], duration_us: u64, tick_us: u64) -> Option<Vec<BoardTick>> {
    if tick_us == 0 {
        return None;
    }
    let n = duration_us / tick_us;
    let got: Vec<&RecTick> = ticks.iter().filter(|t| t.t_us <= duration_us).collect();
    let complete = got.len() as u64 == n && got.iter().enumerate().all(|(i, t)| t.t_us == (i as u64 + 1) * tick_us);
    complete.then(|| got.into_iter().map(board_tick).collect())
}

/// One recorded tick as a run file tick: the same slots as [`board_tick`] (not delivered = nothing written).
pub fn run_tick(t: &RecTick) -> RunTick {
    let slots = board_tick(t)
        .slots
        .into_iter()
        .map(|s| {
            s.map(|o| match o {
                SlotOut::Value(v) => RunSlot::V(v),
                SlotOut::Reject => RunSlot::Reject,
            })
        })
        .collect();
    RunTick { t_us: t.t_us, slots }
}

/// What the run record says about the run, besides the recorded frames and ticks.
#[derive(Debug, Clone, Default)]
pub struct RunMeta {
    pub mission: String,
    pub cohort: String,
    pub group: String,
    pub pack_id: String,
    pub app_version: String,
    pub board_uid: String,
    pub runtime_build: String,
    pub student_crc32: String,
    pub seed_vehicle: u64,
    pub seed_scenario: u64,
    pub duration_us: u64,
    pub tick_us: u64,
    pub started: Option<SystemTime>,
}

/// The run record of a check (docs/distribution.md §3.2): every frame as delivered, every board tick.
pub fn run_record(meta: &RunMeta, rec: &CheckRecord) -> RunRecord {
    RunRecord {
        mission: meta.mission.clone(),
        cohort: meta.cohort.clone(),
        group: meta.group.clone(),
        pack_id: meta.pack_id.clone(),
        app_version: meta.app_version.clone(),
        board_uid: meta.board_uid.clone(),
        runtime_build: meta.runtime_build.clone(),
        student_crc32: meta.student_crc32.clone(),
        seed_vehicle: meta.seed_vehicle.to_string(),
        seed_scenario: meta.seed_scenario.to_string(),
        duration_us: meta.duration_us,
        tick_us: meta.tick_us,
        started_utc: meta.started.map(utc_stamp).unwrap_or_default(),
        frames: rec.frames.iter().map(|f| RunFrame { t_us: f.t_us, id: f.id, flags: f.flags, data: f.data.clone() }).collect(),
        ticks: rec.ticks.iter().map(run_tick).collect(),
    }
}

/// Counts of a judged check (trainer report).
#[derive(Debug, Clone)]
struct Judged {
    verdict: Verdict,
    frames: usize,
    ticks: usize,
}

/// End of a check: judged (instructor, reference pack) or recorded (student, group pack).
#[derive(Debug, Clone)]
enum Outcome {
    Judged(Judged),
    Recorded(PathBuf),
}

/// Why a check was cancelled: the public reason (M-11) and a private explanation (report only).
#[derive(Debug, Clone)]
struct Cancel {
    reason: &'static str,
    why: String,
}

impl Cancel {
    fn incomplete(why: impl Into<String>) -> Cancel {
        Cancel { reason: CANCEL_INCOMPLETE, why: why.into() }
    }
    fn board(why: impl Into<String>) -> Cancel {
        Cancel { reason: CANCEL_BOARD, why: why.into() }
    }
    fn app() -> Cancel {
        Cancel { reason: CANCEL_APP, why: "application closed during the check".into() }
    }
}

/// Every tick of the run must be there (a lost tick cancels the check: ch. 24 §2.9.5).
fn complete(rec: &CheckRecord, duration_us: u64, tick_us: u64) -> Result<usize, Cancel> {
    board_ticks(&rec.ticks, duration_us, tick_us).map(|b| b.len()).ok_or_else(|| {
        Cancel::incomplete(format!("tick results incomplete: {} recorded for {} expected", rec.ticks.len(), duration_us / tick_us))
    })
}

/// The judgement of the instructor tool (`n7rt_vehicle::judge_run`) on the run record of the check.
fn judge(pack: &Pack, run: &RunRecord, rec: &CheckRecord, duration_us: u64, tick_us: u64) -> Result<Judged, Cancel> {
    let ticks = complete(rec, duration_us, tick_us)?;
    let frames = rec.frames.iter().filter(|f| f.t_us <= duration_us).count();
    match guarded(|| judge_run(pack, run)) {
        Ok(Ok(verdict)) => Ok(Judged { verdict, frames, ticks }),
        Ok(Err(e)) => Err(Cancel::incomplete(format!("run not judged: {e}"))),
        Err(()) => Err(Cancel::incomplete("internal error of the judgement")),
    }
}

/// Writes the student's run file into `<documents>/N7RT CAN Lab/essais/`.
fn record(documents: Option<&Path>, run: &RunRecord, rec: &CheckRecord, started: SystemTime) -> Result<PathBuf, Cancel> {
    complete(rec, run.duration_us, run.tick_us)?;
    let fail = |why: &str| Cancel { reason: CANCEL_WRITE, why: why.into() };
    let bytes = guarded(|| run.to_file_bytes()).map_err(|()| fail("run file not encoded"))?;
    let dir = documents.ok_or_else(|| fail("no documents directory"))?.join(RUNS_SUBDIR[0]).join(RUNS_SUBDIR[1]);
    std::fs::create_dir_all(&dir).map_err(|_| fail("runs directory not created"))?;
    let group = if safe_name(&run.group) { run.group.as_str() } else { "groupe" };
    let path = unique_path(&dir, &run_file_name(&run.mission, group, started)).ok_or_else(|| fail("no free file name"))?;
    std::fs::write(&path, bytes).map_err(|_| fail("run file not written"))?;
    Ok(path)
}

// --------------------------------------------------------------------------------------------------- controller

#[derive(Debug, Clone, Default)]
pub struct MissionConfig {
    /// Fixed seed of the vehicle and the scenario (tests only); None = drawn from the OS at each session or check.
    pub fixed_seed: Option<u64>,
    /// The public library.
    pub library_dir: Option<PathBuf>,
    /// `%APPDATA%\fr.n7rt.canlab`: student state and imported packs (`student/`).
    pub app_data_dir: Option<PathBuf>,
    /// The user's documents: run files go to `<documents>/N7RT CAN Lab/essais/`.
    pub documents_dir: Option<PathBuf>,
    /// How the instructor directory is found at login.
    pub instructor: InstructorLocator,
    /// Version written into run files (empty = this crate's version).
    pub app_version: String,
    /// Wait after a failed instructor login (None = 1 s).
    pub failed_login_delay: Option<Duration>,
}

impl MissionConfig {
    /// The application's configuration: library and instructor directory found from the environment and the
    /// executable's location.
    pub fn discover(app_data_dir: Option<PathBuf>, documents_dir: Option<PathBuf>) -> MissionConfig {
        MissionConfig {
            fixed_seed: None,
            library_dir: Catalog::discover().library_dir,
            instructor: InstructorLocator::from_environment(app_data_dir.as_deref()),
            app_data_dir,
            documents_dir,
            app_version: String::new(),
            failed_login_delay: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
enum Phase {
    #[default]
    Idle,
    Running,
    Done(bool),
    Recorded(String),
    Cancelled(&'static str),
}

#[derive(Debug, Clone, Default)]
struct CheckState {
    phase: Phase,
    elapsed_s: f64,
    total_s: f64,
}

#[derive(Debug, Clone)]
enum OpenKind {
    /// Instructor: the reference pack; the trainer's reports go to this directory.
    Reference { reports: Option<PathBuf> },
    /// Student: a group pack (clear header).
    Group(PackHeader),
}

struct Open {
    id: String,
    title: String,
    pack: Arc<Pack>,
    kind: OpenKind,
    statement: StatementView,
    library: Vec<LibEntry>,
    inputs: LiveInputs,
    sensor: CompId,
    tick_period_ms: u16,
    /// Progression of a release 0.3 group pack (sealed workflow, progress, fiche).
    session: Option<Arc<Session>>,
    /// Digest of the bench text last kept sealed with the progression (0: never).
    bench_digest: AtomicU64,
}

#[derive(Default)]
struct State {
    open: Option<Arc<Open>>,
    check: CheckState,
}

/// Mode of the application, in memory only. The instructor code verified at login is kept for the instructor tools
/// (the console tool's library checks it again at each command) and wiped at logout.
#[derive(Default)]
struct Access {
    mode: Mode,
    instructor_dir: Option<PathBuf>,
    code: Option<String>,
    last_run: Option<PathBuf>,
}

impl Access {
    fn forget_code(&mut self) {
        if let Some(code) = self.code.take() {
            let mut b = code.into_bytes();
            wipe(&mut b);
        }
    }
}

struct Inner {
    lab: Arc<Lab>,
    phys: Arc<PhysLab>,
    cfg: MissionConfig,
    store: StudentStore,
    /// Serialises open, close, mode changes and the start of a check.
    ops: Mutex<()>,
    state: Mutex<State>,
    access: Mutex<Access>,
    closing: AtomicBool,
    #[cfg(test)]
    fail_check_spawn: AtomicBool,
    /// Library copies already checked against their manifest digest.
    verified: Mutex<HashMap<PathBuf, bool>>,
}

/// Handle used by the Tauri commands. Cheap to clone.
#[derive(Clone)]
pub struct Missions {
    inner: Arc<Inner>,
}

fn board_ready(s: &UiBatch) -> bool {
    let st = &s.status;
    st.port.is_some()
        && !st.stalled
        && matches!(st.state, LinkState::Ready | LinkState::Session)
        && st.board.as_ref().is_some_and(|b| b.runtime_verified && b.compatible && b.student_status == 1)
}

fn board_gone(s: &UiBatch) -> bool {
    s.status.port.is_none()
        || matches!(s.status.state, LinkState::Lost | LinkState::Absent | LinkState::Disconnected | LinkState::PortBusy)
}

fn link_errors(s: &UiBatch) -> u64 {
    s.stats.rx_framing.values().sum::<u64>() + s.stats.rx_rejects + s.stats.board_seq_gaps
}

/// Stops the running session and starts a new one (same tick period unless given): the new source takes over at t = 0.
fn restart_session(lab: &Lab, tick: Option<u16>) {
    let s = lab.snapshot();
    if let Some(sess) = s.status.session.as_ref().filter(|x| x.phase == "active" || x.phase == "starting") {
        let tick_period_ms = tick.unwrap_or(sess.tick_period_ms);
        lab.command(Command::StopSession);
        lab.command(Command::StartSession { tick_period_ms });
    }
}

fn file_name(p: &Path) -> Option<String> {
    p.file_name().map(|n| n.to_string_lossy().into_owned())
}

impl Missions {
    /// The application's controller (student mode).
    pub fn start(lab: Arc<Lab>, phys: Arc<PhysLab>, cfg: MissionConfig) -> Missions {
        let store = StudentStore { app_data_dir: cfg.app_data_dir.clone() };
        Missions {
            inner: Arc::new(Inner {
                lab,
                phys,
                cfg,
                store,
                ops: Mutex::new(()),
                state: Mutex::new(State::default()),
                access: Mutex::new(Access::default()),
                closing: AtomicBool::new(false),
                #[cfg(test)]
                fail_check_spawn: AtomicBool::new(false),
                verified: Mutex::new(HashMap::new()),
            }),
        }
    }

    pub fn mode(&self) -> Mode {
        self.inner.access.lock().unwrap().mode
    }

    /// Reference packs (instructor mode only) and the library.
    pub fn catalog(&self) -> Catalog {
        let dir = self.inner.access.lock().unwrap().instructor_dir.clone();
        Catalog { missions_dir: dir.map(|d| d.join("missions")), library_dir: self.inner.cfg.library_dir.clone() }
    }

    /// Instructor: every mission, available when its reference pack loads. Student: every mission, the playable ones
    /// can be chosen.
    pub fn list(&self) -> Vec<MissionEntry> {
        match self.mode() {
            Mode::Instructor => self.catalog().list(),
            Mode::Student => student_list(),
        }
    }

    pub fn view(&self) -> MissionView {
        let st = self.inner.state.lock().unwrap();
        let c = &st.check;
        let check = CheckView {
            state: match c.phase {
                Phase::Idle => "idle",
                Phase::Running => "running",
                Phase::Done(_) => "done",
                Phase::Recorded(_) => "recorded",
                Phase::Cancelled(_) => "cancelled",
            },
            elapsed_s: c.elapsed_s,
            total_s: c.total_s,
            verdict: match c.phase {
                Phase::Done(true) => Some("pass"),
                Phase::Done(false) => Some("fail"),
                _ => None,
            },
            reason: match c.phase {
                Phase::Cancelled(r) => Some(r),
                _ => None,
            },
            run_file: match &c.phase {
                Phase::Recorded(name) => Some(name.clone()),
                _ => None,
            },
        };
        match st.open.as_ref() {
            None => MissionView {
                id: None,
                title: None,
                open: false,
                kind: None,
                group: None,
                statement: None,
                library: Vec::new(),
                check,
                tick_period_ms: None,
                progression: None,
            },
            Some(o) => MissionView {
                id: Some(o.id.clone()),
                title: Some(o.title.clone()),
                open: true,
                kind: Some(match o.kind {
                    OpenKind::Reference { .. } => "reference",
                    OpenKind::Group(_) => "group",
                }),
                group: match &o.kind {
                    OpenKind::Group(h) => Some(h.group.clone()),
                    OpenKind::Reference { .. } => None,
                },
                statement: Some(o.statement.clone()),
                library: o.library.iter().map(|e| e.item.clone()).collect(),
                progression: o.session.as_ref().map(|s| {
                    s.touch(now_s());
                    // never while another operation holds the bench: a mission being opened replaces it
                    if c.phase != Phase::Running {
                        if let Ok(_ops) = self.inner.ops.try_lock() {
                            keep_anomaly(&self.inner, s);
                            keep_bench(&self.inner, o, s);
                        }
                    }
                    let running = (c.phase == Phase::Running).then_some((c.elapsed_s, c.total_s));
                    let mut v = s.view(now_s(), running, board_ready(&self.inner.lab.snapshot()));
                    v.documents = o.library.iter().map(|e| DocumentChoice { id: library_id(e), title: e.item.title.clone() }).collect();
                    if let Some(inc) = v.incident.as_mut() {
                        let (parts, pins) = bench_choices(&self.inner.phys);
                        inc.parts = parts;
                        inc.pins = pins;
                    }
                    v
                }),
                check,
                tick_period_ms: Some(o.tick_period_ms),
            },
        }
    }

    fn running(&self) -> bool {
        self.inner.state.lock().unwrap().check.phase == Phase::Running
    }

    // ------------------------------------------------------------------------------------------------ modes

    /// The student's saved state: locked mission, imported pack, last run file.
    pub fn student_state(&self) -> StudentView {
        let s = self.inner.store.load();
        StudentView {
            locked_title: s.mission.as_deref().and_then(known).map(|k| k.title.to_owned()),
            locked: s.mission,
            pack: s.pack.map(|p| PackInfo { cohort: p.cohort, group: p.group }),
            last_run: self.inner.access.lock().unwrap().last_run.as_deref().and_then(file_name),
        }
    }

    pub fn access(&self) -> AccessView {
        AccessView {
            mode: self.mode(),
            student: self.student_state(),
            edition: n7rt_vehicle::EDITION,
            oracle: n7rt_vehicle::ORACLE,
        }
    }

    /// Full path of the last run file (e2e harness only: the UI gets its name).
    pub fn last_run_path(&self) -> Option<PathBuf> {
        self.inner.access.lock().unwrap().last_run.clone()
    }

    fn require_instructor(&self) -> Result<PathBuf, String> {
        let a = self.inner.access.lock().unwrap();
        match (a.mode, a.instructor_dir.clone()) {
            (Mode::Instructor, Some(d)) => Ok(d),
            _ => Err(INSTRUCTOR_ONLY.into()),
        }
    }

    /// The instructor directory and the code verified at login (instructor tools).
    fn instructor_tools(&self) -> Result<(PathBuf, String), String> {
        let a = self.inner.access.lock().unwrap();
        match (a.mode, a.instructor_dir.clone(), a.code.clone()) {
            (Mode::Instructor, Some(d), Some(c)) => Ok((d, c)),
            _ => Err(INSTRUCTOR_ONLY.into()),
        }
    }

    fn require_student(&self) -> Result<(), String> {
        match self.mode() {
            Mode::Student => Ok(()),
            Mode::Instructor => Err(STUDENT_ONLY.into()),
        }
    }

    /// Instructor mode until logout or the end of the process: the instructor directory must be found on this machine
    /// and the code must match its `instructor/auth.json`. A failed attempt waits 1 s.
    pub fn instructor_login(&self, code: &str) -> Result<AccessView, String> {
        let fail = |msg: &str| -> Result<AccessView, String> {
            thread::sleep(self.inner.cfg.failed_login_delay.unwrap_or(FAILED_LOGIN_DELAY));
            Err(msg.to_owned())
        };
        let Some(dir) = self.inner.cfg.instructor.locate() else { return fail(MSG_NO_INSTRUCTOR) };
        let Some(phc) = read_auth(&dir) else { return fail(MSG_NO_INSTRUCTOR) };
        let typed = guarded(|| normalize_code(code)).unwrap_or_default();
        if typed.is_empty() || !guarded(|| verify_code(&phc, &typed)).unwrap_or(false) {
            return fail(MSG_WRONG_INSTRUCTOR_CODE);
        }
        {
            let mut a = self.inner.access.lock().unwrap();
            a.forget_code();
            a.mode = Mode::Instructor;
            a.instructor_dir = Some(dir);
            a.code = Some(typed);
        }
        Ok(self.access())
    }

    /// Back to student mode; a mission opened with its reference pack is closed.
    pub fn instructor_logout(&self) -> Result<AccessView, String> {
        let _ops = self.inner.ops.lock().unwrap();
        if self.running() {
            return Err(RUNNING.into());
        }
        let reference = self.inner.state.lock().unwrap().open.as_ref().is_some_and(|o| matches!(o.kind, OpenKind::Reference { .. }));
        if reference {
            self.close_inner()?;
        }
        {
            let mut a = self.inner.access.lock().unwrap();
            a.mode = Mode::Student;
            a.instructor_dir = None;
            a.forget_code();
        }
        Ok(self.access())
    }

    /// Clears the student lock and the imported packs on this machine, closes the mission (back to training).
    pub fn instructor_reset(&self) -> Result<AccessView, String> {
        self.require_instructor()?;
        let _ops = self.inner.ops.lock().unwrap();
        if self.running() {
            return Err(RUNNING.into());
        }
        self.close_inner()?;
        self.inner.state.lock().unwrap().check = CheckState::default();
        self.inner.store.clear()?;
        if let Ok(dir) = self.inner.store.student_dir() {
            session::reset_all(&dir)?;
        }
        self.inner.access.lock().unwrap().last_run = None;
        Ok(self.access())
    }

    /// Group packs of a cohort (docs/distribution.md §3.3, `n7rt_tool::gen_packs`): one row per group with its code
    /// and its file; a group already generated is refused unless `replace` (new code, new pack).
    pub fn instructor_gen_packs(&self, mission: &str, cohort: &str, groups: &[String], replace: bool) -> Result<Vec<GenRow>, String> {
        let (dir, code) = self.instructor_tools()?;
        if !known(mission).is_some_and(|k| k.playable) {
            return Err(format!("Mission indisponible : {NOT_YET}."));
        }
        crate::cohort::gen_packs(&dir, &code, mission, cohort, groups, replace)
    }

    /// Verdict and private detail of a student's run file (`n7rt_tool::judge`; instructor only). `name` is the file
    /// name the instructor chose (the judgement is kept under it).
    pub fn instructor_judge(&self, name: &str, run_file: &[u8]) -> Result<Judgement, String> {
        let (dir, code) = self.instructor_tools()?;
        if run_file.len() > MAX_FILE_BYTES {
            return Err("Fichier trop volumineux pour un fichier d'essai.".into());
        }
        crate::cohort::judge(&dir, &code, name, run_file)
    }

    /// Locks the mission of the session (persisted); only an instructor reset frees it.
    pub fn student_choose(&self, id: &str) -> Result<AccessView, String> {
        self.require_student()?;
        let _ops = self.inner.ops.lock().unwrap();
        let state = self.inner.store.load();
        if let Some(m) = state.mission.as_deref() {
            return if m == id { Ok(self.access()) } else { Err(locked_message(m)) };
        }
        let k = known(id).ok_or_else(|| "Mission inconnue.".to_owned())?;
        if !k.playable {
            return Err(format!("Mission indisponible : {NOT_YET}."));
        }
        let chosen_utc = Some(utc_stamp(SystemTime::now()));
        self.inner.store.save(&StudentState { mission: Some(id.to_owned()), chosen_utc, pack: None })?;
        Ok(self.access())
    }

    /// Imports the group pack of the locked mission: copied as it is (sealed) into `student/packs/`.
    pub fn student_import_pack(&self, bytes: &[u8]) -> Result<AccessView, String> {
        self.require_student()?;
        let _ops = self.inner.ops.lock().unwrap();
        let mut state = self.inner.store.load();
        let mission = state.mission.clone().ok_or_else(|| CHOOSE_FIRST.to_owned())?;
        if bytes.len() > MAX_FILE_BYTES {
            return Err(NOT_A_PACK.into());
        }
        let header = match guarded(|| read_pack_header(bytes)) {
            Ok(Ok(h)) => h,
            _ => return Err(NOT_A_PACK.into()),
        };
        if header.mission != mission {
            return Err(OTHER_MISSION_PACK.into());
        }
        if !safe_name(&header.group) || !safe_name(&header.cohort) {
            return Err(NOT_A_PACK.into());
        }
        let file = format!("{}-{}.{PACK_EXT}", header.mission, header.group);
        self.inner.store.store_pack(&file, bytes)?;
        state.pack = Some(PackRef { file, mission: header.mission, cohort: header.cohort, group: header.group, pack_id: header.pack_id });
        self.inner.store.save(&state)?;
        Ok(self.access())
    }

    /// Opens the locked mission with the imported group pack and the group code (key and plaintext in memory only).
    pub fn student_open(&self, code: &str) -> Result<MissionView, String> {
        let i = &self.inner;
        let _ops = i.ops.lock().unwrap();
        if self.running() {
            return Err(RUNNING.into());
        }
        let state = i.store.load();
        let mission = state.mission.clone().ok_or_else(|| CHOOSE_FIRST.to_owned())?;
        let pref = state.pack.clone().filter(|p| p.mission == mission).ok_or_else(|| IMPORT_FIRST.to_owned())?;
        let already = i.state.lock().unwrap().open.as_ref().is_some_and(|o| {
            o.id == mission && matches!(&o.kind, OpenKind::Group(h) if h.pack_id == pref.pack_id && h.group == pref.group)
        });
        // the same pack is open: nothing to do, unless another document replaced its bench (opened again then)
        if already && on_bench(i) {
            return Ok(self.view());
        }
        // opened again on a replaced bench: its session goes on, one writer of a progression (session::hold)
        let current = if already { i.state.lock().unwrap().open.as_ref().and_then(|o| o.session.clone()) } else { None };
        self.bench_free()?;
        let sealed = i.store.read_pack(&pref.file).ok_or_else(|| "Pack du groupe introuvable : importez-le à nouveau.".to_owned())?;
        let typed = guarded(|| normalize_code(code)).unwrap_or_default();
        if typed.is_empty() {
            return Err(MSG_WRONG_GROUP_CODE.into());
        }
        let (header, mut plain) = match guarded(|| open_pack(&sealed, &typed)) {
            Ok(Ok(x)) => x,
            Ok(Err(SealError::WrongCode)) => return Err(MSG_WRONG_GROUP_CODE.into()),
            _ => return Err(PACK_UNREADABLE.into()),
        };
        // A release 0.3 plaintext wraps the student pack and the sealed workflow; a 0.2 plaintext is the pack itself.
        let split = match std::str::from_utf8(&plain) {
            Ok(text) if header.mission == mission => guarded(|| sealed::split(text)).ok().and_then(Result::ok),
            _ => None,
        };
        wipe(&mut plain);
        drop(plain);
        let Some((pack_text, workflow)) = split else { return Err(PACK_UNREADABLE.into()) };
        let pack = guarded(|| Pack::from_student_json(&pack_text));
        let mut pack_bytes = pack_text.into_bytes();
        wipe(&mut pack_bytes);
        drop(pack_bytes);
        let pack = match pack {
            // A group pack is a student pack (no oracle), of the locked mission, with its bench.
            Ok(Ok(p))
                if guarded(|| p.is_student() && p.mission().eq_ignore_ascii_case(&mission) && !p.bench_json().trim().is_empty())
                    .unwrap_or(false) =>
            {
                p
            }
            _ => return Err(PACK_UNREADABLE.into()),
        };
        let session = match workflow {
            None => None,
            Some(_) if current.is_some() => current,
            Some(value) => {
                let parsed = guarded(|| sealed::parse_workflow(&value, &mission, pack.tick_period_ms())).ok().and_then(Result::ok);
                let workflow = parsed.ok_or_else(|| PACK_UNREADABLE.to_owned())?;
                let owner = Owner {
                    mission: mission.clone(),
                    cohort: header.cohort.clone(),
                    group: header.group.clone(),
                    pack_id: header.pack_id.clone(),
                    workflow_sha256: workflow.sha256.clone(),
                };
                let dir = session::progress_dir(&i.store.student_dir()?, &mission, &header.pack_id);
                let library = Catalog { missions_dir: None, library_dir: i.cfg.library_dir.clone() }.library(&mission);
                let ids = library.iter().map(library_id).collect();
                Some(Arc::new(Session::open(workflow, dir, owner, ids, now_s(), local_offset_s())))
            }
        };
        // the student's own bench, kept sealed with the progression, when it still has the reference's parts
        let saved = session.as_ref().and_then(|s| s.load_bench()).filter(|(text, _)| {
            guarded(|| {
                let reference = n7rt_circuit::persist::load(pack.bench_json()).ok();
                let mine = n7rt_circuit::persist::load(text).ok();
                matches!((reference, mine), (Some(r), Some(m)) if pcheck::same_parts(&m, &r))
            })
            .unwrap_or(false)
        });
        self.open_with(&mission, pack, OpenKind::Group(header), session, saved)
    }

    /// Opens the folder of the run files.
    pub fn open_runs_folder(&self) -> Result<(), String> {
        let docs = self.inner.cfg.documents_dir.as_ref().ok_or_else(|| "Dossier Documents introuvable.".to_owned())?;
        let dir = docs.join(RUNS_SUBDIR[0]).join(RUNS_SUBDIR[1]);
        std::fs::create_dir_all(&dir).map_err(|_| "Dossier des essais inaccessible.".to_owned())?;
        open_external(&dir.to_string_lossy()).map_err(|_| "Ouverture du dossier impossible.".to_owned())
    }

    // ------------------------------------------------------------------------------------------- open, close

    /// Why a student cannot open a mission directly (the core refuses whatever the UI shows).
    fn student_refusal(&self, id: &str) -> String {
        match self.inner.store.load().mission {
            Some(m) if m != id => locked_message(&m),
            Some(_) => "Saisissez le code du groupe pour ouvrir la mission.".into(),
            None => CHOOSE_FIRST.into(),
        }
    }

    /// The bench replaces the Physical Lab document: never over unsaved work of the student.
    fn bench_free(&self) -> Result<(), String> {
        if guarded(|| self.inner.phys.snapshot()).is_ok_and(|s| s.dirty && s.disclosure == Disclosure::Exploration) {
            return Err("Le circuit ouvert dans le Physical Lab a des modifications non enregistrées : enregistrez-le ou \
                        choisissez « Nouveau », puis ouvrez la mission."
                .into());
        }
        Ok(())
    }

    /// Instructor: loads the reference pack, the bench (Physical Lab, mission mode) and switches the bus to the mission
    /// vehicle; a running session restarts at t = 0 with the mission's tick period. Student: refused (the locked
    /// mission opens with the group code, [`Missions::student_open`]).
    pub fn open(&self, id: &str) -> Result<MissionView, String> {
        let i = &self.inner;
        let _ops = i.ops.lock().unwrap();
        let dir = match self.mode() {
            Mode::Student => return Err(self.student_refusal(id)),
            Mode::Instructor => self.require_instructor()?,
        };
        if self.running() {
            return Err(RUNNING.into());
        }
        let k = known(id).ok_or_else(|| "Mission inconnue.".to_owned())?;
        if !k.playable {
            return Err(format!("Mission indisponible : {NOT_YET}."));
        }
        statement(id).ok_or_else(|| "Énoncé de la mission illisible.".to_owned())?;
        self.bench_free()?;
        let pack = self.catalog().load_pack(id).map_err(|e| format!("Mission indisponible : {e}."))?;
        let reports = Some(dir.join("missions").join(id).join("reports"));
        self.open_with(id, pack, OpenKind::Reference { reports }, None, None)
    }

    /// Instructor: reloads the current reference bench and starts its source afresh; the student choice persists.
    pub fn reset(&self) -> Result<MissionView, String> {
        self.require_instructor()?;
        let id = self.inner.state.lock().unwrap().open.as_ref().map(|o| o.id.clone())
            .ok_or_else(|| "Aucune mission ouverte.".to_owned())?;
        self.open(&id)
    }

    /// Bench, vehicle source, session restart (callers hold `ops`).
    /// `saved`: the student's sealed bench and whether it was kept under the activation now in force.
    fn open_with(&self, id: &str, pack: Pack, kind: OpenKind, session: Option<Arc<Session>>, saved: Option<(String, bool)>) -> Result<MissionView, String> {
        let i = &self.inner;
        let statement = statement(id).ok_or_else(|| "Énoncé de la mission illisible.".to_owned())?;
        let pack = Arc::new(pack);
        let internal = || format!("Mission indisponible : {UNREADABLE} : erreur interne du moteur véhicule.");
        // a bench kept before the activation of the anomaly in force does not hold its setting yet
        let restored = saved.as_ref().is_some_and(|(_, current)| *current);
        let (bench, ecu_name, sensor_name, channels, tick) = guarded(|| {
            (
                saved.map(|(text, _)| text).unwrap_or_else(|| pack.bench_json().to_owned()),
                pack.bench_ecu().to_owned(),
                pack.bench_sensor().to_owned(),
                pack.adc_channels().to_vec(),
                pack.tick_period_ms(),
            )
        })
        .map_err(|()| internal())?;
        // Validate the replacement before touching the current document or bus.
        // A malformed source or a world command aimed at a sensor cannot leave
        // the previous mission attached to a half-loaded physical bench.
        let prepared = guarded(|| -> Result<(), ()> {
            use n7rt_circuit::Kind;
            let circuit = n7rt_circuit::persist::load(&bench).map_err(|_| ())?;
            let components = circuit.components();
            let ecu = components.values().find(|c| c.label == ecu_name).ok_or(())?;
            let sensor = components.values().find(|c| c.label == sensor_name).ok_or(())?;
            let expected = match id {
                "wsf" => (Kind::EcuWsf, Kind::FrontAxle),
                "sas" => (Kind::EcuSas, Kind::SteeringColumn),
                "bps" => (Kind::EcuBps, Kind::BrakeHydraulics),
                _ => (Kind::EcuApps, Kind::Mlx90377),
            };
            if (ecu.kind, sensor.kind) != expected { return Err(()); }
            let ecu = ecu.id;
            let sensor = sensor.id;
            let mut core = crate::phys::PhysCore::new(circuit);
            core.configure_source(ecu, &pack).map_err(|_| ())?;
            for (quantity, [lo, hi]) in pack.world_ranges() {
                for value in [*lo, *hi] {
                    core.apply(&PhysCommand::Interact { interact: Interact::SetWorld {
                        comp: sensor, quantity: quantity.clone(), value,
                    } }).map_err(|_| ())?;
                }
            }
            Ok(())
        });
        if !matches!(prepared, Ok(Ok(()))) {
            return Err("Le banc ou la configuration physique de la mission est invalide.".into());
        }
        i.lab.quiesce_bus_source().map_err(|_| "La session précédente ne peut pas être arrêtée.".to_owned())?;
        let abandon = || {
            i.lab.set_bus_source(SourceKind::Training);
            restart_session(&i.lab, None);
            {
                let mut state = i.state.lock().unwrap();
                state.open = None;
                state.check = CheckState::default();
            }
            let _ = guarded(|| { i.phys.new_circuit(); i.phys.set_disclosure(Disclosure::Exploration); });
        };
        let loaded = guarded(|| i.phys.load_circuit_json(&format!("Banc {}", id.to_uppercase()), &bench, Disclosure::Mission));
        if !matches!(loaded, Ok(Ok(_))) {
            abandon();
            return Err("Le banc de la mission ne peut pas être chargé dans le Physical Lab.".into());
        }
        let find = |d: &str| guarded(|| i.phys.find_designator(d)).ok().flatten();
        let (Some(ecu), Some(sensor)) = (find(&ecu_name), find(&sensor_name)) else {
            abandon();
            return Err("Le banc de la mission est incomplet.".into());
        };
        let inputs = LiveInputs { phys: i.phys.clone(), ecu, channels };
        if !matches!(guarded(|| i.phys.configure_source(ecu, &pack)), Ok(Ok(()))) {
            abandon();
            return Err("La configuration physique de la mission est invalide.".into());
        }
        let open = Arc::new(Open {
            id: id.to_owned(),
            title: statement.title.clone(),
            pack: pack.clone(),
            kind,
            statement: StatementView {
                summary: statement.summary.clone(),
                requirements: statement.requirements.clone(),
                slots: statement.slots.clone(),
            },
            library: Catalog { missions_dir: None, library_dir: i.cfg.library_dir.clone() }.library(id),
            inputs: inputs.clone(),
            sensor,
            tick_period_ms: tick,
            session,
            bench_digest: AtomicU64::new(0),
        });
        let _ = (pack, inputs);
        if let Some(s) = open.session.as_deref() {
            // a reopening: the anomaly in force comes back on the bench (I-3), and the repair records a killed
            // application left open are completed
            apply_physical(i, s, false, restored);
            if let Some(circuit) = guarded(|| i.phys.circuit_copy()).ok().flatten() {
                s.complete_records(&n7rt_circuit::persist::save(&circuit));
            }
        }
        i.lab.set_bus_source(SourceKind::Mission(Box::new(live_factory(i, &open))));
        restart_session(&i.lab, Some(tick));
        {
            let mut st = i.state.lock().unwrap();
            st.open = Some(open);
            st.check = CheckState::default();
        }
        Ok(self.view())
    }

    /// Back to the training ECU; the Physical Lab gets a new circuit in exploration mode.
    pub fn close(&self) -> Result<MissionView, String> {
        let _ops = self.inner.ops.lock().unwrap();
        if self.running() {
            return Err(RUNNING.into());
        }
        self.close_inner()?;
        Ok(self.view())
    }

    /// Callers hold `ops` and checked that no check runs.
    fn close_inner(&self) -> Result<(), String> {
        let i = &self.inner;
        if i.state.lock().unwrap().open.is_some() {
            i.lab.quiesce_bus_source().map_err(|_| "La session précédente ne peut pas être arrêtée.".to_owned())?;
        }
        let was = {
            let mut st = i.state.lock().unwrap();
            st.check = CheckState::default();
            st.open.take()
        };
        if let Some(s) = was.as_ref().and_then(|o| o.session.as_ref()) {
            s.flush_active();
        }
        if was.is_some() {
            i.lab.set_bus_source(SourceKind::Training);
            restart_session(&i.lab, None);
            let _ = guarded(|| {
                i.phys.new_circuit();
                i.phys.set_disclosure(Disclosure::Exploration);
            });
        }
        Ok(())
    }

    /// Starts the hidden nominal check in the background; `view()` follows it. With a reference pack (instructor) it
    /// ends with a verdict; with a group pack it ends with a run file and no verdict.
    pub fn check(&self) -> Result<MissionView, String> {
        let i = &self.inner;
        let _ops = i.ops.lock().unwrap();
        let open = i.state.lock().unwrap().open.clone().ok_or_else(|| "Ouvrez d'abord une mission.".to_owned())?;
        if self.running() {
            return Err("Vérification déjà en cours.".into());
        }
        if open.session.is_some() {
            return Err("Ce pack suit la progression de la mission : utilisez le bouton de l'étape en cours.".into());
        }
        if matches!(open.kind, OpenKind::Reference { .. }) {
            self.require_instructor()?;
            if !n7rt_vehicle::ORACLE {
                return Err(ORACLE_ABSENT.into());
            }
        }
        if !board_ready(&i.lab.snapshot()) {
            return Err(BOARD_NOT_READY.into());
        }
        let seeds = match i.cfg.fixed_seed {
            Some(s) => Seeds { vehicle: s, scenario: s },
            None => Seeds { vehicle: os_seed(), scenario: os_seed() },
        };
        let duration_us = guarded(|| NominalScenario::new(&open.pack, seeds.scenario).duration_us())
            .map_err(|()| "Vérification impossible : erreur interne du moteur véhicule.".to_owned())?;
        let previous_check = {
            let mut st = i.state.lock().unwrap();
            std::mem::replace(&mut st.check,
                CheckState { phase: Phase::Running, elapsed_s: 0.0, total_s: duration_us as f64 / 1e6 })
        };
        let inner = i.clone();
        #[cfg(test)]
        let fail_spawn = i.fail_check_spawn.swap(false, Ordering::Relaxed);
        #[cfg(not(test))]
        let fail_spawn = false;
        let spawned = if fail_spawn {
            Err(std::io::Error::other("injected check thread creation failure"))
        } else {
            thread::Builder::new()
                .name("mission-check".into())
                .spawn(move || run_check(inner, open, seeds, duration_us))
        };
        if spawned.is_err() {
            // No worker exists to clear Running. Keep the previous result and
            // the live source untouched; ops still serialises all transitions.
            i.state.lock().unwrap().check = previous_check;
            return Err("Vérification impossible : thread non créé.".to_owned());
        }
        Ok(self.view())
    }

    /// The nominal part of the fiche of the open release 0.3 progression.
    pub fn fiche_get(&self) -> Result<NominalPart, String> {
        let open = self.inner.state.lock().unwrap().open.clone().ok_or_else(|| "Aucune mission ouverte.".to_owned())?;
        let session = open.session.as_ref().ok_or_else(|| NO_PROGRESSION.to_owned())?;
        Ok(session.nominal())
    }

    /// Records the nominal part of the fiche (shape checks only: nothing is compared before a verification).
    pub fn fiche_set(&self, part: NominalPart) -> Result<MissionView, String> {
        let open = self.inner.state.lock().unwrap().open.clone().ok_or_else(|| "Aucune mission ouverte.".to_owned())?;
        let session = open.session.as_ref().ok_or_else(|| NO_PROGRESSION.to_owned())?;
        if self.running() {
            return Err(RUNNING.into());
        }
        session.set_nominal(part)?;
        Ok(self.view())
    }

    /// The incident part `rank` of the fiche, once its incident is active (or done).
    pub fn incident_get(&self, rank: usize) -> Result<Option<IncidentPart>, String> {
        let open = self.inner.state.lock().unwrap().open.clone().ok_or_else(|| "Aucune mission ouverte.".to_owned())?;
        let session = open.session.as_ref().ok_or_else(|| NO_PROGRESSION.to_owned())?;
        Ok(session.incident(rank))
    }

    /// Records an incident part of the fiche (shape checks only).
    pub fn incident_set(&self, part: IncidentPart) -> Result<MissionView, String> {
        let open = self.inner.state.lock().unwrap().open.clone().ok_or_else(|| "Aucune mission ouverte.".to_owned())?;
        let session = open.session.as_ref().ok_or_else(|| NO_PROGRESSION.to_owned())?;
        if self.running() {
            return Err(RUNNING.into());
        }
        session.set_incident(part)?;
        Ok(self.view())
    }

    /// « Consigner la mesure » of the incident in progress: the conditions of the bench (interaction states, simulated
    /// time, the multimeter) go in the fiche; the raw frames of the last seconds and the circuit are sealed beside it.
    pub fn consign(&self) -> Result<MissionView, String> {
        let i = &self.inner;
        let _ops = i.ops.lock().unwrap();
        let open = i.state.lock().unwrap().open.clone().ok_or_else(|| "Aucune mission ouverte.".to_owned())?;
        let session = open.session.clone().ok_or_else(|| NO_PROGRESSION.to_owned())?;
        if self.running() {
            return Err(RUNNING.into());
        }
        let rank = session.active().map(|a| a.rank).ok_or_else(|| session::CONSIGN_REFUSED.to_owned())?;
        if !on_bench(i) {
            return Err(BENCH_LEFT.into());
        }
        let circuit = guarded(|| i.phys.circuit_copy()).ok().flatten().ok_or_else(|| BENCH_NOT_READY.to_owned())?;
        let values = guarded(|| i.phys.values(0)).ok().flatten().ok_or_else(|| BENCH_NOT_READY.to_owned())?;
        // the evidence is what the bus shows now: frames of the running session only (a stopped one leaves its last
        // frames in the ring, under other conditions)
        let no_frames = || "Aucune trame récente sur le bus : lancez le véhicule, puis consignez la mesure.".to_owned();
        let sid = i.lab.snapshot().status.session.as_ref().filter(|s| s.phase == "active").map(|s| s.id).ok_or_else(no_frames)?;
        let rows: Vec<_> = i.lab.recent_rows().into_iter().filter(|r| r.session_id == sid).collect();
        let last = rows.last().map(|r| r.t_us).ok_or_else(no_frames)?;
        let frames: Vec<PreuveFrame> =
            rows.iter().filter(|r| r.t_us + PREUVE_US >= last).map(|r| PreuveFrame { t_us: r.t_us, id: r.can_id, flags: r.flags, data: r.data.clone() }).collect();
        let label = |p: &Option<PinRef>| -> Option<String> {
            let p = p.as_ref()?;
            circuit.components().get(&p.comp).map(|c| format!("{}.{}", c.label, p.pin))
        };
        let meter = MeterRecord {
            com: label(&values.meter.com),
            vohm: label(&values.meter.vohm),
            fonction: (values.meter.com.is_some() || values.meter.vohm.is_some()).then(|| match values.meter.function {
                MeterFn::VDc => "V_DC".to_owned(),
                MeterFn::Ohm => "OHM".to_owned(),
            }),
            lecture: Some(values.meter.display.clone()),
        };
        let etats = circuit
            .components()
            .values()
            .filter_map(|c| circuit.state(c.id).filter(|s| !s.is_empty()).map(|s| (c.label.clone(), s.clone())))
            .collect();
        let conditions = Conditions { etats, t_sim_us: (values.sim_time_ns.max(0) / 1000) as u64, multimetre: meter };
        let part = session.incident_part(rank).unwrap_or(IncidentPart { rang: rank as u8, ..Default::default() });
        let grandeur = part.etat_observe.map(|e| incident::imposed(e, part.cible.kind).1).unwrap_or_else(|| "—".to_owned());
        let preuve = Preuve { frames, circuit: n7rt_circuit::persist::save(&circuit) };
        session.consign(rank, grandeur, conditions, &preuve, now_s())?;
        Ok(self.view())
    }

    /// R-11 « Reproduire l'anomalie » (E1–E3, the step not validated, no attempt running): the descriptor in force goes
    /// back on the live bench, a wire defect on the wire now joining its two pins, a setting to the incident's value;
    /// an anomaly of the vehicle side is in force already and nothing changes. The circuit stays the student's: it
    /// must be repaired again before the verification.
    pub fn reproduce(&self) -> Result<MissionView, String> {
        let i = &self.inner;
        let _ops = i.ops.lock().unwrap();
        let open = i.state.lock().unwrap().open.clone().ok_or_else(|| "Aucune mission ouverte.".to_owned())?;
        let session = open.session.clone().ok_or_else(|| NO_PROGRESSION.to_owned())?;
        if self.running() {
            return Err(RUNNING.into());
        }
        if !on_bench(i) {
            return Err(BENCH_LEFT.into());
        }
        let (_, content) = active_incident(&session).ok_or_else(|| session::REPRODUCE_REFUSED.to_owned())?;
        let circuit = guarded(|| i.phys.circuit_copy()).ok().flatten().ok_or_else(|| BENCH_NOT_READY.to_owned())?;
        let system = |op: SystemOp| {
            let _ = guarded(|| i.phys.command_internal(PhysCommand::System { op }));
        };
        let mut carrier = None;
        match &content.apply {
            Apply::WireDefect { a, b, defect } => {
                system(SystemOp::ClearDefects);
                if let (Some(a), Some(b)) = (pin_of(&circuit, a), pin_of(&circuit, b)) {
                    system(SystemOp::AttachDefect { a, b, defect: *defect });
                }
                carrier = guarded(|| i.phys.defects()).ok().flatten().and_then(|(carriers, _)| carriers.iter().next().map(|w| session.tag(w.0)));
            }
            Apply::Setting { designator, param, value } => {
                if let Some(comp) = circuit.components().values().find(|x| &x.label == designator).map(|x| x.id) {
                    system(SystemOp::SetParam { comp, name: param.clone(), value: Value::Num(*value) });
                }
            }
            _ => {}
        }
        session.reproduce(now_s(), carrier)?;
        keep_bench(i, &open, &session);
        Ok(self.view())
    }

    /// The folder of the final fiches: `Documents/N7RT CAN Lab/fiches`.
    fn fiches_dir(&self) -> Option<PathBuf> {
        self.inner.cfg.documents_dir.as_ref().map(|d| d.join(RUNS_SUBDIR[0]).join("fiches"))
    }

    /// E5: opens the final fiche of the closed mission (exported again when the export is missing).
    pub fn fiche_open(&self) -> Result<MissionView, String> {
        let open = self.inner.state.lock().unwrap().open.clone().ok_or_else(|| "Aucune mission ouverte.".to_owned())?;
        let session = open.session.as_ref().ok_or_else(|| NO_PROGRESSION.to_owned())?;
        let root = self.fiches_dir().ok_or_else(|| "Dossier Documents introuvable.".to_owned())?;
        let page = match session.exported_page(&root) {
            Some(p) => p,
            None => session.export(&root, now_s())?,
        };
        open_external(&page.to_string_lossy()).map_err(|_| "Ouverture de la fiche finale impossible.".to_owned())?;
        Ok(self.view())
    }

    /// Starts the verification of the current step of a release 0.3 progression in the background; `view()` follows
    /// it. E0: the nominal fiche against the sealed truth, the student's circuit against the reference bench, and the
    /// hidden nominal test replayed to the board and compared with the sealed expected outputs.
    pub fn verify(&self) -> Result<MissionView, String> {
        let i = &self.inner;
        let _ops = i.ops.lock().unwrap();
        let open = i.state.lock().unwrap().open.clone().ok_or_else(|| "Ouvrez d'abord une mission.".to_owned())?;
        let session = open.session.clone().ok_or_else(|| NO_PROGRESSION.to_owned())?;
        if self.running() {
            return Err(RUNNING.into());
        }
        let snapshot = i.lab.snapshot();
        if !board_ready(&snapshot) {
            return Err(BOARD_NOT_READY.into());
        }
        if !on_bench(i) {
            return Err(BENCH_LEFT.into());
        }
        let crc = snapshot.status.board.as_ref().map(|b| b.student_crc32.clone()).unwrap_or_default();
        keep_bench(i, &open, &session);
        let live = guarded(|| i.phys.circuit_copy()).ok().flatten().ok_or_else(|| BENCH_NOT_READY.to_owned())?;
        let circuit_sha256 = sha256_hex(n7rt_circuit::persist::save(&live).as_bytes());
        let uid = snapshot.status.board.as_ref().map(|b| b.uid.clone()).unwrap_or_default();
        let (step, test) = session.begin(now_s(), attempt_id(), circuit_sha256, crc, uid)?;
        let total_s = test.vector.duration_us as f64 / 1e6;
        let previous = {
            let mut st = i.state.lock().unwrap();
            std::mem::replace(&mut st.check, CheckState { phase: Phase::Running, elapsed_s: 0.0, total_s })
        };
        let (inner, worker_session) = (i.clone(), session.clone());
        let spawned = thread::Builder::new()
            .name("mission-verify".into())
            .spawn(move || run_attempt(inner, open, worker_session, step, test, live));
        if spawned.is_err() {
            i.state.lock().unwrap().check = previous;
            let _ = session.finish(StepOutcome::Cancelled { reason: CANCEL_INCOMPLETE.into() }, now_s());
            return Err("Vérification impossible : thread non créé.".to_owned());
        }
        Ok(self.view())
    }

    /// Opens a library document: the local copy when it matches its manifest digest, else the publisher's address.
    pub fn library_open(&self, index: usize) -> Result<(), String> {
        let open = self.inner.state.lock().unwrap().open.clone().ok_or_else(|| "Aucune mission ouverte.".to_owned())?;
        let entry = open.library.get(index).ok_or_else(|| "Document inconnu.".to_owned())?;
        let target = self.target(entry).ok_or_else(|| "Document indisponible.".to_owned())?;
        open_external(&target).map_err(|_| "Ouverture du document impossible.".to_owned())
    }

    fn target(&self, e: &LibEntry) -> Option<String> {
        if let Some((path, sha)) = e.local.as_ref().filter(|(p, _)| p.is_file()) {
            let ok = *self.inner.verified.lock().unwrap().entry(path.clone()).or_insert_with(|| sha256_file(path).as_deref() == Some(sha));
            if ok {
                return Some(path.to_string_lossy().into_owned());
            }
            eprintln!("library: a local copy does not match its manifest digest; the publisher's address is opened instead");
        }
        e.url.clone()
    }

    /// The application is closing: a running check ends as « application fermée »; the bench is kept.
    pub fn shutdown(&self) {
        // after any command in progress (« Reproduire » between its bench changes): the bench kept is a whole one
        let _ops = self.inner.ops.lock().unwrap_or_else(|e| e.into_inner());
        if !self.running() {
            let open = self.inner.state.lock().unwrap().open.clone();
            if let Some((o, s)) = open.as_ref().and_then(|o| o.session.clone().map(|s| (o, s))) {
                keep_anomaly(&self.inner, &s);
                keep_bench(&self.inner, o, &s);
                s.flush_active();
            }
        }
        self.inner.closing.store(true, Ordering::Relaxed);
    }
}

// ------------------------------------------------------------------------------------------------- check thread

fn set_elapsed(i: &Inner, s: f64) {
    let mut st = i.state.lock().unwrap();
    st.check.elapsed_s = s.min(st.check.total_s).max(0.0);
}

fn wait_until(i: &Inner, timeout: Duration, what: &str, pred: impl Fn(&UiBatch) -> bool) -> Result<UiBatch, Cancel> {
    let deadline = Instant::now() + timeout;
    loop {
        if i.closing.load(Ordering::Relaxed) {
            return Err(Cancel::app());
        }
        let s = i.lab.snapshot();
        if pred(&s) {
            return Ok(s);
        }
        if board_gone(&s) {
            return Err(Cancel::board(format!("board lost while waiting for {what}")));
        }
        if Instant::now() > deadline {
            return Err(Cancel::incomplete(format!("timeout waiting for {what}")));
        }
        thread::sleep(Duration::from_millis(20));
    }
}

/// The check session still runs cleanly: same session, board present and answering, no loss, no NACK, no link error,
/// same student code.
fn still_clean(s: &UiBatch, sid: u32, base_link: u64, crc: &Option<String>) -> Result<(), Cancel> {
    if board_gone(s) {
        return Err(Cancel::board("board lost"));
    }
    match s.status.session.as_ref() {
        Some(x) if x.id == sid && x.phase == "active" => {}
        _ => return Err(Cancel::incomplete("check session ended")),
    }
    if s.status.stalled {
        return Err(Cancel::incomplete("board mute (STALLED)"));
    }
    if s.stats.nacks > 0 || s.stats.lost > 0 {
        return Err(Cancel::incomplete(format!("{} NACK, {} lost frames", s.stats.nacks, s.stats.lost)));
    }
    if link_errors(s) != base_link {
        return Err(Cancel::incomplete("link errors during the check"));
    }
    if s.status.board.as_ref().map(|b| b.student_crc32.clone()) != *crc {
        return Err(Cancel::incomplete("student code changed during the check"));
    }
    Ok(())
}

/// Plays the check and returns what crossed the link.
fn check_body(i: &Inner, open: &Open, factory: Box<dyn SourceFactory>, duration_us: u64) -> Result<CheckRecord, Cancel> {
    let lab = &i.lab;
    let base = lab.snapshot();
    let base_link = link_errors(&base);
    let crc = base.status.board.as_ref().map(|b| b.student_crc32.clone());
    if base.status.session.is_some() {
        lab.command(Command::StopSession);
    }
    wait_until(i, SESSION_WAIT, "no session", |s| s.status.session.is_none())?;
    lab.set_bus_source(SourceKind::Mission(factory));
    lab.check_record_start();
    lab.command(Command::StartSession { tick_period_ms: open.tick_period_ms });
    let started = wait_until(i, SESSION_WAIT, "the check session", |s| s.status.session.as_ref().is_some_and(|x| x.phase == "active"))?;
    let sid = started.status.session.as_ref().map(|x| x.id).unwrap_or_default();
    let t0 = Instant::now();
    let limit = Duration::from_micros(duration_us) + CHECK_SLACK;
    let mut done_at: Option<Instant> = None;
    loop {
        thread::sleep(POLL);
        if i.closing.load(Ordering::Relaxed) {
            return Err(Cancel::app());
        }
        still_clean(&lab.snapshot(), sid, base_link, &crc)?;
        let p = lab.check_record_progress();
        if p.overflow {
            return Err(Cancel::incomplete("record bound reached"));
        }
        set_elapsed(i, lab.bus_time_us().min(duration_us) as f64 / 1e6);
        match done_at {
            Some(t) if t.elapsed() >= CHECK_GRACE => break,
            Some(_) => {}
            None if p.last_tick_us.is_some_and(|t| t >= duration_us) => done_at = Some(Instant::now()),
            None => {}
        }
        if t0.elapsed() > limit {
            return Err(Cancel::incomplete(format!("not finished after {} s", limit.as_secs())));
        }
    }
    let rec = lab.check_record_stop();
    if rec.overflow || rec.session_id != Some(sid) {
        return Err(Cancel::incomplete("record of another session or incomplete"));
    }
    Ok(rec)
}

/// Back to the live slider: mission source, sensor position of before the check, a normal session.
fn restore(i: &Inner, open: &Open, position: std::collections::BTreeMap<String, f64>) -> Result<(), String> {
    let _ = i.lab.check_record_stop();
    if i.closing.load(Ordering::Relaxed) {
        return Ok(());
    }
    i.lab.quiesce_bus_source()?;
    for (quantity, value) in position {
        let interact = if quantity == "position" { Interact::SetPot { comp: open.sensor, position: value } }
            else { Interact::SetWorld { comp: open.sensor, quantity, value } };
        match guarded(|| i.phys.command_internal(PhysCommand::Interact { interact })) {
            Ok(Ok(_)) => {}
            _ => return Err("La position physique ne peut pas être restaurée.".into()),
        }
    }
    i.lab.set_bus_source(SourceKind::Mission(Box::new(live_factory(i, open))));
    let s = i.lab.snapshot();
    if s.status.session.is_some() {
        i.lab.command(Command::StopSession);
    }
    if !board_gone(&s) && s.status.board.is_some() {
        i.lab.command(Command::StartSession { tick_period_ms: open.tick_period_ms });
    }
    Ok(())
}

fn run_check(inner: Arc<Inner>, open: Arc<Open>, seeds: Seeds, duration_us: u64) {
    let started = SystemTime::now();
    let position = sensor_position(&inner.phys, open.sensor);
    let board = inner.lab.snapshot().status.board;
    // Bench read-only and its interaction states frozen in the UI for the whole check (ch. 24 §2.9.1 step 3, §7.5).
    let _ = guarded(|| inner.phys.set_check_lock(true));
    let factory = Box::new(CheckFactory { pack: open.pack.clone(), inputs: open.inputs.clone(), sensor: open.sensor, seeds });
    let mut recorded = catch_unwind(AssertUnwindSafe(|| check_body(&inner, &open, factory, duration_us)))
        .unwrap_or_else(|_| Err(Cancel::incomplete("internal error during the check")));
    if restore(&inner, &open, position).is_err() {
        recorded = Err(Cancel::incomplete("physical source or position could not be restored"));
    }
    let _ = guarded(|| inner.phys.set_check_lock(false));
    let tick_us = open.tick_period_ms as u64 * 1000;
    let (cohort, group, pack_id) = match &open.kind {
        OpenKind::Group(h) => (h.cohort.clone(), h.group.clone(), h.pack_id.clone()),
        OpenKind::Reference { .. } => Default::default(),
    };
    let b = board.as_ref();
    let meta = RunMeta {
        mission: open.id.clone(),
        cohort,
        group,
        pack_id,
        app_version: if inner.cfg.app_version.is_empty() { env!("CARGO_PKG_VERSION").to_owned() } else { inner.cfg.app_version.clone() },
        board_uid: b.map(|b| b.uid.clone()).unwrap_or_default(),
        runtime_build: b.map(|b| format!("{} {}", b.runtime, b.build_id).trim().to_owned()).unwrap_or_default(),
        student_crc32: b.map(|b| b.student_crc32.clone()).unwrap_or_default(),
        seed_vehicle: seeds.vehicle,
        seed_scenario: seeds.scenario,
        duration_us,
        tick_us,
        started: Some(started),
    };
    let outcome = recorded.and_then(|rec| {
        let run = run_record(&meta, &rec);
        match &open.kind {
            OpenKind::Reference { .. } => judge(&open.pack, &run, &rec, duration_us, tick_us).map(Outcome::Judged),
            OpenKind::Group(_) => record(inner.cfg.documents_dir.as_deref(), &run, &rec, started).map(Outcome::Recorded),
        }
    });
    if let OpenKind::Reference { reports } = &open.kind {
        write_report(reports.as_deref(), &open.id, started, seeds, duration_us, board.as_ref(), &outcome);
    }
    if let Ok(Outcome::Recorded(path)) = &outcome {
        inner.access.lock().unwrap().last_run = Some(path.clone());
    }
    let mut st = inner.state.lock().unwrap();
    st.check.phase = match &outcome {
        Ok(Outcome::Judged(j)) => Phase::Done(j.verdict.pass),
        Ok(Outcome::Recorded(path)) => Phase::Recorded(file_name(path).unwrap_or_default()),
        Err(c) => Phase::Cancelled(c.reason),
    };
    if outcome.is_ok() {
        st.check.elapsed_s = st.check.total_s;
    }
}

/// One verification of a release 0.3 progression: the hidden test of the step replayed to the board under the bench
/// lock, then the verdict of the session (PASS ⇔ fiche conforming ∧ circuit equal to the reference bench ∧ board
/// outputs equal to the sealed expected outputs). A lost tick, a NACK, a changed code or a delivery that differs from
/// the vector cancels the attempt instead of failing it (ch. 24 §2.9.5).
fn run_attempt(inner: Arc<Inner>, open: Arc<Open>, session: Arc<Session>, step: usize, test: HiddenTestRef, live: n7rt_circuit::Circuit) {
    let started = SystemTime::now();
    let position = sensor_position(&inner.phys, open.sensor);
    let board = inner.lab.snapshot().status.board;
    let _ = guarded(|| inner.phys.set_check_lock(true));
    let duration_us = test.vector.duration_us;
    let reference = guarded(|| n7rt_circuit::persist::load(open.pack.bench_json()).ok()).ok().flatten();
    let content = session.workflow.steps.get(step).and_then(|s| s.incident.clone());
    let active = session.active();
    // P-1 … P-4 on the circuit at the start of the attempt (ch. 24 §2.9.2), kept in memory
    let defects = guarded(|| inner.phys.defects()).ok().flatten();
    let bench_ok = match &reference {
        Some(r) => physical_checks(&session, &live, r, content.as_ref(), active.as_ref(), defects),
        None => false,
    };
    let factory = Box::new(ReplayFactory::new(&test.vector));
    let mut recorded = catch_unwind(AssertUnwindSafe(|| check_body(&inner, &open, factory, duration_us)))
        .unwrap_or_else(|_| Err(Cancel::incomplete("internal error during the verification")));
    if restore(&inner, &open, position).is_err() {
        recorded = Err(Cancel::incomplete("physical source or position could not be restored"));
    }
    let _ = guarded(|| inner.phys.set_check_lock(false));
    let tick_us = open.tick_period_ms as u64 * 1000;
    if let Ok(rec) = &recorded {
        session.set_bus(sha256_hex(format!("{:?}\n{:?}", rec.frames, rec.ticks).as_bytes()));
    }
    let outcome = match recorded {
        Err(c) => StepOutcome::Cancelled { reason: c.reason.to_owned() },
        Ok(rec) if !test.vector.delivered_exactly(&rec.frames) => StepOutcome::Cancelled { reason: CANCEL_INCOMPLETE.to_owned() },
        Ok(rec) => match board_ticks(&rec.ticks, duration_us, tick_us) {
            None => StepOutcome::Cancelled { reason: CANCEL_INCOMPLETE.to_owned() },
            Some(ticks) if step == crate::workflow::progress::QUALIFICATION => match compare_competences(&test.expected, &ticks) {
                // E4 (ch. 24 §6): the Physical Lab is not involved, there is no fiche part, PASS ⇔ eight competences
                Some(cps) => {
                    let all = cps.iter().all(|c| *c);
                    StepOutcome::Judged { pass: all, diagnostic: None, verified: Some(all), competences: Some(cps.to_vec()) }
                }
                None => StepOutcome::Cancelled { reason: CANCEL_INCOMPLETE.to_owned() },
            },
            Some(ticks) => {
                let compared = compare_ticks(&test.expected, &ticks);
                let measure_ok = match &content {
                    Some(c) => guarded(|| key_measure(&session, &open.pack, step, c)).unwrap_or(false),
                    None => true,
                };
                let diagnostic = session.diagnostic(step, measure_ok);
                let verified = bench_ok && compared.pass;
                let mut written = true;
                if step == 0 {
                    // The hidden nominal test is also kept as a run file: the instructor tool re-judges it with the
                    // reference consumer (evidence of the in-app verdict).
                    let (cohort, group, pack_id) = match &open.kind {
                        OpenKind::Group(h) => (h.cohort.clone(), h.group.clone(), h.pack_id.clone()),
                        OpenKind::Reference { .. } => Default::default(),
                    };
                    let b = board.as_ref();
                    let meta = RunMeta {
                        mission: open.id.clone(),
                        cohort,
                        group,
                        pack_id,
                        app_version: if inner.cfg.app_version.is_empty() { env!("CARGO_PKG_VERSION").to_owned() } else { inner.cfg.app_version.clone() },
                        board_uid: b.map(|b| b.uid.clone()).unwrap_or_default(),
                        runtime_build: b.map(|b| format!("{} {}", b.runtime, b.build_id).trim().to_owned()).unwrap_or_default(),
                        student_crc32: b.map(|b| b.student_crc32.clone()).unwrap_or_default(),
                        seed_vehicle: 0,
                        seed_scenario: 0,
                        duration_us,
                        tick_us,
                        started: Some(started),
                    };
                    let run = run_record(&meta, &rec);
                    match record(inner.cfg.documents_dir.as_deref(), &run, &rec, started) {
                        Ok(path) => inner.access.lock().unwrap().last_run = Some(path),
                        Err(_) => written = false,
                    }
                }
                if written {
                    StepOutcome::Judged { pass: diagnostic && verified, diagnostic: Some(diagnostic), verified: Some(verified), competences: None }
                } else {
                    // without its run file the attempt proves nothing to the instructor: cancelled, not counted
                    StepOutcome::Cancelled { reason: CANCEL_WRITE.to_owned() }
                }
            }
        },
    };
    let transition = session.finish(outcome, now_s());
    // T3: the anomaly of the validated step goes, the next one comes, on the live bench and source
    if let Ok(t) = transition {
        // the repair record of a validated incident needs only the circuit of the attempt: kept even when the
        // application is closing
        if t.verdict == AttemptVerdict::Pass && (1..=3).contains(&step) {
            let _ = session.record_validation(step, &live);
        }
        if t.verdict == AttemptVerdict::Pass && inner.closing.load(Ordering::Relaxed) {
            // no bench to change any more: the next incident's record opens on the circuit of the attempt, its
            // anomaly goes on the bench at the next opening
            if let Some(k) = t.opened.filter(|k| (1..=3).contains(k)) {
                let _ = session.record_activation(k, &n7rt_circuit::persist::save(&live));
            }
        }
        if t.verdict == AttemptVerdict::Pass && !inner.closing.load(Ordering::Relaxed) {
            if let Some(old) = &content {
                remove_physical(&inner, old);
            }
            if let Some(k) = t.opened.filter(|k| (1..=3).contains(k)) {
                apply_physical(&inner, &session, true, true);
                if let Some(circuit) = guarded(|| inner.phys.circuit_copy()).ok().flatten() {
                    let _ = session.record_activation(k, &n7rt_circuit::persist::save(&circuit));
                }
            }
            let vehicle_side = |c: Option<&IncidentContent>| c.is_some_and(|c| matches!(c.apply, Apply::FrameFault { .. } | Apply::SourceOverride { .. }));
            let next = session.active().and_then(|a| session.workflow.steps.get(a.rank).and_then(|s| s.incident.clone()));
            if vehicle_side(content.as_ref()) || vehicle_side(next.as_ref()) {
                relive(&inner, &open);
            }
            keep_bench(&inner, &open, &session);
            // E5: the mission is closed, the final fiche goes to the Documents folder (retried from the view)
            if t.closed {
                if let Some(root) = inner.cfg.documents_dir.as_ref().map(|d| d.join(RUNS_SUBDIR[0]).join("fiches")) {
                    let _ = session.export(&root, now_s());
                }
            }
        }
    }
    inner.state.lock().unwrap().check = CheckState::default();
}

/// The designated parts of the live bench and their pins (« désignateur.broche »), in natural order.
fn bench_choices(phys: &PhysLab) -> (Vec<String>, Vec<String>) {
    let Some(circuit) = guarded(|| phys.circuit_copy()).ok().flatten() else { return (Vec::new(), Vec::new()) };
    let natural = |s: &String| -> (String, u64) {
        let digits: String = s.chars().rev().take_while(|c| c.is_ascii_digit()).collect::<Vec<_>>().into_iter().rev().collect();
        (s[..s.len() - digits.len()].to_owned(), digits.parse().unwrap_or(0))
    };
    let mut parts: Vec<String> = circuit
        .components()
        .values()
        .filter(|c| !c.label.is_empty() && !matches!(c.kind, n7rt_circuit::Kind::Junction | n7rt_circuit::Kind::Ground))
        .map(|c| c.label.clone())
        .collect();
    parts.sort_by_key(natural);
    let mut pins: Vec<String> = circuit
        .components()
        .values()
        .filter(|c| parts.contains(&c.label))
        .flat_map(|c| n7rt_circuit::kind_def(c.kind).pins.iter().map(move |p| format!("{}.{}", c.label, p.id)))
        .collect();
    pins.sort_by(|a, b| {
        let (da, pa) = a.split_once('.').unwrap_or((a, ""));
        let (db, pb) = b.split_once('.').unwrap_or((b, ""));
        natural(&da.to_owned()).cmp(&natural(&db.to_owned())).then(pa.cmp(pb))
    });
    (parts, pins)
}

/// Bus time kept as the evidence of a recorded measure (the spec's 2,000 s, plus room for 100 frames of 20 ms).
const PREUVE_US: u64 = 2_500_000;

/// The anomaly in force and its sealed content.
fn active_incident(session: &Session) -> Option<(Active, IncidentContent)> {
    let a = session.active()?;
    let c = session.workflow.steps.get(a.rank)?.incident.clone()?;
    Some((a, c))
}

/// The live source of the open mission, with the anomaly in force when it acts on the vehicle.
fn live_factory(i: &Inner, open: &Open) -> LiveFactory {
    let mut pack = open.pack.clone();
    let mut fault = None;
    if let Some((_, c)) = open.session.as_deref().and_then(active_incident) {
        match &c.apply {
            Apply::FrameFault { id, byte, mask, period, burst, phase } => {
                fault = Some(FrameFault { id: *id, byte: *byte, mask: *mask, period: *period, burst: *burst, phase: *phase })
            }
            Apply::SourceOverride { set } => {
                if let Ok(Ok(p)) = guarded(|| open.pack.with_source_overrides(set)) {
                    pack = Arc::new(p);
                }
            }
            _ => {}
        }
    }
    LiveFactory { pack, inputs: open.inputs.clone(), seed: i.cfg.fixed_seed, fault }
}

/// Restarts the live session on the live source of now (after a change of the anomaly in force).
fn relive(i: &Inner, open: &Open) {
    if i.lab.quiesce_bus_source().is_err() {
        return;
    }
    i.lab.set_bus_source(SourceKind::Mission(Box::new(live_factory(i, open))));
    let s = i.lab.snapshot();
    if s.status.session.is_some() {
        i.lab.command(Command::StopSession);
    }
    if !board_gone(&s) && s.status.board.is_some() {
        i.lab.command(Command::StartSession { tick_period_ms: open.tick_period_ms });
    }
}

/// The wire of `circuit` whose keyed tag is `tag`.
fn tagged_wire(session: &Session, circuit: &Circuit, tag: &str) -> Option<WireId> {
    circuit.wires().keys().copied().find(|w| session.tag(w.0) == tag)
}

fn pin_of(circuit: &Circuit, p: &PinName) -> Option<PinRef> {
    circuit.components().values().find(|x| x.label == p.0).map(|x| PinRef { comp: x.id, pin: p.1.clone() })
}

/// Puts the physical part of the anomaly in force on the live bench: at its activation (`fresh`), or at a reopening
/// (the wire defect goes back on its carrier, found by its tag; a setting is re-applied only to a bench that was not
/// restored, a restored bench holding it already, or its repair).
fn apply_physical(i: &Inner, session: &Session, fresh: bool, restored: bool) {
    if !on_bench(i) {
        return;
    }
    let Some((active, c)) = active_incident(session) else { return };
    let Some(circuit) = guarded(|| i.phys.circuit_copy()).ok().flatten() else { return };
    match &c.apply {
        Apply::WireDefect { a, b, defect } => {
            let op = match active.carrier.as_deref().filter(|_| !fresh) {
                Some(tag) => match tagged_wire(session, &circuit, tag) {
                    Some(wire) => SystemOp::DefectOn { wire, defect: *defect },
                    // the carrier is gone: the student replaced the wire
                    None => return,
                },
                None => match (pin_of(&circuit, a), pin_of(&circuit, b)) {
                    (Some(a), Some(b)) => SystemOp::AttachDefect { a, b, defect: *defect },
                    _ => return,
                },
            };
            let _ = guarded(|| i.phys.command_internal(PhysCommand::System { op }));
            track_carrier(i, session);
        }
        Apply::Setting { designator, param, value } if fresh || !restored => {
            if let Some(comp) = circuit.components().values().find(|x| &x.label == designator).map(|x| x.id) {
                let op = SystemOp::SetParam { comp, name: param.clone(), value: Value::Num(*value) };
                let _ = guarded(|| i.phys.command_internal(PhysCommand::System { op }));
            }
        }
        _ => {}
    }
}

/// Removes the physical part of a validated incident (P-3 and P-4 proved the repair; a setting stays as repaired).
fn remove_physical(i: &Inner, content: &IncidentContent) {
    if matches!(content.apply, Apply::WireDefect { .. }) {
        let _ = guarded(|| i.phys.command_internal(PhysCommand::System { op: SystemOp::ClearDefects }));
    }
}

/// Records the carrier of the wire defect in force once a wire carries it.
fn track_carrier(i: &Inner, session: &Session) {
    let Some((active, c)) = active_incident(session) else { return };
    if !matches!(c.apply, Apply::WireDefect { .. }) || active.carrier.is_some() {
        return;
    }
    if let Some((carriers, _)) = guarded(|| i.phys.defects()).ok().flatten() {
        if let Some(w) = carriers.iter().next() {
            let _ = session.set_carrier(Some(session.tag(w.0)));
        }
    }
}

/// The Physical Lab still holds a mission bench (another document replaces it on « Nouveau », « Ouvrir »,
/// « Démonstration »): the mission never saves, changes or judges anything else.
fn on_bench(i: &Inner) -> bool {
    guarded(|| i.phys.on_mission_bench()).unwrap_or(false)
}

/// Keeps the wire defect in force where it belongs: a pending defect that found its wire is recorded; a defect the
/// engine lost (restart after a panic) goes back on its carrier, or waits again for its wire; a carrier merged into
/// another wire by the junction cleanup hands its tag to that wire, which carries the defect now.
fn keep_anomaly(i: &Inner, session: &Session) {
    if !on_bench(i) {
        return;
    }
    let Some((active, c)) = active_incident(session) else { return };
    let Apply::WireDefect { a, b, defect } = &c.apply else { return };
    let Some((carriers, pending)) = guarded(|| i.phys.defects()).ok().flatten() else { return };
    let Some(circuit) = guarded(|| i.phys.circuit_copy()).ok().flatten() else { return };
    let system = |op: SystemOp| {
        let _ = guarded(|| i.phys.command_internal(PhysCommand::System { op }));
    };
    match active.carrier {
        None if carriers.is_empty() && !pending => {
            if let (Some(a), Some(b)) = (pin_of(&circuit, a), pin_of(&circuit, b)) {
                system(SystemOp::AttachDefect { a, b, defect: *defect });
            }
            track_carrier(i, session);
        }
        None => track_carrier(i, session),
        Some(tag) if carriers.is_empty() && !pending => {
            if let Some(wire) = tagged_wire(session, &circuit, &tag) {
                system(SystemOp::DefectOn { wire, defect: *defect });
            }
        }
        Some(tag) => {
            // the tag follows the defect: to the wire a junction merge gave it, and back to the original wire when the
            // merge is undone (the merged wire may still exist then, without the defect)
            let tagged = tagged_wire(session, &circuit, &tag);
            if tagged.map_or(true, |w| !carriers.contains(&w)) {
                if let Some(w) = carriers.iter().find(|w| circuit.wires().contains_key(w)) {
                    let _ = session.set_carrier(Some(session.tag(w.0)));
                }
            }
        }
    }
}

/// Keeps the student's bench sealed with the progression when its text changed (the document itself, not the
/// published values, which lag behind a command by up to one publication period).
fn keep_bench(i: &Inner, open: &Open, session: &Session) {
    if !on_bench(i) {
        return;
    }
    let Some(circuit) = guarded(|| i.phys.circuit_copy()).ok().flatten() else { return };
    let text = n7rt_circuit::persist::save(&circuit);
    let digest = text.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3)).max(1);
    if open.bench_digest.load(Ordering::Relaxed) == digest {
        return;
    }
    if session.save_bench(&text).is_ok() {
        open.bench_digest.store(digest, Ordering::Relaxed);
    }
}

/// P-1 … P-4 (ch. 24 §2.9.2): connectivity and parameters equal to the reference (the incident's repair settings
/// exempted), no hidden defect on an existing wire, and the incident's target repaired.
fn physical_checks(
    session: &Session,
    live: &Circuit,
    reference: &Circuit,
    content: Option<&IncidentContent>,
    active: Option<&Active>,
    defects: Option<(std::collections::BTreeSet<WireId>, bool)>,
) -> bool {
    let exempt: Vec<(&str, &str)> = content.map(|c| c.repair.exempt.iter().map(|(d, p)| (d.as_str(), p.as_str())).collect()).unwrap_or_default();
    let p12 = pcheck::same_connectivity(live, reference) && pcheck::same_parameters(live, reference, &exempt);
    let p3 = defects.is_some_and(|(carriers, _)| carriers.iter().all(|w| !live.wires().contains_key(w)));
    let param = |designator: &str, name: &str| -> Option<f64> {
        let c = live.components().values().find(|x| x.label == designator)?;
        match c.params.get(name)? {
            Value::Num(v) => Some(*v),
            _ => None,
        }
    };
    let p4 = content.is_none_or_all(|c| {
        c.repair.p4.iter().all(|check| match check {
            P4::WireReplaced => active.and_then(|a| a.carrier.as_deref()).is_some_and(|tag| tagged_wire(session, live, tag).is_none()),
            P4::ParamInRange { designator, param: name, lo, hi, changed } => param(designator, name).is_some_and(|v| {
                let set = match &c.apply {
                    Apply::Setting { designator: d, param: q, value } if d == designator && q == name => Some(*value),
                    _ => None,
                };
                v >= *lo && v <= *hi && (!changed || set != Some(v))
            }),
        })
    });
    p12 && p3 && p4
}

trait NoneOrAll<T> {
    fn is_none_or_all(self, f: impl FnOnce(T) -> bool) -> bool;
}

impl<T> NoneOrAll<T> for Option<T> {
    fn is_none_or_all(self, f: impl FnOnce(T) -> bool) -> bool {
        match self {
            None => true,
            Some(x) => f(x),
        }
    }
}

/// The key measure of the incident part of `step`: the student's value against the engine's value at the recorded
/// conditions, and pertinent (ch. 24 §3.4).
fn key_measure(session: &Session, pack: &Pack, step: usize, c: &IncidentContent) -> bool {
    let Some(part) = session.incident_part(step) else { return false };
    // the declared state imposes the quantity: another state than the truth cannot give its measure
    if part.etat_observe != Some(c.truth.state) {
        return false;
    }
    let Some(student) = incident::student_value(&part, &c.measure) else { return false };
    let Some(preuve) = session.preuve(step) else { return false };
    let anomaly = match &c.apply {
        Apply::WireDefect { defect, .. } => {
            // the wire that carried the defect when the measure was recorded, in the recorded circuit (a later
            // « Reproduire l'anomalie » or junction merge may have moved the defect since)
            let circuit = n7rt_circuit::persist::load(&preuve.circuit).ok();
            let wire = match (circuit, session.consigned_carrier(step)) {
                (Some(circuit), Some(tag)) => tagged_wire(session, &circuit, &tag),
                _ => None,
            };
            Anomaly::Wire { wire, defect: *defect }
        }
        Apply::FrameFault { id, byte, mask, period, burst, phase } => {
            Anomaly::Frame(FrameFault { id: *id, byte: *byte, mask: *mask, period: *period, burst: *burst, phase: *phase })
        }
        Apply::Setting { designator, param, .. } => {
            let Some(reference) = n7rt_circuit::persist::load(pack.bench_json())
                .ok()
                .and_then(|r| r.components().values().find(|x| &x.label == designator).and_then(|x| x.params.get(param).cloned()))
            else {
                return false;
            };
            Anomaly::Setting { designator: designator.clone(), param: param.clone(), reference }
        }
        Apply::SourceOverride { .. } => Anomaly::Source,
    };
    match measure::evaluate(&c.measure, &preuve, pack, &anomaly) {
        Some((expected, without)) => c.measure.judge(student, expected, without),
        None => false,
    }
}

// ------------------------------------------------------------------------------------------------------- report

/// UTC civil date and time of a Unix time (proleptic Gregorian, days algorithm of H. Hinnant).
fn utc(secs: u64) -> (i64, u32, u32, u32, u32, u32) {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    (y, m, d, (rem / 3600) as u32, (rem / 60 % 60) as u32, (rem % 60) as u32)
}

/// `20261006T142233Z`.
pub fn utc_stamp(t: SystemTime) -> String {
    let secs = t.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let (y, mo, d, h, mi, s) = utc(secs);
    format!("{y:04}{mo:02}{d:02}T{h:02}{mi:02}{s:02}Z")
}

/// The trainer's report (`<instructor>/missions/<id>/reports/<stamp>.json`). Never shown to the student.
fn write_report(
    dir: Option<&Path>,
    id: &str,
    started: SystemTime,
    seeds: Seeds,
    duration_us: u64,
    board: Option<&crate::runtime::BoardIdentity>,
    outcome: &Result<Outcome, Cancel>,
) {
    let Some(dir) = dir else { return };
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let stamp = utc_stamp(started);
    let Some(path) = unique_path(dir, &format!("{stamp}.json")) else { return };
    let mut report = serde_json::json!({
        "format": "n7rt-mission-report",
        "version": 1,
        "mission": id,
        "check": "nominal",
        "started_utc": stamp,
        "seed_vehicle": seeds.vehicle.to_string(),
        "seed_scenario": seeds.scenario.to_string(),
        "duration_us": duration_us,
        "board": board.map(|b| serde_json::json!({
            "runtime": b.runtime, "build_id": b.build_id, "uid": b.uid, "student_crc32": b.student_crc32,
        })),
    });
    let obj = report.as_object_mut().expect("object");
    match outcome {
        Ok(Outcome::Judged(j)) => {
            obj.insert("result".into(), (if j.verdict.pass { "pass" } else { "fail" }).into());
            obj.insert("pass".into(), j.verdict.pass.into());
            obj.insert("frames".into(), j.frames.into());
            obj.insert("ticks".into(), j.ticks.into());
            obj.insert("detail".into(), j.verdict.detail.clone().into());
        }
        Ok(Outcome::Recorded(_)) => {
            obj.insert("result".into(), "recorded".into());
        }
        Err(c) => {
            obj.insert("result".into(), "cancelled".into());
            obj.insert("reason".into(), c.reason.into());
            obj.insert("detail".into(), c.why.clone().into());
        }
    }
    if let Ok(text) = serde_json::to_string_pretty(&report) {
        let _ = std::fs::write(path, text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_dates() {
        assert_eq!(utc(0), (1970, 1, 1, 0, 0, 0));
        // 2026-10-06 14:22:33 UTC
        assert_eq!(utc(1_791_296_553), (2026, 10, 6, 14, 22, 33));
        assert_eq!(utc(951_782_400), (2000, 2, 29, 0, 0, 0));
    }

    #[test]
    fn clip_keeps_char_boundaries() {
        assert_eq!(clip("éèà", 2), "éè…");
        assert_eq!(clip("abc", 5), "abc");
    }

    #[test]
    fn run_ticks_mirror_the_board_ticks() {
        use crate::engine::RecEntry;
        let t = RecTick {
            t_us: 10_000,
            status: 0,
            entries: vec![RecEntry { slot: 0, kind: 0, value: 1.5 }, RecEntry { slot: 2, kind: 1, value: 0.0 }],
        };
        let r = run_tick(&t);
        assert_eq!(r.t_us, 10_000);
        assert_eq!(r.slots.len(), 8);
        assert_eq!((r.slots[0], r.slots[1], r.slots[2]), (Some(RunSlot::V(1.5)), None, Some(RunSlot::Reject)));
        let not_delivered = RecTick { status: 1, ..t };
        assert!(run_tick(&not_delivered).slots.iter().all(Option::is_none));
    }
}

#[cfg(all(test, n7rt_lab_oracle_tests))]
mod source_clock_tests {
    include!(concat!(env!("N7RT_LAB_ORACLE_TESTS"), "/source_clock.rs"));
}
