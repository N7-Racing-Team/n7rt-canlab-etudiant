//! Incidents E1–E3 (private spec ch. 24 §2.1–§2.9, §3.4–§3.6), generic and public: the sealed content of an
//! incident step (how the anomaly is applied, the truth of the fiche, the key measure, the repair checks), the
//! incident part of the fiche the student fills, the public key-measure rule, the unit notations and the
//! conformity of the structured fields. Nothing here holds a mission value: the instructor edition resolves them
//! on the group's variant and seals them in the group pack.
//!
//! - The free text (`justification`) is stored and exported, never read.
//! - The quantity of the key measure is imposed by the declared observed state and target type (public rule
//!   [`imposed`]); the student gives the value and its unit; the application captures the conditions and the
//!   evidence when the student presses « Consigner la mesure ».
//! - A conductor is designated by its two pins, as an unordered pair, never by a wire identifier.
//! - Diagnostic CONFORME ⇔ target, category, observed state and key measure all conform; the student is never told
//!   which one does not.

use std::collections::BTreeMap;

use n7rt_circuit::ValueMap;
use serde::{Deserialize, Serialize};

use crate::phys::WireDefect;

const MAX_TEXT: usize = 2_000;
const MAX_NAME: usize = 64;

/// A pin of the bench by designator and pin identifier: `["X9", "k"]`.
pub type PinName = (String, String);

// ------------------------------------------------------------------------------------------------ public lists

/// Catégorie (public definitions, mutually exclusive).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Category {
    #[serde(rename = "SENSOR/PHYSICAL")]
    SensorPhysical,
    #[serde(rename = "SOURCE_ECU/APPLICATION")]
    SourceEcuApplication,
    #[serde(rename = "MESSAGE/E2E")]
    MessageE2e,
    #[serde(rename = "CAN_BUS/PROTOCOL")]
    CanBusProtocol,
}

/// État observé, in the public order (the truth is the first line that holds).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Observed {
    #[serde(rename = "TRAMES_ABSENTES")]
    FramesAbsent,
    #[serde(rename = "TRAMES_FIGÉES")]
    FramesFrozen,
    #[serde(rename = "PROTECTION_NON_CONFORME")]
    ProtectionNonConforming,
    #[serde(rename = "SÉQUENCE_NON_CONFORME")]
    SequenceNonConforming,
    #[serde(rename = "DÉFAUT_SIGNALÉ_PAR_LA_SOURCE")]
    FaultReportedBySource,
    #[serde(rename = "VALEUR_HORS_PLAGE")]
    ValueOutOfRange,
    #[serde(rename = "VALEUR_INCOHÉRENTE")]
    ValueInconsistent,
    #[serde(rename = "AUCUNE_ANOMALIE_SUR_LE_BUS")]
    NoAnomalyOnBus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TargetKind {
    #[serde(rename = "CONDUCTEUR")]
    Conductor,
    #[serde(rename = "COMPOSANT")]
    Component,
    #[serde(rename = "IDENTIFIANT")]
    Identifier,
}

/// What the key measure is, by the public rule (ch. 24 §3.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quantity {
    Rate,
    Increment,
    Proportion,
    Voltage,
    Raw,
    Value,
    SupplyVoltage,
}

pub const UNITS_RATE: &[&str] = &["trames/s"];
pub const UNITS_NONE: &[&str] = &["sans unité"];
pub const UNITS_PROPORTION: &[&str] = &["%", "fraction"];
pub const UNITS_VOLTAGE: &[&str] = &["V", "mV"];
pub const UNITS_VALUE: &[&str] = &["%", "fraction", "tr/min", "Hz", "°", "rad", "bar", "kPa", "0/1"];

