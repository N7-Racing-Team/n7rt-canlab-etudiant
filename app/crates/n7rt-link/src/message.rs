//! Typed messages and payload layouts (spec sections 6 to 8).

use std::fmt;

use crate::MAX_PAYLOAD;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    PcToBoard,
    BoardToPc,
}

/// Message type codes (spec section 7.1).
pub mod ty {
    pub const HELLO: u8 = 0x01;
    pub const SESSION_START: u8 = 0x02;
    pub const SESSION_STOP: u8 = 0x03;
    pub const RESET_REQ: u8 = 0x04;
    pub const NACK_PC: u8 = 0x05;
    pub const VCAN_FRAME: u8 = 0x10;
    /// Reserved (increment I4): refused in v0.
    pub const VCAN_STATUS: u8 = 0x11;
    pub const SIM_TIME: u8 = 0x12;
    pub const HEARTBEAT_PC: u8 = 0x7F;
    pub const HELLO_ACK: u8 = 0x81;
    pub const SESSION_ACK: u8 = 0x82;
    pub const FAULT_REPORT: u8 = 0x83;
    pub const NACK_BOARD: u8 = 0x84;
    pub const RESULT: u8 = 0x90;
    pub const TICK_RESULT: u8 = 0x91;
    pub const STUDENT_LOG: u8 = 0x92;
    /// Reserved (V2): refused in v0.
    pub const VCAN_TX: u8 = 0x93;
    pub const HEARTBEAT_BOARD: u8 = 0xFF;
}

/// Reject codes carried by `NACK` (spec section 11).
pub mod code {
    pub const E_UNKNOWN_TYPE: u8 = 0x01;
    pub const E_VERSION: u8 = 0x02;
    pub const E_LENGTH: u8 = 0x03;
    pub const E_PAYLOAD: u8 = 0x04;
    pub const E_STATE: u8 = 0x10;
    pub const E_CREDIT: u8 = 0x11;
    pub const E_TIME: u8 = 0x13;

    pub fn name(code: u8) -> Option<&'static str> {
        Some(match code {
            E_UNKNOWN_TYPE => "E_UNKNOWN_TYPE",
            E_VERSION => "E_VERSION",
            E_LENGTH => "E_LENGTH",
            E_PAYLOAD => "E_PAYLOAD",
            E_STATE => "E_STATE",
            E_CREDIT => "E_CREDIT",
            E_TIME => "E_TIME",
            _ => return None,
        })
    }

    pub fn from_name(name: &str) -> Option<u8> {
        [E_UNKNOWN_TYPE, E_VERSION, E_LENGTH, E_PAYLOAD, E_STATE, E_CREDIT, E_TIME]
            .into_iter()
            .find(|&c| self::name(c) == Some(name))
    }
}

// --- CAN constants, identical to pc/dashboard/can_model.py (spec section 8) -------------------
pub const CAN_EXT: u8 = 0x01;
pub const CAN_RTR: u8 = 0x02;
pub const CAN_FD: u8 = 0x04;
pub const CAN_BRS: u8 = 0x08;
pub const CAN_ESI: u8 = 0x10;
pub const CAN_FLAGS_MASK: u8 = 0x1F;
pub const STD_ID_MAX: u32 = 0x7FF;
pub const EXT_ID_MAX: u32 = 0x1FFF_FFFF;
pub const CLASSIC_MAX_LEN: usize = 8;
/// Index = DLC.
pub const FD_LENGTHS: [usize; 16] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 12, 16, 20, 24, 32, 48, 64];
pub const MAX_CAN_DATA: usize = 64;
pub const MAX_ENTRIES: usize = 8;
pub const MAX_LOG_TEXT: usize = 120;
pub const ENTRY_SIZE: usize = 6;
pub const RESULT_FLAG_PUBLISH_OVERFLOW: u8 = 0x01;
pub const RESULT_FLAG_OVER_BUDGET: u8 = 0x02;

