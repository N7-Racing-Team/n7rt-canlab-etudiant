//! The progression of an open release 0.3 group pack: the sealed workflow, the persisted progress and fiche, the
//! student view and the preconditions of a verification. The mission controller runs the hidden test itself; this
//! module decides, records and words the result with the exact public messages (private spec ch. 24 §7).
//!
//! Files (only written here, atomically, with an integrity digest): `<dir>/progress.json` (steps and attempts) and
//! `<dir>/fiche.json` (the structured fiche). A progression that does not verify is refused: the mission opens, the
//! fiche stays readable, verifications are refused until the instructor resets the mission.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use sha2::{Digest, Sha256};

use crate::workflow::fiche::{self, NominalPart};
use crate::workflow::incident::{self, Conditions, Consigned, IncidentPart};
use crate::workflow::measure::Preuve;
use crate::workflow::progress::{
    Active, AttemptVerdict, Evidence, Exported, Outcome, Owner, Progress, Refusal, StepState, Transition, CLOSURE, STEPS, VERIFIED_STEPS,
};
use crate::workflow::sealed::{HiddenTest, Workflow};
use crate::workflow::store;

pub const STEP_NAMES: [&str; STEPS] =
    ["Essai nominal", "Incident 1", "Incident 2", "Incident 3", "Qualification finale", "Clôture"];
const ORDER_E0: &str = "Essai nominal. Identifiez les trames de votre ECU sur le bus, décodez-les, programmez la carte et documentez votre décodage dans la fiche. Lancez la vérification quand votre code et votre fiche sont prêts.";
const ORDER_QUALIFICATION: &str = "Qualification finale. Une séquence d'essais cachée évalue votre code. Le résultat est donné par compétence.";
const ORDER_CLOSED: &str = "Mission clôturée. Votre fiche finale est disponible.";
const DETAIL_HIDDEN: &str = "Le détail n'est pas communiqué. Reprenez vos mesures et vos essais.";
pub const UNREADABLE: &str = "Progression illisible sur ce poste : demandez à l'encadrant de réinitialiser la mission.";
/// Another instance of the application holds the progression.
pub const IN_USE: &str = "Cette progression est déjà ouverte dans une autre fenêtre de CAN Lab : fermez cette fenêtre-là, puis \
                          rouvrez la mission.";
pub const UNAVAILABLE: &str = "Cette étape n'est pas encore disponible dans ce pack de groupe.";
pub const FROZEN: &str = "La partie nominale de la fiche est validée : ses champs contrôlés ne sont plus modifiables.";
pub const FICHE_UNREADABLE: &str = "Fiche illisible sur ce poste : demandez à l'encadrant de réinitialiser la mission.";
pub const FICHE_LOCKED: &str = "Vérification en cours : la fiche est figée pour l'essai.";
/// ch. 24 §7.3 M-06.
pub const BOARD: &str = "Connectez la carte et vérifiez que votre programme est chargé.";
pub const INCIDENT_FROZEN: &str = "Cette partie de la fiche est validée : ses champs contrôlés ne sont plus modifiables.";
pub const INCIDENT_CLOSED: &str = "Cette partie de la fiche n'est pas ouverte.";
pub const CONSIGN_REFUSED: &str = "La mesure ne peut être consignée que pendant l'incident en cours, hors vérification.";
/// R-11 outside an incident step in progress.
pub const REPRODUCE_REFUSED: &str = "« Reproduire l'anomalie » n'existe que pendant un incident non validé, hors vérification.";
pub const CLOSED_FICHE: &str = "La mission est clôturée : la fiche n'est plus modifiable.";
const EXPORTED: &str = "La fiche finale est exportée dans le dossier Documents.";
const NOT_EXPORTED: &str = "La fiche finale n'a pas pu être exportée : utilisez « Ouvrir la fiche finale » pour réessayer.";
const BENCH_FILE: &str = "bench.sealed";
pub const NOT_SAVED: &str = "La progression n'a pas pu être enregistrée sur ce poste : prévenez l'encadrant.";
const MAX_TEXT: usize = 2_000;
const MAX_ROWS: usize = 64;

fn order(step: usize) -> String {
    match step {
        0 => ORDER_E0.into(),
        1..=3 => format!("Incident {step}. Le véhicule revient d'essai avec une anomalie. Observez-la au banc, sur le bus et dans vos résultats ; consignez votre diagnostic dans la fiche ; réparez le véhicule ou votre code, ou les deux ; puis lancez « Vérifier la réparation »."),
        4 => ORDER_QUALIFICATION.into(),
        _ => ORDER_CLOSED.into(),
    }
}

fn button(step: usize) -> &'static str {
    match step {
        0 => "Lancer la vérification",
        1..=3 => "Vérifier la réparation",
        _ => "Lancer la qualification",
    }
}

fn state_text(s: StepState) -> &'static str {
    match s {
        StepState::Todo => "À FAIRE",
        StepState::Open => "EN COURS",
        StepState::Passed => "VALIDÉ",
        StepState::Failed => "ÉCHEC À CORRIGER",
    }
}

/// Local civil date and time of a Unix time, `JJ/MM/AAAA` and `HH:MM` (UTC when the local offset is unknown).
fn civil(unix_s: u64, offset_s: i64) -> (String, String) {
    let t = unix_s as i64 + offset_s;
    let days = t.div_euclid(86_400);
    let rem = t.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    (format!("{d:02}/{m:02}/{y:04}"), format!("{:02}:{:02}", rem / 3_600, rem % 3_600 / 60))
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct StepView {
    pub name: String,
    pub state: String,
    /// « le JJ/MM/AAAA à HH:MM » when validated.
    pub since: Option<String>,
    pub current: bool,
}

/// A document the fiche may cite: the mission library, same entries in the same order (ch. 24 §7.6).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DocumentChoice {
    /// What the fiche records: the manifest SHA-256 of the local copy, or the primary address.
    pub id: String,
    pub title: String,
}

/// What the interface shows of the progression: states, the work order, the button and the messages. Nothing of
/// the truth, of the hidden tests or of the checks that failed.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ProgressionView {
    pub readable: bool,
    pub steps: Vec<StepView>,
    pub current: Option<usize>,
    pub order: String,
    pub button: String,
    /// The verification can start now (preconditions met, nothing running).
    pub ready: bool,
    /// An attempt is running (the fiche is read-only for it; M-10 is in `blocked`).
    pub running: bool,
    /// Why not, with the exact message (M-04, M-05, unavailable, unreadable…).
    pub blocked: Option<String>,
    /// Lines of the last verdict (M-01, M-03n, M-11…), until the next attempt.
    pub feedback: Vec<String>,
    /// The nominal part of the fiche can be edited.
    pub nominal_editable: bool,
    pub fiche_state: String,
    /// Choices of « Documents utilisés » (filled by the mission controller from the open library).
    pub documents: Vec<DocumentChoice>,
    /// The incident part of the current step E1–E3.
    pub incident: Option<IncidentView>,
}

/// What the interface shows around the incident part being filled (never the truth).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct IncidentView {
    pub rank: usize,
    /// The controlled fields can be changed (the step is open and nothing runs).
    pub editable: bool,
    /// The quantity imposed by the declared observed state and target type, once the state is chosen.
    pub grandeur: Option<String>,
    pub units: Vec<String>,
    pub hint: String,
    /// « le JJ/MM/AAAA à HH:MM » once the measure is recorded.
    pub consigned: Option<String>,
    /// The public lists, with their displayed definitions (same for the four missions).
    pub categories: Vec<Choice>,
    pub states: Vec<Choice>,
    /// The public key-measure rule, one line per state: the quantity for a component target, for the other targets,
    /// and the accepted units.
    pub rules: Vec<RuleView>,
    /// What can be designated on the bench: its parts (designators) and their pins (« désignateur.broche »), filled by
    /// the mission controller from the live circuit.
    pub parts: Vec<String>,
    pub pins: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Choice {
    pub value: String,
    pub definition: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct RuleView {
    pub state: String,
    pub component: String,
    pub other: String,
    pub units: Vec<String>,
}

fn public_lists() -> (Vec<Choice>, Vec<Choice>, Vec<RuleView>) {
    let choice = |v: &str, d: &str| Choice { value: v.to_owned(), definition: d.to_owned() };
    let categories = incident::CATEGORIES.iter().map(|(_, v, d)| choice(v, d)).collect();
    let states = incident::STATES.iter().map(|(_, v, d)| choice(v, d)).collect();
    let rules = incident::STATES
        .iter()
        .map(|(s, v, _)| {
            let (_, component, units) = incident::imposed(*s, Some(incident::TargetKind::Component));
            RuleView { state: (*v).to_owned(), component, other: incident::imposed(*s, None).1, units: units.iter().map(|u| (*u).to_owned()).collect() }
        })
        .collect();
    (categories, states, rules)
}

/// The fiche document on disk.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct FicheDoc {
    format: String,
    version: u32,
    seq: u64,
    nominal: NominalPart,
    #[serde(default)]
    incidents: Vec<IncidentPart>,
}

