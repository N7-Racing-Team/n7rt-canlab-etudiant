//! Link engine: the `link` thread owns the serial port and runs the PC side of N7RT-Link v0
//! (docs/n7rt-link-v0.md sections 9 and 10): handshake, sessions, credits, frame/answer
//! matching, liveness (STALLED / STUDENT_BUSY), loss detection and reconnection.
//!
//! Threads and queues (nothing here ever runs on the UI thread):
//! - UI → link: `Command` channel (unbounded, a few commands per second at most);
//! - sim → link: bus FIFO of `sim::QUEUE_CAPACITY` items (frames + tick times), backpressure
//!   at `sim::HIGH_WATER` pauses the bus clock instead of dropping;
//! - link → board: at most `W = HELLO_ACK.rx_window` frames in flight (credits);
//! - link → UI: `Shared`, drained by the UI bridge (Tauri layer) at 20 Hz; rows above
//!   `UI_ROWS_CAP` per drain are not shown (counted), never blocking the link.

use std::collections::{BTreeMap, VecDeque};
use std::io;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::mpsc::{self, RecvTimeoutError, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use n7rt_link::{
    code, message_name, ty, BoardHeartbeat, Encoder, Entry, Event, FaultReport, FrameResult, Hello, HelloAck,
    Message, Nack, Packet, SessionAck, SessionStart, StudentLog, TickResult, VcanFrame,
};

use crate::discovery::{self, PortInfo};
use crate::metrics::{Histogram, RateMeter};
use crate::model::*;
use crate::runtime::{self, BoardIdentity};
use crate::sim::{self, BusItem, BusKind, GenFrame, SimShared, SourceKind, Stimuli};

// --- Timings of spec section 10 ---------------------------------------------------------------
const T_HELLO: Duration = Duration::from_millis(200);
const HELLO_TRIES: u8 = 10;
const T_SESSION: Duration = Duration::from_millis(500);
/// First send + 3 retransmissions.
const SESSION_TRIES: u8 = 4;
const T_STALL: Duration = Duration::from_millis(750);
const T_BUSY_MS: u16 = 500;
const T_HB_TX: Duration = Duration::from_millis(250);
const T_RESET: Duration = Duration::from_millis(1500);
const T_LOST_RETRY: Duration = Duration::from_millis(500);
// --- Application choices --------------------------------------------------------------------
const T_ABSENT_RETRY: Duration = Duration::from_millis(1000);
const T_SILENT_RETRY: Duration = Duration::from_millis(2000);
/// A frame overtaken by a later answer (or covered by `acked_frame_seq`) is lost after this.
const LATE_GRACE: Duration = Duration::from_millis(300);
const NO_ANSWER_TIMEOUT: Duration = Duration::from_secs(5);
const CLOSING_TIMEOUT: Duration = Duration::from_secs(1);
const CLOSE_ACK_WAIT: Duration = Duration::from_millis(250);
const SCAN_PERIOD: Duration = Duration::from_secs(1);
const STATUS_PERIOD: Duration = Duration::from_millis(50);
/// Serial read timeout: the longest the link waits for board bytes before serving its queues.
pub const READ_TIMEOUT: Duration = Duration::from_millis(2);
/// Longest time one pass of the link spends writing; the rest stays queued for the next pass.
const WRITE_SLICE: Duration = Duration::from_millis(20);
/// A port that accepts no byte for this long is considered lost (a busy ST-LINK, for instance
/// while it closes an SWD session, only pauses for a fraction of it).
const WRITE_STALL_LIMIT: Duration = Duration::from_secs(3);
const TX_BACKLOG_MAX: usize = 64 * 1024;
/// Windows write timeout (see `open_port`): a write still incomplete after this resets the link.
#[cfg_attr(not(windows), allow(dead_code))]
const WRITE_TIMEOUT_WINDOWS: Duration = Duration::from_secs(2);
const TX_BATCH: usize = 2048;
pub const UI_ROWS_CAP: usize = 128;
const UI_LOGS_CAP: usize = 256;
const UI_ERRORS_CAP: usize = 256;
const MAX_RESTARTS: u32 = 3;

// --- Transport abstraction (real serial port, or an in-memory board in tests) ------------------

pub trait Transport: Send {
    /// Bytes available, or `Ok(0)` after the read timeout.
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize>;
    fn write(&mut self, data: &[u8]) -> io::Result<usize>;
    /// Drops bytes received before the port was (re)opened by this program.
    fn clear_input(&mut self) {}
    /// True when a write that returns early reports exactly what was sent (POSIX). When false
    /// (Windows usbser after a write timeout), the unsent remainder cannot be known: retrying
    /// could duplicate bytes, so a short write resets the connection instead.
    fn exact_partial_writes(&self) -> bool {
        true
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenError {
    /// The port exists but another program holds it.
    Busy(String),
    /// The OS refuses access (Linux: user not in `dialout`/`uucp`).
    Permission(String),
    Missing(String),
    Other(String),
}

pub trait Connector: Send {
    fn scan(&mut self) -> Result<Vec<PortInfo>, String>;
    fn open(&mut self, port: &str) -> Result<Box<dyn Transport>, OpenError>;
}

struct SerialTransport(Box<dyn serialport::SerialPort>);

/// Windows: reads keep the short timeout, writes get a long one of their own (serialport sets
/// a single value for both). See `Transport::exact_partial_writes`.
#[cfg(windows)]
fn open_port(builder: serialport::SerialPortBuilder) -> serialport::Result<Box<dyn serialport::SerialPort>> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Devices::Communication::{SetCommTimeouts, COMMTIMEOUTS};
    let port = builder.open_native()?;
    let timeouts = COMMTIMEOUTS {
        ReadIntervalTimeout: u32::MAX,
        ReadTotalTimeoutMultiplier: u32::MAX,
        ReadTotalTimeoutConstant: READ_TIMEOUT.as_millis() as u32,
        WriteTotalTimeoutMultiplier: 0,
        WriteTotalTimeoutConstant: WRITE_TIMEOUT_WINDOWS.as_millis() as u32,
    };
    // SAFETY: the handle belongs to `port`, alive for the call; the struct is fully initialised.
    if unsafe { SetCommTimeouts(port.as_raw_handle() as _, &timeouts) } == 0 {
        return Err(serialport::Error::new(serialport::ErrorKind::Io(io::ErrorKind::Other), io::Error::last_os_error().to_string()));
    }
    Ok(Box::new(port))
}

#[cfg(not(windows))]
fn open_port(builder: serialport::SerialPortBuilder) -> serialport::Result<Box<dyn serialport::SerialPort>> {
    builder.open()
}

impl Transport for SerialTransport {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self.0.read(buf) {
            Err(e) if e.kind() == io::ErrorKind::TimedOut => Ok(0),
            other => other,
        }
    }
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        match self.0.write(data) {
            Err(e) if e.kind() == io::ErrorKind::TimedOut => Ok(0),
            other => other,
        }
    }
    fn clear_input(&mut self) {
        let _ = self.0.clear(serialport::ClearBuffer::Input);
    }
    fn exact_partial_writes(&self) -> bool {
        !cfg!(windows)
    }
}

/// The real thing: `serialport` at 921 600 Bd 8N1, no flow control (spec section 2).
pub struct SerialConnector;

impl Connector for SerialConnector {
    fn scan(&mut self) -> Result<Vec<PortInfo>, String> {
        discovery::scan()
    }

    fn open(&mut self, port: &str) -> Result<Box<dyn Transport>, OpenError> {
        let opened = open_port(
            serialport::new(port, n7rt_link::BAUD)
                .data_bits(serialport::DataBits::Eight)
                .parity(serialport::Parity::None)
                .stop_bits(serialport::StopBits::One)
                .flow_control(serialport::FlowControl::None)
                .timeout(READ_TIMEOUT),
        );
        match opened {
            Ok(p) => Ok(Box::new(SerialTransport(p))),
            Err(e) => {
                let text = e.description.clone();
                Err(match e.kind {
                    // serialport reports a port held by someone else (Windows ERROR_ACCESS_DENIED,
                    // POSIX EBUSY) like a missing one: a port the OS still lists is busy.
                    serialport::ErrorKind::NoDevice => {
                        let listed = discovery::scan().map(|l| l.iter().any(|p| p.port == port)).unwrap_or(false);
                        if listed {
                            OpenError::Busy(text)
                        } else {
                            OpenError::Missing(text)
                        }
                    }
                    serialport::ErrorKind::Io(io::ErrorKind::PermissionDenied) => OpenError::Permission(text),
                    serialport::ErrorKind::Io(io::ErrorKind::NotFound) => OpenError::Missing(text),
                    _ => OpenError::Other(text),
                })
            }
        }
    }
}

// --- Check recording (mission nominal check, docs/apps-mission-slice.md §6) --------------------

/// A frame as actually sent to the board in `VCAN_FRAME`.
#[derive(Debug, Clone, PartialEq)]
pub struct RecFrame {
    pub t_us: u64,
    pub id: u32,
    pub flags: u8,
    pub data: Vec<u8>,
}

/// One publication of a `TICK_RESULT`: `kind` 0 = value, 1 = reject.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RecEntry {
    pub slot: u8,
    pub kind: u8,
    pub value: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RecTick {
    pub t_us: u64,
    /// 0 = delivered to the student code, 1 = not delivered.
    pub status: u8,
    pub entries: Vec<RecEntry>,
}

