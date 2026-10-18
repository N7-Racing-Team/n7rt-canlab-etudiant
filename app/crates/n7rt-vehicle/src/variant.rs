//! Group variants (vehicle spec ch. 01 §2.8): a full pack derived deterministically from a 32-byte group seed.
//!
//! The PRNG is the counter-based one of the engine, keyed by SHA-256 of the seed, with one stream per element:
//! - phases: whole milliseconds in [⌈jitter + 0.5 ms⌉ (≥ 1 ms) ; T − margin], all planned emissions of the
//!   hyperperiod at least the minimal spacing apart (greedy by increasing period, candidates drawn uniformly;
//!   64 attempts, else the base phases);
//! - layout: the header signals (P22 CRC and counter, alive counter) stay; the value signals go to the free bits by a
//!   randomized depth-first search (longest first), Intel or Motorola for signals longer than 8 bits (8-bit and
//!   shorter signals sit in one byte, where both orders coincide), AUTOSAR layout rule (≥ 8 bits: starts or ends on
//!   a byte boundary; shorter: within one byte); base layout when the search budget runs out;
//! - scale of the physical signals (a unit, no enumeration, 8…16 bits): factor f0/k taken in the 1-2-2.5-5 series
//!   (k integer: every base value stays exactly representable), offset o0 − r with r a round shift of the same series
//!   (span/20 ≤ |r| ≤ span, multiple of f0); the base physical range stays representable, the SNA code stays reserved
//!   and outside the emitted range, at most 16 useful bits, a signed raw range keeps 0 inside; f0 and o0 are each
//!   kept with probability ½;
//! - status enumerations (those of the qualifying signals and the source ECU channel status): permutation of their
//!   codes; qualifier codes and static ECU values follow;
//! - Data ID lists: 16 bytes per P22 frame drawn without replacement in 0x00…0xFF;
//! - jitter and drift seed: 64 bits (XORed into the session seed by the engine).
//! Identifiers, periods, DLC, protection kinds, lengths and signedness never change. The result is re-read with the
//! validation of `Pack::load`, then checked against its base ([`check`]).

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::codec::{self, ByteOrder};
use crate::pack::{self, Frame, PackData, Protection, Role, Signal};
use crate::prng;

const STREAM_PHASE: u64 = 0x1_0000;
const STREAM_LAYOUT: u64 = 0x2_0000;
const STREAM_SCALE: u64 = 0x3_0000;
const STREAM_ENUM: u64 = 0x4_0000;
const STREAM_DATA_ID: u64 = 0x5_0000;
const STREAM_JITTER_SEED: u64 = 0x6_0000;

const PHASE_ATTEMPTS: u64 = 64;
const LAYOUT_BUDGET: u32 = 100_000;
const PHASE_STEP_US: u64 = 1000;
pub(crate) const MAX_USEFUL_BITS: u32 = 16;

struct Rng {
    key: u64,
    stream: u64,
    index: u64,
}

impl Rng {
    fn new(key: u64, stream: u64) -> Rng {
        Rng { key, stream, index: 0 }
    }

    fn next(&mut self) -> u64 {
        let v = prng::draw(self.key, self.stream, self.index);
        self.index += 1;
        v
    }

    /// Uniform in 0…n−1 (n ≥ 1).
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn shuffle<T>(&mut self, v: &mut [T]) {
        for i in (1..v.len()).rev() {
            let j = self.below(i + 1);
            v.swap(i, j);
        }
    }
}

fn digest(domain: &[u8], seed: &[u8; 32]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(domain);
    h.update(seed);
    h.finalize().into()
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Integer written as a JSON number or a `"0x…"` / decimal string (the pack convention).
fn num(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => {
            let t = s.trim();
            match t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
                Some(h) => i64::from_str_radix(h, 16).ok(),
                None => t.parse().ok(),
            }
        }
        _ => None,
    }
}

