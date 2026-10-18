//! Storage of the Physical Lab (contract §7 and §8.1): user circuits in `<data>/circuits/<name>.json`, and the
//! journal spilled to `<data>/sessions/<start>/journal.ndjson` beyond 10 000 entries, next to the initial circuit
//! of the session (`initial.json`). This file is a spilled prefix; a full replay also needs the live RAM tail.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use n7rt_circuit::{persist, Circuit};

use super::{FileEntry, FileError};

pub(crate) const CIRCUITS: &str = "circuits";
pub(crate) const SESSIONS: &str = "sessions";

/// Device names Windows refuses as file names, whatever the extension.
const RESERVED: [&str; 24] = [
    "CON", "PRN", "AUX", "NUL", "COM0", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT0", "LPT1",
    "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// `[A-Za-z0-9][A-Za-z0-9_-]{0,63}`, reserved Windows names refused (any case).
pub(crate) fn valid_name(name: &str) -> bool {
    let b = name.as_bytes();
    !b.is_empty()
        && b.len() <= 64
        && b[0].is_ascii_alphanumeric()
        && b.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'_' || *c == b'-')
        && !RESERVED.iter().any(|r| r.eq_ignore_ascii_case(name))
}

fn io_error(e: io::Error) -> FileError {
    FileError::Io { message: e.to_string() }
}

/// The circuit file named `name`, case-insensitively (an exact match first): (stem as stored, path).
fn find(dir: &Path, name: &str) -> Result<Option<(String, PathBuf)>, FileError> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(io_error(e)),
    };
    let mut found: Vec<(String, PathBuf)> = entries
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            let stem = path.file_name()?.to_str()?.strip_suffix(".json")?.to_string();
            stem.eq_ignore_ascii_case(name).then_some((stem, path))
        })
        .collect();
    found.sort();
    let exact = found.iter().position(|(s, _)| s == name);
    Ok(match exact {
        Some(i) => Some(found.swap_remove(i)),
        None => found.into_iter().next(),
    })
}

/// Written next to the target then renamed over it: a failed write never leaves a truncated circuit.
fn write_atomic(path: &Path, text: &str) -> io::Result<()> {
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, text)?;
    fs::rename(&tmp, path).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })
}

/// Canonical text of `circuit` with `name` as its name. The name is replaced in the canonical text and the
/// result kept only if it loads back to a circuit that saves to the very same text; otherwise the circuit keeps
/// its own name.
pub(crate) fn renamed_text(circuit: &Circuit, name: &str) -> String {
    let text = persist::save(circuit);
    if circuit.name() == name {
        return text;
    }
    let (Ok(old), Ok(new)) = (serde_json::to_string(circuit.name()), serde_json::to_string(name)) else { return text };
    let needle = format!("\"name\": {old}");
    let Some(at) = text.find(&needle) else { return text };
    let candidate = format!("{}\"name\": {new}{}", &text[..at], &text[at + needle.len()..]);
    match persist::load(&candidate) {
        Ok(c) if c.name() == name && persist::save(&c) == candidate => candidate,
        _ => text,
    }
}

/// Writes `<data>/circuits/<name>.json`. A file of the same name in another case is replaced only with
/// `overwrite`, and then removed so that the stored name is the new one.
pub(crate) fn save(data: &Path, name: &str, overwrite: bool, text: &str) -> Result<(), FileError> {
    let dir = data.join(CIRCUITS);
    fs::create_dir_all(&dir).map_err(io_error)?;
    let existing = find(&dir, name)?;
    if existing.is_some() && !overwrite {
        return Err(FileError::NameExists);
    }
    if let Some((stem, path)) = &existing {
        if stem != name {
            fs::remove_file(path).map_err(io_error)?;
        }
    }
    write_atomic(&dir.join(format!("{name}.json")), text).map_err(io_error)
}

