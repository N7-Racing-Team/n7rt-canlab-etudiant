//! The handle behind [`super::PhysLab`] (contract §8.1, §8.3): the `phys` thread paces the core on the monotonic
//! clock in RUN (at most 20 frames per wake, then the lag is counted and the slow flag raised), values are
//! published at most every 50 ms and after each command, and a panic of the core is caught: the edit or
//! interaction in cause is quarantined (its payload, whatever its base revision), the core is rebuilt from the
//! last committed document (STOP, solver reset, empty history) with the diagnostic `engine_restarted`; beyond 3
//! restarts every phys command answers `engine_unavailable`.
//!
//! Callers lock the core once per call; the thread locks it once per frame and lets a waiting caller in
//! between two frames, so a call never waits for more than one frame computation.

use std::collections::{BTreeMap, VecDeque};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime};

use n7rt_circuit::{persist, Circuit, CompId, Edit, Interact, Refusal, RefusalCode, Value, ValueMap};
use n7rt_solver::{SolverStats, FRAME_NS};

use super::files;
use super::kernel::Core;
use super::probe::{Capture, Frame};
use super::{
    CommandReply, DemoEntry, Diagnostic, Disclosure, EngineStats, FileEntry, FileError, JournalEntry, JournalStatus, OpState, PhysCommand,
    PhysConfig, PhysError, PhysFrame, PreviewReply, RunState, Severity, SnapshotDto, ValuesDto,
};

pub(crate) const MAX_RESTARTS: u32 = 3;
pub(crate) const MAX_CATCH_UP: u64 = 20;
pub(crate) const PUBLISH_PERIOD: Duration = Duration::from_millis(50);
/// Journal entries kept in memory before spilling to the session file.
pub(crate) const JOURNAL_MEMORY: usize = 10_000;
/// Entries left in memory after a spill (Journal tab, set_pot coalescing).
const JOURNAL_KEEP: usize = 1_000;
const QUARANTINE_MAX: usize = 32;
pub(crate) const NEW_NAME: &str = "sans titre";

pub(crate) struct Lab {
    shared: Arc<Shared>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

struct Shared {
    host: Mutex<Host>,
    wake: Condvar,
    stop: AtomicBool,
    /// Callers waiting for the lock: the thread yields to them between two frames.
    waiting: AtomicUsize,
    data_dir: Option<PathBuf>,
}

/// Monotonic pacing of RUN.
struct Pace {
    origin: Instant,
    done: u64,
    lag_frames: u64,
    slow_until: Option<Instant>,
    /// (end instant, compute time in µs) of the frames of the last second.
    times: VecDeque<(Instant, f64)>,
}

impl Pace {
    fn new() -> Pace {
        Pace { origin: Instant::now(), done: 0, lag_frames: 0, slow_until: None, times: VecDeque::new() }
    }

    fn restart(&mut self, now: Instant) {
        self.origin = now;
        self.done = 0;
        self.lag_frames = 0;
    }

    fn due(&self, now: Instant) -> u64 {
        let elapsed = now.saturating_duration_since(self.origin).as_nanos() / FRAME_NS as u128;
        u64::try_from(elapsed).unwrap_or(u64::MAX).saturating_sub(self.done)
    }

    fn until_next(&self, now: Instant) -> Duration {
        let next = self.origin + Duration::from_nanos((self.done + 1).saturating_mul(FRAME_NS as u64));
        next.saturating_duration_since(now)
    }

    /// More frames due than one wake may compute: count them, drop them (re-sync) and raise the slow flag.
    fn fall_behind(&mut self, lag: u64, now: Instant) {
        self.lag_frames = lag;
        self.origin += Duration::from_nanos(lag.saturating_mul(FRAME_NS as u64));
        self.slow_until = Some(now + Duration::from_secs(1));
    }

    fn record(&mut self, compute: Duration, now: Instant) {
        self.times.push_back((now, compute.as_secs_f64() * 1e6));
        while self.times.front().is_some_and(|(t, _)| now.duration_since(*t) > Duration::from_secs(1)) {
            self.times.pop_front();
        }
    }

    fn p95(&self) -> f64 {
        if self.times.is_empty() {
            return 0.0;
        }
        let mut v: Vec<f64> = self.times.iter().map(|(_, us)| *us).collect();
        v.sort_by(f64::total_cmp);
        let i = ((v.len() as f64 * 0.95).ceil() as usize).clamp(1, v.len()) - 1;
        v[i]
    }

    fn stats(&self, s: &SolverStats, restarts: u32) -> EngineStats {
        EngineStats {
            lag_frames: self.lag_frames,
            slow: self.slow_until.is_some_and(|t| Instant::now() < t),
            frame_us_p95: self.p95(),
            substeps_last: s.substeps_last_frame,
            min_dt_ns: s.min_dt_ns,
            islands: s.islands,
            restarts,
        }
    }
}

/// Spill state of the journal of the current core.
struct Session {
    started: String,
    initial: Circuit,
    spilled_entries: u64,
    lost_entries: u64,
    archive_corrupt: bool,
    fault: Option<JournalFault>,
}

#[derive(Clone, Copy)]
enum JournalFault { NoStorage, Io, Serialization, CorruptArchive }

impl Session {
    fn new(initial: &Circuit) -> Session {
        Session { started: files::timestamp(SystemTime::now()), initial: initial.clone(), spilled_entries: 0,
            lost_entries: 0, archive_corrupt: false, fault: None }
    }

