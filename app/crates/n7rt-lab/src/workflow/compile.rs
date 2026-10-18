//! Instructor edition: renders the sealed workflow of a group pack (release 0.3). The code is generic and public:
//! everything mission-specific comes from the group's full variant pack (instructor side only) and from the private
//! truth file of the mission; the expected outputs come from the private reference consumer, so the rendering
//! succeeds only in the instructor edition (in the public edition `RefConsumer::try_new` refuses).
//!
//! - The hidden vectors are rendered on the reference bench with the student projection of the variant, exactly as
//!   the student application would run it live: the background consumer of a student pack is neutral, so no decision
//!   of the reference reaches the decoy frames replayed to the board.
//! - The expected outputs are those of the reference consumer of the full variant, fed tick before frame; the
//!   per-slot rules (tolerance ±½ LSB, logical outputs, transition window, comparison start) are read from the full
//!   pack's slot and comparison parameters.
//! - The nominal truth is resolved on the variant from the truth file's selectors (frame roles, signal and status
//!   sources); notes and labels of the truth file are never sealed.
//!
//! Private truth file (`missions/<m>/workflow-v0.json`, « n7rt-mission-workflow » v1):
//! `{"format", "version": 1, "mission", "pool": K, "nominal": {"frames": ["data", "status"?], "signals": [source, …],
//! "statuses": [source, …], "decisions": [DD-1, DD-2, DD-3, DD-4]}, "notes": …}`. The supervision values are those
//! of the reference consumer for the mission's DATA frame (first timeout, timeout and staleness in periods of the
//! frame, tolerated consecutive losses = largest accepted counter jump − 1), read from the full pack.

use std::collections::BTreeMap;

use n7rt_circuit::persist;
use n7rt_vehicle::{NominalScenario, Pack, RefConsumer, SlotOut, Vehicle};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::phys::WireDefect;
use crate::workflow::bench::VirtualBench;
use crate::workflow::compare::{Expected, SlotRule};
use crate::workflow::fiche::{Decision, FieldTruth, FrameTruth, NominalTruth};
use crate::workflow::incident::{parse_pin, Apply, Category, IncidentContent, IncidentTruth, MeasureSpec, Observed, PinName, Repair, Target, P4};
use crate::workflow::sequence::{self, Draw, SeqAnomaly};
use crate::workflow::sealed::{wrap, MAX_POOL, WORKFLOW_FORMAT, WORKFLOW_VERSION};
use crate::workflow::vector::HiddenVector;

pub const TRUTH_FORMAT: &str = "n7rt-mission-workflow";
pub const TRUTH_VERSION: u64 = 1;
const HDR_CRC: &str = "e2e.crc";
const HDR_COUNTER: &str = "e2e.counter";
const HDR_ALIVE: &str = "alive";

fn bad(what: &str) -> String {
    format!("vérité de mission invalide : {what}")
}

/// An integer written as a JSON number or a `"0x…"` / decimal string (the pack convention).
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

/// Positions least significant bit first (pack convention: Intel `start` = LSB, Motorola `start` = MSB).
fn positions(start: u16, len: u8, motorola: bool) -> Vec<u16> {
    if !motorola {
        return (0..len as u16).map(|i| start + i).collect();
    }
    let mut msb_first = Vec::with_capacity(len as usize);
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

struct SignalDoc<'a> {
    source: &'a str,
    positions: Vec<u16>,
    v: &'a Value,
}

fn signals(frame: &Value) -> Result<Vec<SignalDoc<'_>>, String> {
    frame["signals"]
        .as_array()
        .ok_or_else(|| bad("signaux"))?
        .iter()
        .map(|s| {
            let start = s["start"].as_u64().filter(|x| *x < 64).ok_or_else(|| bad("position"))? as u16;
            let len = s["len"].as_u64().filter(|x| (1..=32).contains(x)).ok_or_else(|| bad("longueur"))? as u8;
            let motorola = match s["order"].as_str() {
                None | Some("intel") => false,
                Some("motorola") => true,
                Some(_) => return Err(bad("ordre")),
            };
            Ok(SignalDoc { source: s["source"].as_str().unwrap_or(""), positions: positions(start, len, motorola), v: s })
        })
        .collect()
}