/// The quantity imposed by the declared state (and target type), its wording and the accepted units.
pub fn imposed(state: Observed, kind: Option<TargetKind>) -> (Quantity, String, &'static [&'static str]) {
    let concerned = match kind {
        Some(TargetKind::Component) => "de la trame DATA de l'ECU cible",
        _ => "de l'identifiant cible",
    };
    match state {
        Observed::FramesAbsent => (Quantity::Rate, format!("Nombre de trames {concerned} reçues en 1,000 s"), UNITS_RATE),
        Observed::FramesFrozen => {
            (Quantity::Increment, "Incrément du compteur entre deux trames consécutives de la trame concernée".into(), UNITS_NONE)
        }
        Observed::ProtectionNonConforming => (
            Quantity::Proportion,
            "Proportion des trames concernées dont la protection est conforme, sur 100 trames consécutives".into(),
            UNITS_PROPORTION,
        ),
        Observed::SequenceNonConforming => (Quantity::Rate, "Nombre de trames concernées reçues en 1,000 s".into(), UNITS_RATE),
        Observed::FaultReportedBySource => (
            Quantity::Voltage,
            match kind {
                Some(TargetKind::Component) => "Tension continue, par rapport à la broche de masse de l'ECU source, à la broche de l'ECU source reliée à la sortie de la cible".into(),
                _ => "Tension continue, par rapport à la broche de masse de l'ECU source, à la broche de l'ECU source où aboutit la cible".into(),
            },
            UNITS_VOLTAGE,
        ),
        Observed::ValueOutOfRange => (Quantity::Raw, "Valeur brute (entier) du signal concerné".into(), UNITS_NONE),
        Observed::ValueInconsistent => (
            Quantity::Value,
            "Valeur physique transmise par l'ECU source pour le signal qui dépend de la cible, moyenne sur 1 s, aux conditions capturées".into(),
            UNITS_VALUE,
        ),
        Observed::NoAnomalyOnBus => (Quantity::SupplyVoltage, "Tension à la broche LV_IN de l'ECU source".into(), UNITS_VOLTAGE),
    }
}

/// Public definitions of the categories, in their order (ch. 24 §3.6, verbatim).
pub const CATEGORIES: [(Category, &str, &str); 4] = [
    (Category::SensorPhysical, "SENSOR/PHYSICAL", "Défaut du capteur, de son faisceau, de son montage ou de la grandeur physique. Les trames de l'ECU source restent conformes à leur protection et à leur rythme"),
    (Category::SourceEcuApplication, "SOURCE_ECU/APPLICATION", "Défaut de l'ECU source lui-même (logique, configuration, étalonnage, alimentation de l'ECU). Ses trames restent conformes à leur protection ; leur contenu ou leur présence est faux"),
    (Category::MessageE2e, "MESSAGE/E2E", "Le message ne passe pas le contrôle de protection de bout en bout, ou un autre émetteur utilise le même identifiant"),
    (Category::CanBusProtocol, "CAN_BUS/PROTOCOL", "Défaut de la couche CAN : trames d'erreur, acquittement, bus-off, débit"),
];

/// Public definitions of the observed states, in the public order: the truth is the first line that holds
/// (ch. 24 §3.6, verbatim).
pub const STATES: [(Observed, &str, &str); 8] = [
    (Observed::FramesAbsent, "TRAMES_ABSENTES", "Les trames n'apparaissent plus sur le bus"),
    (Observed::FramesFrozen, "TRAMES_FIGÉES", "Présentes à leur rythme, contenu et compteur identiques d'une trame à l'autre"),
    (Observed::ProtectionNonConforming, "PROTECTION_NON_CONFORME", "Au moins une trame sur 100 a une protection non conforme"),
    (Observed::SequenceNonConforming, "SÉQUENCE_NON_CONFORME", "Protection conforme, mais compteur discontinu ou trames en surnombre"),
    (Observed::FaultReportedBySource, "DÉFAUT_SIGNALÉ_PAR_LA_SOURCE", "Trames conformes ; la source signale un défaut (statut différent de « OK » ou code d'invalidité)"),
    (Observed::ValueOutOfRange, "VALEUR_HORS_PLAGE", "Trames conformes, statut OK, valeur hors de la plage codable ou code d'invalidité sous statut OK"),
    (Observed::ValueInconsistent, "VALEUR_INCOHÉRENTE", "Trames conformes, statut OK, valeur dans la plage mais contraire à la redondance, à la grandeur imposée au banc ou à une autre grandeur transmise"),
    (Observed::NoAnomalyOnBus, "AUCUNE_ANOMALIE_SUR_LE_BUS", "Rien de ce qui précède"),
];

