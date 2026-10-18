//! In-memory board speaking N7RT-Link v0, for engine tests without hardware
//! (docs/can-lab-architecture.md section 13, "fausse carte").
//!
//! It answers like the pinned runtime running the SDK example `student_decoder.c`. It is a test
//! double only: S1.4 is validated on the real board, never with this.

use std::collections::VecDeque;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use n7rt_link::{
    code, ty, BoardHeartbeat, Direction, Encoder, Entry, Event, FrameResult, HelloAck, Message, Nack, Receiver,
    SessionAck, StudentLog, TickResult,
};

use crate::discovery::PortInfo;
use crate::engine::{Connector, OpenError, Transport};
use crate::runtime::{expected_challenge, pinned, DEV_ID_H7};

pub struct FakeBoard {
    rx: Receiver,
    enc: Encoder,
    out: VecDeque<u8>,
    pending_in: Vec<u8>,
    boot: Instant,
    last_hb: Instant,
    session: Option<(u32, u16)>,
    bus_time: u64,
    next_tick: u64,
    acked: u32,
    rx_frames: u32,
    frames_seen: u32,
    /// Core halted (breakpoint): nothing is processed or emitted until cleared.
    pub halted: bool,
    /// While halted, received bytes are lost (UART overrun) instead of queued.
    pub overrun_when_halted: bool,
    /// Drop the RESULT of every n-th frame (noise on the line).
    pub drop_every: Option<u32>,
    pub student_status: u8,
    /// Header `seq` of every packet received since the last (re)open, for duplicate detection.
    pub seen_seqs: Vec<u16>,
    /// What the student code publishes at each tick (`t_us` → entries); nothing when absent.
    pub tick_answer: Option<TickAnswer>,
}

/// Programmed publications of the fake student code (tests of verifications that must pass).
pub type TickAnswer = Arc<dyn Fn(u64) -> Vec<Entry> + Send + Sync>;

impl FakeBoard {
    pub fn new() -> Self {
        let now = Instant::now();
        Self {
            rx: Receiver::new(Direction::PcToBoard),
            enc: Encoder::new(),
            out: VecDeque::new(),
            pending_in: Vec::new(),
            boot: now,
            last_hb: now,
            session: None,
            bus_time: 0,
            next_tick: 0,
            acked: 0,
            rx_frames: 0,
            frames_seen: 0,
            halted: false,
            overrun_when_halted: false,
            drop_every: None,
            student_status: 1,
            seen_seqs: Vec::new(),
            tick_answer: None,
        }
    }

    fn emit(&mut self, msg: Message) {
        let (_, wire) = self.enc.encode(&msg).expect("fake board message is valid");
        self.out.extend(wire);
    }

    fn hello_ack(&self, reason: u8, nonce: &[u8; 16]) -> Message {
        Message::HelloAck(HelloAck {
            reason,
            proto: 0,
            rt_major: pinned::VERSION.0,
            rt_minor: pinned::VERSION.1,
            rt_patch: pinned::VERSION.2,
            abi_major: pinned::ABI.0,
            abi_minor: pinned::ABI.1,
            build_id: pinned::BUILD_ID,
            challenge: expected_challenge(nonce),
            uid: *b"FAKE-BOARD-0",
            dev_id: DEV_ID_H7,
            rev_id: 0x2003,
            reset_flags: 1 << 24,
            student_status: self.student_status,
            student_abi_major: if self.student_status == 1 { 1 } else { 0 },
            student_abi_minor: 0,
            student_crc32: 0x5CBC_80B3,
            rx_window: 32,
            fault_pending: 0,
        })
    }

    fn reboot(&mut self) {
        let student_status = self.student_status;
        *self = FakeBoard { student_status, drop_every: self.drop_every, tick_answer: self.tick_answer.take(), ..FakeBoard::new() };
        self.out.push_back(0);
        let ack = self.hello_ack(1, &[0; 16]);
        self.emit(ack);
    }

    /// What a port can hold when it is opened: the tail of a frame and an answer to a HELLO of
    /// an earlier connection (its challenge cannot match the new nonce).
    pub fn inject_stale(&mut self) {
        self.out.extend([0x42, 0x13, 0x37, 0x00]);
        let ack = self.hello_ack(0, &[0x5A; 16]);
        self.emit(ack);
        self.emit(Message::SessionAck(SessionAck { session_id: 0xDEAD, op: 2, status: 0 }));
    }