/// The nominal truth of the group: `full` is the full variant pack JSON, `nominal` the truth file's nominal part.
pub fn nominal_truth(full: &Value, nominal: &Value) -> Result<NominalTruth, String> {
    let emitter = full["source_ecu"]["emitter"].as_str().ok_or_else(|| bad("émetteur de la mission"))?;
    let names = |key: &str| -> Result<Vec<String>, String> {
        nominal[key].as_array().ok_or_else(|| bad(key))?.iter().map(|x| x.as_str().map(str::to_owned).ok_or_else(|| bad(key))).collect()
    };
    let (frame_roles, required_signals, required_statuses) = (names("frames")?, names("signals")?, names("statuses")?);
    let mut found_signals = Vec::new();
    let mut found_statuses = Vec::new();
    let mut found_roles = Vec::new();
    let mut frames = Vec::new();
    for f in full["frames"].as_array().ok_or_else(|| bad("trames"))? {
        if f["emitter"].as_str() != Some(emitter) {
            continue;
        }
        let id = num(&f["id"]).filter(|x| (0..=0x7FF).contains(x)).ok_or_else(|| bad("identifiant"))? as u32;
        let dlc = f["dlc"].as_u64().filter(|x| *x <= 8).ok_or_else(|| bad("DLC"))? as u8;
        let period = f["period_ms"].as_f64().filter(|x| x.is_finite() && *x > 0.0).ok_or_else(|| bad("période"))?;
        let sigs = signals(f)?;
        let at = |source: &str| sigs.iter().find(|s| s.source == source).map(|s| s.positions.clone());
        let (protection, alive, role) = match f["protection"]["kind"].as_str() {
            Some("p22") => (Some((at(HDR_CRC).ok_or_else(|| bad("CRC"))?, at(HDR_COUNTER).ok_or_else(|| bad("compteur"))?)), None, "data"),
            Some("alive") => (None, Some(at(HDR_ALIVE).ok_or_else(|| bad("compteur de vie"))?), "status"),
            Some("none") => (None, None, "other"),
            _ => return Err(bad("protection")),
        };
        // statuses: the signals another signal of the frame is qualified by, with the raw « OK » value
        let ok_of = |source: &str| -> Option<i64> {
            sigs.iter().find_map(|s| (s.v["qualifier"]["source"].as_str() == Some(source)).then(|| num(&s.v["qualifier"]["ok"])).flatten())
        };
        let mut fields = Vec::new();
        for s in &sigs {
            if [HDR_CRC, HDR_COUNTER, HDR_ALIVE].contains(&s.source) {
                continue;
            }
            let ok = ok_of(s.source);
            let required = if ok.is_some() && required_statuses.iter().any(|x| x == s.source) {
                found_statuses.push(s.source.to_owned());
                true
            } else if required_signals.iter().any(|x| x == s.source) {
                found_signals.push(s.source.to_owned());
                true
            } else {
                false
            };
            let factor = s.v["factor"].as_f64().unwrap_or(1.0);
            let offset = s.v["offset"].as_f64().unwrap_or(0.0);
            if !factor.is_finite() || factor == 0.0 || !offset.is_finite() {
                return Err(bad("facteur ou offset"));
            }
            fields.push(FieldTruth {
                positions: s.positions.clone(),
                signed: s.v["signed"].as_bool().unwrap_or(false),
                factor,
                offset,
                invalid: s.v.get("sna").and_then(num),
                ok,
                required,
            });
        }
        let required = frame_roles.iter().any(|r| r == role);
        if required {
            found_roles.push(role.to_owned());
        }
        frames.push(FrameTruth { id, dlc, period_ms: period.round() as u32, protection, alive, required, fields });
    }
    let all = |want: &[String], got: &[String]| want.iter().all(|w| got.iter().filter(|g| *g == w).count() == 1);
    if frames.is_empty() || !all(&frame_roles, &found_roles) {
        return Err(bad("trames exigées introuvables"));
    }
    if !all(&required_signals, &found_signals) || !all(&required_statuses, &found_statuses) {
        return Err(bad("signaux ou statuts exigés introuvables"));
    }
    let data = frames.iter().find(|f| f.protection.is_some()).ok_or_else(|| bad("trame DATA"))?;
    let p22 = &full["consumer"]["p22"];
    let periods = |k: &str| p22[k].as_u64().and_then(|x| u32::try_from(x).ok()).ok_or_else(|| bad("supervision"));
    let jump = full["consumer"]["max_delta_counter"].as_u64().filter(|x| *x >= 1).ok_or_else(|| bad("supervision"))?;
    let sv = [
        periods("first_timeout_periods")? * data.period_ms,
        periods("timeout_periods")? * data.period_ms,
        periods("stale_periods")? * data.period_ms,
        (jump - 1) as u32,
    ];
    let decisions: Vec<Decision> = serde_json::from_value(nominal["decisions"].clone()).map_err(|_| bad("décisions"))?;
    let decisions: [Decision; 4] = decisions.try_into().map_err(|_| bad("décisions"))?;
    Ok(NominalTruth { frames, supervision: sv, decisions })
}

/// Frames of the hidden nominal test (`NominalScenario` of `scenario_seed`, vehicle jitter of `vehicle_seed`) on the
/// reference bench of the student projection `student`, with the sensor driven as the live check drives it.
pub fn render_nominal(student: &Pack, vehicle_seed: u64, scenario_seed: u64) -> Result<HiddenVector, String> {
    let circuit = persist::load(student.bench_json()).map_err(|_| bad("banc de référence illisible"))?;
    let mut bench = VirtualBench::new(circuit, student, scenario_seed).map_err(|_| bad("banc de référence"))?;
    let scenario = NominalScenario::new(student, scenario_seed);
    let duration = scenario.duration_us();
    let ranges = student.world_ranges().clone();
    let mut vehicle = Vehicle::new(student, vehicle_seed);
    let mut last = None;
    let frames = bench
        .drive(&mut vehicle, duration, &mut |ms, bench| {
            let p = scenario.position_at(ms * 1000);
            let q = if p.is_finite() { (p.clamp(0.0, 1.0) * 1000.0).round() as i64 } else { 0 };
            if last == Some(q) {
                return Ok(());
            }
            last = Some(q);
            let position = q as f64 / 1000.0;
            let values: BTreeMap<String, f64> = if ranges.is_empty() {
                [("position".to_owned(), position)].into()
            } else {
                ranges.iter().map(|(quantity, [lo, hi])| (quantity.clone(), lo + (hi - lo) * position)).collect()
            };
            bench.set_sensor(&values)
        })
        .map_err(|_| bad("trajectoire refusée par le banc"))?;
    HiddenVector::new(duration, frames)
}

fn is_value(slot: Option<&Option<SlotOut>>, v: f64) -> bool {
    matches!(slot, Some(Some(SlotOut::Value(x))) if (*x - v).abs() < 0.5)
}