/// What crossed the link during a check, for one session: the first one seen after the start.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CheckRecord {
    pub session_id: Option<u32>,
    pub frames: Vec<RecFrame>,
    pub ticks: Vec<RecTick>,
    /// A bound was reached: the record is incomplete.
    pub overflow: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CheckProgress {
    pub session_id: Option<u32>,
    pub frames: usize,
    pub ticks: usize,
    pub last_tick_us: Option<u64>,
    pub overflow: bool,
}

/// Bounds of a check record (a nominal check is 9 s of bus time: ~1 000 ticks, well under 20 000 frames).
pub const CHECK_MAX_FRAMES: usize = 200_000;
pub const CHECK_MAX_TICKS: usize = 20_000;

/// Shared by the link thread (writer) and the mission controller (start, stop). The recorded session's rows
/// and slots are never published to the UI (ch. 24 §7.5: the bus view and the slots are covered during a
/// check, and the course of the hidden test is never displayed), also after the recording stops.
#[derive(Default)]
struct CheckRecorder {
    active: AtomicBool,
    rec: Mutex<CheckRecord>,
    /// `1 << 32 | session id` of the recorded session, 0 when none.
    masked: AtomicU64,
}

impl CheckRecorder {
    fn is_active(&self) -> bool {
        self.active.load(std::sync::atomic::Ordering::Relaxed)
    }

    fn masked(&self) -> Option<u32> {
        let m = self.masked.load(std::sync::atomic::Ordering::Relaxed);
        (m >> 32 == 1).then_some(m as u32)
    }

    /// The record of `session`, if it is the recorded one (the first session seen locks the record).
    fn with(&self, session: u32, f: impl FnOnce(&mut CheckRecord)) {
        if !self.is_active() {
            return;
        }
        let mut r = self.rec.lock().unwrap();
        match r.session_id {
            None => {
                r.session_id = Some(session);
                self.masked.store(1 << 32 | session as u64, std::sync::atomic::Ordering::Relaxed);
            }
            Some(s) if s != session => return,
            Some(_) => {}
        }
        f(&mut r);
    }

    fn frame(&self, session: u32, g: &GenFrame) {
        self.with(session, |r| {
            if r.frames.len() >= CHECK_MAX_FRAMES {
                r.overflow = true;
            } else {
                r.frames.push(RecFrame { t_us: g.t_us, id: g.can_id, flags: g.flags, data: g.data.clone() });
            }
        });
    }

    fn tick(&self, session: u32, t: &TickResult) {
        self.with(session, |r| {
            if r.ticks.len() >= CHECK_MAX_TICKS {
                r.overflow = true;
            } else {
                let entries = t.entries.iter().map(|e| RecEntry { slot: e.slot, kind: e.kind, value: e.value }).collect();
                r.ticks.push(RecTick { t_us: t.t_us, status: t.status, entries });
            }
        });
    }
}

// --- Public handle ----------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum Command {
    RefreshPorts,
    /// Connect to a probe serial number (preferred: survives re-plugs) or a port name.
    Connect { port: Option<String>, serial: Option<String> },
    Disconnect,
    StartSession { tick_period_ms: u16 },
    StopSession,
    SetStimuli { stimuli: Stimuli, rev: u64 },
    SendOne,
    /// RESET_REQ: the board reboots and announces itself with HELLO_ACK(boot).
    ResetBoard,
    Shutdown,
}

#[derive(Debug, Clone)]
pub struct LabConfig {
    /// Connect at start-up when exactly one ST-LINK is present.
    pub auto_connect: bool,
    /// Start-up target (probe serial number), e.g. from the command line.
    pub serial: Option<String>,
    /// Keep every completed frame and log (automated verification).
    pub record: bool,
    pub stimuli: Stimuli,
}

impl Default for LabConfig {
    fn default() -> Self {
        Self { auto_connect: true, serial: None, record: false, stimuli: Stimuli::default() }
    }
}

#[derive(Default)]
struct Inner {
    rev: u64,
    drained: u64,
    status: Status,
    stats: Stats,
    rows: VecDeque<FrameRow>,
    logs: VecDeque<LogRow>,
    errors: VecDeque<ErrorRow>,
    slots: Vec<SlotView>,
    rows_not_shown: u64,
    logs_dropped: u64,
    errors_dropped: u64,
    record: Option<crate::e2e::Recorder>,
    /// The frames shown during the last [`RECENT_US`] of the current session (evidence of a recorded measure).
    recent: VecDeque<FrameRow>,
}

/// Bus time kept by [`Lab::recent_rows`].
pub const RECENT_US: u64 = 3_000_000;
const RECENT_CAP: usize = 8_192;

pub struct Lab {
    cmd: mpsc::Sender<Command>,
    shared: Arc<Mutex<Inner>>,
    sim: Arc<SimShared>,
    check: Arc<CheckRecorder>,
    threads: Mutex<Vec<JoinHandle<()>>>,
}

impl Lab {
    pub fn start(cfg: LabConfig, connector: Box<dyn Connector>) -> Lab {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (bus_tx, bus_rx) = mpsc::sync_channel(sim::QUEUE_CAPACITY);
        let simsh = SimShared::new(cfg.stimuli.clone());
        let shared = Arc::new(Mutex::new(Inner {
            record: cfg.record.then(crate::e2e::Recorder::default),
            ..Inner::default()
        }));
        let check = Arc::new(CheckRecorder::default());
        let sim_thread = sim::spawn(simsh.clone(), bus_tx);
        let mut engine = Engine::new(cfg, connector, cmd_rx, bus_rx, simsh.clone(), shared.clone(), check.clone());
        let link_thread = thread::Builder::new()
            .name("link".into())
            .spawn(move || engine.run_guarded())
            .expect("spawn link thread");
        Lab { cmd: cmd_tx, shared, sim: simsh, check, threads: Mutex::new(vec![link_thread, sim_thread]) }
    }

    /// Bus source played from the next session start on (training ECU or mission vehicle).
    pub fn set_bus_source(&self, source: SourceKind) {
        self.sim.set_source(source);
    }

    /// A mission document can be replaced only after its source released the physical clock.
    /// Installing the replacement factory rearms future session starts.
    pub fn quiesce_bus_source(&self) -> Result<(), String> {
        self.sim.stop_and_wait_source(Duration::from_secs(5))
    }

