//! The sealed content of a group pack. The plaintext of a release 0.3 pack is the wrapper
//! `{"format": "n7rt-group-content", "version": 1, "pack": <student pack v1>, "workflow": <n7rt-workflow v1>}`;
//! the plaintext of a 0.2 pack is the student pack itself (legacy mode: nominal run file, no fiche). A 0.2
//! application refuses a 0.3 pack as unreadable. The workflow stays in the Rust core: never sent to the interface,
//! never written to disk in clear.
//!
//! Workflow v1: `{"format": "n7rt-workflow", "version": 1, "mission": id, "steps": [step, …]}` where steps are
//! numbered from E0 without gap; a step holds its pool of hidden tests `{"duration_us", "vector" (n7rt-vec),
//! "expected" (n7rt-exp)}` and, for E0, the nominal truth, for E1–E3 its incident (`incident::IncidentContent`).

use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::workflow::compare::Expected;
use crate::workflow::fiche::NominalTruth;
use crate::workflow::incident::IncidentContent;
use crate::workflow::progress::VERIFIED_STEPS;
use crate::workflow::vector::HiddenVector;

pub const CONTENT_FORMAT: &str = "n7rt-group-content";
pub const CONTENT_VERSION: u64 = 1;
pub const WORKFLOW_FORMAT: &str = "n7rt-workflow";
pub const WORKFLOW_VERSION: u64 = 1;
/// Hidden tests per step (the pool replaces the server's fresh seed per attempt).
pub const MAX_POOL: usize = 8;

pub struct HiddenTest {
    pub vector: HiddenVector,
    pub expected: Expected,
}

pub struct StepContent {
    pub tests: Vec<HiddenTest>,
    pub nominal: Option<NominalTruth>,
    /// E1–E3: the incident (how it is applied, the truth of its fiche part, the key measure, the repair checks).
    pub incident: Option<IncidentContent>,
}

pub struct Workflow {
    pub mission: String,
    /// SHA-256 of the workflow section: binds a progression to this pack content.
    pub sha256: String,
    pub steps: Vec<StepContent>,
    /// Key of the files a progression keeps sealed on disk (bench copy, measure evidence) and of its tags:
    /// SHA-256 of a label and the section's bytes. Never derivable from the disk (the binding digest has no label).
    pub(crate) key: [u8; 32],
}

impl std::fmt::Debug for Workflow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Workflow {{ mission: {}, steps: {} }}", self.mission, self.steps.len())
    }
}

fn bad() -> String {
    "contenu du pack de groupe illisible".to_owned()
}

fn only(o: &serde_json::Map<String, Value>, keys: &[&str]) -> Result<(), String> {
    if o.keys().all(|k| keys.contains(&k.as_str())) { Ok(()) } else { Err(bad()) }
}

fn test(v: &Value, tick_us: u64) -> Result<HiddenTest, String> {
    let o = v.as_object().ok_or_else(bad)?;
    only(o, &["duration_us", "vector", "expected"])?;
    let duration_us = o.get("duration_us").and_then(Value::as_u64).filter(|d| *d > 0).ok_or_else(bad)?;
    let vector = HiddenVector::decode(duration_us, o.get("vector").and_then(Value::as_str).ok_or_else(bad)?)?;
    let expected = Expected::from_json(o.get("expected").ok_or_else(bad)?)?;
    if expected.tick_us != tick_us || expected.ticks.len() as u64 != duration_us / tick_us {
        return Err(bad());
    }
    Ok(HiddenTest { vector, expected })
}