/// The first compared tick of a mission with a cut output (APPS): the reference is up, every value slot published and
/// the cut at 0. The reference sets a start latch only when its first data come after its implausibility delay (the
/// frame phases of a variant decide), and lifts it with the pedal at rest; when its data come in time it never sets
/// it. A later cut is a reaction of the run, compared like any output, never the start latch: taking it for one moved
/// the start past whole segments (a group whose CP-1 then had no point at all).
fn first_compared(ticks: &[n7rt_vehicle::TickOut], rules: &[SlotRule], cut: Option<usize>) -> Result<usize, String> {
    let Some(c) = cut else { return Ok(0) };
    let never = || bad("verrou de démarrage jamais levé");
    let published = |t: &n7rt_vehicle::TickOut| rules.iter().enumerate().all(|(s, r)| r.logic || matches!(t.slots.get(s), Some(Some(SlotOut::Value(_)))));
    let up = ticks.iter().position(published).ok_or_else(never)?;
    // a cut set before the reference is up or within START_LATCH_US after: the start latch, compared from its release
    let horizon = ticks[up].t_us + START_LATCH_US;
    match ticks.iter().take_while(|t| t.t_us <= horizon).position(|t| is_value(t.slots.get(c), 1.0)) {
        Some(on) => Ok(on + ticks[on..].iter().position(|t| is_value(t.slots.get(c), 0.0) && published(t)).ok_or_else(never)?),
        None => Ok(up + ticks[up..].iter().position(|t| is_value(t.slots.get(c), 0.0)).ok_or_else(never)?),
    }
}

/// How long after the reference is up a cut is still its start latch (its implausibility delay and more).
const START_LATCH_US: u64 = 300_000;

/// Expected outputs of `vector` for the full variant pack `full`. Err outside the instructor edition, or when the
/// reference never lifts its start latch.
pub fn expected(full: &Pack, vector: &HiddenVector) -> Result<Expected, String> {
    let ticks = reference_ticks(full, vector)?;
    let tick_us = full.tick_period_ms() as u64 * 1000;
    let doc: Value = serde_json::from_str(&full.to_json()).map_err(|_| bad("pack complet"))?;
    let tol_lsb = doc["compare"]["value_tolerance_lsb"].as_f64().ok_or_else(|| bad("tolérance"))?;
    let window_ms = doc["compare"]["transition_tolerance_ms"].as_u64().ok_or_else(|| bad("fenêtre"))?;
    let slots = doc["slots"].as_array().filter(|s| !s.is_empty()).ok_or_else(|| bad("slots"))?;
    let mut rules = vec![SlotRule { tol: 0.0, logic: false }; slots.len()];
    let mut cut = None;
    for s in slots {
        let index = s["slot"].as_u64().map(|x| x as usize).filter(|x| *x < rules.len()).ok_or_else(|| bad("slot"))?;
        let lsb = s["lsb"].as_f64().filter(|x| x.is_finite() && *x >= 0.0).ok_or_else(|| bad("LSB"))?;
        rules[index] = SlotRule { tol: lsb * tol_lsb * (1.0 + 1e-6), logic: s["kind"].as_str() == Some("logic") };
        if s["output"].as_str() == Some("cut") {
            cut = Some(index);
        }
    }
    let start = first_compared(&ticks, &rules, cut)?;
    if ticks.is_empty() || start >= ticks.len() {
        return Err(bad("sorties attendues vides"));
    }
    Ok(Expected { tick_us, start, window_ticks: (window_ms * 1000 / tick_us) as usize, rules, ticks, cp: None })
}

/// The outputs of the reference consumer of `full` at every tick of `vector`, as a board running it would publish
/// them (no comparison window: a consumer of another layout may never lift its start latch).
pub fn reference_ticks(full: &Pack, vector: &HiddenVector) -> Result<Vec<n7rt_vehicle::TickOut>, String> {
    let mut consumer = RefConsumer::try_new(full)?;
    let tick_us = full.tick_period_ms() as u64 * 1000;
    if tick_us == 0 {
        return Err(bad("période de tick"));
    }
    let n = vector.duration_us / tick_us;
    let mut ticks = Vec::with_capacity(n as usize);
    let mut k = 1u64;
    for f in &vector.frames {
        while k <= n && k * tick_us <= f.t_us {
            ticks.push(consumer.on_tick(k * tick_us));
            k += 1;
        }
        consumer.on_frame(f.t_us, f.id, f.flags, &f.data);
    }
    while k <= n {
        ticks.push(consumer.on_tick(k * tick_us));
        k += 1;
    }
    Ok(ticks)
}

// ------------------------------------------------------------------------------------------------- incidents

/// The mission ECU's frame carrying the signal of `source` in the variant: its identifier, the signal's positions
/// (least significant first), the signal JSON and the frame JSON.
fn frame_of_source<'a>(doc: &'a Value, source: &str) -> Result<(u32, Vec<u16>, &'a Value, &'a Value), String> {
    let emitter = doc["source_ecu"]["emitter"].as_str().ok_or_else(|| bad("émetteur de la mission"))?;
    for f in doc["frames"].as_array().ok_or_else(|| bad("trames"))? {
        if f["emitter"].as_str() != Some(emitter) {
            continue;
        }
        let sigs = signals(f)?;
        if let Some(sig) = sigs.iter().find(|x| x.source == source) {
            let id = num(&f["id"]).filter(|x| (0..=0x7FF).contains(x)).ok_or_else(|| bad("identifiant"))? as u32;
            return Ok((id, sig.positions.clone(), sig.v, f));
        }
    }
    Err(bad("source introuvable dans les trames de la mission"))
}

/// A pin `désignateur.broche` of the reference bench (the designator and the pin must exist).
fn bench_pin(bench: &VirtualBench, text: &Value) -> Result<PinName, String> {
    let name = text.as_str().and_then(parse_pin).ok_or_else(|| bad("broche"))?;
    let pin = bench.pin(&name).ok_or_else(|| bad("broche absente du banc"))?;
    let comp = &bench.core().document().circuit().components()[&pin.comp];
    if !n7rt_circuit::kind_def(comp.kind).pins.iter().any(|d| d.id == pin.pin) {
        return Err(bad("broche absente du banc"));
    }
    Ok(name)
}

