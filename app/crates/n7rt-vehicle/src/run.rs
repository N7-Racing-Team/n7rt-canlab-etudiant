//! Run files (docs/distribution.md §3.2). Their judgement against the full pack of the group's variant belongs to the
//! private oracle (`judge_run`).
//!
//! File = canonical JSON (UTF-8): no whitespace, object keys sorted by byte order, strings escaped as serde_json
//! does, integers in decimal, no JSON float anywhere (a published slot value is the shortest decimal string that
//! reads back to the same f64, "NaN", "inf" or "-inf"). `integrity` = SHA-256 (lowercase hex) of the canonical JSON
//! of the document without that key. Reading accepts any JSON layout and recomputes the canonical form.
//!
//! A run file comes from a student and its fields are shown to the instructor as they are: reading refuses a file
//! over [`MAX_BYTES`] before decoding it, and refuses (« fichier d'essai illisible ») text fields outside their form:
//! `pack_id` 32 lowercase hexadecimal characters; `mission`, `cohort`, `group` 1 to 64 letters, digits, `-`, `_`;
//! `started_utc` a UTC stamp (`20261006T142233Z` as the application writes it, or `2026-10-06T14:22:33Z`); the other
//! strings printable ASCII, 64 characters at most.

use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use crate::{RunFrame, RunRecord, RunSlot, RunTick};

pub(crate) const RUN_FORMAT: &str = "n7rt-run";
pub(crate) const RUN_VERSION: u64 = 1;
/// Largest run file read (bytes). A nominal APPS run is a few hundred KiB.
pub(crate) const MAX_BYTES: usize = 8 << 20;

const ERR_UNREADABLE: &str = "fichier d'essai illisible";
const ERR_TAMPERED: &str = "empreinte invalide : fichier altéré";
const ERR_VERSION: &str = "version non prise en charge";

const KEYS: [&str; 18] = [
    "format",
    "version",
    "mission",
    "cohort",
    "group",
    "pack_id",
    "app_version",
    "board_uid",
    "runtime_build",
    "student_crc32",
    "seed_vehicle",
    "seed_scenario",
    "duration_us",
    "tick_us",
    "started_utc",
    "frames",
    "ticks",
    "integrity",
];

/// Appends the canonical JSON of `v` to `out`.
pub(crate) fn canonical(v: &Value, out: &mut String) {
    match v {
        Value::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            out.push('{');
            for (i, k) in keys.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&Value::String(k.clone()).to_string());
                out.push(':');
                canonical(&m[k], out);
            }
            out.push('}');
        }
        Value::Array(a) => {
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                canonical(x, out);
            }
            out.push(']');
        }
        other => out.push_str(&other.to_string()),
    }
}