pub(crate) fn variant(base: &PackData, seed: &[u8; 32]) -> Result<PackData, String> {
    if base.student {
        return Err("pack étudiant : variante impossible".into());
    }
    if !base.raw.is_object() {
        return Err("pack : document source indisponible".into());
    }
    let h = digest(b"n7rt-variant-key\0", seed);
    let key = u64::from_le_bytes(h[..8].try_into().expect("8 bytes"));
    let mut raw = base.raw.clone();

    for (fi, p) in draw_phases(base, key).into_iter().enumerate() {
        raw["frames"][fi]["phase_ms"] = json!(p as f64 / 1000.0);
    }
    for (fi, f) in base.frames.iter().enumerate() {
        if let Some(layout) = draw_layout(f, key, fi) {
            for (si, start, order) in layout {
                let s = &mut raw["frames"][fi]["signals"][si];
                s["start"] = json!(start);
                s["order"] = json!(if order == ByteOrder::Motorola { "motorola" } else { "intel" });
            }
        }
        for (si, sig) in f.signals.iter().enumerate() {
            if let Some((factor, offset, min, max)) = draw_scale(sig, &base.raw["frames"][fi]["signals"][si], key, fi, si) {
                let s = &mut raw["frames"][fi]["signals"][si];
                s["factor"] = json!(factor);
                s["offset"] = json!(offset);
                s["min"] = json!(min);
                s["max"] = json!(max);
            }
        }
    }
    permute_status_codes(base, &mut raw, key);

    let p22: Vec<usize> =
        (0..base.frames.len()).filter(|&i| matches!(base.frames[i].protection, Protection::P22 { .. })).collect();
    if p22.len() * 16 > 256 {
        return Err("variante : plus de listes de Data ID que d'octets disponibles".into());
    }
    let mut pool: Vec<u8> = (0..=255).collect();
    let mut rng = Rng::new(key, STREAM_DATA_ID);
    for i in 0..p22.len() * 16 {
        let j = i + rng.below(256 - i);
        pool.swap(i, j);
    }
    for (k, &fi) in p22.iter().enumerate() {
        let ids: Vec<Value> = pool[16 * k..16 * k + 16].iter().map(|b| json!(format!("0x{b:02X}"))).collect();
        raw["frames"][fi]["protection"]["data_ids"] = Value::Array(ids);
    }

    let jitter_seed = Rng::new(key, STREAM_JITTER_SEED).next().max(1);
    raw["jitter_seed"] = json!(format!("{jitter_seed:016x}"));
    raw["variant"] = json!(format!("G-{}", hex(&digest(b"n7rt-variant-name\0", seed)[..6])));
    raw["provenance"] = json!({ "base": base.raw["variant"].clone(), "rule": "vehicle-spec ch. 01 §2.8" });

    let text = serde_json::to_string(&raw).map_err(|_| "variante : sérialisation impossible".to_string())?;
    let d = pack::parse(&text, base.bench_json.clone())?;
    check(base, &d)?;
    Ok(d)
}

// ---------------------------------------------------------------------------------------------------- phases

/// Circular distance from `t` to its nearest neighbour in `times` (sorted) is at least `spacing`.
fn clear(times: &[u64], t: u64, hyper: u64, spacing: u64) -> bool {
    if times.is_empty() {
        return true;
    }
    let n = times.len();
    let i = times.partition_point(|&e| e < t);
    [times[i % n], times[(i + n - 1) % n]].iter().all(|&e| {
        let d = t.abs_diff(e);
        d.min(hyper - d) >= spacing
    })
}