fn number(v: &Value, what: &str) -> Result<f64, String> {
    v.as_f64().filter(|x| x.is_finite()).ok_or_else(|| bad(what))
}

/// The value of a source ECU parameter after the truth's change: `scale` (multiplied) or `add` (a numeric offset,
/// resolved by the truth file).
fn override_value(doc: &Value, item: &Value) -> Result<(String, Value), String> {
    let path = item["path"].as_str().ok_or_else(|| bad("chemin de surcharge"))?.to_owned();
    let mut at = &doc["source_ecu"];
    for key in path.split('.') {
        at = &at[key];
    }
    if !at.is_number() {
        return Err(bad("paramètre de l'ECU source inconnu"));
    }
    if let Some(k) = item.get("scale") {
        let v = number(at, "paramètre")? * number(k, "facteur")?;
        return Ok((path, serde_json::json!(v)));
    }
    if let Some(d) = item.get("add") {
        // an integer parameter stays an integer
        return match (at.as_i64(), d.as_i64()) {
            (Some(x), Some(k)) => Ok((path, serde_json::json!(x + k))),
            _ => Ok((path, serde_json::json!(number(at, "paramètre")? + number(d, "décalage")?))),
        };
    }
    Err(bad("surcharge sans règle"))
}

/// The sealed content of incident `inc` (truth file) on the group's variant, how its hidden sequence applies the
/// anomaly, and its sequence. Labels and catalogue lines of the truth file are never sealed.
fn incident_content(doc: &Value, student: &Pack, inc: &Value) -> Result<(IncidentContent, SeqAnomaly, Value), String> {
    let circuit = persist::load(student.bench_json()).map_err(|_| bad("banc de référence illisible"))?;
    let bench = VirtualBench::new(circuit, student, 0)?;
    let a = &inc["apply"];
    let (apply, seq) = match a["kind"].as_str() {
        Some("wire_defect") => {
            let (pa, pb) = (bench_pin(&bench, &a["a"])?, bench_pin(&bench, &a["b"])?);
            let defect: WireDefect = serde_json::from_value(a["defect"].clone()).map_err(|_| bad("défaut de fil"))?;
            // R-9: one wire object joins the two pins on the reference bench
            let (ra, rb) = (bench.pin(&pa).ok_or_else(|| bad("broche"))?, bench.pin(&pb).ok_or_else(|| bad("broche"))?);
            let joining = bench.core().document().circuit().wires().values().filter(|w| (w.a == ra && w.b == rb) || (w.a == rb && w.b == ra)).count();
            if joining != 1 {
                return Err(bad("les deux broches ne sont pas reliées par un seul fil du banc de référence"));
            }
            (Apply::WireDefect { a: pa.clone(), b: pb.clone(), defect }, SeqAnomaly::Wire { a: pa, b: pb, defect })
        }
        Some("frame_fault") => {
            let (id, positions, _, frame) = frame_of_source(doc, a["of_source"].as_str().ok_or_else(|| bad("source"))?)?;
            if frame["protection"]["kind"].as_str() != Some("p22") {
                return Err(bad("corruption d'une trame sans protection"));
            }
            let lsb = *positions.first().ok_or_else(|| bad("position"))?;
            let (byte, bit) = ((lsb / 8) as u8, lsb % 8);
            if byte == 0 || (byte == 1 && bit < 4) {
                return Err(bad("la corruption toucherait le CRC ou le compteur"));
            }
            let int = |k: &str| a[k].as_u64().and_then(|x| u16::try_from(x).ok()).ok_or_else(|| bad(k));
            let (period, burst, phase) = (int("period")?, int("burst")?, int("phase")?);
            if period == 0 || burst > period || phase >= period {
                return Err(bad("motif de corruption"));
            }
            let f = n7rt_vehicle::FrameFault { id, byte, mask: 1 << bit, period, burst, phase };
            (Apply::FrameFault { id, byte, mask: 1 << bit, period, burst, phase }, SeqAnomaly::Frame(f))
        }
        Some("setting") => {
            let designator = a["designator"].as_str().ok_or_else(|| bad("composant"))?.to_owned();
            let param = a["param"].as_str().ok_or_else(|| bad("paramètre"))?.to_owned();
            let value = number(&a["value"], "valeur du réglage")?;
            let comp = bench.component(&designator).ok_or_else(|| bad("composant absent du banc"))?;
            if !bench.core().document().circuit().components()[&comp].params.contains_key(&param) {
                return Err(bad("paramètre absent du composant"));
            }
            (Apply::Setting { designator: designator.clone(), param: param.clone(), value }, SeqAnomaly::Setting { designator, param, value })
        }
        Some("source_override") => {
            let set = a["set"].as_array().ok_or_else(|| bad("surcharges"))?.iter().map(|x| override_value(doc, x)).collect::<Result<Vec<_>, _>>()?;
            let overridden = student.with_source_overrides(&set)?;
            (Apply::SourceOverride { set }, SeqAnomaly::Source(overridden))
        }
        _ => return Err(bad("application de l'incident")),
    };
    let t = &inc["truth"];
    let target = match t["target"]["type"].as_str() {
        Some("CONDUCTEUR") => {
            let pins = t["target"]["pins"].as_array().filter(|p| p.len() == 2).ok_or_else(|| bad("cible"))?;
            Target::Conductor { pins: (bench_pin(&bench, &pins[0])?, bench_pin(&bench, &pins[1])?) }
        }
        Some("COMPOSANT") => {
            let d = t["target"]["designator"].as_str().ok_or_else(|| bad("cible"))?;
            bench.component(d).ok_or_else(|| bad("composant cible absent du banc"))?;
            Target::Component { designator: d.to_owned() }
        }
        Some("IDENTIFIANT") => Target::Identifier { id: frame_of_source(doc, t["target"]["of_source"].as_str().ok_or_else(|| bad("cible"))?)?.0 },
        _ => return Err(bad("cible")),
    };
    let category: Category = serde_json::from_value(t["category"].clone()).map_err(|_| bad("catégorie"))?;
    let state: Observed = serde_json::from_value(t["state"].clone()).map_err(|_| bad("état observé"))?;
    let m = &inc["measure"];
    let factor = number(&m["factor"], "facteur de pertinence")?;
    let of_source = |key: &str| -> Result<(u32, &Value), String> {
        let (id, _, sig, _) = frame_of_source(doc, m[key].as_str().ok_or_else(|| bad("source de la mesure"))?)?;
        Ok((id, sig))
    };
    let optional = |key: &str| -> Result<f64, String> { m.get(key).map(|v| number(v, "tolérance")).transpose().map(|x| x.unwrap_or(0.0)) };
    let measure = match m["quantity"].as_str() {
        Some("voltage") => MeasureSpec::Voltage {
            pin: bench_pin(&bench, &m["pin"])?,
            reference: bench_pin(&bench, &m["reference"])?,
            tol_abs: number(&m["tol_abs"], "tolérance")?,
            tol_rel: number(&m["tol_rel"], "tolérance")?,
            factor,
        },
        Some("proportion") => MeasureSpec::Proportion { id: of_source("of_source")?.0, tol_points: number(&m["tol_points"], "tolérance")?, factor },
        Some("rate") => MeasureSpec::Rate { id: of_source("of_source")?.0, tol: number(&m["tol"], "tolérance")?, factor },
        Some("increment") => {
            // the counter of the frame's protection: P22 (4 bits) or an alive counter of its own width
            let (id, _, _, frame) = frame_of_source(doc, m["of_source"].as_str().ok_or_else(|| bad("source de la mesure"))?)?;
            let modulo = match frame["protection"]["kind"].as_str() {
                Some("p22") => 16,
                Some("alive") => frame["protection"]["bits"].as_u64().filter(|b| (1..=32).contains(b)).map(|b| 1i64 << b).ok_or_else(|| bad("compteur"))?,
                _ => return Err(bad("trame sans compteur")),
            };
            MeasureSpec::Increment { id, modulo, factor }
        }
        Some("raw") => {
            let source = m["source"].as_str().ok_or_else(|| bad("source"))?;
            MeasureSpec::Raw { id: of_source("source")?.0, source: source.to_owned(), factor }
        }
        Some("value") => {
            let source = m["source"].as_str().ok_or_else(|| bad("source"))?;
            let (id, sig) = of_source("source")?;
            let lsb = sig["factor"].as_f64().unwrap_or(1.0).abs();
            // the mean is taken in the signal's unit, the tolerance in LSB too: the canonical unit must be that one
            // (« 0/1 » for a signal without unit)
            let canon = m["canon"].as_str().ok_or_else(|| bad("unité canonique"))?;
            if sig["unit"].as_str().unwrap_or("0/1") != canon {
                return Err(bad("unité canonique différente de celle du signal"));
            }
            MeasureSpec::Value {
                id,
                source: source.to_owned(),
                canon: canon.to_owned(),
                tol_abs: optional("tol_abs")?.max(optional("tol_lsb")? * lsb),
                tol_rel: optional("tol_rel")?,
                factor,
            }
        }
        _ => return Err(bad("grandeur de la mesure")),
    };
    let r = &inc["repair"];
    let p4: Vec<P4> = serde_json::from_value(r["p4"].clone()).map_err(|_| bad("contrôle P-4"))?;
    for check in &p4 {
        if let P4::ParamInRange { designator, param, lo, hi, .. } = check {
            let comp = bench.component(designator).ok_or_else(|| bad("composant de P-4 absent du banc"))?;
            if !bench.core().document().circuit().components()[&comp].params.contains_key(param) || lo > hi {
                return Err(bad("contrôle P-4"));
            }
        }
    }
    let exempt: Vec<(String, String)> = serde_json::from_value(r["exempt"].clone()).map_err(|_| bad("exemptions P-2"))?;
    let content = IncidentContent { apply, truth: IncidentTruth { target, category, state }, measure, repair: Repair { p4, exempt } };
    Ok((content, seq, inc["sequence"].clone()))
}

