//! Text values of parameters (contract §4) and number formatting for French messages.
//!
//! Grammar: `[sign] number [prefix] [unit]`, spaces allowed between tokens, sign `+`, `-` or `−` (U+2212), decimal
//! point or comma, exponent `e±n`, prefixes p n u µ (U+00B5) μ (U+03BC) m k M G, RKM notation (4k7, 2R2, 1M5, R47,
//! 47R), optional unit that must be the parameter's, "inf" / "∞" for +∞. The value is read exactly: the complete
//! decimal string ("2.2e-9") is built first and parsed once with `f64::from_str`, never multiplied by a prefix.

/// Result of a syntactically valid text. `Num` may be non-finite when the exponent overflows ("1e999").
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Parsed {
    Num(f64),
    Inf { negative: bool },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TextError {
    /// Not the grammar of §4 (`param_parse`).
    Syntax,
    /// A unit other than the parameter's (`param_unit`).
    Unit,
}

fn is_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\u{a0}' | '\u{202f}' | '\u{2009}')
}

fn prefix_exp(c: char) -> Option<i64> {
    match c {
        'p' => Some(-12),
        'n' => Some(-9),
        'u' | '\u{b5}' | '\u{3bc}' => Some(-6),
        'm' => Some(-3),
        'k' => Some(3),
        'M' => Some(6),
        'G' => Some(9),
        _ => None,
    }
}

/// Accepted spellings of a parameter unit.
fn unit_matches(text: &str, unit: &str) -> bool {
    match unit {
        "" => false,
        "\u{3a9}" => {
            text == "\u{3a9}" || text == "\u{2126}" || text.eq_ignore_ascii_case("ohm") || text.eq_ignore_ascii_case("ohms")
        }
        "bit" => text.eq_ignore_ascii_case("bit") || text.eq_ignore_ascii_case("bits"),
        u => text.eq_ignore_ascii_case(u),
    }
}

fn starts_with_ci(s: &[char], word: &str) -> bool {
    let w: Vec<char> = word.chars().collect();
    s.len() >= w.len() && s.iter().zip(&w).all(|(a, b)| a.eq_ignore_ascii_case(b))
}

fn digits(s: &[char], i: &mut usize) -> String {
    let start = *i;
    while *i < s.len() && s[*i].is_ascii_digit() {
        *i += 1;
    }
    s[start..*i].iter().collect()
}

/// Checks the optional unit that ends the text at `i`.
fn tail(s: &[char], mut i: usize, unit: &str) -> Result<(), TextError> {
    while i < s.len() && is_space(s[i]) {
        i += 1;
    }
    let rest: String = s[i..].iter().collect();
    let rest = rest.trim_end_matches(is_space);
    if rest.is_empty() || unit_matches(rest, unit) {
        Ok(())
    } else if rest.chars().all(char::is_alphabetic) {
        Err(TextError::Unit)
    } else {
        Err(TextError::Syntax)
    }
}

/// Parses a text value for a parameter whose unit is `unit` ("" when dimensionless).
pub(crate) fn parse(text: &str, unit: &str) -> Result<Parsed, TextError> {
    let s: Vec<char> = text.chars().collect();
    let n = s.len();
    let mut i = 0;
    let skip = |i: &mut usize| {
        while *i < n && is_space(s[*i]) {
            *i += 1;
        }
    };
    skip(&mut i);
    let mut negative = false;
    if i < n && matches!(s[i], '+' | '-' | '\u{2212}') {
        negative = s[i] != '+';
        i += 1;
        skip(&mut i);
    }
    if i < n && s[i] == '\u{221e}' {
        tail(&s, i + 1, unit)?;
        return Ok(Parsed::Inf { negative });
    }
    if starts_with_ci(&s[i..], "inf") {
        // Only "inf" itself: "infinity" is not in the grammar.
        if s.get(i + 3).is_some_and(char::is_ascii_alphabetic) {
            return Err(TextError::Syntax);
        }
        tail(&s, i + 3, unit)?;
        return Ok(Parsed::Inf { negative });
    }

    let int = digits(&s, &mut i);
    let mut frac = String::new();
    let mut exp: i64 = 0;
    let rkm_letter = |c: char| c == 'R' || prefix_exp(c).is_some();
    let rkm = if i + 1 < n && rkm_letter(s[i]) && s[i + 1].is_ascii_digit() && (!int.is_empty() || s[i] == 'R') {
        // RKM: the letter is both the decimal mark and the multiplier (4k7, 2R2, R47).
        exp = prefix_exp(s[i]).unwrap_or(0);
        i += 1;
        frac = digits(&s, &mut i);
        true
    } else if i < n && s[i] == 'R' && !int.is_empty() {
        // 47R
        i += 1;
        true
    } else {
        false
    };
    if rkm {
        if i < n && (matches!(s[i], '.' | ',' | 'e' | 'E') || s[i].is_ascii_digit()) {
            return Err(TextError::Syntax);
        }
    } else {
        if i < n && matches!(s[i], '.' | ',') {
            i += 1;
            frac = digits(&s, &mut i);
        }
        if int.is_empty() && frac.is_empty() {
            return Err(TextError::Syntax);
        }
        if i < n && matches!(s[i], 'e' | 'E') {
            let mut j = i + 1;
            let mut exp_negative = false;
            if j < n && matches!(s[j], '+' | '-' | '\u{2212}') {
                exp_negative = s[j] != '+';
                j += 1;
            }
            let e = digits(&s, &mut j);
            if e.is_empty() {
                return Err(TextError::Syntax);
            }
            // Saturated: anything beyond reads as 0 or ∞ anyway.
            let magnitude = e.bytes().fold(0i64, |acc, b| (acc * 10 + i64::from(b - b'0')).min(1_000_000_000));
            exp = if exp_negative { -magnitude } else { magnitude };
            i = j;
        }
        skip(&mut i);
        let suffix: String = s[i..].iter().collect();
        if i < n && !unit_matches(suffix.trim_end_matches(is_space), unit) {
            if let Some(e) = prefix_exp(s[i]) {
                exp += e;
                i += 1;
            }
        }
    }
    tail(&s, i, unit)?;

    let literal = format!(
        "{}{}.{}e{}",
        if negative { "-" } else { "" },
        if int.is_empty() { "0" } else { &int },
        if frac.is_empty() { "0" } else { &frac },
        exp
    );
    literal.parse::<f64>().map(Parsed::Num).map_err(|_| TextError::Syntax)
}

