//! Link engine against the in-memory board (no hardware). Real-board validation is the
//! automated scenario of the application (`stm32lab app-test`).

use std::sync::atomic::Ordering;
use std::thread;
use std::time::{Duration, Instant};

use n7rt_lab::e2e::{check_oracle, Verification};
use n7rt_lab::fake::FakeHandle;
use n7rt_lab::model::{LinkState, UiBatch};
use n7rt_lab::sim::Stimuli;
use n7rt_lab::{Command, Lab, LabConfig};

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

fn start(fake: &FakeHandle) -> Lab {
    let cfg = LabConfig { record: true, stimuli: Stimuli { rate_hz: 0, ..Stimuli::default() }, ..LabConfig::default() };
    Lab::start(cfg, fake.connector())
}

fn ready(s: &UiBatch) -> bool {
    s.status.state == LinkState::Ready
}

fn active(s: &UiBatch) -> bool {
    s.status.session.as_ref().is_some_and(|x| x.phase == "active")
}

#[test]
fn handshake_session_frames_and_stop() {
    let fake = FakeHandle::new();
    let lab = start(&fake);
    let s = wait_for(&lab, "ready", 3000, ready);
    let board = s.status.board.expect("board identity");
    assert!(board.runtime_verified && board.compatible, "{board:?}");
    assert!(s.status.can_start);

    lab.command(Command::StartSession { tick_period_ms: 10 });
    wait_for(&lab, "active session", 3000, active);
    lab.command(Command::SetStimuli { stimuli: Stimuli { rate_hz: 400, pot: 42, ..Stimuli::default() }, rev: 1 });
    thread::sleep(Duration::from_millis(800));
    lab.command(Command::SetStimuli { stimuli: Stimuli { rate_hz: 0, ..Stimuli::default() }, rev: 2 });
    lab.command(Command::SendOne);
    thread::sleep(Duration::from_millis(200));
    lab.command(Command::StopSession);
    let s = wait_for(&lab, "session closed", 3000, |s| s.status.session.is_none() && ready(s));

    let st = &s.stats;
    assert!(st.frames_sent >= 200, "{st:?}");
    assert_eq!(st.results, st.frames_sent, "{st:?}");
    assert_eq!((st.lost, st.nacks, st.aborted), (0, 0, 0), "{st:?}");
    assert!(st.tick_results > 50 && st.sim_time_sent > 50, "{st:?}");
    assert!(st.in_flight_max <= 32);
    let record = lab.record();
    assert_eq!(record.len() as u64, st.frames_sent);
    assert!(record.iter().all(|r| r.outcome == "result"));
    assert!(record.windows(2).all(|w| w[0].frame_seq + 1 == w[1].frame_seq), "frames in order, no gap");
    let mut v = Verification::default();
    check_oracle(&record, &mut v);
    assert!(v.oracle_mismatches.is_empty() && v.oracle_checked == record.len(), "{v:?}");
    assert!(lab.log_record().iter().any(|l| l.text == "student_decoder ready"));
}

#[test]
fn repeated_connect_to_same_board_preserves_active_session() {
    let fake = FakeHandle::new();
    let lab = start(&fake);
    wait_for(&lab, "ready", 3000, ready);
    lab.command(Command::StartSession { tick_period_ms: 10 });
    let first = wait_for(&lab, "active", 3000, active).status.session.unwrap().id;
    lab.command(Command::Connect { port: None, serial: Some("FAKE0001".into()) });
    lab.command(Command::SendOne);
    let result = wait_for(&lab, "result after repeated connect", 3000, |s| s.stats.results > 0);
    assert!(active(&result), "{:?}", result.status);
    assert_eq!(result.status.session.unwrap().id, first);
    assert_eq!(result.stats.aborted, 0);
    assert!(result.errors.is_empty(), "{:?}", result.errors);
}

#[test]
fn stale_bytes_at_open_are_not_errors() {
    let fake = FakeHandle::new();
    fake.board.lock().unwrap().inject_stale();
    let lab = start(&fake);
    let s = wait_for(&lab, "ready", 3000, ready);
    assert!(s.status.board.as_ref().unwrap().runtime_verified, "the foreign HELLO_ACK must not be taken");
    thread::sleep(Duration::from_millis(100));
    let s = lab.drain().unwrap();
    assert!(s.stats.rx_framing.is_empty() && s.stats.board_seq_gaps == 0, "{:?}", s.stats);
    assert!(s.stats.pre_sync_discarded >= 2, "{:?}", s.stats);
    assert!(s.errors.is_empty(), "{:?}", s.errors);
}

#[test]
fn reboot_resumes_session() {
    let fake = FakeHandle::new();
    let lab = start(&fake);
    wait_for(&lab, "ready", 3000, ready);
    lab.command(Command::StartSession { tick_period_ms: 10 });
    let first = wait_for(&lab, "active", 3000, active).status.session.unwrap().id;
    lab.command(Command::ResetBoard);
    let s = wait_for(&lab, "resumed session", 4000, |s| active(s) && s.status.session.as_ref().unwrap().id != first);
    assert!(s.status.board.unwrap().runtime_verified);
}

#[test]
fn busy_port_is_reported_then_recovers() {
    let fake = FakeHandle::new();
    fake.busy.store(true, Ordering::SeqCst);
    let lab = start(&fake);
    let s = wait_for(&lab, "port busy", 3000, |s| s.status.state == LinkState::PortBusy);
    assert!(s.status.message.contains("FAKE0") && s.status.message.contains("autre application"), "{}", s.status.message);
    fake.busy.store(false, Ordering::SeqCst);
    wait_for(&lab, "ready after release", 4000, ready);
}