/// The frames of the mission ECU between two instants differ (the anomaly reaches the bus).
fn differs(doc: &Value, a: &[n7rt_vehicle::BusFrame], b: &[n7rt_vehicle::BusFrame], from_us: u64, to_us: u64) -> bool {
    let emitter = doc["source_ecu"]["emitter"].as_str().unwrap_or_default();
    let ids: Vec<u32> = doc["frames"]
        .as_array()
        .map(|fs| fs.iter().filter(|f| f["emitter"].as_str() == Some(emitter)).filter_map(|f| num(&f["id"])).map(|x| x as u32).collect())
        .unwrap_or_default();
    let pick = |v: &[n7rt_vehicle::BusFrame]| -> Vec<(u64, u32, Vec<u8>)> {
        v.iter().filter(|f| ids.contains(&f.id) && f.t_us >= from_us && f.t_us <= to_us).map(|f| (f.t_us, f.id, f.data.clone())).collect()
    };
    pick(a) != pick(b)
}

/// Renders the pool of hidden tests of the incident step `step` (1–3); refuses an incident that does not reach the
/// bus during V-B, or, unless the truth file says a correct consumer is immune to it (`outputs_change: false`: a
/// defect only a wrong decoding reveals), one that does not change the expected outputs.
#[allow(clippy::too_many_arguments)]
fn incident_tests(full: &Pack, student: &Pack, doc: &Value, seq: &SeqAnomaly, sequence_doc: &Value, outputs_change: bool, pool: usize, seed: &[u8; 32], step: usize) -> Result<Vec<Value>, String> {
    let circuit = persist::load(student.bench_json()).map_err(|_| bad("banc de référence illisible"))?;
    let rest_state = VirtualBench::new(circuit, student, 0)?.sensor_state();
    let quantity = sequence_doc["quantity"].as_str().ok_or_else(|| bad("grandeur de la séquence"))?;
    let rest = *rest_state.get(quantity).ok_or_else(|| bad("grandeur absente du capteur"))?;
    let mut tests = Vec::with_capacity(pool);
    for i in 0..pool {
        let mut draw = Draw::new(derived(seed, step, i, "draw"));
        let drawn = sequence::draw(sequence_doc, rest, &mut draw)?;
        let anomaly = match seq {
            // the phase of the corruption is drawn per hidden test (the live phase is the instance's)
            SeqAnomaly::Frame(f) => SeqAnomaly::Frame(n7rt_vehicle::FrameFault { phase: (draw.next() % f.period as u64) as u16, ..*f }),
            other => other.clone(),
        };
        let (vs, ss) = (derived(seed, step, i, "vehicle"), derived(seed, step, i, "scenario"));
        let frames = sequence::render(student, &drawn, &anomaly, vs, ss)?;
        let healthy = sequence::render(student, &drawn, &SeqAnomaly::None, vs, ss)?;
        if !differs(doc, &frames, &healthy, drawn.anomaly_on_ms * 1000, drawn.anomaly_off_ms * 1000 + 200_000) {
            return Err(bad("l'incident ne se voit pas sur le bus pendant V-B"));
        }
        let duration_us = drawn.duration_ms * 1000;
        let vector = HiddenVector::new(duration_us, frames)?;
        let outputs = expected(full, &vector)?;
        let reference = expected(full, &HiddenVector::new(duration_us, healthy)?)?;
        if outputs_change && reference.ticks == outputs.ticks {
            return Err(bad("l'incident ne change pas les sorties attendues"));
        }
        tests.push(serde_json::json!({"duration_us": duration_us, "vector": vector.encode(), "expected": outputs.to_json()}));
    }
    Ok(tests)
}