fn draw_phases(d: &PackData, key: u64) -> Vec<u64> {
    let (hyper, spacing, margin) = (d.hyperperiod_us, d.min_spacing_us, d.min_margin_us);
    // the earliest arrival (φ − jitter) stays at least 0.5 ms inside its own evaluation window
    let min_phase = (d.jitter_us + 500).div_ceil(PHASE_STEP_US).max(1) * PHASE_STEP_US;
    let mut order: Vec<usize> = (0..d.frames.len()).collect();
    order.sort_by_key(|&i| (d.frames[i].period_us, i));
    'attempt: for attempt in 0..PHASE_ATTEMPTS {
        let mut rng = Rng::new(key, STREAM_PHASE + attempt);
        let mut times: Vec<u64> = Vec::new();
        let mut phases = vec![0u64; d.frames.len()];
        for &fi in &order {
            let period = d.frames[fi].period_us;
            let count = hyper / period;
            let mut cands = Vec::new();
            let mut p = min_phase;
            while p < period && period - p >= margin {
                if (0..count).all(|k| clear(&times, p + k * period, hyper, spacing)) {
                    cands.push(p);
                }
                p += PHASE_STEP_US;
            }
            if cands.is_empty() {
                continue 'attempt;
            }
            let p = cands[rng.below(cands.len())];
            phases[fi] = p;
            for k in 0..count {
                let t = p + k * period;
                let i = times.partition_point(|&e| e < t);
                times.insert(i, t);
            }
        }
        return phases;
    }
    d.frames.iter().map(|f| f.phase_us).collect()
}

// ---------------------------------------------------------------------------------------------------- layout

/// Positions inside the payload, distinct, and the AUTOSAR layout rule ([PRS_E2E_01451]–[PRS_E2E_01453], ch. 01 §2.5):
/// a signal of 8 bits or more starts or ends on a byte boundary, a shorter one stays within one byte.
pub(crate) fn layout_ok(pos: &[u16], nbits: usize) -> bool {
    if pos.is_empty() || pos.iter().any(|&p| p as usize >= nbits) {
        return false;
    }
    let mut seen = BTreeSet::new();
    if !pos.iter().all(|p| seen.insert(*p)) {
        return false;
    }
    let len = pos.len();
    if len >= 8 {
        pos[0] % 8 == 0 || pos[len - 1] % 8 == 7
    } else {
        pos.iter().all(|p| p / 8 == pos[0] / 8)
    }
}

fn candidates(len: usize, used: &[bool]) -> Vec<(u16, ByteOrder)> {
    let orders: &[ByteOrder] = if len > 8 { &[ByteOrder::Intel, ByteOrder::Motorola] } else { &[ByteOrder::Intel] };
    let mut v = Vec::new();
    for &order in orders {
        for start in 0..used.len() as u16 {
            let pos = codec::positions(start, len as u8, order);
            if layout_ok(&pos, used.len()) && pos.iter().all(|&p| !used[p as usize]) {
                v.push((start, order));
            }
        }
    }
    v
}

struct Search<'a> {
    frame: &'a Frame,
    items: Vec<usize>,
    used: Vec<bool>,
    rng: Rng,
    budget: u32,
    out: Vec<(usize, u16, ByteOrder)>,
}

impl Search<'_> {
    fn place(&mut self, k: usize) -> bool {
        if k == self.items.len() {
            return true;
        }
        let si = self.items[k];
        let len = self.frame.signals[si].positions.len();
        let mut c = candidates(len, &self.used);
        self.rng.shuffle(&mut c);
        for (start, order) in c {
            if self.budget == 0 {
                return false;
            }
            self.budget -= 1;
            let pos = codec::positions(start, len as u8, order);
            for &p in &pos {
                self.used[p as usize] = true;
            }
            self.out.push((si, start, order));
            if self.place(k + 1) {
                return true;
            }
            self.out.pop();
            for &p in &pos {
                self.used[p as usize] = false;
            }
        }
        false
    }
}

fn draw_layout(f: &Frame, key: u64, fi: usize) -> Option<Vec<(usize, u16, ByteOrder)>> {
    let mut used = vec![false; f.dlc as usize * 8];
    for s in f.signals.iter().filter(|s| s.role != Role::Value) {
        for &p in &s.positions {
            used[p as usize] = true;
        }
    }
    let mut items: Vec<usize> = (0..f.signals.len()).filter(|&i| f.signals[i].role == Role::Value).collect();
    items.sort_by_key(|&i| std::cmp::Reverse(f.signals[i].positions.len()));
    let mut s = Search {
        frame: f,
        items,
        used,
        rng: Rng::new(key, STREAM_LAYOUT + fi as u64),
        budget: LAYOUT_BUDGET,
        out: Vec::new(),
    };
    s.place(0).then_some(s.out)
}

