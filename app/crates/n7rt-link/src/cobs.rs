//! COBS: canonical encoder, liberal decoder (spec section 4.3).

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CobsError {
    Empty,
    /// A code byte equal to 0x00.
    ZeroCode { offset: usize },
    /// A block that runs past the end of the frame.
    BlockOverrun { offset: usize },
    /// A 0x00 inside a block (impossible after delimiter splitting, kept for direct callers).
    ZeroInBlock { offset: usize },
}

impl fmt::Display for CobsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CobsError::Empty => write!(f, "empty frame"),
            CobsError::ZeroCode { offset } => write!(f, "zero byte at offset {offset}"),
            CobsError::BlockOverrun { offset } => write!(f, "block at offset {offset} overruns the frame"),
            CobsError::ZeroInBlock { offset } => write!(f, "zero byte inside block at offset {offset}"),
        }
    }
}

impl std::error::Error for CobsError {}

/// Canonical COBS: no empty block is emitted after a final 254-byte block.
pub fn cobs_encode(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + data.len() / 254 + 1);
    let mut len = 0usize; // bytes of the current block, which ends at the current index
    let last = data.len().wrapping_sub(1);
    for (i, &b) in data.iter().enumerate() {
        if b == 0 {
            out.push((len + 1) as u8);
            out.extend_from_slice(&data[i - len..i]);
            len = 0;
            continue;
        }
        len += 1;
        if len == 254 {
            out.push(0xFF);
            out.extend_from_slice(&data[i + 1 - 254..=i]);
            len = 0;
            if i == last {
                return out;
            }
        }
    }
    out.push((len + 1) as u8);
    out.extend_from_slice(&data[data.len() - len..]);
    out
}

pub fn cobs_decode(data: &[u8]) -> Result<Vec<u8>, CobsError> {
    let mut out = Vec::with_capacity(data.len());
    cobs_decode_into(data, &mut out)?;
    Ok(out)
}

/// Decodes into `out` (cleared first), so a receiver can reuse one buffer.
pub fn cobs_decode_into(data: &[u8], out: &mut Vec<u8>) -> Result<(), CobsError> {
    out.clear();
    if data.is_empty() {
        return Err(CobsError::Empty);
    }
    let n = data.len();
    let mut i = 0;
    while i < n {
        let code = data[i];
        if code == 0 {
            return Err(CobsError::ZeroCode { offset: i });
        }
        let end = i + code as usize;
        if end > n {
            return Err(CobsError::BlockOverrun { offset: i });
        }
        let chunk = &data[i + 1..end];
        if chunk.contains(&0) {
            return Err(CobsError::ZeroInBlock { offset: i });
        }
        out.extend_from_slice(chunk);
        i = end;
        if code != 0xFF && i < n {
            out.push(0);
        }
    }
    Ok(())
}
