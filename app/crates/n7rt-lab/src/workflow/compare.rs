//! Public comparator of a hidden test (ch. 24 §2.9.5), driven by sealed data only. The instructor edition computes
//! the expected outputs with the reference consumer when it generates the pack, together with the comparison start
//! and the per-slot rules; this module compares the board's ticks with them and knows nothing of the missions.
//!
//! Rules: every expected tick must have a board tick; from `start` on, each written expected slot must match the
//! board: two rejects match, two values match within `tol`, anything else does not; a logical slot also matches
//! when the board's output equals the expected output of a tick at most `window_ticks` away.
//! Text form `n7rt-exp` v1 (sealed JSON): see [`Expected::from_json`].

use n7rt_vehicle::{BoardTick, SlotOut, TickOut};
use serde_json::Value;

pub const EXPECTED_FORMAT: &str = "n7rt-exp";
pub const EXPECTED_VERSION: u64 = 1;
const MAX_TICKS: usize = 20_000;
const MAX_SLOTS: usize = 32;

/// How one slot is compared: the absolute tolerance on values, and whether it is a logical output.
#[derive(Clone, Copy, PartialEq)]
pub struct SlotRule {
    pub tol: f64,
    pub logic: bool,
}

/// Expected outputs of a hidden test. `ticks[i]` is the tick `(i + 1)·tick_us`.
#[derive(Clone, PartialEq)]
pub struct Expected {
    pub tick_us: u64,
    pub start: usize,
    pub window_ticks: usize,
    pub rules: Vec<SlotRule>,
    pub ticks: Vec<TickOut>,
    /// Qualification: the competence (1–8) of every comparison point, `cp[i][s]` for tick `i` and slot `s`
    /// (0 where nothing is compared).
    pub cp: Option<Vec<Vec<u8>>>,
}

/// Competences of the final qualification (ch. 24 §6.3).
pub const COMPETENCES: usize = 8;

/// Sealed content: the expected values are never printed.
impl std::fmt::Debug for Expected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Expected {{ ticks: {}, slots: {} }}", self.ticks.len(), self.rules.len())
    }
}

/// Result of a comparison: counts and the first differences, for the instructor's report only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comparison {
    pub pass: bool,
    pub compared_from_us: u64,
    pub missing_ticks: usize,
    pub mismatches: usize,
    /// First differences as (tick time µs, slot), at most [`FIRST_LISTED`].
    pub first: Vec<(u64, usize)>,
}

pub const FIRST_LISTED: usize = 8;

fn matches(expected: Option<SlotOut>, got: Option<SlotOut>, tol: f64) -> bool {
    match (expected, got) {
        (Some(SlotOut::Reject), Some(SlotOut::Reject)) => true,
        (Some(SlotOut::Value(e)), Some(SlotOut::Value(g))) => (e - g).abs() <= tol,
        _ => false,
    }
}

fn slot(t: &TickOut, s: usize) -> Option<SlotOut> {
    t.slots.get(s).copied().flatten()
}

/// Whether each point of the comparison passes, from `start` on: (tick index, slot, pass).
fn points(expected: &Expected, board: &[BoardTick]) -> Vec<(usize, usize, bool)> {
    let by_t: std::collections::BTreeMap<u64, &BoardTick> = board.iter().map(|b| (b.t_us, b)).collect();
    let last = expected.ticks.len().saturating_sub(1);
    let mut out = Vec::new();
    for i in expected.start..expected.ticks.len() {
        let e = &expected.ticks[i];
        let b = by_t.get(&e.t_us);
        for (s, want) in e.slots.iter().enumerate() {
            let (Some(want), Some(rule)) = (want, expected.rules.get(s)) else { continue };
            let got = b.and_then(|b| b.slots.get(s).copied().flatten());
            let ok = b.is_some()
                && if rule.logic {
                    let lo = i.saturating_sub(expected.window_ticks);
                    let hi = (i + expected.window_ticks).min(last);
                    (lo..=hi).any(|j| matches(slot(&expected.ticks[j], s), got, rule.tol))
                } else {
                    matches(Some(*want), got, rule.tol)
                };
            out.push((i, s, ok));
        }
    }
    out
}