    fn loss_notice(&self) -> Option<Diagnostic> {
        if self.lost_entries == 0 && !self.archive_corrupt { return None; }
        let cause = match self.fault {
            Some(JournalFault::NoStorage) => "aucun dossier de données configuré",
            Some(JournalFault::Serialization) => "sérialisation impossible",
            Some(JournalFault::CorruptArchive) => "archive endommagée, écriture désactivée jusqu'au prochain document",
            _ => "écriture du journal impossible",
        };
        Some(Diagnostic { code: "journal_incomplete".into(), severity: Severity::Warning, items: vec![],
            message_fr: format!("Journal incomplet : {} entrées non archivées ({cause}). L'historique intégral ne peut pas être exporté ou rejoué ; seules les parties conservées restent disponibles.", self.lost_entries) })
    }
}

struct Host {
    /// None once the engine is unavailable.
    core: Option<Core>,
    name: String,
    panics: u32,
    /// Last committed document, the restart point after a panic.
    last_good: Circuit,
    last_good_gen: u64,
    /// Last reported revision: revisions keep increasing across loads and restarts.
    last_rev: u64,
    quarantine: Vec<Culprit>,
    disclosure: Disclosure,
    /// Identity of the current document (SnapshotDto::doc_id), increased at every replacement.
    doc_id: u64,
    /// The document is a mission bench (load_circuit_json): never saved, its journal never shown, whatever the
    /// disclosure, until another document replaces it.
    sealed: bool,
    /// Check lock (mission check driving the bench): the interaction states shown to the UI, frozen when the lock
    /// was set. While Some, every UI-side command is refused (`bench_locked`).
    check_lock: Option<BTreeMap<CompId, ValueMap>>,
    /// The multimeter as shown when the check lock was set: the trajectory the check drives is never displayed, on the
    /// meter either.
    check_meter: Option<super::MeterValue>,
    seq: u64,
    published: Option<ValuesDto>,
    publish_after_frame: bool,
    last_publish: Instant,
    pace: Pace,
    /// A mission source owns simulated time while this generation token is active.
    source_clock: Option<u64>,
    next_source_clock: u64,
    session: Session,
    last_frame: Option<Arc<PhysFrame>>,
    #[cfg(test)]
    inject_panics: u32,
    #[cfg(test)]
    inject_journal_rollback_failure: bool,
}

/// What a panic quarantines: the payload of an edit or an interaction. Run, Stop, Reset, Undo and Redo never
/// are: the restart already leaves the core in STOP with an empty history, and [`MAX_RESTARTS`] bounds a repeat.
#[derive(Clone, Debug, PartialEq)]
enum Culprit {
    Edit(Edit),
    Interact(Interact),
}

impl Culprit {
    fn of(command: &PhysCommand) -> Option<Culprit> {
        match command {
            PhysCommand::Edit { edit, .. } => Some(Culprit::Edit(edit.clone())),
            PhysCommand::Interact { interact } => Some(Culprit::Interact(interact.clone())),
            PhysCommand::Undo | PhysCommand::Redo | PhysCommand::Run | PhysCommand::Stop | PhysCommand::Reset | PhysCommand::System { .. }
            | PhysCommand::Meter { .. } => None,
        }
    }
}

fn restart_notice(n: u32) -> Diagnostic {
    Diagnostic {
        code: "engine_restarted".to_string(),
        severity: Severity::Error,
        message_fr: format!(
            "Moteur de simulation redémarré après une erreur interne ({n}/{MAX_RESTARTS}) : circuit repris au dernier état validé, simulation à l'arrêt"
        ),
        items: Vec::new(),
    }
}

/// Save refused while a mission bench is the document.
fn mission_locked() -> PhysError {
    PhysError::Refused {
        refusal: Refusal {
            code: RefusalCode::MissionLocked,
            ambiguity: None,
            message_fr: "Enregistrement indisponible en mission.".to_string(),
            at: None,
            items: Vec::new(),
        },
    }
}

/// A command of the mission engine received from the interface: a forged call must not clear a defect.
fn internal_only() -> PhysError {
    PhysError::Refused {
        refusal: Refusal {
            code: RefusalCode::InternalOnly,
            ambiguity: None,
            message_fr: "Commande réservée au moteur de mission.".to_string(),
            at: None,
            items: Vec::new(),
        },
    }
}

/// UI-side call refused while a mission check drives the bench.
fn bench_locked() -> PhysError {
    PhysError::Refused {
        refusal: Refusal {
            code: RefusalCode::BenchLocked,
            ambiguity: None,
            message_fr: "Banc en lecture seule pendant l'essai.".to_string(),
            at: None,
            items: Vec::new(),
        },
    }
}

fn source_clock_owned() -> PhysError {
    PhysError::Refused {
        refusal: Refusal {
            code: RefusalCode::SourceClockOwned,
            ambiguity: None,
            message_fr: "Horloge du banc pilotée par la séance CAN : arrêtez la séance avant cette commande.".into(),
            at: None,
            items: Vec::new(),
        },
    }
}

/// While the check lock is on, the UI sees the interaction states (and the contact / travel they show) as they were
/// when it was set: the trajectory driven by the check is never displayed.
fn freeze_values(v: &mut ValuesDto, frozen: &BTreeMap<CompId, ValueMap>) {
    for c in v.comps.iter_mut() {
        let Some(f) = frozen.get(&c.id) else { continue };
        c.state = f.clone();
        match (&mut c.op, f.get("position"), f.get("closed")) {
            (OpState::Pot { position }, Some(Value::Num(x)), _) => *position = *x,
            (OpState::Contact { closed }, _, Some(Value::Bool(b))) => *closed = *b,
            _ => {}
        }
    }
}

fn empty_frame() -> Arc<PhysFrame> {
    Arc::new(PhysFrame { inner: Frame::build(Arc::new(Capture::empty()), 0, Disclosure::Exploration, &[]) })
}

impl Host {
    fn start() -> Host {
        let circuit = Circuit::new(NEW_NAME);
        let mut core = catch_unwind(AssertUnwindSafe(|| Core::new(circuit.clone()))).ok();
        if let Some(c) = core.as_mut() {
            c.set_doc_id(1);
        }
        let dead = core.is_none();
        let last_good_gen = core.as_ref().map_or(0, Core::doc_gen);
        let last_rev = core.as_ref().map_or(0, Core::rev);
        Host {
            core,
            name: NEW_NAME.to_string(),
            panics: if dead { MAX_RESTARTS + 1 } else { 0 },
            session: Session::new(&circuit),
            last_good: circuit,
            last_good_gen,
            last_rev,
            quarantine: Vec::new(),
            disclosure: Disclosure::Exploration,
            doc_id: 1,
            sealed: false,
            check_lock: None,
            check_meter: None,
            seq: 1,
            published: None,
            publish_after_frame: false,
            last_publish: Instant::now(),
            pace: Pace::new(),
            source_clock: None,
            next_source_clock: 0,
            last_frame: None,
            #[cfg(test)]
            inject_panics: 0,
            #[cfg(test)]
            inject_journal_rollback_failure: false,
        }
    }

    fn running(&self) -> bool {
        self.core.as_ref().is_some_and(|c| c.run_state() == RunState::Run)
    }

    fn autonomously_running(&self) -> bool {
        self.source_clock.is_none() && self.running()
    }

    fn publish(&mut self) {
        self.seq += 1;
        self.published = None;
        self.last_publish = Instant::now();
    }

    /// After every successful operation: remember the committed document and the revision.
    fn after_success(&mut self) {
        if let Some(core) = &self.core {
            self.last_rev = core.rev();
            if core.doc_gen() != self.last_good_gen {
                self.last_good = core.document().circuit().clone();
                self.last_good_gen = core.doc_gen();
            }
        }
    }

    fn next_offset(&self) -> u64 {
        self.core.as_ref().map_or(self.last_rev, Core::rev).max(self.last_rev) + 1
    }

    fn on_panic(&mut self, culprit: Option<Culprit>) {
        self.panics += 1;
        if let Some(c) = culprit {
            if self.quarantine.len() >= QUARANTINE_MAX {
                self.quarantine.remove(0);
            }
            self.quarantine.push(c);
        }
        let offset = self.next_offset();
        self.core = None;
        self.source_clock = None;
        self.publish_after_frame = false;
        if self.panics <= MAX_RESTARTS {
            let circuit = self.last_good.clone();
            match catch_unwind(AssertUnwindSafe(|| Core::new(circuit))) {
                Ok(mut core) => {
                    self.doc_id += 1;
                    core.set_doc_id(self.doc_id);
                    core.set_rev_offset(offset);
                    core.set_disclosure(self.disclosure);
                    core.set_notices(vec![restart_notice(self.panics)]);
                    self.last_rev = core.rev();
                    self.last_good_gen = core.doc_gen();
                    self.session = Session::new(&self.last_good);
                    self.core = Some(core);
                }
                Err(_) => self.panics = MAX_RESTARTS + 1,
            }
        }
        self.publish();
    }

