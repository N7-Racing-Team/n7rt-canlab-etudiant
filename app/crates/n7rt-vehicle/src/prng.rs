//! Counter-based PRNG keyed by (session seed, stream, index) (architecture §10.4): SplitMix64 finaliser chain.

pub(crate) const STREAM_JITTER: u64 = 0x0100;
pub(crate) const STREAM_THERMAL: u64 = 0x0200;
pub(crate) const STREAM_LOG_MEMORY: u64 = 0x0300;
pub(crate) const STREAM_CELL_INDEX: u64 = 0x0301;
pub(crate) const STREAM_NOMINAL: u64 = 0x0400;

fn mix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

pub(crate) fn draw(seed: u64, stream: u64, index: u64) -> u64 {
    mix(mix(mix(seed) ^ stream) ^ index)
}

/// Uniform integer in [lo ; hi].
pub(crate) fn uniform_int(seed: u64, stream: u64, index: u64, lo: i64, hi: i64) -> i64 {
    if hi <= lo {
        return lo;
    }
    let span = (hi - lo) as u64 + 1;
    lo + (draw(seed, stream, index) % span) as i64
}

/// Uniform real in [0 ; 1[.
pub(crate) fn uniform_unit(seed: u64, stream: u64, index: u64) -> f64 {
    (draw(seed, stream, index) >> 11) as f64 / (1u64 << 53) as f64
}