const FICHE_FORMAT: &str = "n7rt-mission-fiche";

struct State {
    progress: Result<Progress, String>,
    /// The fiche on disk does not verify: shown empty, never saved over, no verification.
    fiche_error: Option<String>,
    nominal: NominalPart,
    incidents: Vec<IncidentPart>,
    seq: u64,
    feedback: Vec<String>,
}

pub struct Session {
    pub workflow: Workflow,
    dir: PathBuf,
    library: Vec<String>,
    utc_offset_s: i64,
    state: Mutex<State>,
    /// One writer of the sealed bench at a time (the view and a verification may both keep it).
    bench_lock: Mutex<()>,
    /// Active time not saved yet: the last poll and the seconds counted since the last save.
    clock: Mutex<(Option<u64>, u64)>,
    /// `session.lock` held for the life of the session ([`hold`]).
    _instance: Option<std::fs::File>,
    /// Random identifier of this opening, recorded in the evidence of each attempt.
    nonce: String,
}

/// Holds `<dir>/session.lock` for the life of a session, so that a second instance of the application never writes
/// the same progression (its saves would bring an older state back). On Windows the file is shared for reading only:
/// a reader (a backup, an antivirus) opens it, another writer does not; elsewhere the file only exists. A few retries
/// cover a session of this instance still being released.
fn hold(dir: &Path) -> Option<std::fs::File> {
    std::fs::create_dir_all(dir).ok()?;
    for _ in 0..20 {
        let mut o = std::fs::OpenOptions::new();
        o.read(true).write(true).create(true).truncate(false);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            const FILE_SHARE_READ: u32 = 1;
            o.share_mode(FILE_SHARE_READ);
        }
        if let Ok(f) = o.open(dir.join("session.lock")) {
            return Some(f);
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    None
}

/// Files a progression writes after its `progress.json`: present without it, the history was deleted.
fn has_history(dir: &Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|entries| {
        entries.flatten().any(|f| {
            let name = f.file_name().to_string_lossy().into_owned();
            ["fiche.json", "bench.sealed", "activation-", "preuve-", "progress.json.prev"].iter().any(|p| name.starts_with(p))
        })
    })
}

/// What a consignation seals: the evidence, and the keyed tag of the wire that carried the defect then (inside the
/// sealed file only, R-10): the verification puts the defect back on that wire of the recorded circuit, wherever a
/// later « Reproduire l'anomalie » or a junction merge moved it since.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SealedPreuve {
    preuve: Preuve,
    carrier: Option<String>,
}

#[derive(Serialize)]
struct SealedPreuveRef<'a> {
    preuve: &'a Preuve,
    carrier: Option<&'a str>,
}

/// The sealed copy of the student's bench and the activation it reflects (rank and time of the anomaly in force
/// when it was kept): a bench kept before an activation does not hold the incident's setting yet.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BenchCopy {
    active: Option<(usize, u64)>,
    circuit: String,
}

/// Directory of the progression of a pack: `<student dir>/progress/<mission>-<pack id>`.
pub fn progress_dir(student_dir: &Path, mission: &str, pack_id: &str) -> PathBuf {
    let safe: String = pack_id.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').take(64).collect();
    student_dir.join("progress").join(format!("{mission}-{safe}"))
}

fn load_progress(dir: &Path, owner: &Owner, now_s: u64, key: &store::Key) -> Result<Progress, String> {
    let path = dir.join("progress.json");
    match store::load_json_keyed(&path, key)? {
        // the progression of a pack starts once: its other files without progress.json are a deleted history
        None if has_history(dir) => Err(UNREADABLE.into()),
        None => {
            let p = Progress::open(owner.clone(), now_s);
            store::save_json_keyed(&path, &serde_json::to_value(&p).map_err(|_| UNREADABLE.to_owned())?, key)?;
            Ok(p)
        }
        Some(v) => {
            let mut p: Progress = serde_json::from_value(v).map_err(|_| UNREADABLE.to_owned())?;
            p.validate().map_err(|_| UNREADABLE.to_owned())?;
            if &p.owner != owner {
                return Err(UNREADABLE.into());
            }
            if p.cancel_interrupted(now_s, "application fermée").is_some() {
                store::save_json_keyed(&path, &serde_json::to_value(&p).map_err(|_| UNREADABLE.to_owned())?, key)?;
            }
            Ok(p)
        }
    }
}

impl Session {
    /// Opens the progression of `workflow` for `owner` under `dir`; `library` lists the identifiers a document of the
    /// fiche may cite. Never fails: an unreadable progression is kept as such.
    pub fn open(workflow: Workflow, dir: PathBuf, owner: Owner, library: Vec<String>, now_s: u64, utc_offset_s: i64) -> Session {
        let key = Some(workflow.key);
        let instance = hold(&dir);
        let progress = if instance.is_some() { load_progress(&dir, &owner, now_s, &key) } else { Err(IN_USE.to_owned()) };
        let (nominal, incidents, seq, fiche_error) = match store::load_json_keyed(&dir.join("fiche.json"), &key) {
            Ok(None) => (NominalPart::default(), Vec::new(), 0, None),
            Ok(Some(v)) => match serde_json::from_value::<FicheDoc>(v) {
                Ok(f) if f.format == FICHE_FORMAT && f.version == 1 && f.incidents.iter().all(|i| (1..=3).contains(&i.rang)) => {
                    (f.nominal, f.incidents, f.seq, None)
                }
                _ => (NominalPart::default(), Vec::new(), 0, Some(FICHE_UNREADABLE.to_owned())),
            },
            Err(_) => (NominalPart::default(), Vec::new(), 0, Some(FICHE_UNREADABLE.to_owned())),
        };
        let state = State { progress, fiche_error, nominal, incidents, seq, feedback: Vec::new() };
        Session {
            workflow,
            dir,
            library,
            utc_offset_s,
            state: Mutex::new(state),
            bench_lock: Mutex::new(()),
            clock: Mutex::new((None, 0)),
            _instance: instance,
            nonce: {
                let mut b = [0u8; 16];
                if getrandom::fill(&mut b).is_err() {
                    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
                    b.copy_from_slice(&t.to_le_bytes());
                }
                b.iter().map(|x| format!("{x:02x}")).collect()
            },
        }
    }

    pub fn nominal(&self) -> NominalPart {
        self.state.lock().unwrap().nominal.clone()
    }

    pub fn sheet_seq(&self) -> u64 {
        self.state.lock().unwrap().seq
    }

    /// Records the nominal part (shape checks only; never compared here). Refused once E0 is validated.
    pub fn set_nominal(&self, part: NominalPart) -> Result<(), String> {
        shape(&part)?;
        let mut st = self.state.lock().unwrap();
        if let Some(e) = &st.fiche_error {
            return Err(e.clone());
        }
        let passed = {
            let p = st.progress.as_ref().map_err(|e| e.clone())?;
            if p.running.is_some() {
                return Err(FICHE_LOCKED.into());
            }
            if p.closed() {
                return Err(CLOSED_FICHE.into());
            }
            p.steps[0].state == StepState::Passed
        };
        if passed {
            // free texts (justification, pages, signal names) stay editable; every controlled field is frozen
            let mut frozen = st.nominal.clone();
            frozen.justification = part.justification.clone();
            frozen.documents_utilises.iter_mut().zip(&part.documents_utilises).for_each(|(a, b)| a.pages = b.pages.clone());
            frozen.signaux.iter_mut().zip(&part.signaux).for_each(|(a, b)| a.nom = b.nom.clone());
            if frozen != part {
                return Err(FROZEN.into());
            }
        }
        self.write_fiche(&mut st, Some(part), None)
    }

