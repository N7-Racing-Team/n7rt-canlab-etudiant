//! Hidden test vectors (oracle as data). The instructor edition renders the frames of a hidden test on the reference
//! bench when it generates a group pack; the student application replays them to the board unchanged and never
//! simulates them again. Text form `n7rt-vec` v1: lowercase hex of a binary stream, one record per frame:
//! LEB128 delay since the previous frame (µs), N7RT-Link flags (u8), identifier (u32 little endian: 11 bits, or 29
//! with EXT), DLC (≤ 15), then the data bytes the DLC and the flags imply (classic ≤ 8, FD up to 64, RTR none).

use n7rt_link::{can_data_length, CAN_EXT, CAN_FLAGS_MASK, EXT_ID_MAX, STD_ID_MAX};
use n7rt_vehicle::BusFrame;

use crate::engine::RecFrame;

pub const VECTOR_FORMAT: &str = "n7rt-vec";
pub const VECTOR_VERSION: u32 = 1;
/// A 28 s qualification at ~535 frames/s stays far below this bound.
pub const MAX_VECTOR_FRAMES: usize = 60_000;

/// The frames of one hidden test, sorted by time, all within `duration_us`.
#[derive(Clone, PartialEq, Eq)]
pub struct HiddenVector {
    pub duration_us: u64,
    pub frames: Vec<BusFrame>,
}

/// Sealed content: the frames are never printed.
impl std::fmt::Debug for HiddenVector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "HiddenVector {{ duration_us: {}, frames: {} }}", self.duration_us, self.frames.len())
    }
}

impl HiddenVector {
    /// Checks the invariants the replay relies on: identifiers on 11 bits (29 with EXT), known flags, data length
    /// implied by the DLC and the flags, non-decreasing times within the duration, bounded count.
    pub fn new(duration_us: u64, frames: Vec<BusFrame>) -> Result<HiddenVector, String> {
        if frames.len() > MAX_VECTOR_FRAMES {
            return Err("vecteur caché trop long".into());
        }
        let mut last = 0u64;
        for f in &frames {
            let id_max = if f.flags & CAN_EXT != 0 { EXT_ID_MAX } else { STD_ID_MAX };
            if f.id > id_max || f.flags & !CAN_FLAGS_MASK != 0 || can_data_length(f.dlc, f.flags) != Some(f.data.len()) {
                return Err("trame de vecteur caché invalide".into());
            }
            if f.t_us < last || f.t_us > duration_us {
                return Err("trames de vecteur caché hors de l'ordre ou de la durée".into());
            }
            last = f.t_us;
        }
        Ok(HiddenVector { duration_us, frames })
    }

    pub fn encode(&self) -> String {
        let mut bytes = Vec::with_capacity(self.frames.len() * 12);
        let mut last = 0u64;
        for f in &self.frames {
            let mut delay = f.t_us - last;
            last = f.t_us;
            loop {
                let low = (delay & 0x7F) as u8;
                delay >>= 7;
                if delay == 0 {
                    bytes.push(low);
                    break;
                }
                bytes.push(low | 0x80);
            }
            bytes.push(f.flags);
            bytes.extend_from_slice(&f.id.to_le_bytes());
            bytes.push(f.dlc);
            bytes.extend_from_slice(&f.data);
        }
        let mut text = String::with_capacity(bytes.len() * 2);
        for b in bytes {
            text.push(char::from_digit((b >> 4) as u32, 16).unwrap_or('0'));
            text.push(char::from_digit((b & 0xF) as u32, 16).unwrap_or('0'));
        }
        text
    }