    fn ticks_to(&mut self, t: u64, period_ms: u16) {
        let p = period_ms as u64 * 1000;
        while p > 0 && self.next_tick <= t {
            let t_us = self.next_tick;
            let entries = self.tick_answer.as_ref().map(|answer| answer(t_us)).unwrap_or_default();
            self.emit(Message::TickResult(TickResult { t_us, status: 0, flags: 0, cycles: 10, entries }));
            self.next_tick += p;
        }
        self.bus_time = t;
    }

    fn nack(&mut self, seq: u16, ref_type: u8, c: u8) {
        self.emit(Message::BoardNack(Nack { ref_seq: seq, ref_type, code: c }));
    }

    fn handle(&mut self, ev: Event) {
        let p = match ev {
            Event::Packet(p) => {
                self.seen_seqs.push(p.seq);
                p
            }
            Event::Reject { code, type_, seq, .. } => return self.nack(seq, type_, code),
            Event::FrameError(_) => return,
        };
        match p.message {
            Message::Hello(h) => {
                let ack = self.hello_ack(0, &h.nonce);
                self.emit(ack);
            }
            Message::ResetReq => self.reboot(),
            Message::SessionStart(s) => {
                let status = if self.student_status == 1 { 0 } else { 1 };
                if status == 0 && self.session.map(|(id, _)| id) != Some(s.session_id) {
                    self.session = Some((s.session_id, s.tick_period_ms));
                    self.bus_time = 0;
                    self.next_tick = s.tick_period_ms as u64 * 1000;
                    self.acked = 0;
                    self.emit(Message::StudentLog(StudentLog { t_us: 0, text: "student_decoder ready".into() }));
                }
                self.emit(Message::SessionAck(SessionAck { session_id: s.session_id, op: 1, status }));
            }
            Message::SessionStop { session_id } => {
                let status = if self.session.map(|(id, _)| id) == Some(session_id) { 0 } else { 2 };
                if status == 0 {
                    self.session = None;
                }
                self.emit(Message::SessionAck(SessionAck { session_id, op: 2, status }));
            }
            Message::VcanFrame(f) => {
                self.rx_frames += 1;
                let Some((_, period)) = self.session else { return self.nack(p.seq, ty::VCAN_FRAME, code::E_STATE) };
                if f.t_us < self.bus_time {
                    return self.nack(p.seq, ty::VCAN_FRAME, code::E_TIME);
                }
                self.ticks_to(f.t_us, period);
                self.acked = f.frame_seq;
                self.frames_seen += 1;
                if self.drop_every.is_some_and(|n| self.frames_seen % n == 0) {
                    return;
                }
                let mut entries = vec![Entry { slot: 0, kind: 0, value: f.data.len() as f32 }];
                entries.push(match f.data.first() {
                    Some(&b) => Entry { slot: 1, kind: 0, value: b as f32 },
                    None => Entry { slot: 1, kind: 1, value: 0.0 },
                });
                self.emit(Message::Result(FrameResult { frame_seq: f.frame_seq, status: 0, flags: 0, cycles: 120, entries }));
            }
            Message::SimTime { t_us } => {
                let Some((_, period)) = self.session else { return self.nack(p.seq, ty::SIM_TIME, code::E_STATE) };
                if t_us < self.bus_time {
                    return self.nack(p.seq, ty::SIM_TIME, code::E_TIME);
                }
                self.ticks_to(t_us, period);
            }
            _ => {}
        }
    }

    fn input(&mut self, data: &[u8]) {
        if self.halted {
            if !self.overrun_when_halted {
                self.pending_in.extend_from_slice(data);
            }
            return;
        }
        let mut events = Vec::new();
        self.rx.feed(data, |e| events.push(e));
        for e in events {
            self.handle(e);
        }
    }

    fn service(&mut self, now: Instant) {
        if self.halted {
            return;
        }
        if !self.pending_in.is_empty() {
            let data = std::mem::take(&mut self.pending_in);
            self.input(&data);
        }
        if now - self.last_hb >= Duration::from_millis(250) {
            self.last_hb = now;
            let hb = BoardHeartbeat {
                uptime_ms: now.duration_since(self.boot).as_millis() as u32,
                acked_frame_seq: self.acked,
                hook_running_ms: 0,
                state: self.session.map_or(0, |_| 1),
                student_disabled: 0,
                rx_frames: self.rx_frames,
                rx_seq_gaps: 0,
                rx_framing_errors: 0,
                rx_crc_errors: 0,
                rx_overruns: 0,
                log_dropped: 0,
                queue_high_water: 1,
                max_cycles: 120,
            };
            self.emit(Message::BoardHeartbeat(hb));
        }
    }
}