/// A seed of the pool, derived from the group seed, the step and the index.
/// The incident of a group: the truth entry itself or one of its `variants` (objects whose fields replace the entry's),
/// drawn from the group seed. The pack holds the group's incident only; the other variants stay in the truth file, so
/// the diagnosis of another group (its target, its measured value) is not this group's.
pub fn group_incident(inc: &Value, seed: &[u8; 32], step: usize) -> Result<Value, String> {
    let n = incident_variants(inc)?;
    incident_variant(inc, (derived(seed, step, 0, "incident-variant") % (n as u64 + 1)) as usize)
}

/// The number of `variants` of an incident entry (0 without any).
pub fn incident_variants(inc: &Value) -> Result<usize, String> {
    match inc.get("variants") {
        None => Ok(0),
        Some(Value::Array(list)) if !list.is_empty() && list.iter().all(Value::is_object) => Ok(list.len()),
        _ => Err(bad("variantes d'incident")),
    }
}

/// Variant `k` of an incident entry: 0 the entry itself, `k` the fields of `variants[k - 1]` over the entry's (its
/// rank never changes).
pub fn incident_variant(inc: &Value, k: usize) -> Result<Value, String> {
    if k > incident_variants(inc)? {
        return Err(bad("variante d'incident"));
    }
    let mut out = inc.clone();
    let o = out.as_object_mut().ok_or_else(|| bad("incident"))?;
    let variants = o.remove("variants");
    if k > 0 {
        let fields = variants.as_ref().and_then(|v| v.get(k - 1)).and_then(Value::as_object).ok_or_else(|| bad("variante d'incident"))?;
        for (key, value) in fields {
            if key == "rank" || key == "variants" {
                return Err(bad("variante d'incident"));
            }
            o.insert(key.clone(), value.clone());
        }
    }
    Ok(out)
}

fn derived(seed: &[u8; 32], step: usize, index: usize, what: &str) -> u64 {
    let mut h = Sha256::new();
    h.update(b"n7rt-workflow-seed");
    h.update(seed);
    h.update([step as u8, index as u8]);
    h.update(what.as_bytes());
    let d = h.finalize();
    u64::from_le_bytes([d[0], d[1], d[2], d[3], d[4], d[5], d[6], d[7]])
}

