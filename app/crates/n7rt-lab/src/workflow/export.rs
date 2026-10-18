//! Final fiche of a closed mission (private spec ch. 24 §3.7, adapted without server): the student's dossier in the
//! Documents folder, written once at closure (and again on request):
//!
//! - `fiche.json`: the canonical fiche (nominal part, incident parts with their recorded conditions, the competences
//!   of the qualification, the chronology of the steps and every attempt), the reference the instructor re-judges;
//! - `fiche.html`: the same content to read or print, a static page (no script, every text escaped);
//! - `scellés/`: the sealed evidence of the measures, the sealed bench and a sealed copy of `fiche.json`, readable
//!   only with the group pack and its code (a `fiche.json` edited afterwards no longer matches the copy).
//!
//! Nothing of the truth, of the hidden tests or of the expected outputs is written: the student's own entries, the
//! verdicts the application showed, and dates.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::workflow::fiche::NominalPart;
use crate::workflow::incident::IncidentPart;
use crate::workflow::progress::{AttemptVerdict, Progress, StepState};
use crate::workflow::qualification::LABELS;

pub const EXPORT_FORMAT: &str = "n7rt-mission-fiche-finale";

/// `<root>/<mission>-<group>-<AAAAMMJJ-HHMM>/`: the folder of one export (local time stamp from `stamp`).
pub fn folder(root: &Path, mission: &str, group: &str, stamp: &str) -> PathBuf {
    let safe = |s: &str| s.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').take(40).collect::<String>();
    root.join(format!("{}-{}-{}", safe(mission), safe(group), safe(stamp)))
}

fn verdict(v: AttemptVerdict) -> &'static str {
    match v {
        AttemptVerdict::Pass => "PASS",
        AttemptVerdict::Fail => "FAIL",
        AttemptVerdict::Cancelled => "ANNULÉE",
    }
}

fn state(s: StepState) -> &'static str {
    match s {
        StepState::Todo => "À FAIRE",
        StepState::Open => "EN COURS",
        StepState::Passed => "VALIDÉ",
        StepState::Failed => "ÉCHEC À CORRIGER",
    }
}

/// The canonical final fiche. `dates` turns a Unix time into « JJ/MM/AAAA HH:MM » (local).
pub fn document(p: &Progress, nominal: &NominalPart, incidents: &[IncidentPart], names: &[&str], dates: &dyn Fn(u64) -> String) -> Value {
    let qualification = p
        .attempts
        .iter()
        .rev()
        .find(|a| a.step == crate::workflow::progress::QUALIFICATION && a.verdict == AttemptVerdict::Pass)
        .and_then(|a| a.competences.clone())
        .map(|cps| {
            cps.iter()
                .enumerate()
                .map(|(k, ok)| json!({"id": format!("CP-{}", k + 1), "libelle": LABELS.get(k).copied().unwrap_or(""), "resultat": if *ok { "PASS" } else { "FAIL" }}))
                .collect::<Vec<_>>()
        });
    json!({
        "format": EXPORT_FORMAT,
        "version": 1,
        "mission": p.owner.mission,
        "cohorte": p.owner.cohort,
        "groupe": p.owner.group,
        "pack_id": p.owner.pack_id,
        "ouverte_le": dates(p.opened_s),
        "temps_actif": format!("{} h {:02} min", p.active_s / 3600, p.active_s % 3600 / 60),
        "etapes": p.steps.iter().enumerate().map(|(k, s)| json!({
            "etape": names.get(k).copied().unwrap_or(""),
            "etat": state(s.state),
            "le": dates(s.since_s),
        })).collect::<Vec<_>>(),
        "nominal": nominal,
        "incidents": incidents.iter().map(|part| {
            let mut v = serde_json::to_value(part).unwrap_or(Value::Null);
            let at = part.mesure_cle.consignation.as_ref().map(|c| dates(c.consignee_s));
            if let Some(o) = v.as_object_mut() {
                if let Some(at) = at {
                    o.insert("consignee_le".into(), json!(at));
                }
                // the repair the application recorded (never the student's entry)
                let repair = p.repairs.iter().find(|r| r.rank == part.rang as usize);
                o.insert("reparation".into(), json!({
                    "crc32_activation": repair.map(|r| r.crc32_activation.clone()),
                    "crc32_validation": repair.and_then(|r| r.crc32_validation.clone()),
                    "circuit": repair.map(|r| r.circuit.clone()).unwrap_or_default(),
                }));
            }
            v
        }).collect::<Vec<_>>(),
        "qualification": {"competences": qualification},
        "reproductions": p.reproductions.iter().map(|r| json!({"incident": r.rank, "le": dates(r.at_s)})).collect::<Vec<_>>(),
        "tentatives": p.attempts.iter().map(|a| json!({
            "id": a.id,
            "etape": names.get(a.step).copied().unwrap_or(""),
            "demandee_le": dates(a.requested_s),
            "terminee_le": dates(a.ended_s),
            "verdict": verdict(a.verdict),
            // the qualification has neither a diagnostic nor a repair
            "diagnostic": a.diagnostic.filter(|_| a.step != crate::workflow::progress::QUALIFICATION).map(|d| if d { "CONFORME" } else { "NON_CONFORME" }),
            "reparation": a.verified.filter(|_| a.step != crate::workflow::progress::QUALIFICATION).map(|v| if v { "VÉRIFIÉE" } else { "NON_VÉRIFIÉE" }),
            "annulation": a.cancel,
            "sheet_seq": a.evidence.sheet_seq,
            "circuit_sha256": a.evidence.circuit_sha256,
            "crc32": a.evidence.crc32,
            "session": a.evidence.session,
            "carte": a.evidence.board_uid,
            "bus_sha256": a.evidence.bus_sha256,
        })).collect::<Vec<_>>(),
    })
}