    /// Writes the fiche with a new nominal part and/or incident part (atomic, one sequence number more).
    fn write_fiche(&self, st: &mut State, nominal: Option<NominalPart>, incident: Option<IncidentPart>) -> Result<(), String> {
        let seq = st.seq + 1;
        let mut incidents = st.incidents.clone();
        if let Some(part) = incident {
            match incidents.iter_mut().find(|x| x.rang == part.rang) {
                Some(x) => *x = part,
                None => {
                    incidents.push(part);
                    incidents.sort_by_key(|x| x.rang);
                }
            }
        }
        let nominal = nominal.unwrap_or_else(|| st.nominal.clone());
        let doc = FicheDoc { format: FICHE_FORMAT.into(), version: 1, seq, nominal: nominal.clone(), incidents: incidents.clone() };
        store::save_json_keyed(&self.dir.join("fiche.json"), &serde_json::to_value(&doc).map_err(|_| "fiche invalide".to_owned())?, &Some(self.workflow.key))?;
        st.nominal = nominal;
        st.incidents = incidents;
        st.seq = seq;
        Ok(())
    }

    /// The incident part `rank` (1–3) once its incident is activated (the step is open or done).
    pub fn incident(&self, rank: usize) -> Option<IncidentPart> {
        let st = self.state.lock().unwrap();
        let p = st.progress.as_ref().ok()?;
        if !(1..=3).contains(&rank) || p.steps[rank].state == StepState::Todo {
            return None;
        }
        Some(st.incidents.iter().find(|x| x.rang as usize == rank).cloned().unwrap_or(IncidentPart { rang: rank as u8, ..Default::default() }))
    }

    /// Records an incident part (shape checks only; never compared here). The recorded measure is the application's:
    /// what the interface sends there is ignored. Once its step is validated only the justification changes.
    pub fn set_incident(&self, part: IncidentPart) -> Result<(), String> {
        incident::shape(&part)?;
        let mut st = self.state.lock().unwrap();
        if let Some(e) = &st.fiche_error {
            return Err(e.clone());
        }
        let p = st.progress.as_ref().map_err(|e| e.clone())?;
        let rank = part.rang as usize;
        if !(1..=3).contains(&rank) || p.steps[rank].state == StepState::Todo {
            return Err(INCIDENT_CLOSED.into());
        }
        if p.running.is_some() {
            return Err(FICHE_LOCKED.into());
        }
        if p.closed() {
            return Err(CLOSED_FICHE.into());
        }
        let stored = st.incidents.iter().find(|x| x.rang as usize == rank).cloned().unwrap_or(IncidentPart { rang: part.rang, ..Default::default() });
        let mut next = part;
        next.mesure_cle.consignation = stored.mesure_cle.consignation.clone();
        if p.steps[rank].state == StepState::Passed {
            let mut frozen = stored.clone();
            frozen.justification = next.justification.clone();
            if frozen != next {
                return Err(INCIDENT_FROZEN.into());
            }
        }
        self.write_fiche(&mut st, None, Some(next))
    }

    /// The evidence of a measure is named by its digest: a new recording never overwrites the one the fiche binds
    /// until the fiche itself is written.
    fn preuve_path(&self, rank: usize, sha: &str) -> PathBuf {
        self.dir.join(format!("preuve-{rank}-{}.sealed", &sha[..sha.len().min(16)]))
    }

    /// « Consigner la mesure » of the open incident: the conditions go in the fiche, the evidence is sealed beside it.
    pub fn consign(&self, rank: usize, grandeur: String, conditions: Conditions, preuve: &Preuve, now_s: u64) -> Result<(), String> {
        let mut st = self.state.lock().unwrap();
        if let Some(e) = &st.fiche_error {
            return Err(e.clone());
        }
        let p = st.progress.as_ref().map_err(|e| e.clone())?;
        if p.current() != Some(rank) || !(1..=3).contains(&rank) || p.running.is_some() {
            return Err(CONSIGN_REFUSED.into());
        }
        let carrier = p.active.as_ref().and_then(|a| a.carrier.clone().filter(|c| *c != self.filler(a)));
        let text = serde_json::to_vec(&SealedPreuveRef { preuve, carrier: carrier.as_deref() }).map_err(|_| CONSIGN_REFUSED.to_owned())?;
        let sha: String = Sha256::digest(&text).iter().map(|b| format!("{b:02x}")).collect();
        let sealed = n7rt_seal::seal_local(&self.workflow.key, format!("n7rt-preuve-{rank}").as_bytes(), &text).map_err(|_| CONSIGN_REFUSED.to_owned())?;
        let path = self.preuve_path(rank, &sha);
        store::write_atomic(&path, &sealed)?;
        let mut part = st.incidents.iter().find(|x| x.rang as usize == rank).cloned().unwrap_or(IncidentPart { rang: rank as u8, ..Default::default() });
        part.mesure_cle.consignation = Some(Consigned { consignee_s: now_s, grandeur, conditions, preuve_sha256: sha });
        if let Err(e) = self.write_fiche(&mut st, None, Some(part)) {
            // the fiche still binds the previous evidence, which is intact
            let _ = std::fs::remove_file(&path);
            return Err(e);
        }
        // the previous evidence of this incident is no longer bound by the fiche
        let keep = path.file_name().map(|n| n.to_os_string());
        if let Ok(entries) = std::fs::read_dir(&self.dir) {
            for f in entries.flatten() {
                let name = f.file_name();
                let old = name.to_string_lossy().starts_with(&format!("preuve-{rank}-")) && Some(&name) != keep.as_ref();
                if old {
                    let _ = std::fs::remove_file(f.path());
                }
            }
        }
        Ok(())
    }

    /// The sealed evidence of the measure recorded for incident `rank`, when it matches the fiche.
    pub fn preuve(&self, rank: usize) -> Option<Preuve> {
        self.sealed_preuve(rank).map(|s| s.preuve)
    }

    /// The keyed tag of the wire that carried the defect when the measure of incident `rank` was recorded.
    pub fn consigned_carrier(&self, rank: usize) -> Option<String> {
        self.sealed_preuve(rank).and_then(|s| s.carrier)
    }

    fn sealed_preuve(&self, rank: usize) -> Option<SealedPreuve> {
        let sha = {
            let st = self.state.lock().unwrap();
            st.incidents.iter().find(|x| x.rang as usize == rank)?.mesure_cle.consignation.as_ref()?.preuve_sha256.clone()
        };
        let sealed = std::fs::read(self.preuve_path(rank, &sha)).ok()?;
        let text = n7rt_seal::open_local(&self.workflow.key, format!("n7rt-preuve-{rank}").as_bytes(), &sealed).ok()?;
        let digest: String = Sha256::digest(&text).iter().map(|b| format!("{b:02x}")).collect();
        (digest == sha).then(|| serde_json::from_slice(&text).ok()).flatten()
    }

    /// Keeps the student's bench (canonical text, hidden parameters of the mission parts included) sealed on disk,
    /// with the activation it reflects (the anomaly in force now).
    pub fn save_bench(&self, text: &str) -> Result<(), String> {
        let _one = self.bench_lock.lock().unwrap();
        let active = self.active().map(|a| (a.rank, a.since_s));
        let copy = serde_json::to_vec(&BenchCopy { active, circuit: text.to_owned() }).map_err(|_| NOT_SAVED.to_owned())?;
        let sealed = n7rt_seal::seal_local(&self.workflow.key, b"n7rt-bench", &copy).map_err(|_| NOT_SAVED.to_owned())?;
        store::write_atomic(&self.dir.join(BENCH_FILE), &sealed)
    }

    /// The student's bench kept by [`Session::save_bench`], if any and readable, and whether it was kept under the
    /// activation now in force (else the anomaly's setting is not in it yet).
    pub fn load_bench(&self) -> Option<(String, bool)> {
        let sealed = std::fs::read(self.dir.join(BENCH_FILE)).ok()?;
        let copy: BenchCopy = serde_json::from_slice(&n7rt_seal::open_local(&self.workflow.key, b"n7rt-bench", &sealed).ok()?).ok()?;
        let current = copy.active == self.active().map(|a| (a.rank, a.since_s));
        Some((copy.circuit, current))
    }

    /// Keyed tag of a wire object (the carrier of a wire defect is persisted as this tag, never as its identifier).
    pub fn tag(&self, wire: u32) -> String {
        let mut h = Sha256::new();
        h.update(self.workflow.key);
        h.update(b"n7rt-carrier");
        h.update(wire.to_le_bytes());
        h.finalize().iter().map(|b| format!("{b:02x}")).collect()
    }

