//! N7RT-Link v0 codec.
//!
//! Normative contract: `docs/n7rt-link-v0.md`. This crate uses the generic public
//! codec interfaces and framing invariants. Public tests exercise generated input
//! properties; byte-exact conformance is executed by the restricted instructor
//! harness against the current sources.
//!
//! Session state (credits, time order, `E_STATE`...) is not modelled here: it belongs to the
//! link engine of the application (`n7rt-lab`).

mod cobs;
mod crc;
mod message;
mod receiver;

pub use cobs::{cobs_decode, cobs_decode_into, cobs_encode, CobsError};
pub use crc::crc32;
pub use message::*;
pub use receiver::{parse_frame, Event, FramingError, Packet, Receiver};

pub const PROTO_VERSION: u8 = 0;
pub const HEADER_SIZE: usize = 6;
pub const CRC_SIZE: usize = 4;
pub const MIN_PACKET: usize = HEADER_SIZE + CRC_SIZE;
pub const MAX_PAYLOAD: usize = 255;
pub const MAX_PACKET: usize = HEADER_SIZE + MAX_PAYLOAD + CRC_SIZE;
/// Canonical COBS of a 265-byte packet, delimiter excluded.
pub const MAX_ENCODED: usize = MAX_PACKET + 2;
pub const DELIMITER: u8 = 0x00;
pub const BAUD: u32 = 921_600;
/// 8N1: start + 8 data + stop.
pub const UART_BITS_PER_BYTE: u32 = 10;

/// Packet with `proto = 0` and the true payload length (spec section 4.1).
pub fn build_packet(type_: u8, seq: u16, payload: &[u8]) -> Vec<u8> {
    assert!(payload.len() <= MAX_PAYLOAD, "payload of {} bytes exceeds MAX_PAYLOAD", payload.len());
    build_packet_raw(PROTO_VERSION, type_, seq, payload.len() as u16, payload)
}

/// Packet with arbitrary header fields, for robustness tests. The CRC is always correct.
pub fn build_packet_raw(proto: u8, type_: u8, seq: u16, len_field: u16, payload: &[u8]) -> Vec<u8> {
    let mut p = Vec::with_capacity(HEADER_SIZE + payload.len() + CRC_SIZE);
    p.push(proto);
    p.push(type_);
    p.extend_from_slice(&seq.to_le_bytes());
    p.extend_from_slice(&len_field.to_le_bytes());
    p.extend_from_slice(payload);
    let crc = crc32(&p);
    p.extend_from_slice(&crc.to_le_bytes());
    p
}

/// `COBS(packet) ‖ 0x00` (spec section 4.3).
pub fn encode_wire(packet: &[u8]) -> Vec<u8> {
    let mut wire = cobs_encode(packet);
    wire.push(DELIMITER);
    wire
}

/// Largest possible wire size (COBS + delimiter) of a packet carrying `payload_len` bytes.
pub fn wire_bound(payload_len: usize) -> usize {
    let n = HEADER_SIZE + payload_len + CRC_SIZE;
    n + n.div_ceil(254) + 1
}

/// Encodes messages of one sender and numbers them: `seq` starts at 0 when the port opens and
/// grows by one per packet, modulo 65 536 (spec section 9.1).
#[derive(Debug, Default, Clone)]
pub struct Encoder {
    next_seq: u16,
}

impl Encoder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn next_seq(&self) -> u16 {
        self.next_seq
    }

    /// Validated message → `(seq, wire bytes)`. The sequence number is consumed only on success.
    pub fn encode(&mut self, msg: &Message) -> Result<(u16, Vec<u8>), PayloadError> {
        let payload = msg.encode_payload()?;
        let seq = self.next_seq;
        self.next_seq = seq.wrapping_add(1);
        Ok((seq, encode_wire(&build_packet(msg.type_code(), seq, &payload))))
    }
}