impl Default for FakeBoard {
    fn default() -> Self {
        Self::new()
    }
}

/// Controls shared by a test and the fake port.
#[derive(Clone)]
pub struct FakeHandle {
    pub board: Arc<Mutex<FakeBoard>>,
    /// The port is listed by the OS.
    pub present: Arc<AtomicBool>,
    /// Another program holds the port.
    pub busy: Arc<AtomicBool>,
    /// The next write is delivered but reported as 0 bytes written, like a Windows write
    /// timeout racing the USB transfer; the port then says partial counts are not exact.
    pub short_write_next: Arc<AtomicBool>,
    /// Duplicate packets seen by the board (same header `seq` twice in a row of connections).
    pub duplicates: Arc<std::sync::atomic::AtomicU32>,
}

impl FakeHandle {
    pub fn new() -> Self {
        Self {
            board: Arc::new(Mutex::new(FakeBoard::new())),
            present: Arc::new(AtomicBool::new(true)),
            busy: Arc::new(AtomicBool::new(false)),
            short_write_next: Arc::new(AtomicBool::new(false)),
            duplicates: Arc::new(std::sync::atomic::AtomicU32::new(0)),
        }
    }

    pub fn port() -> PortInfo {
        PortInfo {
            port: "FAKE0".into(),
            vid: Some(0x0483),
            pid: Some(0x374E),
            serial: Some("FAKE0001".into()),
            product: Some("STLINK-V3 (fake)".into()),
            probe: Some("STLINK-V3E"),
        }
    }

    pub fn connector(&self) -> Box<dyn Connector> {
        Box::new(FakeConnector(self.clone()))
    }
}

impl Default for FakeHandle {
    fn default() -> Self {
        Self::new()
    }
}

struct FakeConnector(FakeHandle);

impl Connector for FakeConnector {
    fn scan(&mut self) -> Result<Vec<PortInfo>, String> {
        Ok(if self.0.present.load(Ordering::SeqCst) { vec![FakeHandle::port()] } else { Vec::new() })
    }

    fn open(&mut self, port: &str) -> Result<Box<dyn Transport>, OpenError> {
        if !self.0.present.load(Ordering::SeqCst) || port != "FAKE0" {
            return Err(OpenError::Missing(port.into()));
        }
        if self.0.busy.load(Ordering::SeqCst) {
            return Err(OpenError::Busy(port.into()));
        }
        // A new connection numbers its packets from 0 again.
        self.0.board.lock().unwrap().seen_seqs.clear();
        Ok(Box::new(FakeTransport(self.0.clone())))
    }
}

struct FakeTransport(FakeHandle);

impl Transport for FakeTransport {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if !self.0.present.load(Ordering::SeqCst) {
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, "device removed"));
        }
        let n = {
            let mut b = self.0.board.lock().unwrap();
            b.service(Instant::now());
            let n = buf.len().min(b.out.len());
            for (dst, src) in buf.iter_mut().zip(b.out.drain(..n)) {
                *dst = src;
            }
            n
        };
        if n == 0 {
            thread::sleep(Duration::from_millis(1));
        }
        Ok(n)
    }

    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        if !self.0.present.load(Ordering::SeqCst) {
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, "device removed"));
        }
        let mut b = self.0.board.lock().unwrap();
        let before = b.seen_seqs.len();
        b.input(data);
        // A RESET_REQ in `data` reboots the board, which forgets what it saw.
        let dup = if b.seen_seqs.len() >= before {
            b.seen_seqs[before..].iter().filter(|s| b.seen_seqs[..before].contains(s)).count()
        } else {
            0
        };
        self.0.duplicates.fetch_add(dup as u32, Ordering::SeqCst);
        if self.0.short_write_next.swap(false, Ordering::SeqCst) {
            return Ok(0);
        }
        Ok(data.len())
    }

    fn exact_partial_writes(&self) -> bool {
        false
    }
}