/// Renders the workflow section of a group: `full` is the group's full variant, `student` its student projection,
/// `truth` the private truth file of the mission, `seed` the group seed (deterministic: same inputs, same section).
pub fn compile_workflow(full: &Pack, student: &Pack, truth: &Value, seed: &[u8; 32]) -> Result<Value, String> {
    if truth["format"].as_str() != Some(TRUTH_FORMAT) || truth["version"].as_u64() != Some(TRUTH_VERSION) {
        return Err(bad("format"));
    }
    if truth["mission"].as_str() != Some(full.mission()) || student.mission() != full.mission() {
        return Err(bad("mission"));
    }
    let pool = truth["pool"].as_u64().map(|x| x as usize).filter(|x| (1..=MAX_POOL).contains(x)).ok_or_else(|| bad("pool"))?;
    let doc: Value = serde_json::from_str(&full.to_json()).map_err(|_| bad("pack complet"))?;
    let nominal = nominal_truth(&doc, &truth["nominal"])?;
    let mut tests = Vec::with_capacity(pool);
    for i in 0..pool {
        let vector = render_nominal(student, derived(seed, 0, i, "vehicle"), derived(seed, 0, i, "scenario"))?;
        let expected = expected(full, &vector)?;
        tests.push(serde_json::json!({"duration_us": vector.duration_us, "vector": vector.encode(), "expected": expected.to_json()}));
    }
    let mut steps = vec![serde_json::json!({"step": 0, "nominal": serde_json::to_value(&nominal).map_err(|_| bad("vérité nominale"))?, "tests": tests})];
    // incidents 1, 2, 3 in order (a truth file may stop early: the pack then holds the steps it has)
    let incidents = truth.get("incidents").and_then(Value::as_array).cloned().unwrap_or_default();
    for (k, inc) in incidents.iter().enumerate() {
        let step = k + 1;
        if inc["rank"].as_u64() != Some(step as u64) || step > 3 {
            return Err(bad("rangs des incidents"));
        }
        let inc = &group_incident(inc, seed, step)?;
        let (content, seq, sequence_doc) = incident_content(&doc, student, inc)?;
        let outputs_change = inc.get("outputs_change").map_or(Some(true), Value::as_bool).ok_or_else(|| bad("outputs_change"))?;
        let tests = incident_tests(full, student, &doc, &seq, &sequence_doc, outputs_change, pool, seed, step)?;
        steps.push(serde_json::json!({"step": step, "incident": serde_json::to_value(&content).map_err(|_| bad("incident"))?, "tests": tests}));
    }
    if let Some(q) = truth.get("qualification") {
        if steps.len() != 4 {
            return Err(bad("qualification sans les trois incidents"));
        }
        if let Some(supply) = q["supply"].as_str() {
            if !crate::workflow::qualification::has_supply(student, supply) {
                return Err(bad("alimentation de la qualification absente du banc"));
            }
        }
        let seeds = |i: usize, what: &str| derived(seed, 4, i, what);
        let tests = crate::workflow::qualification::tests(full, student, &doc, q, pool, &seeds)?;
        steps.push(serde_json::json!({"step": 4, "tests": tests}));
    }
    Ok(serde_json::json!({
        "format": WORKFLOW_FORMAT,
        "version": WORKFLOW_VERSION,
        "mission": full.mission(),
        "steps": steps,
    }))
}