/// Qualification: each competence passes when all its points pass (ch. 24 §6.4); `None` without labels or when a
/// tick of the board is missing (the attempt is then cancelled, not judged).
pub fn compare_competences(expected: &Expected, board: &[BoardTick]) -> Option<[bool; COMPETENCES]> {
    let labels = expected.cp.as_ref()?;
    let times: std::collections::BTreeSet<u64> = board.iter().map(|b| b.t_us).collect();
    if expected.ticks.iter().any(|e| !times.contains(&e.t_us)) {
        return None;
    }
    let mut pass = [true; COMPETENCES];
    for (i, s, ok) in points(expected, board) {
        let cp = labels.get(i).and_then(|row| row.get(s)).copied().unwrap_or(0) as usize;
        if !ok && (1..=COMPETENCES).contains(&cp) {
            pass[cp - 1] = false;
        }
    }
    Some(pass)
}

pub fn compare_ticks(expected: &Expected, board: &[BoardTick]) -> Comparison {
    let by_t: std::collections::BTreeMap<u64, &BoardTick> = board.iter().map(|b| (b.t_us, b)).collect();
    let missing_ticks = expected.ticks.iter().filter(|e| !by_t.contains_key(&e.t_us)).count();
    let last = expected.ticks.len().saturating_sub(1);
    let mut mismatches = 0usize;
    let mut first = Vec::new();
    for i in expected.start..expected.ticks.len() {
        let e = &expected.ticks[i];
        let Some(b) = by_t.get(&e.t_us) else { continue };
        for (s, want) in e.slots.iter().enumerate() {
            let (Some(want), Some(rule)) = (want, expected.rules.get(s)) else { continue };
            let got = b.slots.get(s).copied().flatten();
            let ok = if rule.logic {
                let lo = i.saturating_sub(expected.window_ticks);
                let hi = (i + expected.window_ticks).min(last);
                (lo..=hi).any(|j| matches(slot(&expected.ticks[j], s), got, rule.tol))
            } else {
                matches(Some(*want), got, rule.tol)
            };
            if !ok {
                mismatches += 1;
                if first.len() < FIRST_LISTED {
                    first.push((e.t_us, s));
                }
            }
        }
    }
    Comparison {
        pass: missing_ticks == 0 && mismatches == 0 && !expected.ticks.is_empty(),
        compared_from_us: expected.ticks.get(expected.start).map_or(0, |t| t.t_us),
        missing_ticks,
        mismatches,
        first,
    }
}

impl Expected {
    /// Reads `{"format": "n7rt-exp", "version": 1, "tick_us": P, "start": i, "window_ticks": w,
    /// "rules": [[tol, logic], …], "ticks": [[slot, …], …]}` where a slot is `null` (not written), `"R"` (reject)
    /// or a number. Tick `i` is at `(i + 1)·P`. Strict: unknown keys, bad shapes or non-finite numbers are refused.
    pub fn from_json(v: &Value) -> Result<Expected, String> {
        let bad = || "sorties attendues illisibles".to_owned();
        let o = v.as_object().ok_or_else(bad)?;
        if o.keys().any(|k| !matches!(k.as_str(), "format" | "version" | "tick_us" | "start" | "window_ticks" | "rules" | "ticks" | "cp")) {
            return Err(bad());
        }
        if o.get("format").and_then(Value::as_str) != Some(EXPECTED_FORMAT) || o.get("version").and_then(Value::as_u64) != Some(EXPECTED_VERSION) {
            return Err(bad());
        }
        let int = |k: &str| o.get(k).and_then(Value::as_u64).ok_or_else(bad);
        let tick_us = int("tick_us")?;
        let start = int("start")? as usize;
        let window_ticks = int("window_ticks")? as usize;
        if tick_us == 0 || window_ticks > 100 {
            return Err(bad());
        }
        let rules = o
            .get("rules")
            .and_then(Value::as_array)
            .ok_or_else(bad)?
            .iter()
            .map(|r| match r.as_array().map(Vec::as_slice) {
                Some([tol, logic]) => match (tol.as_f64(), logic.as_bool()) {
                    (Some(tol), Some(logic)) if tol.is_finite() && tol >= 0.0 => Ok(SlotRule { tol, logic }),
                    _ => Err(bad()),
                },
                _ => Err(bad()),
            })
            .collect::<Result<Vec<_>, _>>()?;
        if rules.len() > MAX_SLOTS {
            return Err(bad());
        }
        let rows = o.get("ticks").and_then(Value::as_array).ok_or_else(bad)?;
        if rows.is_empty() || rows.len() > MAX_TICKS || start >= rows.len() {
            return Err(bad());
        }
        let mut ticks = Vec::with_capacity(rows.len());
        for (i, row) in rows.iter().enumerate() {
            let cells = row.as_array().ok_or_else(bad)?;
            if cells.len() > rules.len() {
                return Err(bad());
            }
            let slots = cells
                .iter()
                .map(|c| match c {
                    Value::Null => Ok(None),
                    Value::String(s) if s == "R" => Ok(Some(SlotOut::Reject)),
                    Value::Number(n) => n.as_f64().filter(|x| x.is_finite()).map(|x| Some(SlotOut::Value(x))).ok_or_else(bad),
                    _ => Err(bad()),
                })
                .collect::<Result<Vec<_>, _>>()?;
            ticks.push(TickOut { t_us: (i as u64 + 1) * tick_us, slots });
        }
        // competence labels: one row per tick, one number 0–8 per slot
        let cp = match o.get("cp") {
            None => None,
            Some(v) => {
                let rows = v.as_array().filter(|r| r.len() == ticks.len()).ok_or_else(bad)?;
                let mut out = Vec::with_capacity(rows.len());
                for (row, tick) in rows.iter().zip(&ticks) {
                    let cells = row.as_array().filter(|c| c.len() == tick.slots.len()).ok_or_else(bad)?;
                    out.push(cells.iter().map(|c| c.as_u64().filter(|x| *x <= COMPETENCES as u64).map(|x| x as u8).ok_or_else(bad)).collect::<Result<Vec<_>, _>>()?);
                }
                Some(out)
            }
        };
        Ok(Expected { tick_us, start, window_ticks, rules, ticks, cp })
    }

