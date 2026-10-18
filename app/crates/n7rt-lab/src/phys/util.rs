//! Small deterministic helpers of the Physical Lab engine: FNV-1a hashing, reading constructors, designators and
//! the number format of the French diagnostic messages.

use n7rt_circuit::{kind_def, Circuit, CompId, Kind, PinRef, Value};

use super::Reading;

/// FNV-1a 64 (contract §8.2: digest; state keys of the lowering).
pub(crate) struct Fnv(u64);

impl Fnv {
    pub fn new() -> Self {
        Fnv(0xcbf2_9ce4_8422_2325)
    }

    pub fn bytes(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 ^= u64::from(*b);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }

    pub fn u8(&mut self, x: u8) {
        self.bytes(&[x]);
    }

    pub fn u32(&mut self, x: u32) {
        self.bytes(&x.to_le_bytes());
    }

    pub fn u64(&mut self, x: u64) {
        self.bytes(&x.to_le_bytes());
    }

    /// Length-prefixed, so that consecutive strings cannot alias.
    pub fn str(&mut self, s: &str) {
        self.u64(s.len() as u64);
        self.bytes(s.as_bytes());
    }

    pub fn value(&mut self, v: &Value) {
        match v {
            Value::Bool(b) => {
                self.u8(1);
                self.u8(u8::from(*b));
            }
            Value::Int(i) => {
                self.u8(2);
                self.bytes(&i.to_le_bytes());
            }
            Value::Num(x) => {
                self.u8(3);
                self.u64(x.to_bits());
            }
            Value::Choice(s) => {
                self.u8(4);
                self.str(s);
            }
        }
    }

    pub fn finish(&self) -> u64 {
        self.0
    }
}

pub(crate) fn fnv_text(text: &str) -> u64 {
    let mut h = Fnv::new();
    h.bytes(text.as_bytes());
    h.finish()
}

// --------------------------------------------------------------------------------------------------- readings

pub(crate) fn unavailable<T>(reason: &str) -> Reading<T> {
    Reading::Unavailable { reason: reason.to_string() }
}

/// A computed value: never a non-finite number out of the core.
pub(crate) fn finite(v: f64) -> Reading<f64> {
    if v.is_finite() {
        Reading::Valid { v }
    } else {
        unavailable("solver_singular")
    }
}

/// The same invalid state for another quantity (a Floating voltage gives a Floating level or sample).
pub(crate) fn recast<T, U>(r: &Reading<T>) -> Reading<U> {
    match r {
        Reading::Floating => Reading::Floating,
        Reading::NoReference => Reading::NoReference,
        Reading::Undefined => Reading::Undefined,
        Reading::Open => Reading::Open,
        Reading::Unavailable { reason } => Reading::Unavailable { reason: reason.clone() },
        Reading::Valid { .. } | Reading::Saturated { .. } => unavailable("invalid_reading"),
    }
}

// ------------------------------------------------------------------------------------------------- designators

/// Designator of a component; "GND" for a ground symbol, "J<id>" for a junction.
pub(crate) fn comp_label(circuit: &Circuit, id: CompId) -> String {
    match circuit.components().get(&id) {
        Some(c) if !c.label.is_empty() => c.label.clone(),
        Some(c) if c.kind == Kind::Ground => "GND".to_string(),
        _ => format!("J{}", id.0),
    }
}

/// "<designator>.<short pin name>" ("U1.PD0", "R1.2").
pub(crate) fn pin_label(circuit: &Circuit, pin: &PinRef) -> String {
    let short = circuit
        .components()
        .get(&pin.comp)
        .and_then(|c| kind_def(c.kind).pins.iter().find(|p| p.id == pin.pin))
        .map_or(pin.pin.as_str(), |p| p.short);
    format!("{}.{}", comp_label(circuit, pin.comp), short)
}

// ------------------------------------------------------------------------------------------------------ numbers

/// Engineering format of the messages, as the UI displays values: point decimal, 4 significant digits, prefix,
/// U+00A0 before the unit, U+2212 for the sign, micro written U+00B5. Comparisons only (no logarithm), so the
/// text is the same on every platform.
pub(crate) fn si(v: f64, unit: &str) -> String {
    if !v.is_finite() {
        return "\u{2014}".to_string();
    }
    let a = v.abs();
    let (scaled, prefix) = if a == 0.0 {
        (0.0, "")
    } else if a >= 1e9 {
        (v / 1e9, "G")
    } else if a >= 1e6 {
        (v / 1e6, "M")
    } else if a >= 1e3 {
        (v / 1e3, "k")
    } else if a >= 1.0 {
        (v, "")
    } else if a >= 1e-3 {
        (v * 1e3, "m")
    } else if a >= 1e-6 {
        (v * 1e6, "\u{b5}")
    } else if a >= 1e-9 {
        (v * 1e9, "n")
    } else {
        (v * 1e12, "p")
    };
    let digits = sig4(scaled);
    if unit.is_empty() && prefix.is_empty() {
        digits
    } else {
        format!("{digits}\u{a0}{prefix}{unit}")
    }
}

fn sig4(x: f64) -> String {
    let a = x.abs();
    let decimals = if a >= 100.0 {
        1
    } else if a >= 10.0 {
        2
    } else {
        3
    };
    let mut s = format!("{a:.decimals$}");
    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
    if x < 0.0 && s != "0" {
        format!("\u{2212}{s}")
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engineering_format() {
        assert_eq!(si(4.9989934, "V"), "4.999\u{a0}V");
        assert_eq!(si(0.010066, "A"), "10.07\u{a0}mA");
        assert_eq!(si(-0.3, "V"), "\u{2212}300\u{a0}mV");
        assert_eq!(si(35.0e-6, "V"), "35\u{a0}\u{b5}V");
        assert_eq!(si(120.0, "\u{3a9}"), "120\u{a0}\u{3a9}");
        assert_eq!(si(0.0, "A"), "0\u{a0}A");
        assert_eq!(si(4095.0, ""), "4.095\u{a0}k");
        assert_eq!(si(12.0, ""), "12");
    }

    #[test]
    fn fnv_streaming_and_length_prefixes_preserve_input_boundaries() {
        for len in 1..128 {
            let bytes: Vec<u8> = (0..len).map(|i| (i * 41 + len * 13) as u8).collect();
            let mut whole = Fnv::new();
            whole.bytes(&bytes);
            for size in 1..=len {
                let mut chunked = Fnv::new();
                for chunk in bytes.chunks(size) { chunked.bytes(chunk); }
                assert_eq!(whole.finish(), chunked.finish());
            }
            let mut changed = bytes.clone();
            changed[len / 2] ^= 1;
            let mut other = Fnv::new();
            other.bytes(&changed);
            assert_ne!(whole.finish(), other.finish());
            let text = "x".repeat(len);
            let mut left = Fnv::new(); left.str(&text); left.str("x");
            let mut right = Fnv::new(); right.str(""); right.str(&format!("{text}x"));
            assert_ne!(left.finish(), right.finish());
        }
    }
}