    /// Replaces the core by a fresh one on `circuit` (new, load, demo): STOP, reset, empty undo history.
    fn replace(&mut self, circuit: Circuit, name: String) -> Result<SnapshotDto, PhysError> {
        if self.core.is_none() {
            return Err(PhysError::EngineUnavailable);
        }
        let offset = self.next_offset();
        let fresh = circuit.clone();
        match catch_unwind(AssertUnwindSafe(|| Core::new(fresh))) {
            Ok(mut core) => {
                self.doc_id += 1;
                self.source_clock = None;
                core.set_doc_id(self.doc_id);
                core.set_rev_offset(offset);
                core.set_disclosure(self.disclosure);
                self.last_rev = core.rev();
                self.last_good_gen = core.doc_gen();
                self.session = Session::new(&circuit);
                self.last_good = circuit;
                self.core = Some(core);
                self.name = name;
                self.quarantine.clear();
                self.publish_after_frame = false;
                self.pace.lag_frames = 0;
                self.publish();
                Ok(self.snapshot())
            }
            Err(_) => {
                self.on_panic(None);
                Err(PhysError::EngineUnavailable)
            }
        }
    }

    fn snapshot(&mut self) -> SnapshotDto {
        let name = self.name.clone();
        let built = self.core.as_ref().map(|core| catch_unwind(AssertUnwindSafe(|| core.snapshot(&name))));
        let mut s = match built {
            Some(Ok(s)) => s,
            Some(Err(_)) => {
                self.on_panic(None);
                return self.snapshot();
            }
            None => self.fallback_snapshot(),
        };
        if let Some(frozen) = &self.check_lock {
            for (id, state) in s.states.iter_mut() {
                if let Some(f) = frozen.get(id) {
                    *state = f.clone();
                }
            }
        }
        s
    }

    /// Refused while the check lock is on (UI-side calls only).
    fn unlocked(&self) -> Result<(), PhysError> {
        if self.check_lock.is_some() {
            return Err(bench_locked());
        }
        Ok(())
    }

    /// The engine is unavailable: the last committed document, as it was.
    fn fallback_snapshot(&self) -> SnapshotDto {
        let c = &self.last_good;
        let nets = catch_unwind(AssertUnwindSafe(|| c.netlist().nets)).unwrap_or_default();
        let states = catch_unwind(AssertUnwindSafe(|| {
            c.components().keys().filter_map(|id| c.state(*id).map(|s| (*id, s.clone()))).collect()
        }))
        .unwrap_or_default();
        SnapshotDto {
            doc_id: self.doc_id,
            rev: self.last_rev,
            name: self.name.clone(),
            dirty: false,
            components: c.components().values().cloned().collect(),
            wires: c.wires().values().cloned().collect(),
            states,
            nets,
            can_undo: false,
            can_redo: false,
            run: RunState::Stop,
            sim_time_ns: 0,
            disclosure: self.disclosure,
        }
    }

    /// Another document replaced the mission bench: exploration again, saving allowed.
    fn leave_bench(&mut self) {
        self.sealed = false;
        if self.disclosure != Disclosure::Exploration {
            self.disclosure = Disclosure::Exploration;
            if let Some(core) = self.core.as_mut() {
                core.set_disclosure(Disclosure::Exploration);
            }
            self.publish();
        }
    }

    /// Bounds RAM in every storage mode. Loss is sticky and visible; archives with an unknown partial append
    /// are quarantined. A marker is best effort, and its absence never promises a complete disk archive.
    fn spill(&mut self, data_dir: &Option<PathBuf>) {
        let Some(core) = self.core.as_mut() else { return };
        let len = core.journal().len();
        if len <= JOURNAL_MEMORY {
            return;
        }
        let n = len - JOURNAL_KEEP;
        if self.sealed {
            // The session file starts with the initial circuit: a mission bench (hidden values) never reaches the
            // data directory. Its oldest entries are dropped instead.
            core.drain_journal(n);
            return;
        }
        let mut fault = None;
        let mut rollback_failed = false;
        if let Some(dir) = data_dir {
            // Never resume appending across a known gap unless the on-disk loss notice can be persisted first.
            let marked = self.session.lost_entries == 0
                || files::mark_journal_incomplete(dir, &self.session.started, self.session.lost_entries, self.session.archive_corrupt).is_ok();
            if self.session.archive_corrupt {
                fault = Some(JournalFault::CorruptArchive);
            } else if !marked {
                fault = Some(JournalFault::Io);
            } else {
                match core.journal()[..n].iter().map(serde_json::to_string).collect::<Result<Vec<_>, _>>() {
                    Err(_) => fault = Some(JournalFault::Serialization),
                    Ok(lines) => {
                        let initial = persist::save(&self.session.initial);
                        #[cfg(test)]
                        let injected = std::mem::take(&mut self.inject_journal_rollback_failure);
                        #[cfg(not(test))]
                        let injected = false;
                        let result = if injected {
                            Err(files::JournalAppendError { error: std::io::Error::other("injected rollback failure"), rollback_failed: true })
                        } else { files::append_journal(dir, &self.session.started, &initial, &lines) };
                        if let Err(error) = result {
                            rollback_failed = error.rollback_failed;
                            let _ = error.error;
                            fault = Some(if rollback_failed { JournalFault::CorruptArchive } else { JournalFault::Io });
                        }
                    }
                }
            }
        } else {
            fault = Some(JournalFault::NoStorage);
        }
        core.drain_journal(n);
        if let Some(fault) = fault {
            self.session.lost_entries = self.session.lost_entries.saturating_add(n as u64);
            self.session.archive_corrupt |= rollback_failed;
            self.session.fault = Some(fault);
            if let Some(dir) = data_dir {
                let _ = files::mark_journal_incomplete(dir, &self.session.started, self.session.lost_entries, self.session.archive_corrupt);
            }
            self.publish();
        } else {
            self.session.spilled_entries = self.session.spilled_entries.saturating_add(n as u64);
        }
    }
}

fn lock(shared: &Shared) -> MutexGuard<'_, Host> {
    shared.waiting.fetch_add(1, Ordering::SeqCst);
    let g = shared.host.lock().unwrap_or_else(|e| e.into_inner());
    shared.waiting.fetch_sub(1, Ordering::SeqCst);
    g
}

impl Lab {
    pub fn start(cfg: PhysConfig) -> Lab {
        let shared = Arc::new(Shared {
            host: Mutex::new(Host::start()),
            wake: Condvar::new(),
            stop: AtomicBool::new(false),
            waiting: AtomicUsize::new(0),
            data_dir: cfg.data_dir,
        });
        let s = shared.clone();
        let thread = thread::Builder::new().name("phys".into()).spawn(move || run(&s)).ok();
        Lab { shared, thread: Mutex::new(thread) }
    }

