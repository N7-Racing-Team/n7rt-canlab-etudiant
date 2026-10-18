//! Canonical JSON of contract §7. Save writes fixed key orders through serialize structs; load reads a generic JSON
//! tree so that every problem becomes a violation instead of stopping at the first one.

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::{Map, Value as Json};

use crate::model::default_states;
use crate::persist::{FORMAT, VERSION};
use crate::{
    kind_def, Circuit, CompId, Component, Kind, PinRef, Pt, Rot, Value, ValueMap, ValueType, Violation, Wire, WireId,
    LIBRARY_VERSION, LIBRARY_VERSIONS_READ,
};

#[derive(Serialize)]
struct FileOut<'a> {
    format: &'static str,
    version: u32,
    library_version: u32,
    name: &'a str,
    next_id: u32,
    components: Vec<CompOut<'a>>,
    wires: Vec<&'a Wire>,
}

#[derive(Serialize)]
struct CompOut<'a> {
    id: CompId,
    kind: Kind,
    label: &'a str,
    pos: Pt,
    rot: Rot,
    params: &'a ValueMap,
    state: ValueMap,
}

pub(crate) fn save(c: &Circuit) -> String {
    let components = c
        .components
        .values()
        .map(|comp| {
            let stored = c.states.get(&comp.id);
            let state = kind_def(comp.kind)
                .states
                .iter()
                .filter(|s| s.persistent)
                .map(|s| {
                    let v = stored.and_then(|m| m.get(s.name)).cloned().unwrap_or_else(|| s.default.to_value());
                    (s.name.to_string(), v)
                })
                .collect();
            CompOut { id: comp.id, kind: comp.kind, label: &comp.label, pos: comp.pos, rot: comp.rot, params: &comp.params, state }
        })
        .collect();
    let out = FileOut {
        format: FORMAT,
        version: VERSION,
        library_version: LIBRARY_VERSION,
        name: &c.name,
        next_id: c.next_id,
        components,
        wires: c.wires.values().collect(),
    };
    let mut text = serde_json::to_string_pretty(&out).unwrap_or_default();
    text.push('\n');
    text
}

// ------------------------------------------------------------------------------------------------------- load

fn v(code: &str, message: String) -> Violation {
    Violation { code: code.to_string(), message_fr: message, items: Vec::new(), at: None }
}

fn json_invalid(message: String) -> Violation {
    v("json_invalid", message)
}

fn keys_exactly(obj: &Map<String, Json>, keys: &[&str], what: &str, out: &mut Vec<Violation>) {
    for k in obj.keys() {
        if !keys.contains(&k.as_str()) {
            out.push(json_invalid(format!("{what} : champ \u{ab}\u{a0}{k}\u{a0}\u{bb} inconnu")));
        }
    }
    for k in keys {
        if !obj.contains_key(*k) {
            out.push(json_invalid(format!("{what} : champ \u{ab}\u{a0}{k}\u{a0}\u{bb} absent")));
        }
    }
}

fn get_u32(obj: &Map<String, Json>, key: &str, what: &str, out: &mut Vec<Violation>) -> Option<u32> {
    let x = obj.get(key)?;
    match x.as_u64().and_then(|n| u32::try_from(n).ok()) {
        Some(n) => Some(n),
        None => {
            out.push(json_invalid(format!("{what} : {key} doit \u{ea}tre un entier positif sur 32 bits")));
            None
        }
    }
}

fn get_str<'a>(obj: &'a Map<String, Json>, key: &str, what: &str, out: &mut Vec<Violation>) -> Option<&'a str> {
    let x = obj.get(key)?;
    match x.as_str() {
        Some(s) => Some(s),
        None => {
            out.push(json_invalid(format!("{what} : {key} doit \u{ea}tre une cha\u{ee}ne")));
            None
        }
    }
}

fn as_pt(x: &Json) -> Option<Pt> {
    let a = x.as_array()?;
    if a.len() != 2 {
        return None;
    }
    let c = |j: &Json| j.as_i64().and_then(|n| i32::try_from(n).ok());
    Some(Pt { x: c(&a[0])?, y: c(&a[1])? })
}

/// JSON value of a parameter or state of type `vtype`, faithfully: type and range are judged by `check`. Integers
/// of Number parameters are converted exactly; "inf" is +∞ only for NumberOrInf.
fn to_value(x: &Json, vtype: ValueType) -> Option<Value> {
    match x {
        Json::Bool(b) => Some(Value::Bool(*b)),
        Json::Number(n) => match vtype {
            ValueType::Number | ValueType::NumberOrInf => n.as_f64().map(Value::Num),
            _ => n.as_i64().map(Value::Int).or_else(|| n.as_f64().map(Value::Num)),
        },
        Json::String(s) if s == "inf" && vtype == ValueType::NumberOrInf => Some(Value::Num(f64::INFINITY)),
        Json::String(s) => Some(Value::Choice(s.clone())),
        _ => None,
    }
}

