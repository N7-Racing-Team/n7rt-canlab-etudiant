//! Stream receiver: bytes in, events out (spec section 5).

use crate::cobs::cobs_decode_into;
use crate::crc::crc32;
use crate::message::{code, decode_payload, DecodeError, Direction, Message};
use crate::{CRC_SIZE, DELIMITER, HEADER_SIZE, MAX_ENCODED, MAX_PAYLOAD, MIN_PACKET, PROTO_VERSION};

/// Framing errors: nothing in the frame is trustworthy, it is dropped and counted, never answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FramingError {
    Overflow,
    Cobs,
    Short,
    Crc,
}

impl FramingError {
    pub fn name(self) -> &'static str {
        match self {
            FramingError::Overflow => "E_OVERFLOW",
            FramingError::Cobs => "E_COBS",
            FramingError::Short => "E_SHORT",
            FramingError::Crc => "E_CRC",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Packet {
    pub type_: u8,
    pub seq: u16,
    pub message: Message,
    /// Bytes after the part defined by v0 (compatible extension, ignored).
    pub extension: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Packet(Packet),
    FrameError(FramingError),
    /// The header is trustworthy (CRC passed): the board answers `NACK(seq, type_, code)`.
    Reject { code: u8, type_: u8, seq: u16, reason: &'static str },
}

/// Applies the normative check order to one delimiter-free frame.
pub fn parse_frame(encoded: &[u8], dir: Direction) -> Event {
    parse_frame_with(encoded, dir, &mut Vec::with_capacity(MAX_ENCODED))
}

fn parse_frame_with(encoded: &[u8], dir: Direction, packet: &mut Vec<u8>) -> Event {
    if cobs_decode_into(encoded, packet).is_err() {
        return Event::FrameError(FramingError::Cobs);
    }
    if packet.len() < MIN_PACKET {
        return Event::FrameError(FramingError::Short);
    }
    let body_len = packet.len() - CRC_SIZE;
    let crc = u32::from_le_bytes(packet[body_len..].try_into().expect("4 bytes"));
    if crc32(&packet[..body_len]) != crc {
        return Event::FrameError(FramingError::Crc);
    }
    let (proto, type_) = (packet[0], packet[1]);
    let seq = u16::from_le_bytes([packet[2], packet[3]]);
    let length = u16::from_le_bytes([packet[4], packet[5]]) as usize;
    let reject = |code: u8, reason: &'static str| Event::Reject { code, type_, seq, reason };
    if proto != PROTO_VERSION {
        return reject(code::E_VERSION, "proto is not 0");
    }
    if length > MAX_PAYLOAD || length != packet.len() - MIN_PACKET {
        return reject(code::E_LENGTH, "header len differs from the actual payload");
    }
    let payload = &packet[HEADER_SIZE..body_len];
    match decode_payload(dir, type_, payload) {
        Ok((message, used)) => Event::Packet(Packet { type_, seq, message, extension: payload[used..].to_vec() }),
        Err(DecodeError::UnknownType) => reject(code::E_UNKNOWN_TYPE, "type not accepted in this direction"),
        Err(DecodeError::Payload(e)) => reject(code::E_PAYLOAD, e.0),
    }
}

/// One instance per direction and per link. Events never depend on how the bytes are chunked.
#[derive(Debug)]
pub struct Receiver {
    dir: Direction,
    buf: Vec<u8>,
    overflow: bool,
    scratch: Vec<u8>,
}

impl Receiver {
    /// `dir` is the direction of the bytes received (`BoardToPc` on the PC side).
    pub fn new(dir: Direction) -> Self {
        Self { dir, buf: Vec::with_capacity(MAX_ENCODED), overflow: false, scratch: Vec::with_capacity(MAX_ENCODED) }
    }

    pub fn feed(&mut self, data: &[u8], mut sink: impl FnMut(Event)) {
        for &b in data {
            if b == DELIMITER {
                if self.overflow {
                    sink(Event::FrameError(FramingError::Overflow));
                } else if !self.buf.is_empty() {
                    sink(parse_frame_with(&self.buf, self.dir, &mut self.scratch));
                }
                self.buf.clear();
                self.overflow = false;
            } else if !self.overflow {
                if self.buf.len() == MAX_ENCODED {
                    self.overflow = true;
                    self.buf.clear();
                } else {
                    self.buf.push(b);
                }
            }
        }
    }

    pub fn feed_collect(&mut self, data: &[u8]) -> Vec<Event> {
        let mut events = Vec::new();
        self.feed(data, |e| events.push(e));
        events
    }

    /// Bytes waiting for a delimiter.
    pub fn pending(&self) -> usize {
        self.buf.len()
    }

    /// Forgets a partial frame (after the port is reopened).
    pub fn reset(&mut self) {
        self.buf.clear();
        self.overflow = false;
    }
}