    pub fn snapshot(&self) -> SnapshotDto {
        lock(&self.shared).snapshot()
    }

    pub fn preview(&self, base_rev: u64, edit: &Edit) -> Result<PreviewReply, PhysError> {
        let mut guard = lock(&self.shared);
        let h = &mut *guard;
        h.unlocked()?;
        let Some(core) = h.core.as_ref() else { return Err(PhysError::EngineUnavailable) };
        match catch_unwind(AssertUnwindSafe(|| core.preview(base_rev, edit))) {
            Ok(Ok(reply)) => Ok(reply),
            Ok(Err(refusal)) => Err(PhysError::Refused { refusal }),
            Err(_) => {
                h.on_panic(None);
                Err(PhysError::EngineUnavailable)
            }
        }
    }

    /// UI-side command: refused while the check lock is on, and always for a command of the mission engine.
    pub fn command(&self, command: PhysCommand) -> Result<CommandReply, PhysError> {
        if matches!(command, PhysCommand::System { .. }) {
            return Err(internal_only());
        }
        let mut guard = lock(&self.shared);
        guard.unlocked()?;
        if guard.source_clock.is_some() && matches!(command, PhysCommand::Run | PhysCommand::Stop | PhysCommand::Reset) {
            return Err(source_clock_owned());
        }
        self.exec(&mut guard, command)
    }

    /// Command of the mission controller (Rust): never refused by the check lock.
    pub fn command_internal(&self, command: PhysCommand) -> Result<CommandReply, PhysError> {
        let mut guard = lock(&self.shared);
        self.exec(&mut guard, command)
    }

    pub fn set_check_lock(&self, on: bool) {
        let mut h = lock(&self.shared);
        h.check_lock = if on {
            let frozen = h.core.as_ref().map(|core| {
                let c = core.document().circuit();
                c.components().keys().filter_map(|id| c.state(*id).map(|s| (*id, s.clone()))).collect()
            });
            Some(frozen.unwrap_or_default())
        } else {
            None
        };
        if !on {
            // the display was frozen during the check: its average restarts on the bench given back
            if let Some(core) = h.core.as_mut() {
                core.restart_meter();
            }
        }
        let shown = h.core.as_ref().map(|core| core.meter_value());
        h.check_meter = on.then(|| shown.unwrap_or_else(|| super::MeterValue { display: "—".into(), ..Default::default() }));
        h.publish();
    }

    /// A copy of the live circuit (mission controller: physical checks of a verification); `None` without a document.
    pub fn circuit_copy(&self) -> Option<n7rt_circuit::Circuit> {
        lock(&self.shared).core.as_ref().map(|core| core.document().circuit().clone())
    }

    pub fn defects(&self) -> Option<(std::collections::BTreeSet<n7rt_circuit::WireId>, bool)> {
        lock(&self.shared).core.as_ref().map(|core| core.defects())
    }

    /// The document is a mission bench (loaded by the mission controller, not replaced since by another document).
    pub fn on_mission_bench(&self) -> bool {
        let h = lock(&self.shared);
        h.core.is_some() && h.disclosure == Disclosure::Mission
    }

    pub fn check_locked(&self) -> bool {
        lock(&self.shared).check_lock.is_some()
    }

    /// A command under the lock: quarantine, catch_unwind, pacing restart on RUN, publication.
    fn exec(&self, h: &mut Host, command: PhysCommand) -> Result<CommandReply, PhysError> {
        // A stop/reset invalidates a source session. Its later Drop cannot release a newer owner.
        let invalidates_clock = matches!(command, PhysCommand::Stop | PhysCommand::Reset);
        let culprit = Culprit::of(&command);
        if culprit.as_ref().is_some_and(|c| h.quarantine.contains(c)) {
            return Err(PhysError::EngineUnavailable);
        }
        #[cfg(test)]
        let inject = std::mem::take(&mut h.inject_panics) > 0;
        #[cfg(not(test))]
        let inject = false;
        let Some(core) = h.core.as_mut() else { return Err(PhysError::EngineUnavailable) };
        let was_running = core.run_state() == RunState::Run;
        let result = catch_unwind(AssertUnwindSafe(|| {
            if inject {
                panic!("panne simulée du cœur (test)");
            }
            core.apply(&command)
        }));
        match result {
            Ok(Ok(reply)) => {
                if invalidates_clock { h.source_clock = None; }
                let running = h.running();
                h.after_success();
                if running && !was_running {
                    h.pace.restart(Instant::now());
                    self.shared.wake.notify_all();
                }
                h.publish();
                h.publish_after_frame = running;
                h.spill(&self.shared.data_dir);
                Ok(reply)
            }
            Ok(Err(refusal)) => Err(PhysError::Refused { refusal }),
            Err(_) => {
                h.on_panic(culprit);
                Err(PhysError::EngineUnavailable)
            }
        }
    }

    pub fn values(&self, since_seq: u64) -> Option<ValuesDto> {
        let mut guard = lock(&self.shared);
        let h = &mut *guard;
        if h.seq <= since_seq {
            return None;
        }
        if h.published.is_none() {
            let core = h.core.as_ref()?;
            let stats = h.pace.stats(&core.solver_stats(), h.panics.min(MAX_RESTARTS));
            let (seq, disclosure) = (h.seq, h.disclosure);
            match catch_unwind(AssertUnwindSafe(|| (core.values(seq, disclosure, stats), core.frame()))) {
                Ok((mut values, frame)) => {
                    if !h.sealed && disclosure == Disclosure::Exploration {
                        if let Some(notice) = h.session.loss_notice() {
                            values.diagnostics.push(notice);
                            super::diag::sort(&mut values.diagnostics);
                        }
                    }
                    if let Some(frozen) = &h.check_lock {
                        freeze_values(&mut values, frozen);
                    }
                    if let Some(meter) = &h.check_meter {
                        values.meter = meter.clone();
                    }
                    h.last_frame = Some(frame);
                    h.published = Some(values);
                }
                Err(_) => {
                    h.on_panic(None);
                    return None;
                }
            }
        }
        h.published.clone()
    }

    pub fn probe(&self) -> Arc<PhysFrame> {
        let mut guard = lock(&self.shared);
        let h = &mut *guard;
        let built = h.core.as_ref().map(|core| catch_unwind(AssertUnwindSafe(|| core.frame())));
        match built {
            Some(Ok(frame)) => {
                h.last_frame = Some(frame.clone());
                frame
            }
            Some(Err(_)) => {
                h.on_panic(None);
                h.last_frame.clone().unwrap_or_else(empty_frame)
            }
            None => h.last_frame.clone().unwrap_or_else(empty_frame),
        }
    }

    pub fn configure_source(&self, ecu: CompId, pack: &n7rt_vehicle::Pack) -> Result<(), String> {
        let mut h=lock(&self.shared);
        let core=h.core.as_mut().ok_or_else(|| "moteur physique indisponible".to_string())?;
        core.configure_source(ecu,pack)?;
        h.last_frame=None;
        h.publish();
        self.shared.wake.notify_all();
        Ok(())
    }