    /// E5: writes the final fiche of the closed mission under `root` (canonical JSON, readable page, sealed
    /// evidence and a sealed copy of the canonical fiche) and records the export. Returns the page; the verdict lines
    /// say whether the export succeeded.
    pub fn export(&self, root: &Path, now_s: u64) -> Result<PathBuf, String> {
        let done = self.export_inner(root, now_s);
        let mut st = self.state.lock().unwrap();
        let line = if done.is_ok() { EXPORTED } else { NOT_EXPORTED };
        if !st.feedback.iter().any(|l| l == line) {
            st.feedback.retain(|l| l != EXPORTED && l != NOT_EXPORTED);
            st.feedback.push(line.into());
        }
        done
    }

    fn export_inner(&self, root: &Path, now_s: u64) -> Result<PathBuf, String> {
        let mut st = self.state.lock().unwrap();
        let p = st.progress.as_ref().map_err(|e| e.clone())?.clone();
        if !p.closed() {
            return Err("La mission n'est pas clôturée.".into());
        }
        let offset = self.utc_offset_s;
        let dates = move |t: u64| {
            let (d, h) = civil(t, offset);
            format!("{d} {h}")
        };
        let (d, h) = civil(now_s, offset);
        let stamp = format!("{}{}{}-{}", &d[6..10], &d[3..5], &d[0..2], h.replace(':', ""));
        let folder = crate::workflow::export::folder(root, &p.owner.mission, &p.owner.group, &stamp);
        let doc = crate::workflow::export::document(&p, &st.nominal, &st.incidents, &STEP_NAMES, &dates);
        let fail = |_| "La fiche finale ne peut pas être écrite dans le dossier Documents.".to_owned();
        std::fs::create_dir_all(folder.join("scellés")).map_err(fail)?;
        let json = serde_json::to_vec_pretty(&doc).map_err(|_| "fiche finale invalide".to_owned())?;
        store::write_atomic(&folder.join("fiche.json"), &json)?;
        let page = folder.join("fiche.html");
        let sha: String = Sha256::digest(&json).iter().map(|b| format!("{b:02x}")).collect();
        store::write_atomic(&page, crate::workflow::export::html(&doc, &sha).as_bytes())?;
        // a copy sealed with the progression's key: a fiche.json edited afterwards no longer matches it
        let sealed = n7rt_seal::seal_local(&self.workflow.key, b"n7rt-fiche-finale", &json).map_err(|_| "fiche finale invalide".to_owned())?;
        store::write_atomic(&folder.join("scellés").join("fiche-finale.sealed"), &sealed)?;
        for f in std::fs::read_dir(&self.dir).map_err(fail)?.flatten() {
            let name = f.file_name();
            if name.to_string_lossy().ends_with(".sealed") {
                std::fs::copy(f.path(), folder.join("scellés").join(&name)).map_err(fail)?;
            }
        }
        let mut next = p;
        next.exported = Some(Exported { at_s: now_s, folder: folder.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default() });
        self.save(&next)?;
        st.progress = Ok(next);
        Ok(page)
    }

    /// The page of the last export of the final fiche under `root`, if it is still there.
    pub fn exported_page(&self, root: &Path) -> Option<PathBuf> {
        let st = self.state.lock().unwrap();
        let e = st.progress.as_ref().ok()?.exported.clone()?;
        Some(root.join(e.folder).join("fiche.html")).filter(|p| p.is_file())
    }

    /// The anomaly in force.
    /// The anomaly in force; its `carrier` is `None` while no wire carries a wire defect, and for any other anomaly.
    pub fn active(&self) -> Option<Active> {
        let mut a = self.state.lock().unwrap().progress.as_ref().ok()?.active.clone()?;
        if a.carrier.as_deref() == Some(self.filler(&a).as_str()) {
            a.carrier = None;
        }
        Some(a)
    }

    /// What progress.json holds instead of a carrier tag: a keyed value of the same shape, derived from the
    /// activation, so that the file never tells a wire defect from another anomaly (R-10).
    fn filler(&self, a: &Active) -> String {
        let mut h = Sha256::new();
        h.update(self.workflow.key);
        h.update(b"n7rt-carrier-none");
        h.update((a.rank as u64).to_le_bytes());
        h.update(a.since_s.to_le_bytes());
        h.finalize().iter().map(|b| format!("{b:02x}")).collect()
    }

    /// Records the carrier of the wire defect in force, `None` for none (saved before it counts).
    pub fn set_carrier(&self, tag: Option<String>) -> Result<(), String> {
        let mut st = self.state.lock().unwrap();
        let mut next = st.progress.as_ref().map_err(|e| e.clone())?.clone();
        let Some(a) = next.active.as_mut() else { return Ok(()) };
        let stored = tag.unwrap_or_else(|| self.filler(a));
        if a.carrier.as_deref() == Some(stored.as_str()) {
            return Ok(());
        }
        a.carrier = Some(stored);
        self.save(&next)?;
        st.progress = Ok(next);
        Ok(())
    }

    /// T3: the anomaly of incident `rank` came on `circuit` (the bench with it). The circuit is kept sealed and the
    /// repair record opened with the CRC-32 of the attempt that validated the previous step (the code on the board).
    pub fn record_activation(&self, rank: usize, circuit: &str) -> Result<(), String> {
        let sealed = n7rt_seal::seal_local(&self.workflow.key, format!("n7rt-activation-{rank}").as_bytes(), circuit.as_bytes()).map_err(|_| NOT_SAVED.to_owned())?;
        store::write_atomic(&self.dir.join(format!("activation-{rank}.sealed")), &sealed)?;
        let mut st = self.state.lock().unwrap();
        let mut next = st.progress.as_ref().map_err(|e| e.clone())?.clone();
        let crc = next.attempts.iter().rev().find(|a| a.step + 1 == rank && a.verdict == AttemptVerdict::Pass).map(|a| a.evidence.crc32.clone()).unwrap_or_default();
        next.repairs.retain(|r| r.rank != rank);
        next.repairs.push(crate::workflow::progress::RepairRecord { rank, crc32_activation: crc, crc32_validation: None, circuit: Vec::new() });
        self.save(&next)?;
        st.progress = Ok(next);
        Ok(())
    }

    /// T3: incident `rank` validated on `circuit` (the verified bench): the CRC-32 of the validating attempt and the
    /// changes since the activation complete its repair record.
    pub fn record_validation(&self, rank: usize, circuit: &n7rt_circuit::Circuit) -> Result<(), String> {
        let before = std::fs::read(self.dir.join(format!("activation-{rank}.sealed")))
            .ok()
            .and_then(|s| n7rt_seal::open_local(&self.workflow.key, format!("n7rt-activation-{rank}").as_bytes(), &s).ok())
            .and_then(|b| String::from_utf8(b).ok())
            .and_then(|t| n7rt_circuit::persist::load(&t).ok());
        let changes = before.map(|b| crate::workflow::pcheck::changes(&b, circuit)).unwrap_or_default();
        let mut st = self.state.lock().unwrap();
        let mut next = st.progress.as_ref().map_err(|e| e.clone())?.clone();
        let crc = next.attempts.iter().rev().find(|a| a.step == rank && a.verdict == AttemptVerdict::Pass).map(|a| a.evidence.crc32.clone());
        match next.repairs.iter_mut().find(|r| r.rank == rank) {
            Some(r) => {
                r.crc32_validation = crc;
                r.circuit = changes;
            }
            None => next.repairs.push(crate::workflow::progress::RepairRecord { rank, crc32_activation: String::new(), crc32_validation: crc, circuit: changes }),
        }
        self.save(&next)?;
        st.progress = Ok(next);
        Ok(())
    }

    /// Completes the repair records that a killed application left open: the CRC-32 of a validated incident, from
    /// its validating attempt, and the record of the incident in force, opened on `circuit` (the bench at this
    /// opening). The circuit changes of an incident validated meanwhile are not known any more and stay empty.
    pub fn complete_records(&self, circuit: &str) {
        let (activation, validations) = {
            let st = self.state.lock().unwrap();
            let Ok(p) = st.progress.as_ref() else { return };
            let activation = p.active.as_ref().map(|a| a.rank).filter(|k| !p.repairs.iter().any(|r| r.rank == *k));
            let validations: Vec<usize> = (1..=3)
                .filter(|k| p.steps[*k].state == StepState::Passed && !p.repairs.iter().any(|r| r.rank == *k && r.crc32_validation.is_some()))
                .collect();
            (activation, validations)
        };
        if let Some(k) = activation {
            let _ = self.record_activation(k, circuit);
        }
        if validations.is_empty() {
            return;
        }
        let mut st = self.state.lock().unwrap();
        let Ok(p) = st.progress.as_ref() else { return };
        let mut next = p.clone();
        for k in validations {
            let crc = next.attempts.iter().rev().find(|a| a.step == k && a.verdict == AttemptVerdict::Pass).map(|a| a.evidence.crc32.clone());
            match next.repairs.iter_mut().find(|r| r.rank == k) {
                Some(r) => r.crc32_validation = crc,
                None => next.repairs.push(crate::workflow::progress::RepairRecord { rank: k, crc32_activation: String::new(), crc32_validation: crc, circuit: Vec::new() }),
            }
        }
        if self.save(&next).is_ok() {
            st.progress = Ok(next);
        }
    }

