//! Virtual CAN generator (`sim` thread, 1 ms logical step on the monotonic clock).
//!
//! It plays one virtual ECU whose frames the student's node receives. The stimuli are the
//! ECU's inputs (a potentiometer, a button) and its frame format, identifier, DLC and rate.
//! The way an input lands in the frame bytes is a fixed, public **training** encoding (byte 0 =
//! potentiometer, byte 1 = button, byte 2 = rolling counter, then the byte index); it contains no
//! mission content, and the UI never shows it: the UI only shows raw frames and what the
//! student's code published.
//!
//! Output: a FIFO of bus items (frames and `SIM_TIME` tick boundaries) with non-decreasing
//! `t_us`, consumed by the link thread. Backpressure: when the FIFO holds `HIGH_WATER` items
//! (board stalled at a breakpoint, credits exhausted), the bus clock pauses instead of dropping
//! frames, and the pause is counted (`paused_ms`).
//!
//! Bus source. Without a mission the thread plays the training ECU above. With a mission open it
//! plays a [`BusSource`] instead (the mission vehicle, `crate::mission`), created at each session
//! start (t = 0) by the installed [`SourceFactory`]: at each step the source advances to the new bus
//! time and its frames (classic, flags 0) are interleaved with the ticks in non-decreasing `t_us`,
//! ticks first at equal timestamps (N7RT-Link v0 §9.4). A source change takes effect at the next
//! session start.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{SyncSender, TrySendError};
use std::sync::{Arc, Condvar, Mutex, TryLockError};
use std::thread;
use std::time::{Duration, Instant};

use n7rt_link::{can_data_length, CAN_BRS, CAN_EXT, CAN_FD, CAN_RTR, EXT_ID_MAX, STD_ID_MAX};
use n7rt_vehicle::BusFrame;
use serde::{Deserialize, Serialize};

/// A bus source played by the sim thread (mission vehicle). `advance` returns the frames emitted in
/// (previous t, `t_us`], sorted by `t_us`; it is called with a non-decreasing `t_us`, from 0.
pub trait BusSource: Send {
    fn advance(&mut self, t_us: u64) -> Vec<BusFrame>;
}

/// Creates the source of each session, at t = 0.
pub trait SourceFactory: Send {
    fn start(&mut self) -> Box<dyn BusSource>;
}

/// What the sim thread plays from the next session start on.
#[derive(Default)]
pub enum SourceKind {
    /// The training ECU (stimuli), the historical cockpit.
    #[default]
    Training,
    /// A mission source.
    Mission(Box<dyn SourceFactory>),
}

impl std::fmt::Debug for SourceKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            SourceKind::Training => "Training",
            SourceKind::Mission(_) => "Mission",
        })
    }
}

/// Interleaves the frames of a source (sorted by time) with the tick boundaries up to `t_us`: every tick
/// `k·P ≤ frame.t_us` goes before the frame, so ticks precede frames of the same timestamp (N7RT-Link v0
/// §9.4). `last` is the last emitted time: an out-of-order frame is clamped to it, so `t_us` never decreases.
pub fn interleave(
    frames: Vec<BusFrame>,
    t_us: u64,
    period: u64,
    next_tick: &mut u64,
    last: &mut u64,
    stim_rev: u64,
    out: &mut dyn FnMut(BusKind),
) {
    fn ticks_to(t: u64, period: u64, next_tick: &mut u64, last: &mut u64, out: &mut dyn FnMut(BusKind)) {
        while period > 0 && *next_tick <= t {
            *last = (*last).max(*next_tick);
            out(BusKind::Time(*next_tick));
            *next_tick += period;
        }
    }
    for f in frames {
        let t = f.t_us.max(*last);
        ticks_to(t, period, next_tick, last, out);
        *last = t;
        // the data length follows the flags (classic ≤ 8, FD up to 64, RTR none)
        let flags = f.flags & n7rt_link::CAN_FLAGS_MASK;
        let dlc = f.dlc.min(15);
        let mut data = f.data;
        data.resize(can_data_length(dlc, flags).unwrap_or(0), 0);
        out(BusKind::Frame(GenFrame { t_us: t, can_id: f.id, flags, dlc, data, stim_rev }));
    }
    ticks_to(t_us, period, next_tick, last, out);
}

