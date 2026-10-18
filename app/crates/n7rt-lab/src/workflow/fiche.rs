//! The nominal part of the fiche (private spec ch. 24 §3.3, public schema) and its check against the sealed truth of
//! the group (§3.6). The student fills structured fields; the free fields (`justification`, signal `nom`, document
//! `pages`) are stored and exported but never read by the check.
//!
//! - Completeness (precondition of a verification, message M-04) is generic: every entry present has all its
//!   controlled fields filled, each list has at least one entry, supervision and decisions are filled. It never
//!   says which frames, signals or statuses are required.
//! - Conformity compares with the sealed truth: frame set (required frames present, nothing outside the mission
//!   ECU's frames), DLC exact, period ±1 ms, protection and counters by bit map; each required signal and status
//!   described exactly once and every entry describing a real field of the variant, with its sign, factor (relative
//!   error ≤ 1e-6), offset (≤ ½ factor) and invalid or OK code exact; supervision values and decisions exact. The
//!   student only ever learns « conforme » or « non conforme ».
//!
//! Bit positions (§3.3, DC-24-11): bit i of the frame is bit (i mod 8) of byte ⌊i/8⌋, byte 0 first transmitted.
//! `lsb_bit` is the position of the least significant bit; INTEL weights grow i, i+1, …; MOTOROLA weights grow up
//! the byte, then continue at bit 0 of the previous byte. A description is compared by its positions, least
//! significant first, so the same bits with the same weights are equal whatever the declared order.

use serde::{Deserialize, Serialize};

