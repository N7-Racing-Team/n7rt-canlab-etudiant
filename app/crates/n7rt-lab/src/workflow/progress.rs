//! Progression of one student mission (private spec ch. 24 §1 and §4.6, adapted without server): the six steps
//! E0 Essai nominal, E1–E3 incidents, E4 qualification finale, E5 clôture; their displayed states; the attempts and
//! the rules between them. Pure logic: the caller gives the time (Unix seconds) and the attempt identifiers;
//! `store` persists the result.
//!
//! - Strict series: a step opens only when the previous one passes; a passed step never reopens; nothing is skipped.
//! - Unlimited attempts; at least 120 s between two verifications of the same step (300 s for E4), counted from the
//!   end of its last verdict that was not a cancellation. Cancellations do not count, unless the step already has
//!   six of them in the last hour: then they start the interval too.
//! - A failure changes nothing but the displayed state. A cancellation changes nothing at all.
//! - An attempt still running when the application stops is cancelled at the next opening.
//!
//! Without server the 48 h window, the freeze and the single-workstation lease do not exist, and the local clock can
//! be moved by the owner of the computer: these limits are documented with the extended D7 limit.

use serde::{Deserialize, Serialize};

pub const STEPS: usize = 6;
/// Steps that are verified (E0–E4); E5 is the closure.
pub const VERIFIED_STEPS: usize = 5;
pub const QUALIFICATION: usize = 4;
pub const CLOSURE: usize = 5;
pub const PROGRESS_FORMAT: &str = "n7rt-progress";
pub const PROGRESS_VERSION: u32 = 1;
pub const INTERVAL_S: u64 = 120;
pub const INTERVAL_QUALIFICATION_S: u64 = 300;
pub const CANCEL_CAP: usize = 6;
const HOUR_S: u64 = 3_600;
const MAX_ATTEMPTS: usize = 10_000;

/// Displayed state of a step: À FAIRE, EN COURS, VALIDÉ, ÉCHEC À CORRIGER.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepState {
    Todo,
    Open,
    Passed,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttemptVerdict {
    Pass,
    Fail,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepRecord {
    pub state: StepState,
    pub since_s: u64,
}

/// What identifies the evidence of an attempt (fiche version, circuit, student code, hidden vector used) and binds it
/// to its run: the opening of the mission it ran in, the board, and what crossed the link during the check.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub sheet_seq: u64,
    pub circuit_sha256: String,
    pub crc32: String,
    pub vector: usize,
    /// Random identifier of the opening of the mission (one per session).
    #[serde(default)]
    pub session: String,
    /// Unique identifier of the board at the start of the attempt.
    #[serde(default)]
    pub board_uid: String,
    /// SHA-256 of the frames sent and the ticks received during the check, once the run is recorded.
    #[serde(default)]
    pub bus_sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Running {
    pub id: String,
    pub step: usize,
    pub requested_s: u64,
    pub evidence: Evidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Attempt {
    pub id: String,
    pub step: usize,
    pub requested_s: u64,
    pub ended_s: u64,
    pub verdict: AttemptVerdict,
    /// Fiche part of the step conform; `None` when not judged (cancelled, or E4 which has no fiche part).
    pub diagnostic: Option<bool>,
    /// Real state verified (hidden nominal test for E0, repair for E1–E3); `None` when not judged.
    pub verified: Option<bool>,
    /// E4: the competence lines CP-1…CP-8 in order.
    pub competences: Option<Vec<bool>>,
    /// Public reason of a cancellation.
    pub cancel: Option<String>,
    pub evidence: Evidence,
}

/// The anomaly in force (invariant I-1: exactly one while an incident step E1–E3 is to be done, none otherwise). It is
/// activated in the same transition that opens its step and removed in the one that validates it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Active {
    /// Rank 1–3 of the incident (= its step).
    pub rank: usize,
    pub since_s: u64,
    /// Keyed tag of the wire object that carries a wire defect (never its identifier); `None` while the defect waits
    /// for a wire, or for another kind of anomaly.
    pub carrier: Option<String>,
}

/// « Reproduire l'anomalie » (R-11): the incident in force set again on the bench by the student, a trace of work for
/// the final fiche (never a new anomaly, never counted).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reproduction {
    pub rank: usize,
    pub at_s: u64,
}

