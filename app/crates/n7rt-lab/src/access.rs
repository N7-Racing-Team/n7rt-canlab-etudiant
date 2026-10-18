//! Student and instructor modes (docs/distribution.md §2): where the instructor directory is, the student's locked
//! mission and imported group pack (`<app data>/student/`), run file names, and the byte transport of the UI.
//!
//! Secrecy: nothing here holds a code, a key or a mission value. The student state names a mission, a cohort, a group
//! and a pack id (all in the clear header of the pack). The instructor directory is only located here; its private
//! files are read by the controller after the instructor code is verified.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

pub const INSTRUCTOR_ENV: &str = "N7RT_INSTRUCTOR_DIR";
/// Name of the instructor repository, looked for next to the application (docs/distribution.md §2.2).
pub const INSTRUCTOR_REPO: &str = "n7rt-canlab-missions";
/// `%APPDATA%\fr.n7rt.canlab\instructor.json` {"instructor_dir": "<absolute path>"}, written by the instructor install.
pub const INSTRUCTOR_CONFIG: &str = "instructor.json";
pub const AUTH_FILE: &str = "instructor/auth.json";
pub const MASTER_KEY_FILE: &str = "instructor/master.key";
pub const PACK_EXT: &str = "n7rtpack";
pub const RUN_EXT: &str = "n7rtrun";
/// `Documents\N7RT CAN Lab\essais`.
pub const RUNS_SUBDIR: [&str; 2] = ["N7RT CAN Lab", "essais"];
/// Bound of a judged run file (bytes), the one of n7rt-tool. Checked before decoding: [`decode_upload`].
pub const MAX_RUN_BYTES: usize = n7rt_tool::MAX_RUN_BYTES as usize;
/// Bound of an imported group pack (bytes), the one of n7rt-tool. Checked before decoding: [`decode_upload`].
pub const MAX_PACK_BYTES: usize = n7rt_tool::MAX_PACK_BYTES as usize;
/// Bound the controller applies after decoding, whatever the file: the larger of the two.
pub const MAX_FILE_BYTES: usize = if MAX_RUN_BYTES > MAX_PACK_BYTES { MAX_RUN_BYTES } else { MAX_PACK_BYTES };

pub const MSG_WRONG_INSTRUCTOR_CODE: &str = "Code encadrant incorrect.";
pub const MSG_NO_INSTRUCTOR: &str = "Ressources encadrant introuvables sur ce poste.";
pub const MSG_WRONG_GROUP_CODE: &str = "Code de groupe incorrect.";
pub const MSG_RUN_TOO_LARGE: &str = "Fichier trop volumineux pour un fichier d'essai.";
pub const MSG_PACK_TOO_LARGE: &str = "Fichier trop volumineux pour un pack de groupe.";

/// Application mode. Student is the default; Instructor lasts until logout or the end of the process (memory only).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    Student,
    Instructor,
}

// ---------------------------------------------------------------------------------------- instructor directory

/// How the instructor directory is found, in order: the variable, the path registered by the instructor install,
/// a directory holding `instructor/auth.json` walking up from each start, a sibling `n7rt-canlab-missions` of any
/// of those ancestors.
#[derive(Debug, Clone, Default)]
pub struct InstructorLocator {
    pub env: Option<OsString>,
    pub config_file: Option<PathBuf>,
    pub starts: Vec<PathBuf>,
}

#[derive(Deserialize)]
struct InstructorConfig {
    instructor_dir: String,
}

impl InstructorLocator {
    /// The application's locator: `N7RT_INSTRUCTOR_DIR`, `<app data>/instructor.json`, the executable's directory and
    /// the current directory.
    pub fn from_environment(app_data_dir: Option<&Path>) -> InstructorLocator {
        InstructorLocator {
            env: std::env::var_os(INSTRUCTOR_ENV),
            config_file: app_data_dir.map(|d| d.join(INSTRUCTOR_CONFIG)),
            starts: crate::mission::default_starts(),
        }
    }

    /// A fixed directory (tests, e2e).
    pub fn fixed(dir: impl Into<PathBuf>) -> InstructorLocator {
        InstructorLocator { env: Some(dir.into().into_os_string()), config_file: None, starts: Vec::new() }
    }

