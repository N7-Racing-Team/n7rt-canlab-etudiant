//! Persistence of the student progression (private spec ch. 24 §9.1, adapted): only the Rust core writes these
//! files. Each document carries an `integrity` field over its canonical JSON without that field: HMAC-SHA256 with the
//! progression's key when one is given (the key derives from the decrypted pack content and never reaches the disk),
//! else SHA-256. A write goes to a temporary file of its own, is flushed, the current file is kept as `<name>.prev`,
//! then the temporary file replaces it in one rename, so that the current file is never missing nor torn once written.
//! A file that does not verify is refused, never repaired, and never replaced by its previous version: going back to
//! `.prev` would let anyone erase the last verdict (and its interval) by deleting or damaging the current file. The
//! previous version stays on disk for the instructor. The key raises the bar from editing a JSON file to decrypting
//! the group pack (extended D7 limit); restoring a whole earlier copy of the folder stays possible offline, and the
//! instructor re-judges exported evidence.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use hmac::{Hmac, Mac};
use serde_json::Value;
use sha2::{Digest, Sha256};

/// The key of a progression's integrity fields (none: plain SHA-256).
pub type Key = Option<[u8; 32]>;

pub const INTEGRITY: &str = "integrity";
/// Bound of one progression document (the fiche with its evidence frames stays far below).
pub const MAX_DOCUMENT_BYTES: u64 = 16 * 1024 * 1024;

fn digest(v: &Value, key: &Key) -> Result<String, String> {
    let mut body = v.clone();
    body.as_object_mut().ok_or_else(|| "document de progression invalide".to_owned())?.remove(INTEGRITY);
    let bytes = serde_json::to_vec(&body).map_err(|_| "document de progression invalide".to_owned())?;
    let sum: Vec<u8> = match key {
        Some(k) => {
            let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(k).map_err(|_| "clé de progression invalide".to_owned())?;
            mac.update(b"n7rt-progress-integrity\0");
            mac.update(&bytes);
            mac.finalize().into_bytes().to_vec()
        }
        None => Sha256::digest(&bytes).to_vec(),
    };
    Ok(sum.iter().map(|b| format!("{b:02x}")).collect())
}

/// A temporary file of its own beside `path` (two writers never share one).
fn temporary(path: &Path) -> PathBuf {
    static N: AtomicU64 = AtomicU64::new(0);
    sibling(path, &format!(".{}-{}.tmp", std::process::id(), N.fetch_add(1, Ordering::Relaxed)))
}

fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(suffix);
    path.with_file_name(name)
}

/// Writes `value` (an object) with its integrity digest, atomically.
pub fn save_json(path: &Path, value: &Value) -> Result<(), String> {
    save_json_keyed(path, value, &None)
}

/// [`save_json`] with the integrity keyed by `key`.
pub fn save_json_keyed(path: &Path, value: &Value, key: &Key) -> Result<(), String> {
    let fail = |what: &str| format!("enregistrement de la progression impossible ({what})");
    let mut doc = value.clone();
    let sum = digest(&doc, key)?;
    doc.as_object_mut().ok_or_else(|| fail("document"))?.insert(INTEGRITY.into(), Value::String(sum));
    let bytes = serde_json::to_vec_pretty(&doc).map_err(|_| fail("encodage"))?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|_| fail("dossier"))?;
    }
    let tmp = temporary(path);
    {
        let mut f = fs::File::create(&tmp).map_err(|_| fail("fichier temporaire"))?;
        f.write_all(&bytes).map_err(|_| fail("écriture"))?;
        f.sync_all().map_err(|_| fail("écriture"))?;
    }
    if path.is_file() {
        fs::copy(path, sibling(path, ".prev")).map_err(|_| fail("version précédente"))?;
    }
    fs::rename(&tmp, path).map_err(|_| fail("remplacement"))
}

/// Writes raw bytes atomically (`.tmp`, flush, rename): the sealed files of a progression, which their AEAD tag
/// authenticates.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let fail = |what: &str| format!("enregistrement de la progression impossible ({what})");
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|_| fail("dossier"))?;
    }
    let tmp = temporary(path);
    {
        let mut f = fs::File::create(&tmp).map_err(|_| fail("fichier temporaire"))?;
        f.write_all(bytes).map_err(|_| fail("écriture"))?;
        f.sync_all().map_err(|_| fail("écriture"))?;
    }
    fs::rename(&tmp, path).map_err(|_| fail("remplacement"))
}