/// The repair the application records for an incident (ch. 24 §3.4 `reparation`, never written by the student): the
/// board's CRC-32 of the student zone when the anomaly came and at the verification that validated the step, and the
/// changes of the circuit between those two moments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepairRecord {
    pub rank: usize,
    pub crc32_activation: String,
    #[serde(default)]
    pub crc32_validation: Option<String>,
    #[serde(default)]
    pub circuit: Vec<String>,
}

/// Who and what the progression belongs to. A different pack content never continues a progression.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Owner {
    pub mission: String,
    pub cohort: String,
    pub group: String,
    pub pack_id: String,
    pub workflow_sha256: String,
}

/// Bound of the recorded « Reproduire l'anomalie » actions (a file beyond it is refused).
pub const MAX_REPRODUCTIONS: usize = 1000;
/// Bound of the circuit changes of a recorded repair.
pub const MAX_CHANGES: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Progress {
    pub format: String,
    pub version: u32,
    pub owner: Owner,
    pub opened_s: u64,
    pub steps: Vec<StepRecord>,
    pub attempts: Vec<Attempt>,
    pub running: Option<Running>,
    /// Next pooled hidden vector of each verified step.
    pub cursor: Vec<usize>,
    /// The anomaly in force.
    #[serde(default)]
    pub active: Option<Active>,
    /// The final fiche, once exported (closure).
    #[serde(default)]
    pub exported: Option<Exported>,
    /// « Reproduire l'anomalie » actions, oldest first.
    #[serde(default)]
    pub reproductions: Vec<Reproduction>,
    /// The recorded repairs of the incidents (one per rank).
    #[serde(default)]
    pub repairs: Vec<RepairRecord>,
    /// Time the mission was open in the application, in seconds.
    #[serde(default)]
    pub active_s: u64,
}

/// Where and when the final fiche was exported (the folder name, under the student's Documents).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Exported {
    pub at_s: u64,
    pub folder: String,
}

/// Why a verification cannot start now (the view turns it into the public messages).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// The step is not the open one (not yet open, or already passed).
    NotOpen,
    /// The mission is closed.
    Closed,
    /// An attempt is already running.
    Running,
    /// The minimum interval since the last verdict of this step is not over.
    Interval { remaining_s: u64 },
    /// The nonce of an attempt already recorded.
    Replayed,
}

/// How an attempt ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Judged { pass: bool, diagnostic: Option<bool>, verified: Option<bool>, competences: Option<Vec<bool>> },
    Cancelled { reason: String },
}

/// What a finished attempt changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transition {
    pub step: usize,
    pub verdict: AttemptVerdict,
    /// The step opened by a pass (E1–E3 activate their incident, E4 the qualification).
    pub opened: Option<usize>,
    pub closed: bool,
}

impl Progress {
    /// T1: the first opening of the mission with this pack content. E0 is open, everything else to do.
    pub fn open(owner: Owner, now_s: u64) -> Progress {
        let mut steps = vec![StepRecord { state: StepState::Todo, since_s: now_s }; STEPS];
        steps[0].state = StepState::Open;
        Progress {
            format: PROGRESS_FORMAT.into(),
            version: PROGRESS_VERSION,
            owner,
            opened_s: now_s,
            steps,
            attempts: Vec::new(),
            running: None,
            cursor: vec![0; VERIFIED_STEPS],
            active: None,
            exported: None,
            reproductions: Vec::new(),
            repairs: Vec::new(),
            active_s: 0,
        }
    }