pub const QUEUE_CAPACITY: usize = 1024;
pub const HIGH_WATER: usize = 512;
pub const MAX_RATE_HZ: u32 = 1000;
/// Largest clock step taken at once (after a scheduling hiccup): bounds bursts.
const MAX_STEP: Duration = Duration::from_millis(50);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FrameFormat {
    /// CAN 2.0A, 11-bit identifier.
    Classic,
    /// CAN 2.0B, 29-bit identifier.
    Extended,
    Fd,
    FdBrs,
    /// Remote frame (classic, no data).
    Remote,
}

impl FrameFormat {
    pub fn flags(self) -> u8 {
        match self {
            FrameFormat::Classic => 0,
            FrameFormat::Extended => CAN_EXT,
            FrameFormat::Fd => CAN_FD,
            FrameFormat::FdBrs => CAN_FD | CAN_BRS,
            FrameFormat::Remote => CAN_RTR,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stimuli {
    /// Virtual potentiometer, 0..=255.
    pub pot: u8,
    pub button: bool,
    pub format: FrameFormat,
    pub can_id: u32,
    pub dlc: u8,
    /// Periodic frames per second, 0 = only on demand.
    pub rate_hz: u32,
}

impl Default for Stimuli {
    fn default() -> Self {
        Self { pot: 128, button: false, format: FrameFormat::Classic, can_id: 0x123, dlc: 8, rate_hz: 10 }
    }
}

impl Stimuli {
    pub fn validate(&self) -> Result<(), String> {
        let max = if self.format == FrameFormat::Extended { EXT_ID_MAX } else { STD_ID_MAX };
        if self.can_id > max {
            return Err(format!("identifiant 0x{:X} hors limites (max 0x{max:X} pour ce format)", self.can_id));
        }
        if self.dlc > 15 {
            return Err(format!("DLC {} hors limites (0 à 15)", self.dlc));
        }
        if self.rate_hz > MAX_RATE_HZ {
            return Err(format!("cadence {} trames/s hors limites (max {MAX_RATE_HZ})", self.rate_hz));
        }
        Ok(())
    }

    /// The training encoding: identical for every run, documented in the module header.
    pub fn frame(&self, counter: u8) -> (u32, u8, u8, Vec<u8>) {
        let flags = self.format.flags();
        let len = can_data_length(self.dlc, flags).unwrap_or(0);
        let data = (0..len)
            .map(|i| match i {
                0 => self.pot,
                1 => self.button as u8,
                2 => counter,
                _ => i as u8,
            })
            .collect();
        (self.can_id, flags, self.dlc, data)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct GenFrame {
    pub t_us: u64,
    pub can_id: u32,
    pub flags: u8,
    pub dlc: u8,
    pub data: Vec<u8>,
    /// Revision of the stimuli the frame was generated from (UI latency measurement).
    pub stim_rev: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BusKind {
    Frame(GenFrame),
    /// Tick boundary, sent as `SIM_TIME` (spec section 9.4: a conforming host sends one per tick).
    Time(u64),
}

#[derive(Debug, Clone, PartialEq)]
pub struct BusItem {
    /// Session generation: items of an older session are discarded by the link.
    pub epoch: u64,
    pub kind: BusKind,
}

#[derive(Debug)]
struct Ctl {
    epoch: u64,
    running: bool,
    tick_period_us: u64,
    stimuli: Stimuli,
    stim_rev: u64,
    one_shots: u32,
    shutdown: bool,
    source: SourceKind,
    /// A document transition inhibits starts until a replacement factory is installed.
    source_quiesced: bool,
}

#[derive(Debug, Default)]
struct SourceActivity {
    active: Mutex<bool>,
    idle: Condvar,
}

/// The idle acknowledgement follows the source's destructor, including release of its physical clock.
struct ActiveSource {
    source: Option<Box<dyn BusSource>>,
    activity: Arc<SourceActivity>,
}

impl ActiveSource {
    fn new(source: Box<dyn BusSource>, activity: Arc<SourceActivity>) -> Self {
        *activity.active.lock().unwrap() = true;
        Self { source: Some(source), activity }
    }

    fn advance(&mut self, t_us: u64) -> Vec<BusFrame> {
        self.source.as_mut().expect("active source").advance(t_us)
    }
}

impl Drop for ActiveSource {
    fn drop(&mut self) {
        drop(self.source.take());
        *self.activity.active.lock().unwrap() = false;
        self.activity.idle.notify_all();
    }
}

/// State shared by the UI commands, the link thread and the sim thread.
#[derive(Debug)]
pub struct SimShared {
    ctl: Mutex<Ctl>,
    cv: Condvar,
    source_activity: Arc<SourceActivity>,
    pub depth: AtomicUsize,
    pub high_water: AtomicUsize,
    pub paused_us: AtomicU64,
    pub generated: AtomicU64,
    pub dropped: AtomicU64,
    pub bus_time_us: AtomicU64,
    pub max_step_us: AtomicU64,
    pub panics: AtomicU64,
    /// Mission sources that failed (panic at creation or while advancing): the bus then carries ticks only.
    pub source_errors: AtomicU64,
    stopped: AtomicBool,
}

impl SimShared {
    pub fn new(stimuli: Stimuli) -> Arc<Self> {
        Arc::new(Self {
            ctl: Mutex::new(Ctl {
                epoch: 0,
                running: false,
                tick_period_us: 0,
                stimuli,
                stim_rev: 0,
                one_shots: 0,
                shutdown: false,
                source: SourceKind::Training,
                source_quiesced: false,
            }),
            cv: Condvar::new(),
            source_activity: Arc::new(SourceActivity::default()),
            depth: AtomicUsize::new(0),
            high_water: AtomicUsize::new(0),
            paused_us: AtomicU64::new(0),
            generated: AtomicU64::new(0),
            dropped: AtomicU64::new(0),
            bus_time_us: AtomicU64::new(0),
            max_step_us: AtomicU64::new(0),
            panics: AtomicU64::new(0),
            source_errors: AtomicU64::new(0),
            stopped: AtomicBool::new(true),
        })
    }

    /// Installs the source played from the next session start on.
    pub fn set_source(&self, source: SourceKind) {
        let mut c = self.ctl.lock().unwrap();
        c.source = source;
        c.source_quiesced = false;
    }

    /// True when a mission source is installed.
    pub fn mission_source(&self) -> bool {
        matches!(self.ctl.lock().unwrap().source, SourceKind::Mission(_))
    }

    /// Starts a session's bus at t = 0.
    pub fn start(&self, epoch: u64, tick_period_ms: u16) {
        let mut c = self.ctl.lock().unwrap();
        if c.source_quiesced { return; }
        c.epoch = epoch;
        c.running = true;
        c.tick_period_us = tick_period_ms as u64 * 1000;
        c.one_shots = 0;
        for a in [&self.paused_us, &self.generated, &self.dropped, &self.bus_time_us, &self.max_step_us] {
            a.store(0, Ordering::Relaxed);
        }
        self.high_water.store(0, Ordering::Relaxed);
        self.stopped.store(false, Ordering::Relaxed);
        self.cv.notify_all();
    }

    pub fn stop(&self) {
        let mut c = self.ctl.lock().unwrap();
        c.running = false;
        c.one_shots = 0;
        self.stopped.store(true, Ordering::Relaxed);
    }

    /// Stop and wait for the actual source drop before replacing its physical document.
    /// Waiting uses a separate mutex: a sim iteration may need `ctl` in order to drop its source.
    /// Once the stop request is accepted, starts remain inhibited until `set_source`,
    /// even if the source-drop acknowledgement times out.
    pub fn stop_and_wait_source(&self, timeout: Duration) -> Result<(), String> {
        let deadline = Instant::now().checked_add(timeout).ok_or("invalid source-stop timeout")?;
        loop {
            match self.ctl.try_lock() {
                Ok(mut c) => {
                    c.source_quiesced = true;
                    c.running = false;
                    c.one_shots = 0;
                    self.stopped.store(true, Ordering::Relaxed);
                    self.cv.notify_all();
                    break;
                }
                Err(TryLockError::Poisoned(_)) => return Err("bus source control unavailable".into()),
                Err(TryLockError::WouldBlock) => {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() { return Err("bus source did not stop before the deadline".into()); }
                    thread::sleep(remaining.min(Duration::from_millis(1)));
                }
            }
        }
        let mut active = self.source_activity.active.lock().map_err(|_| "bus source activity unavailable")?;
        while *active {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() { return Err("bus source did not release its physical clock before the deadline".into()); }
            active = self.source_activity.idle.wait_timeout(active, remaining)
                .map_err(|_| "bus source activity unavailable")?.0;
        }
        Ok(())
    }

    pub fn set_stimuli(&self, stimuli: Stimuli, rev: u64) {
        let mut c = self.ctl.lock().unwrap();
        c.stimuli = stimuli;
        c.stim_rev = rev;
    }

    pub fn stimuli(&self) -> (Stimuli, u64) {
        let c = self.ctl.lock().unwrap();
        (c.stimuli.clone(), c.stim_rev)
    }

    /// One frame now, whatever the rate.
    pub fn one_shot(&self) {
        self.ctl.lock().unwrap().one_shots += 1;
    }

    pub fn shutdown(&self) {
        self.ctl.lock().unwrap().shutdown = true;
        self.cv.notify_all();
    }

    pub fn is_running(&self) -> bool {
        !self.stopped.load(Ordering::Relaxed)
    }

    /// Called by the consumer for every item it takes out of the FIFO.
    pub fn consumed(&self) {
        self.depth.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Restarts after a panic (at most `MAX_RESTARTS` times), like the link thread; each panic is
/// counted in `SimShared::panics` and reported by the link.
pub fn spawn(shared: Arc<SimShared>, tx: SyncSender<BusItem>) -> thread::JoinHandle<()> {
    thread::Builder::new()
        .name("sim".into())
        .spawn(move || {
            for _ in 0..=MAX_RESTARTS {
                let (sh, tx) = (shared.clone(), tx.clone());
                if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(sh, tx))).is_ok() {
                    return;
                }
                shared.panics.fetch_add(1, Ordering::Relaxed);
            }
        })
        .expect("spawn sim thread")
}

const MAX_RESTARTS: u32 = 3;

/// The source of the current session: the training ECU, a mission source, or a mission source that failed
/// (ticks only).
enum Playing {
    Training,
    Source(ActiveSource),
    Failed,
}

fn run(sh: Arc<SimShared>, tx: SyncSender<BusItem>) {
    let mut epoch = 0u64;
    let (mut t_us, mut next_tick, mut frac, mut counter) = (0u64, 0u64, 0f64, 0u8);
    let mut last_t = 0u64;
    let mut playing = Playing::Training;
    let mut last = Instant::now();
    loop {
        let (stimuli, rev, period, shots) = {
            let mut c = sh.ctl.lock().unwrap();
            while !c.running && !c.shutdown {
                // A stopped mission releases its physical clock immediately.
                drop(std::mem::replace(&mut playing, Playing::Training));
                c = sh.cv.wait(c).unwrap();
                last = Instant::now();
            }
            if c.shutdown {
                return;
            }
            if c.epoch != epoch {
                // Drop the previous owner before creating the replacement source.
                drop(std::mem::replace(&mut playing, Playing::Training));
                epoch = c.epoch;
                (t_us, frac, counter, last_t) = (0, 0.0, 0, 0);
                next_tick = c.tick_period_us;
                playing = match &mut c.source {
                    SourceKind::Training => Playing::Training,
                    SourceKind::Mission(factory) => match catch_unwind(AssertUnwindSafe(|| factory.start())) {
                        Ok(src) => Playing::Source(ActiveSource::new(src, sh.source_activity.clone())),
                        Err(_) => {
                            sh.source_errors.fetch_add(1, Ordering::Relaxed);
                            Playing::Failed
                        }
                    },
                };
                last = Instant::now();
            }
            (c.stimuli.clone(), c.stim_rev, c.tick_period_us, std::mem::take(&mut c.one_shots))
        };
        let now = Instant::now();
        let step = now.duration_since(last);
        last = now;
        sh.max_step_us.fetch_max(step.as_micros() as u64, Ordering::Relaxed);
        let step = step.min(MAX_STEP);
        let push = |kind: BusKind| -> bool {
            // Counted before the item becomes visible: the consumer's decrement always follows
            // this increment, so `depth` never underflows.
            let d = sh.depth.fetch_add(1, Ordering::Relaxed) + 1;
            match tx.try_send(BusItem { epoch, kind }) {
                Ok(()) => {
                    sh.high_water.fetch_max(d, Ordering::Relaxed);
                    true
                }
                Err(e) => {
                    sh.depth.fetch_sub(1, Ordering::Relaxed);
                    if matches!(e, TrySendError::Full(_)) {
                        sh.dropped.fetch_add(1, Ordering::Relaxed);
                    }
                    false
                }
            }
        };
        if sh.depth.load(Ordering::Relaxed) >= HIGH_WATER {
            sh.paused_us.fetch_add(step.as_micros() as u64, Ordering::Relaxed);
        } else {
            t_us += step.as_micros() as u64;
            sh.bus_time_us.store(t_us, Ordering::Relaxed);
            let mut failed = false;
            match &mut playing {
                Playing::Training => {
                    while period > 0 && next_tick <= t_us {
                        push(BusKind::Time(next_tick));
                        next_tick += period;
                    }
                    frac += stimuli.rate_hz as f64 * step.as_secs_f64();
                    let periodic = frac.floor();
                    frac -= periodic;
                    for _ in 0..(periodic as u32 + shots) {
                        let (can_id, flags, dlc, data) = stimuli.frame(counter);
                        counter = counter.wrapping_add(1);
                        if push(BusKind::Frame(GenFrame { t_us, can_id, flags, dlc, data, stim_rev: rev })) {
                            sh.generated.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
                Playing::Source(src) => {
                    // A failing source never takes the bus clock down: the session goes on with ticks only.
                    let frames = catch_unwind(AssertUnwindSafe(|| src.advance(t_us))).unwrap_or_else(|_| {
                        failed = true;
                        Vec::new()
                    });
                    interleave(frames, t_us, period, &mut next_tick, &mut last_t, rev, &mut |kind| {
                        let frame = matches!(kind, BusKind::Frame(_));
                        if push(kind) && frame {
                            sh.generated.fetch_add(1, Ordering::Relaxed);
                        }
                    });
                }
                Playing::Failed => {
                    interleave(Vec::new(), t_us, period, &mut next_tick, &mut last_t, rev, &mut |kind| {
                        push(kind);
                    });
                }
            }
            if failed {
                sh.source_errors.fetch_add(1, Ordering::Relaxed);
                playing = Playing::Failed;
            }
        }
        // 1 ms logical step; the next iteration measures the real elapsed time.
        thread::sleep(Duration::from_millis(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn training_encoding_and_limits() {
        let s = Stimuli { pot: 200, button: true, ..Stimuli::default() };
        assert_eq!(s.frame(7), (0x123, 0, 8, vec![200, 1, 7, 3, 4, 5, 6, 7]));
        let fd = Stimuli { format: FrameFormat::FdBrs, dlc: 15, ..Stimuli::default() };
        assert_eq!(fd.frame(0).3.len(), 64);
        let rtr = Stimuli { format: FrameFormat::Remote, dlc: 4, ..Stimuli::default() };
        assert!(rtr.frame(0).3.is_empty());
        assert!(Stimuli { can_id: 0x800, ..Stimuli::default() }.validate().is_err());
        assert!(Stimuli { can_id: 0x800, format: FrameFormat::Extended, ..Stimuli::default() }.validate().is_ok());
        assert!(Stimuli { rate_hz: 5000, ..Stimuli::default() }.validate().is_err());
    }

    #[test]
    fn concurrent_consumer_never_breaks_the_depth_counter() {
        let sh = SimShared::new(Stimuli { rate_hz: 1000, ..Stimuli::default() });
        let (tx, rx) = std::sync::mpsc::sync_channel(QUEUE_CAPACITY);
        let h = spawn(sh.clone(), tx);
        sh.start(1, 1);
        let consumer = {
            let sh = sh.clone();
            thread::spawn(move || {
                let deadline = Instant::now() + Duration::from_millis(1500);
                let mut n = 0u64;
                while Instant::now() < deadline {
                    while rx.try_recv().is_ok() {
                        sh.consumed();
                        n += 1;
                    }
                }
                n
            })
        };
        let consumed = consumer.join().unwrap();
        sh.stop();
        sh.shutdown();
        h.join().unwrap();
        assert_eq!(sh.panics.load(Ordering::Relaxed), 0);
        assert!(consumed > 2000, "{consumed} items in 1.5 s at 1000 frames/s + 1000 ticks/s");
        assert!(sh.depth.load(Ordering::Relaxed) < QUEUE_CAPACITY);
    }

    #[test]
    fn generator_rate_and_ticks() {
        let sh = SimShared::new(Stimuli { rate_hz: 500, ..Stimuli::default() });
        let (tx, rx) = std::sync::mpsc::sync_channel(QUEUE_CAPACITY);
        let h = spawn(sh.clone(), tx);
        sh.start(1, 10);
        thread::sleep(Duration::from_millis(400));
        sh.stop();
        sh.shutdown();
        h.join().unwrap();
        let items: Vec<BusItem> = rx.try_iter().collect();
        let frames = items.iter().filter(|i| matches!(i.kind, BusKind::Frame(_))).count();
        let ticks = items.iter().filter(|i| matches!(i.kind, BusKind::Time(_))).count();
        assert!((150..=260).contains(&frames), "{frames} frames in ~400 ms at 500/s");
        assert!((30..=50).contains(&ticks), "{ticks} ticks of 10 ms in ~400 ms");
        let times: Vec<u64> = items
            .iter()
            .map(|i| match &i.kind {
                BusKind::Frame(f) => f.t_us,
                BusKind::Time(t) => *t,
            })
            .collect();
        assert!(times.windows(2).all(|w| w[0] <= w[1]), "bus time must not decrease");
    }

    // ---------------------------------------------------------------- mission source (test double)

    /// One frame every `period` µs from `period` on (a stand-in for the mission vehicle).
    struct Every {
        period: u64,
        next: u64,
        id: u32,
        panic_at: Option<u64>,
    }

    impl BusSource for Every {
        fn advance(&mut self, t_us: u64) -> Vec<BusFrame> {
            if self.panic_at.is_some_and(|t| t_us >= t) {
                panic!("source failure (test)");
            }
            let mut out = Vec::new();
            while self.next <= t_us {
                out.push(BusFrame { t_us: self.next, id: self.id, flags: 0, dlc: 2, data: vec![0xA5, 0x5A] });
                self.next += self.period;
            }
            out
        }
    }

    struct EveryFactory {
        started: Arc<AtomicU64>,
        panic_at: Option<u64>,
    }

    struct BlockingDropSource {
        entered: std::sync::mpsc::Sender<()>,
        dropping: std::sync::mpsc::Sender<()>,
        advance_gate: Arc<std::sync::Barrier>,
        drop_gate: Arc<std::sync::Barrier>,
        released: Arc<AtomicBool>,
        first: bool,
    }

    impl BusSource for BlockingDropSource {
        fn advance(&mut self, _t_us: u64) -> Vec<BusFrame> {
            if self.first {
                self.first = false;
                self.entered.send(()).unwrap();
                self.advance_gate.wait();
            }
            Vec::new()
        }
    }

    impl Drop for BlockingDropSource {
        fn drop(&mut self) {
            self.dropping.send(()).unwrap();
            self.drop_gate.wait();
            self.released.store(true, Ordering::SeqCst);
        }
    }

    struct BlockingDropFactory(Option<BlockingDropSource>);

    impl SourceFactory for BlockingDropFactory {
        fn start(&mut self) -> Box<dyn BusSource> { Box::new(self.0.take().unwrap()) }
    }

    #[test]
    fn quiesce_ack_waits_for_completed_drop_and_inhibits_old_factory_restarts() {
        let sh = SimShared::new(Stimuli::default());
        let (tx, _rx) = std::sync::mpsc::sync_channel(QUEUE_CAPACITY);
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (dropping_tx, dropping_rx) = std::sync::mpsc::channel();
        let advance_gate = Arc::new(std::sync::Barrier::new(2));
        let drop_gate = Arc::new(std::sync::Barrier::new(2));
        let released = Arc::new(AtomicBool::new(false));
        sh.set_source(SourceKind::Mission(Box::new(BlockingDropFactory(Some(BlockingDropSource {
            entered: entered_tx, dropping: dropping_tx, advance_gate: advance_gate.clone(),
            drop_gate: drop_gate.clone(), released: released.clone(), first: true,
        })))));
        let worker = spawn(sh.clone(), tx);
        sh.start(1, 10);
        entered_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let waiter = {
            let sh = sh.clone();
            thread::spawn(move || done_tx.send(sh.stop_and_wait_source(Duration::from_secs(3))).unwrap())
        };
        // The source is demonstrably still executing, independently of scheduling of the waiter.
        assert!(done_rx.recv_timeout(Duration::from_millis(20)).is_err());
        advance_gate.wait();
        dropping_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        // Completing advance is insufficient: the physical-clock release may occur in Drop.
        assert!(!released.load(Ordering::SeqCst));
        assert!(done_rx.recv_timeout(Duration::from_millis(20)).is_err());
        drop_gate.wait();
        assert_eq!(done_rx.recv_timeout(Duration::from_secs(3)).unwrap(), Ok(()));
        waiter.join().unwrap();
        assert!(released.load(Ordering::SeqCst));
        sh.start(2, 10);
        assert!(!sh.is_running(), "old factory cannot start between ACK and document replacement");
        sh.set_source(SourceKind::Training);
        sh.start(3, 10);
        assert!(sh.is_running(), "the replacement factory rearms starts");
        sh.stop(); sh.shutdown(); worker.join().unwrap();
        assert_eq!(sh.source_errors.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn quiesce_deadline_is_bounded_even_while_factory_creation_holds_control() {
        let sh = SimShared::new(Stimuli::default());
        let (locked_tx, locked_rx) = std::sync::mpsc::channel();
        let gate = Arc::new(std::sync::Barrier::new(2));
        let holder = {
            let sh = sh.clone(); let gate = gate.clone();
            thread::spawn(move || {
                let _ctl = sh.ctl.lock().unwrap();
                locked_tx.send(()).unwrap();
                gate.wait();
            })
        };
        locked_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        let before = Instant::now();
        let result = sh.stop_and_wait_source(Duration::from_millis(20));
        let elapsed = before.elapsed();
        gate.wait(); holder.join().unwrap();
        assert!(result.is_err());
        assert!(elapsed < Duration::from_secs(1), "deadline cannot wait indefinitely on factory creation");
        assert_eq!(sh.stop_and_wait_source(Duration::from_secs(1)), Ok(()));
    }

    impl SourceFactory for EveryFactory {
        fn start(&mut self) -> Box<dyn BusSource> {
            self.started.fetch_add(1, Ordering::Relaxed);
            Box::new(Every { period: 5_000, next: 5_000, id: 0x2A, panic_at: self.panic_at })
        }
    }

    fn time_of(k: &BusKind) -> u64 {
        match k {
            BusKind::Frame(f) => f.t_us,
            BusKind::Time(t) => *t,
        }
    }

    /// Non-decreasing time, and no frame before a tick of the same timestamp.
    fn assert_ordered(kinds: &[BusKind]) {
        assert!(kinds.windows(2).all(|w| time_of(&w[0]) <= time_of(&w[1])), "bus time must not decrease");
        for w in kinds.windows(2) {
            if let (BusKind::Frame(f), BusKind::Time(t)) = (&w[0], &w[1]) {
                assert!(f.t_us < *t, "tick {t} after a frame of the same time");
            }
        }
    }

    fn frame(t_us: u64, id: u32) -> BusFrame {
        BusFrame { t_us, id, flags: 0, dlc: 1, data: vec![7] }
    }

    #[test]
    fn interleave_puts_ticks_before_frames_of_the_same_time() {
        let (mut next_tick, mut last, mut out) = (10_000, 0, Vec::new());
        let frames = vec![frame(5_000, 1), frame(10_000, 2), frame(10_000, 3), frame(15_000, 4)];
        interleave(frames, 20_000, 10_000, &mut next_tick, &mut last, 0, &mut |k| out.push(k));
        let seq: Vec<(u64, Option<u32>)> = out
            .iter()
            .map(|k| match k {
                BusKind::Frame(f) => (f.t_us, Some(f.can_id)),
                BusKind::Time(t) => (*t, None),
            })
            .collect();
        assert_eq!(
            seq,
            [(5_000, Some(1)), (10_000, None), (10_000, Some(2)), (10_000, Some(3)), (15_000, Some(4)), (20_000, None)]
        );
        assert_eq!((next_tick, last), (30_000, 20_000));
        assert!(out.iter().all(|k| !matches!(k, BusKind::Frame(f) if f.flags != 0)), "classic frames, flags 0");
        assert_ordered(&out);
    }

    #[test]
    fn interleave_never_lets_time_go_back() {
        let (mut next_tick, mut last, mut out) = (20_000, 12_000, Vec::new());
        interleave(vec![frame(11_000, 1), frame(13_000, 2)], 14_000, 10_000, &mut next_tick, &mut last, 0, &mut |k| out.push(k));
        assert_eq!(out.iter().map(time_of).collect::<Vec<_>>(), [12_000, 13_000]);
        // No tick: nothing at all when the period is 0.
        let (mut next_tick, mut last, mut out) = (0, 0, Vec::new());
        interleave(vec![frame(1_000, 1)], 50_000, 0, &mut next_tick, &mut last, 0, &mut |k| out.push(k));
        assert_eq!(out.len(), 1);
    }

    fn collect(rx: &std::sync::mpsc::Receiver<BusItem>, sh: &SimShared, epoch: u64) -> Vec<BusKind> {
        let mut out = Vec::new();
        while let Ok(i) = rx.try_recv() {
            sh.consumed();
            if i.epoch == epoch {
                out.push(i.kind);
            }
        }
        out
    }

    #[test]
    fn mission_source_replaces_the_training_ecu_from_the_next_session() {
        let started = Arc::new(AtomicU64::new(0));
        let sh = SimShared::new(Stimuli { rate_hz: 200, ..Stimuli::default() });
        let (tx, rx) = std::sync::mpsc::sync_channel(QUEUE_CAPACITY);
        let h = spawn(sh.clone(), tx);
        sh.set_source(SourceKind::Mission(Box::new(EveryFactory { started: started.clone(), panic_at: None })));
        assert!(sh.mission_source());
        sh.start(1, 10);
        thread::sleep(Duration::from_millis(300));
        sh.stop();
        let mission = collect(&rx, &sh, 1);
        sh.set_source(SourceKind::Training);
        sh.start(2, 10);
        thread::sleep(Duration::from_millis(200));
        sh.stop();
        sh.shutdown();
        h.join().unwrap();
        let training = collect(&rx, &sh, 2);

        assert_eq!(started.load(Ordering::Relaxed), 1, "one source per session start");
        let frames: Vec<&GenFrame> = mission.iter().filter_map(|k| if let BusKind::Frame(f) = k { Some(f) } else { None }).collect();
        let ticks = mission.iter().filter(|k| matches!(k, BusKind::Time(_))).count();
        assert!(frames.len() >= 30 && ticks >= 15, "{} frames, {ticks} ticks in ~300 ms", frames.len());
        assert!(frames.iter().all(|f| f.can_id == 0x2A && f.flags == 0 && f.dlc == 2 && f.data == [0xA5, 0x5A]));
        assert!(frames.iter().all(|f| f.t_us % 5_000 == 0), "source timestamps kept");
        assert_ordered(&mission);
        let training_ids: Vec<u32> = training.iter().filter_map(|k| if let BusKind::Frame(f) = k { Some(f.can_id) } else { None }).collect();
        assert!(!training_ids.is_empty() && training_ids.iter().all(|&id| id == 0x123), "{training_ids:?}");
        assert_eq!(sh.source_errors.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn a_failing_source_leaves_the_ticks_running() {
        let started = Arc::new(AtomicU64::new(0));
        let sh = SimShared::new(Stimuli::default());
        let (tx, rx) = std::sync::mpsc::sync_channel(QUEUE_CAPACITY);
        let h = spawn(sh.clone(), tx);
        sh.set_source(SourceKind::Mission(Box::new(EveryFactory { started, panic_at: Some(50_000) })));
        sh.start(1, 10);
        thread::sleep(Duration::from_millis(250));
        sh.stop();
        sh.shutdown();
        h.join().unwrap();
        let items = collect(&rx, &sh, 1);
        assert_eq!(sh.source_errors.load(Ordering::Relaxed), 1);
        assert_eq!(sh.panics.load(Ordering::Relaxed), 0, "the sim thread itself never restarted");
        assert!(items.iter().filter(|k| matches!(k, BusKind::Time(_))).count() >= 12, "ticks go on");
        assert!(items.iter().all(|k| !matches!(k, BusKind::Frame(f) if f.t_us >= 50_000)));
        assert_ordered(&items);
    }
}