    /// The `n7rt-exp` v1 form read by [`Expected::from_json`] (written by the instructor edition).
    pub fn to_json(&self) -> Value {
        let cell = |s: &Option<SlotOut>| match s {
            None => Value::Null,
            Some(SlotOut::Reject) => Value::String("R".into()),
            Some(SlotOut::Value(x)) => serde_json::json!(x),
        };
        let mut v = serde_json::json!({
            "format": EXPECTED_FORMAT,
            "version": EXPECTED_VERSION,
            "tick_us": self.tick_us,
            "start": self.start,
            "window_ticks": self.window_ticks,
            "rules": self.rules.iter().map(|r| serde_json::json!([r.tol, r.logic])).collect::<Vec<_>>(),
            "ticks": self.ticks.iter().map(|t| t.slots.iter().map(cell).collect::<Vec<_>>()).collect::<Vec<_>>(),
        });
        if let (Some(cp), Some(o)) = (&self.cp, v.as_object_mut()) {
            o.insert("cp".into(), serde_json::json!(cp));
        }
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const P: u64 = 10_000;

    fn v(x: f64) -> Option<SlotOut> {
        Some(SlotOut::Value(x))
    }
    const R: Option<SlotOut> = Some(SlotOut::Reject);

    /// Slot 0 a value (tol 0.5), slot 1 a logic output (window 2 ticks).
    fn expected(rows: Vec<Vec<Option<SlotOut>>>, start: usize) -> Expected {
        Expected {
            tick_us: P,
            start,
            window_ticks: 2,
            rules: vec![SlotRule { tol: 0.5, logic: false }, SlotRule { tol: 0.5, logic: true }],
            ticks: rows.into_iter().enumerate().map(|(i, slots)| TickOut { t_us: (i as u64 + 1) * P, slots }).collect(),
            cp: None,
        }
    }

    fn board(rows: Vec<Vec<Option<SlotOut>>>) -> Vec<BoardTick> {
        rows.into_iter().enumerate().map(|(i, slots)| BoardTick { t_us: (i as u64 + 1) * P, slots }).collect()
    }

    fn sample() -> Expected {
        expected(vec![vec![v(1.0), v(0.0)], vec![v(2.0), v(0.0)], vec![R, v(1.0)], vec![v(4.0), v(1.0)], vec![None, v(1.0)]], 0)
    }

    #[test]
    fn identical_outputs_pass() {
        let e = sample();
        let b = board(e.ticks.iter().map(|t| t.slots.clone()).collect());
        let c = compare_ticks(&e, &b);
        assert!(c.pass, "{c:?}");
        assert_eq!((c.missing_ticks, c.mismatches, c.compared_from_us), (0, 0, P));
    }

    #[test]
    fn values_within_tolerance_pass_and_beyond_fail() {
        let e = sample();
        let mut rows: Vec<_> = e.ticks.iter().map(|t| t.slots.clone()).collect();
        rows[0][0] = v(1.5);
        assert!(compare_ticks(&e, &board(rows.clone())).pass);
        rows[0][0] = v(1.5001);
        let c = compare_ticks(&e, &board(rows));
        assert!(!c.pass);
        assert_eq!(c.first, vec![(P, 0)]);
    }

    #[test]
    fn reject_and_value_never_match() {
        let e = sample();
        let mut rows: Vec<_> = e.ticks.iter().map(|t| t.slots.clone()).collect();
        rows[2][0] = v(0.0);
        assert!(!compare_ticks(&e, &board(rows.clone())).pass);
        rows[2][0] = R;
        rows[1][0] = R;
        assert!(!compare_ticks(&e, &board(rows)).pass);
    }

    #[test]
    fn unwritten_board_slot_fails_and_unwritten_expected_slot_is_free() {
        let e = sample();
        let mut rows: Vec<_> = e.ticks.iter().map(|t| t.slots.clone()).collect();
        rows[4][0] = v(123.0);
        assert!(compare_ticks(&e, &board(rows.clone())).pass, "slot 0 not written by the reference at tick 5");
        rows[3][0] = None;
        assert!(!compare_ticks(&e, &board(rows)).pass);
    }

    #[test]
    fn logic_transition_is_accepted_within_the_window() {
        let e = sample();
        let mut rows: Vec<_> = e.ticks.iter().map(|t| t.slots.clone()).collect();
        rows[1][1] = v(1.0); // one tick early
        assert!(compare_ticks(&e, &board(rows.clone())).pass);
        rows[0][1] = v(1.0); // two ticks early
        assert!(compare_ticks(&e, &board(rows.clone())).pass);
        let mut late: Vec<_> = e.ticks.iter().map(|t| t.slots.clone()).collect();
        late[2][1] = v(0.0);
        late[3][1] = v(0.0);
        assert!(compare_ticks(&e, &board(late.clone())).pass, "two ticks late");
        late[4][1] = v(0.0);
        assert!(!compare_ticks(&e, &board(late)).pass, "three ticks late");
    }

    #[test]
    fn ticks_before_start_are_not_compared_but_must_exist() {
        let e = Expected { start: 2, ..sample() };
        let mut rows: Vec<_> = e.ticks.iter().map(|t| t.slots.clone()).collect();
        rows[0] = vec![R, R];
        rows[1] = vec![None, None];
        let c = compare_ticks(&e, &board(rows.clone()));
        assert!(c.pass, "{c:?}");
        assert_eq!(c.compared_from_us, 3 * P);
        let c = compare_ticks(&e, &board(rows[1..].to_vec()));
        assert!(!c.pass);
        assert!(c.missing_ticks > 0);
    }

    #[test]
    fn missing_or_extra_board_ticks() {
        let e = sample();
        let rows: Vec<_> = e.ticks.iter().map(|t| t.slots.clone()).collect();
        let mut b = board(rows.clone());
        b.remove(3);
        let c = compare_ticks(&e, &b);
        assert!(!c.pass);
        assert_eq!(c.missing_ticks, 1);
        let mut extra = board(rows);
        extra.push(BoardTick { t_us: 99 * P, slots: vec![R] });
        assert!(compare_ticks(&e, &extra).pass, "a board tick after the test is ignored");
    }

    #[test]
    fn json_round_trip_and_strictness() {
        let e = sample();
        let j = e.to_json();
        assert_eq!(Expected::from_json(&j).unwrap(), e);
        let mut extra = j.clone();
        extra["slots"] = serde_json::json!([]);
        assert!(Expected::from_json(&extra).is_err());
        for (key, value) in [
            ("version", serde_json::json!(2)),
            ("format", serde_json::json!("x")),
            ("start", serde_json::json!(5)),
            ("tick_us", serde_json::json!(0)),
            ("rules", serde_json::json!([[-1.0, false]])),
            ("ticks", serde_json::json!([])),
            ("ticks", serde_json::json!([["x"]])),
            ("ticks", serde_json::json!([[1, 2, 3]])),
        ] {
            let mut broken = j.clone();
            broken[key] = value;
            assert!(Expected::from_json(&broken).is_err(), "{key}");
        }
    }

    #[test]
    fn debug_never_prints_values() {
        assert_eq!(format!("{:?}", sample()), "Expected { ticks: 5, slots: 2 }");
    }

    #[test]
    fn an_empty_expectation_never_passes() {
        let e = Expected { ticks: vec![], start: 0, ..sample() };
        assert!(!compare_ticks(&e, &[]).pass);
    }
}