    /// The invariants a progression read from disk must satisfy (else it is refused, never repaired): passed steps
    /// form a prefix, followed by exactly one open or failed step and the steps to do; E5 is passed exactly when E4
    /// is; the running attempt, the attempts and the cursors refer to existing steps.
    pub fn validate(&self) -> Result<(), String> {
        let bad = |why: &str| Err(format!("progression invalide : {why}"));
        if self.format != PROGRESS_FORMAT || self.version != PROGRESS_VERSION {
            return bad("format");
        }
        if self.steps.len() != STEPS || self.cursor.len() != VERIFIED_STEPS || self.attempts.len() > MAX_ATTEMPTS {
            return bad("tailles");
        }
        let passed = self.steps.iter().take_while(|s| s.state == StepState::Passed).count();
        let closed = passed == STEPS;
        if !closed {
            if passed >= CLOSURE || !matches!(self.steps[passed].state, StepState::Open | StepState::Failed) {
                return bad("étape ouverte");
            }
            if self.steps[passed + 1..].iter().any(|s| s.state != StepState::Todo) {
                return bad("étapes suivantes");
            }
        }
        if self.attempts.iter().any(|a| a.step >= VERIFIED_STEPS) || self.running.as_ref().is_some_and(|r| r.step >= VERIFIED_STEPS) {
            return bad("tentatives");
        }
        // every attempt has its own random nonce: one seen twice is a replayed record
        let mut ids: Vec<&str> = self.attempts.iter().map(|a| a.id.as_str()).chain(self.running.as_ref().map(|r| r.id.as_str())).collect();
        ids.sort_unstable();
        if ids.windows(2).any(|w| w[0] == w[1]) {
            return bad("tentative rejouée");
        }
        if self.running.as_ref().is_some_and(|r| Some(r.step) != self.current()) {
            return bad("tentative en cours");
        }
        let incident = self.current().filter(|k| (1..=3).contains(k));
        if self.active.as_ref().map(|a| a.rank) != incident {
            return bad("anomalie en vigueur");
        }
        if self.reproductions.len() > MAX_REPRODUCTIONS || self.reproductions.iter().any(|r| !(1..=3).contains(&r.rank)) {
            return bad("reproductions");
        }
        let ranks: std::collections::BTreeSet<usize> = self.repairs.iter().map(|r| r.rank).collect();
        if ranks.len() != self.repairs.len() || self.repairs.iter().any(|r| !(1..=3).contains(&r.rank) || r.circuit.len() > MAX_CHANGES) {
            return bad("réparations");
        }
        Ok(())
    }

    /// The step to work on (E0–E4), `None` once closed.
    pub fn current(&self) -> Option<usize> {
        (0..VERIFIED_STEPS).find(|&k| matches!(self.steps[k].state, StepState::Open | StepState::Failed))
    }

    pub fn closed(&self) -> bool {
        self.steps[CLOSURE].state == StepState::Passed
    }

    fn interval_s(step: usize) -> u64 {
        if step == QUALIFICATION { INTERVAL_QUALIFICATION_S } else { INTERVAL_S }
    }

    /// Seconds before a new verification of `step` is allowed (0 = allowed).
    pub fn remaining_s(&self, step: usize, now_s: u64) -> u64 {
        let of_step = || self.attempts.iter().filter(move |a| a.step == step);
        // a time ahead of the clock (the clock was set back) counts neither in the hour nor for the interval: the
        // step is never blocked until the clock catches up
        let cancelled_last_hour = of_step()
            .filter(|a| a.verdict == AttemptVerdict::Cancelled && a.requested_s <= now_s && now_s - a.requested_s < HOUR_S)
            .count();
        let counted = cancelled_last_hour > CANCEL_CAP;
        let last = of_step().filter(|a| counted || a.verdict != AttemptVerdict::Cancelled).map(|a| a.ended_s).max();
        match last {
            Some(end) if end <= now_s => Self::interval_s(step).saturating_sub(now_s - end),
            _ => 0,
        }
    }

    pub fn check(&self, step: usize, now_s: u64) -> Result<(), Refusal> {
        if self.closed() {
            return Err(Refusal::Closed);
        }
        if self.running.is_some() {
            return Err(Refusal::Running);
        }
        if self.current() != Some(step) {
            return Err(Refusal::NotOpen);
        }
        match self.remaining_s(step, now_s) {
            0 => Ok(()),
            remaining_s => Err(Refusal::Interval { remaining_s }),
        }
    }

    /// T2: an attempt of `step` starts; it uses the next hidden vector of the step's pool of `pool` vectors.
    pub fn begin(&mut self, step: usize, now_s: u64, id: String, pool: usize, mut evidence: Evidence) -> Result<Running, Refusal> {
        self.check(step, now_s)?;
        if self.attempts.iter().any(|a| a.id == id) {
            return Err(Refusal::Replayed);
        }
        evidence.vector = if pool == 0 { 0 } else { self.cursor[step] % pool };
        self.cursor[step] = self.cursor[step].wrapping_add(1);
        let running = Running { id, step, requested_s: now_s, evidence };
        self.running = Some(running.clone());
        Ok(running)
    }