    /// The mission is open and polled at `now_s`: the active time grows by the time since the previous poll (at most
    /// 30 s: a longer gap is the application closed or asleep), saved once a minute and by [`Session::flush_active`].
    pub fn touch(&self, now_s: u64) {
        let due = {
            let mut c = self.clock.lock().unwrap();
            if let Some(last) = c.0.filter(|l| *l <= now_s && now_s - *l <= 30) {
                c.1 += now_s - last;
            }
            c.0 = Some(now_s);
            c.1 >= 60
        };
        if due {
            self.flush_active();
        }
    }

    /// Saves the active time counted since the last save.
    pub fn flush_active(&self) {
        let pending = std::mem::take(&mut self.clock.lock().unwrap().1);
        if pending == 0 {
            return;
        }
        let mut st = self.state.lock().unwrap();
        let Ok(p) = st.progress.as_ref() else { return };
        let mut next = p.clone();
        next.active_s += pending;
        if self.save(&next).is_ok() {
            st.progress = Ok(next);
        } else {
            self.clock.lock().unwrap().1 += pending;
        }
    }

    /// R-11: records « Reproduire l'anomalie » for the incident in force (E1–E3, not validated, no attempt
    /// running) and returns its rank. The caller has set the anomaly again on the bench; `carrier` is the tag of the
    /// wire that carries its wire defect now (`None`: none, or another anomaly). One save for every kind of anomaly:
    /// the files change alike whatever the anomaly (R-10).
    pub fn reproduce(&self, now_s: u64, carrier: Option<String>) -> Result<usize, String> {
        let mut st = self.state.lock().unwrap();
        let p = st.progress.as_ref().map_err(|e| e.clone())?;
        let rank = p.active.as_ref().filter(|_| p.running.is_none()).map(|a| a.rank).ok_or_else(|| REPRODUCE_REFUSED.to_owned())?;
        let mut next = p.clone();
        if next.reproductions.len() >= crate::workflow::progress::MAX_REPRODUCTIONS {
            next.reproductions.remove(0);
        }
        next.reproductions.push(crate::workflow::progress::Reproduction { rank, at_s: now_s });
        if let Some(a) = next.active.as_mut() {
            a.carrier = Some(carrier.unwrap_or_else(|| self.filler(a)));
        }
        self.save(&next)?;
        st.progress = Ok(next);
        Ok(rank)
    }

    fn save(&self, p: &Progress) -> Result<(), String> {
        store::save_json_keyed(&self.dir.join("progress.json"), &serde_json::to_value(p).map_err(|_| UNREADABLE.to_owned())?, &Some(self.workflow.key))
    }

    /// Why a verification cannot start now, or the step and the hidden test to use.
    fn precondition(&self, st: &State, now_s: u64) -> Result<(usize, &HiddenTest), String> {
        let p = st.progress.as_ref().map_err(|e| e.clone())?;
        if let Some(e) = &st.fiche_error {
            return Err(e.clone());
        }
        let step = p.current().ok_or_else(|| ORDER_CLOSED.to_owned())?;
        let content = self.workflow.steps.get(step).ok_or_else(|| UNAVAILABLE.to_owned())?;
        let missing = match step {
            4 => Vec::new(),
            0 => fiche::missing(&st.nominal),
            1..=3 if content.incident.is_some() => match st.incidents.iter().find(|x| x.rang as usize == step) {
                Some(part) => incident::missing(part),
                None => incident::missing(&IncidentPart { rang: step as u8, ..Default::default() }),
            },
            _ => return Err(UNAVAILABLE.into()),
        };
        if !missing.is_empty() {
            return Err(format!("Complétez la fiche de cette étape avant la vérification. Champs à renseigner : {}.", missing.join(", ")));
        }
        match p.check(step, now_s) {
            Ok(()) => {}
            Err(Refusal::Interval { remaining_s }) => {
                return Err(format!("Prochaine vérification possible dans {} min {} s.", remaining_s / 60, remaining_s % 60))
            }
            Err(Refusal::Running) => return Err("Vérification déjà en cours.".into()),
            Err(Refusal::Closed) => return Err(ORDER_CLOSED.into()),
            Err(Refusal::NotOpen | Refusal::Replayed) => return Err(UNAVAILABLE.into()),
        }
        let pool = content.tests.len().max(1);
        let index = p.cursor[step] % pool;
        Ok((step, &content.tests[index]))
    }

    /// T2: an attempt starts. Returns the step, a copy of its hidden test and the attempt identifier.
    pub fn begin(&self, now_s: u64, id: String, circuit_sha256: String, crc32: String, board_uid: String) -> Result<(usize, HiddenTestRef), String> {
        let mut st = self.state.lock().unwrap();
        let (step, test) = self.precondition(&st, now_s)?;
        let test = HiddenTestRef { vector: test.vector.clone(), expected: test.expected.clone() };
        let pool = self.workflow.steps[step].tests.len();
        let seq = st.seq;
        let mut next = st.progress.as_ref().map_err(|e| e.clone())?.clone();
        let evidence = Evidence { sheet_seq: seq, circuit_sha256, crc32, vector: 0, session: self.nonce.clone(), board_uid, bus_sha256: None };
        next.begin(step, now_s, id, pool, evidence).map_err(|_| "Vérification impossible.".to_owned())?;
        // saved before it counts: a failed save leaves the progression as it was
        self.save(&next)?;
        st.progress = Ok(next);
        st.feedback.clear();
        Ok((step, test))
    }

    /// The fiche part of `step` conforms to the sealed truth: E0 the nominal part; E1–E3 the target, the category, the
    /// observed state and the key measure (`measure`: its verdict, computed by the caller on the recorded evidence).
    pub fn diagnostic(&self, step: usize, measure: bool) -> bool {
        let st = self.state.lock().unwrap();
        let content = self.workflow.steps.get(step);
        match (step, content.and_then(|s| s.nominal.as_ref()), content.and_then(|s| s.incident.as_ref())) {
            (0, Some(truth), _) => fiche::conforms(&st.nominal, truth, &self.library),
            (1..=3, _, Some(inc)) => {
                measure && st.incidents.iter().find(|x| x.rang as usize == step).is_some_and(|part| incident::fields_conform(part, &inc.truth))
            }
            _ => false,
        }
    }

    /// The incident part the fiche holds for `step` (the caller computes its key measure).
    pub fn incident_part(&self, step: usize) -> Option<IncidentPart> {
        self.state.lock().unwrap().incidents.iter().find(|x| x.rang as usize == step).cloned()
    }

    /// What crossed the link during the running attempt (SHA-256), kept in its evidence when it ends.
    pub fn set_bus(&self, sha256: String) {
        if let Ok(p) = self.state.lock().unwrap().progress.as_mut() {
            if let Some(r) = p.running.as_mut() {
                r.evidence.bus_sha256 = Some(sha256);
            }
        }
    }

    /// The identifier of this opening of the mission.
    pub fn nonce(&self) -> &str {
        &self.nonce
    }

