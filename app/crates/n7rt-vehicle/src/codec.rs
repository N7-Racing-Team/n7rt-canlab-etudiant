//! Signal placement and scaling. Bit k is bit (k mod 8) of byte ⌊k/8⌋, bit 0 = least significant.
//! Intel: the signal occupies start … start+len−1, LSB at `start`. Motorola: `start` is the MSB; the next bits go
//! down within the byte, then continue at bit 7 of the next byte (p ← p−1 if p mod 8 ≠ 0, else p+15).

use crate::pack::Signal;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ByteOrder {
    Intel,
    Motorola,
}

/// Bit positions of a signal, least significant bit first.
pub(crate) fn positions(start: u16, len: u8, order: ByteOrder) -> Vec<u16> {
    let len = len as usize;
    match order {
        ByteOrder::Intel => (0..len).map(|i| start + i as u16).collect(),
        ByteOrder::Motorola => {
            let mut msb_first = Vec::with_capacity(len);
            let mut p = start as i32;
            for i in 0..len {
                msb_first.push(p.max(0) as u16);
                if i + 1 < len {
                    p = if p % 8 != 0 { p - 1 } else { p + 15 };
                }
            }
            msb_first.reverse();
            msb_first
        }
    }
}

/// Raw value range representable on `len` bits.
pub(crate) fn raw_bounds(len: u8, signed: bool) -> (i64, i64) {
    let len = len as u32;
    if signed {
        (-(1i64 << (len - 1)), (1i64 << (len - 1)) - 1)
    } else {
        (0, (1i64 << len) - 1)
    }
}

pub(crate) fn write_bits(data: &mut [u8], pos: &[u16], bits: u64) {
    for (i, &p) in pos.iter().enumerate() {
        let byte = (p / 8) as usize;
        let mask = 1u8 << (p % 8);
        if (bits >> i) & 1 == 1 {
            data[byte] |= mask;
        } else {
            data[byte] &= !mask;
        }
    }
}

pub(crate) fn read_bits(data: &[u8], pos: &[u16]) -> u64 {
    let mut v = 0u64;
    for (i, &p) in pos.iter().enumerate() {
        let byte = (p / 8) as usize;
        if byte < data.len() && data[byte] >> (p % 8) & 1 == 1 {
            v |= 1 << i;
        }
    }
    v
}

/// Raw value of a signal read from a payload (two's complement for signed signals).
pub(crate) fn read_raw(sig: &Signal, data: &[u8]) -> i64 {
    let bits = read_bits(data, &sig.positions);
    let len = sig.positions.len() as u32;
    if sig.signed && len < 64 && (bits >> (len - 1)) & 1 == 1 {
        (bits as i64) - (1i64 << len)
    } else {
        bits as i64
    }
}

pub(crate) fn write_raw(sig: &Signal, data: &mut [u8], raw: i64) {
    let len = sig.positions.len() as u32;
    let mask = if len >= 64 { u64::MAX } else { (1u64 << len) - 1 };
    write_bits(data, &sig.positions, (raw as u64) & mask);
}

/// Physical value → raw: rounding to the nearest step, half up, then saturation to the emitted range.
pub(crate) fn phys_to_raw(sig: &Signal, phys: f64) -> i64 {
    let x = ((phys - sig.offset) / sig.factor + 0.5 + 1e-9).floor();
    if x.is_nan() {
        return sig.min;
    }
    let x = x.clamp(sig.min as f64, sig.max as f64);
    x as i64
}

pub(crate) fn raw_to_phys(sig: &Signal, raw: i64) -> f64 {
    raw as f64 * sig.factor + sig.offset
}

/// Writes a physical value, or the SNA code for `None` (all ones when the signal has no SNA).
pub(crate) fn write_value(sig: &Signal, data: &mut [u8], value: Option<f64>) {
    match value {
        Some(v) => write_raw(sig, data, phys_to_raw(sig, v)),
        None => match sig.sna {
            Some(s) => write_raw(sig, data, s),
            None => write_bits(data, &sig.positions, u64::MAX),
        },
    }
}

/// Decoded value: `None` for the SNA code.
pub(crate) fn read_value(sig: &Signal, data: &[u8]) -> Option<f64> {
    let raw = read_raw(sig, data);
    if sig.sna == Some(raw) {
        None
    } else {
        Some(raw_to_phys(sig, raw))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn motorola_example_of_the_convention() {
        // 12-bit signal with MSB at bit 23: the whole byte 2 (bits 11…4), then bits 7…4 of byte 3 (bits 3…0).
        let p = positions(23, 12, ByteOrder::Motorola);
        assert_eq!(p, vec![28, 29, 30, 31, 16, 17, 18, 19, 20, 21, 22, 23]);
    }

    #[test]
    fn intel_positions() {
        assert_eq!(positions(12, 4, ByteOrder::Intel), vec![12, 13, 14, 15]);
    }
}