pub const FORMAT_STANDARD: &str = "STANDARD_11_BITS";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Order {
    #[serde(rename = "INTEL")]
    Intel,
    #[serde(rename = "MOTOROLA")]
    Motorola,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Sign {
    #[serde(rename = "NON_SIGNÉ")]
    Unsigned,
    #[serde(rename = "COMPLÉMENT_À_DEUX")]
    TwosComplement,
}

/// `code_invalide`: a raw integer, or explicitly none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum InvalidCode {
    Code(i64),
    Aucun(NoCode),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NoCode {
    #[serde(rename = "AUCUN")]
    Aucun,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BitField {
    pub ordre: Option<Order>,
    pub lsb_bit: Option<u16>,
    pub longueur: Option<u16>,
}

impl BitField {
    fn filled(&self) -> bool {
        self.ordre.is_some() && self.lsb_bit.is_some() && self.longueur.is_some()
    }
    fn empty(&self) -> bool {
        self.ordre.is_none() && self.lsb_bit.is_none() && self.longueur.is_none()
    }
    /// Positions least significant first, `None` when incomplete or outside a classic 8-byte frame.
    pub fn positions(&self) -> Option<Vec<u16>> {
        let (order, lsb, len) = (self.ordre?, self.lsb_bit?, self.longueur?);
        if lsb > 63 || len == 0 || len > 64 {
            return None;
        }
        let mut out = Vec::with_capacity(len as usize);
        let mut p = lsb as i32;
        for i in 0..len {
            if !(0..64).contains(&p) {
                return None;
            }
            out.push(p as u16);
            if i + 1 < len {
                p = match order {
                    Order::Intel => p + 1,
                    Order::Motorola if p % 8 == 7 => p - 15,
                    Order::Motorola => p + 1,
                };
            }
        }
        Some(out)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Protection {
    pub presente: Option<bool>,
    #[serde(default)]
    pub crc: BitField,
    #[serde(default)]
    pub compteur: BitField,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrameEntry {
    pub identifiant: Option<u32>,
    pub format: Option<String>,
    pub dlc: Option<u8>,
    pub periode_ms: Option<u32>,
    #[serde(default)]
    pub protection: Protection,
    #[serde(default)]
    pub compteur_de_vie: BitField,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignalEntry {
    /// Free text (the student's name for the signal): never checked.
    #[serde(default)]
    pub nom: String,
    pub identifiant: Option<u32>,
    #[serde(default)]
    pub slots: Vec<u8>,
    pub ordre: Option<Order>,
    pub lsb_bit: Option<u16>,
    pub longueur: Option<u16>,
    pub signe: Option<Sign>,
    pub facteur: Option<f64>,
    pub offset: Option<f64>,
    pub code_invalide: Option<InvalidCode>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatusEntry {
    pub identifiant: Option<u32>,
    #[serde(default)]
    pub slots: Vec<u8>,
    pub ordre: Option<Order>,
    pub lsb_bit: Option<u16>,
    pub longueur: Option<u16>,
    pub code_ok: Option<i64>,
}

impl SignalEntry {
    pub fn champ(&self) -> BitField {
        BitField { ordre: self.ordre, lsb_bit: self.lsb_bit, longueur: self.longueur }
    }
}

impl StatusEntry {
    pub fn champ(&self) -> BitField {
        BitField { ordre: self.ordre, lsb_bit: self.lsb_bit, longueur: self.longueur }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Supervision {
    pub premier_timeout_ms: Option<u32>,
    pub timeout_ms: Option<u32>,
    pub peremption_ms: Option<u32>,
    pub pertes_consecutives_tolerees: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Decision {
    #[serde(rename = "REJETER")]
    Rejeter,
    #[serde(rename = "PUBLIER")]
    Publier,
    #[serde(rename = "BORNER")]
    Borner,
    #[serde(rename = "IGNORER_SES_DONNÉES")]
    IgnorerSesDonnees,
    #[serde(rename = "UTILISER_SES_DONNÉES")]
    UtiliserSesDonnees,
    #[serde(rename = "PUBLIER_LA_DERNIÈRE")]
    PublierLaDerniere,
}

/// The four design decisions DD-1…DD-4 and the values each one accepts (public schema).
pub const DECISIONS: [(&str, &[Decision]); 4] = [
    ("DD-1_valeur_egale_au_code_invalide", &[Decision::Rejeter, Decision::Publier]),
    ("DD-2_trame_a_protection_non_conforme", &[Decision::IgnorerSesDonnees, Decision::UtiliserSesDonnees]),
    ("DD-3_valeur_hors_plage_sous_statut_ok", &[Decision::Rejeter, Decision::Borner, Decision::Publier]),
    ("DD-4_donnee_plus_vieille_que_la_peremption", &[Decision::Rejeter, Decision::PublierLaDerniere]),
];

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentUse {
    /// Library entry (its SHA-256 or primary address), chosen from the mission library.
    pub document: Option<String>,
    /// Free text: never checked.
    #[serde(default)]
    pub pages: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NominalPart {
    #[serde(default)]
    pub documents_utilises: Vec<DocumentUse>,
    #[serde(default)]
    pub trames: Vec<FrameEntry>,
    #[serde(default)]
    pub signaux: Vec<SignalEntry>,
    #[serde(default)]
    pub statuts: Vec<StatusEntry>,
    #[serde(default)]
    pub supervision: Supervision,
    /// DD-1…DD-4 in order.
    #[serde(default)]
    pub decisions: [Option<Decision>; 4],
    /// Free text: never checked.
    #[serde(default)]
    pub justification: String,
}

// ------------------------------------------------------------------------------------------------- sealed truth

/// A real field of a mission frame in the group's variant, positions least significant first.
#[derive(Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FieldTruth {
    pub positions: Vec<u16>,
    pub signed: bool,
    pub factor: f64,
    pub offset: f64,
    /// Raw invalid (SNA) code, if the field has one.
    pub invalid: Option<i64>,
    /// When the field is a status: the raw value meaning « OK ».
    pub ok: Option<i64>,
    pub required: bool,
}

#[derive(Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FrameTruth {
    pub id: u32,
    pub dlc: u8,
    pub period_ms: u32,
    /// CRC and counter positions of the end-to-end protection, when the frame has one.
    pub protection: Option<(Vec<u16>, Vec<u16>)>,
    pub alive: Option<Vec<u16>>,
    pub required: bool,
    pub fields: Vec<FieldTruth>,
}

/// What the nominal part must say for this group (sealed; resolved on the group's variant by the instructor edition).
#[derive(Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NominalTruth {
    /// Every frame of the mission ECU (DATA and STATUS): the only identifiers a nominal part may describe.
    pub frames: Vec<FrameTruth>,
    pub supervision: [u32; 4],
    pub decisions: [Decision; 4],
}

impl std::fmt::Debug for NominalTruth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "NominalTruth {{ frames: {} }}", self.frames.len())
    }
}

// --------------------------------------------------------------------------------------------- completeness

/// Labels of the controlled fields still empty (M-04), by position in the lists. Never depends on the truth.
pub fn missing(part: &NominalPart) -> Vec<String> {
    let mut out = Vec::new();
    if part.documents_utilises.is_empty() {
        out.push("Documents utilisés : au moins un document".into());
    }
    for (i, d) in part.documents_utilises.iter().enumerate() {
        if d.document.is_none() {
            out.push(format!("Document {} : document", i + 1));
        }
    }
    if part.trames.is_empty() {
        out.push("Trames : au moins une trame".into());
    }
    for (i, t) in part.trames.iter().enumerate() {
        let n = i + 1;
        let mut need = |ok: bool, what: &str| {
            if !ok {
                out.push(format!("Trame {n} : {what}"));
            }
        };
        need(t.identifiant.is_some(), "identifiant");
        need(t.format.is_some(), "format");
        need(t.dlc.is_some(), "DLC");
        need(t.periode_ms.is_some(), "période");
        need(t.protection.presente.is_some(), "protection présente");
        if t.protection.presente == Some(true) {
            need(t.protection.crc.filled(), "CRC de protection");
            need(t.protection.compteur.filled(), "compteur de protection");
        }
        need(t.compteur_de_vie.empty() || t.compteur_de_vie.filled(), "compteur de vie incomplet");
    }
    if part.signaux.is_empty() {
        out.push("Signaux : au moins un signal".into());
    }
    for (i, s) in part.signaux.iter().enumerate() {
        let n = i + 1;
        let mut need = |ok: bool, what: &str| {
            if !ok {
                out.push(format!("Signal {n} : {what}"));
            }
        };
        need(s.identifiant.is_some(), "identifiant");
        need(!s.slots.is_empty(), "slots");
        need(s.champ().filled(), "position");
        need(s.signe.is_some(), "signe");
        need(s.facteur.is_some(), "facteur");
        need(s.offset.is_some(), "offset");
        need(s.code_invalide.is_some(), "code invalide");
    }
    if part.statuts.is_empty() {
        out.push("Statuts : au moins un statut".into());
    }
    for (i, s) in part.statuts.iter().enumerate() {
        let n = i + 1;
        let mut need = |ok: bool, what: &str| {
            if !ok {
                out.push(format!("Statut {n} : {what}"));
            }
        };
        need(s.identifiant.is_some(), "identifiant");
        need(!s.slots.is_empty(), "slots");
        need(s.champ().filled(), "position");
        need(s.code_ok.is_some(), "code OK");
    }
    let sv = &part.supervision;
    for (value, what) in [
        (sv.premier_timeout_ms, "premier timeout"),
        (sv.timeout_ms, "timeout"),
        (sv.peremption_ms, "péremption"),
        (sv.pertes_consecutives_tolerees, "pertes consécutives tolérées"),
    ] {
        if value.is_none() {
            out.push(format!("Supervision : {what}"));
        }
    }
    for (i, d) in part.decisions.iter().enumerate() {
        if d.is_none() {
            out.push(format!("Décision DD-{}", i + 1));
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------- conformity

fn field_ok(e: &SignalEntry, f: &FieldTruth) -> bool {
    let Some(factor) = e.facteur.filter(|x| x.is_finite()) else { return false };
    let Some(offset) = e.offset.filter(|x| x.is_finite()) else { return false };
    let sign = if f.signed { Sign::TwosComplement } else { Sign::Unsigned };
    let sign_ok = f.positions.len() <= 1 || e.signe == Some(sign);
    let factor_ok = f.factor != 0.0 && ((factor - f.factor) / f.factor).abs() <= 1e-6;
    let offset_ok = (offset - f.offset).abs() <= f.factor.abs() / 2.0;
    let code_ok = match (e.code_invalide, f.invalid) {
        (Some(InvalidCode::Code(c)), Some(t)) => c == t,
        (Some(InvalidCode::Aucun(_)), None) => true,
        _ => false,
    };
    sign_ok && factor_ok && offset_ok && code_ok
}

/// The nominal part conforms to the truth. `library` lists the mission library entries a document may cite.
/// Assumes nothing about completeness: an incomplete part does not conform.
pub fn conforms(part: &NominalPart, truth: &NominalTruth, library: &[String]) -> bool {
    if !missing(part).is_empty() {
        return false;
    }
    if !part.documents_utilises.iter().all(|d| d.document.as_ref().is_some_and(|x| library.contains(x))) {
        return false;
    }
    let frame = |id: Option<u32>| truth.frames.iter().find(|f| Some(f.id) == id);

    // frames: required present, nothing else than the mission ECU's frames, each once and right
    let mut seen = Vec::new();
    for t in &part.trames {
        let Some(f) = frame(t.identifiant) else { return false };
        if seen.contains(&f.id) {
            return false;
        }
        seen.push(f.id);
        let protection_ok = match (&f.protection, t.protection.presente) {
            (Some((crc, counter)), Some(true)) => {
                t.protection.crc.positions().as_ref() == Some(crc) && t.protection.compteur.positions().as_ref() == Some(counter)
            }
            (None, Some(false)) => t.protection.crc.empty() && t.protection.compteur.empty(),
            _ => false,
        };
        let alive_ok = match &f.alive {
            Some(p) => t.compteur_de_vie.positions().as_ref() == Some(p),
            None => t.compteur_de_vie.empty(),
        };
        let period_ok = t.periode_ms.is_some_and(|p| p.abs_diff(f.period_ms) <= 1);
        if t.format.as_deref() != Some(FORMAT_STANDARD) || t.dlc != Some(f.dlc) || !period_ok || !protection_ok || !alive_ok {
            return false;
        }
    }
    if truth.frames.iter().any(|f| f.required && !seen.contains(&f.id)) {
        return false;
    }

    // signals: every entry a real field of a mission frame, right; each required field exactly once
    let mut count: Vec<Vec<usize>> = truth.frames.iter().map(|f| vec![0; f.fields.len()]).collect();
    for s in &part.signaux {
        let Some(fi) = truth.frames.iter().position(|f| Some(f.id) == s.identifiant) else { return false };
        let Some(pos) = s.champ().positions() else { return false };
        let Some(k) = truth.frames[fi].fields.iter().position(|x| x.positions == pos) else { return false };
        if !field_ok(s, &truth.frames[fi].fields[k]) {
            return false;
        }
        count[fi][k] += 1;
    }
    // statuses: every entry a status field of a mission frame with its OK code; each required status exactly once
    let mut status_count: Vec<Vec<usize>> = truth.frames.iter().map(|f| vec![0; f.fields.len()]).collect();
    for s in &part.statuts {
        let Some(fi) = truth.frames.iter().position(|f| Some(f.id) == s.identifiant) else { return false };
        let Some(pos) = s.champ().positions() else { return false };
        let Some(k) = truth.frames[fi].fields.iter().position(|x| x.positions == pos && x.ok.is_some()) else { return false };
        if s.code_ok != truth.frames[fi].fields[k].ok {
            return false;
        }
        status_count[fi][k] += 1;
    }
    for (fi, f) in truth.frames.iter().enumerate() {
        for (k, field) in f.fields.iter().enumerate() {
            let (signal, status) = (count[fi][k], status_count[fi][k]);
            if signal > 1 || status > 1 {
                return false;
            }
            if field.required && (if field.ok.is_some() { status } else { signal }) != 1 {
                return false;
            }
        }
    }

    let sv = &part.supervision;
    let supervision = [sv.premier_timeout_ms, sv.timeout_ms, sv.peremption_ms, sv.pertes_consecutives_tolerees];
    supervision.iter().zip(truth.supervision).all(|(got, want)| *got == Some(want))
        && part.decisions.iter().zip(truth.decisions).all(|(got, want)| *got == Some(want))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(order: Order, lsb: u16, len: u16) -> BitField {
        BitField { ordre: Some(order), lsb_bit: Some(lsb), longueur: Some(len) }
    }

    #[test]
    fn positions_follow_the_public_convention() {
        assert_eq!(f(Order::Intel, 12, 4).positions(), Some(vec![12, 13, 14, 15]));
        // the codec's Motorola example: MSB at bit 23, 12 bits, least significant bit at 28
        assert_eq!(f(Order::Motorola, 28, 12).positions(), Some(vec![28, 29, 30, 31, 16, 17, 18, 19, 20, 21, 22, 23]));
        // a field inside one byte is the same in both orders
        assert_eq!(f(Order::Intel, 9, 3).positions(), f(Order::Motorola, 9, 3).positions());
        assert_eq!(f(Order::Intel, 62, 3).positions(), None, "past the 8th byte");
        assert_eq!(f(Order::Motorola, 4, 8).positions(), None, "before the first byte");
        assert_eq!(f(Order::Intel, 0, 0).positions(), None);
        assert_eq!(BitField { ordre: None, ..f(Order::Intel, 0, 1) }.positions(), None);
    }

    /// One protected DATA frame (required) with a value, its status and an invalid code; one STATUS frame (optional)
    /// with an alive counter.
    fn truth() -> NominalTruth {
        let field = |positions: Vec<u16>, ok: Option<i64>, required: bool, invalid: Option<i64>| FieldTruth {
            positions,
            signed: false,
            factor: if ok.is_some() { 1.0 } else { 0.1 },
            offset: if ok.is_some() { 0.0 } else { -5.0 },
            invalid,
            ok,
            required,
        };
        NominalTruth {
            frames: vec![
                FrameTruth {
                    id: 0x7A5,
                    dlc: 8,
                    period_ms: 20,
                    protection: Some(((0..8).collect(), (8..12).collect())),
                    alive: None,
                    required: true,
                    fields: vec![
                        field((16..28).collect(), None, true, Some(0xFFF)),
                        field((28..30).collect(), Some(0), true, None),
                        field((40..48).collect(), None, false, None),
                    ],
                },
                FrameTruth {
                    id: 0x4C3,
                    dlc: 2,
                    period_ms: 100,
                    protection: None,
                    alive: Some((0..8).collect()),
                    required: false,
                    fields: vec![field((0..8).collect(), None, false, None), field((8..10).collect(), Some(1), false, None)],
                },
            ],
            supervision: [240, 120, 80, 2],
            decisions: [Decision::Publier, Decision::UtiliserSesDonnees, Decision::Borner, Decision::PublierLaDerniere],
        }
    }

    fn library() -> Vec<String> {
        vec!["doc-a".into(), "doc-b".into()]
    }

    fn right() -> NominalPart {
        NominalPart {
            documents_utilises: vec![DocumentUse { document: Some("doc-b".into()), pages: "p. 12".into() }],
            trames: vec![FrameEntry {
                identifiant: Some(0x7A5),
                format: Some(FORMAT_STANDARD.into()),
                dlc: Some(8),
                periode_ms: Some(20),
                protection: Protection { presente: Some(true), crc: f(Order::Intel, 0, 8), compteur: f(Order::Intel, 8, 4) },
                compteur_de_vie: BitField::default(),
            }],
            signaux: vec![SignalEntry {
                nom: "course".into(),
                identifiant: Some(0x7A5),
                slots: vec![0, 1],
                ordre: Some(Order::Intel),
                lsb_bit: Some(16),
                longueur: Some(12),
                signe: Some(Sign::Unsigned),
                facteur: Some(0.1),
                offset: Some(-5.0),
                code_invalide: Some(InvalidCode::Code(0xFFF)),
            }],
            statuts: vec![StatusEntry { identifiant: Some(0x7A5), slots: vec![1], ordre: Some(Order::Intel), lsb_bit: Some(28), longueur: Some(2), code_ok: Some(0) }],
            supervision: Supervision { premier_timeout_ms: Some(240), timeout_ms: Some(120), peremption_ms: Some(80), pertes_consecutives_tolerees: Some(2) },
            decisions: [Some(Decision::Publier), Some(Decision::UtiliserSesDonnees), Some(Decision::Borner), Some(Decision::PublierLaDerniere)],
            justification: "texte libre".into(),
        }
    }

    #[test]
    fn the_right_part_conforms_and_free_text_never_matters() {
        let (t, lib) = (truth(), library());
        assert!(missing(&right()).is_empty(), "{:?}", missing(&right()));
        assert!(conforms(&right(), &t, &lib));
        let mut p = right();
        p.justification = "n'importe quoi".repeat(100);
        p.signaux[0].nom = "autre nom".into();
        p.documents_utilises[0].pages = String::new();
        assert!(conforms(&p, &t, &lib));
        // a signal inside one byte may be declared in either order
        let mut p = right();
        p.statuts[0].ordre = Some(Order::Motorola);
        assert!(conforms(&p, &t, &lib));
    }

    #[test]
    fn every_single_controlled_mistake_fails() {
        let (t, lib) = (truth(), library());
        let mut cases: Vec<(&str, NominalPart)> = Vec::new();
        let mut add = |why, change: &dyn Fn(&mut NominalPart)| {
            let mut p = right();
            change(&mut p);
            cases.push((why, p));
        };
        add("document outside the library", &|p| p.documents_utilises[0].document = Some("autre".into()));
        add("unknown frame", &|p| p.trames[0].identifiant = Some(0x111));
        add("format", &|p| p.trames[0].format = Some("ÉTENDU".into()));
        add("dlc", &|p| p.trames[0].dlc = Some(7));
        add("period beyond 1 ms", &|p| p.trames[0].periode_ms = Some(22));
        add("protection absent", &|p| { p.trames[0].protection = Protection { presente: Some(false), ..Protection::default() }; });
        add("crc bits", &|p| p.trames[0].protection.crc = f(Order::Intel, 1, 8));
        add("counter bits", &|p| p.trames[0].protection.compteur = f(Order::Intel, 8, 3));
        add("alive counter on a frame without", &|p| p.trames[0].compteur_de_vie = f(Order::Intel, 56, 8));
        add("frame twice", &|p| { let t0 = p.trames[0].clone(); p.trames.push(t0); });
        add("required frame missing", &|p| {
            p.trames[0] = FrameEntry { identifiant: Some(0x4C3), format: Some(FORMAT_STANDARD.into()), dlc: Some(2), periode_ms: Some(100),
                protection: Protection { presente: Some(false), ..Protection::default() }, compteur_de_vie: f(Order::Intel, 0, 8) };
        });
        add("signal on another identifier", &|p| p.signaux[0].identifiant = Some(0x123));
        add("signal not a real field", &|p| p.signaux[0].longueur = Some(11));
        add("sign", &|p| p.signaux[0].signe = Some(Sign::TwosComplement));
        add("factor", &|p| p.signaux[0].facteur = Some(0.1000002));
        add("offset beyond half a step", &|p| p.signaux[0].offset = Some(-5.051));
        add("invalid code", &|p| p.signaux[0].code_invalide = Some(InvalidCode::Code(0xFFE)));
        add("no invalid code declared", &|p| p.signaux[0].code_invalide = Some(InvalidCode::Aucun(NoCode::Aucun)));
        add("required signal twice", &|p| { let s = p.signaux[0].clone(); p.signaux.push(s); });
        add("required signal missing", &|p| {
            p.signaux[0] = SignalEntry { lsb_bit: Some(40), longueur: Some(8), facteur: Some(0.1), offset: Some(-5.0),
                code_invalide: Some(InvalidCode::Aucun(NoCode::Aucun)), ..p.signaux[0].clone() };
        });
        add("status OK code", &|p| p.statuts[0].code_ok = Some(1));
        add("status not a status field", &|p| { p.statuts[0].lsb_bit = Some(16); p.statuts[0].longueur = Some(12); });
        add("supervision", &|p| p.supervision.peremption_ms = Some(81));
        add("decision", &|p| p.decisions[2] = Some(Decision::Rejeter));
        for (why, p) in cases {
            assert!(!conforms(&p, &t, &lib), "{why}");
        }
        // optional real fields and frames may be described, but then they must be right
        let mut p = right();
        p.signaux.push(SignalEntry { lsb_bit: Some(40), longueur: Some(8), facteur: Some(0.1), offset: Some(-5.0),
            code_invalide: Some(InvalidCode::Aucun(NoCode::Aucun)), ..p.signaux[0].clone() });
        p.trames.push(FrameEntry { identifiant: Some(0x4C3), format: Some(FORMAT_STANDARD.into()), dlc: Some(2), periode_ms: Some(99),
            protection: Protection { presente: Some(false), ..Protection::default() }, compteur_de_vie: f(Order::Intel, 0, 8) });
        assert!(conforms(&p, &t, &lib));
        p.trames[1].compteur_de_vie = BitField::default();
        assert!(!conforms(&p, &t, &lib));
    }

    #[test]
    fn a_status_counts_only_as_a_status() {
        let (t, lib) = (truth(), library());
        // the required status described as a raw signal while an optional status fills the status list: its OK code
        // was never declared
        let mut p = right();
        p.statuts = vec![StatusEntry { identifiant: Some(0x4C3), slots: vec![2], ordre: Some(Order::Intel), lsb_bit: Some(8), longueur: Some(2), code_ok: Some(1) }];
        p.signaux.push(SignalEntry { lsb_bit: Some(28), longueur: Some(2), facteur: Some(1.0), offset: Some(0.0),
            code_invalide: Some(InvalidCode::Aucun(NoCode::Aucun)), ..p.signaux[0].clone() });
        assert!(!conforms(&p, &t, &lib));
        // described once in each list, both right: conforms
        let mut p = right();
        p.signaux.push(SignalEntry { lsb_bit: Some(28), longueur: Some(2), facteur: Some(1.0), offset: Some(0.0),
            code_invalide: Some(InvalidCode::Aucun(NoCode::Aucun)), ..p.signaux[0].clone() });
        assert!(conforms(&p, &t, &lib));
    }

    #[test]
    fn completeness_names_positions_only() {
        let mut p = NominalPart::default();
        let m = missing(&p);
        assert!(m.contains(&"Trames : au moins une trame".to_owned()));
        assert!(m.contains(&"Décision DD-4".to_owned()));
        p.trames.push(FrameEntry::default());
        p.signaux.push(SignalEntry::default());
        let m = missing(&p);
        assert!(m.contains(&"Trame 1 : identifiant".to_owned()));
        assert!(m.contains(&"Signal 1 : code invalide".to_owned()));
        let mut half = right();
        half.trames[0].compteur_de_vie.lsb_bit = Some(0);
        assert_eq!(missing(&half), vec!["Trame 1 : compteur de vie incomplet".to_owned()]);
        assert!(!conforms(&half, &truth(), &library()));
    }

    #[test]
    fn the_part_round_trips_in_the_public_schema() {
        let p = right();
        let v = serde_json::to_value(&p).unwrap();
        assert_eq!(v["signaux"][0]["signe"], "NON_SIGNÉ");
        assert_eq!(v["signaux"][0]["ordre"], "INTEL");
        assert_eq!(v["decisions"][1], "UTILISER_SES_DONNÉES");
        assert_eq!(serde_json::from_value::<NominalPart>(v.clone()).unwrap(), p);
        let mut none = v.clone();
        none["signaux"][0]["code_invalide"] = serde_json::json!("AUCUN");
        let back: NominalPart = serde_json::from_value(none).unwrap();
        assert_eq!(back.signaux[0].code_invalide, Some(InvalidCode::Aucun(NoCode::Aucun)));
        let mut extra = v;
        extra["trames"][0]["nom"] = serde_json::json!("x");
        assert!(serde_json::from_value::<NominalPart>(extra).is_err());
    }

    #[test]
    fn truth_debug_is_redacted() {
        assert_eq!(format!("{:?}", truth()), "NominalTruth { frames: 2 }");
    }
}