// ----------------------------------------------------------------------------------------------------- scale

/// 1-2-2.5-5 series from 1e-5 to 5e5, as the nearest doubles of the decimal values.
fn series() -> Vec<f64> {
    let mut v = Vec::new();
    for e in -5..=5 {
        for m in ["1", "2", "2.5", "5"] {
            v.push(format!("{m}e{e}").parse::<f64>().expect("decimal literal"));
        }
    }
    v
}

/// The double nearest to `x` written with 12 significant digits (removes the noise of decimal arithmetic).
fn clean(x: f64) -> f64 {
    format!("{x:.11e}").parse().unwrap_or(x)
}

fn int_ratio(x: f64) -> Option<i64> {
    let r = x.round();
    (x.is_finite() && (x - r).abs() <= 1e-6 * r.abs().max(1.0)).then_some(r as i64)
}

/// Bits needed by the raw range (sign bit included for a signed signal).
pub(crate) fn useful_bits(lo: i64, hi: i64, signed: bool) -> u32 {
    let mag = lo.unsigned_abs().max(hi.unsigned_abs());
    let b = 64 - mag.leading_zeros();
    if signed {
        b + 1
    } else {
        b
    }
}

/// Raw range of the physical range [pmin ; pmax] with (f, o), if exact, representable, SNA outside, ≤ 16 bits.
fn raw_range(sig: &Signal, f: f64, o: f64, pmin: f64, pmax: f64) -> Option<(i64, i64)> {
    let lo = int_ratio((pmin - o) / f)?;
    let hi = int_ratio((pmax - o) / f)?;
    let (blo, bhi) = codec::raw_bounds(sig.positions.len() as u8, sig.signed);
    if lo > hi || lo < blo || hi > bhi || sig.sna.is_some_and(|s| (lo..=hi).contains(&s)) {
        return None;
    }
    // plausibility: a signed raw range keeps raw 0 inside (no all-negative or all-positive signed field)
    if sig.signed && !(lo <= 0 && hi >= 0) {
        return None;
    }
    (useful_bits(lo, hi, sig.signed) <= MAX_USEFUL_BITS).then_some((lo, hi))
}

fn eligible(sig: &Signal, raw_sig: &Value) -> bool {
    sig.role == Role::Value
        && raw_sig.get("enum").map_or(true, Value::is_null)
        && raw_sig.get("unit").and_then(Value::as_str).is_some_and(|u| !u.trim().is_empty())
        && (8..=MAX_USEFUL_BITS as usize).contains(&sig.positions.len())
        && sig.factor > 0.0
}

/// New (factor, offset, min, max) of a physical signal, `None` when it keeps its base scale.
fn draw_scale(sig: &Signal, raw_sig: &Value, key: u64, fi: usize, si: usize) -> Option<(f64, f64, i64, i64)> {
    if !eligible(sig, raw_sig) {
        return None;
    }
    let (f0, o0) = (sig.factor, sig.offset);
    let pmin = sig.min as f64 * f0 + o0;
    let pmax = sig.max as f64 * f0 + o0;
    let span = pmax - pmin;
    if !(span > 0.0) || raw_range(sig, f0, o0, pmin, pmax).is_none() {
        return None;
    }
    let series = series();
    let shifts: Vec<f64> = series
        .iter()
        .copied()
        .filter(|&r| r >= span / 20.0 * (1.0 - 1e-9) && r <= span * (1.0 + 1e-9) && int_ratio(r / f0).is_some())
        .flat_map(|r| [r, -r])
        .collect();
    let offsets = |f: f64| -> Vec<(f64, (i64, i64))> {
        shifts.iter().filter_map(|&r| {
            let o = clean(o0 - r);
            raw_range(sig, f, o, pmin, pmax).map(|mm| (o, mm))
        })
        .collect()
    };
    let finer: Vec<f64> = series
        .iter()
        .copied()
        .filter(|&f| f < f0 * (1.0 - 1e-9) && int_ratio(f0 / f).is_some())
        .filter(|&f| raw_range(sig, f, o0, pmin, pmax).is_some() || !offsets(f).is_empty())
        .collect();
    let mut rng = Rng::new(key, STREAM_SCALE + ((fi as u64) << 8) + si as u64);
    let f = if finer.is_empty() || rng.below(2) == 0 { f0 } else { finer[rng.below(finer.len())] };
    let shifted = offsets(f);
    let (o, (min, max)) = match raw_range(sig, f, o0, pmin, pmax) {
        Some(mm) if shifted.is_empty() || rng.below(2) == 0 => (o0, mm),
        _ if !shifted.is_empty() => shifted[rng.below(shifted.len())],
        _ => return None,
    };
    (f != f0 || o != o0).then_some((f, o, min, max))
}