    pub fn set_sensor_seed(&self,seed:u64) {
        let mut h=lock(&self.shared);
        if let Some(core)=h.core.as_mut() {core.set_sensor_seed(seed);}
    }

    /// Atomically stops and resets the physical model before a bus session takes its clock.
    /// The ordinary phys thread sleeps until the matching token is released.
    pub fn acquire_source_clock(&self) -> Result<u64, String> {
        let mut h = lock(&self.shared);
        if h.source_clock.is_some() { return Err("physical source clock already owned".into()); }
        let token = h.next_source_clock.checked_add(1).ok_or("physical source clock generation exhausted")?;
        self.exec(&mut h, PhysCommand::Stop).map_err(|_| "physical source cannot stop")?;
        self.exec(&mut h, PhysCommand::Reset).map_err(|_| "physical source cannot reset")?;
        h.source_clock = Some(token);
        h.next_source_clock = token;
        h.last_frame = None;
        self.shared.wake.notify_all();
        Ok(token)
    }

    /// Advances only the owned clock, without wall-clock catch-up or skipped frames.
    /// The reset preview at ms=0 and repeated reads of the current sample are allowed.
    pub fn advance_source_clock(&self, token: u64, ms: u64) -> Result<Arc<PhysFrame>, String> {
        let mut h = lock(&self.shared);
        if h.source_clock != Some(token) { return Err("stale physical source clock".into()); }
        let core = h.core.as_ref().ok_or("physical engine unavailable")?;
        if core.run_state() != RunState::Run { return Err("physical source is stopped".into()); }
        let current = core.frame_index();
        let gap = ms.checked_sub(current).ok_or("physical source clock moved backwards")?;
        if gap > MAX_CATCH_UP { return Err("physical source clock step exceeds 20 ms".into()); }
        let expected_ns = ms.checked_mul(FRAME_NS as u64).and_then(|n| i64::try_from(n).ok())
            .ok_or("physical source clock overflow")?;
        for _ in 0..gap {
            let t0 = Instant::now();
            let result = catch_unwind(AssertUnwindSafe(|| h.core.as_mut().expect("checked core").step()));
            if result.is_err() {
                h.on_panic(None);
                return Err("physical engine restarted during source step".into());
            }
            let t1 = Instant::now();
            h.pace.record(t1 - t0, t1);
            h.after_success();
            if h.publish_after_frame {
                h.publish_after_frame = false;
                h.publish();
            }
            h.spill(&self.shared.data_dir);
        }
        let frame = match catch_unwind(AssertUnwindSafe(|| h.core.as_ref().expect("checked core").frame())) {
            Ok(frame) => frame,
            Err(_) => {
                h.on_panic(None);
                return Err("physical engine restarted during source capture".into());
            }
        };
        if super::CircuitProbe::sim_time_ns(&*frame) != expected_ns {
            return Err("physical source capture time mismatch".into());
        }
        h.last_frame = Some(frame.clone());
        if h.last_publish.elapsed() >= PUBLISH_PERIOD { h.publish(); }
        Ok(frame)
    }

    /// A stale source never changes the clock of its successor.
    pub fn release_source_clock(&self, token: u64) -> Result<(), String> {
        let mut h = lock(&self.shared);
        if h.source_clock != Some(token) { return Err("stale physical source clock".into()); }
        h.source_clock = None;
        h.pace.restart(Instant::now());
        self.shared.wake.notify_all();
        Ok(())
    }

    pub fn set_disclosure(&self, disclosure: Disclosure) {
        let mut h = lock(&self.shared);
        h.disclosure = disclosure;
        if let Some(core) = h.core.as_mut() {
            core.set_disclosure(disclosure);
        }
        h.publish();
    }

    /// Nothing for a mission bench (sealed) or in mission mode: the journal would show what drove the bench.
    pub fn journal_tail(&self, n: usize) -> Vec<JournalEntry> {
        let h = lock(&self.shared);
        if h.sealed || h.disclosure == Disclosure::Mission {
            return Vec::new();
        }
        h.core.as_ref().map_or_else(Vec::new, |core| {
            let j = core.journal();
            j[j.len().saturating_sub(n)..].to_vec()
        })
    }

    pub fn journal_status(&self) -> Result<JournalStatus, PhysError> {
        let h = lock(&self.shared);
        if h.sealed || h.disclosure == Disclosure::Mission { return Err(mission_locked()); }
        let core = h.core.as_ref().ok_or(PhysError::EngineUnavailable)?;
        Ok(JournalStatus { retained_entries: core.journal().len(), spilled_entries: h.session.spilled_entries,
            lost_entries: h.session.lost_entries, archive_corrupt: h.session.archive_corrupt,
            lossless: h.session.lost_entries == 0 && !h.session.archive_corrupt })
    }

    /// Guard for any future integral export/replay of prefix + RAM tail. Does not finalize the disk prefix.
    pub fn require_lossless_session(&self) -> Result<(), PhysError> {
        let status = self.journal_status()?;
        if !status.lossless {
            return Err(PhysError::File { error: FileError::Io {
                message: "Journal incomplet : export ou replay intégral refusé".into() } });
        }
        Ok(())
    }

    /// Unavailable engine: the circuit stays as it was (the infallible signature cannot refuse).
    pub fn new_circuit(&self) -> SnapshotDto {
        let mut h = lock(&self.shared);
        if h.check_lock.is_some() {
            // The infallible signature cannot refuse: the bench stays.
            return h.snapshot();
        }
        match h.replace(Circuit::new(NEW_NAME), NEW_NAME.to_string()) {
            Ok(_) => {
                h.leave_bench();
                h.snapshot()
            }
            Err(_) => h.snapshot(),
        }
    }

    /// The mission bench (docs/apps-mission-slice.md §5): parsed by `persist::load` (hidden parameters included),
    /// it replaces the document like a load (STOP, reset, empty history and journal), takes `disclosure`, then
    /// runs. Saving stays refused until another document replaces it.
    pub fn load_circuit_json(&self, name: &str, json: &str, disclosure: Disclosure) -> Result<SnapshotDto, PhysError> {
        let mut guard = lock(&self.shared);
        let h = &mut *guard;
        if h.core.is_none() {
            return Err(PhysError::EngineUnavailable);
        }
        let circuit = match catch_unwind(AssertUnwindSafe(|| persist::load(json))) {
            Ok(Ok(circuit)) => circuit,
            Ok(Err(violations)) => return Err(PhysError::File { error: FileError::Invalid { violations } }),
            Err(_) => {
                h.on_panic(None);
                return Err(PhysError::EngineUnavailable);
            }
        };
        h.disclosure = disclosure;
        h.replace(circuit, name.to_string())?;
        h.sealed = true;
        self.exec(h, PhysCommand::Run)?;
        Ok(h.snapshot())
    }