// ------------------------------------------------------------------------------------------------- formatting

const SIGNIFICANT: usize = 6;

/// Engineering notation: mantissa in [1, 1000), at most 6 significant digits (truncated, never rounded up), trailing
/// zeros removed, decimal point; prefix p … T. Values outside the prefix range fall back to "1.5e15".
fn engineering(x: f64) -> (String, &'static str) {
    if x == 0.0 {
        return ("0".to_string(), "");
    }
    let sci = format!("{:e}", x.abs());
    let (mantissa, e) = sci.split_once('e').unwrap_or((&sci, "0"));
    let e: i32 = e.parse().unwrap_or(0);
    let sign = if x < 0.0 { "\u{2212}" } else { "" };
    let e3 = e.div_euclid(3) * 3;
    let prefix = match e3 {
        -12 => "p",
        -9 => "n",
        -6 => "\u{b5}",
        -3 => "m",
        0 => "",
        3 => "k",
        6 => "M",
        9 => "G",
        12 => "T",
        _ => return (format!("{sign}{mantissa}e{}", e.to_string().replace('-', "\u{2212}")), ""),
    };
    let int_len = (e - e3 + 1) as usize;
    let mut d: String = mantissa.chars().filter(char::is_ascii_digit).take(SIGNIFICANT).collect();
    while d.len() < int_len {
        d.push('0');
    }
    let (ip, fp) = d.split_at(int_len);
    let fp = fp.trim_end_matches('0');
    let num = if fp.is_empty() { ip.to_string() } else { format!("{ip}.{fp}") };
    (format!("{sign}{num}"), prefix)
}

fn plain(x: f64) -> String {
    let s = format!("{x}");
    match s.strip_prefix('-') {
        Some(rest) => format!("\u{2212}{rest}"),
        None => s,
    }
}

/// "10\u{a0}kΩ", "62.5\u{a0}mW", "0.5" (dimensionless numbers keep plain notation), "∞".
pub(crate) fn quantity(x: f64, unit: &str) -> String {
    if x.is_nan() {
        return "NaN".to_string();
    }
    if x.is_infinite() {
        return if x > 0.0 { "\u{221e}".to_string() } else { "\u{2212}\u{221e}".to_string() };
    }
    if unit.is_empty() {
        return plain(x);
    }
    let (num, prefix) = engineering(x);
    format!("{num}\u{a0}{prefix}{unit}")
}

/// Integer with its unit, without prefix: "1000\u{a0}Hz", "12\u{a0}bit", "3".
pub(crate) fn integer(i: i64, unit: &str) -> String {
    let s = if i < 0 { format!("\u{2212}{}", i.unsigned_abs()) } else { i.to_string() };
    if unit.is_empty() {
        s
    } else {
        format!("{s}\u{a0}{unit}")
    }
}

/// « 0 Ω hors plage [10 mΩ ; 100 MΩ] ».
pub(crate) fn out_of_range(value: &str, min: &str, max: &str) -> String {
    format!("{value} hors plage [{min} ; {max}]")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engineering_format_truncates_to_six_digits() {
        assert_eq!(quantity(0.0, "\u{3a9}"), "0\u{a0}\u{3a9}");
        assert_eq!(quantity(0.01, "\u{3a9}"), "10\u{a0}m\u{3a9}");
        assert_eq!(quantity(1e8, "\u{3a9}"), "100\u{a0}M\u{3a9}");
        assert_eq!(quantity(0.0625, "W"), "62.5\u{a0}mW");
        assert_eq!(quantity(4.7e-6, "F"), "4.7\u{a0}\u{b5}F");
        assert_eq!(quantity(1e12, "\u{3a9}"), "1\u{a0}T\u{3a9}");
        assert_eq!(quantity(1.499997, "V"), "1.49999\u{a0}V");
        assert_eq!(quantity(-5.0, "V"), "\u{2212}5\u{a0}V");
        assert_eq!(quantity(1e15, "V"), "1e15\u{a0}V");
        assert_eq!(quantity(0.5, ""), "0.5");
        assert_eq!(quantity(f64::INFINITY, "\u{3a9}"), "\u{221e}");
        assert_eq!(integer(1000, "Hz"), "1000\u{a0}Hz");
    }

    #[test]
    fn parse_builds_the_exact_literal() {
        assert_eq!(parse("2.2n", "F"), Ok(Parsed::Num(2.2e-9)));
        assert_eq!(parse("1e999", ""), Ok(Parsed::Num(f64::INFINITY)));
        assert_eq!(parse(" \u{2212} inf ", "\u{3a9}"), Ok(Parsed::Inf { negative: true }));
        assert_eq!(parse("4k7k", "\u{3a9}"), Err(TextError::Unit));
        assert_eq!(parse("4k7.5", "\u{3a9}"), Err(TextError::Syntax));
    }
}
