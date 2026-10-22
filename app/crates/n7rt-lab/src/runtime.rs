//! Pinned runtime releases and board identification from `HELLO_ACK`.
//!
//! The release constants and images come from `firmware/n7rt_student/runtime-release*.json` and the
//! SDK runtime objects (see build.rs): the application never keeps its own list of runtimes. The
//! board family (NUCLEO-H753ZI or NUCLEO-G0B1RE) is the `DEV_ID` the board reports.

use n7rt_link::HelloAck;
use serde::Serialize;
use sha2::{Digest, Sha256};

/// Runtime of the NUCLEO-H753ZI.
pub mod pinned {
    include!(concat!(env!("OUT_DIR"), "/runtime_release.rs"));
}

/// Runtime of the NUCLEO-G0B1RE.
pub mod pinned_g0b1 {
    include!(concat!(env!("OUT_DIR"), "/runtime_release_g0b1.rs"));
}

/// Protocol versions this application speaks (`HELLO.proto_min..=proto_max`).
pub const PROTO_MIN: u8 = 0;
pub const PROTO_MAX: u8 = 0;
/// DBGMCU_IDCODE.DEV_ID of the STM32H742/743/753/750.
pub const DEV_ID_H7: u16 = 0x450;
/// DBGMCU_IDCODE.DEV_ID of the STM32G0B0/B1/C1.
pub const DEV_ID_G0B1: u16 = 0x467;

/// A supported board and the runtime pinned for it.
#[derive(Debug, PartialEq)]
pub struct Family {
    pub key: &'static str,
    pub board: &'static str,
    pub mcu: &'static str,
    pub dev_id: u16,
    pub version: (u8, u8, u8),
    pub version_str: &'static str,
    pub build_id: [u8; 8],
    pub build_id_hex: &'static str,
    pub abi: (u16, u16),
    pub image: &'static [u8],
    /// Reset causes in the `reset_flags` of HELLO_ACK: RCC_RSR on the H7, RCC_CSR on the G0.
    pub reset_bits: &'static [(u32, &'static str)],
}

pub const H753: Family = Family {
    key: "h753",
    board: "NUCLEO-H753ZI",
    mcu: "STM32H753",
    dev_id: DEV_ID_H7,
    version: pinned::VERSION,
    version_str: pinned::VERSION_STR,
    build_id: pinned::BUILD_ID,
    build_id_hex: pinned::BUILD_ID_HEX,
    abi: pinned::ABI,
    image: pinned::IMAGE,
    reset_bits: &[
        (1 << 30, "basse consommation"),
        (1 << 28, "watchdog fenêtre"),
        (1 << 26, "watchdog"),
        (1 << 24, "reset logiciel"),
        (1 << 23, "mise sous tension"),
        (1 << 22, "broche NRST"),
        (1 << 21, "brown-out"),
    ],
};

pub const G0B1: Family = Family {
    key: "g0b1",
    board: "NUCLEO-G0B1RE",
    mcu: "STM32G0B1",
    dev_id: DEV_ID_G0B1,
    version: pinned_g0b1::VERSION,
    version_str: pinned_g0b1::VERSION_STR,
    build_id: pinned_g0b1::BUILD_ID,
    build_id_hex: pinned_g0b1::BUILD_ID_HEX,
    abi: pinned_g0b1::ABI,
    image: pinned_g0b1::IMAGE,
    reset_bits: &[
        (1 << 31, "basse consommation"),
        (1 << 30, "watchdog fenêtre"),
        (1 << 29, "watchdog"),
        (1 << 28, "reset logiciel"),
        (1 << 27, "mise sous tension ou brown-out"),
        (1 << 26, "broche NRST"),
        (1 << 25, "chargement des option bytes"),
    ],
};

pub const FAMILIES: [&Family; 2] = [&H753, &G0B1];

pub fn family_for_dev_id(dev_id: u16) -> Option<&'static Family> {
    FAMILIES.into_iter().find(|f| f.dev_id == dev_id)
}

/// `SHA-256(nonce ‖ runtime image)`, the value a NUCLEO-H753ZI running its pinned runtime returns.
pub fn expected_challenge(nonce: &[u8; 16]) -> [u8; 32] {
    expected_challenge_for(&H753, nonce)
}

/// `SHA-256(nonce ‖ runtime image)` for the runtime pinned for this board family.
pub fn expected_challenge_for(family: &Family, nonce: &[u8; 16]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(nonce);
    h.update(family.image);
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
    reset_causes_for(&H753, flags)
}