    /// T3/T4/T5: the running attempt ends; the verdict is recorded and worded.
    pub fn finish(&self, outcome: Outcome, now_s: u64) -> Result<Transition, String> {
        let mut st = self.state.lock().unwrap();
        let mut next = st.progress.as_ref().map_err(|e| e.clone())?.clone();
        let t = next.finish(outcome.clone(), now_s)?;
        if let Some(a) = next.active.as_mut().filter(|a| a.carrier.is_none()) {
            a.carrier = Some(self.filler(a));
        }
        // saved before it counts (one retry). If the disk refuses, nothing is committed: the attempt ends as the next
        // opening will see it (interrupted, not counted), and no anomaly moves
        let saved = self.save(&next).or_else(|_| {
            std::thread::sleep(std::time::Duration::from_millis(200));
            self.save(&next)
        });
        if saved.is_err() {
            // the attempt ends cancelled in memory as well: a later save records it as cancelled, it never vanishes
            if let Ok(p) = st.progress.as_mut() {
                if p.finish(Outcome::Cancelled { reason: "progression non enregistrée".into() }, now_s).is_err() {
                    p.running = None;
                }
            }
            st.feedback = vec![
                "Vérification interrompue (progression non enregistrée). Elle n'est pas comptée.".into(),
                NOT_SAVED.into(),
            ];
            return Ok(Transition { step: t.step, verdict: AttemptVerdict::Cancelled, opened: None, closed: false });
        }
        st.progress = Ok(next);
        let (date, hour) = civil(now_s, self.utc_offset_s);
        st.feedback = match (&outcome, t.step) {
            (Outcome::Cancelled { reason }, _) => {
                vec![format!("Vérification interrompue ({reason}). Elle n'est pas comptée. Relancez quand le banc est prêt.")]
            }
            (Outcome::Judged { competences: Some(cps), .. }, 4) => {
                let mut lines = Vec::new();
                lines.extend(cps.iter().enumerate().map(|(k, ok)| {
                    format!("CP-{} — {} : {}", k + 1, crate::workflow::qualification::LABELS.get(k).copied().unwrap_or(""), if *ok { "PASS" } else { "FAIL" })
                }));
                if t.closed {
                    lines.push("Mission clôturée.".into());
                }
                lines
            }
            (Outcome::Judged { pass: true, .. }, _) => {
                let mut lines = vec![format!("Vérification réussie. Étape validée le {date} à {hour}.")];
                // M-02 only for a step this pack really holds: no anomaly is announced that does not exist
                let held = |k: usize| self.workflow.steps.get(k).is_some();
                match t.opened {
                    Some(k @ 1..=3) if held(k) => lines.push(format!("Incident {k} ouvert. Le véhicule revient d'essai avec une anomalie.")),
                    Some(4) if held(4) => lines.push("Qualification finale ouverte.".into()),
                    _ if t.closed => lines.push("Mission clôturée.".into()),
                    _ => {}
                }
                lines
            }
            (Outcome::Judged { pass: false, diagnostic, verified, .. }, 0) => vec![
                "Échec à corriger.".into(),
                format!("Fiche nominale : {}", if *diagnostic == Some(true) { "conforme" } else { "non conforme" }),
                format!("Essai nominal : {}", if *verified == Some(true) { "réussi" } else { "non réussi" }),
                DETAIL_HIDDEN.into(),
            ],
            (Outcome::Judged { pass: false, diagnostic, verified, .. }, _) => vec![
                "Échec à corriger.".into(),
                format!("Diagnostic (fiche) : {}", if *diagnostic == Some(true) { "conforme" } else { "non conforme" }),
                format!("Réparation (essai caché) : {}", if *verified == Some(true) { "vérifiée" } else { "non vérifiée" }),
                DETAIL_HIDDEN.into(),
            ],
        };
        Ok(t)
    }

    /// The view of the progression. `running` = (elapsed s, total s) of the attempt in progress; `board_ready` = the
    /// board is connected, its runtime verified and the student code loaded (precondition M-06).
    pub fn view(&self, now_s: u64, running: Option<(f64, f64)>, board_ready: bool) -> ProgressionView {
        let st = self.state.lock().unwrap();
        let p = match &st.progress {
            Ok(p) => p,
            Err(e) => {
                return ProgressionView {
                    readable: false,
                    steps: STEP_NAMES[..VERIFIED_STEPS].iter().map(|n| StepView { name: (*n).into(), state: "—".into(), since: None, current: false }).collect(),
                    current: None,
                    order: String::new(),
                    button: button(0).into(),
                    ready: false,
                    running: false,
                    blocked: Some(e.clone()),
                    feedback: Vec::new(),
                    nominal_editable: false,
                    fiche_state: "En cours de rédaction".into(),
                    documents: Vec::new(),
                    incident: None,
                }
            }
        };
        let current = p.current();
        // five step boxes; the sixth box of the screen is the fiche (`fiche_state`), the closure stays internal
        let steps = p
            .steps
            .iter()
            .take(VERIFIED_STEPS)
            .enumerate()
            .map(|(k, s)| StepView {
                name: STEP_NAMES[k].into(),
                state: state_text(s.state).into(),
                since: (s.state == StepState::Passed).then(|| {
                    let (d, h) = civil(s.since_s, self.utc_offset_s);
                    format!("le {d} à {h}")
                }),
                current: current == Some(k),
            })
            .collect();
        // M-10 counts the seconds left, as the covers of the bus and the slots do; then preconditions 3, 4, 5
        let blocked = match running {
            Some((elapsed, total)) => Some(format!(
                "{} en cours — {} s. Ne débranchez pas la carte et ne fermez pas l'application.",
                if current == Some(crate::workflow::progress::QUALIFICATION) { "Qualification" } else { "Essai" },
                (total - elapsed - 1e-9).ceil().max(0.0) as u64
            )),
            None => match self.precondition(&st, now_s) {
                Err(e) => Some(e),
                Ok(_) if !board_ready => Some(BOARD.into()),
                Ok(_) => None,
            },
        };
        let step = current.unwrap_or(CLOSURE);
        let held = step == CLOSURE || self.workflow.steps.get(step).is_some();
        let incident = (1..=3).contains(&step).then(|| {
            let part = st.incidents.iter().find(|x| x.rang as usize == step).cloned().unwrap_or_default();
            let imposed = part.etat_observe.map(|e| incident::imposed(e, part.cible.kind));
            let (categories, states, rules) = public_lists();
            IncidentView {
                rank: step,
                editable: running.is_none() && p.running.is_none(),
                grandeur: imposed.as_ref().map(|i| i.1.clone()),
                units: imposed.map(|i| i.2.iter().map(|u| (*u).to_owned()).collect()).unwrap_or_default(),
                hint: incident::MEASURE_HINT.into(),
                consigned: part.mesure_cle.consignation.as_ref().map(|c| {
                    let (d, h) = civil(c.consignee_s, self.utc_offset_s);
                    format!("le {d} à {h}")
                }),
                categories,
                states,
                rules,
                parts: Vec::new(),
                pins: Vec::new(),
            }
        });
        ProgressionView {
            readable: true,
            steps,
            current,
            order: if held { order(step) } else { UNAVAILABLE.into() },
            button: button(step).into(),
            ready: running.is_none() && blocked.is_none(),
            running: running.is_some(),
            blocked: if p.closed() { None } else { blocked },
            feedback: st.feedback.clone(),
            nominal_editable: p.steps[0].state != StepState::Passed,
            fiche_state: match (&p.exported, p.closed()) {
                (Some(e), _) => {
                    let (d, h) = civil(e.at_s, self.utc_offset_s);
                    format!("Exportée le {d} à {h}")
                }
                (None, true) => "Clôturée, exportation à reprendre".into(),
                (None, false) => "En cours de rédaction".into(),
            },
            documents: Vec::new(),
            incident,
        }
    }

    /// Attempts of the current step that were judged (never shown as a score).
    pub fn judged_attempts(&self) -> usize {
        let st = self.state.lock().unwrap();
        st.progress.as_ref().map(|p| p.attempts.iter().filter(|a| a.verdict != AttemptVerdict::Cancelled).count()).unwrap_or(0)
    }
}

/// A copy of one hidden test for the check thread.
pub struct HiddenTestRef {
    pub vector: crate::workflow::vector::HiddenVector,
    pub expected: crate::workflow::compare::Expected,
}

/// Removes the progressions of the student directory (instructor reset).
pub fn reset_all(student_dir: &Path) -> Result<(), String> {
    let dir = student_dir.join("progress");
    match std::fs::remove_dir_all(&dir) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err("Réinitialisation des progressions impossible.".into()),
    }
}