/// Reads the workflow section for a pack whose tick period is `tick_period_ms`.
pub fn parse_workflow(v: &Value, mission: &str, tick_period_ms: u16) -> Result<Workflow, String> {
    let o = v.as_object().ok_or_else(bad)?;
    only(o, &["format", "version", "mission", "steps"])?;
    if o.get("format").and_then(Value::as_str) != Some(WORKFLOW_FORMAT)
        || o.get("version").and_then(Value::as_u64) != Some(WORKFLOW_VERSION)
        || o.get("mission").and_then(Value::as_str) != Some(mission)
    {
        return Err(bad());
    }
    let tick_us = tick_period_ms as u64 * 1000;
    let rows = o.get("steps").and_then(Value::as_array).ok_or_else(bad)?;
    if rows.is_empty() || rows.len() > VERIFIED_STEPS {
        return Err(bad());
    }
    let mut steps = Vec::with_capacity(rows.len());
    for (k, row) in rows.iter().enumerate() {
        let s = row.as_object().ok_or_else(bad)?;
        only(s, &["step", "tests", "nominal", "incident"])?;
        if s.get("step").and_then(Value::as_u64) != Some(k as u64) {
            return Err(bad());
        }
        let tests = s
            .get("tests")
            .and_then(Value::as_array)
            .filter(|t| !t.is_empty() && t.len() <= MAX_POOL)
            .ok_or_else(bad)?
            .iter()
            .map(|t| test(t, tick_us))
            .collect::<Result<Vec<_>, _>>()?;
        let nominal = match (k, s.get("nominal")) {
            (0, Some(n)) => Some(serde_json::from_value::<NominalTruth>(n.clone()).map_err(|_| bad())?),
            (0, None) | (_, Some(_)) => return Err(bad()),
            (_, None) => None,
        };
        let incident = match (k, s.get("incident")) {
            (1..=3, Some(i)) => Some(serde_json::from_value::<IncidentContent>(i.clone()).map_err(|_| bad())?),
            (1..=3, None) | (_, Some(_)) => return Err(bad()),
            (_, None) => None,
        };
        // the qualification is judged by competence: every hidden test of E4, and only of E4, is labelled
        if tests.iter().any(|t| t.expected.cp.is_some() != (k == crate::workflow::progress::QUALIFICATION)) {
            return Err(bad());
        }
        steps.push(StepContent { tests, nominal, incident });
    }
    let bytes = serde_json::to_vec(v).map_err(|_| bad())?;
    let sha256 = Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
    let mut h = Sha256::new();
    h.update(b"n7rt-progress-key\0");
    h.update(&bytes);
    let key = h.finalize().into();
    Ok(Workflow { mission: mission.to_owned(), sha256, steps, key })
}

/// Splits a decrypted plaintext into the student pack text (for `Pack::from_student_json`) and the workflow section,
/// if any. The caller parses the pack first, then [`parse_workflow`] with the pack's mission and tick period.
pub fn split(plaintext: &str) -> Result<(String, Option<Value>), String> {
    let v: Value = serde_json::from_str(plaintext).map_err(|_| bad())?;
    match v.get("format").and_then(Value::as_str) {
        Some(CONTENT_FORMAT) => {
            let o = v.as_object().ok_or_else(bad)?;
            only(o, &["format", "version", "pack", "workflow"])?;
            if o.get("version").and_then(Value::as_u64) != Some(CONTENT_VERSION) {
                return Err(bad());
            }
            let pack = o.get("pack").filter(|p| p.is_object()).ok_or_else(bad)?;
            let workflow = o.get("workflow").filter(|w| w.is_object()).ok_or_else(bad)?;
            Ok((serde_json::to_string(pack).map_err(|_| bad())?, Some(workflow.clone())))
        }
        _ => Ok((plaintext.to_owned(), None)),
    }
}