fn read_verified(path: &Path, key: &Key) -> Result<Option<Value>, String> {
    let meta = match fs::metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("progression illisible".into()),
    };
    if meta.len() > MAX_DOCUMENT_BYTES {
        return Err("progression illisible (taille)".into());
    }
    let bytes = fs::read(path).map_err(|_| "progression illisible".to_owned())?;
    let doc: Value = serde_json::from_slice(&bytes).map_err(|_| "progression illisible".to_owned())?;
    let stated = doc.get(INTEGRITY).and_then(Value::as_str).ok_or_else(|| "progression illisible (intégrité)".to_owned())?;
    if digest(&doc, key)? != stated {
        return Err("progression altérée (intégrité)".into());
    }
    let mut body = doc;
    if let Some(o) = body.as_object_mut() {
        o.remove(INTEGRITY);
    }
    Ok(Some(body))
}

/// The document at `path` without its integrity field: `Ok(None)` when neither it nor its previous version exists; an
/// error when it does not verify, or when it is missing beside a previous version (deleted: a write never leaves it
/// missing).
pub fn load_json(path: &Path) -> Result<Option<Value>, String> {
    load_json_keyed(path, &None)
}

/// [`load_json`] with the integrity keyed by `key`.
pub fn load_json_keyed(path: &Path, key: &Key) -> Result<Option<Value>, String> {
    match read_verified(path, key)? {
        Some(v) => Ok(Some(v)),
        None if sibling(path, ".prev").exists() => Err("progression illisible (version courante absente)".into()),
        None => Ok(None),
    }
}

/// Removes a document and its temporary and previous versions (instructor reset).
pub fn remove_json(path: &Path) -> Result<(), String> {
    for p in [path.to_path_buf(), sibling(path, ".tmp"), sibling(path, ".prev")] {
        match fs::remove_file(&p) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("réinitialisation de la progression impossible".into()),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("n7rt-store-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn saved_documents_verify_and_reload() {
        let d = dir("reload");
        let p = d.join("progress.json");
        assert_eq!(load_json(&p).unwrap(), None);
        let v = serde_json::json!({"a": 1, "b": "é"});
        save_json(&p, &v).unwrap();
        assert_eq!(load_json(&p).unwrap(), Some(v.clone()));
        let on_disk: Value = serde_json::from_slice(&fs::read(&p).unwrap()).unwrap();
        assert_eq!(on_disk[INTEGRITY].as_str().unwrap().len(), 64);
        assert!(!sibling(&p, ".tmp").exists());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_tampered_document_is_refused_never_rolled_back() {
        let d = dir("tamper");
        let p = d.join("progress.json");
        save_json(&p, &serde_json::json!({"step": 0})).unwrap();
        save_json(&p, &serde_json::json!({"step": 1})).unwrap();
        let text = fs::read_to_string(&p).unwrap().replace("\"step\": 1", "\"step\": 4");
        fs::write(&p, text).unwrap();
        assert!(load_json(&p).is_err(), "refused, and the valid previous version is not used instead");
        assert!(sibling(&p, ".prev").exists(), "the previous version stays for the instructor");
        fs::write(&p, b"{not json").unwrap();
        assert!(load_json(&p).is_err());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_deleted_document_is_refused_when_its_previous_version_remains() {
        let d = dir("deleted");
        let p = d.join("progress.json");
        save_json(&p, &serde_json::json!({"step": 0})).unwrap();
        save_json(&p, &serde_json::json!({"step": 1})).unwrap();
        fs::remove_file(&p).unwrap();
        assert!(load_json(&p).is_err(), "deleting the current file never brings the previous one back");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_missing_integrity_field_is_refused() {
        let d = dir("nointegrity");
        let p = d.join("progress.json");
        fs::create_dir_all(&d).unwrap();
        fs::write(&p, br#"{"step": 3}"#).unwrap();
        assert!(load_json(&p).is_err());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn remove_clears_every_version() {
        let d = dir("remove");
        let p = d.join("fiche.json");
        save_json(&p, &serde_json::json!({"x": 1})).unwrap();
        save_json(&p, &serde_json::json!({"x": 2})).unwrap();
        fs::write(sibling(&p, ".tmp"), b"partial").unwrap();
        remove_json(&p).unwrap();
        assert!(!p.exists() && !sibling(&p, ".prev").exists() && !sibling(&p, ".tmp").exists());
        assert_eq!(load_json(&p).unwrap(), None);
        remove_json(&p).unwrap();
        let _ = fs::remove_dir_all(&d);
    }
}