// ------------------------------------------------------------------------------------------- status codes

/// Permutes the codes of the status enumerations (those of the qualifying signals and the source ECU channel
/// status); the qualifier codes and the static ECU values written with those codes follow.
fn permute_status_codes(base: &PackData, raw: &mut Value, key: u64) {
    let frames = base.raw["frames"].as_array().cloned().unwrap_or_default();
    let mut status: BTreeSet<String> = BTreeSet::new();
    if let Some(s) = base.raw["source_ecu"]["status_enum"].as_str() {
        status.insert(s.to_string());
    }
    let mut enum_of: BTreeMap<String, String> = BTreeMap::new();
    for fr in &frames {
        let sigs = fr["signals"].as_array().cloned().unwrap_or_default();
        for s in &sigs {
            if let (Some(src), Some(e)) = (s["source"].as_str(), s["enum"].as_str()) {
                enum_of.insert(src.to_string(), e.to_string());
            }
            if let Some(q) = s["qualifier"]["source"].as_str() {
                if let Some(e) = sigs.iter().find(|o| o["source"].as_str() == Some(q)).and_then(|o| o["enum"].as_str()) {
                    status.insert(e.to_string());
                }
            }
        }
    }
    let names: BTreeSet<String> =
        base.raw["enums"].as_object().map(|m| m.keys().cloned().collect()).unwrap_or_default();
    let mut maps: BTreeMap<String, BTreeMap<i64, i64>> = BTreeMap::new();
    for (i, name) in names.iter().enumerate() {
        if !status.contains(name) {
            continue;
        }
        let entries: Vec<(String, i64)> = base.raw["enums"][name]
            .as_object()
            .map(|m| m.iter().filter_map(|(k, v)| num(v).map(|c| (k.clone(), c))).collect())
            .unwrap_or_default();
        let mut codes: Vec<i64> = entries.iter().map(|e| e.1).collect();
        codes.sort_unstable();
        let mut shuffled = codes.clone();
        Rng::new(key, STREAM_ENUM + i as u64).shuffle(&mut shuffled);
        let map: BTreeMap<i64, i64> = codes.into_iter().zip(shuffled).collect();
        for (k, c) in &entries {
            raw["enums"][name][k] = json!(map[c]);
        }
        maps.insert(name.clone(), map);
    }
    // qualifier codes
    for (fi, fr) in frames.iter().enumerate() {
        let sigs = fr["signals"].as_array().cloned().unwrap_or_default();
        for (si, s) in sigs.iter().enumerate() {
            let Some(q) = s["qualifier"]["source"].as_str() else { continue };
            let Some(e) = enum_of.get(q) else { continue };
            if let (Some(map), Some(ok)) = (maps.get(e), num(&s["qualifier"]["ok"])) {
                if let Some(new) = map.get(&ok) {
                    raw["frames"][fi]["signals"][si]["qualifier"]["ok"] = json!(new);
                }
            }
        }
    }
    // static ECU values
    let ecus = base.raw["background"]["static_ecus"].as_array().cloned().unwrap_or_default();
    for (ei, ecu) in ecus.iter().enumerate() {
        let prefix = ecu["prefix"].as_str().unwrap_or_default();
        let phases = ecu["phases"].as_array().cloned().unwrap_or_default();
        for (pi, ph) in phases.iter().enumerate() {
            let Some(values) = ph["values"].as_object() else { continue };
            for (k, v) in values {
                let Some(map) = enum_of.get(&format!("{prefix}.{k}")).and_then(|e| maps.get(e)) else { continue };
                let Some(x) = v.as_f64().filter(|x| x.fract() == 0.0) else { continue };
                if let Some(new) = map.get(&(x as i64)) {
                    raw["background"]["static_ecus"][ei]["phases"][pi]["values"][k] = json!(new);
                }
            }
        }
    }
}