fn parse_component(x: &Json, index: usize, out: &mut Vec<Violation>) -> Option<(Component, ValueMap)> {
    let what = format!("Composant n\u{b0}{}", index + 1);
    let Some(obj) = x.as_object() else {
        out.push(json_invalid(format!("{what} : objet attendu")));
        return None;
    };
    keys_exactly(obj, &["id", "kind", "label", "pos", "rot", "params", "state"], &what, out);
    let id = get_u32(obj, "id", &what, out);
    let kind = match obj.get("kind") {
        None => None,
        Some(k) => match serde_json::from_value::<Kind>(k.clone()) {
            Ok(kind) => Some(kind),
            Err(_) => {
                out.push(v("unknown_kind", format!("{what} : type {k} inconnu de la biblioth\u{e8}que")));
                None
            }
        },
    };
    let label = get_str(obj, "label", &what, out);
    let pos = obj.get("pos").and_then(|p| {
        let pt = as_pt(p);
        if pt.is_none() {
            out.push(json_invalid(format!("{what} : pos doit \u{ea}tre [x, y] en entiers")));
        }
        pt
    });
    let rot = obj.get("rot").and_then(|r| {
        let rot = r.as_u64().and_then(|n| u16::try_from(n).ok()).and_then(|n| Rot::try_from(n).ok());
        if rot.is_none() {
            out.push(json_invalid(format!("{what} : rotation {r} (attendu 0, 90, 180 ou 270)")));
        }
        rot
    });
    let (id, kind, label, pos, rot) = (id?, kind?, label?, pos?, rot?);
    let def = kind_def(kind);

    let mut params = ValueMap::new();
    match obj.get("params").map(Json::as_object) {
        Some(Some(map)) => {
            for (k, x) in map {
                let vtype = def.params.iter().find(|p| p.name == k).map_or(ValueType::Choice, |p| p.vtype);
                let numeric = matches!(vtype, ValueType::Number | ValueType::NumberOrInf);
                match to_value(x, vtype) {
                    // NaN, ±∞ and text where a JSON number is due ("inf" only for NumberOrInf): json_invalid (§7).
                    Some(Value::Choice(s)) if numeric => out.push(json_invalid(format!(
                        "{what} : {k} = \u{ab}\u{a0}{s}\u{a0}\u{bb}, nombre JSON attendu{}",
                        if vtype == ValueType::NumberOrInf { " ou \"inf\"" } else { "" }
                    ))),
                    Some(value) => {
                        params.insert(k.clone(), value);
                    }
                    None => out.push(v("param_type", format!("{what} : valeur de {k} illisible ({x})"))),
                }
            }
        }
        Some(None) => out.push(json_invalid(format!("{what} : params doit \u{ea}tre un objet"))),
        None => {}
    }

    // Transient states are never saved: they restart from their defaults.
    let mut states = default_states(kind);
    for s in def.states.iter().filter(|s| s.persistent) {
        states.remove(s.name);
    }
    match obj.get("state").map(Json::as_object) {
        Some(Some(map)) => {
            for (k, x) in map {
                match def.states.iter().find(|s| s.name == k) {
                    Some(sd) if !sd.persistent => out.push(v(
                        "state_unknown",
                        format!("{what} : l'\u{e9}tat {k} est transitoire et ne s'enregistre pas"),
                    )),
                    Some(sd) => match to_value(x, sd.vtype) {
                        Some(value) => {
                            states.insert(k.clone(), value);
                        }
                        None => out.push(v("state_out_of_range", format!("{what} : valeur de l'\u{e9}tat {k} illisible ({x})"))),
                    },
                    None => out.push(v("state_unknown", format!("{what} : \u{e9}tat {k} inconnu"))),
                }
            }
        }
        Some(None) => out.push(json_invalid(format!("{what} : state doit \u{ea}tre un objet"))),
        None => {}
    }
    Some((Component { id: CompId(id), kind, label: label.to_string(), pos, rot, params }, states))
}

fn parse_pin(x: Option<&Json>, what: &str, out: &mut Vec<Violation>) -> Option<PinRef> {
    let obj = x?.as_object();
    let pin = obj.and_then(|o| {
        let comp = o.get("comp")?.as_u64().and_then(|n| u32::try_from(n).ok())?;
        let pin = o.get("pin")?.as_str()?;
        (o.len() == 2).then(|| PinRef { comp: CompId(comp), pin: pin.to_string() })
    });
    if pin.is_none() {
        out.push(json_invalid(format!("{what} : extr\u{e9}mit\u{e9} attendue sous la forme {{\"comp\": id, \"pin\": \"p1\"}}")));
    }
    pin
}

fn parse_wire(x: &Json, index: usize, out: &mut Vec<Violation>) -> Option<Wire> {
    let what = format!("Fil n\u{b0}{}", index + 1);
    let Some(obj) = x.as_object() else {
        out.push(json_invalid(format!("{what} : objet attendu")));
        return None;
    };
    keys_exactly(obj, &["id", "a", "b", "bends"], &what, out);
    let id = get_u32(obj, "id", &what, out);
    let a = parse_pin(obj.get("a"), &what, out);
    let b = parse_pin(obj.get("b"), &what, out);
    let bends = obj.get("bends").and_then(|bs| {
        let pts: Option<Vec<Pt>> = bs.as_array().and_then(|a| a.iter().map(as_pt).collect());
        if pts.is_none() {
            out.push(json_invalid(format!("{what} : bends doit \u{ea}tre une liste de [x, y] entiers")));
        }
        pts
    });
    Some(Wire { id: WireId(id?), a: a?, b: b?, bends: bends? })
}