/// HTML text: `&`, `<`, `>`, `"` and `'` escaped.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

fn cell(v: &Value) -> String {
    match v {
        Value::Null => "—".to_owned(),
        Value::String(s) => escape(s),
        Value::Bool(b) => (if *b { "oui" } else { "non" }).to_owned(),
        Value::Number(n) => escape(&n.to_string()),
        other => escape(&other.to_string()),
    }
}

/// Rows of a two-column table from an object (nested values as compact JSON).
fn kv(o: &Value) -> String {
    let mut out = String::from("<table>");
    if let Some(map) = o.as_object() {
        for (k, v) in map {
            out.push_str(&format!("<tr><th>{}</th><td>{}</td></tr>", escape(k), cell(v)));
        }
    }
    out.push_str("</table>");
    out
}

/// A table from an array of objects (columns of the first row).
fn table(rows: &Value) -> String {
    let Some(rows) = rows.as_array().filter(|r| !r.is_empty()) else { return "<p>—</p>".to_owned() };
    let cols: Vec<String> = rows[0].as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default();
    let mut out = String::from("<table><tr>");
    for c in &cols {
        out.push_str(&format!("<th>{}</th>", escape(c)));
    }
    out.push_str("</tr>");
    for r in rows {
        out.push_str("<tr>");
        for c in &cols {
            out.push_str(&format!("<td>{}</td>", cell(&r[c])));
        }
        out.push_str("</tr>");
    }
    out.push_str("</table>");
    out
}