/// The instruction shown next to the key measure (ch. 24 §3.5, verbatim).
pub const MEASURE_HINT: &str = "Relevez cette grandeur dans des conditions où l'anomalie se manifeste, sans bouger les curseurs avant d'appuyer sur Consigner.";

/// `value` in `unit` expressed in the canonical unit `canon`; `None` for two different quantities (notations of the
/// same quantity are converted: V/mV, tr/min/Hz with N = 60 teeth, %/fraction, °/rad, bar/kPa).
pub fn to_canon(value: f64, unit: &str, canon: &str) -> Option<f64> {
    if !value.is_finite() {
        return None;
    }
    let factor = match (unit, canon) {
        (u, c) if u == c => 1.0,
        ("mV", "V") => 1e-3,
        ("V", "mV") => 1e3,
        ("fraction", "%") => 100.0,
        ("%", "fraction") => 1e-2,
        ("Hz", "tr/min") | ("tr/min", "Hz") => 1.0,
        ("rad", "°") => 180.0 / std::f64::consts::PI,
        ("°", "rad") => std::f64::consts::PI / 180.0,
        ("kPa", "bar") => 1e-2,
        ("bar", "kPa") => 1e2,
        _ => return None,
    };
    Some(value * factor)
}

// ------------------------------------------------------------------------------------------------ sealed content