/// Content hook of the pack generation (`n7rt_tool::gen_packs_with`, `test_pack_with`): when the mission has a truth
/// file in the instructor directory (`missions/<m>/workflow-v0.json`), the release 0.3 wrapper rendered for the group;
/// otherwise the 0.2 content. Refused outside the instructor edition (no reference consumer to render the expected
/// outputs).
pub fn group_content(root: &std::path::Path, full: &Pack, seed: &[u8; 32]) -> Result<Option<String>, String> {
    let path = root.join("missions").join(full.mission()).join("workflow-v0.json");
    if !path.is_file() {
        return Ok(None);
    }
    let unreadable = || bad("fichier de vérité illisible");
    let truth: Value = serde_json::from_slice(&std::fs::read(&path).map_err(|_| unreadable())?).map_err(|_| unreadable())?;
    let student_text = full.student_json();
    let student = Pack::from_student_json(&student_text)?;
    let workflow = compile_workflow(full, &student, &truth, seed)?;
    let pack_value: Value = serde_json::from_str(&student_text).map_err(|_| bad("pack étudiant"))?;
    serde_json::to_string(&wrap(&pack_value, &workflow)).map(Some).map_err(|_| bad("contenu du pack"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ticks of three slots: a value, a logic output and the cut; `None` = not written, `R` = rejected.
    fn ticks(rows: &[[&str; 3]]) -> Vec<n7rt_vehicle::TickOut> {
        let out = |x: &str| match x {
            "-" => None,
            "R" => Some(SlotOut::Reject),
            v => Some(SlotOut::Value(v.parse().unwrap())),
        };
        rows.iter().enumerate().map(|(i, r)| n7rt_vehicle::TickOut { t_us: (i as u64 + 1) * 10_000, slots: r.iter().map(|x| out(x)).collect() }).collect()
    }

    #[test]
    fn a_group_gets_one_variant_of_each_incident_drawn_from_its_seed() {
        let inc = serde_json::json!({"rank": 1, "label": "X", "apply": {"a": "P1.a"}, "measure": {"pin": "P1.a"},
            "variants": [{"apply": {"a": "P2.a"}, "measure": {"pin": "P2.a"}}, {"apply": {"a": "P3.a"}}]});
        assert_eq!(incident_variants(&inc), Ok(2));
        let base = incident_variant(&inc, 0).unwrap();
        assert!(base.get("variants").is_none() && base["apply"]["a"] == "P1.a");
        let second = incident_variant(&inc, 1).unwrap();
        assert_eq!((second["apply"]["a"].as_str(), second["measure"]["pin"].as_str(), second["label"].as_str()), (Some("P2.a"), Some("P2.a"), Some("X")));
        assert_eq!(incident_variant(&inc, 2).unwrap()["measure"]["pin"], "P1.a", "fields not given stay the entry's");
        assert!(incident_variant(&inc, 3).is_err());
        assert!(incident_variant(&serde_json::json!({"rank": 1, "variants": [{"rank": 2}]}), 1).is_err(), "a variant never moves its rank");
        assert!(incident_variants(&serde_json::json!({"variants": []})).is_err());
        // deterministic per group, and the three alternatives all drawn over a few groups
        let pick = |b: u8| group_incident(&inc, &[b; 32], 1).unwrap()["apply"]["a"].as_str().unwrap().to_owned();
        assert_eq!(pick(7), pick(7));
        let seen: std::collections::BTreeSet<String> = (0..40u8).map(pick).collect();
        assert_eq!(seen.len(), 3, "{seen:?}");
        // an entry without variants is the incident of every group
        let plain = serde_json::json!({"rank": 2, "apply": {"a": "Q"}});
        assert_eq!(group_incident(&plain, &[1; 32], 2).unwrap(), plain);
    }

    #[test]
    fn the_comparison_starts_once_the_reference_is_up_whether_it_latched_or_not() {
        let rules = [SlotRule { tol: 0.1, logic: false }, SlotRule { tol: 0.0, logic: true }, SlotRule { tol: 0.0, logic: true }];
        // data late: latched at start, lifted at tick 4; the cut of tick 6 is a reaction of the run
        let latched = ticks(&[["R", "1", "0"], ["R", "1", "1"], ["5", "0", "1"], ["5", "0", "1"], ["5", "0", "0"], ["5", "0", "0"], ["5", "1", "1"]]);
        assert_eq!(first_compared(&latched, &rules, Some(2)), Ok(4));
        // data in time: never latched at start; a cut of the run, 400 ms later, is not the start latch
        let mut rows = vec![["-", "1", "0"]];
        rows.extend(std::iter::repeat_n(["5", "0", "0"], 40));
        rows.extend([["5", "1", "0"], ["5", "1", "1"], ["5", "0", "1"], ["5", "0", "0"]]);
        let on_time = ticks(&rows);
        assert_eq!(first_compared(&on_time, &rules, Some(2)), Ok(1));
        assert_eq!(first_compared(&on_time, &rules, None), Ok(0), "no cut output: everything is compared");
        // the latch set on the tick the data come, or just after: still the start latch
        let just_after = ticks(&[["R", "1", "0"], ["5", "0", "0"], ["5", "0", "1"], ["5", "0", "1"], ["5", "0", "0"]]);
        assert_eq!(first_compared(&just_after, &rules, Some(2)), Ok(4));
        let never = ticks(&[["R", "1", "1"], ["5", "0", "1"]]);
        assert!(first_compared(&never, &rules, Some(2)).is_err(), "a reference that never lifts its latch is refused");
    }

    #[test]
    fn pack_positions_match_the_fiche_convention() {
        use crate::workflow::fiche::{BitField, Order};
        // pack Motorola: start = MSB 23, 12 bits; fiche: least significant bit 28, MOTOROLA
        let fiche = BitField { ordre: Some(Order::Motorola), lsb_bit: Some(28), longueur: Some(12) };
        assert_eq!(Some(positions(23, 12, true)), fiche.positions());
        let intel = BitField { ordre: Some(Order::Intel), lsb_bit: Some(16), longueur: Some(12) };
        assert_eq!(Some(positions(16, 12, false)), intel.positions());
    }

    #[test]
    fn numbers_follow_the_pack_convention() {
        assert_eq!(num(&serde_json::json!(1957)), Some(1957));
        assert_eq!(num(&serde_json::json!("0x7A5")), Some(0x7A5));
        assert_eq!(num(&serde_json::json!(" 12 ")), Some(12));
        assert_eq!(num(&serde_json::json!("x")), None);
        assert_eq!(num(&serde_json::json!(1.5)), None);
    }

    fn pack_doc() -> Value {
        serde_json::json!({
            "source_ecu": {"emitter": "ECU_X"},
            "consumer": {"p22": {"first_timeout_periods": 12, "timeout_periods": 6, "stale_periods": 4}, "max_delta_counter": 3},
            "frames": [
                {"id": "0x7A5", "emitter": "ECU_X", "period_ms": 20, "dlc": 8, "protection": {"kind": "p22"}, "signals": [
                    {"source": "e2e.crc", "start": 0, "len": 8},
                    {"source": "e2e.counter", "start": 8, "len": 4},
                    {"source": "x.st", "start": 12, "len": 2},
                    {"source": "x.v", "start": 23, "len": 12, "order": "motorola", "factor": 0.1, "offset": -5, "sna": "0xFFF",
                     "qualifier": {"source": "x.st", "ok": 0}}]},
                {"id": 1219, "emitter": "ECU_X", "period_ms": 100, "dlc": 2, "protection": {"kind": "alive"}, "signals": [
                    {"source": "alive", "start": 0, "len": 8}, {"source": "x.mode", "start": 8, "len": 3}]},
                {"id": "0x5D2", "emitter": "VCU", "period_ms": 20, "dlc": 8, "protection": {"kind": "none"}, "signals": []}]
        })
    }

    fn nominal() -> Value {
        serde_json::json!({"frames": ["data"], "signals": ["x.v"], "statuses": ["x.st"],
                           "decisions": ["PUBLIER", "UTILISER_SES_DONNÉES", "BORNER", "PUBLIER_LA_DERNIÈRE"]})
    }

    #[test]
    fn the_nominal_truth_is_resolved_on_the_variant() {
        let t = nominal_truth(&pack_doc(), &nominal()).unwrap();
        assert_eq!(t.frames.len(), 2, "only the mission ECU's frames");
        let data = &t.frames[0];
        assert_eq!((data.id, data.dlc, data.period_ms, data.required), (0x7A5, 8, 20, true));
        assert_eq!(data.protection, Some(((0..8).collect(), (8..12).collect())));
        assert_eq!(data.fields.len(), 2, "headers are not fields");
        let st = &data.fields[0];
        assert_eq!((st.ok, st.required), (Some(0), true));
        let v = &data.fields[1];
        assert_eq!(v.positions, positions(23, 12, true));
        assert_eq!((v.factor, v.offset, v.invalid, v.ok, v.required), (0.1, -5.0, Some(0xFFF), None, true));
        let status = &t.frames[1];
        assert_eq!((status.id, status.required, status.alive.clone()), (1219, false, Some((0..8).collect())));
        assert_eq!(t.supervision, [240, 120, 80, 2], "consumer periods of the 20 ms DATA frame, counter jump − 1");
    }

    #[test]
    fn selectors_must_resolve_exactly_once() {
        for (key, value) in [
            ("signals", serde_json::json!(["x.absent"])),
            ("statuses", serde_json::json!(["x.v"])),
            ("frames", serde_json::json!(["data", "other"])),
            ("decisions", serde_json::json!(["REJETER"])),
        ] {
            let mut n = nominal();
            n[key] = value;
            assert!(nominal_truth(&pack_doc(), &n).is_err(), "{key}");
        }
    }
}