    /// Component of the current document with this designator.
    pub fn find_designator(&self, designator: &str) -> Option<n7rt_circuit::CompId> {
        let h = lock(&self.shared);
        let circuit = h.core.as_ref().map_or(&h.last_good, |c| c.document().circuit());
        circuit.components().values().find(|c| !c.label.is_empty() && c.label == designator).map(|c| c.id)
    }

    pub fn save(&self, name: &str, overwrite: bool) -> Result<SnapshotDto, PhysError> {
        let mut guard = lock(&self.shared);
        let h = &mut *guard;
        if h.sealed || h.disclosure == Disclosure::Mission {
            return Err(mission_locked());
        }
        let Some(core) = h.core.as_mut() else { return Err(PhysError::EngineUnavailable) };
        if !files::valid_name(name) {
            return Err(PhysError::File { error: FileError::BadName });
        }
        let Some(dir) = self.shared.data_dir.as_deref() else { return Err(PhysError::File { error: FileError::NoStorage }) };
        let text = match catch_unwind(AssertUnwindSafe(|| files::renamed_text(core.document().circuit(), name))) {
            Ok(t) => t,
            Err(_) => {
                h.on_panic(None);
                return Err(PhysError::EngineUnavailable);
            }
        };
        files::save(dir, name, overwrite, &text).map_err(|error| PhysError::File { error })?;
        core.mark_saved();
        h.name = name.to_string();
        Ok(h.snapshot())
    }

    pub fn load(&self, name: &str) -> Result<SnapshotDto, PhysError> {
        let mut h = lock(&self.shared);
        h.unlocked()?;
        if h.core.is_none() {
            return Err(PhysError::EngineUnavailable);
        }
        if !files::valid_name(name) {
            return Err(PhysError::File { error: FileError::BadName });
        }
        let Some(dir) = self.shared.data_dir.as_deref() else { return Err(PhysError::File { error: FileError::NoStorage }) };
        let (stem, text) = files::read(dir, name).map_err(|error| PhysError::File { error })?;
        match catch_unwind(AssertUnwindSafe(|| persist::load(&text))) {
            Ok(Ok(circuit)) => {
                h.replace(circuit, stem)?;
                h.leave_bench();
                Ok(h.snapshot())
            }
            Ok(Err(violations)) => Err(PhysError::File { error: FileError::Invalid { violations } }),
            Err(_) => {
                h.on_panic(None);
                Err(PhysError::EngineUnavailable)
            }
        }
    }

    pub fn list(&self) -> Result<Vec<FileEntry>, PhysError> {
        if lock(&self.shared).core.is_none() {
            return Err(PhysError::EngineUnavailable);
        }
        let Some(dir) = self.shared.data_dir.as_deref() else { return Err(PhysError::File { error: FileError::NoStorage }) };
        files::list(dir).map_err(|error| PhysError::File { error })
    }

    pub fn demos(&self) -> Vec<DemoEntry> {
        catch_unwind(|| {
            n7rt_circuit::demos()
                .iter()
                .filter(|d| !d.hidden)
                .map(|d| DemoEntry { id: d.id.to_string(), title_fr: d.title_fr.to_string(), summary_fr: d.summary_fr.to_string() })
                .collect()
        })
        .unwrap_or_default()
    }

    pub fn load_demo(&self, id: &str) -> Result<SnapshotDto, PhysError> {
        let mut h = lock(&self.shared);
        h.unlocked()?;
        if h.core.is_none() {
            return Err(PhysError::EngineUnavailable);
        }
        let found = catch_unwind(|| n7rt_circuit::demos().iter().find(|d| d.id == id).map(|d| (d.json, d.id)));
        let Ok(Some((json, demo_id))) = found else { return Err(PhysError::File { error: FileError::NotFound }) };
        match catch_unwind(AssertUnwindSafe(|| persist::load(json))) {
            Ok(Ok(circuit)) => {
                let name = if circuit.name().is_empty() { demo_id.to_string() } else { circuit.name().to_string() };
                h.replace(circuit, name)?;
                h.leave_bench();
                Ok(h.snapshot())
            }
            Ok(Err(violations)) => Err(PhysError::File { error: FileError::Invalid { violations } }),
            Err(_) => {
                h.on_panic(None);
                Err(PhysError::EngineUnavailable)
            }
        }
    }

    /// Stops the `phys` thread. Idempotent.
    pub fn shutdown(&self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        {
            // Under the lock: the thread either sees the flag before waiting or gets the notification.
            let _g = lock(&self.shared);
            self.shared.wake.notify_all();
        }
        let handle = self.thread.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(t) = handle {
            let _ = t.join();
        }
    }
}

impl Drop for Lab {
    fn drop(&mut self) {
        self.shutdown();
    }
}

// --------------------------------------------------------------------------------------------- the phys thread

fn run(shared: &Shared) {
    let mut g = shared.host.lock().unwrap_or_else(|e| e.into_inner());
    loop {
        if shared.stop.load(Ordering::SeqCst) {
            return;
        }
        let wait = if g.autonomously_running() { Some(g.pace.until_next(Instant::now())) } else { None };
        g = match wait {
            Some(d) if d.is_zero() => g,
            Some(d) => match shared.wake.wait_timeout(g, d) {
                Ok((g, _)) => g,
                Err(e) => e.into_inner().0,
            },
            None => match shared.wake.wait(g) {
                Ok(g) => g,
                Err(e) => e.into_inner(),
            },
        };
        if shared.stop.load(Ordering::SeqCst) {
            return;
        }
        g = tick(shared, g);
    }
}

/// Computes the frames due (at most [`MAX_CATCH_UP`]), one lock per frame, then publishes if 50 ms passed.
fn tick<'a>(shared: &'a Shared, mut g: MutexGuard<'a, Host>) -> MutexGuard<'a, Host> {
    if !g.autonomously_running() {
        return g;
    }
    let now = Instant::now();
    let due = g.pace.due(now);
    if due > MAX_CATCH_UP {
        g.pace.fall_behind(due - MAX_CATCH_UP, now);
    } else {
        g.pace.lag_frames = 0;
    }
    for _ in 0..MAX_CATCH_UP {
        if !g.autonomously_running() || g.pace.due(Instant::now()) == 0 {
            break;
        }
        let h = &mut *g;
        let Some(core) = h.core.as_mut() else { break };
        let t0 = Instant::now();
        match catch_unwind(AssertUnwindSafe(|| core.step())) {
            Ok(()) => {
                let t1 = Instant::now();
                h.pace.done += 1;
                h.pace.record(t1 - t0, t1);
                h.after_success();
                if h.publish_after_frame {
                    h.publish_after_frame = false;
                    h.publish();
                }
                h.spill(&shared.data_dir);
            }
            Err(_) => {
                h.on_panic(None);
                break;
            }
        }
        drop(g);
        let until = Instant::now() + Duration::from_millis(2);
        while shared.waiting.load(Ordering::SeqCst) > 0 && Instant::now() < until {
            thread::yield_now();
        }
        g = shared.host.lock().unwrap_or_else(|e| e.into_inner());
    }
    if g.last_publish.elapsed() >= PUBLISH_PERIOD {
        g.publish();
    }
    g
}