    pub fn locate(&self) -> Option<PathBuf> {
        if let Some(v) = self.env.as_ref().filter(|v| !v.is_empty()) {
            let p = PathBuf::from(v);
            return p.is_dir().then_some(p);
        }
        if let Some(cfg) = self.config_file.as_deref().and_then(|f| std::fs::read(f).ok()) {
            if let Ok(c) = serde_json::from_slice::<InstructorConfig>(&cfg) {
                let p = PathBuf::from(c.instructor_dir);
                if p.is_absolute() && p.is_dir() {
                    return Some(p);
                }
            }
        }
        let ancestors = || self.starts.iter().flat_map(|s| s.ancestors());
        if let Some(a) = ancestors().find(|a| a.join(AUTH_FILE).is_file()) {
            return Some(a.to_path_buf());
        }
        ancestors().map(|a| a.join(INSTRUCTOR_REPO)).find(|p| p.is_dir())
    }
}

/// `instructor/auth.json`.
#[derive(Debug, Deserialize)]
pub struct AuthFile {
    pub format: String,
    pub version: u32,
    pub code_phc: String,
}

/// The PHC string of the instructor code, or None when the file is absent or not an auth file.
pub fn read_auth(dir: &Path) -> Option<String> {
    let a: AuthFile = serde_json::from_slice(&std::fs::read(dir.join(AUTH_FILE)).ok()?).ok()?;
    (a.format == "n7rt-instructor-auth" && a.version == 1 && a.code_phc.starts_with("$argon2")).then_some(a.code_phc)
}

// ---------------------------------------------------------------------------------------------- student state

/// The imported group pack: its file under `student/packs/` and its clear header.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackRef {
    pub file: String,
    pub mission: String,
    pub cohort: String,
    pub group: String,
    pub pack_id: String,
}

/// `student/state.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StudentState {
    /// Mission chosen for the session (locked).
    pub mission: Option<String>,
    pub chosen_utc: Option<String>,
    pub pack: Option<PackRef>,
}

#[derive(Serialize, Deserialize)]
struct StateFile {
    format: String,
    version: u32,
    #[serde(flatten)]
    state: StudentState,
}

const STATE_FORMAT: &str = "n7rt-student-state";

/// The student's files under the application data directory.
#[derive(Debug, Clone, Default)]
pub struct StudentStore {
    pub app_data_dir: Option<PathBuf>,
}

impl StudentStore {
    fn dir(&self) -> Result<PathBuf, String> {
        self.app_data_dir.as_ref().map(|d| d.join("student")).ok_or_else(|| "Dossier de l'application introuvable.".to_owned())
    }

    pub fn packs_dir(&self) -> Result<PathBuf, String> {
        Ok(self.dir()?.join("packs"))
    }

    /// The student directory (`<app data>/student`): lock, imported packs, progressions of release 0.3 packs.
    pub fn student_dir(&self) -> Result<PathBuf, String> {
        self.dir()
    }

    /// The saved state; an absent or unreadable file is no lock.
    pub fn load(&self) -> StudentState {
        let Ok(dir) = self.dir() else { return StudentState::default() };
        std::fs::read(dir.join("state.json"))
            .ok()
            .and_then(|b| serde_json::from_slice::<StateFile>(&b).ok())
            .filter(|f| f.format == STATE_FORMAT && f.version == 1)
            .map(|f| f.state)
            .unwrap_or_default()
    }

    pub fn save(&self, state: &StudentState) -> Result<(), String> {
        let dir = self.dir()?;
        let failed = || "État de la session non enregistré : dossier de l'application inaccessible.".to_owned();
        std::fs::create_dir_all(&dir).map_err(|_| failed())?;
        let file = StateFile { format: STATE_FORMAT.into(), version: 1, state: state.clone() };
        let text = serde_json::to_string_pretty(&file).map_err(|_| failed())?;
        let tmp = dir.join("state.json.tmp");
        std::fs::write(&tmp, text).map_err(|_| failed())?;
        std::fs::rename(&tmp, dir.join("state.json")).map_err(|_| failed())
    }