/// Shape checks of the form (no answer involved): identifiers on 11 bits, classic DLC, positions 0–63 and lengths
/// 1–64, finite numbers, bounded texts and lists.
fn shape(part: &NominalPart) -> Result<(), String> {
    let bad = |what: &str| Err(format!("Fiche : {what}."));
    if part.justification.chars().count() > MAX_TEXT {
        return bad("justification de plus de 2 000 caractères");
    }
    if part.trames.len() > MAX_ROWS || part.signaux.len() > MAX_ROWS || part.statuts.len() > MAX_ROWS || part.documents_utilises.len() > MAX_ROWS {
        return bad("trop de lignes");
    }
    let id_ok = |id: Option<u32>| id.map_or(true, |x| x <= 0x7FF);
    let bits_ok = |lsb: Option<u16>, len: Option<u16>| lsb.map_or(true, |x| x <= 63) && len.map_or(true, |x| (1..=64).contains(&x));
    for t in &part.trames {
        let fields = [&t.protection.crc, &t.protection.compteur, &t.compteur_de_vie];
        if !id_ok(t.identifiant) || t.dlc.is_some_and(|d| d > 8) || !fields.iter().all(|f| bits_ok(f.lsb_bit, f.longueur)) {
            return bad("trame hors des limites du formulaire");
        }
    }
    for s in &part.signaux {
        let finite = [s.facteur, s.offset].iter().all(|x| x.map_or(true, f64::is_finite));
        if !id_ok(s.identifiant) || !bits_ok(s.lsb_bit, s.longueur) || !finite || s.slots.len() > 32 || s.nom.chars().count() > 200 {
            return bad("signal hors des limites du formulaire");
        }
    }
    for s in &part.statuts {
        if !id_ok(s.identifiant) || !bits_ok(s.lsb_bit, s.longueur) || s.slots.len() > 32 {
            return bad("statut hors des limites du formulaire");
        }
    }
    if part.documents_utilises.iter().any(|d| d.pages.chars().count() > 200 || d.document.as_ref().is_some_and(|x| x.len() > 512)) {
        return bad("document hors des limites du formulaire");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflow::sealed::parse_workflow;

    fn owner() -> Owner {
        Owner { mission: "apps".into(), cohort: "c".into(), group: "G1".into(), pack_id: "p1".into(), workflow_sha256: "w".into() }
    }

    fn workflow() -> Workflow {
        let v = serde_json::json!({
            "format": "n7rt-workflow", "version": 1, "mission": "apps",
            "steps": [{"step": 0, "nominal": crate::workflow::sealed::tests::sample_truth(),
                       "tests": [crate::workflow::sealed::tests::sample_test(100_000, 10_000)]}]
        });
        parse_workflow(&v, "apps", 10).unwrap()
    }

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("n7rt-session-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn civil_time_matches_known_dates() {
        assert_eq!(civil(0, 0), ("01/01/1970".to_owned(), "00:00".to_owned()));
        assert_eq!(civil(1_791_873_000, 7_200), ("13/10/2026".to_owned(), "08:30".to_owned()));
    }

    #[test]
    fn a_new_progression_opens_e0_and_waits_for_a_complete_fiche() {
        let d = dir("new");
        let s = Session::open(workflow(), d.clone(), owner(), vec!["doc".into()], 1_000, 0);
        let v = s.view(1_000, None, true);
        assert!(v.readable && !v.ready);
        assert_eq!(v.steps[0].state, "EN COURS");
        assert_eq!(v.steps[1].state, "À FAIRE");
        assert_eq!(v.button, "Lancer la vérification");
        assert!(v.blocked.as_deref().unwrap().starts_with("Complétez la fiche de cette étape avant la vérification. Champs à renseigner : "));
        assert!(d.join("progress.json").is_file());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn verdicts_are_worded_with_the_public_messages_and_persist() {
        let d = dir("verdicts");
        let s = Session::open(workflow(), d.clone(), owner(), vec!["doc".into()], 0, 0);
        {
            // skip the fiche precondition for this test: begin through the progress directly
            let mut st = s.state.lock().unwrap();
            let p = st.progress.as_mut().unwrap();
            p.begin(0, 0, "a1".into(), 1, Evidence::default()).unwrap();
        }
        s.finish(Outcome::Judged { pass: false, diagnostic: Some(true), verified: Some(false), competences: None }, 60).unwrap();
        let v = s.view(60, None, true);
        assert_eq!(v.feedback, vec!["Échec à corriger.", "Fiche nominale : conforme", "Essai nominal : non réussi", DETAIL_HIDDEN]);
        assert_eq!(v.steps[0].state, "ÉCHEC À CORRIGER");
        // reopened from disk: same progression, the attempt kept
        drop(s);
        let again = Session::open(workflow(), d.clone(), owner(), vec![], 100, 0);
        assert_eq!(again.judged_attempts(), 1);
        assert_eq!(again.view(100, None, true).steps[0].state, "ÉCHEC À CORRIGER");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn another_pack_content_or_a_tampered_file_is_unreadable() {
        let d = dir("owner");
        drop(Session::open(workflow(), d.clone(), owner(), vec![], 0, 0));
        let other = Owner { workflow_sha256: "x".into(), ..owner() };
        let s = Session::open(workflow(), d.clone(), other, vec![], 0, 0);
        let v = s.view(0, None, true);
        assert!(!v.readable && !v.ready);
        assert_eq!(v.blocked.as_deref(), Some(UNREADABLE));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// A nominal part with every controlled field filled (values of no mission).
    fn complete() -> NominalPart {
        use crate::workflow::fiche::{BitField, Decision, DocumentUse, FrameEntry, InvalidCode, Order, Protection, Sign, SignalEntry, StatusEntry, Supervision};
        let field = |lsb, len| BitField { ordre: Some(Order::Intel), lsb_bit: Some(lsb), longueur: Some(len) };
        NominalPart {
            documents_utilises: vec![DocumentUse { document: Some("doc".into()), pages: String::new() }],
            trames: vec![FrameEntry { identifiant: Some(0x7A5), format: Some("STANDARD_11_BITS".into()), dlc: Some(8), periode_ms: Some(20),
                protection: Protection { presente: Some(true), crc: field(0, 8), compteur: field(8, 4) }, compteur_de_vie: BitField::default() }],
            signaux: vec![SignalEntry { nom: String::new(), identifiant: Some(0x7A5), slots: vec![0], ordre: Some(Order::Intel), lsb_bit: Some(16),
                longueur: Some(8), signe: Some(Sign::Unsigned), facteur: Some(1.0), offset: Some(0.0), code_invalide: Some(InvalidCode::Code(255)) }],
            statuts: vec![StatusEntry { identifiant: Some(0x7A5), slots: vec![0], ordre: Some(Order::Intel), lsb_bit: Some(24), longueur: Some(2), code_ok: Some(0) }],
            supervision: Supervision { premier_timeout_ms: Some(1), timeout_ms: Some(1), peremption_ms: Some(1), pertes_consecutives_tolerees: Some(0) },
            decisions: [Some(Decision::Publier); 4],
            justification: String::new(),
        }
    }

    #[test]
    fn the_board_comes_last_five_steps_are_shown_and_m10_counts_down() {
        let d = dir("board");
        let s = Session::open(workflow(), d.clone(), owner(), vec!["doc".into()], 0, 0);
        let v = s.view(0, None, false);
        assert_eq!(v.steps.len(), 5, "the sixth box is the fiche");
        assert!(v.blocked.as_deref().unwrap().starts_with("Complétez la fiche"), "M-04 comes before M-06");
        s.set_nominal(complete()).unwrap();
        assert_eq!(s.view(0, None, false).blocked.as_deref(), Some(BOARD));
        let v = s.view(0, None, true);
        assert!(v.ready && v.blocked.is_none() && !v.running);
        let v = s.view(0, Some((2.2, 9.0)), true);
        assert!(v.running && !v.ready);
        assert_eq!(v.blocked.as_deref(), Some("Essai en cours — 7 s. Ne débranchez pas la carte et ne fermez pas l'application."));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_form_refuses_out_of_range_shapes_but_accepts_partial_fiches() {
        let d = dir("shape");
        let s = Session::open(workflow(), d.clone(), owner(), vec![], 0, 0);
        let mut part = NominalPart::default();
        part.trames.push(fiche::FrameEntry { identifiant: Some(0x7FF), ..Default::default() });
        s.set_nominal(part.clone()).unwrap();
        assert_eq!(s.sheet_seq(), 1);
        part.trames[0].identifiant = Some(0x800);
        assert!(s.set_nominal(part.clone()).is_err());
        part.trames[0].identifiant = Some(1);
        part.justification = "x".repeat(2_001);
        assert!(s.set_nominal(part).is_err());
        drop(s);
        let reopened = Session::open(workflow(), d.clone(), owner(), vec![], 0, 0);
        assert_eq!(reopened.nominal().trames[0].identifiant, Some(0x7FF));
        let _ = std::fs::remove_dir_all(&d);
    }

    fn evidence() -> Evidence {
        Evidence::default()
    }

    /// E0 passed through the progress directly: incident 1 is in force.
    fn at_incident_1(name: &str) -> (Session, PathBuf) {
        let d = dir(name);
        let s = Session::open(workflow(), d.clone(), owner(), vec![], 0, 0);
        s.state.lock().unwrap().progress.as_mut().unwrap().begin(0, 0, "a0".into(), 1, evidence()).unwrap();
        s.finish(Outcome::Judged { pass: true, diagnostic: Some(true), verified: Some(true), competences: None }, 10).unwrap();
        (s, d)
    }

    fn stored_carrier(d: &Path) -> String {
        let v: serde_json::Value = serde_json::from_slice(&std::fs::read(d.join("progress.json")).unwrap()).unwrap();
        let text = v.to_string();
        let at = text.find("\"carrier\":\"").expect("a carrier string") + 11;
        text[at..at + 64].to_owned()
    }

    #[test]
    fn the_carrier_field_never_tells_a_wire_defect_from_another_anomaly() {
        let (s, d) = at_incident_1("carrier");
        assert_eq!(s.active().map(|a| (a.rank, a.carrier)), Some((1, None)), "no carrier yet");
        let filler = stored_carrier(&d);
        assert!(filler.len() == 64 && filler.chars().all(|c| c.is_ascii_hexdigit()), "a keyed value of the tag's shape on disk");
        let tag = s.tag(42);
        s.set_carrier(Some(tag.clone())).unwrap();
        assert_eq!(s.active().unwrap().carrier.as_deref(), Some(tag.as_str()));
        assert_eq!(stored_carrier(&d), tag);
        s.set_carrier(None).unwrap();
        assert_eq!(s.active().unwrap().carrier, None);
        assert_eq!(stored_carrier(&d), filler, "back to the filler of this activation");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn reproducing_the_anomaly_is_recorded_and_refused_during_an_attempt() {
        let (s, d) = at_incident_1("reproduce");
        assert_eq!(s.reproduce(500, None), Ok(1));
        let saved = store::load_json_keyed(&d.join("progress.json"), &Some(s.workflow.key)).unwrap().unwrap();
        let p: Progress = serde_json::from_value(saved).unwrap();
        assert_eq!(p.reproductions, vec![crate::workflow::progress::Reproduction { rank: 1, at_s: 500 }]);
        assert_eq!(s.active().unwrap().carrier, None, "no carrier given: the filler, read as none");
        assert_eq!(s.reproduce(520, Some("ab".repeat(32))), Ok(1));
        assert_eq!(s.active().unwrap().carrier, Some("ab".repeat(32)), "the carrier of now, in the same save");
        s.state.lock().unwrap().progress.as_mut().unwrap().begin(1, 700, "a1".into(), 1, evidence()).unwrap();
        assert_eq!(s.reproduce(710, None), Err(REPRODUCE_REFUSED.to_owned()));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_qualification_shows_its_eight_lines_only_and_its_own_running_text() {
        let d = dir("qualification");
        let s = Session::open(workflow(), d.clone(), owner(), vec![], 0, 0);
        {
            let mut st = s.state.lock().unwrap();
            let p = st.progress.as_mut().unwrap();
            for k in 0..4 {
                p.steps[k].state = StepState::Passed;
            }
            p.steps[4].state = StepState::Open;
            p.begin(4, 100, "q1".into(), 1, evidence()).unwrap();
        }
        let running = s.view(110, Some((10.0, 28.0)), true);
        assert_eq!(running.blocked.as_deref(), Some("Qualification en cours — 18 s. Ne débranchez pas la carte et ne fermez pas l'application."));
        let mut cps = [true; 8];
        cps[1] = false;
        s.finish(Outcome::Judged { pass: false, diagnostic: None, verified: Some(false), competences: Some(cps.to_vec()) }, 130).unwrap();
        let v = s.view(130, None, true);
        assert_eq!(v.feedback.len(), 8, "{:?}", v.feedback);
        assert_eq!(v.feedback[1], "CP-2 — Décoder les valeurs nominales : FAIL");
        assert!(v.feedback.iter().all(|l| l.starts_with("CP-")));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_progress_edited_by_hand_never_takes_effect() {
        let (s, d) = at_incident_1("tamper");
        drop(s);
        // every step marked passed, the JSON still well formed: the integrity check refuses it
        let path = d.join("progress.json");
        let mut doc: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        fn pass_all(v: &mut serde_json::Value) {
            match v {
                serde_json::Value::Object(m) => {
                    if let Some(serde_json::Value::Array(steps)) = m.get_mut("steps") {
                        for step in steps {
                            step["state"] = serde_json::json!("passed");
                        }
                    }
                    for x in m.values_mut() {
                        pass_all(x);
                    }
                }
                serde_json::Value::Array(a) => a.iter_mut().for_each(pass_all),
                _ => {}
            }
        }
        pass_all(&mut doc);
        std::fs::write(&path, serde_json::to_vec_pretty(&doc).unwrap()).unwrap();
        let again = Session::open(workflow(), d.clone(), owner(), vec![], 100, 0);
        let v = again.view(100, None, true);
        assert!(v.steps.iter().any(|x| x.state != "VALIDÉ"), "the edited steps are not trusted: {:?}", v.steps);
        assert!(again.export(&d.join("Documents"), 100).is_err(), "no closure, no export");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_export_of_one_state_is_reproducible() {
        let d = dir("export");
        let s = Session::open(workflow(), d.clone(), owner(), vec![], 0, 0);
        {
            let mut st = s.state.lock().unwrap();
            let p = st.progress.as_mut().unwrap();
            for k in 0..STEP_NAMES.len() {
                p.steps[k].state = StepState::Passed;
            }
        }
        let a = s.export(&d.join("a"), 1_000).unwrap();
        let b = s.export(&d.join("b"), 2_000).unwrap();
        let read = |page: &Path| std::fs::read(page.with_file_name("fiche.json")).unwrap();
        assert_eq!(read(&a), read(&b), "the same state gives the same fiche.json, whatever the export instant");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    #[cfg(windows)]
    fn a_second_instance_never_writes_an_open_progression() {
        let d = dir("instance");
        let first = Session::open(workflow(), d.clone(), owner(), vec![], 0, 0);
        let second = Session::open(workflow(), d.clone(), owner(), vec![], 0, 0);
        let v = second.view(0, None, true);
        assert!(!v.readable && !v.nominal_editable);
        assert_eq!(v.blocked.as_deref(), Some(IN_USE));
        assert!(second.set_nominal(NominalPart::default()).is_err(), "nothing written through the second instance");
        assert!(std::fs::read(d.join("session.lock")).is_ok(), "a reader opens the lock file");
        drop(second);
        drop(first);
        assert!(Session::open(workflow(), d.clone(), owner(), vec![], 0, 0).view(0, None, true).readable, "free again");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_deleted_history_is_refused_never_restarted() {
        let d = dir("history");
        let s = Session::open(workflow(), d.clone(), owner(), vec!["doc".into()], 0, 0);
        s.set_nominal(complete()).unwrap();
        drop(s);
        std::fs::remove_file(d.join("progress.json")).unwrap();
        let again = Session::open(workflow(), d.clone(), owner(), vec![], 0, 0);
        let v = again.view(0, None, true);
        assert_eq!(v.blocked.as_deref(), Some(UNREADABLE), "the fiche without its progression: a deleted history");
        assert!(!v.nominal_editable && again.set_nominal(complete()).is_err(), "an unreadable progression freezes the fiche");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_measure_keeps_the_carrier_of_its_recording() {
        let (s, d) = at_incident_1("consigned");
        let tag = s.tag(7);
        s.set_carrier(Some(tag.clone())).unwrap();
        let meter = crate::workflow::incident::MeterRecord { com: None, vohm: None, fonction: None, lecture: None };
        let conditions = Conditions { etats: std::collections::BTreeMap::new(), t_sim_us: 0, multimetre: meter };
        let preuve = Preuve { frames: Vec::new(), circuit: String::new() };
        s.consign(1, "V".into(), conditions, &preuve, 20).unwrap();
        // « Reproduire » puts the defect on another wire: the measure still names the wire of its recording
        s.reproduce(30, Some(s.tag(8))).unwrap();
        assert_eq!(s.consigned_carrier(1), Some(tag));
        assert_eq!(s.preuve(1), Some(preuve));
        let _ = std::fs::remove_dir_all(&d);
    }
}