/// The readable page of the final fiche (static, printable); `sha256` is the digest of `fiche.json`, printed at the
/// foot of the page.
pub fn html(doc: &Value, sha256: &str) -> String {
    let mut b = String::new();
    b.push_str("<!doctype html><html lang=\"fr\"><head><meta charset=\"utf-8\"><title>Fiche finale</title><style>");
    b.push_str("body{font-family:system-ui,sans-serif;margin:24px;color:#111}table{border-collapse:collapse;margin:8px 0}th,td{border:1px solid #999;padding:4px 8px;text-align:left;vertical-align:top}h2{margin-top:24px}");
    b.push_str("</style></head><body>");
    b.push_str(&format!(
        "<h1>Fiche finale — mission {} — groupe {}</h1><p>Cohorte {} · ouverte le {} · temps actif {}</p>",
        cell(&doc["mission"]),
        cell(&doc["groupe"]),
        cell(&doc["cohorte"]),
        cell(&doc["ouverte_le"]),
        cell(&doc["temps_actif"])
    ));
    b.push_str("<h2>Étapes</h2>");
    b.push_str(&table(&doc["etapes"]));
    let n = &doc["nominal"];
    b.push_str("<h2>Partie nominale</h2><h3>Documents utilisés</h3>");
    b.push_str(&table(&n["documents_utilises"]));
    b.push_str("<h3>Trames</h3>");
    b.push_str(&table(&n["trames"]));
    b.push_str("<h3>Signaux</h3>");
    b.push_str(&table(&n["signaux"]));
    b.push_str("<h3>Statuts</h3>");
    b.push_str(&table(&n["statuts"]));
    b.push_str("<h3>Supervision</h3>");
    b.push_str(&kv(&n["supervision"]));
    b.push_str(&format!("<h3>Décisions</h3><p>{}</p>", cell(&n["decisions"])));
    b.push_str(&format!("<h3>Justification</h3><p>{}</p>", cell(&n["justification"])));
    for inc in doc["incidents"].as_array().into_iter().flatten() {
        b.push_str(&format!("<h2>Incident {}</h2>", cell(&inc["rang"])));
        b.push_str("<h3>Cible</h3>");
        b.push_str(&kv(&inc["cible"]));
        b.push_str(&format!(
            "<table><tr><th>Catégorie</th><td>{}</td></tr><tr><th>État observé</th><td>{}</td></tr></table>",
            cell(&inc["categorie"]),
            cell(&inc["etat_observe"])
        ));
        let m = &inc["mesure_cle"];
        let c = &m["consignation"];
        b.push_str(&format!(
            "<h3>Mesure clé</h3><table><tr><th>Grandeur</th><td>{}</td></tr><tr><th>Valeur</th><td>{} {}</td></tr><tr><th>Consignée</th><td>{}</td></tr><tr><th>Multimètre</th><td>{}</td></tr></table>",
            cell(&c["grandeur"]),
            cell(&m["valeur"]),
            cell(&m["unite"]),
            cell(&inc["consignee_le"]),
            cell(&c["conditions"]["multimetre"]["lecture"])
        ));
        b.push_str(&format!("<h3>Justification</h3><p>{}</p>", cell(&inc["justification"])));
        let r = &inc["reparation"];
        b.push_str(&format!(
            "<h3>Réparation enregistrée</h3><table><tr><th>CRC-32 à l'activation</th><td>{}</td></tr><tr><th>CRC-32 à la validation</th><td>{}</td></tr></table>",
            cell(&r["crc32_activation"]),
            cell(&r["crc32_validation"])
        ));
        let lines: Vec<String> = r["circuit"].as_array().into_iter().flatten().map(|l| format!("<li>{}</li>", cell(l))).collect();
        b.push_str(&if lines.is_empty() { "<p>Circuit : aucune modification.</p>".to_owned() } else { format!("<ul>{}</ul>", lines.concat()) });
    }
    b.push_str("<h2>Qualification finale</h2>");
    b.push_str(&table(&doc["qualification"]["competences"]));
    b.push_str("<h2>Reproductions de l'anomalie</h2>");
    b.push_str(&table(&doc["reproductions"]));
    b.push_str("<h2>Tentatives</h2>");
    b.push_str(&table(&doc["tentatives"]));
    b.push_str(&format!("<p><small>Empreinte SHA-256 de fiche.json : {}</small></p>", escape(sha256)));
    b.push_str("</body></html>\n");
    b
}

/// Bound of an exported `fiche.json` read back by the instructor.
const MAX_EXPORT_BYTES: u64 = 8 << 20;