/// Main reset causes captured at boot, in the reset register of this board family.
pub fn reset_causes_for(family: &Family, flags: u32) -> Vec<&'static str> {
    family.reset_bits.iter().filter(|(b, _)| flags & b != 0).map(|(_, n)| *n).collect()
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct BoardIdentity {
    /// Board model (`NUCLEO-H753ZI`, `NUCLEO-G0B1RE`), or `""` for an unsupported microcontroller.
    pub board: &'static str,
    /// Board family key (`h753`, `g0b1`), `""` when unsupported.
    pub family: &'static str,
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
    let supported = family_for_dev_id(ack.dev_id);
    // An unsupported microcontroller is refused below; the remaining checks then use the H753 pin.
    let family = supported.unwrap_or(&H753);
    let runtime_verified = supported.is_some() && ack.challenge == expected_challenge_for(family, nonce);
    let mut problems = Vec::new();
    if supported.is_none() {
        problems.push(format!(
            "microcontrôleur DEV_ID 0x{:03X} non pris en charge : CAN Lab fonctionne avec une {} (0x{:03X}) ou une {} (0x{:03X})",
            ack.dev_id, H753.board, H753.dev_id, G0B1.board, G0B1.dev_id
        ));
    }
    if !(PROTO_MIN..=PROTO_MAX).contains(&ack.proto) {
        problems.push(format!("protocole N7RT-Link v{} non pris en charge (v{PROTO_MIN} attendu)", ack.proto));
    }
    let version = (ack.rt_major, ack.rt_minor, ack.rt_patch);
    if version != family.version || ack.build_id != family.build_id {
        problems.push(format!(
            "runtime {}.{}.{} (build {}) au lieu du runtime publié pour la {} : {} (build {})",
            ack.rt_major,
            ack.rt_minor,
            ack.rt_patch,
            hex(&ack.build_id),
            family.board,
            family.version_str,
            family.build_id_hex
        ));
    }
    if ack.abi_major != family.abi.0 {
        problems.push(format!("ABI runtime {}.{} au lieu de {}.{}", ack.abi_major, ack.abi_minor, family.abi.0, family.abi.1));
    }
    if supported.is_some() && !runtime_verified {
        problems.push("défi d'intégrité refusé : l'image runtime de la carte n'est pas celle publiée".into());
    }
    BoardIdentity {
        board: supported.map_or("", |f| f.board),
        family: supported.map_or("", |f| f.key),
        runtime: format!("{}.{}.{}", ack.rt_major, ack.rt_minor, ack.rt_patch),
        build_id: hex(&ack.build_id),
        abi: format!("{}.{}", ack.abi_major, ack.abi_minor),
        proto: ack.proto,
        uid: uid_words(&ack.uid),
        dev_id: ack.dev_id,
        rev_id: ack.rev_id,
        reset_flags: ack.reset_flags,
        reset_causes: reset_causes_for(family, ack.reset_flags),
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
    fn every_pinned_runtime_matches_its_release() {
        assert_eq!(hex(&Sha256::digest(pinned_g0b1::IMAGE)), pinned_g0b1::IMAGE_SHA256);
        assert_eq!(hex(&pinned_g0b1::BUILD_ID), pinned_g0b1::BUILD_ID_HEX);
        assert_ne!(pinned::IMAGE, pinned_g0b1::IMAGE);
    }

    fn ack_of(family: &Family, nonce: &[u8; 16]) -> HelloAck {
        HelloAck {
            reason: 0,
            proto: 0,
            rt_major: family.version.0,
            rt_minor: family.version.1,
            rt_patch: family.version.2,
            abi_major: family.abi.0,
            abi_minor: family.abi.1,
            build_id: family.build_id,
            uid: [0; 12],
            dev_id: family.dev_id,
            rev_id: 0,
            reset_flags: 0,
            student_status: 1,
            student_abi_major: 1,
            student_abi_minor: 0,
            student_crc32: 0,
            rx_window: 32,
            fault_pending: 0,
            challenge: expected_challenge_for(family, nonce),
        }
    }

    #[test]
    fn each_board_is_checked_against_its_own_runtime() {
        let nonce = [7u8; 16];
        for family in FAMILIES {
            let id = assess(&ack_of(family, &nonce), &nonce);
            assert!(id.compatible && id.runtime_verified, "{}: {:?}", family.board, id.problems);
            assert_eq!((id.board, id.family), (family.board, family.key));
        }
        // The G0B1 runtime answered by a board claiming to be an H753 is refused, and the reverse.
        let mut swapped = ack_of(&G0B1, &nonce);
        swapped.dev_id = DEV_ID_H7;
        assert!(!assess(&swapped, &nonce).compatible);
        let mut unknown = ack_of(&H753, &nonce);
        unknown.dev_id = 0x413;
        let id = assess(&unknown, &nonce);
        assert!(!id.compatible && !id.runtime_verified && id.board.is_empty());
        assert!(id.problems[0].contains("non pris en charge"));
    }

    #[test]
    fn reset_causes_follow_the_board_family() {
        assert_eq!(reset_causes_for(&H753, 1 << 24), ["reset logiciel"]);
        assert_eq!(reset_causes_for(&G0B1, 0x1400_0000), ["reset logiciel", "broche NRST"]);
    }

    #[test]
    fn uid_is_printed_as_st_words() {
        let uid: [u8; 12] = [0x46, 0x00, 0x2d, 0x00, 0x0e, 0x51, 0x33, 0x32, 0x31, 0x33, 0x36, 0x32];
        assert_eq!(uid_words(&uid), "002D0046-3233510E-32363331");
    }
}