// ----------------------------------------------------------------------------------------------------- check

fn kind(p: &Protection) -> u8 {
    match p {
        Protection::P22 { .. } => 0,
        Protection::Alive { .. } => 1,
        Protection::None => 2,
    }
}

/// Invariants of a variant against its base, beyond the load validation (spacing, margin, overlap, header shape):
/// same identifiers, periods, DLC, emitters, protection kinds and signals; header signals in place; no value signal
/// in the P22 header; layout rule; resolution at least the base one; base physical range kept, SNA unchanged and
/// outside the emitted range, ≤ 16 useful bits for a rescaled signal; Data ID bytes all distinct.
pub(crate) fn check(base: &PackData, v: &PackData) -> Result<(), String> {
    let fail = |fi: usize, what: &str| -> Result<(), String> { Err(format!("variante : frames[{fi}] : {what}")) };
    if base.frames.len() != v.frames.len() {
        return Err("variante : matrice différente de sa base".into());
    }
    let mut bytes = BTreeSet::new();
    let mut count = 0usize;
    for (fi, (b, f)) in base.frames.iter().zip(&v.frames).enumerate() {
        if b.id != f.id
            || b.period_us != f.period_us
            || b.dlc != f.dlc
            || b.emitter != f.emitter
            || kind(&b.protection) != kind(&f.protection)
            || b.signals.len() != f.signals.len()
        {
            return fail(fi, "identifiant, période, DLC, émetteur ou protection modifié");
        }
        let nbits = f.dlc as usize * 8;
        let p22 = matches!(f.protection, Protection::P22 { .. });
        for (bs, s) in b.signals.iter().zip(&f.signals) {
            if bs.role != s.role || bs.source != s.source || bs.signed != s.signed || bs.positions.len() != s.positions.len()
            {
                return fail(fi, "signal modifié");
            }
            if s.role != Role::Value {
                if bs.positions != s.positions {
                    return fail(fi, "en-tête déplacé");
                }
                continue;
            }
            if p22 && s.positions.iter().any(|&p| p < 12) {
                return fail(fi, "signal dans l'en-tête P22");
            }
            if !layout_ok(&s.positions, nbits) {
                return fail(fi, "règle de mise en page non respectée");
            }
            if s.factor.abs() > bs.factor.abs() * (1.0 + 1e-9) {
                return fail(fi, "résolution inférieure à celle de la base");
            }
            let rescaled = s.factor != bs.factor || s.offset != bs.offset || s.min != bs.min || s.max != bs.max;
            if rescaled {
                let (bmin, bmax) = (bs.min as f64 * bs.factor + bs.offset, bs.max as f64 * bs.factor + bs.offset);
                let tol = 1e-9 * bmin.abs().max(bmax.abs()).max(1.0);
                let range_kept = (s.min as f64 * s.factor + s.offset - bmin).abs() <= tol
                    && (s.max as f64 * s.factor + s.offset - bmax).abs() <= tol;
                if !range_kept
                    || s.sna != bs.sna
                    || s.sna.is_some_and(|x| (s.min..=s.max).contains(&x))
                    || useful_bits(s.min, s.max, s.signed) > MAX_USEFUL_BITS
                    || (s.signed && !(s.min <= 0 && s.max >= 0))
                {
                    return fail(fi, "échelle hors des bornes de la variante");
                }
            }
        }
        if let Protection::P22 { data_ids } = &f.protection {
            bytes.extend(data_ids.iter().copied());
            count += data_ids.len();
        }
    }
    if bytes.len() != count {
        return Err("variante : octets de Data ID en double".into());
    }
    Ok(())
}