    pub fn decode(duration_us: u64, text: &str) -> Result<HiddenVector, String> {
        let bad = || "vecteur caché illisible".to_owned();
        let digits = text.as_bytes();
        if digits.len() % 2 != 0 {
            return Err(bad());
        }
        let nibble = |c: u8| match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'a'..=b'f' => Some(c - b'a' + 10),
            _ => None,
        };
        let mut bytes = Vec::with_capacity(digits.len() / 2);
        for pair in digits.chunks_exact(2) {
            bytes.push(nibble(pair[0]).ok_or_else(bad)? << 4 | nibble(pair[1]).ok_or_else(bad)?);
        }
        let mut frames = Vec::new();
        let mut at = 0usize;
        let mut t = 0u64;
        while at < bytes.len() {
            if frames.len() >= MAX_VECTOR_FRAMES {
                return Err("vecteur caché trop long".into());
            }
            let mut delay = 0u64;
            let mut shift = 0u32;
            loop {
                let b = *bytes.get(at).ok_or_else(bad)?;
                at += 1;
                if shift > 56 {
                    return Err(bad());
                }
                delay |= ((b & 0x7F) as u64) << shift;
                if b & 0x80 == 0 {
                    break;
                }
                shift += 7;
            }
            t = t.checked_add(delay).ok_or_else(bad)?;
            let head = bytes.get(at..at + 6).ok_or_else(bad)?;
            let flags = head[0];
            let id = u32::from_le_bytes([head[1], head[2], head[3], head[4]]);
            let dlc = head[5];
            at += 6;
            let len = can_data_length(dlc, flags).ok_or_else(bad)?;
            let data = bytes.get(at..at + len).ok_or_else(bad)?.to_vec();
            at += len;
            frames.push(BusFrame { t_us: t, id, flags, dlc, data });
        }
        HiddenVector::new(duration_us, frames)
    }

    /// The board received exactly this vector: the same frames, in the same order, at the same bus times, with the
    /// same flags. Frames recorded after the duration are ignored. Anything else cancels the attempt.
    pub fn delivered_exactly(&self, sent: &[RecFrame]) -> bool {
        let sent: Vec<&RecFrame> = sent.iter().filter(|f| f.t_us <= self.duration_us).collect();
        sent.len() == self.frames.len()
            && sent.iter().zip(&self.frames).all(|(s, f)| s.t_us == f.t_us && s.id == f.id && s.flags == f.flags && s.data == f.data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(t_us: u64, id: u32, data: &[u8]) -> BusFrame {
        BusFrame { t_us, id, flags: 0, dlc: data.len() as u8, data: data.to_vec() }
    }

    fn sample() -> HiddenVector {
        HiddenVector::new(
            9_000_000,
            vec![frame(0, 0x7A5, &[1, 2, 3, 4, 5, 6, 7, 8]), frame(0, 0x4C3, &[]), frame(20_000, 0x7FF, &[0xFF]),
                 frame(8_999_999, 0x001, &[0, 0])],
        )
        .unwrap()
    }

    #[test]
    fn round_trip_is_exact() {
        let v = sample();
        let text = v.encode();
        assert!(text.bytes().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
        assert_eq!(HiddenVector::decode(v.duration_us, &text).unwrap(), v);
    }

    #[test]
    fn large_delays_use_several_bytes() {
        let v = HiddenVector::new(u64::MAX, vec![frame(1 << 40, 0x123, &[9])]).unwrap();
        assert_eq!(HiddenVector::decode(u64::MAX, &v.encode()).unwrap(), v);
    }

    #[test]
    fn invalid_frames_are_refused() {
        assert!(HiddenVector::new(10, vec![frame(0, 0x800, &[])]).is_err());
        assert!(HiddenVector::new(10, vec![BusFrame { t_us: 0, id: 1, flags: 0, dlc: 2, data: vec![0] }]).is_err());
        // flagged frames: an extended identifier, a remote frame without data, an FD frame of 12 bytes
        let flagged = vec![
            BusFrame { t_us: 1, id: 0x1ABC_DEF0, flags: CAN_EXT, dlc: 2, data: vec![1, 2] },
            BusFrame { t_us: 2, id: 0x123, flags: n7rt_link::CAN_RTR, dlc: 4, data: vec![] },
            BusFrame { t_us: 3, id: 0x124, flags: n7rt_link::CAN_FD, dlc: 9, data: vec![9; 12] },
        ];
        let v = HiddenVector::new(10, flagged.clone()).unwrap();
        assert_eq!(HiddenVector::decode(10, &v.encode()).unwrap().frames, flagged);
        assert!(HiddenVector::new(10, vec![BusFrame { t_us: 0, id: 0x800, flags: 0, dlc: 0, data: vec![] }]).is_err(), "11 bits without EXT");
        assert!(HiddenVector::new(10, vec![BusFrame { t_us: 0, id: 1, flags: n7rt_link::CAN_FD, dlc: 9, data: vec![0; 9] }]).is_err(), "FD length");
        assert!(HiddenVector::new(10, vec![frame(5, 1, &[]), frame(4, 1, &[])]).is_err());
        assert!(HiddenVector::new(10, vec![frame(11, 1, &[])]).is_err());
    }

    #[test]
    fn corrupt_text_is_refused() {
        let text = sample().encode();
        for broken in [&text[..text.len() - 1], &text[..text.len() - 2], "zz", "0A"] {
            assert!(HiddenVector::decode(9_000_000, broken).is_err(), "{broken}");
        }
        assert!(HiddenVector::decode(10, &sample().encode()).is_err(), "frames beyond the duration");
        assert!(HiddenVector::decode(10, "ffffffffffffffffff7f").is_err(), "overlong delay");
    }

    #[test]
    fn debug_never_prints_frames() {
        let text = format!("{:?}", sample());
        assert!(!text.contains("1957") && !text.contains("0x7A5"), "{text}");
    }

    #[test]
    fn exact_delivery_requires_every_frame_unchanged() {
        let v = sample();
        let sent: Vec<RecFrame> = v.frames.iter().map(|f| RecFrame { t_us: f.t_us, id: f.id, flags: 0, data: f.data.clone() }).collect();
        assert!(v.delivered_exactly(&sent));
        let mut late = sent.clone();
        late.push(RecFrame { t_us: 9_000_001, id: 0x7A5, flags: 0, data: vec![] });
        assert!(v.delivered_exactly(&late), "frames after the duration are ignored");
        let mut changed = sent.clone();
        changed[2].data[0] ^= 1;
        assert!(!v.delivered_exactly(&changed));
        let mut flagged = sent.clone();
        flagged[0].flags = 1;
        assert!(!v.delivered_exactly(&flagged));
        assert!(!v.delivered_exactly(&sent[1..]));
        let mut shifted = sent;
        shifted[1].t_us += 1;
        assert!(!v.delivered_exactly(&shifted));
    }
}