#[cfg(test)]
#[path = "journal_retention_tests.rs"]
mod journal_retention_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::phys::bench::ref_pot_rc;
    use crate::phys::kernel::replay;
    use crate::phys::CircuitProbe;
    use n7rt_circuit::{Kind, Value};

    fn lab() -> Lab {
        Lab::start(PhysConfig { data_dir: None })
    }

    /// The handle on the circuit of contract §9.1, built through the document API.
    fn lab_ref_pot_rc() -> Lab {
        let lab = lab();
        lock(&lab.shared).replace(ref_pot_rc(), "ref-pot-rc".to_string()).expect("circuit loaded");
        lab
    }

    fn demo_comp(lab: &Lab, kind: Kind) -> n7rt_circuit::CompId {
        lab.snapshot().components.iter().find(|c| c.kind == kind).map(|c| c.id).expect("component of the circuit")
    }

    #[test]
    fn panics_restart_three_times_then_engine_unavailable() {
        let lab = lab_ref_pot_rc();
        let rv1 = demo_comp(&lab, Kind::Potentiometer);
        let rev0 = lab.snapshot().rev;
        for n in 1..=MAX_RESTARTS {
            lock(&lab.shared).inject_panics = 1;
            // A different command each time: a quarantined one is refused before reaching the core.
            let culprit = PhysCommand::Interact { interact: Interact::SetPot { comp: rv1, position: f64::from(n) / 10.0 } };
            assert_eq!(lab.command(culprit.clone()), Err(PhysError::EngineUnavailable), "panic {n}");
            // Quarantined: the same command is refused without being run again.
            assert_eq!(lab.command(culprit), Err(PhysError::EngineUnavailable));
            let v = lab.values(0).expect("values after a restart");
            assert_eq!(v.engine.restarts, n);
            assert_eq!(v.run, RunState::Stop);
            assert!(v.diagnostics.iter().any(|d| d.code == "engine_restarted" && d.severity == Severity::Error));
            assert!(lab.probe().diagnostics_contain("engine_restarted"));
            let s = lab.snapshot();
            assert!(s.rev > rev0, "revisions keep increasing across restarts");
            assert_eq!(s.components.len(), lab.snapshot().components.len());
            // The rebuilt core works.
            assert!(lab.command(PhysCommand::Run).is_ok());
            assert!(lab.command(PhysCommand::Stop).is_ok());
        }
        lock(&lab.shared).inject_panics = 1;
        assert_eq!(lab.command(PhysCommand::Run), Err(PhysError::EngineUnavailable), "fourth panic");
        assert_eq!(lab.command(PhysCommand::Stop), Err(PhysError::EngineUnavailable));
        assert_eq!(lab.preview(0, &Edit::Delete { components: vec![], wires: vec![] }), Err(PhysError::EngineUnavailable));
        assert_eq!(lab.load_demo("ref-pot-rc"), Err(PhysError::EngineUnavailable));
        assert_eq!(lab.list(), Err(PhysError::EngineUnavailable));
        assert_eq!(lab.save("x", false), Err(PhysError::EngineUnavailable));
        // The infallible calls keep answering with the last committed document.
        assert!(!lab.snapshot().components.is_empty());
        let _ = lab.probe();
        lab.shutdown();
    }

    #[test]
    fn restart_notice_disappears_with_a_new_circuit() {
        let lab = lab();
        lock(&lab.shared).inject_panics = 1;
        assert!(lab.command(PhysCommand::Run).is_err());
        assert!(lab.values(0).expect("values").diagnostics.iter().any(|d| d.code == "engine_restarted"));
        lab.new_circuit();
        assert!(!lab.values(0).expect("values").diagnostics.iter().any(|d| d.code == "engine_restarted"));
        assert_eq!(lab.values(0).expect("values").engine.restarts, 1, "the count stays");
    }

    fn restarts(lab: &Lab) -> u32 {
        lab.values(0).expect("values").engine.restarts
    }

    /// A panic in a command without payload quarantines nothing: after the restart RUN and STOP still work, and
    /// the command itself reaches the core again.
    #[test]
    fn a_panic_in_run_stop_reset_undo_or_redo_quarantines_nothing() {
        for culprit in [PhysCommand::Stop, PhysCommand::Run, PhysCommand::Reset, PhysCommand::Undo, PhysCommand::Redo] {
            let lab = lab_ref_pot_rc();
            lock(&lab.shared).inject_panics = 1;
            assert_eq!(lab.command(culprit.clone()), Err(PhysError::EngineUnavailable), "{culprit:?}");
            assert_eq!(restarts(&lab), 1);
            assert!(lab.command(PhysCommand::Run).is_ok(), "RUN after a panic in {culprit:?}");
            assert_eq!(lab.values(0).expect("values").run, RunState::Run);
            assert!(lab.command(PhysCommand::Stop).is_ok(), "STOP after a panic in {culprit:?}");
            assert_eq!(lab.values(0).expect("values").run, RunState::Stop);
            // Undo and redo are refused by the empty history, not by a quarantine.
            assert_ne!(lab.command(culprit.clone()), Err(PhysError::EngineUnavailable), "{culprit:?}");
            assert_eq!(restarts(&lab), 1);
            lab.shutdown();
        }
    }

    /// The restart moves the revision: the edit sent again with the fresh base_rev is still the quarantined one.
    #[test]
    fn a_quarantined_edit_or_interaction_stays_refused_whatever_its_revision() {
        let lab = lab_ref_pot_rc();
        let r1 = demo_comp(&lab, Kind::Resistor);
        let edit = Edit::SetParam { comp: r1, name: "resistance".into(), value: Value::Num(2200.0) };
        let rev0 = lab.snapshot().rev;
        lock(&lab.shared).inject_panics = 1;
        assert_eq!(lab.command(PhysCommand::Edit { base_rev: rev0, edit: edit.clone() }), Err(PhysError::EngineUnavailable));
        let rev1 = lab.snapshot().rev;
        assert!(rev1 > rev0, "the restart moves the revision");
        assert_eq!(lab.command(PhysCommand::Edit { base_rev: rev1, edit: edit.clone() }), Err(PhysError::EngineUnavailable));
        assert_eq!(restarts(&lab), 1, "refused before reaching the core");
        assert_eq!(lab.snapshot().rev, rev1, "nothing committed");
        let other = Edit::SetParam { comp: r1, name: "resistance".into(), value: Value::Num(4700.0) };
        assert!(lab.command(PhysCommand::Edit { base_rev: rev1, edit: other }).is_ok());

        let rv1 = demo_comp(&lab, Kind::Potentiometer);
        let set = Interact::SetPot { comp: rv1, position: 0.25 };
        lock(&lab.shared).inject_panics = 1;
        assert_eq!(lab.command(PhysCommand::Interact { interact: set.clone() }), Err(PhysError::EngineUnavailable));
        assert_eq!(lab.command(PhysCommand::Interact { interact: set }), Err(PhysError::EngineUnavailable));
        assert_eq!(restarts(&lab), 2);
        let moved = Interact::SetPot { comp: rv1, position: 0.5 };
        assert!(lab.command(PhysCommand::Interact { interact: moved }).is_ok());

        // A new document clears the quarantine.
        lock(&lab.shared).replace(ref_pot_rc(), "ref-pot-rc".to_string()).expect("circuit loaded");
        assert_eq!(demo_comp(&lab, Kind::Resistor), r1, "same payload on the reloaded circuit");
        let rev2 = lab.snapshot().rev;
        assert!(lab.command(PhysCommand::Edit { base_rev: rev2, edit }).is_ok());
        assert_eq!(restarts(&lab), 2);
        lab.shutdown();
    }

    /// Contract §8.2 (b): the real thread runs 10 s with scripted commands; its journal replayed without
    /// thread gives the same digest.
    #[test]
    fn thread_journal_replays_to_the_same_digest() {
        let lab = lab_ref_pot_rc();
        let initial = lock(&lab.shared).last_good.clone();
        let rv1 = demo_comp(&lab, Kind::Potentiometer);
        let s1 = demo_comp(&lab, Kind::Switch);
        let c1 = demo_comp(&lab, Kind::Capacitor);
        lab.command(PhysCommand::Run).expect("run");
        let t0 = Instant::now();
        let mut k = 0u32;
        while t0.elapsed() < Duration::from_secs(10) {
            let cmd = match k % 9 {
                0 | 3 | 6 => PhysCommand::Interact { interact: Interact::SetPot { comp: rv1, position: f64::from(k % 1000) / 1000.0 } },
                1 => PhysCommand::Interact { interact: Interact::SetSwitch { comp: s1, closed: k % 2 == 0 } },
                2 => PhysCommand::Edit {
                    base_rev: lab.snapshot().rev,
                    edit: Edit::SetParam {
                        comp: c1,
                        name: "capacitance".into(),
                        value: n7rt_circuit::Value::Num(if k % 4 == 0 { 4.7e-6 } else { 1e-5 }),
                    },
                },
                4 if k % 36 == 4 => PhysCommand::Reset,
                5 if k % 27 == 5 => PhysCommand::Stop,
                7 => PhysCommand::Run,
                _ => PhysCommand::Undo,
            };
            let _ = lab.command(cmd);
            k += 1;
            thread::sleep(Duration::from_millis(7));
        }
        lab.command(PhysCommand::Stop).expect("stop");
        let (journal, frames, live) = {
            let h = lock(&lab.shared);
            let core = h.core.as_ref().expect("alive");
            (core.journal().to_vec(), core.frame_index(), core.digest())
        };
        lab.shutdown();
        assert!(frames > 0 && journal.len() > 100, "the thread ran: {frames} frames, {} entries", journal.len());
        let replayed = replay(&initial, &journal, frames);
        assert_eq!(replayed.digest(), live);
        assert_eq!(replayed.journal(), &journal[..], "the replay journals the same entries");
    }

    #[test]
    fn values_are_published_after_commands_and_while_running() {
        let lab = lab();
        let v0 = lab.values(0).expect("first values");
        assert!(lab.values(v0.seq).is_none(), "nothing new");
        lab.command(PhysCommand::Run).expect("run");
        let v1 = lab.values(v0.seq).expect("published after the command");
        assert!(v1.seq > v0.seq);
        thread::sleep(Duration::from_millis(150));
        let v2 = lab.values(v1.seq).expect("published while running");
        assert!(v2.sim_time_ns > 0, "the thread advanced the simulated time");
        assert_eq!(v2.run, RunState::Run);
        lab.command(PhysCommand::Stop).expect("stop");
        let t = lab.values(0).expect("values").sim_time_ns;
        thread::sleep(Duration::from_millis(60));
        assert_eq!(lab.snapshot().sim_time_ns, t, "STOP freezes the time");
    }

    #[test]
    fn pacing_counts_the_lag_and_raises_the_slow_flag() {
        let mut p = Pace::new();
        let o = p.origin;
        let ms = |n: u64| o + Duration::from_millis(n);
        assert_eq!(p.due(ms(0)), 0);
        assert_eq!(p.until_next(ms(0)), Duration::from_millis(1));
        assert_eq!(p.due(ms(7)), 7);
        p.done = 7;
        assert_eq!(p.due(ms(7)), 0);
        // 45 frames due: 25 are dropped (re-sync), 20 remain for this wake.
        assert_eq!(p.due(ms(52)), 45);
        p.fall_behind(45 - MAX_CATCH_UP, ms(52));
        assert_eq!((p.lag_frames, p.due(ms(52))), (25, MAX_CATCH_UP));
        let s = p.stats(&SolverStats::default(), 0);
        assert!(s.slow && s.lag_frames == 25);
        assert_eq!(p.stats(&SolverStats::default(), 0).restarts, 0);
        p.record(Duration::from_micros(30), ms(52));
        p.record(Duration::from_micros(10), ms(53));
        assert_eq!(p.p95(), 30.0);
        // Older than one second: forgotten.
        p.record(Duration::from_micros(5), ms(2000));
        assert_eq!(p.p95(), 5.0);
    }

    #[test]
    fn a_starved_thread_drops_frames_instead_of_racing() {
        let lab = lab_ref_pot_rc();
        let name = lab.thread.lock().ok().and_then(|t| t.as_ref().and_then(|h| h.thread().name().map(str::to_string)));
        assert_eq!(name.as_deref(), Some("phys"), "the harness measures the CPU of this thread by name");
        lab.command(PhysCommand::Run).expect("run");
        thread::sleep(Duration::from_millis(50));
        let t0 = {
            let h = lock(&lab.shared);
            let t0 = h.core.as_ref().expect("core").frame().sim_time_ns();
            // 300 ms without the lock: the thread cannot compute anything.
            thread::sleep(Duration::from_millis(300));
            t0
        };
        thread::sleep(Duration::from_millis(60));
        let v = lab.values(0).expect("values");
        let advanced_ms = (v.sim_time_ns - t0) / 1_000_000;
        assert!(v.engine.slow, "slow flag raised");
        // At most 20 frames of catch-up, then real time again: far less than the 360 ms elapsed.
        assert!(advanced_ms > 0 && advanced_ms < 200, "simulated time advanced by {advanced_ms} ms");
        lab.shutdown();
    }

    #[test]
    fn running_values_are_published_at_most_every_50_ms() {
        let lab = lab_ref_pot_rc();
        lab.command(PhysCommand::Run).expect("run");
        let first = lab.values(0).expect("values").seq;
        let mut seqs = vec![first];
        let t0 = Instant::now();
        while t0.elapsed() < Duration::from_millis(500) {
            if let Some(v) = lab.values(*seqs.last().expect("seq")) {
                seqs.push(v.seq);
            }
            thread::sleep(Duration::from_millis(3));
        }
        let publications = seqs.len() - 1;
        assert!((2..=11).contains(&publications), "{publications} publications in 500 ms");
        lab.shutdown();
    }

    impl PhysFrame {
        fn diagnostics_contain(&self, code: &str) -> bool {
            self.inner.diagnostics.iter().any(|d| d.code == code)
        }
    }
}
