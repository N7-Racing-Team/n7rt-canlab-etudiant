//! Pinned runtime release and board identification from `HELLO_ACK`.
//!
//! The release constants and image come from `firmware/n7rt_student/runtime-release.json` and the
//! SDK runtime object (see build.rs): the application never keeps its own list of runtimes.

use n7rt_link::HelloAck;
use serde::Serialize;
use sha2::{Digest, Sha256};

pub mod pinned {
    include!(concat!(env!("OUT_DIR"), "/runtime_release.rs"));
}

/// Protocol versions this application speaks (`HELLO.proto_min..=proto_max`).
pub const PROTO_MIN: u8 = 0;
pub const PROTO_MAX: u8 = 0;
/// DBGMCU_IDCODE.DEV_ID of the STM32H742/743/753/750.
pub const DEV_ID_H7: u16 = 0x450;

/// `SHA-256(nonce ‖ runtime image)`, the value a board running the pinned runtime returns.
pub fn expected_challenge(nonce: &[u8; 16]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(nonce);
    h.update(pinned::IMAGE);
    h.finalize().into()
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// MCU unique ID as ST tools print it: three 32-bit words, little-endian on the wire.
pub fn uid_words(uid: &[u8; 12]) -> String {
    let w = |i: usize| u32::from_le_bytes(uid[4 * i..4 * i + 4].try_into().unwrap());
    format!("{:08X}-{:08X}-{:08X}", w(0), w(1), w(2))
}

pub fn student_status_text(status: u8) -> &'static str {
    match status {
        0 => "aucun code étudiant",
        1 => "code étudiant chargé",
        2 => "en-tête étudiant invalide",
        3 => "ABI étudiante incompatible",
        4 => "image étudiante hors bornes",
        5 => "code étudiant désactivé après une faute",
        _ => "statut inconnu",
    }
}

/// Main causes in RCC_RSR (STM32H7) as captured at boot.
pub fn reset_causes(flags: u32) -> Vec<&'static str> {
    const BITS: [(u32, &str); 7] = [
        (1 << 30, "basse consommation"),
        (1 << 28, "watchdog fenêtre"),
        (1 << 26, "watchdog"),
        (1 << 24, "reset logiciel"),
        (1 << 23, "mise sous tension"),
        (1 << 22, "broche NRST"),
        (1 << 21, "brown-out"),
    ];
    BITS.iter().filter(|(b, _)| flags & b != 0).map(|(_, n)| *n).collect()
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct BoardIdentity {
    pub runtime: String,
    pub build_id: String,
    pub abi: String,
    pub proto: u8,
    pub uid: String,
    pub dev_id: u16,
    pub rev_id: u16,
    pub reset_flags: u32,
    pub reset_causes: Vec<&'static str>,
    pub student_status: u8,
    pub student_status_text: &'static str,
    pub student_abi: String,
    pub student_crc32: String,
    pub rx_window: u8,
    pub fault_pending: bool,
    /// The challenge proves the board holds the pinned runtime image, byte for byte.
    pub runtime_verified: bool,
    pub compatible: bool,
    /// Human-readable reasons when `compatible` is false.
    pub problems: Vec<String>,
}

pub fn assess(ack: &HelloAck, nonce: &[u8; 16]) -> BoardIdentity {
    let runtime_verified = ack.challenge == expected_challenge(nonce);
    let mut problems = Vec::new();
    if !(PROTO_MIN..=PROTO_MAX).contains(&ack.proto) {
        problems.push(format!("protocole N7RT-Link v{} non pris en charge (v{PROTO_MIN} attendu)", ack.proto));
    }
    let version = (ack.rt_major, ack.rt_minor, ack.rt_patch);
    if version != pinned::VERSION || ack.build_id != pinned::BUILD_ID {
        problems.push(format!(
            "runtime {}.{}.{} (build {}) au lieu du runtime publié {} (build {})",
            ack.rt_major,
            ack.rt_minor,
            ack.rt_patch,
            hex(&ack.build_id),
            pinned::VERSION_STR,
            pinned::BUILD_ID_HEX
        ));
    }
    if ack.abi_major != pinned::ABI.0 {
        problems.push(format!("ABI runtime {}.{} au lieu de {}.{}", ack.abi_major, ack.abi_minor, pinned::ABI.0, pinned::ABI.1));
    }
    if !runtime_verified {
        problems.push("défi d'intégrité refusé : l'image runtime de la carte n'est pas celle publiée".into());
    }
    if ack.dev_id != DEV_ID_H7 {
        problems.push(format!("microcontrôleur DEV_ID 0x{:03X} au lieu de 0x{DEV_ID_H7:03X} (STM32H753)", ack.dev_id));
    }
    BoardIdentity {
        runtime: format!("{}.{}.{}", ack.rt_major, ack.rt_minor, ack.rt_patch),
        build_id: hex(&ack.build_id),
        abi: format!("{}.{}", ack.abi_major, ack.abi_minor),
        proto: ack.proto,
        uid: uid_words(&ack.uid),
        dev_id: ack.dev_id,
        rev_id: ack.rev_id,
        reset_flags: ack.reset_flags,
        reset_causes: reset_causes(ack.reset_flags),
        student_status: ack.student_status,
        student_status_text: student_status_text(ack.student_status),
        student_abi: format!("{}.{}", ack.student_abi_major, ack.student_abi_minor),
        student_crc32: format!("0x{:08X}", ack.student_crc32),
        rx_window: ack.rx_window,
        fault_pending: ack.fault_pending == 1,
        runtime_verified,
        compatible: problems.is_empty(),
        problems,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_image_matches_release() {
        let digest: String = hex(&Sha256::digest(pinned::IMAGE));
        assert_eq!(digest, pinned::IMAGE_SHA256);
        assert_eq!(hex(&pinned::BUILD_ID), pinned::BUILD_ID_HEX);
    }

    #[test]
    fn uid_is_printed_as_st_words() {
        let uid: [u8; 12] = [0x46, 0x00, 0x2d, 0x00, 0x0e, 0x51, 0x33, 0x32, 0x31, 0x33, 0x36, 0x32];
        assert_eq!(uid_words(&uid), "002D0046-3233510E-32363331");
    }
}