    /// Copies a group pack as it is into `packs/`, removing the packs imported before. A pack over
    /// [`MAX_PACK_BYTES`] is refused and the imported one kept.
    pub fn store_pack(&self, name: &str, bytes: &[u8]) -> Result<(), String> {
        if bytes.len() > MAX_PACK_BYTES {
            return Err(MSG_PACK_TOO_LARGE.to_owned());
        }
        let dir = self.packs_dir()?;
        let failed = || "Pack non importé : dossier de l'application inaccessible.".to_owned();
        std::fs::create_dir_all(&dir).map_err(|_| failed())?;
        remove_packs(&dir);
        std::fs::write(dir.join(name), bytes).map_err(|_| failed())
    }

    /// A pack of `packs/` by its file name (`<mission>-<group>.n7rtpack`, nothing else); None over
    /// [`MAX_PACK_BYTES`] (checked before reading).
    pub fn read_pack(&self, name: &str) -> Option<Vec<u8>> {
        let stem = name.strip_suffix(&format!(".{PACK_EXT}"))?;
        if !safe_name(stem) && !stem.split_once('-').is_some_and(|(m, g)| safe_name(m) && safe_name(g)) {
            return None;
        }
        let path = self.packs_dir().ok()?.join(name);
        if std::fs::metadata(&path).ok()?.len() > MAX_PACK_BYTES as u64 {
            return None;
        }
        std::fs::read(path).ok()
    }

    /// Instructor reset: no lock, no imported pack.
    pub fn clear(&self) -> Result<(), String> {
        let dir = self.dir()?;
        let state = dir.join("state.json");
        if state.exists() && std::fs::remove_file(&state).is_err() {
            return Err("Réinitialisation impossible : état de la session non supprimé.".into());
        }
        remove_packs(&dir.join("packs"));
        Ok(())
    }
}

fn remove_packs(dir: &Path) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_file() && p.extension().is_some_and(|x| x == PACK_EXT) {
            let _ = std::fs::remove_file(p);
        }
    }
}