/// Reads a circuit file: (stem as stored, text).
pub(crate) fn read(data: &Path, name: &str) -> Result<(String, String), FileError> {
    let Some((stem, path)) = find(&data.join(CIRCUITS), name)? else { return Err(FileError::NotFound) };
    let text = fs::read_to_string(&path).map_err(|e| if e.kind() == io::ErrorKind::NotFound { FileError::NotFound } else { io_error(e) })?;
    Ok((stem, text))
}

/// Circuit files with a valid name, sorted by name (case-insensitive).
pub(crate) fn list(data: &Path) -> Result<Vec<FileEntry>, FileError> {
    let entries = match fs::read_dir(data.join(CIRCUITS)) {
        Ok(e) => e,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(io_error(e)),
    };
    let mut out: Vec<FileEntry> = entries
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            let stem = path.file_name()?.to_str()?.strip_suffix(".json")?.to_string();
            if !valid_name(&stem) || !e.file_type().ok()?.is_file() {
                return None;
            }
            let modified = e.metadata().ok()?.modified().ok()?;
            let ms = modified.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
            Some(FileEntry { name: stem, modified_unix_ms: ms })
        })
        .collect();
    out.sort_by(|a, b| (a.name.to_ascii_lowercase(), &a.name).cmp(&(b.name.to_ascii_lowercase(), &b.name)));
    Ok(out)
}

/// UTC "YYYYMMDD-HHMMSS-mmm", the name of a session directory.
pub(crate) fn timestamp(t: SystemTime) -> String {
    let ms = t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64);
    let (days, rem) = (ms.div_euclid(86_400_000), ms.rem_euclid(86_400_000));
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}{m:02}{d:02}-{:02}{:02}{:02}-{:03}", rem / 3_600_000, rem / 60_000 % 60, rem / 1000 % 60, rem % 1000)
}

/// Proleptic Gregorian date of a day count since 1970-01-01 (H. Hinnant's civil_from_days).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

/// A failed append may leave the archive unusable if even truncating back to its original length fails.
#[derive(Debug)]
pub(crate) struct JournalAppendError {
    pub error: io::Error,
    pub rollback_failed: bool,
}

impl From<io::Error> for JournalAppendError {
    fn from(error: io::Error) -> Self { Self { error, rollback_failed: false } }
}

trait JournalStorage: Write {
    fn journal_len(&self) -> io::Result<u64>;
    fn rollback(&mut self, len: u64) -> io::Result<()>;
    fn sync_journal(&mut self) -> io::Result<()>;
}

impl JournalStorage for File {
    fn journal_len(&self) -> io::Result<u64> { Ok(self.metadata()?.len()) }
    fn rollback(&mut self, len: u64) -> io::Result<()> {
        self.set_len(len)?;
        self.seek(SeekFrom::Start(len))?;
        self.sync_data()
    }
    fn sync_journal(&mut self) -> io::Result<()> { self.sync_data() }
}

fn append_bytes(sink: &mut impl JournalStorage, bytes: &[u8]) -> Result<(), JournalAppendError> {
    let before = sink.journal_len()?;
    let result = sink.write_all(bytes).and_then(|_| sink.flush()).and_then(|_| sink.sync_journal());
    if let Err(error) = result {
        let rollback_failed = sink.rollback(before).is_err();
        return Err(JournalAppendError { error, rollback_failed });
    }
    Ok(())
}

/// Appends a whole batch, rolling back any partial write/flush/sync failure. A rollback failure is sticky:
/// the caller must stop appending to this archive, rather than retry a batch on an unknown prefix.
pub(crate) fn append_journal(data: &Path, session: &str, initial: &str, lines: &[String]) -> Result<(), JournalAppendError> {
    let dir = data.join(SESSIONS).join(session);
    fs::create_dir_all(&dir)?;
    let init = dir.join("initial.json");
    if !init.exists() {
        write_atomic(&init, initial)?;
    }
    // Host serializes writes under its mutex. On Windows append-only handles cannot set_len during
    // rollback, so request ordinary write permission and explicitly seek to the committed prefix end.
    let mut f = OpenOptions::new().create(true).read(true).write(true).open(dir.join("journal.ndjson"))?;
    f.seek(SeekFrom::End(0))?;
    let mut buf = String::new();
    for l in lines {
        buf.push_str(l);
        buf.push('\n');
    }
    append_bytes(&mut f, buf.as_bytes())
}