/// The wrapper written by the instructor edition.
pub fn wrap(pack: &Value, workflow: &Value) -> Value {
    serde_json::json!({"format": CONTENT_FORMAT, "version": CONTENT_VERSION, "pack": pack, "workflow": workflow})
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::workflow::compare::SlotRule;
    use crate::workflow::fiche::{Decision, FrameTruth};
    use n7rt_vehicle::{BusFrame, SlotOut, TickOut};

    pub(crate) fn sample_test(duration_us: u64, tick_us: u64) -> Value {
        let vector = HiddenVector::new(duration_us, vec![BusFrame { t_us: 0, id: 0x7A5, flags: 0, dlc: 1, data: vec![7] }]).unwrap();
        let n = (duration_us / tick_us) as usize;
        let expected = Expected {
            tick_us,
            start: 0,
            window_ticks: 2,
            rules: vec![SlotRule { tol: 0.05, logic: false }],
            ticks: (0..n).map(|i| TickOut { t_us: (i as u64 + 1) * tick_us, slots: vec![Some(SlotOut::Value(1.0))] }).collect(),
            cp: None,
        };
        serde_json::json!({"duration_us": duration_us, "vector": vector.encode(), "expected": expected.to_json()})
    }

    pub(crate) fn sample_truth() -> Value {
        serde_json::to_value(NominalTruth {
            frames: vec![FrameTruth { id: 0x7A5, dlc: 1, period_ms: 20, protection: None, alive: None, required: true, fields: vec![] }],
            supervision: [1, 2, 3, 1],
            decisions: [Decision::Publier, Decision::UtiliserSesDonnees, Decision::Borner, Decision::PublierLaDerniere],
        })
        .unwrap()
    }

    fn workflow() -> Value {
        serde_json::json!({
            "format": WORKFLOW_FORMAT, "version": 1, "mission": "apps",
            "steps": [{"step": 0, "nominal": sample_truth(), "tests": [sample_test(100_000, 10_000), sample_test(50_000, 10_000)]}]
        })
    }

    #[test]
    fn a_workflow_reads_and_binds_its_digest() {
        let w = parse_workflow(&workflow(), "apps", 10).unwrap();
        assert_eq!(w.steps.len(), 1);
        assert_eq!(w.steps[0].tests.len(), 2);
        assert_eq!(w.steps[0].tests[1].expected.ticks.len(), 5);
        assert!(w.steps[0].nominal.is_some());
        assert_eq!(w.sha256.len(), 64);
        let mut other = workflow();
        other["steps"][0]["tests"].as_array_mut().unwrap().pop();
        assert_ne!(parse_workflow(&other, "apps", 10).unwrap().sha256, w.sha256);
        assert_eq!(format!("{w:?}"), "Workflow { mission: apps, steps: 1 }");
    }

    #[test]
    fn workflows_are_read_strictly() {
        let mut cases: Vec<(&str, Value)> = Vec::new();
        let mut add = |why, change: &dyn Fn(&mut Value)| {
            let mut v = workflow();
            change(&mut v);
            cases.push((why, v));
        };
        add("other mission", &|v| v["mission"] = "wsf".into());
        add("version", &|v| v["version"] = 2.into());
        add("unknown key", &|v| v["notes"] = "x".into());
        add("step numbering", &|v| v["steps"][0]["step"] = 1.into());
        add("E0 without nominal truth", &|v| { v["steps"][0].as_object_mut().unwrap().remove("nominal"); });
        add("empty pool", &|v| v["steps"][0]["tests"] = serde_json::json!([]));
        add("expected of another length", &|v| v["steps"][0]["tests"][0]["duration_us"] = 200_000.into());
        add("unknown test key", &|v| v["steps"][0]["tests"][0]["seed"] = 1.into());
        add("nominal truth on E1", &|v| {
            let mut e1 = v["steps"][0].clone();
            e1["step"] = 1.into();
            v["steps"].as_array_mut().unwrap().push(e1);
        });
        for (why, v) in cases {
            assert!(parse_workflow(&v, "apps", 10).is_err(), "{why}");
        }
        assert!(parse_workflow(&workflow(), "apps", 20).is_err(), "other tick period");
    }

    #[test]
    fn split_keeps_legacy_packs_and_unwraps_new_ones() {
        let legacy = r#"{"format":"n7rt-student-pack","format_version":1}"#;
        let (pack, workflow_value) = split(legacy).unwrap();
        assert_eq!(pack, legacy);
        assert!(workflow_value.is_none());
        let pack_value = serde_json::json!({"format": "n7rt-student-pack", "format_version": 1});
        let text = serde_json::to_string(&wrap(&pack_value, &workflow())).unwrap();
        let (pack, workflow_value) = split(&text).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&pack).unwrap(), pack_value);
        assert_eq!(workflow_value.unwrap(), workflow());
        let mut broken = wrap(&pack_value, &workflow());
        broken["extra"] = 1.into();
        assert!(split(&serde_json::to_string(&broken).unwrap()).is_err());
        assert!(split("not json").is_err());
    }
}