    /// T3/T4/T5: the running attempt ends. Err when no attempt runs.
    pub fn finish(&mut self, outcome: Outcome, now_s: u64) -> Result<Transition, String> {
        let r = self.running.take().ok_or_else(|| "aucune tentative en cours".to_owned())?;
        let (verdict, diagnostic, verified, competences, cancel) = match outcome {
            Outcome::Judged { pass, diagnostic, verified, competences } => {
                (if pass { AttemptVerdict::Pass } else { AttemptVerdict::Fail }, diagnostic, verified, competences, None)
            }
            Outcome::Cancelled { reason } => (AttemptVerdict::Cancelled, None, None, None, Some(reason)),
        };
        if self.attempts.len() >= MAX_ATTEMPTS {
            self.attempts.remove(0);
        }
        self.attempts.push(Attempt {
            id: r.id,
            step: r.step,
            requested_s: r.requested_s,
            ended_s: now_s,
            verdict,
            diagnostic,
            verified,
            competences,
            cancel,
            evidence: r.evidence,
        });
        let mut t = Transition { step: r.step, verdict, opened: None, closed: false };
        match verdict {
            AttemptVerdict::Pass => {
                self.steps[r.step] = StepRecord { state: StepState::Passed, since_s: now_s };
                let next = r.step + 1;
                if next == CLOSURE {
                    self.steps[CLOSURE] = StepRecord { state: StepState::Passed, since_s: now_s };
                    t.closed = true;
                } else {
                    self.steps[next] = StepRecord { state: StepState::Open, since_s: now_s };
                    t.opened = Some(next);
                }
                // I-1, I-5: the incident of the validated step is removed, the next one activated, in one commit
                self.active = (1..=3).contains(&next).then_some(Active { rank: next, since_s: now_s, carrier: None });
            }
            AttemptVerdict::Fail => {
                if self.steps[r.step].state != StepState::Failed {
                    self.steps[r.step] = StepRecord { state: StepState::Failed, since_s: now_s };
                }
            }
            AttemptVerdict::Cancelled => {}
        }
        Ok(t)
    }