pub(crate) fn load(text: &str) -> Result<Circuit, Vec<Violation>> {
    if text.starts_with('\u{feff}') {
        return Err(vec![json_invalid("Fichier avec BOM : UTF-8 sans BOM attendu".to_string())]);
    }
    let root: Json = match serde_json::from_str(text) {
        Ok(root) => root,
        Err(e) => {
            let kind = match e.classify() {
                serde_json::error::Category::Eof => "fin de fichier inattendue",
                _ => "erreur de syntaxe",
            };
            return Err(vec![json_invalid(format!("JSON invalide : {kind} (ligne {}, colonne {})", e.line(), e.column()))]);
        }
    };
    let Some(obj) = root.as_object() else {
        return Err(vec![json_invalid("JSON invalide : objet attendu \u{e0} la racine".to_string())]);
    };

    // Identity of the file first: nothing else is read from an unknown format or version.
    let mut fatal = Vec::new();
    match obj.get("format").and_then(Json::as_str) {
        Some(f) if f == FORMAT => {}
        Some(f) => fatal.push(v("format_unknown", format!("Format \u{ab}\u{a0}{f}\u{a0}\u{bb} inconnu : {FORMAT} attendu"))),
        None => fatal.push(v("format_unknown", format!("Champ format absent : {FORMAT} attendu"))),
    }
    match obj.get("version").and_then(Json::as_u64) {
        Some(n) if n == u64::from(VERSION) => {}
        Some(n) => fatal.push(v("version_unsupported", format!("Version {n} non prise en charge (version {VERSION} attendue)"))),
        None => fatal.push(v("version_unsupported", format!("Champ version absent ou illisible (version {VERSION} attendue)"))),
    }
    let library = obj.get("library_version").and_then(Json::as_u64);
    match library {
        Some(n) if LIBRARY_VERSIONS_READ.iter().any(|v| u64::from(*v) == n) => {}
        Some(n) => fatal.push(v(
            "library_mismatch",
            format!("Biblioth\u{e8}que version {n} : ce logiciel utilise la version {LIBRARY_VERSION}, sans migration"),
        )),
        None => fatal.push(v("library_mismatch", format!("Champ library_version absent (version {LIBRARY_VERSION} attendue)"))),
    }
    if !fatal.is_empty() {
        return Err(fatal);
    }

    let mut out = Vec::new();
    keys_exactly(obj, &["format", "version", "library_version", "name", "next_id", "components", "wires"], "Fichier", &mut out);
    let name = get_str(obj, "name", "Fichier", &mut out).unwrap_or_default().to_string();
    let next_id = get_u32(obj, "next_id", "Fichier", &mut out).unwrap_or(0);

    let mut components = BTreeMap::new();
    let mut states = BTreeMap::new();
    match obj.get("components").map(Json::as_array) {
        Some(Some(list)) => {
            for (i, x) in list.iter().enumerate() {
                if let Some((mut comp, st)) = parse_component(x, i, &mut out) {
                    // Version 2 predates output current limiting. Zero preserves its
                    // original unconstrained model; current files remain strict.
                    if library == Some(2) && comp.kind == Kind::Mlx90377 {
                        comp.params.entry("output_i_lim".into()).or_insert(Value::Num(0.0));
                    }
                    let since = comp.kind.since_library();
                    if library.is_some_and(|n| n < u64::from(since)) {
                        out.push(v(
                            "library_mismatch",
                            format!("Composant n\u{b0}{} : type absent de la biblioth\u{e8}que version {}", i + 1, library.unwrap_or(0)),
                        ));
                    }
                    if components.contains_key(&comp.id) {
                        out.push(v("duplicate_id", format!("Identifiant de composant {} en double", comp.id.0)));
                        continue;
                    }
                    states.insert(comp.id, st);
                    components.insert(comp.id, comp);
                }
            }
        }
        Some(None) => out.push(json_invalid("Fichier : components doit \u{ea}tre une liste".to_string())),
        None => {}
    }
    let mut wires = BTreeMap::new();
    match obj.get("wires").map(Json::as_array) {
        Some(Some(list)) => {
            for (i, x) in list.iter().enumerate() {
                if let Some(w) = parse_wire(x, i, &mut out) {
                    if wires.contains_key(&w.id) {
                        out.push(v("duplicate_id", format!("Identifiant de fil {} en double", w.id.0)));
                        continue;
                    }
                    wires.insert(w.id, w);
                }
            }
        }
        Some(None) => out.push(json_invalid("Fichier : wires doit \u{ea}tre une liste".to_string())),
        None => {}
    }

    let circuit = Circuit { name, next_id, components, wires, states };
    out.extend(circuit.check());
    if out.is_empty() {
        Ok(circuit)
    } else {
        Err(out)
    }
}
