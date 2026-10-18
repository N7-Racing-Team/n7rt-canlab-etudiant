//! What the core publishes to the UI. Plain data, serialised to JSON by the Tauri layer.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::discovery::PortInfo;
use crate::runtime::BoardIdentity;
use crate::sim::Stimuli;

/// PC-side link state (spec section 10). `STALLED` and `STUDENT_BUSY` are overlays
/// (`Status::stalled`, `Status::student_busy`): the state underneath is kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LinkState {
    /// The user asked not to be connected.
    #[default]
    Disconnected,
    /// No compatible port (or no N7RT-Link answer on it).
    Absent,
    /// The port exists but another application holds it.
    PortBusy,
    Handshake,
    Ready,
    NoStudent,
    StudentDisabled,
    Incompatible,
    Session,
    /// The port disappeared (unplugged); reopening is retried every 500 ms.
    Lost,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SessionView {
    pub id: u32,
    /// "starting" | "active" | "stopping" | "closing"
    pub phase: &'static str,
    pub mode: u8,
    pub tick_period_ms: u16,
    pub elapsed_ms: u64,
    pub bus_time_us: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct HeartbeatView {
    pub uptime_ms: u32,
    pub acked_frame_seq: u32,
    pub hook_running_ms: u16,
    pub state: u8,
    pub student_disabled: bool,
    pub rx_frames: u32,
    pub rx_seq_gaps: u16,
    pub rx_framing_errors: u16,
    pub rx_crc_errors: u16,
    pub rx_overruns: u16,
    pub log_dropped: u16,
    pub queue_high_water: u8,
    pub max_cycles: u32,
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct Status {
    pub state: LinkState,
    pub stalled: bool,
    pub student_busy: bool,
    /// One sentence describing the situation and, when relevant, what to do.
    pub message: String,
    /// What the user asked to connect to (probe serial or port), if anything.
    pub target: Option<String>,
    pub port: Option<PortInfo>,
    pub ports: Vec<PortInfo>,
    pub board: Option<BoardIdentity>,
    pub session: Option<SessionView>,
    pub heartbeat: Option<HeartbeatView>,
    /// Last FAULT_REPORT of this connection, decoded.
    pub fault: Option<String>,
    pub stimuli: Stimuli,
    pub stim_rev: u64,
    pub can_start: bool,
    pub can_stop: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct QueueStats {
    pub sim_depth: usize,
    pub sim_high_water: usize,
    pub sim_capacity: usize,
    pub sim_paused_ms: u64,
    pub sim_dropped: u64,
    pub sim_generated: u64,
    pub sim_max_step_us: u64,
    pub ui_rows_capacity: usize,
    pub ui_rows_not_shown: u64,
    pub ui_logs_dropped: u64,
    pub ui_errors_dropped: u64,
    pub link_loop_max_us: u64,
    pub write_max_us: u64,
    pub read_chunk_max: usize,
    /// Largest number of bytes waiting for the serial port.
    pub tx_backlog_max: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct Stats {
    /// Session counters (reset at each SESSION_START).
    pub frames_sent: u64,
    pub results: u64,
    pub nacks: u64,
    pub nack_codes: BTreeMap<String, u64>,
    pub lost: u64,
    pub late: u64,
    pub aborted: u64,
    pub in_flight: u32,
    pub in_flight_max: u32,
    pub window: u32,
    pub credit_waits: u64,
    pub tx_fps: f64,
    pub rx_fps: f64,
    pub rtt_count: u64,
    pub rtt_p50_us: u32,
    pub rtt_p95_us: u32,
    pub rtt_p99_us: u32,
    pub rtt_max_us: u32,
    pub rtt_mean_us: f64,
    pub sim_time_sent: u64,
    pub tick_results: u64,
    pub logs: u64,
    /// Link counters (since the application started).
    pub rx_framing: BTreeMap<String, u64>,
    pub rx_rejects: u64,
    pub board_seq_gaps: u64,
    /// Events received before this connection's HELLO_ACK (left over from before), ignored.
    pub pre_sync_discarded: u64,
    pub bytes_tx: u64,
    pub bytes_rx: u64,
    pub queues: QueueStats,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct EntryView {
    pub slot: u8,
    /// 0 = published value, 1 = reject.
    pub kind: u8,
    /// IEEE-754 binary32 bits: exact, and defined for NaN/infinity (JSON numbers are not).
    pub bits: u32,
}

/// One VCAN_FRAME and what the board answered.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct FrameRow {
    pub session_id: u32,
    pub frame_seq: u32,
    pub t_us: u64,
    pub can_id: u32,
    pub flags: u8,
    pub dlc: u8,
    /// Lowercase hex, no separator.
    pub data: String,
    /// "result" | "nack" | "lost" | "aborted"
    pub outcome: &'static str,
    pub status: u8,
    pub result_flags: u8,
    pub cycles: u32,
    pub entries: Vec<EntryView>,
    pub nack: Option<String>,
    pub rtt_us: Option<u32>,
    pub stim_rev: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SlotView {
    pub slot: u8,
    pub kind: u8,
    pub bits: u32,
    /// "frame" | "tick"
    pub source: &'static str,
    /// frame_seq for a frame, bus time (µs) for a tick.
    pub at: u64,
    pub updates: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct LogRow {
    pub session_id: u32,
    pub t_us: u64,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ErrorRow {
    /// Milliseconds since the application started.
    pub at_ms: u64,
    /// "framing" | "reject" | "nack" | "fault" | "port" | "link" | "session" | "info"
    pub kind: &'static str,
    pub text: String,
}

/// Everything the UI needs since its previous batch.
#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct UiBatch {
    pub rev: u64,
    pub status: Status,
    pub stats: Stats,
    pub rows: Vec<FrameRow>,
    pub logs: Vec<LogRow>,
    pub errors: Vec<ErrorRow>,
    pub slots: Vec<SlotView>,
}