fn sha256_hex(text: &str) -> String {
    Sha256::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

fn hex(data: &[u8]) -> String {
    data.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 || !s.is_ascii() {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok()).collect()
}

fn slot_json(s: &Option<RunSlot>) -> Value {
    match s {
        None => Value::Null,
        Some(RunSlot::Reject) => json!("reject"),
        Some(RunSlot::V(x)) => json!({ "v": format!("{x:?}") }),
    }
}

fn document(r: &RunRecord) -> Value {
    let frames: Vec<Value> = r
        .frames
        .iter()
        .map(|f| json!({ "t_us": f.t_us, "id": f.id, "flags": f.flags, "data": hex(&f.data) }))
        .collect();
    let ticks: Vec<Value> = r
        .ticks
        .iter()
        .map(|t| json!({ "t_us": t.t_us, "slots": t.slots.iter().map(slot_json).collect::<Vec<_>>() }))
        .collect();
    let mut m = Map::new();
    let mut put = |k: &str, v: Value| {
        m.insert(k.to_string(), v);
    };
    put("format", json!(RUN_FORMAT));
    put("version", json!(RUN_VERSION));
    put("mission", json!(r.mission));
    put("cohort", json!(r.cohort));
    put("group", json!(r.group));
    put("pack_id", json!(r.pack_id));
    put("app_version", json!(r.app_version));
    put("board_uid", json!(r.board_uid));
    put("runtime_build", json!(r.runtime_build));
    put("student_crc32", json!(r.student_crc32));
    put("seed_vehicle", json!(r.seed_vehicle));
    put("seed_scenario", json!(r.seed_scenario));
    put("duration_us", json!(r.duration_us));
    put("tick_us", json!(r.tick_us));
    put("started_utc", json!(r.started_utc));
    put("frames", Value::Array(frames));
    put("ticks", Value::Array(ticks));
    Value::Object(m)
}

pub(crate) fn to_bytes(r: &RunRecord) -> Vec<u8> {
    let mut doc = document(r);
    let mut text = String::new();
    canonical(&doc, &mut text);
    let integrity = sha256_hex(&text);
    doc["integrity"] = json!(integrity);
    let mut out = String::with_capacity(text.len() + 96);
    canonical(&doc, &mut out);
    out.into_bytes()
}

fn bad(field: &str) -> String {
    format!("{ERR_UNREADABLE} (champ `{field}`)")
}

fn get_str(m: &Map<String, Value>, k: &str) -> Result<String, String> {
    m.get(k).and_then(Value::as_str).map(str::to_string).ok_or_else(|| bad(k))
}

/// A text field of the file, kept only when `form` accepts it.
fn get_text(m: &Map<String, Value>, k: &str, form: fn(&str) -> bool) -> Result<String, String> {
    get_str(m, k).and_then(|s| if form(&s) { Ok(s) } else { Err(bad(k)) })
}

/// `mission`, `cohort`, `group`: 1 to 64 letters, digits, `-`, `_` (what the pack header and the tools accept).
fn is_name(s: &str) -> bool {
    (1..=64).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// `pack_id`: 32 lowercase hexadecimal characters, as in the pack header.
fn is_pack_id(s: &str) -> bool {
    s.len() == 32 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// `started_utc`: `20261006T142233Z` (the application's stamp) or `2026-10-06T14:22:33Z`.
fn is_stamp(s: &str) -> bool {
    let form: &[u8] = match s.len() {
        16 => b"ddddddddTddddddZ",
        20 => b"dddd-dd-ddTdd:dd:ddZ",
        _ => return false,
    };
    s.bytes().zip(form).all(|(c, &f)| if f == b'd' { c.is_ascii_digit() } else { c == f })
}

/// The other strings: printable ASCII (no control character), 64 characters at most.
fn is_text(s: &str) -> bool {
    s.len() <= 64 && s.bytes().all(|b| (0x20..=0x7e).contains(&b))
}

fn get_u64(v: &Value, k: &str) -> Result<u64, String> {
    v.get(k).and_then(Value::as_u64).ok_or_else(|| bad(k))
}

fn map_u64(m: &Map<String, Value>, k: &str) -> Result<u64, String> {
    m.get(k).and_then(Value::as_u64).ok_or_else(|| bad(k))
}

fn exact_keys(v: &Value, keys: &[&str], field: &str) -> Result<(), String> {
    match v.as_object() {
        Some(o) if o.len() == keys.len() && keys.iter().all(|k| o.contains_key(*k)) => Ok(()),
        _ => Err(bad(field)),
    }
}

fn read_slot(v: &Value) -> Result<Option<RunSlot>, String> {
    match v {
        Value::Null => Ok(None),
        Value::String(s) if s == "reject" => Ok(Some(RunSlot::Reject)),
        Value::Object(o) if o.len() == 1 => {
            let x = o.get("v").and_then(Value::as_str).and_then(|s| s.parse::<f64>().ok()).ok_or_else(|| bad("slots"))?;
            Ok(Some(RunSlot::V(x)))
        }
        _ => Err(bad("slots")),
    }
}

pub(crate) fn from_bytes(bytes: &[u8]) -> Result<RunRecord, String> {
    if bytes.len() > MAX_BYTES {
        return Err(format!("{ERR_UNREADABLE} (plus de {} Mio)", MAX_BYTES >> 20));
    }
    let v: Value = serde_json::from_slice(bytes).map_err(|_| ERR_UNREADABLE.to_string())?;
    let Value::Object(mut m) = v else { return Err(ERR_UNREADABLE.to_string()) };
    if m.get("format").and_then(Value::as_str) != Some(RUN_FORMAT) {
        return Err(bad("format"));
    }
    match m.get("version").map(Value::as_u64) {
        Some(Some(RUN_VERSION)) => {}
        Some(Some(_)) => return Err(ERR_VERSION.to_string()),
        _ => return Err(bad("version")),
    }
    let Some(Value::String(integrity)) = m.remove("integrity") else { return Err(ERR_TAMPERED.to_string()) };
    let doc = Value::Object(m);
    let mut text = String::new();
    canonical(&doc, &mut text);
    if sha256_hex(&text) != integrity {
        return Err(ERR_TAMPERED.to_string());
    }
    let Value::Object(m) = doc else { unreachable!() };
    if let Some(k) = m.keys().find(|k| !KEYS.contains(&k.as_str())) {
        // The key is named only when it is a plain name: nothing from the file reaches a console unchecked.
        return Err(if is_name(k) { bad(k) } else { format!("{ERR_UNREADABLE} (champ inconnu)") });
    }
    let frames = m
        .get("frames")
        .and_then(Value::as_array)
        .ok_or_else(|| bad("frames"))?
        .iter()
        .map(|f| {
            exact_keys(f, &["t_us", "id", "flags", "data"], "frames")?;
            Ok(RunFrame {
                t_us: get_u64(f, "t_us")?,
                id: u32::try_from(get_u64(f, "id")?).map_err(|_| bad("id"))?,
                flags: u8::try_from(get_u64(f, "flags")?).map_err(|_| bad("flags"))?,
                data: f["data"].as_str().and_then(unhex).ok_or_else(|| bad("data"))?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let ticks = m
        .get("ticks")
        .and_then(Value::as_array)
        .ok_or_else(|| bad("ticks"))?
        .iter()
        .map(|t| {
            exact_keys(t, &["t_us", "slots"], "ticks")?;
            let slots = t["slots"].as_array().ok_or_else(|| bad("slots"))?.iter().map(read_slot).collect::<Result<_, _>>()?;
            Ok(RunTick { t_us: get_u64(t, "t_us")?, slots })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(RunRecord {
        mission: get_text(&m, "mission", is_name)?,
        cohort: get_text(&m, "cohort", is_name)?,
        group: get_text(&m, "group", is_name)?,
        pack_id: get_text(&m, "pack_id", is_pack_id)?,
        app_version: get_text(&m, "app_version", is_text)?,
        board_uid: get_text(&m, "board_uid", is_text)?,
        runtime_build: get_text(&m, "runtime_build", is_text)?,
        student_crc32: get_text(&m, "student_crc32", is_text)?,
        seed_vehicle: get_text(&m, "seed_vehicle", is_text)?,
        seed_scenario: get_text(&m, "seed_scenario", is_text)?,
        duration_us: map_u64(&m, "duration_us")?,
        tick_us: map_u64(&m, "tick_us")?,
        started_utc: get_text(&m, "started_utc", is_stamp)?,
        frames,
        ticks,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run() -> RunRecord {
        RunRecord {
            mission: "apps".into(),
            cohort: "promo-2026_A".into(),
            group: "G01".into(),
            pack_id: "00112233445566778899aabbccddeeff".into(),
            app_version: "0.1.0".into(),
            board_uid: "12345678-9ABCDEF0-0FEDCBA9".into(),
            runtime_build: "0.3.0 0123456789abcdef".into(),
            student_crc32: "0x12345678".into(),
            seed_vehicle: "18446744073709551615".into(),
            seed_scenario: "0".into(),
            duration_us: 20_000,
            tick_us: 10_000,
            started_utc: "20261006T142233Z".into(),
            frames: vec![RunFrame { t_us: 1_500, id: 0x10, flags: 0, data: vec![1, 2] }],
            ticks: vec![RunTick { t_us: 10_000, slots: vec![Some(RunSlot::V(2.5)), None, Some(RunSlot::Reject)] }],
        }
    }

    /// The file of `r` with `key` set to `value` and the integrity recomputed (what anyone can do).
    fn forged(r: &RunRecord, key: &str, value: Value) -> Vec<u8> {
        let mut doc = document(r);
        doc[key] = value;
        let mut text = String::new();
        canonical(&doc, &mut text);
        doc["integrity"] = json!(sha256_hex(&text));
        doc.to_string().into_bytes()
    }

    #[test]
    fn files_the_application_writes_read_back() {
        let r = run();
        assert_eq!(from_bytes(&to_bytes(&r)).unwrap(), r);
        let mut iso = run();
        iso.started_utc = "2026-10-06T14:22:33Z".into();
        iso.board_uid = String::new();
        assert_eq!(from_bytes(&to_bytes(&iso)).unwrap(), iso);
        let mut long = run();
        long.mission = "m".repeat(64);
        long.runtime_build = "~".repeat(64);
        assert_eq!(from_bytes(&to_bytes(&long)).unwrap(), long);
    }

    #[test]
    fn text_fields_outside_their_form_are_refused() {
        let r = run();
        assert!(from_bytes(&forged(&r, "group", json!("G02"))).is_ok(), "a forged but well-formed file is read");
        let spoof = "G01\r\nVERDICT : réussi";
        for (key, value) in [
            ("pack_id", "00112233445566778899AABBCCDDEEFF"),
            ("pack_id", "0011223344556677"),
            ("pack_id", "00112233445566778899aabbccddeefg"),
            ("mission", ""),
            ("mission", "apps/../x"),
            ("cohort", "promo 2026"),
            ("group", spoof),
            ("group", "é1"),
            ("group", &"G".repeat(65)),
            ("started_utc", ""),
            ("started_utc", "2026-10-06 14:22:33"),
            ("started_utc", "20261006T142233Z\u{1b}[2J"),
            ("started_utc", "2026100xT142233Z"),
            ("app_version", "0.1.0\n"),
            ("board_uid", "\u{1b}[31mUID"),
            ("runtime_build", &"r".repeat(65)),
            ("student_crc32", "0x1234\u{7f}"),
            ("seed_vehicle", "7\r"),
            ("seed_scenario", "é"),
        ] {
            let e = from_bytes(&forged(&r, key, json!(value))).unwrap_err();
            assert_eq!(e, format!("{ERR_UNREADABLE} (champ `{key}`)"), "{key} = {value:?}");
        }
        // A non-string value too, and an unknown key is named only when it is a plain name.
        assert_eq!(from_bytes(&forged(&r, "group", json!(7))).unwrap_err(), bad("group"));
        assert_eq!(from_bytes(&forged(&r, "extra", json!(1))).unwrap_err(), bad("extra"));
        let e = from_bytes(&forged(&r, "x\r\nVERDICT : réussi", json!(1))).unwrap_err();
        assert_eq!(e, format!("{ERR_UNREADABLE} (champ inconnu)"));
    }

    #[test]
    fn a_file_over_the_bound_is_refused_before_decoding() {
        // Zeroed memory: the pages are never touched.
        let mut big = vec![0u8; MAX_BYTES + 1];
        assert_eq!(from_bytes(&big).unwrap_err(), format!("{ERR_UNREADABLE} (plus de 8 Mio)"));
        // At the bound, the bytes are decoded (and refused for what they are).
        big.truncate(MAX_BYTES);
        assert_eq!(from_bytes(&big).unwrap_err(), ERR_UNREADABLE);
    }
}