#[test]
fn halted_core_stalls_then_resumes_without_loss() {
    let fake = FakeHandle::new();
    let lab = start(&fake);
    wait_for(&lab, "ready", 3000, ready);
    lab.command(Command::StartSession { tick_period_ms: 10 });
    wait_for(&lab, "active", 3000, active);
    lab.command(Command::SetStimuli { stimuli: Stimuli { rate_hz: 200, ..Stimuli::default() }, rev: 1 });
    thread::sleep(Duration::from_millis(300));
    fake.board.lock().unwrap().halted = true;
    wait_for(&lab, "stalled", 3000, |s| s.status.stalled);
    thread::sleep(Duration::from_millis(500));
    fake.board.lock().unwrap().halted = false;
    wait_for(&lab, "resumed", 3000, |s| !s.status.stalled);
    thread::sleep(Duration::from_millis(400));
    lab.command(Command::StopSession);
    let s = wait_for(&lab, "closed", 3000, |s| s.status.session.is_none());
    assert_eq!(s.stats.lost, 0, "{:?}", s.stats);
    assert_eq!(s.stats.results, s.stats.frames_sent, "{:?}", s.stats);
}

#[test]
fn frames_lost_during_a_halt_free_their_credits() {
    let fake = FakeHandle::new();
    fake.board.lock().unwrap().overrun_when_halted = true;
    let lab = start(&fake);
    wait_for(&lab, "ready", 3000, ready);
    lab.command(Command::StartSession { tick_period_ms: 10 });
    wait_for(&lab, "active", 3000, active);
    lab.command(Command::SetStimuli { stimuli: Stimuli { rate_hz: 200, ..Stimuli::default() }, rev: 1 });
    thread::sleep(Duration::from_millis(300));
    fake.board.lock().unwrap().halted = true;
    wait_for(&lab, "stalled", 3000, |s| s.status.stalled);
    fake.board.lock().unwrap().halted = false;
    let resumed = wait_for(&lab, "resumed", 3000, |s| !s.status.stalled).stats.results;
    let s = wait_for(&lab, "traffic flows again", 3000, |s| s.stats.results > resumed + 100);
    assert!(s.stats.lost > 0, "frames sent into the overrun are counted: {:?}", s.stats);
    lab.command(Command::SetStimuli { stimuli: Stimuli { rate_hz: 0, ..Stimuli::default() }, rev: 2 });
    let s = wait_for(&lab, "nothing in flight", 3000, |s| s.stats.in_flight == 0);
    assert_eq!(s.stats.results + s.stats.lost, s.stats.frames_sent, "{:?}", s.stats);
}

#[test]
fn uncertain_short_write_resets_instead_of_duplicating() {
    let fake = FakeHandle::new();
    let lab = start(&fake);
    wait_for(&lab, "ready", 3000, ready);
    lab.command(Command::StartSession { tick_period_ms: 10 });
    wait_for(&lab, "active", 3000, active);
    lab.command(Command::SetStimuli { stimuli: Stimuli { rate_hz: 200, ..Stimuli::default() }, rev: 1 });
    thread::sleep(Duration::from_millis(200));
    fake.short_write_next.store(true, Ordering::SeqCst);
    // The link is reset and the session resumed on a fresh connection.
    thread::sleep(Duration::from_millis(300));
    wait_for(&lab, "session resumed", 4000, active);
    thread::sleep(Duration::from_millis(300));
    assert_eq!(fake.duplicates.load(Ordering::SeqCst), 0, "a byte was sent twice");
    let s = lab.drain().unwrap();
    assert!(!s.errors.iter().any(|e| e.text.contains("E_TIME")), "{:?}", s.errors);
}

#[test]
fn unplug_then_replug_reconnects() {
    let fake = FakeHandle::new();
    let lab = start(&fake);
    wait_for(&lab, "ready", 3000, ready);
    fake.present.store(false, Ordering::SeqCst);
    wait_for(&lab, "lost", 3000, |s| s.status.state == LinkState::Lost);
    fake.present.store(true, Ordering::SeqCst);
    let t = Instant::now();
    wait_for(&lab, "ready again", 4000, ready);
    assert!(t.elapsed() < Duration::from_secs(3), "reconnection took {:?}", t.elapsed());
}

#[test]
fn lost_answers_are_counted_not_hidden() {
    let fake = FakeHandle::new();
    fake.board.lock().unwrap().drop_every = Some(10);
    let lab = start(&fake);
    wait_for(&lab, "ready", 3000, ready);
    lab.command(Command::StartSession { tick_period_ms: 10 });
    wait_for(&lab, "active", 3000, active);
    lab.command(Command::SetStimuli { stimuli: Stimuli { rate_hz: 200, ..Stimuli::default() }, rev: 1 });
    thread::sleep(Duration::from_millis(600));
    lab.command(Command::SetStimuli { stimuli: Stimuli { rate_hz: 0, ..Stimuli::default() }, rev: 2 });
    thread::sleep(Duration::from_millis(700));
    let s = lab.snapshot();
    assert!(s.stats.lost > 0, "{:?}", s.stats);
    assert_eq!(s.stats.lost + s.stats.results, s.stats.frames_sent, "{:?}", s.stats);
}