/// Names that go into file names and directory names: 1–40 of `[A-Za-z0-9_-]`.
pub fn safe_name(s: &str) -> bool {
    (1..=40).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

// -------------------------------------------------------------------------------------------------- run files

/// `20261006-142233` (UTC).
pub fn run_stamp(t: SystemTime) -> String {
    let s = crate::mission::utc_stamp(t);
    // utc_stamp: 20261006T142233Z
    format!("{}-{}", &s[0..8], &s[9..15])
}

/// `essai-<mission>-<group>-<yyyymmdd-hhmmss>.n7rtrun`.
pub fn run_file_name(mission: &str, group: &str, t: SystemTime) -> String {
    format!("essai-{mission}-{group}-{}.{RUN_EXT}", run_stamp(t))
}

/// A path in `dir` for `name` that does not exist yet (`-2`, `-3`… before the extension).
pub fn unique_path(dir: &Path, name: &str) -> Option<PathBuf> {
    let (stem, ext) = name.rsplit_once('.').unwrap_or((name, ""));
    (1..100)
        .map(|k| if k == 1 { dir.join(name) } else { dir.join(format!("{stem}-{k}.{ext}")) })
        .find(|p| !p.exists())
}

// ------------------------------------------------------------------------------------------------------ base64

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 with padding (files sent by the UI).
pub fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() {
                out.push(B64[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Standard base64 (padding optional, ASCII whitespace ignored); None on any other character.
pub fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let (mut acc, mut bits) = (0u32, 0u32);
    let mut pad = 0;
    for b in text.bytes() {
        if b.is_ascii_whitespace() {
            continue;
        }
        if b == b'=' {
            pad += 1;
            continue;
        }
        if pad > 0 {
            return None;
        }
        let v = B64.iter().position(|&c| c == b)? as u32;
        acc = acc << 6 | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    (pad <= 2 && bits < 6).then_some(out)
}

/// A file of at most `max` bytes sent by the UI as base64 text (no whitespace): refused with `too_large` when the text
/// is longer than the base64 of `max` bytes, before anything is decoded (the padding then settles the last bytes);
/// « Fichier illisible. » when it is not base64.
pub fn decode_upload(data: &str, max: usize, too_large: &str) -> Result<Vec<u8>, String> {
    if data.len() > max.div_ceil(3) * 4 {
        return Err(too_large.to_owned());
    }
    let bytes = base64_decode(data).ok_or_else(|| "Fichier illisible.".to_owned())?;
    if bytes.len() > max {
        return Err(too_large.to_owned());
    }
    Ok(bytes)
}

/// Overwrites a buffer that held a decrypted pack before it is freed.
pub fn wipe(buf: &mut [u8]) {
    for b in buf.iter_mut() {
        // SAFETY: a valid, aligned, exclusive reference.
        unsafe { std::ptr::write_volatile(b, 0) };
    }
    std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_round_trips() {
        for n in 0..40usize {
            let data: Vec<u8> = (0..n).map(|i| (i * 37 + 11) as u8).collect();
            let text = base64_encode(&data);
            assert_eq!(text.len() % 4, 0);
            assert_eq!(base64_decode(&text).as_deref(), Some(data.as_slice()), "{n}");
        }
        assert_eq!(base64_encode(b"Man"), "TWFu");
        assert_eq!(base64_encode(b"Ma"), "TWE=");
        assert_eq!(base64_decode("TWE").as_deref(), Some(&b"Ma"[..]));
        assert_eq!(base64_decode("TW\nFu").as_deref(), Some(&b"Man"[..]));
        assert!(base64_decode("TW=u").is_none());
        assert!(base64_decode("T*Fu").is_none());
    }

    #[test]
    fn uploads_are_bounded_before_decoding() {
        assert_eq!((MAX_RUN_BYTES, MAX_PACK_BYTES, MAX_FILE_BYTES), (8 << 20, 8 << 20, 8 << 20));
        assert_eq!(decode_upload(&base64_encode(b"Man"), 3, "trop").as_deref(), Ok(&b"Man"[..]));
        assert_eq!(decode_upload(&base64_encode(b"Mans"), 3, "trop"), Err("trop".to_owned()));
        assert_eq!(decode_upload(&base64_encode(b"Ma"), 1, "trop"), Err("trop".to_owned()), "same text length, one byte over");
        // A text too long is refused before it is decoded (this one is not even base64).
        assert_eq!(decode_upload("*****", 3, "trop"), Err("trop".to_owned()));
        assert_eq!(decode_upload("T*Fu", 3, "trop"), Err("Fichier illisible.".to_owned()));
        let pack = vec![7u8; MAX_PACK_BYTES];
        assert_eq!(decode_upload(&base64_encode(&pack), MAX_PACK_BYTES, MSG_PACK_TOO_LARGE), Ok(pack));
        let over = base64_encode(&vec![7u8; MAX_PACK_BYTES + 1]);
        assert_eq!(decode_upload(&over, MAX_PACK_BYTES, MSG_PACK_TOO_LARGE), Err(MSG_PACK_TOO_LARGE.to_owned()));
        // a release 0.3 pack and a run file share the bound of 8 MiB
        assert_eq!(decode_upload(&over, MAX_RUN_BYTES, MSG_RUN_TOO_LARGE), Err(MSG_RUN_TOO_LARGE.to_owned()));
    }

    #[test]
    fn packs_over_the_bound_are_neither_stored_nor_read() {
        let dir = std::env::temp_dir().join(format!("n7rt-access-packs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = StudentStore { app_data_dir: Some(dir.clone()) };
        store.store_pack("apps-G01.n7rtpack", b"{}").unwrap();
        assert_eq!(store.store_pack("apps-G02.n7rtpack", &vec![0u8; MAX_PACK_BYTES + 1]), Err(MSG_PACK_TOO_LARGE.to_owned()));
        assert_eq!(store.read_pack("apps-G01.n7rtpack").as_deref(), Some(&b"{}"[..]), "the imported pack is kept");
        let big = store.packs_dir().unwrap().join("apps-G03.n7rtpack");
        std::fs::File::create(&big).unwrap().set_len(MAX_PACK_BYTES as u64 + 1).unwrap();
        assert!(store.read_pack("apps-G03.n7rtpack").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn names_and_stamps() {
        assert!(safe_name("G01") && safe_name("promo-2026_A"));
        assert!(!safe_name("") && !safe_name("../x") && !safe_name("a b") && !safe_name("é") && !safe_name(&"x".repeat(41)));
        let t = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_791_296_553);
        assert_eq!(run_stamp(t), "20261006-142233");
        assert_eq!(run_file_name("apps", "G01", t), "essai-apps-G01-20261006-142233.n7rtrun");
    }
}