    #[cfg(test)]
    pub(crate) fn source_error_count(&self) -> u64 {
        self.sim.source_errors.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Bus time of the current (or last) session, µs.
    pub fn bus_time_us(&self) -> u64 {
        self.sim.bus_time_us.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Starts recording the frames sent and the `TICK_RESULT`s received, for the next session seen (start it
    /// while no session is open). The recorded session is never shown in the UI.
    pub fn check_record_start(&self) {
        *self.check.rec.lock().unwrap() = CheckRecord::default();
        self.check.active.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    /// Stops the recording and hands the record over.
    pub fn check_record_stop(&self) -> CheckRecord {
        self.check.active.store(false, std::sync::atomic::Ordering::Relaxed);
        std::mem::take(&mut *self.check.rec.lock().unwrap())
    }

    pub fn check_record_progress(&self) -> CheckProgress {
        let r = self.check.rec.lock().unwrap();
        CheckProgress {
            session_id: r.session_id,
            frames: r.frames.len(),
            ticks: r.ticks.len(),
            last_tick_us: r.ticks.last().map(|t| t.t_us),
            overflow: r.overflow,
        }
    }

    /// Never blocks.
    pub fn command(&self, c: Command) {
        let _ = self.cmd.send(c);
    }

    /// Everything new since the previous call, or `None` if nothing changed.
    pub fn drain(&self) -> Option<UiBatch> {
        let mut g = self.shared.lock().unwrap();
        if g.rev == g.drained {
            return None;
        }
        g.drained = g.rev;
        Some(Self::batch(&mut g, true))
    }

    /// The frames shown during the last [`RECENT_US`] of bus time of the current session, oldest first.
    pub fn recent_rows(&self) -> Vec<FrameRow> {
        self.shared.lock().unwrap().recent.iter().cloned().collect()
    }

    /// Full current state (rows are left for the next drain).
    pub fn snapshot(&self) -> UiBatch {
        let mut g = self.shared.lock().unwrap();
        Self::batch(&mut g, false)
    }

    fn batch(g: &mut Inner, take: bool) -> UiBatch {
        let mut stats = g.stats.clone();
        stats.queues.ui_rows_capacity = UI_ROWS_CAP;
        stats.queues.ui_rows_not_shown = g.rows_not_shown;
        stats.queues.ui_logs_dropped = g.logs_dropped;
        stats.queues.ui_errors_dropped = g.errors_dropped;
        UiBatch {
            rev: g.rev,
            status: g.status.clone(),
            stats,
            rows: if take { g.rows.drain(..).collect() } else { Vec::new() },
            logs: if take { g.logs.drain(..).collect() } else { Vec::new() },
            errors: if take { g.errors.drain(..).collect() } else { Vec::new() },
            slots: g.slots.clone(),
        }
    }

    /// The last completed frames (only with `LabConfig::record`, see `e2e::Recorder`).
    pub fn record(&self) -> Vec<FrameRow> {
        self.shared.lock().unwrap().record.as_ref().map(|r| r.rows.iter().cloned().collect()).unwrap_or_default()
    }

    pub fn log_record(&self) -> Vec<LogRow> {
        self.shared.lock().unwrap().record.as_ref().map(|r| r.logs.iter().cloned().collect()).unwrap_or_default()
    }

    /// (frames recorded since start-up, answered frames checked by the oracle, mismatches).
    pub fn oracle(&self) -> (u64, usize, Vec<String>) {
        let g = self.shared.lock().unwrap();
        g.record.as_ref().map_or((0, 0, Vec::new()), |r| (r.total_rows, r.oracle_checked, r.oracle_mismatches.clone()))
    }
}

impl Lab {
    /// Closes the session (SESSION_STOP) and the port, then stops the threads. Idempotent.
    pub fn shutdown(&self) {
        let _ = self.cmd.send(Command::Shutdown);
        self.sim.shutdown();
        let threads: Vec<_> = self.threads.lock().unwrap().drain(..).collect();
        for t in threads {
            let _ = t.join();
        }
    }
}

impl Drop for Lab {
    fn drop(&mut self) {
        self.shutdown();
    }
}

// --- Engine state -----------------------------------------------------------------------------

struct Conn {
    io: Box<dyn Transport>,
    port: PortInfo,
    rx: n7rt_link::Receiver,
    enc: Encoder,
    last_rx: Instant,
    last_hb_tx: Instant,
    nonce: [u8; 16],
    /// Handshake in progress: last HELLO sent and number of HELLOs.
    hello: Option<(Instant, u8)>,
    board_seq: Option<u16>,
    reset_sent: Option<Instant>,
    /// False until the HELLO_ACK of this connection: what arrives before is left over from
    /// before the port was opened (board heartbeats, an answer to a previous connection).
    synced: bool,
    /// HELLO_ACKs whose challenge does not match this handshake's nonce.
    foreign_acks: u8,
}

struct InFlight {
    frame_seq: u32,
    seq: u16,
    sent_at: Instant,
    overdue_since: Option<Instant>,
    row: FrameRow,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Phase {
    Starting { sent_at: Instant, tries: u8 },
    Active,
    Stopping { sent_at: Instant, tries: u8 },
    /// Stopped: waiting for the answers of frames still in flight.
    Closing { since: Instant },
}

struct Session {
    id: u32,
    mode: u8,
    tick_period_ms: u16,
    phase: Phase,
    started: Instant,
    epoch: u64,
    last_frame_seq: u32,
    max_acked: u32,
    inflight: VecDeque<InFlight>,
    pending: Option<BusItem>,
    out_of_session_reported: bool,
}

#[derive(Clone)]
struct Target {
    serial: Option<String>,
    port: Option<String>,
}

impl Target {
    fn label(&self) -> String {
        self.serial.clone().or_else(|| self.port.clone()).unwrap_or_default()
    }
}

struct SessionAcc {
    frames_sent: u64,
    results: u64,
    nacks: u64,
    nack_codes: BTreeMap<String, u64>,
    lost: u64,
    late: u64,
    aborted: u64,
    in_flight_max: u32,
    credit_waits: u64,
    sim_time_sent: u64,
    tick_results: u64,
    logs: u64,
    rtt: Histogram,
    tx_rate: RateMeter,
    rx_rate: RateMeter,
}

impl SessionAcc {
    fn new() -> Self {
        Self {
            frames_sent: 0,
            results: 0,
            nacks: 0,
            nack_codes: BTreeMap::new(),
            lost: 0,
            late: 0,
            aborted: 0,
            in_flight_max: 0,
            credit_waits: 0,
            sim_time_sent: 0,
            tick_results: 0,
            logs: 0,
            rtt: Histogram::new(25, 200_000),
            tx_rate: RateMeter::new(),
            rx_rate: RateMeter::new(),
        }
    }
}

#[derive(Default)]
struct LinkAcc {
    rx_framing: BTreeMap<String, u64>,
    rx_rejects: u64,
    board_seq_gaps: u64,
    bytes_tx: u64,
    bytes_rx: u64,
    loop_max_us: u64,
    write_max_us: u64,
    read_chunk_max: usize,
    pre_sync_discarded: u64,
    tx_backlog_max: usize,
}

struct Engine {
    connector: Box<dyn Connector>,
    cmd_rx: mpsc::Receiver<Command>,
    bus_rx: mpsc::Receiver<BusItem>,
    sim: Arc<SimShared>,
    shared: Arc<Mutex<Inner>>,
    t0: Instant,
    auto_connect: bool,
    target: Option<Target>,
    conn: Option<Conn>,
    state: LinkState,
    stalled: bool,
    unstalled_at: Instant,
    busy: bool,
    message: String,
    board: Option<BoardIdentity>,
    student_status: u8,
    fault: Option<String>,
    window: u32,
    hb: Option<HeartbeatView>,
    session: Option<Session>,
    next_epoch: u64,
    pending_start: Option<u16>,
    /// Tick period of a session interrupted by a board reboot or a lost port, to resume it.
    resume: Option<u16>,
    recently_lost: VecDeque<u32>,
    ports: Vec<PortInfo>,
    last_scan: Option<Instant>,
    retry_at: Instant,
    acc: SessionAcc,
    lacc: LinkAcc,
    out_rows: Vec<FrameRow>,
    out_logs: Vec<LogRow>,
    out_errors: Vec<ErrorRow>,
    slots: BTreeMap<u8, SlotView>,
    slots_dirty: bool,
    status_dirty: bool,
    last_status_pub: Instant,
    txbuf: Vec<u8>,
    tx_stuck_since: Option<Instant>,
    sim_panics_seen: u64,
    source_errors_seen: u64,
    check: Arc<CheckRecorder>,
}

/// Frames still waiting for an answer, for the credit rule (spec 9.2): `last sent − acked`,
/// where frames already answered or counted lost below the oldest pending one are no longer in
/// flight. Without the second term, frames lost at the tail of the window (board UART overrun
/// while the core was halted) would hold their credits forever.
fn in_flight(s: &Session) -> u32 {
    let resolved = s.inflight.front().map_or(s.last_frame_seq, |f| f.frame_seq - 1);
    s.last_frame_seq - s.max_acked.max(resolved)
}

fn random_bytes<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).expect("OS random generator");
    b
}

fn code_name(c: u8) -> String {
    code::name(c).map(str::to_owned).unwrap_or_else(|| format!("code 0x{c:02X}"))
}

fn entry_views(entries: &[Entry]) -> Vec<EntryView> {
    entries.iter().map(|e| EntryView { slot: e.slot, kind: e.kind, bits: e.value.to_bits() }).collect()
}

fn transient(e: &io::Error) -> bool {
    matches!(e.kind(), io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted)
}

impl Engine {
    fn new(
        cfg: LabConfig,
        connector: Box<dyn Connector>,
        cmd_rx: mpsc::Receiver<Command>,
        bus_rx: mpsc::Receiver<BusItem>,
        sim: Arc<SimShared>,
        shared: Arc<Mutex<Inner>>,
        check: Arc<CheckRecorder>,
    ) -> Self {
        let now = Instant::now();
        let target = cfg.serial.map(|sn| Target { serial: Some(sn), port: None });
        Engine {
            connector,
            cmd_rx,
            bus_rx,
            sim,
            shared,
            t0: now,
            auto_connect: cfg.auto_connect && target.is_none(),
            target,
            conn: None,
            state: LinkState::Disconnected,
            stalled: false,
            unstalled_at: now,
            busy: false,
            message: "Recherche des cartes…".into(),
            board: None,
            student_status: 0,
            fault: None,
            window: 1,
            hb: None,
            session: None,
            next_epoch: 0,
            pending_start: None,
            resume: None,
            recently_lost: VecDeque::new(),
            ports: Vec::new(),
            last_scan: None,
            retry_at: now,
            acc: SessionAcc::new(),
            lacc: LinkAcc::default(),
            out_rows: Vec::new(),
            out_logs: Vec::new(),
            out_errors: Vec::new(),
            slots: BTreeMap::new(),
            slots_dirty: false,
            status_dirty: true,
            last_status_pub: now,
            txbuf: Vec::with_capacity(TX_BATCH * 2),
            tx_stuck_since: None,
            sim_panics_seen: 0,
            source_errors_seen: 0,
            check,
        }
    }

    // --- Main loop ----------------------------------------------------------------------------

    /// `catch_unwind` around the loop (docs/can-lab-architecture.md section 8): after a panic
    /// the link restarts from a clean state, at most `MAX_RESTARTS` times.
    fn run_guarded(&mut self) {
        let mut restarts = 0;
        loop {
            match catch_unwind(AssertUnwindSafe(|| self.run())) {
                Ok(()) => return,
                Err(panic) => {
                    let what = panic
                        .downcast_ref::<&str>()
                        .map(|s| s.to_string())
                        .or_else(|| panic.downcast_ref::<String>().cloned())
                        .unwrap_or_default();
                    restarts += 1;
                    self.recover();
                    self.error("link", format!("Erreur interne du moteur de liaison ({what}) : redémarrage {restarts}/{MAX_RESTARTS}."));
                    if restarts > MAX_RESTARTS {
                        self.set_state(LinkState::Disconnected, "Moteur de liaison arrêté après des erreurs internes répétées : relancez l'application.".into());
                        self.publish();
                        return;
                    }
                }
            }
        }
    }

    fn recover(&mut self) {
        self.sim.stop();
        self.session = None;
        self.conn = None;
        self.txbuf.clear();
        self.tx_stuck_since = None;
        self.stalled = false;
        self.busy = false;
        self.retry_at = Instant::now() + T_LOST_RETRY;
        self.state = LinkState::Lost;
    }

    fn run(&mut self) {
        loop {
            let started = Instant::now();
            loop {
                match self.cmd_rx.try_recv() {
                    Ok(Command::Shutdown) | Err(TryRecvError::Disconnected) => return self.shutdown(),
                    Ok(c) => self.on_command(c),
                    Err(TryRecvError::Empty) => break,
                }
            }
            if self.conn.is_some() {
                self.pump_tx();
                self.flush();
                self.pump_rx();
                self.timers();
                self.flush();
            } else {
                self.idle();
            }
            self.lacc.loop_max_us = self.lacc.loop_max_us.max(started.elapsed().as_micros() as u64);
            let panics = self.sim.panics.load(std::sync::atomic::Ordering::Relaxed);
            if panics != self.sim_panics_seen {
                self.sim_panics_seen = panics;
                self.error("link", format!("Erreur interne du générateur de bus virtuel : redémarrage {panics}/3."));
            }
            let source_errors = self.sim.source_errors.load(std::sync::atomic::Ordering::Relaxed);
            if source_errors != self.source_errors_seen {
                self.source_errors_seen = source_errors;
                self.error("link", "Erreur interne du véhicule simulé : le bus ne porte plus que les ticks de cette session.".into());
            }
            self.publish();
            if self.conn.is_none() {
                match self.cmd_rx.recv_timeout(Duration::from_millis(20)) {
                    Ok(Command::Shutdown) | Err(RecvTimeoutError::Disconnected) => return self.shutdown(),
                    Ok(c) => self.on_command(c),
                    Err(RecvTimeoutError::Timeout) => {}
                }
            }
        }
    }

    fn shutdown(&mut self) {
        self.close_conn(LinkState::Disconnected, "Application fermée.".into(), true);
        self.publish();
    }

    // --- Commands -----------------------------------------------------------------------------

    fn on_command(&mut self, c: Command) {
        match c {
            Command::RefreshPorts => self.rescan(),
            Command::Connect { port, serial } => {
                let target = Target { serial: serial.filter(|s| !s.is_empty()), port: port.filter(|p| !p.is_empty()) };
                if target.serial.is_none() && target.port.is_none() {
                    return self.error("port", "Choisissez une carte avant de vous connecter.".into());
                }
                // Repeating Connect for the same live board must not tear down its link/session.
                if self.conn.as_ref().is_some_and(|current| match target.serial.as_ref() {
                    Some(serial) => current.port.serial.as_ref() == Some(serial),
                    None => target.port.as_ref() == Some(&current.port.port),
                }) {
                    return;
                }
                if self.conn.is_some() {
                    self.close_conn(LinkState::Absent, "Changement de carte…".into(), true);
                }
                self.message = format!("Connexion à {}…", target.label());
                self.target = Some(target);
                self.auto_connect = false;
                self.retry_at = Instant::now();
                self.status_dirty = true;
            }
            Command::Disconnect => {
                self.target = None;
                self.auto_connect = false;
                self.resume = None;
                self.close_conn(LinkState::Disconnected, "Déconnecté. Choisissez une carte puis « Connecter ».".into(), true);
            }
            Command::StartSession { tick_period_ms } => self.start_session(tick_period_ms),
            Command::StopSession => {
                self.resume = None;
                self.stop_session();
            }
            Command::SetStimuli { stimuli, rev } => match stimuli.validate() {
                Ok(()) => {
                    self.sim.set_stimuli(stimuli, rev);
                    self.status_dirty = true;
                }
                Err(e) => self.error("session", format!("Stimuli refusés : {e}.")),
            },
            Command::SendOne => {
                if matches!(self.session.as_ref().map(|s| s.phase), Some(Phase::Active)) {
                    self.sim.one_shot();
                } else {
                    self.error("session", "Démarrez une session pour envoyer une trame.".into());
                }
            }
            Command::ResetBoard => {
                if self.conn.is_none() {
                    return self.error("session", "Aucune carte connectée.".into());
                }
                self.resume = self.session.as_ref().filter(|s| s.phase == Phase::Active).map(|s| s.tick_period_ms);
                self.end_session(None);
                self.send(&Message::ResetReq);
                if let Some(c) = self.conn.as_mut() {
                    c.reset_sent = Some(Instant::now());
                    c.hello = None;
                }
                self.set_state(LinkState::Handshake, "Redémarrage de la carte demandé (RESET_REQ)…".into());
            }
            Command::Shutdown => {}
        }
    }

    fn start_session(&mut self, tick_period_ms: u16) {
        if self.conn.is_none() {
            return self.error("session", "Connectez d'abord une carte.".into());
        }
        if tick_period_ms > 1000 {
            return self.error("session", format!("Période de tick {tick_period_ms} ms hors limites (≤ 1000)."));
        }
        match self.session.as_ref().map(|s| s.phase) {
            Some(Phase::Closing { .. }) | Some(Phase::Stopping { .. }) => {
                self.pending_start = Some(tick_period_ms);
                return;
            }
            Some(_) => return,
            None => {}
        }
        if self.stalled || !matches!(self.state, LinkState::Ready | LinkState::StudentDisabled) {
            return self.error("session", format!("Impossible de démarrer une session : {}", self.message));
        }
        let id = loop {
            let v = u32::from_le_bytes(random_bytes());
            if v != 0 {
                break v;
            }
        };
        self.next_epoch += 1;
        self.acc = SessionAcc::new();
        self.slots.clear();
        self.slots_dirty = true;
        self.fault = None;
        let now = Instant::now();
        self.send(&Message::SessionStart(SessionStart { session_id: id, mode: 1, tick_period_ms }));
        self.session = Some(Session {
            id,
            mode: 1,
            tick_period_ms,
            phase: Phase::Starting { sent_at: now, tries: 1 },
            started: now,
            epoch: self.next_epoch,
            last_frame_seq: 0,
            max_acked: 0,
            inflight: VecDeque::new(),
            pending: None,
            out_of_session_reported: false,
        });
        self.message = format!("Ouverture de la session {id:08X}…");
        self.status_dirty = true;
    }

    fn stop_session(&mut self) {
        let Some(s) = self.session.as_mut() else { return };
        if !matches!(s.phase, Phase::Active | Phase::Starting { .. }) {
            return;
        }
        let id = s.id;
        s.phase = Phase::Stopping { sent_at: Instant::now(), tries: 1 };
        s.pending = None;
        self.sim.stop();
        self.send(&Message::SessionStop { session_id: id });
        self.message = format!("Arrêt de la session {id:08X}…");
        self.status_dirty = true;
    }

    /// Ends the session locally: frames still in flight are reported as aborted.
    fn end_session(&mut self, why: Option<String>) {
        self.sim.stop();
        self.pending_start = None;
        if let Some(s) = self.session.take() {
            for f in s.inflight {
                self.acc.aborted += 1;
                let mut row = f.row;
                row.outcome = "aborted";
                self.out_rows.push(row);
            }
            if let Some(w) = why {
                self.error("session", w);
            }
        }
        self.status_dirty = true;
    }

    // --- Port management ----------------------------------------------------------------------

    fn rescan(&mut self) {
        match self.connector.scan() {
            Ok(ports) => {
                if ports != self.ports {
                    self.ports = ports;
                    self.status_dirty = true;
                }
            }
            Err(e) => self.error("port", e),
        }
        self.last_scan = Some(Instant::now());
    }

    fn resolve(&self, t: &Target) -> Option<PortInfo> {
        if let Some(sn) = &t.serial {
            // The port name may change after a re-plug; the probe serial number does not.
            return self.ports.iter().find(|p| p.serial.as_deref() == Some(sn.as_str())).cloned();
        }
        let name = t.port.as_ref()?;
        self.ports.iter().find(|p| &p.port == name).cloned()
    }

    fn idle(&mut self) {
        let now = Instant::now();
        if self.last_scan.map_or(true, |t| now - t >= SCAN_PERIOD) {
            self.rescan();
        }
        if self.auto_connect && self.target.is_none() {
            let boards: Vec<&PortInfo> = self.ports.iter().filter(|p| p.is_stlink()).collect();
            match boards.as_slice() {
                [one] => {
                    self.target = Some(Target { serial: one.serial.clone(), port: Some(one.port.clone()) });
                    self.auto_connect = false;
                    self.retry_at = now;
                }
                [] => self.set_state(
                    LinkState::Absent,
                    "Aucune carte détectée : branchez la NUCLEO-H753ZI par son connecteur ST-LINK (CN1).".into(),
                ),
                _ => {
                    self.auto_connect = false;
                    self.set_state(LinkState::Disconnected, "Plusieurs cartes détectées : choisissez-en une puis « Connecter ».".into())
                }
            }
        }
        if self.target.is_some() && now >= self.retry_at {
            self.try_open(now);
        }
    }

    fn try_open(&mut self, now: Instant) {
        self.rescan();
        let Some(target) = self.target.clone() else { return };
        let Some(info) = self.resolve(&target) else {
            if self.state != LinkState::Lost {
                self.set_state(
                    LinkState::Absent,
                    format!("Carte {} non détectée : vérifiez le câble USB (connecteur ST-LINK CN1).", target.label()),
                );
            }
            self.retry_at = now + if self.state == LinkState::Lost { T_LOST_RETRY } else { T_ABSENT_RETRY };
            return;
        };
        let port = info.port.clone();
        match self.connector.open(&port) {
            Ok(mut io) => {
                io.clear_input();
                let mut rx = n7rt_link::Receiver::new(n7rt_link::Direction::BoardToPc);
                rx.reset();
                self.conn = Some(Conn {
                    io,
                    port: info,
                    rx,
                    enc: Encoder::new(),
                    last_rx: now,
                    last_hb_tx: now,
                    nonce: [0; 16],
                    hello: None,
                    board_seq: None,
                    reset_sent: None,
                    synced: false,
                    foreign_acks: 0,
                });
                self.stalled = false;
                self.busy = false;
                self.board = None;
                self.hb = None;
                self.fault = None;
                // Spec 4.3: a 0x00 first purges any partial frame on the board side.
                self.txbuf.push(n7rt_link::DELIMITER);
                self.send_hello(true);
                self.set_state(LinkState::Handshake, format!("Port {port} ouvert : poignée de main N7RT-Link…"));
            }
            Err(OpenError::Busy(_)) => {
                if self.state != LinkState::PortBusy {
                    self.error("port", format!("Port {port} occupé par une autre application."));
                }
                self.set_state(
                    LinkState::PortBusy,
                    format!(
                        "Le port {port} est déjà ouvert par une autre application (console série de STM32CubeIDE, \
                         autre instance du CAN Lab, moniteur série…). Fermez-la : la connexion reprendra seule."
                    ),
                );
                self.retry_at = now + T_ABSENT_RETRY;
            }
            Err(OpenError::Permission(d)) => {
                self.set_state(
                    LinkState::PortBusy,
                    format!(
                        "Accès refusé au port {port} ({d}). Sous Linux, ajoutez votre compte au groupe « dialout » \
                         (ou « uucp »), puis rouvrez votre session."
                    ),
                );
                self.retry_at = now + T_SILENT_RETRY;
            }
            Err(OpenError::Missing(_)) => {
                self.set_state(LinkState::Absent, format!("Port {port} indisponible : nouvelle tentative…"));
                self.retry_at = now + T_LOST_RETRY;
            }
            Err(OpenError::Other(d)) => {
                self.set_state(LinkState::Absent, format!("Ouverture de {port} impossible : {d}"));
                self.retry_at = now + T_ABSENT_RETRY;
            }
        }
    }

    fn close_conn(&mut self, state: LinkState, message: String, graceful: bool) {
        if graceful && self.conn.is_some() {
            if let Some(s) = self.session.as_ref().filter(|s| matches!(s.phase, Phase::Active | Phase::Starting { .. })) {
                let id = s.id;
                self.sim.stop();
                self.send(&Message::SessionStop { session_id: id });
                self.flush();
                if let Some(s) = self.session.as_mut() {
                    s.phase = Phase::Stopping { sent_at: Instant::now(), tries: SESSION_TRIES };
                }
                // Let the board close the session cleanly before the port goes away.
                let deadline = Instant::now() + CLOSE_ACK_WAIT;
                while Instant::now() < deadline
                    && self.conn.is_some()
                    && matches!(self.session.as_ref().map(|s| s.phase), Some(Phase::Stopping { .. }))
                {
                    self.pump_rx();
                }
            }
        }
        self.end_session(None);
        self.conn = None;
        self.txbuf.clear();
        self.tx_stuck_since = None;
        self.stalled = false;
        self.busy = false;
        self.board = None;
        self.hb = None;
        self.set_state(state, message);
    }

    fn on_port_error(&mut self, e: io::Error) {
        let port = self.conn.as_ref().map(|c| c.port.port.clone()).unwrap_or_default();
        if let Some(s) = self.session.as_ref().filter(|s| s.phase == Phase::Active) {
            self.resume = Some(s.tick_period_ms);
        }
        self.end_session(Some(format!("Port {port} perdu : session interrompue.")));
        self.error("port", format!("Port {port} perdu ({e})."));
        self.conn = None;
        self.txbuf.clear();
        self.tx_stuck_since = None;
        self.close_conn(
            LinkState::Lost,
            format!("Liaison perdue sur {port} (carte débranchée ?) : reconnexion automatique dès son retour…"),
            false,
        );
        self.retry_at = Instant::now() + T_LOST_RETRY;
    }

    // --- Transmission ------------------------------------------------------------------------

    /// Encodes into the transmit buffer; returns the header `seq`.
    fn send(&mut self, msg: &Message) -> Option<u16> {
        let conn = self.conn.as_mut()?;
        match conn.enc.encode(msg) {
            Ok((seq, wire)) => {
                self.txbuf.extend_from_slice(&wire);
                Some(seq)
            }
            Err(e) => {
                let name = msg.name();
                self.error("link", format!("Message {name} invalide, non envoyé : {e}"));
                None
            }
        }
    }

    fn send_hello(&mut self, new_nonce: bool) {
        let now = Instant::now();
        let Some(conn) = self.conn.as_mut() else { return };
        if new_nonce {
            conn.nonce = random_bytes();
            conn.hello = None;
        }
        let tries = conn.hello.map_or(0, |(_, t)| t) + 1;
        conn.hello = Some((now, tries));
        let nonce = conn.nonce;
        self.send(&Message::Hello(Hello { proto_min: runtime::PROTO_MIN, proto_max: runtime::PROTO_MAX, nonce }));
    }

    /// Writes what the port accepts within `WRITE_SLICE`; the rest stays queued (credits stop
    /// the producer meanwhile). Bytes are never dropped, so frames are never truncated.
    fn flush(&mut self) {
        if self.txbuf.is_empty() {
            self.tx_stuck_since = None;
            return;
        }
        let Some(conn) = self.conn.as_mut() else {
            self.txbuf.clear();
            return;
        };
        let started = Instant::now();
        let exact = conn.io.exact_partial_writes();
        let mut off = 0;
        let mut failure = None;
        while off < self.txbuf.len() && started.elapsed() < WRITE_SLICE {
            let short = |what: &str| io::Error::new(io::ErrorKind::TimedOut, format!("écriture série incomplète ({what})"));
            match conn.io.write(&self.txbuf[off..]) {
                Ok(n) if n == self.txbuf.len() - off || (n > 0 && exact) => off += n,
                Ok(_) if exact => {}
                Err(e) if transient(&e) && exact => {}
                Ok(n) => {
                    failure = Some(short(&format!("{n} octets sur {} acceptés en 2 s", self.txbuf.len() - off)));
                    break;
                }
                Err(e) if transient(&e) => {
                    failure = Some(short(&e.to_string()));
                    break;
                }
                Err(e) => {
                    failure = Some(e);
                    break;
                }
            }
        }
        self.lacc.bytes_tx += off as u64;
        self.lacc.write_max_us = self.lacc.write_max_us.max(started.elapsed().as_micros() as u64);
        self.lacc.tx_backlog_max = self.lacc.tx_backlog_max.max(self.txbuf.len() - off);
        self.txbuf.drain(..off);
        if let Some(e) = failure {
            return self.on_port_error(e);
        }
        if self.txbuf.is_empty() || off > 0 {
            self.tx_stuck_since = None;
            return;
        }
        let since = *self.tx_stuck_since.get_or_insert(started);
        if started - since >= WRITE_STALL_LIMIT || self.txbuf.len() > TX_BACKLOG_MAX {
            let e = io::Error::new(io::ErrorKind::TimedOut, "le port n'accepte plus de données depuis 3 s");
            self.on_port_error(e);
        }
    }

    fn pump_tx(&mut self) {
        // Spec 10: in STALLED the PC suspends every send (frames, SIM_TIME, HEARTBEAT).
        if self.stalled {
            return;
        }
        let now = Instant::now();
        let hb_due = self.conn.as_ref().is_some_and(|c| c.hello.is_none() && c.reset_sent.is_none() && now - c.last_hb_tx >= T_HB_TX);
        if hb_due {
            let uptime = now.duration_since(self.t0).as_millis() as u32;
            self.send(&Message::PcHeartbeat { host_uptime_ms: uptime });
            if let Some(c) = self.conn.as_mut() {
                c.last_hb_tx = now;
            }
        }
        let active = matches!(self.session.as_ref().map(|s| s.phase), Some(Phase::Active));
        if !active {
            // No session: items of a finished session are stale.
            while self.bus_rx.try_recv().is_ok() {
                self.sim.consumed();
            }
            return;
        }
        let Engine { session, conn, txbuf, bus_rx, sim, acc, window, check, .. } = self;
        let (Some(s), Some(conn)) = (session.as_mut(), conn.as_mut()) else { return };
        while txbuf.len() < TX_BATCH {
            let item = match s.pending.take() {
                Some(i) => i,
                None => match bus_rx.try_recv() {
                    Ok(i) => {
                        sim.consumed();
                        i
                    }
                    Err(_) => break,
                },
            };
            if item.epoch != s.epoch {
                continue;
            }
            match item.kind {
                BusKind::Time(t_us) => {
                    if let Ok((_, wire)) = conn.enc.encode(&Message::SimTime { t_us }) {
                        txbuf.extend_from_slice(&wire);
                        acc.sim_time_sent += 1;
                    }
                }
                BusKind::Frame(g) => {
                    if in_flight(s) >= *window {
                        s.pending = Some(BusItem { epoch: item.epoch, kind: BusKind::Frame(g) });
                        acc.credit_waits += 1;
                        break;
                    }
                    let frame_seq = s.last_frame_seq + 1;
                    let msg = Message::VcanFrame(VcanFrame {
                        frame_seq,
                        t_us: g.t_us,
                        can_id: g.can_id,
                        flags: g.flags,
                        dlc: g.dlc,
                        bus: 0,
                        data: g.data.clone(),
                    });
                    let Ok((seq, wire)) = conn.enc.encode(&msg) else { continue };
                    txbuf.extend_from_slice(&wire);
                    check.frame(s.id, &g);
                    s.last_frame_seq = frame_seq;
                    s.inflight.push_back(InFlight {
                        frame_seq,
                        seq,
                        sent_at: now,
                        overdue_since: None,
                        row: FrameRow {
                            session_id: s.id,
                            frame_seq,
                            t_us: g.t_us,
                            can_id: g.can_id,
                            flags: g.flags,
                            dlc: g.dlc,
                            data: runtime::hex(&g.data),
                            outcome: "pending",
                            status: 0,
                            result_flags: 0,
                            cycles: 0,
                            entries: Vec::new(),
                            nack: None,
                            rtt_us: None,
                            stim_rev: g.stim_rev,
                        },
                    });
                    acc.frames_sent += 1;
                    acc.tx_rate.add(now, 1);
                    acc.in_flight_max = acc.in_flight_max.max(in_flight(s));
                }
            }
        }
    }

    // --- Reception ----------------------------------------------------------------------------

    fn pump_rx(&mut self) {
        let mut buf = [0u8; 4096];
        let Some(conn) = self.conn.as_mut() else { return };
        let mut events = Vec::new();
        let n = match conn.io.read(&mut buf) {
            Ok(n) => n,
            Err(e) if transient(&e) => 0,
            Err(e) => return self.on_port_error(e),
        };
        if n == 0 {
            return;
        }
        conn.rx.feed(&buf[..n], |e| events.push(e));
        self.lacc.bytes_rx += n as u64;
        self.lacc.read_chunk_max = self.lacc.read_chunk_max.max(n);
        let now = Instant::now();
        for e in events {
            if self.conn.is_none() {
                break;
            }
            self.on_event(e, now);
        }
    }

    fn on_event(&mut self, e: Event, now: Instant) {
        let synced = self.conn.as_ref().is_some_and(|c| c.synced);
        if !synced && !matches!(&e, Event::Packet(p) if matches!(p.message, Message::HelloAck(_))) {
            // Left over from before this connection (spec 4.3: the receiver resynchronises).
            self.lacc.pre_sync_discarded += 1;
            return;
        }
        match e {
            Event::FrameError(f) => {
                *self.lacc.rx_framing.entry(f.name().to_owned()).or_default() += 1;
                self.error("framing", format!("Trame illisible reçue de la carte ({}) : ignorée, resynchronisation.", f.name()));
            }
            Event::Reject { code, type_, seq, reason } => {
                self.lacc.rx_rejects += 1;
                self.error("reject", format!("Paquet de la carte refusé ({}, type 0x{type_:02X}, seq {seq}) : {reason}.", code_name(code)));
            }
            Event::Packet(p) => self.on_packet(p, now),
        }
    }

    fn on_packet(&mut self, p: Packet, now: Instant) {
        let boot = matches!(&p.message, Message::HelloAck(a) if a.reason == 1);
        let mut gap = None;
        let seq = p.seq;
        let was_synced = self.conn.as_ref().is_some_and(|c| c.synced);
        if let Some(conn) = self.conn.as_mut().filter(|c| c.synced) {
            conn.last_rx = now;
            if let Some(prev) = conn.board_seq {
                if !boot && p.seq != prev.wrapping_add(1) {
                    self.lacc.board_seq_gaps += 1;
                    gap = Some((prev.wrapping_add(1), p.seq));
                }
            }
            conn.board_seq = Some(p.seq);
        }
        if let Some((want, got)) = gap {
            let what = p.message.name();
            self.error("link", format!("Numérotation de la carte : seq {want} attendu, {got} reçu ({what}) : paquet(s) de la carte perdu(s)."));
        }
        if self.stalled {
            self.stalled = false;
            self.unstalled_at = now;
            self.status_dirty = true;
            // The board drains its queue at once on resume: what is still unanswered after the
            // grace period never reached it (UART overrun while the core was halted).
            if let Some(s) = self.session.as_mut() {
                for f in s.inflight.iter_mut() {
                    f.overdue_since.get_or_insert(now);
                }
            }
            self.error("info", "La carte répond de nouveau : reprise des envois.".into());
        }
        match p.message {
            Message::HelloAck(a) => self.on_hello_ack(a),
            Message::SessionAck(a) => self.on_session_ack(a, now),
            Message::Result(r) => self.on_result(r, now),
            Message::BoardNack(n) => self.on_nack(n, now),
            Message::TickResult(t) => self.on_tick(t),
            Message::StudentLog(l) => self.on_log(l),
            Message::BoardHeartbeat(h) => self.on_heartbeat(h, now),
            Message::FaultReport(f) => self.on_fault(f),
            // PC -> board types are rejected by the receiver (E_UNKNOWN_TYPE) before this point.
            _ => {}
        }
        if !was_synced {
            // Numbering is followed from this connection's HELLO_ACK on.
            if let Some(conn) = self.conn.as_mut().filter(|c| c.synced) {
                conn.board_seq = Some(seq);
                conn.last_rx = now;
            }
        }
    }

    fn on_hello_ack(&mut self, a: HelloAck) {
        if a.reason == 1 {
            // Spec 9.6: the board rebooted. The session is over and in-flight frames are gone.
            let interrupted = self.session.as_ref().filter(|s| s.phase == Phase::Active).map(|s| s.tick_period_ms);
            if interrupted.is_some() {
                self.resume = interrupted;
            }
            let expected = self.conn.as_ref().is_some_and(|c| c.reset_sent.is_some());
            self.end_session(interrupted.map(|_| "La carte a redémarré : session terminée.".to_owned()));
            let ident = runtime::assess(&a, &[0u8; 16]);
            if !expected {
                self.error("session", format!("La carte a redémarré ({}).", ident.reset_causes.join(", ")));
            }
            if let Some(c) = self.conn.as_mut() {
                c.reset_sent = None;
                c.synced = true;
            }
            self.board = Some(ident);
            self.send_hello(true);
            self.set_state(LinkState::Handshake, "Carte redémarrée : nouvelle poignée de main…".into());
            return;
        }
        let Some(conn) = self.conn.as_mut() else { return };
        let ident = runtime::assess(&a, &conn.nonce);
        if !ident.runtime_verified && conn.hello.is_some() && conn.foreign_acks < 2 {
            // Probably the answer to a HELLO of an earlier connection, still buffered: wait for
            // the answer to ours. A runtime that keeps failing the challenge is refused below.
            conn.foreign_acks += 1;
            return;
        }
        conn.hello = None;
        conn.synced = true;
        self.window = a.rx_window.max(1) as u32;
        self.student_status = a.student_status;
        let in_session = self.session.is_some();
        self.board = Some(ident.clone());
        if !ident.compatible {
            self.resume = None;
            return self.set_state(LinkState::Incompatible, format!("Carte incompatible : {}.", ident.problems.join(" ; ")));
        }
        if in_session {
            return;
        }
        self.settle_ready();
        if a.fault_pending == 1 || a.student_status == 5 {
            // Spec 9.6: never restart a session automatically after a fault.
            self.resume = None;
        }
        if let Some(tick) = self.resume.take().or(self.pending_start.take()) {
            if self.state == LinkState::Ready {
                self.error("info", "Session relancée automatiquement après la reconnexion de la carte.".into());
                self.start_session(tick);
            }
        }
    }

    /// Board state when no session is open, from the last HELLO_ACK.
    fn settle_ready(&mut self) {
        let Some(b) = self.board.clone() else { return };
        let (state, msg) = match self.student_status {
            1 => (
                LinkState::Ready,
                format!("Carte prête : runtime {} vérifié, code étudiant chargé (CRC {}).", b.runtime, b.student_crc32),
            ),
            5 => (
                LinkState::StudentDisabled,
                "Code étudiant désactivé après une faute : corrigez-le, ou démarrez une session pour le réactiver.".into(),
            ),
            _ => (
                LinkState::NoStudent,
                format!(
                    "Runtime vérifié, mais {} : compilez et flashez votre projet depuis STM32CubeIDE.",
                    b.student_status_text
                ),
            ),
        };
        self.set_state(state, msg);
    }

    fn on_session_ack(&mut self, a: SessionAck, now: Instant) {
        let Some(s) = self.session.as_mut() else {
            return self.error("session", format!("SESSION_ACK inattendu (session {:08X}).", a.session_id));
        };
        if a.session_id != s.id {
            let id = a.session_id;
            return self.error("session", format!("SESSION_ACK d'une autre session ({id:08X}) ignoré."));
        }
        match (a.op, s.phase) {
            (1, Phase::Starting { .. }) => match a.status {
                0 => {
                    s.phase = Phase::Active;
                    let (epoch, tick, id) = (s.epoch, s.tick_period_ms, s.id);
                    self.sim.start(epoch, tick);
                    self.set_state(LinkState::Session, format!("Session {id:08X} active : le bus virtuel tourne."));
                }
                status => {
                    self.session = None;
                    self.student_status = if status == 1 { 0 } else { self.student_status };
                    self.error("session", "La carte refuse la session : aucun code étudiant valide.".into());
                    self.settle_ready();
                }
            },
            (2, Phase::Stopping { .. }) => {
                s.phase = Phase::Closing { since: now };
                self.settle_ready();
                self.message = "Session arrêtée.".into();
            }
            // Duplicate answer to a retransmission.
            _ => {}
        }
    }

    fn complete(&mut self, mut f: InFlight, outcome: &'static str) {
        f.row.outcome = outcome;
        self.out_rows.push(f.row);
    }

    fn on_result(&mut self, r: FrameResult, now: Instant) {
        let Some(s) = self.session.as_mut() else { return };
        let Some(i) = s.inflight.iter().position(|f| f.frame_seq == r.frame_seq) else {
            if self.recently_lost.contains(&r.frame_seq) {
                self.acc.late += 1;
                return self.error("link", format!("Réponse tardive pour la trame {} (déjà comptée perdue).", r.frame_seq));
            }
            return self.error("link", format!("RESULT pour une trame inconnue ({}).", r.frame_seq));
        };
        // Answers come in execution order: older unanswered frames are overtaken.
        for f in s.inflight.iter_mut().take(i) {
            f.overdue_since.get_or_insert(now);
        }
        let mut f = s.inflight.remove(i).expect("index found above");
        s.max_acked = s.max_acked.max(r.frame_seq);
        let rtt = now.duration_since(f.sent_at).as_micros() as u32;
        f.row.status = r.status;
        f.row.result_flags = r.flags;
        f.row.cycles = r.cycles;
        f.row.entries = entry_views(&r.entries);
        f.row.rtt_us = Some(rtt);
        self.acc.results += 1;
        self.acc.rtt.record(rtt);
        self.acc.rx_rate.add(now, 1);
        self.update_slots(&r.entries, "frame", r.frame_seq as u64);
        self.complete(f, "result");
    }

    fn on_nack(&mut self, n: Nack, _now: Instant) {
        let name = code_name(n.code);
        if n.ref_type == ty::VCAN_FRAME {
            let found = self.session.as_mut().and_then(|s| {
                let i = s.inflight.iter().position(|f| f.seq == n.ref_seq)?;
                let f = s.inflight.remove(i)?;
                s.max_acked = s.max_acked.max(f.frame_seq);
                Some(f)
            });
            match found {
                Some(mut f) => {
                    self.acc.nacks += 1;
                    *self.acc.nack_codes.entry(name.clone()).or_default() += 1;
                    f.row.nack = Some(name.clone());
                    let fs = f.frame_seq;
                    self.complete(f, "nack");
                    self.error("nack", format!("Trame {fs} refusée par la carte : {name}."));
                }
                None => self.error("nack", format!("NACK {name} pour une trame inconnue (seq {}).", n.ref_seq)),
            }
            return;
        }
        self.acc.nacks += 1;
        *self.acc.nack_codes.entry(name.clone()).or_default() += 1;
        let what = message_name(n7rt_link::Direction::PcToBoard, n.ref_type).unwrap_or("?");
        self.error("nack", format!("La carte a refusé {what} (seq {}) : {name}.", n.ref_seq));
    }

    fn on_tick(&mut self, t: TickResult) {
        self.acc.tick_results += 1;
        if let Some(s) = self.session.as_ref() {
            self.check.tick(s.id, &t);
        }
        self.update_slots(&t.entries, "tick", t.t_us);
    }

    fn on_log(&mut self, l: StudentLog) {
        self.acc.logs += 1;
        let session_id = self.session.as_ref().map_or(0, |s| s.id);
        self.out_logs.push(LogRow { session_id, t_us: l.t_us, text: l.text });
    }

    fn on_heartbeat(&mut self, h: BoardHeartbeat, now: Instant) {
        let busy = h.hook_running_ms > T_BUSY_MS;
        if busy != self.busy {
            self.busy = busy;
            if busy {
                self.error(
                    "session",
                    format!("Le code étudiant s'exécute depuis {} ms (boucle infinie ?).", h.hook_running_ms),
                );
            }
        }
        let mut out_of_session = false;
        if let Some(s) = self.session.as_mut() {
            if h.acked_frame_seq <= s.last_frame_seq && h.acked_frame_seq > s.max_acked {
                s.max_acked = h.acked_frame_seq;
            }
            for f in s.inflight.iter_mut().filter(|f| f.frame_seq <= h.acked_frame_seq) {
                f.overdue_since.get_or_insert(now);
            }
            if s.phase == Phase::Active && h.state == 0 && !s.out_of_session_reported {
                s.out_of_session_reported = true;
                out_of_session = true;
            }
        }
        if out_of_session {
            self.error("session", "La carte indique qu'elle n'est plus en session.".into());
        }
        self.hb = Some(HeartbeatView {
            uptime_ms: h.uptime_ms,
            acked_frame_seq: h.acked_frame_seq,
            hook_running_ms: h.hook_running_ms,
            state: h.state,
            student_disabled: h.student_disabled == 1,
            rx_frames: h.rx_frames,
            rx_seq_gaps: h.rx_seq_gaps,
            rx_framing_errors: h.rx_framing_errors,
            rx_crc_errors: h.rx_crc_errors,
            rx_overruns: h.rx_overruns,
            log_dropped: h.log_dropped,
            queue_high_water: h.queue_high_water,
            max_cycles: h.max_cycles,
        });
        self.status_dirty = true;
    }

    fn on_fault(&mut self, f: FaultReport) {
        let cause = match f.cause {
            1 => "faute processeur",
            2 => "watchdog",
            _ => "faute",
        };
        let hook = match f.hook {
            1 => " dans student_init()".to_owned(),
            2 => format!(" dans student_on_frame() (trame {})", f.frame_seq),
            3 => " dans student_on_tick()".to_owned(),
            _ => String::new(),
        };
        let text = format!(
            "{cause}{hook} : PC 0x{:08X}, LR 0x{:08X}, CFSR 0x{:08X}, HFSR 0x{:08X}, MMFAR 0x{:08X}, BFAR 0x{:08X}",
            f.pc, f.lr, f.cfsr, f.hfsr, f.mmfar, f.bfar
        );
        self.error("fault", format!("Rapport de faute de la carte : {text}."));
        self.fault = Some(text);
        self.status_dirty = true;
    }

    fn update_slots(&mut self, entries: &[Entry], source: &'static str, at: u64) {
        for e in entries {
            let v = self.slots.entry(e.slot).or_insert(SlotView { slot: e.slot, kind: 0, bits: 0, source, at, updates: 0 });
            v.kind = e.kind;
            v.bits = e.value.to_bits();
            v.source = source;
            v.at = at;
            v.updates += 1;
            self.slots_dirty = true;
        }
    }

    // --- Timers -------------------------------------------------------------------------------

    fn timers(&mut self) {
        let now = Instant::now();
        let Some(conn) = self.conn.as_ref() else { return };
        let port = conn.port.port.clone();
        if let Some(sent) = conn.reset_sent {
            if now - sent >= T_RESET {
                self.close_conn(LinkState::Lost, format!("La carte sur {port} ne s'est pas annoncée après RESET_REQ."), false);
                self.retry_at = now + T_LOST_RETRY;
            }
            return;
        }
        if let Some((sent, tries)) = conn.hello {
            if now - sent >= T_HELLO {
                if tries < HELLO_TRIES {
                    self.send_hello(false);
                } else {
                    self.close_conn(
                        LinkState::Absent,
                        format!(
                            "Aucune réponse N7RT-Link sur {port} : la carte exécute-t-elle une image N7RT \
                             (runtime + code étudiant) ? Nouvel essai dans 2 s."
                        ),
                        false,
                    );
                    self.retry_at = now + T_SILENT_RETRY;
                }
            }
            return;
        }
        if !self.stalled && now - conn.last_rx >= T_STALL {
            self.stalled = true;
            self.status_dirty = true;
            self.error(
                "link",
                "Carte muette depuis 750 ms (cœur arrêté par un point d'arrêt ?) : envois suspendus.".into(),
            );
        }
        self.session_timers(now);
    }

    fn session_timers(&mut self, now: Instant) {
        let Some(s) = self.session.as_mut() else { return };
        match s.phase {
            Phase::Starting { sent_at, tries } if now - sent_at >= T_SESSION => {
                if tries < SESSION_TRIES {
                    s.phase = Phase::Starting { sent_at: now, tries: tries + 1 };
                    let (id, mode, tick_period_ms) = (s.id, s.mode, s.tick_period_ms);
                    self.send(&Message::SessionStart(SessionStart { session_id: id, mode, tick_period_ms }));
                } else {
                    self.session = None;
                    self.error("session", "La carte n'a pas confirmé l'ouverture de session (4 essais).".into());
                    self.settle_ready();
                }
                return;
            }
            Phase::Stopping { sent_at, tries } if now - sent_at >= T_SESSION => {
                if tries < SESSION_TRIES {
                    s.phase = Phase::Stopping { sent_at: now, tries: tries + 1 };
                    let id = s.id;
                    self.send(&Message::SessionStop { session_id: id });
                } else {
                    s.phase = Phase::Closing { since: now };
                    self.error("session", "La carte n'a pas confirmé l'arrêt de session (4 essais).".into());
                    self.settle_ready();
                }
                return;
            }
            _ => {}
        }
        // Lost frames: overtaken (or acknowledged by heartbeat) and still unanswered after a
        // grace period, or without any answer for 5 s of non-stalled time.
        let suspended = self.stalled || self.busy;
        let since = self.unstalled_at;
        let mut lost = Vec::new();
        if !suspended {
            let mut i = 0;
            while i < s.inflight.len() {
                let f = &s.inflight[i];
                let overdue = f.overdue_since.is_some_and(|t| now - t >= LATE_GRACE);
                let silent = now - f.sent_at.max(since) >= NO_ANSWER_TIMEOUT;
                if overdue || silent {
                    lost.push(s.inflight.remove(i).expect("index in range"));
                } else {
                    i += 1;
                }
            }
        }
        let closing_done = match s.phase {
            Phase::Closing { since } => s.inflight.is_empty() || now - since >= CLOSING_TIMEOUT,
            _ => false,
        };
        if let (Some(first), Some(last)) = (lost.first().map(|f| f.frame_seq), lost.last().map(|f| f.frame_seq)) {
            let n = lost.len();
            let which = if n == 1 { format!("Trame {first}") } else { format!("{n} trames ({first} à {last})") };
            self.error("link", format!("{which} sans réponse de la carte : comptée(s) perdue(s)."));
        }
        for f in lost {
            self.acc.lost += 1;
            self.recently_lost.push_back(f.frame_seq);
            if self.recently_lost.len() > 256 {
                self.recently_lost.pop_front();
            }
            self.complete(f, "lost");
        }
        if closing_done {
            if let Some(s) = self.session.take() {
                for f in s.inflight {
                    self.acc.lost += 1;
                    self.complete(f, "lost");
                }
            }
            self.status_dirty = true;
            if let Some(tick) = self.pending_start.take() {
                self.start_session(tick);
            }
        }
    }

    // --- Publication to the UI ---------------------------------------------------------------

    fn set_state(&mut self, state: LinkState, message: String) {
        if self.state != state || self.message != message {
            self.state = state;
            self.message = message;
            self.status_dirty = true;
        }
    }

    fn error(&mut self, kind: &'static str, text: String) {
        let at_ms = self.t0.elapsed().as_millis() as u64;
        self.out_errors.push(ErrorRow { at_ms, kind, text });
    }

    fn status_view(&self, now: Instant) -> Status {
        let (stimuli, stim_rev) = self.sim.stimuli();
        let session = self.session.as_ref().map(|s| SessionView {
            id: s.id,
            phase: match s.phase {
                Phase::Starting { .. } => "starting",
                Phase::Active => "active",
                Phase::Stopping { .. } => "stopping",
                Phase::Closing { .. } => "closing",
            },
            mode: s.mode,
            tick_period_ms: s.tick_period_ms,
            elapsed_ms: now.duration_since(s.started).as_millis() as u64,
            bus_time_us: self.sim.bus_time_us.load(std::sync::atomic::Ordering::Relaxed),
        });
        let in_session = self.session.is_some();
        Status {
            state: self.state,
            stalled: self.stalled,
            student_busy: self.busy,
            message: self.message.clone(),
            target: self.target.as_ref().map(Target::label),
            port: self.conn.as_ref().map(|c| c.port.clone()),
            ports: self.ports.clone(),
            board: self.board.clone(),
            session,
            heartbeat: self.hb.clone(),
            fault: self.fault.clone(),
            stimuli,
            stim_rev,
            can_start: self.conn.is_some()
                && !in_session
                && !self.stalled
                && matches!(self.state, LinkState::Ready | LinkState::StudentDisabled),
            can_stop: matches!(self.session.as_ref().map(|s| s.phase), Some(Phase::Active | Phase::Starting { .. })),
        }
    }

    fn stats_view(&mut self, now: Instant) -> Stats {
        use std::sync::atomic::Ordering::Relaxed;
        let in_flight = self.session.as_ref().map_or(0, in_flight);
        let a = &mut self.acc;
        Stats {
            frames_sent: a.frames_sent,
            results: a.results,
            nacks: a.nacks,
            nack_codes: a.nack_codes.clone(),
            lost: a.lost,
            late: a.late,
            aborted: a.aborted,
            in_flight,
            in_flight_max: a.in_flight_max,
            window: self.window,
            credit_waits: a.credit_waits,
            tx_fps: a.tx_rate.per_second(now),
            rx_fps: a.rx_rate.per_second(now),
            rtt_count: a.rtt.count(),
            rtt_p50_us: a.rtt.quantile(0.50),
            rtt_p95_us: a.rtt.quantile(0.95),
            rtt_p99_us: a.rtt.quantile(0.99),
            rtt_max_us: a.rtt.max(),
            rtt_mean_us: a.rtt.mean(),
            sim_time_sent: a.sim_time_sent,
            tick_results: a.tick_results,
            logs: a.logs,
            rx_framing: self.lacc.rx_framing.clone(),
            rx_rejects: self.lacc.rx_rejects,
            board_seq_gaps: self.lacc.board_seq_gaps,
            pre_sync_discarded: self.lacc.pre_sync_discarded,
            bytes_tx: self.lacc.bytes_tx,
            bytes_rx: self.lacc.bytes_rx,
            queues: QueueStats {
                sim_depth: self.sim.depth.load(Relaxed),
                sim_high_water: self.sim.high_water.load(Relaxed),
                sim_capacity: sim::QUEUE_CAPACITY,
                sim_paused_ms: self.sim.paused_us.load(Relaxed) / 1000,
                sim_dropped: self.sim.dropped.load(Relaxed),
                sim_generated: self.sim.generated.load(Relaxed),
                sim_max_step_us: self.sim.max_step_us.load(Relaxed),
                link_loop_max_us: self.lacc.loop_max_us,
                write_max_us: self.lacc.write_max_us,
                read_chunk_max: self.lacc.read_chunk_max,
                tx_backlog_max: self.lacc.tx_backlog_max,
                ..QueueStats::default()
            },
        }
    }

    fn publish(&mut self) {
        let now = Instant::now();
        let status_due = self.status_dirty || now - self.last_status_pub >= STATUS_PERIOD;
        let has_items = !self.out_rows.is_empty() || !self.out_logs.is_empty() || !self.out_errors.is_empty() || self.slots_dirty;
        if !status_due && !has_items {
            return;
        }
        let views = status_due.then(|| (self.status_view(now), self.stats_view(now)));
        // The session of a check is never shown: neither its frames nor its slots (CheckRecorder).
        let masked = self.check.masked();
        let hide_slots = self.check.is_active() || (masked.is_some() && self.session.as_ref().map(|s| s.id) == masked);
        let mut g = self.shared.lock().unwrap();
        for r in self.out_rows.drain(..) {
            if let Some(rec) = g.record.as_mut() {
                rec.row(&r);
            }
            if Some(r.session_id) == masked {
                continue;
            }
            if g.recent.back().is_some_and(|b| b.session_id != r.session_id) {
                g.recent.clear();
            }
            if g.recent.len() == RECENT_CAP {
                g.recent.pop_front();
            }
            g.recent.push_back(r.clone());
            while g.recent.front().is_some_and(|f| f.t_us + RECENT_US < r.t_us) {
                g.recent.pop_front();
            }
            if g.rows.len() == UI_ROWS_CAP {
                g.rows.pop_front();
                g.rows_not_shown += 1;
            }
            g.rows.push_back(r);
        }
        for l in self.out_logs.drain(..) {
            if let Some(rec) = g.record.as_mut() {
                rec.log(&l);
            }
            if g.logs.len() == UI_LOGS_CAP {
                g.logs.pop_front();
                g.logs_dropped += 1;
            }
            g.logs.push_back(l);
        }
        for e in self.out_errors.drain(..) {
            if g.errors.len() == UI_ERRORS_CAP {
                g.errors.pop_front();
                g.errors_dropped += 1;
            }
            g.errors.push_back(e);
        }
        if hide_slots {
            g.slots.clear();
            self.slots_dirty = false;
        } else if self.slots_dirty {
            g.slots = self.slots.values().cloned().collect();
            self.slots_dirty = false;
        }
        if let Some((status, stats)) = views {
            g.status = status;
            g.stats = stats;
            self.status_dirty = false;
            self.last_status_pub = now;
            self.lacc.loop_max_us = 0;
        }
        g.rev += 1;
    }
}