    /// At opening: an attempt found running was interrupted (application closed, crash): it is cancelled.
    pub fn cancel_interrupted(&mut self, now_s: u64, reason: &str) -> Option<Transition> {
        self.running.as_ref()?;
        self.finish(Outcome::Cancelled { reason: reason.into() }, now_s).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owner() -> Owner {
        Owner { mission: "apps".into(), cohort: "c".into(), group: "G01".into(), pack_id: "p".into(), workflow_sha256: "0".repeat(64) }
    }

    fn evidence() -> Evidence {
        Evidence { sheet_seq: 1, circuit_sha256: "a".repeat(64), crc32: "01234567".into(), vector: 99, ..Evidence::default() }
    }

    fn judged(pass: bool) -> Outcome {
        Outcome::Judged { pass, diagnostic: Some(pass), verified: Some(pass), competences: None }
    }

    /// Runs one attempt of the current step at `t`, returns the transition.
    fn attempt(p: &mut Progress, t: u64, outcome: Outcome) -> Transition {
        let step = p.current().unwrap();
        p.begin(step, t, format!("id{t}"), 2, evidence()).unwrap();
        p.finish(outcome, t + 10).unwrap()
    }

    fn states(p: &Progress) -> Vec<StepState> {
        p.steps.iter().map(|s| s.state).collect()
    }

    #[test]
    fn opening_opens_e0_only() {
        let p = Progress::open(owner(), 1_000);
        use StepState::*;
        assert_eq!(states(&p), vec![Open, Todo, Todo, Todo, Todo, Todo]);
        assert_eq!(p.current(), Some(0));
        assert!(p.validate().is_ok());
    }

    #[test]
    fn passes_walk_the_strict_series_to_closure() {
        let mut p = Progress::open(owner(), 0);
        let mut t = 0;
        for k in 0..VERIFIED_STEPS {
            t += 1_000;
            let tr = attempt(&mut p, t, judged(true));
            assert_eq!(tr.step, k);
            assert_eq!(tr.opened, (k + 1 < CLOSURE).then_some(k + 1));
            assert_eq!(tr.closed, k == QUALIFICATION);
            assert!(p.validate().is_ok(), "after step {k}");
            // one anomaly in force exactly while an incident step is to be done (I-1)
            assert_eq!(p.active.as_ref().map(|a| a.rank), (k < 3).then_some(k + 1), "after step {k}");
        }
        assert!(p.closed());
        assert_eq!(p.current(), None);
        assert!(states(&p).iter().all(|s| *s == StepState::Passed));
        assert_eq!(p.check(4, t + 10_000), Err(Refusal::Closed));
    }

    #[test]
    fn a_failure_only_marks_the_step_and_keeps_it_current() {
        let mut p = Progress::open(owner(), 0);
        let tr = attempt(&mut p, 0, judged(false));
        assert_eq!((tr.verdict, tr.opened, tr.closed), (AttemptVerdict::Fail, None, false));
        assert_eq!(p.steps[0].state, StepState::Failed);
        assert_eq!(p.current(), Some(0));
        let tr = attempt(&mut p, 500, judged(true));
        assert_eq!(tr.opened, Some(1));
        assert_eq!(p.steps[0].state, StepState::Passed);
        assert_eq!(p.steps[1].state, StepState::Open);
    }

    #[test]
    fn only_the_open_step_can_be_verified_and_never_twice_at_once() {
        let mut p = Progress::open(owner(), 0);
        assert_eq!(p.check(1, 0), Err(Refusal::NotOpen));
        p.begin(0, 0, "a".into(), 1, evidence()).unwrap();
        assert_eq!(p.check(0, 0), Err(Refusal::Running));
        assert_eq!(p.begin(0, 0, "b".into(), 1, evidence()), Err(Refusal::Running));
        p.finish(judged(true), 5).unwrap();
        assert_eq!(p.check(0, 1_000), Err(Refusal::NotOpen), "a passed step never reopens");
        assert!(p.finish(judged(true), 6).is_err(), "nothing runs");
        // a recorded nonce never starts again, and a file holding it twice is refused
        assert_eq!(p.begin(1, 500, "a".into(), 1, evidence()), Err(Refusal::Replayed));
        let mut twice = p.clone();
        let again = twice.attempts[0].clone();
        twice.attempts.push(again);
        assert!(twice.validate().is_err(), "a replayed attempt record");
    }

    #[test]
    fn interval_counts_from_the_last_judged_verdict_of_the_same_step() {
        let mut p = Progress::open(owner(), 0);
        attempt(&mut p, 100, judged(false)); // ends at 110
        assert_eq!(p.check(0, 110), Err(Refusal::Interval { remaining_s: 120 }));
        assert_eq!(p.check(0, 229), Err(Refusal::Interval { remaining_s: 1 }));
        assert_eq!(p.check(0, 230), Ok(()));
        attempt(&mut p, 230, judged(true)); // E0 passed at 240, E1 open
        assert_eq!(p.check(1, 240), Ok(()), "no interval between different steps");
    }

    #[test]
    fn the_qualification_interval_is_longer() {
        let mut p = Progress::open(owner(), 0);
        for t in [0, 1_000, 2_000, 3_000] {
            attempt(&mut p, t, judged(true));
        }
        attempt(&mut p, 4_000, Outcome::Judged { pass: false, diagnostic: None, verified: None, competences: Some(vec![true; 8]) });
        assert_eq!(p.remaining_s(4, 4_010), 300);
        assert_eq!(p.check(4, 4_310), Ok(()));
    }

    #[test]
    fn cancellations_are_free_until_six_in_an_hour() {
        let mut p = Progress::open(owner(), 0);
        let cancel = || Outcome::Cancelled { reason: "carte".into() };
        for i in 0..CANCEL_CAP as u64 {
            let tr = attempt(&mut p, i * 20, cancel());
            assert_eq!(tr.verdict, AttemptVerdict::Cancelled);
            assert_eq!(p.steps[0].state, StepState::Open, "a cancellation changes nothing");
            assert_eq!(p.remaining_s(0, i * 20 + 10), 0);
        }
        attempt(&mut p, 200, cancel()); // seventh in the hour, ends at 210
        assert_eq!(p.remaining_s(0, 210), 120);
        assert_eq!(p.remaining_s(0, 330), 0);
        // an hour after the first ones, they no longer count
        assert_eq!(p.remaining_s(0, 3_700), 0);
    }

    #[test]
    fn interrupted_attempt_is_cancelled_at_next_opening() {
        let mut p = Progress::open(owner(), 0);
        p.begin(0, 50, "x".into(), 3, evidence()).unwrap();
        let tr = p.cancel_interrupted(9_000, "application fermée").unwrap();
        assert_eq!(tr.verdict, AttemptVerdict::Cancelled);
        assert!(p.running.is_none());
        assert_eq!(p.attempts[0].cancel.as_deref(), Some("application fermée"));
        assert!(p.cancel_interrupted(9_001, "x").is_none());
    }

    #[test]
    fn pooled_vectors_rotate_per_step() {
        let mut p = Progress::open(owner(), 0);
        let mut used = Vec::new();
        for t in [0, 200, 400] {
            let r = p.begin(0, t, format!("{t}"), 2, evidence()).unwrap();
            used.push(r.evidence.vector);
            p.finish(judged(false), t + 1).unwrap();
        }
        assert_eq!(used, vec![0, 1, 0]);
        assert_eq!(p.attempts.iter().map(|a| a.evidence.vector).collect::<Vec<_>>(), vec![0, 1, 0]);
    }

    #[test]
    fn invalid_progressions_are_refused() {
        let good = Progress::open(owner(), 0);
        let mut cases = Vec::new();
        let mut p = good.clone();
        p.steps[2].state = StepState::Open;
        cases.push(("two open steps", p));
        let mut p = good.clone();
        p.steps[0].state = StepState::Todo;
        cases.push(("nothing open", p));
        let mut p = good.clone();
        p.steps[5].state = StepState::Passed;
        cases.push(("closure without the rest", p));
        let mut p = good.clone();
        p.steps.pop();
        cases.push(("five steps", p));
        let mut p = good.clone();
        p.version = 2;
        cases.push(("version", p));
        let mut p = good.clone();
        p.running = Some(Running { id: "r".into(), step: 3, requested_s: 0, evidence: evidence() });
        cases.push(("running on another step", p));
        let mut p = good.clone();
        p.active = Some(Active { rank: 1, since_s: 0, carrier: None });
        cases.push(("anomaly during E0", p));
        let mut p = good.clone();
        attempt(&mut p, 10, judged(true));
        p.active = None;
        cases.push(("incident step without its anomaly", p));
        let mut p = good.clone();
        attempt(&mut p, 10, judged(true));
        p.active.as_mut().unwrap().rank = 2;
        cases.push(("anomaly of another step", p));
        let mut p = good;
        p.steps[0].state = StepState::Passed;
        p.steps[1].state = StepState::Passed;
        p.steps[2].state = StepState::Passed;
        p.steps[3].state = StepState::Passed;
        p.steps[4].state = StepState::Passed;
        cases.push(("E4 passed without closure", p));
        for (why, p) in cases {
            assert!(p.validate().is_err(), "{why}");
        }
    }

    #[test]
    fn failures_and_cancellations_keep_the_anomaly() {
        let mut p = Progress::open(owner(), 0);
        attempt(&mut p, 1_000, judged(true));
        let active = p.active.clone().expect("incident 1 activated with E1");
        attempt(&mut p, 2_000, judged(false));
        attempt(&mut p, 3_000, Outcome::Cancelled { reason: "x".into() });
        assert_eq!(p.active, Some(active), "I-3: unchanged by a failure or a cancellation");
        attempt(&mut p, 4_000, judged(true));
        assert_eq!(p.active.as_ref().map(|a| (a.rank, a.since_s)), Some((2, 4_010)), "incident 2 replaces incident 1");
    }

    #[test]
    fn serde_round_trip_is_strict() {
        let mut p = Progress::open(owner(), 7);
        attempt(&mut p, 10, judged(false));
        let text = serde_json::to_string(&p).unwrap();
        assert_eq!(serde_json::from_str::<Progress>(&text).unwrap(), p);
        let mut v: serde_json::Value = serde_json::from_str(&text).unwrap();
        v["extra"] = serde_json::json!(1);
        assert!(serde_json::from_value::<Progress>(v).is_err());
    }
}