/// Best-effort loss notice. Its absence never means that a spilled prefix is a complete session archive.
pub(crate) fn mark_journal_incomplete(data: &Path, session: &str, lost_entries: u64, archive_corrupt: bool) -> io::Result<()> {
    let dir = data.join(SESSIONS).join(session);
    fs::create_dir_all(&dir)?;
    let text = serde_json::json!({ "format": "n7rt-phys-journal-incomplete", "version": 1,
        "lost_entries": lost_entries, "archive_corrupt": archive_corrupt, "complete": false }).to_string();
    write_atomic(&dir.join("incomplete.json"), &text)
}

#[cfg(test)]
#[path = "journal_io_tests.rs"]
mod journal_io_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_rules() {
        for ok in ["a", "Z9", "tp1-capteur_can", "0abc", &"x".repeat(64)] {
            assert!(valid_name(ok), "{ok}");
        }
        for bad in ["", "-a", "_a", "a b", "a.b", "é", "a/b", "CON", "nul", "Com1", "lpt9", &"x".repeat(65)] {
            assert!(!valid_name(bad), "{bad}");
        }
        // Reserved only as a whole name.
        assert!(valid_name("CONSOLE") && valid_name("com10"));
    }

    #[test]
    fn utc_timestamps() {
        assert_eq!(timestamp(UNIX_EPOCH), "19700101-000000-000");
        // 2000-03-01 (leap year) is day 11017; 2026-10-05 12:34:56.789 UTC.
        assert_eq!(timestamp(UNIX_EPOCH + std::time::Duration::from_secs(11_017 * 86_400)), "20000301-000000-000");
        assert_eq!(timestamp(UNIX_EPOCH + std::time::Duration::from_millis(1_791_203_696_789)), "20261005-123456-789");
    }

    #[test]
    fn save_list_read_case_insensitive() {
        let dir = std::env::temp_dir().join(format!("n7rt-phys-files-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(list(&dir).expect("no directory yet"), Vec::new());
        save(&dir, "Banc", false, "{}\n").expect("saved");
        assert_eq!(save(&dir, "banc", false, "{}\n"), Err(FileError::NameExists));
        save(&dir, "banc", true, "{\"v\": 2}\n").expect("overwritten");
        let names: Vec<String> = list(&dir).expect("listed").into_iter().map(|e| e.name).collect();
        assert_eq!(names, vec!["banc".to_string()], "the old case is replaced");
        assert_eq!(read(&dir, "BANC").expect("read"), ("banc".to_string(), "{\"v\": 2}\n".to_string()));
        assert_eq!(read(&dir, "absent"), Err(FileError::NotFound));
        fs::write(dir.join(CIRCUITS).join("bad name.json"), "x").expect("stray file");
        assert_eq!(list(&dir).expect("listed").len(), 1, "invalid names are not listed");
        append_journal(&dir, "s1", "init\n", &["{\"a\":1}".to_string()]).expect("spill");
        append_journal(&dir, "s1", "other\n", &["{\"a\":2}".to_string()]).expect("spill");
        let session = dir.join(SESSIONS).join("s1");
        assert_eq!(fs::read_to_string(session.join("initial.json")).expect("initial"), "init\n");
        assert_eq!(fs::read_to_string(session.join("journal.ndjson")).expect("journal"), "{\"a\":1}\n{\"a\":2}\n");
        let _ = fs::remove_dir_all(&dir);
    }
}