/// How the anomaly is applied to the live vehicle (sealed; never shown).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Apply {
    /// A hidden defect on the wire joining `a` and `b` (`b` on the ECU side).
    WireDefect { a: PinName, b: PinName, defect: WireDefect },
    /// Corruption after protection of the frames `id` ([`n7rt_vehicle::FrameFault`]).
    FrameFault { id: u32, byte: u8, mask: u8, period: u16, burst: u16, phase: u16 },
    /// A mechanical setting outside the undo history.
    Setting { designator: String, param: String, value: f64 },
    /// Source ECU parameters ([`n7rt_vehicle::Pack::with_source_overrides`]).
    SourceOverride { set: Vec<(String, serde_json::Value)> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum Target {
    #[serde(rename = "CONDUCTEUR")]
    Conductor { pins: (PinName, PinName) },
    #[serde(rename = "COMPOSANT")]
    Component { designator: String },
    #[serde(rename = "IDENTIFIANT")]
    Identifier { id: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IncidentTruth {
    pub target: Target,
    pub category: Category,
    pub state: Observed,
}

/// The key measure of the incident, for its true observed state; `factor` is the pertinence factor (the value
/// without the anomaly must differ by more than `factor` × the tolerance).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "quantity", rename_all = "snake_case", deny_unknown_fields)]
pub enum MeasureSpec {
    /// DC voltage of `pin` against `reference` at the captured conditions, steady state; tolerance
    /// max(`tol_abs`, `tol_rel` × |value|).
    Voltage { pin: PinName, reference: PinName, tol_abs: f64, tol_rel: f64, factor: f64 },
    /// Percentage of the last 100 frames `id` whose protection conforms; tolerance `tol_points`.
    Proportion { id: u32, tol_points: f64, factor: f64 },
    /// Frames `id` in the last 1.000 s; tolerance `tol`.
    Rate { id: u32, tol: f64, factor: f64 },
    /// Counter increment between the last two frames `id`, modulo the counter's range `modulo`; exact.
    Increment { id: u32, modulo: i64, factor: f64 },
    /// Raw value of the signal of source `source` in the last frame `id`; exact.
    Raw { id: u32, source: String, factor: f64 },
    /// Mean physical value of the signal of source `source` of the frames `id` over the last 1.000 s, in `canon`;
    /// tolerance max(`tol_abs`, `tol_rel` × |value|).
    Value { id: u32, source: String, canon: String, tol_abs: f64, tol_rel: f64, factor: f64 },
}

impl MeasureSpec {
    pub fn canon(&self) -> &str {
        match self {
            MeasureSpec::Voltage { .. } => "V",
            MeasureSpec::Proportion { .. } => "%",
            MeasureSpec::Rate { .. } => "trames/s",
            MeasureSpec::Increment { .. } | MeasureSpec::Raw { .. } => "sans unité",
            MeasureSpec::Value { canon, .. } => canon,
        }
    }

    /// The tolerance around an expected value.
    pub fn tolerance(&self, expected: f64) -> f64 {
        match self {
            MeasureSpec::Voltage { tol_abs, tol_rel, .. } | MeasureSpec::Value { tol_abs, tol_rel, .. } => tol_abs.max(tol_rel * expected.abs()),
            MeasureSpec::Proportion { tol_points, .. } => *tol_points,
            MeasureSpec::Rate { tol, .. } => *tol,
            MeasureSpec::Increment { .. } | MeasureSpec::Raw { .. } => 0.0,
        }
    }

    pub fn factor(&self) -> f64 {
        match self {
            MeasureSpec::Voltage { factor, .. }
            | MeasureSpec::Proportion { factor, .. }
            | MeasureSpec::Rate { factor, .. }
            | MeasureSpec::Increment { factor, .. }
            | MeasureSpec::Raw { factor, .. }
            | MeasureSpec::Value { factor, .. } => *factor,
        }
    }

    /// The student value (already in the canonical unit) conforms to `expected`, and `expected` differs enough from
    /// `without` (the same quantity without the anomaly) for the measure to be pertinent.
    pub fn judge(&self, student: f64, expected: f64, without: f64) -> bool {
        let tol = self.tolerance(expected);
        let slack = 1e-9 * expected.abs().max(1.0);
        (student - expected).abs() <= tol + slack && (expected - without).abs() > self.factor() * tol + slack
    }
}

/// What « réparé » means for the incident, judged on the real state (never declared).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum P4 {
    /// The wire object that carried the defect no longer exists (P-1 checks the pins are joined again).
    WireReplaced,
    /// A parameter within [lo, hi], and when `changed` also different from its value at activation.
    ParamInRange { designator: String, param: String, lo: f64, hi: f64, changed: bool },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Repair {
    pub p4: Vec<P4>,
    /// Parameters P-2 does not compare with the reference (the repair setting of the incident).
    pub exempt: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IncidentContent {
    pub apply: Apply,
    pub truth: IncidentTruth,
    pub measure: MeasureSpec,
    pub repair: Repair,
}

// --------------------------------------------------------------------------------------------- the fiche part

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Conducteur {
    /// « désignateur.broche »
    pub a: Option<String>,
    pub b: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cible {
    #[serde(rename = "type")]
    pub kind: Option<TargetKind>,
    #[serde(default)]
    pub conducteur: Conducteur,
    pub composant: Option<String>,
    /// « 0x… », 11 bits.
    pub identifiant: Option<String>,
}

/// The multimeter at the moment of the measure.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeterRecord {
    pub com: Option<String>,
    pub vohm: Option<String>,
    pub fonction: Option<String>,
    pub lecture: Option<String>,
}

/// Conditions captured by the application at « Consigner la mesure »: interaction states of every part of the bench
/// (by designator), simulated time, the multimeter.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Conditions {
    pub etats: BTreeMap<String, ValueMap>,
    pub t_sim_us: u64,
    pub multimetre: MeterRecord,
}

/// Written by the application at « Consigner la mesure ». The evidence itself (raw frames of the last seconds and
/// the circuit) is kept sealed beside the fiche; `preuve_sha256` binds it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Consigned {
    pub consignee_s: u64,
    /// The imposed quantity at that moment (shown; the check uses the declared state at verification).
    pub grandeur: String,
    pub conditions: Conditions,
    pub preuve_sha256: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyMeasure {
    pub valeur: Option<f64>,
    pub unite: Option<String>,
    pub consignation: Option<Consigned>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IncidentPart {
    pub rang: u8,
    #[serde(default)]
    pub cible: Cible,
    pub categorie: Option<Category>,
    pub etat_observe: Option<Observed>,
    #[serde(default)]
    pub mesure_cle: KeyMeasure,
    /// Free text: never checked.
    #[serde(default)]
    pub justification: String,
}

/// « 0x7A5 » → 0x7A5 (11 bits).
pub fn parse_id(text: &str) -> Option<u32> {
    let t = text.trim();
    let h = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X"))?;
    u32::from_str_radix(h, 16).ok().filter(|v| *v <= 0x7FF)
}

/// « X9.k » → ("X9", "k").
pub fn parse_pin(text: &str) -> Option<PinName> {
    let (d, p) = text.trim().split_once('.')?;
    (!d.is_empty() && !p.is_empty() && !p.contains('.')).then(|| (d.to_owned(), p.to_owned()))
}

/// Shape of a part as the interface may send it: sizes, formats and units only (never compared here).
pub fn shape(part: &IncidentPart) -> Result<(), String> {
    let bad = |what: &str| Err(format!("Champ de la fiche d'incident invalide : {what}."));
    if part.justification.chars().count() > MAX_TEXT {
        return bad("justification trop longue");
    }
    let names = [&part.cible.conducteur.a, &part.cible.conducteur.b, &part.cible.composant, &part.cible.identifiant];
    if names.iter().any(|n| n.as_ref().is_some_and(|n| n.chars().count() > MAX_NAME)) {
        return bad("désignation trop longue");
    }
    for p in [&part.cible.conducteur.a, &part.cible.conducteur.b].into_iter().flatten() {
        if parse_pin(p).is_none() {
            return bad("broche (désignateur.broche)");
        }
    }
    if part.cible.identifiant.as_ref().is_some_and(|i| parse_id(i).is_none()) {
        return bad("identifiant (0x… sur 11 bits)");
    }
    if part.mesure_cle.valeur.is_some_and(|v| !v.is_finite()) {
        return bad("valeur de la mesure");
    }
    if let Some(u) = &part.mesure_cle.unite {
        let known = [UNITS_RATE, UNITS_NONE, UNITS_PROPORTION, UNITS_VOLTAGE, UNITS_VALUE].iter().any(|l| l.contains(&u.as_str()));
        if !known {
            return bad("unité");
        }
    }
    Ok(())
}

/// Labels of the controlled fields still empty (M-04). Never depends on the truth.
pub fn missing(part: &IncidentPart) -> Vec<String> {
    let mut out = Vec::new();
    let c = &part.cible;
    let target_ok = match c.kind {
        None => false,
        Some(TargetKind::Conductor) => c.conducteur.a.is_some() && c.conducteur.b.is_some(),
        Some(TargetKind::Component) => c.composant.as_ref().is_some_and(|s| !s.trim().is_empty()),
        Some(TargetKind::Identifier) => c.identifiant.is_some(),
    };
    if !target_ok {
        out.push("Cible".to_owned());
    }
    if part.categorie.is_none() {
        out.push("Catégorie".to_owned());
    }
    if part.etat_observe.is_none() {
        out.push("État observé".to_owned());
    }
    let m = &part.mesure_cle;
    if m.consignation.is_none() {
        out.push("Mesure clé : consigner la mesure".to_owned());
    }
    if m.valeur.is_none() {
        out.push("Mesure clé : valeur".to_owned());
    }
    if m.unite.is_none() {
        out.push("Mesure clé : unité".to_owned());
    }
    out
}

/// The designated target equals the truth (a conductor as an unordered pair of pins).
pub fn target_conforms(c: &Cible, truth: &Target) -> bool {
    match (c.kind, truth) {
        (Some(TargetKind::Conductor), Target::Conductor { pins: (x, y) }) => {
            match (c.conducteur.a.as_deref().and_then(parse_pin), c.conducteur.b.as_deref().and_then(parse_pin)) {
                (Some(a), Some(b)) => (&a == x && &b == y) || (&a == y && &b == x),
                _ => false,
            }
        }
        (Some(TargetKind::Component), Target::Component { designator }) => c.composant.as_deref().map(str::trim) == Some(designator.as_str()),
        (Some(TargetKind::Identifier), Target::Identifier { id }) => c.identifiant.as_deref().and_then(parse_id) == Some(*id),
        _ => false,
    }
}

/// The structured fields other than the measure conform: target, category, observed state.
pub fn fields_conform(part: &IncidentPart, truth: &IncidentTruth) -> bool {
    target_conforms(&part.cible, &truth.target) && part.categorie == Some(truth.category) && part.etat_observe == Some(truth.state)
}

/// The student's key measure in the canonical unit of `spec`, when its unit is a notation of that quantity.
pub fn student_value(part: &IncidentPart, spec: &MeasureSpec) -> Option<f64> {
    let m = &part.mesure_cle;
    to_canon(m.valeur?, m.unite.as_deref()?, spec.canon())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pin(d: &str, p: &str) -> PinName {
        (d.to_owned(), p.to_owned())
    }

    fn part() -> IncidentPart {
        IncidentPart {
            rang: 1,
            cible: Cible {
                kind: Some(TargetKind::Conductor),
                conducteur: Conducteur { a: Some("R7.p2".into()), b: Some("Q3.in".into()) },
                ..Default::default()
            },
            categorie: Some(Category::CanBusProtocol),
            etat_observe: Some(Observed::ValueOutOfRange),
            mesure_cle: KeyMeasure {
                valeur: Some(120.0),
                unite: Some("mV".into()),
                consignation: Some(Consigned { consignee_s: 1, grandeur: "x".into(), conditions: Conditions::default(), preuve_sha256: "0".repeat(64) }),
            },
            justification: "texte libre".into(),
        }
    }

    fn truth() -> IncidentTruth {
        // a combination of no mission (their truths are private)
        IncidentTruth { target: Target::Conductor { pins: (pin("Q3", "in"), pin("R7", "p2")) }, category: Category::CanBusProtocol, state: Observed::ValueOutOfRange }
    }

    #[test]
    fn public_strings_are_exact() {
        let j = serde_json::to_string(&part()).unwrap();
        for s in ["\"type\":\"CONDUCTEUR\"", "\"CAN_BUS/PROTOCOL\"", "\"VALEUR_HORS_PLAGE\""] {
            assert!(j.contains(s), "{s} in {j}");
        }
        let back: IncidentPart = serde_json::from_str(&j).unwrap();
        assert_eq!(back, part());
        assert!(serde_json::from_str::<IncidentPart>(&j.replace("\"rang\"", "\"rank\"")).is_err(), "unknown fields refused");
    }

    #[test]
    fn the_fields_conform_and_the_free_text_never_counts() {
        let t = truth();
        assert!(fields_conform(&part(), &t), "unordered pin pair");
        let mut p = part();
        p.justification = "tout autre chose".into();
        assert!(fields_conform(&p, &t));
        for change in 0..4 {
            let mut p = part();
            match change {
                0 => p.cible.conducteur.b = Some("Q3.out".into()),
                1 => p.categorie = Some(Category::MessageE2e),
                2 => p.etat_observe = Some(Observed::ValueInconsistent),
                _ => p.cible.kind = Some(TargetKind::Component),
            }
            assert!(!fields_conform(&p, &t), "change {change}");
        }
        let id = Cible { kind: Some(TargetKind::Identifier), identifiant: Some("0x7a5".into()), ..Default::default() };
        assert!(target_conforms(&id, &Target::Identifier { id: 0x7A5 }));
        assert!(!target_conforms(&id, &Target::Identifier { id: 0x7A6 }));
        let comp = Cible { kind: Some(TargetKind::Component), composant: Some(" E9 ".into()), ..Default::default() };
        assert!(target_conforms(&comp, &Target::Component { designator: "E9".into() }));
    }

    #[test]
    fn units_convert_between_notations_of_one_quantity_only() {
        assert_eq!(to_canon(120.0, "mV", "V"), Some(0.12));
        assert_eq!(to_canon(0.7, "fraction", "%"), Some(70.0));
        assert_eq!(to_canon(42.0, "Hz", "tr/min"), Some(42.0));
        assert_eq!(to_canon(150.0, "kPa", "bar"), Some(1.5));
        assert!((to_canon(1.0, "rad", "°").unwrap() - 57.29577951308232).abs() < 1e-12);
        assert_eq!(to_canon(1.0, "V", "%"), None);
        assert_eq!(to_canon(f64::NAN, "V", "V"), None);
        let spec = MeasureSpec::Voltage { pin: pin("Q3", "in"), reference: pin("Q3", "gnd"), tol_abs: 0.08, tol_rel: 0.03, factor: 3.0 };
        assert_eq!(student_value(&part(), &spec), Some(0.12));
    }

    #[test]
    fn a_measure_must_be_right_and_pertinent() {
        // tolerances and values of no mission (theirs are private)
        let spec = MeasureSpec::Voltage { pin: pin("Q3", "in"), reference: pin("Q3", "gnd"), tol_abs: 0.08, tol_rel: 0.03, factor: 3.0 };
        assert!(spec.judge(1.25, 1.2, 2.4), "within ±0.08 V, far from the healthy value");
        assert!(!spec.judge(1.3, 1.2, 2.4), "outside the tolerance");
        assert!(!spec.judge(1.2, 1.2, 1.4), "healthy value too close: not pertinent");
        let p = MeasureSpec::Proportion { id: 0x7A5, tol_points: 2.0, factor: 3.0 };
        assert!(p.judge(65.5, 64.0, 100.0) && !p.judge(61.0, 64.0, 100.0));
        let exact = MeasureSpec::Raw { id: 1, source: "x".into(), factor: 1.0 };
        assert!(exact.judge(5.0, 5.0, 4.0) && !exact.judge(6.0, 5.0, 4.0) && !exact.judge(5.0, 5.0, 5.0));
    }

    #[test]
    fn completeness_and_shape_never_depend_on_the_truth() {
        assert!(missing(&part()).is_empty());
        let empty = IncidentPart { rang: 2, ..Default::default() };
        assert_eq!(missing(&empty), ["Cible", "Catégorie", "État observé", "Mesure clé : consigner la mesure", "Mesure clé : valeur", "Mesure clé : unité"]);
        assert!(shape(&part()).is_ok());
        let mut p = part();
        p.cible.identifiant = Some("0x800".into());
        assert!(shape(&p).is_err());
        let mut p = part();
        p.mesure_cle.unite = Some("furlong".into());
        assert!(shape(&p).is_err());
        let mut p = part();
        p.cible.conducteur.a = Some("R7".into());
        assert!(shape(&p).is_err());
    }

    #[test]
    fn the_public_rule_imposes_the_quantity() {
        assert_eq!(imposed(Observed::ProtectionNonConforming, Some(TargetKind::Identifier)).0, Quantity::Proportion);
        assert_eq!(imposed(Observed::FaultReportedBySource, Some(TargetKind::Conductor)).2, UNITS_VOLTAGE);
        assert_eq!(imposed(Observed::ValueInconsistent, Some(TargetKind::Component)).0, Quantity::Value);
        assert!(imposed(Observed::FramesAbsent, Some(TargetKind::Component)).1.contains("DATA de l'ECU cible"));
    }
}