/// The instructor's check of an export folder (`n7rt-pack verify-export`), against `content`, the sealed content of the
/// group that owns its pack regenerated from the master key: the sealed copy opens with that group's progression key
/// and is `fiche.json` byte for byte; the owner is that group; the mission is closed, every step validated, the eight
/// competences passed; every attempt has its own nonce and every judged one its session and bus digest; the nominal
/// part and the three diagnoses conform to the group's own truth (its variant). Ok: the lines of the report; Err: why
/// the export is refused (another pack, an edited or replaced file, an unfinished progression, a foreign answer).
pub fn verify(folder: &Path, mission: &str, cohort: &str, group: &str, content: &str) -> Result<Vec<String>, String> {
    use crate::workflow::{fiche, incident, sealed};
    let read = |name: &str| -> Result<Vec<u8>, String> {
        let path = folder.join(name);
        let len = std::fs::metadata(&path).map_err(|_| format!("{name} absent"))?.len();
        if len > MAX_EXPORT_BYTES {
            return Err(format!("{name} trop volumineux"));
        }
        std::fs::read(&path).map_err(|_| format!("{name} illisible"))
    };
    let json = read("fiche.json")?;
    let copy = read("scellés/fiche-finale.sealed")?;
    let (pack_text, wf) = sealed::split(content)?;
    let wf = wf.ok_or("le pack du groupe n'a pas de progression 0.3")?;
    let pack = n7rt_vehicle::Pack::from_student_json(&pack_text)?;
    let workflow = sealed::parse_workflow(&wf, mission, pack.tick_period_ms())?;
    let opened = n7rt_seal::open_local(&workflow.key, b"n7rt-fiche-finale", &copy)
        .map_err(|_| "copie scellée illisible avec la clé du groupe : export d'un autre pack, ou scellé remplacé".to_owned())?;
    if opened != json {
        return Err("fiche.json ne correspond plus à sa copie scellée : fichier modifié après l'export".into());
    }
    let doc: Value = serde_json::from_slice(&json).map_err(|_| "fiche.json illisible".to_owned())?;
    let text = |k: &str| doc[k].as_str().unwrap_or("");
    if text("format") != EXPORT_FORMAT || doc["version"].as_u64() != Some(1) {
        return Err("format de fiche finale inconnu".into());
    }
    if text("mission") != mission || text("cohorte") != cohort || !text("groupe").eq_ignore_ascii_case(group) {
        return Err(format!("export annoncé pour {}/{}/{}, pack de {mission}/{cohort}/{group}", text("mission"), text("cohorte"), text("groupe")));
    }
    let steps = doc["etapes"].as_array().cloned().unwrap_or_default();
    if steps.len() != crate::workflow::progress::STEPS || steps.iter().any(|s| s["etat"] != state(StepState::Passed)) {
        return Err("mission non clôturée : une étape n'est pas validée".into());
    }
    let cps = doc["qualification"]["competences"].as_array().cloned().unwrap_or_default();
    if cps.len() != LABELS.len() || cps.iter().any(|c| c["resultat"] != "PASS") {
        return Err("qualification finale incomplète".into());
    }
    // attempts: one random nonce each; every judged attempt bound to its session and to what crossed the link
    let attempts = doc["tentatives"].as_array().cloned().unwrap_or_default();
    let hex = |v: &Value, n: usize| v.as_str().is_some_and(|s| s.len() == n && s.bytes().all(|b| b.is_ascii_hexdigit()));
    let mut ids: Vec<&str> = attempts.iter().filter_map(|a| a["id"].as_str()).collect();
    ids.sort_unstable();
    if ids.len() != attempts.len() || ids.windows(2).any(|w| w[0] == w[1]) || !attempts.iter().all(|a| hex(&a["id"], 32)) {
        return Err("tentatives : nonce absent ou rejoué".into());
    }
    for a in attempts.iter().filter(|a| a["verdict"] == "PASS" || a["verdict"] == "FAIL") {
        if !hex(&a["session"], 32) || !hex(&a["bus_sha256"], 64) || !hex(&a["circuit_sha256"], 64) {
            return Err(format!("tentative {} sans session, empreinte du banc ou empreinte du bus", a["id"].as_str().unwrap_or("?")));
        }
    }
    for (k, s) in steps.iter().enumerate().take(crate::workflow::progress::VERIFIED_STEPS) {
        let passed = attempts.iter().filter(|a| a["etape"] == s["etape"] && a["verdict"] == "PASS").collect::<Vec<_>>();
        if passed.is_empty() {
            return Err(format!("étape {} validée sans tentative réussie", s["etape"].as_str().unwrap_or("?")));
        }
        if k < crate::workflow::progress::QUALIFICATION && passed.iter().any(|a| a["diagnostic"] != "CONFORME" || a["reparation"] != "VÉRIFIÉE") {
            return Err(format!("étape {} : tentative réussie sans diagnostic conforme et réparation vérifiée", s["etape"].as_str().unwrap_or("?")));
        }
    }
    // re-judgement against the group's own truth (the documents cited are not re-checked: the library is the student's)
    let nominal: NominalPart = serde_json::from_value(doc["nominal"].clone()).map_err(|_| "partie nominale illisible".to_owned())?;
    let cited: Vec<String> = nominal.documents_utilises.iter().filter_map(|d| d.document.clone()).collect();
    let truth = workflow.steps.first().and_then(|s| s.nominal.as_ref()).ok_or("vérité nominale absente du pack")?;
    if !fiche::conforms(&nominal, truth, &cited) {
        return Err("partie nominale non conforme à la vérité du groupe".into());
    }
    let parts = doc["incidents"].as_array().cloned().unwrap_or_default();
    for k in 1..=3usize {
        let content = workflow.steps.get(k).and_then(|s| s.incident.as_ref()).ok_or("incident absent du pack")?;
        let mut v = parts.iter().find(|p| p["rang"].as_u64() == Some(k as u64)).cloned().ok_or_else(|| format!("incident {k} absent de la fiche"))?;
        let repair = v["reparation"].clone();
        if let Some(o) = v.as_object_mut() {
            o.remove("consignee_le");
            o.remove("reparation");
        }
        let part: IncidentPart = serde_json::from_value(v).map_err(|_| format!("incident {k} illisible"))?;
        if !incident::fields_conform(&part, &content.truth) || incident::student_value(&part, &content.measure).is_none() {
            return Err(format!("incident {k} : diagnostic non conforme à la variante du groupe"));
        }
        if part.mesure_cle.consignation.is_none() || !repair["crc32_validation"].is_string() {
            return Err(format!("incident {k} : mesure consignée ou réparation enregistrée absente"));
        }
    }
    Ok(vec![
        format!("Export conforme : mission {mission}, cohorte {cohort}, groupe {group}."),
        "Copie scellée : identique à fiche.json (clé de progression du groupe).".into(),
        format!("Étapes : {}/{} validées ; qualification : {}/{} compétences.", steps.len(), steps.len(), cps.len(), cps.len()),
        format!("Tentatives : {} (nonces distincts, sessions et empreintes du bus présentes).", attempts.len()),
        "Fiche nominale et trois diagnostics : conformes à la vérité de la variante du groupe.".into(),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_text_is_escaped() {
        assert_eq!(escape("<script>alert('x') & \"y\"</script>"), "&lt;script&gt;alert(&#39;x&#39;) &amp; &quot;y&quot;&lt;/script&gt;");
        let doc = json!({"mission": "<b>", "groupe": "G01", "cohorte": "c", "ouverte_le": "x", "etapes": [], "nominal": {"justification": "<img src=x onerror=1>"},
                         "incidents": [{"rang": 1, "cible": {}, "mesure_cle": {"consignation": null}, "justification": "</p><script>"}], "qualification": {"competences": null}, "tentatives": []});
        let page = html(&doc, "00");
        assert!(!page.contains("<script>") && !page.contains("<img") && !page.contains("<b>"), "{page}");
        assert!(page.contains("&lt;img src=x onerror=1&gt;"));
    }

    #[test]
    fn folders_are_safe_names() {
        let f = folder(Path::new("/d"), "apps", "G01/../x", "20261013-1042");
        assert_eq!(f, Path::new("/d").join("apps-G01x-20261013-1042"));
    }
}