/// Data length implied by a DLC and the frame flags (spec section 8.2); `None` if `dlc > 15`.
pub fn can_data_length(dlc: u8, flags: u8) -> Option<usize> {
    if dlc > 15 {
        return None;
    }
    Some(if flags & CAN_RTR != 0 {
        0
    } else if flags & CAN_FD != 0 {
        FD_LENGTHS[dlc as usize]
    } else {
        (dlc as usize).min(CLASSIC_MAX_LEN)
    })
}

/// Why a payload is invalid. Every case maps to `E_PAYLOAD`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PayloadError(pub &'static str);

impl fmt::Display for PayloadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for PayloadError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    /// Type absent from section 7 for this direction, or reserved (`E_UNKNOWN_TYPE`).
    UnknownType,
    /// `E_PAYLOAD`.
    Payload(PayloadError),
}

fn check(condition: bool, reason: &'static str) -> Result<(), PayloadError> {
    if condition {
        Ok(())
    } else {
        Err(PayloadError(reason))
    }
}

// --- Payload structures --------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hello {
    pub proto_min: u8,
    pub proto_max: u8,
    pub nonce: [u8; 16],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionStart {
    pub session_id: u32,
    /// 1 = exploration, 2 = qualification.
    pub mode: u8,
    pub tick_period_ms: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nack {
    pub ref_seq: u16,
    pub ref_type: u8,
    pub code: u8,
}

/// VirtualCanFrame (spec section 8). The wire `len` field is `data.len()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VcanFrame {
    pub frame_seq: u32,
    pub t_us: u64,
    pub can_id: u32,
    pub flags: u8,
    pub dlc: u8,
    pub bus: u8,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelloAck {
    pub reason: u8,
    pub proto: u8,
    pub rt_major: u8,
    pub rt_minor: u8,
    pub rt_patch: u8,
    pub abi_major: u16,
    pub abi_minor: u16,
    pub build_id: [u8; 8],
    pub challenge: [u8; 32],
    pub uid: [u8; 12],
    pub dev_id: u16,
    pub rev_id: u16,
    pub reset_flags: u32,
    pub student_status: u8,
    pub student_abi_major: u16,
    pub student_abi_minor: u16,
    pub student_crc32: u32,
    pub rx_window: u8,
    pub fault_pending: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionAck {
    pub session_id: u32,
    /// 1 = answer to SESSION_START, 2 = answer to SESSION_STOP.
    pub op: u8,
    /// 0 ok, 1 no valid student code, 2 unknown session.
    pub status: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FaultReport {
    pub cause: u8,
    pub hook: u8,
    pub frame_seq: u32,
    pub cfsr: u32,
    pub hfsr: u32,
    pub mmfar: u32,
    pub bfar: u32,
    pub pc: u32,
    pub lr: u32,
    pub xpsr: u32,
}

/// One publication: `kind` 0 = published value, 1 = reject (`value` = +0.0).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Entry {
    pub slot: u8,
    pub kind: u8,
    pub value: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FrameResult {
    pub frame_seq: u32,
    pub status: u8,
    pub flags: u8,
    pub cycles: u32,
    pub entries: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TickResult {
    pub t_us: u64,
    pub status: u8,
    pub flags: u8,
    pub cycles: u32,
    pub entries: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StudentLog {
    pub t_us: u64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardHeartbeat {
    pub uptime_ms: u32,
    pub acked_frame_seq: u32,
    pub hook_running_ms: u16,
    pub state: u8,
    pub student_disabled: u8,
    pub rx_frames: u32,
    pub rx_seq_gaps: u16,
    pub rx_framing_errors: u16,
    pub rx_crc_errors: u16,
    pub rx_overruns: u16,
    pub log_dropped: u16,
    pub queue_high_water: u8,
    pub max_cycles: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    // PC -> board
    Hello(Hello),
    SessionStart(SessionStart),
    SessionStop { session_id: u32 },
    ResetReq,
    PcNack(Nack),
    VcanFrame(VcanFrame),
    SimTime { t_us: u64 },
    PcHeartbeat { host_uptime_ms: u32 },
    // board -> PC
    HelloAck(HelloAck),
    SessionAck(SessionAck),
    FaultReport(FaultReport),
    BoardNack(Nack),
    Result(FrameResult),
    TickResult(TickResult),
    StudentLog(StudentLog),
    BoardHeartbeat(BoardHeartbeat),
}

/// Name of the message a receiver accepts for `type_` in `dir`, `None` if unknown or reserved.
pub fn message_name(dir: Direction, type_: u8) -> Option<&'static str> {
    use Direction::*;
    Some(match (dir, type_) {
        (PcToBoard, ty::HELLO) => "HELLO",
        (PcToBoard, ty::SESSION_START) => "SESSION_START",
        (PcToBoard, ty::SESSION_STOP) => "SESSION_STOP",
        (PcToBoard, ty::RESET_REQ) => "RESET_REQ",
        (PcToBoard, ty::NACK_PC) => "NACK",
        (PcToBoard, ty::VCAN_FRAME) => "VCAN_FRAME",
        (PcToBoard, ty::SIM_TIME) => "SIM_TIME",
        (PcToBoard, ty::HEARTBEAT_PC) => "HEARTBEAT",
        (BoardToPc, ty::HELLO_ACK) => "HELLO_ACK",
        (BoardToPc, ty::SESSION_ACK) => "SESSION_ACK",
        (BoardToPc, ty::FAULT_REPORT) => "FAULT_REPORT",
        (BoardToPc, ty::NACK_BOARD) => "NACK",
        (BoardToPc, ty::RESULT) => "RESULT",
        (BoardToPc, ty::TICK_RESULT) => "TICK_RESULT",
        (BoardToPc, ty::STUDENT_LOG) => "STUDENT_LOG",
        (BoardToPc, ty::HEARTBEAT_BOARD) => "HEARTBEAT",
        _ => return None,
    })
}

impl Message {
    pub fn type_code(&self) -> u8 {
        match self {
            Message::Hello(_) => ty::HELLO,
            Message::SessionStart(_) => ty::SESSION_START,
            Message::SessionStop { .. } => ty::SESSION_STOP,
            Message::ResetReq => ty::RESET_REQ,
            Message::PcNack(_) => ty::NACK_PC,
            Message::VcanFrame(_) => ty::VCAN_FRAME,
            Message::SimTime { .. } => ty::SIM_TIME,
            Message::PcHeartbeat { .. } => ty::HEARTBEAT_PC,
            Message::HelloAck(_) => ty::HELLO_ACK,
            Message::SessionAck(_) => ty::SESSION_ACK,
            Message::FaultReport(_) => ty::FAULT_REPORT,
            Message::BoardNack(_) => ty::NACK_BOARD,
            Message::Result(_) => ty::RESULT,
            Message::TickResult(_) => ty::TICK_RESULT,
            Message::StudentLog(_) => ty::STUDENT_LOG,
            Message::BoardHeartbeat(_) => ty::HEARTBEAT_BOARD,
        }
    }

    pub fn direction(&self) -> Direction {
        if self.type_code() < 0x80 {
            Direction::PcToBoard
        } else {
            Direction::BoardToPc
        }
    }

    pub fn name(&self) -> &'static str {
        message_name(self.direction(), self.type_code()).unwrap_or("?")
    }

    /// Payload bytes, validated with the receiver's own rules (spec sections 7.3 and 8.2).
    pub fn encode_payload(&self) -> Result<Vec<u8>, PayloadError> {
        self.encode_payload_with_extension(&[])
    }

    /// Payload followed by `extension` bytes (a future version's additions, section 6).
    pub fn encode_payload_with_extension(&self, extension: &[u8]) -> Result<Vec<u8>, PayloadError> {
        let mut w = Vec::with_capacity(96);
        self.write_fields(&mut w)?;
        let defined = w.len();
        w.extend_from_slice(extension);
        check(w.len() <= MAX_PAYLOAD, "payload longer than 255 bytes")?;
        let (_, used) = decode_payload(self.direction(), self.type_code(), &w).map_err(|e| match e {
            DecodeError::Payload(p) => p,
            DecodeError::UnknownType => PayloadError("unknown type"),
        })?;
        check(used == defined, "fields do not round-trip")?;
        Ok(w)
    }

    fn write_fields(&self, w: &mut Vec<u8>) -> Result<(), PayloadError> {
        match self {
            Message::Hello(m) => {
                w.extend_from_slice(&[m.proto_min, m.proto_max]);
                w.extend_from_slice(&m.nonce);
            }
            Message::SessionStart(m) => {
                put_u32(w, m.session_id);
                w.push(m.mode);
                put_u16(w, m.tick_period_ms);
            }
            Message::SessionStop { session_id } => put_u32(w, *session_id),
            Message::ResetReq => {}
            Message::PcNack(m) | Message::BoardNack(m) => {
                put_u16(w, m.ref_seq);
                w.extend_from_slice(&[m.ref_type, m.code]);
            }
            Message::VcanFrame(m) => {
                check(m.data.len() <= MAX_CAN_DATA, "len > 64")?;
                put_u32(w, m.frame_seq);
                put_u64(w, m.t_us);
                put_u32(w, m.can_id);
                w.extend_from_slice(&[m.flags, m.dlc, m.data.len() as u8, m.bus]);
                w.extend_from_slice(&m.data);
            }
            Message::SimTime { t_us } => put_u64(w, *t_us),
            Message::PcHeartbeat { host_uptime_ms } => put_u32(w, *host_uptime_ms),
            Message::HelloAck(m) => {
                w.extend_from_slice(&[m.reason, m.proto, m.rt_major, m.rt_minor, m.rt_patch]);
                put_u16(w, m.abi_major);
                put_u16(w, m.abi_minor);
                w.extend_from_slice(&m.build_id);
                w.extend_from_slice(&m.challenge);
                w.extend_from_slice(&m.uid);
                put_u16(w, m.dev_id);
                put_u16(w, m.rev_id);
                put_u32(w, m.reset_flags);
                w.push(m.student_status);
                put_u16(w, m.student_abi_major);
                put_u16(w, m.student_abi_minor);
                put_u32(w, m.student_crc32);
                w.extend_from_slice(&[m.rx_window, m.fault_pending]);
            }
            Message::SessionAck(m) => {
                put_u32(w, m.session_id);
                w.extend_from_slice(&[m.op, m.status]);
            }
            Message::FaultReport(m) => {
                w.extend_from_slice(&[m.cause, m.hook]);
                for v in [m.frame_seq, m.cfsr, m.hfsr, m.mmfar, m.bfar, m.pc, m.lr, m.xpsr] {
                    put_u32(w, v);
                }
            }
            Message::Result(m) => {
                check(m.entries.len() <= MAX_ENTRIES, "more than 8 entries")?;
                put_u32(w, m.frame_seq);
                w.extend_from_slice(&[m.status, m.flags]);
                put_u32(w, m.cycles);
                put_entries(w, &m.entries);
            }
            Message::TickResult(m) => {
                check(m.entries.len() <= MAX_ENTRIES, "more than 8 entries")?;
                put_u64(w, m.t_us);
                w.extend_from_slice(&[m.status, m.flags]);
                put_u32(w, m.cycles);
                put_entries(w, &m.entries);
            }
            Message::StudentLog(m) => {
                check(m.text.len() <= MAX_LOG_TEXT, "text_len > 120")?;
                put_u64(w, m.t_us);
                w.push(m.text.len() as u8);
                w.extend_from_slice(m.text.as_bytes());
            }
            Message::BoardHeartbeat(m) => {
                put_u32(w, m.uptime_ms);
                put_u32(w, m.acked_frame_seq);
                put_u16(w, m.hook_running_ms);
                w.extend_from_slice(&[m.state, m.student_disabled]);
                put_u32(w, m.rx_frames);
                for v in [m.rx_seq_gaps, m.rx_framing_errors, m.rx_crc_errors, m.rx_overruns, m.log_dropped] {
                    put_u16(w, v);
                }
                w.push(m.queue_high_water);
                put_u32(w, m.max_cycles);
            }
        }
        Ok(())
    }
}

fn put_u16(w: &mut Vec<u8>, v: u16) {
    w.extend_from_slice(&v.to_le_bytes());
}

fn put_u32(w: &mut Vec<u8>, v: u32) {
    w.extend_from_slice(&v.to_le_bytes());
}

fn put_u64(w: &mut Vec<u8>, v: u64) {
    w.extend_from_slice(&v.to_le_bytes());
}

fn put_entries(w: &mut Vec<u8>, entries: &[Entry]) {
    w.push(entries.len() as u8);
    for e in entries {
        w.extend_from_slice(&[e.slot, e.kind]);
        w.extend_from_slice(&e.value.to_le_bytes());
    }
}

/// Little-endian reader over a payload whose fixed part was already length-checked.
struct Rd<'a> {
    b: &'a [u8],
    pos: usize,
}

impl<'a> Rd<'a> {
    fn u8(&mut self) -> u8 {
        let v = self.b[self.pos];
        self.pos += 1;
        v
    }
    fn u16(&mut self) -> u16 {
        u16::from_le_bytes(self.arr())
    }
    fn u32(&mut self) -> u32 {
        u32::from_le_bytes(self.arr())
    }
    fn u64(&mut self) -> u64 {
        u64::from_le_bytes(self.arr())
    }
    fn arr<const N: usize>(&mut self) -> [u8; N] {
        let v: [u8; N] = self.b[self.pos..self.pos + N].try_into().expect("length checked");
        self.pos += N;
        v
    }
    fn take(&mut self, n: usize) -> &'a [u8] {
        let v = &self.b[self.pos..self.pos + n];
        self.pos += n;
        v
    }
    fn remaining(&self) -> usize {
        self.b.len() - self.pos
    }
}

fn fixed_size(dir: Direction, type_: u8) -> Option<usize> {
    use Direction::*;
    Some(match (dir, type_) {
        (PcToBoard, ty::HELLO) => 18,
        (PcToBoard, ty::SESSION_START) => 7,
        (PcToBoard, ty::SESSION_STOP) => 4,
        (PcToBoard, ty::RESET_REQ) => 0,
        (PcToBoard, ty::NACK_PC) => 4,
        (PcToBoard, ty::VCAN_FRAME) => 20,
        (PcToBoard, ty::SIM_TIME) => 8,
        (PcToBoard, ty::HEARTBEAT_PC) => 4,
        (BoardToPc, ty::HELLO_ACK) => 80,
        (BoardToPc, ty::SESSION_ACK) => 6,
        (BoardToPc, ty::FAULT_REPORT) => 34,
        (BoardToPc, ty::NACK_BOARD) => 4,
        (BoardToPc, ty::RESULT) => 11,
        (BoardToPc, ty::TICK_RESULT) => 15,
        (BoardToPc, ty::STUDENT_LOG) => 9,
        (BoardToPc, ty::HEARTBEAT_BOARD) => 31,
        _ => return None,
    })
}

fn read_entries(r: &mut Rd<'_>, n: u8) -> Result<Vec<Entry>, PayloadError> {
    let n = n as usize;
    check(n <= MAX_ENTRIES, "more than 8 entries")?;
    check(r.remaining() >= n * ENTRY_SIZE, "payload shorter than n entries")?;
    Ok((0..n)
        .map(|_| Entry { slot: r.u8(), kind: r.u8(), value: f32::from_le_bytes(r.arr()) })
        .collect())
}

fn validate_entries(status: u8, flags: u8, cycles: u32, entries: &[Entry]) -> Result<(), PayloadError> {
    check(status <= 1, "unknown status")?;
    check(flags & !(RESULT_FLAG_PUBLISH_OVERFLOW | RESULT_FLAG_OVER_BUDGET) == 0, "reserved result flag bits set")?;
    if status == 1 {
        check(entries.is_empty() && cycles == 0, "not-delivered result carries data")?;
    }
    for e in entries {
        check(e.kind <= 1, "unknown entry kind")?;
        if e.kind == 1 {
            check(e.value.to_bits() == 0, "reject entry value must be +0.0")?;
        }
    }
    Ok(())
}

fn validate_can(f: &VcanFrame) -> Result<(), PayloadError> {
    check(f.frame_seq != 0, "frame_seq is 0")?;
    check(f.flags & !CAN_FLAGS_MASK == 0, "reserved flag bits set")?;
    check(f.bus == 0, "bus must be 0 in v0")?;
    let (ext, fd, rtr) = (f.flags & CAN_EXT != 0, f.flags & CAN_FD != 0, f.flags & CAN_RTR != 0);
    check(f.can_id <= if ext { EXT_ID_MAX } else { STD_ID_MAX }, "can_id out of range")?;
    check(f.dlc <= 15, "dlc > 15")?;
    check(!(fd && rtr), "RTR does not exist in CAN FD")?;
    check(fd || f.flags & (CAN_BRS | CAN_ESI) == 0, "BRS/ESI require CAN_FD")?;
    check(Some(f.data.len()) == can_data_length(f.dlc, f.flags), "len inconsistent with dlc/flags")?;
    Ok(())
}

/// Decodes a payload received in `dir`: `(message, defined size)`. The bytes after the defined
/// part are a compatible extension and are ignored (spec section 6).
pub fn decode_payload(dir: Direction, type_: u8, payload: &[u8]) -> Result<(Message, usize), DecodeError> {
    let fixed = fixed_size(dir, type_).ok_or(DecodeError::UnknownType)?;
    decode_known(dir, type_, payload, fixed).map_err(DecodeError::Payload)
}

fn decode_known(dir: Direction, type_: u8, payload: &[u8], fixed: usize) -> Result<(Message, usize), PayloadError> {
    use Direction::*;
    check(payload.len() >= fixed, "payload shorter than the fixed part")?;
    let mut r = Rd { b: payload, pos: 0 };
    let msg = match (dir, type_) {
        (PcToBoard, ty::HELLO) => {
            let m = Hello { proto_min: r.u8(), proto_max: r.u8(), nonce: r.arr() };
            check(m.proto_min <= m.proto_max, "proto_min > proto_max")?;
            Message::Hello(m)
        }
        (PcToBoard, ty::SESSION_START) => {
            let m = SessionStart { session_id: r.u32(), mode: r.u8(), tick_period_ms: r.u16() };
            check(m.session_id != 0, "session_id is 0")?;
            check(matches!(m.mode, 1 | 2), "unknown mode")?;
            check(m.tick_period_ms <= 1000, "tick_period_ms > 1000")?;
            Message::SessionStart(m)
        }
        (PcToBoard, ty::SESSION_STOP) => {
            let session_id = r.u32();
            check(session_id != 0, "session_id is 0")?;
            Message::SessionStop { session_id }
        }
        (PcToBoard, ty::RESET_REQ) => Message::ResetReq,
        (PcToBoard, ty::NACK_PC) => Message::PcNack(Nack { ref_seq: r.u16(), ref_type: r.u8(), code: r.u8() }),
        (PcToBoard, ty::VCAN_FRAME) => {
            let (frame_seq, t_us, can_id) = (r.u32(), r.u64(), r.u32());
            let (flags, dlc, len, bus) = (r.u8(), r.u8(), r.u8() as usize, r.u8());
            check(len <= MAX_CAN_DATA, "len > 64")?;
            check(r.remaining() >= len, "payload shorter than len")?;
            let m = VcanFrame { frame_seq, t_us, can_id, flags, dlc, bus, data: r.take(len).to_vec() };
            validate_can(&m)?;
            Message::VcanFrame(m)
        }
        (PcToBoard, ty::SIM_TIME) => Message::SimTime { t_us: r.u64() },
        (PcToBoard, ty::HEARTBEAT_PC) => Message::PcHeartbeat { host_uptime_ms: r.u32() },
        (BoardToPc, ty::HELLO_ACK) => {
            let m = HelloAck {
                reason: r.u8(),
                proto: r.u8(),
                rt_major: r.u8(),
                rt_minor: r.u8(),
                rt_patch: r.u8(),
                abi_major: r.u16(),
                abi_minor: r.u16(),
                build_id: r.arr(),
                challenge: r.arr(),
                uid: r.arr(),
                dev_id: r.u16(),
                rev_id: r.u16(),
                reset_flags: r.u32(),
                student_status: r.u8(),
                student_abi_major: r.u16(),
                student_abi_minor: r.u16(),
                student_crc32: r.u32(),
                rx_window: r.u8(),
                fault_pending: r.u8(),
            };
            check(m.reason <= 1, "unknown reason")?;
            check(m.student_status <= 5, "unknown student_status")?;
            check(m.fault_pending <= 1, "fault_pending not boolean")?;
            check(m.rx_window >= 1, "rx_window is 0")?;
            Message::HelloAck(m)
        }
        (BoardToPc, ty::SESSION_ACK) => {
            let m = SessionAck { session_id: r.u32(), op: r.u8(), status: r.u8() };
            check(m.session_id != 0, "session_id is 0")?;
            check(matches!(m.op, 1 | 2), "unknown op")?;
            check(m.status <= 2, "unknown status")?;
            Message::SessionAck(m)
        }
        (BoardToPc, ty::FAULT_REPORT) => {
            let m = FaultReport {
                cause: r.u8(),
                hook: r.u8(),
                frame_seq: r.u32(),
                cfsr: r.u32(),
                hfsr: r.u32(),
                mmfar: r.u32(),
                bfar: r.u32(),
                pc: r.u32(),
                lr: r.u32(),
                xpsr: r.u32(),
            };
            check(matches!(m.cause, 1 | 2), "unknown cause")?;
            check(m.hook <= 3, "unknown hook")?;
            check((m.hook == 2) == (m.frame_seq != 0), "frame_seq only with hook on_frame")?;
            Message::FaultReport(m)
        }
        (BoardToPc, ty::NACK_BOARD) => Message::BoardNack(Nack { ref_seq: r.u16(), ref_type: r.u8(), code: r.u8() }),
        (BoardToPc, ty::RESULT) => {
            let (frame_seq, status, flags, cycles, n) = (r.u32(), r.u8(), r.u8(), r.u32(), r.u8());
            let entries = read_entries(&mut r, n)?;
            check(frame_seq != 0, "frame_seq is 0")?;
            validate_entries(status, flags, cycles, &entries)?;
            Message::Result(FrameResult { frame_seq, status, flags, cycles, entries })
        }
        (BoardToPc, ty::TICK_RESULT) => {
            let (t_us, status, flags, cycles, n) = (r.u64(), r.u8(), r.u8(), r.u32(), r.u8());
            let entries = read_entries(&mut r, n)?;
            validate_entries(status, flags, cycles, &entries)?;
            Message::TickResult(TickResult { t_us, status, flags, cycles, entries })
        }
        (BoardToPc, ty::STUDENT_LOG) => {
            let (t_us, n) = (r.u64(), r.u8() as usize);
            check(n <= MAX_LOG_TEXT, "text_len > 120")?;
            check(r.remaining() >= n, "payload shorter than text_len")?;
            let raw = r.take(n);
            check(!raw.contains(&0), "NUL byte in text")?;
            let text = std::str::from_utf8(raw).map_err(|_| PayloadError("text is not valid UTF-8"))?;
            Message::StudentLog(StudentLog { t_us, text: text.to_owned() })
        }
        (BoardToPc, ty::HEARTBEAT_BOARD) => {
            let m = BoardHeartbeat {
                uptime_ms: r.u32(),
                acked_frame_seq: r.u32(),
                hook_running_ms: r.u16(),
                state: r.u8(),
                student_disabled: r.u8(),
                rx_frames: r.u32(),
                rx_seq_gaps: r.u16(),
                rx_framing_errors: r.u16(),
                rx_crc_errors: r.u16(),
                rx_overruns: r.u16(),
                log_dropped: r.u16(),
                queue_high_water: r.u8(),
                max_cycles: r.u32(),
            };
            check(m.state <= 2, "unknown state")?;
            check(m.student_disabled <= 1, "student_disabled not boolean")?;
            Message::BoardHeartbeat(m)
        }
        _ => unreachable!("fixed_size covers exactly the known types"),
    };
    Ok((msg, r.pos))
}
