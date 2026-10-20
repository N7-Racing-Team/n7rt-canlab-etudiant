//! Student editor: two source files in the user's profile, and fixed, bounded build/flash commands.
//! The Python helper verifies the SDK and only writes the student flash region. The link is closed
//! before programming and the same probe/MCU/runtime/student CRC are checked after reconnecting.

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Stdio};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use n7rt_lab::model::{LinkState, Status};
use n7rt_lab::{Command, Lab};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::{mission::MissionState, AppState};

const FILES: [&str; 2] = ["student_decoder.c", "student_decoder.h"];
const TEMPLATES: [&str; 2] = [
    include_str!("../../../firmware/n7rt_student/N7RT_CANLab_Student/student/student_decoder.c"),
    include_str!("../../../firmware/n7rt_student/N7RT_CANLab_Student/student/student_decoder.h"),
];
const MAX_SOURCE: usize = 256 * 1024;
const MAX_OUTPUT: usize = 4 * 1024 * 1024;
const BUSY: &str = "Une compilation ou un chargement est en cours : attendez sa fin.";
const FILE_ERROR: &str = "Fichier étudiant inaccessible : vérifiez les droits du dossier de l'application.";
const TOOL_ERROR: &str = "Outil de compilation indisponible : installez les prérequis du projet étudiant.";
// `python` absent from the PATH the application started with, or only the Microsoft Store alias (exit code 9009).
const PYTHON_MISSING: &str = "Python introuvable : installez Python 3.11 ou plus récent en cochant « Add python.exe to PATH », \
    puis fermez et relancez CAN Lab. Voir DEMARRER_ICI.md (prérequis) dans le dossier de l'application.";
const TOOL_SILENT: &str = "L'outil de compilation (Python) s'est arrêté sans réponse : vérifiez Python 3.11 ou plus récent \
    avec les commandes de docs/stm32-toolchain-windows.md (section Diagnostic), puis relancez l'opération.";

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Diagnostic {
    pub severity: String,
    #[serde(default)]
    pub file: Option<String>,
    #[serde(default)]
    pub line: Option<u32>,
    #[serde(default)]
    pub column: Option<u32>,
    pub message: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Artifact {
    pub sha256: String,
    pub student_crc32: u32,
    pub flash_bytes: u64,
    pub ram_bytes: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ToolResult {
    pub ok: bool,
    pub operation: String,
    #[serde(default)]
    pub diagnostics: Vec<Diagnostic>,
    #[serde(default)]
    pub checks: Vec<serde_json::Value>,
    #[serde(default)]
    pub artifact: Option<Artifact>,
    #[serde(default, alias = "error")]
    pub message: Option<String>,
    #[serde(default)]
    pub log: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IdeView {
    pub available: bool,
    pub reason: Option<String>,
    pub busy: bool,
    pub phase: &'static str,
    pub revision: u64,
    pub built_revision: Option<u64>,
    pub files: [&'static str; 2],
    pub diagnostics: Vec<Diagnostic>,
    pub build: Option<ToolResult>,
    pub flash: Option<ToolResult>,
}

#[derive(Debug, Serialize)]
pub struct SourceFile {
    pub name: String,
    pub content: String,
    pub revision: u64,
}

struct Inner {
    workspace: Option<PathBuf>,
    root: Option<PathBuf>,
    view: IdeView,
}

#[derive(Clone)]
pub struct IdeState(Arc<Mutex<Inner>>);

fn source_index(name: &str) -> Result<usize, String> {
    FILES.iter().position(|n| *n == name).ok_or_else(|| "Seuls student_decoder.c et student_decoder.h sont modifiables.".into())
}

fn validate_source(content: &str) -> Result<(), String> {
    if content.len() > MAX_SOURCE {
        return Err("Fichier trop volumineux : limite de 256 Kio par fichier.".into());
    }
    if content.contains('\0') {
        return Err("Le fichier doit contenir du texte C, sans octet nul.".into());
    }
    Ok(())
}

fn linked(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() { return true; }
    #[cfg(windows)] {
        use std::os::windows::fs::MetadataExt;
        return metadata.file_attributes() & 0x400 != 0; // FILE_ATTRIBUTE_REPARSE_POINT
    }
    #[cfg(not(windows))] { false }
}

fn safe_source(workspace: &Path, name: &str) -> Result<PathBuf, String> {
    source_index(name)?;
    let directory = fs::symlink_metadata(workspace).map_err(|_| FILE_ERROR)?;
    if !directory.is_dir() || linked(&directory) { return Err(FILE_ERROR.into()); }
    let path = workspace.join(name);
    if let Ok(meta) = fs::symlink_metadata(&path) {
        if !meta.is_file() || linked(&meta) {
            return Err(FILE_ERROR.into());
        }
    }
    Ok(path)
}

fn read_source(workspace: &Path, name: &str) -> Result<String, String> {
    let path = safe_source(workspace, name)?;
    let mut bytes = Vec::new();
    fs::File::open(path).map_err(|_| FILE_ERROR)?.take((MAX_SOURCE + 1) as u64)
        .read_to_end(&mut bytes).map_err(|_| FILE_ERROR)?;
    if bytes.len() > MAX_SOURCE {
        return Err("Fichier trop volumineux : limite de 256 Kio par fichier.".into());
    }
    let content = String::from_utf8(bytes).map_err(|_| "Le fichier doit être du texte UTF-8.".to_owned())?;
    validate_source(&content)?;
    Ok(content)
}

fn write_source(workspace: &Path, name: &str, content: &str) -> Result<bool, String> {
    validate_source(content)?;
    let path = safe_source(workspace, name)?;
    if path.exists() && read_source(workspace, name).is_ok_and(|current| current == content) {
        return Ok(false);
    }
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
    let pending = workspace.join(format!("{name}.pending-{}-{nonce}", std::process::id()));
    let mut file = OpenOptions::new().write(true).create_new(true).open(&pending).map_err(|_| FILE_ERROR)?;
    let result = file.write_all(content.as_bytes()).and_then(|()| file.sync_all());
    drop(file);
    if result.is_err() || fs::rename(&pending, &path).is_err() {
        let _ = fs::remove_file(pending);
        return Err(FILE_ERROR.into());
    }
    Ok(true)
}

fn discover_root(starts: &[PathBuf]) -> Option<PathBuf> {
    starts.iter().flat_map(|p| p.ancestors()).find(|p| {
        p.join("tools/stm32/student_ide.py").is_file()
            && (p.join("firmware/n7rt_student/N7RT_CANLab_Student").is_dir() || p.join("N7RT_CANLab_Student").is_dir())
    }).map(Path::to_path_buf)
}

impl IdeState {
    pub fn start(data_dir: Option<PathBuf>) -> Self {
        Self::create(data_dir.map(|p| p.join("student-ide")), discover_root(&n7rt_lab::mission::default_starts()))
    }

    fn create(workspace: Option<PathBuf>, root: Option<PathBuf>) -> Self {
        let mut reason = None;
        let workspace = workspace.and_then(|p| {
            if fs::create_dir_all(&p).is_err() || fs::symlink_metadata(&p).map_or(true, |m| linked(&m) || !m.is_dir()) {
                reason = Some(FILE_ERROR.to_owned());
                return None;
            }
            for (name, template) in FILES.iter().zip(TEMPLATES) {
                if !p.join(name).exists() {
                    if let Err(e) = write_source(&p, name, template) {
                        reason = Some(e);
                        return None;
                    }
                }
            }
            Some(p)
        });
        if workspace.is_none() && reason.is_none() {
            reason = Some(FILE_ERROR.into());
        }
        if root.is_none() && reason.is_none() {
            reason = Some("Projet étudiant introuvable : placez le SDK et les outils à côté de l'application.".into());
        }
        let view = IdeView {
            available: workspace.is_some() && root.is_some(), reason, busy: false, phase: "idle",
            revision: 1, built_revision: None, files: FILES, diagnostics: Vec::new(), build: None, flash: None,
        };
        Self(Arc::new(Mutex::new(Inner { workspace, root, view })))
    }

    fn lock(&self) -> Result<MutexGuard<'_, Inner>, String> {
        self.0.lock().map_err(|_| "Éditeur temporairement indisponible : relancez l'application.".into())
    }

    fn view(&self) -> Result<IdeView, String> {
        Ok(self.lock()?.view.clone())
    }

    /// Serial/session commands share this short lock with the start of a flash operation.
    pub fn with_link_available<T>(&self, action: impl FnOnce() -> T) -> Result<T, String> {
        let inner = self.lock()?;
        if matches!(inner.view.phase, "flashing" | "reconnecting") {
            return Err("Chargement en cours : attendez la reconnexion de la carte.".into());
        }
        Ok(action())
    }

    fn read(&self, name: &str) -> Result<SourceFile, String> {
        let inner = self.lock()?;
        let workspace = inner.workspace.as_ref().ok_or(FILE_ERROR)?;
        Ok(SourceFile { name: name.into(), content: read_source(workspace, name)?, revision: inner.view.revision })
    }

    fn save(&self, name: &str, content: &str) -> Result<IdeView, String> {
        let mut inner = self.lock()?;
        if inner.view.busy { return Err(BUSY.into()); }
        let workspace = inner.workspace.as_ref().ok_or(FILE_ERROR)?;
        if write_source(workspace, name, content)? { invalidate(&mut inner.view); }
        Ok(inner.view.clone())
    }

    fn restore(&self) -> Result<IdeView, String> {
        let mut inner = self.lock()?;
        if inner.view.busy { return Err(BUSY.into()); }
        let workspace = inner.workspace.clone().ok_or(FILE_ERROR)?;
        // Validate both destinations before writing either one.
        for name in FILES { safe_source(&workspace, name)?; }
        let mut changed = false;
        for (name, content) in FILES.iter().zip(TEMPLATES) {
            if write_source(&workspace, name, content)? && !changed {
                // The first successful write invalidates the image even if the next file fails.
                invalidate(&mut inner.view);
                changed = true;
            }
        }
        Ok(inner.view.clone())
    }

    fn begin(&self, phase: &'static str) -> Result<(PathBuf, PathBuf, u64), String> {
        let mut inner = self.lock()?;
        if inner.view.busy { return Err(BUSY.into()); }
        let workspace = inner.workspace.clone().ok_or(FILE_ERROR)?;
        let root = inner.root.clone().ok_or(TOOL_ERROR)?;
        for name in FILES { read_source(&workspace, name)?; }
        if phase == "flashing" { current_artifact(&inner.view)?; }
        inner.view.busy = true;
        inner.view.phase = phase;
        inner.view.diagnostics.clear();
        if phase == "building" { inner.view.built_revision = None; inner.view.flash = None; }
        Ok((workspace, root, inner.view.revision))
    }

    fn finish(&self, operation: &str, revision: u64, result: Result<ToolResult, String>) -> Result<IdeView, String> {
        let mut inner = self.lock()?;
        inner.view.busy = false;
        inner.view.phase = "idle";
        match result {
            Ok(report) => {
                inner.view.diagnostics = report.diagnostics.clone();
                if operation == "build" {
                    inner.view.built_revision = (report.ok && report.artifact.is_some() && inner.view.revision == revision).then_some(revision);
                    inner.view.build = Some(report);
                } else { inner.view.flash = Some(report); }
                Ok(inner.view.clone())
            }
            Err(message) => {
                inner.view.diagnostics = vec![Diagnostic { severity: "error".into(), message: message.clone(), ..Diagnostic::default() }];
                if operation == "build" { inner.view.built_revision = None; inner.view.build = None; }
                else { inner.view.flash = None; }
                Err(message)
            }
        }
    }
}

fn invalidate(view: &mut IdeView) {
    view.revision = view.revision.saturating_add(1);
    view.built_revision = None;
    view.flash = None;
    view.diagnostics.clear();
}

fn current_artifact(view: &IdeView) -> Result<Artifact, String> {
    if view.built_revision != Some(view.revision) {
        return Err("Compilez les fichiers enregistrés avant de les charger sur la carte.".into());
    }
    view.build.as_ref().filter(|b| b.ok).and_then(|b| b.artifact.clone())
        .ok_or_else(|| "Aucune compilation valide à charger.".into())
}

fn public_text(text: &str, workspace: &Path, root: &Path) -> String {
    let mut text = text.to_owned();
    for (path, alias) in [(workspace, "projet étudiant"), (root, "SDK")] {
        let value = path.to_string_lossy();
        for spelling in [value.to_string(), value.replace('\\', "/"), value.replace('/', "\\")] {
            text = text.replace(&spelling, alias);
        }
    }
    redact_absolute_paths(&text).chars().take(4096).collect()
}

/// External tools can name a temp directory outside our workspace. Keep machine paths out of IPC,
/// including quoted names with spaces, UNC paths and Unix paths; preserve public web addresses.
fn redact_absolute_paths(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let previous = i.checked_sub(1).map(|n| chars[n]);
        let boundary = previous.is_none_or(|c| c.is_whitespace() || "\"'(<[=:".contains(c));
        let drive = previous.is_none_or(|c| !c.is_ascii_alphanumeric() && c != '_')
            && chars[i].is_ascii_alphabetic() && chars.get(i + 1) == Some(&':')
            && chars.get(i + 2).is_some_and(|c| matches!(c, '/' | '\\'));
        let unc = chars[i] == '\\' && chars.get(i + 1) == Some(&'\\');
        let unix = chars[i] == '/' && boundary && previous != Some(':')
            && chars.get(i + 1).is_some_and(|c| c.is_alphanumeric() || matches!(c, '.' | '_'));
        if drive || unc || unix {
            let start = i;
            let quoted = previous.filter(|c| matches!(c, '\'' | '"'));
            i += if drive { 3 } else if unc { 2 } else { 1 };
            while i < chars.len() {
                let c = chars[i];
                let terminator = quoted.is_some_and(|q| c == q)
                    || matches!(c, '\n' | '\r' | '\'' | '"' | '<' | '>' | ')' | ']' | ';')
                    || (c == ':' && i > start + 2 && chars.get(i + 1).is_some_and(|n| n.is_whitespace() || n.is_ascii_digit()));
                if terminator { break; }
                i += 1;
            }
            out.push_str("<chemin local>");
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

fn sanitize_json(value: &mut serde_json::Value, workspace: &Path, root: &Path) {
    match value {
        serde_json::Value::String(s) => *s = public_text(s, workspace, root),
        serde_json::Value::Array(a) => for v in a { sanitize_json(v, workspace, root); },
        serde_json::Value::Object(o) => for v in o.values_mut() { sanitize_json(v, workspace, root); },
        _ => {}
    }
}

fn decode_report(bytes: &[u8], operation: &str, workspace: &Path, root: &Path, success: bool) -> Result<ToolResult, String> {
    if bytes.len() > MAX_OUTPUT { return Err("Sortie de compilation trop volumineuse.".into()); }
    let mut report: ToolResult = serde_json::from_slice(bytes).map_err(|_| TOOL_SILENT.to_owned())?;
    if report.operation != operation || (report.ok && !success) { return Err("L'outil n'a pas terminé correctement.".into()); }
    if operation == "build" && report.ok {
        let artifact = report.artifact.as_ref().ok_or("La compilation n'a produit aucune image étudiante validée.")?;
        if artifact.sha256.len() != 64 || !artifact.sha256.bytes().all(|b| b.is_ascii_hexdigit()) || artifact.flash_bytes == 0 {
            return Err("Image de compilation invalide : aucune écriture sur la carte n'est possible.".into());
        }
    }
    report.diagnostics.truncate(100);
    for diagnostic in &mut report.diagnostics {
        diagnostic.file = diagnostic.file.as_ref().and_then(|f| {
            let filename = f.rsplit(['/', '\\']).next()?;
            FILES.contains(&filename).then(|| filename.to_owned())
        });
        diagnostic.message = public_text(&diagnostic.message, workspace, root);
    }
    report.checks.truncate(128);
    for check in &mut report.checks { sanitize_json(check, workspace, root); }
    report.message = report.message.map(|s| public_text(&s, workspace, root));
    report.log = report.log.map(|s| public_text(&s, workspace, root));
    if !report.ok && report.diagnostics.is_empty() {
        report.diagnostics.push(Diagnostic {
            severity: "error".into(),
            message: report.message.clone().unwrap_or_else(|| "L'opération n'a pas abouti : consultez les messages de compilation.".into()),
            ..Diagnostic::default()
        });
    }
    Ok(report)
}

fn capture(mut reader: impl Read) -> Vec<u8> {
    let mut out = Vec::new();
    let mut buf = [0u8; 8192];
    while let Ok(n) = reader.read(&mut buf) {
        if n == 0 { break; }
        let keep = n.min((MAX_OUTPUT + 1).saturating_sub(out.len()));
        out.extend_from_slice(&buf[..keep]);
    }
    out
}

fn run_tool(root: &Path, workspace: &Path, operation: &str, extra: &[String]) -> Result<ToolResult, String> {
    let script = root.join("tools/stm32/student_ide.py");
    let mut command = ProcessCommand::new(std::env::var_os("N7RT_PYTHON").unwrap_or_else(|| "python".into()));
    command.args(["-u", "-X", "utf8"]).arg(script).arg(operation).arg("--workspace").arg(workspace).args(extra)
        .current_dir(root).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)] {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    let mut child = command.spawn()
        .map_err(|e| if e.kind() == std::io::ErrorKind::NotFound { PYTHON_MISSING } else { TOOL_ERROR }.to_owned())?;
    let stdout = child.stdout.take().ok_or(TOOL_ERROR)?;
    let stderr = child.stderr.take().ok_or(TOOL_ERROR)?;
    let output = thread::spawn(move || capture(stdout));
    let errors = thread::spawn(move || capture(stderr));
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if started.elapsed() < Duration::from_secs(240) => thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break Err("L'outil ne répond plus : aucune autre action n'a été lancée.".to_owned());
            }
        }
    };
    let bytes = output.join().map_err(|_| TOOL_ERROR.to_owned())?;
    let errors = errors.join().unwrap_or_default(); // Never shown (may contain a private machine path): only classified.
    let status = status?;
    if bytes.iter().all(u8::is_ascii_whitespace) { return Err(silent_failure(status.code(), &errors).into()); }
    decode_report(&bytes, operation, workspace, root, status.success())
}

/// The helper printed no report: Python itself is missing (Store alias) or stopped before the helper could answer.
fn silent_failure(code: Option<i32>, stderr: &[u8]) -> &'static str {
    let text = String::from_utf8_lossy(stderr);
    if code == Some(9009) || text.contains("Microsoft Store") { PYTHON_MISSING } else { TOOL_SILENT }
}

#[derive(Debug)]
struct FlashTarget { serial: String, uid: String, crc: u32 }

fn flash_target(status: &Status, artifact: &Artifact) -> Result<FlashTarget, String> {
    if status.session.is_some() { return Err("Arrêtez la session avant de charger votre programme.".into()); }
    let board = status.board.as_ref().ok_or("Connectez d'abord la carte dans le cockpit.")?;
    if !board.runtime_verified || !board.compatible || !matches!(status.state, LinkState::Ready | LinkState::NoStudent | LinkState::StudentDisabled) {
        return Err("La carte doit être connectée avec le runtime N7RT vérifié avant le chargement.".into());
    }
    let serial = status.port.as_ref().filter(|p| p.is_stlink()).and_then(|p| p.serial.clone())
        .ok_or("Sonde ST-LINK non identifiée : reconnectez la carte.")?;
    Ok(FlashTarget { serial, uid: board.uid.clone(), crc: artifact.student_crc32 })
}

fn wait_disconnected(lab: &Lab) -> Result<(), String> {
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(5) {
        let status = lab.snapshot().status;
        if status.state == LinkState::Disconnected && status.port.is_none() && status.session.is_none() { return Ok(()); }
        thread::sleep(Duration::from_millis(25));
    }
    Err("Le port de la carte n'a pas été libéré : aucun chargement n'a été lancé.".into())
}

fn wait_reconnected(lab: &Lab, target: &FlashTarget, compare_crc: bool) -> Result<(), String> {
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(15) {
        let status = lab.snapshot().status;
        if let Some(board) = status.board {
            if board.uid != target.uid { return Err("Une autre carte a répondu après le chargement.".into()); }
            if matches!(status.state, LinkState::Ready | LinkState::NoStudent | LinkState::StudentDisabled | LinkState::Incompatible) {
                if !board.runtime_verified || !board.compatible { return Err("L'intégrité du runtime n'est pas confirmée après reconnexion.".into()); }
                let crc = u32::from_str_radix(board.student_crc32.trim_start_matches("0x"), 16).ok();
                if compare_crc && (crc != Some(target.crc) || board.student_status != 1) {
                    return Err("Le programme lu sur la carte ne correspond pas à une compilation valide.".into());
                }
                return Ok(());
            }
        }
        thread::sleep(Duration::from_millis(50));
    }
    Err("Chargement terminé, mais la carte ne répond pas encore : vérifiez son câble puis reconnectez-la.".into())
}

#[tauri::command(rename_all = "snake_case")]
pub fn ide_state(state: State<'_, IdeState>) -> Result<IdeView, String> { state.view() }

#[tauri::command(rename_all = "snake_case")]
pub fn ide_read(state: State<'_, IdeState>, name: String) -> Result<SourceFile, String> { state.read(&name) }

#[tauri::command(rename_all = "snake_case")]
pub fn ide_save(state: State<'_, IdeState>, name: String, content: String) -> Result<IdeView, String> { state.save(&name, &content) }

#[tauri::command(rename_all = "snake_case")]
pub fn ide_restore(state: State<'_, IdeState>) -> Result<IdeView, String> { state.restore() }

#[tauri::command(rename_all = "snake_case")]
pub async fn ide_build(state: State<'_, IdeState>) -> Result<IdeView, String> {
    let (workspace, root, revision) = state.begin("building")?;
    let state = state.inner().clone();
    let result = tauri::async_runtime::spawn_blocking(move || run_tool(&root, &workspace, "build", &[])).await
        .unwrap_or_else(|_| Err("Compilation interrompue : relancez-la.".into()));
    state.finish("build", revision, result)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn ide_flash(state: State<'_, IdeState>, app: State<'_, AppState>, missions: State<'_, MissionState>) -> Result<IdeView, String> {
    let (workspace, root, revision) = state.begin("flashing")?;
    let state = state.inner().clone();
    let artifact = current_artifact(&state.view()?)?;
    let target = if missions.0.view().check.state == "running" {
        Err("Attendez la fin de l'essai avant de charger votre programme.".into())
    } else { flash_target(&app.lab.snapshot().status, &artifact) };
    let target = match target { Ok(t) => t, Err(e) => return state.finish("flash", revision, Err(e)) };
    let lab = app.lab.clone();
    let worker_state = state.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        lab.command(Command::Disconnect);
        if let Err(error) = wait_disconnected(&lab) {
            lab.command(Command::Connect { port: None, serial: Some(target.serial.clone()) });
            return Err(error);
        }
        let extra = vec!["--serial".into(), target.serial.clone(), "--uid".into(), target.uid.clone(), "--expected-sha256".into(), artifact.sha256];
        let result = run_tool(&root, &workspace, "flash", &extra);
        if let Ok(mut inner) = worker_state.lock() { inner.view.phase = "reconnecting"; }
        lab.command(Command::Connect { port: None, serial: Some(target.serial.clone()) });
        let reconnect = wait_reconnected(&lab, &target, result.as_ref().is_ok_and(|r| r.ok));
        match result {
            Ok(mut report) if report.ok => {
                reconnect?;
                report.checks.push(serde_json::json!({"name":"reconnection", "ok":true, "message":"Même carte, runtime vérifié et CRC étudiant confirmé."}));
                Ok(report)
            }
            other => other,
        }
    }).await.unwrap_or_else(|_| Err("Chargement interrompu : reconnectez la carte avant de réessayer.".into()));
    state.finish("flash", revision, result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let p = std::env::temp_dir().join(format!("n7rt-ide-{}-{}", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
            fs::create_dir_all(&p).unwrap();
            Self(p)
        }
        fn state(&self) -> IdeState { IdeState::create(Some(self.0.join("workspace")), Some(self.0.clone())) }
    }
    impl Drop for Temp { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }

    fn report() -> ToolResult {
        ToolResult { ok: true, operation: "build".into(), diagnostics: vec![], checks: vec![], message: None, log: None,
            artifact: Some(Artifact { sha256: "a".repeat(64), student_crc32: 123, flash_bytes: 1024, ram_bytes: 128 }) }
    }

    #[test]
    fn a_silent_helper_says_what_to_check() {
        assert_eq!(silent_failure(Some(9009), b""), PYTHON_MISSING);
        let store = "Python est introuvable ; exécutez sans arguments pour installer depuis le Microsoft Store";
        assert_eq!(silent_failure(Some(1), store.as_bytes()), PYTHON_MISSING);
        assert_eq!(silent_failure(Some(1), b"SyntaxError: invalid syntax"), TOOL_SILENT);
        assert!(TOOL_SILENT.contains("docs/stm32-toolchain-windows.md") && PYTHON_MISSING.contains("DEMARRER_ICI.md"));
    }

    #[test]
    fn only_exact_source_names_are_accepted() {
        let temp = Temp::new(); let state = temp.state();
        for name in ["../student_decoder.c", "Student_decoder.c", "n7rt.h", "C:\\student_decoder.c", "student_decoder.c/.."] {
            assert!(state.read(name).is_err());
            assert!(state.save(name, "bad").is_err());
        }
        assert_eq!(state.read(FILES[0]).unwrap().content, TEMPLATES[0]);
    }

    #[test]
    fn unchanged_save_keeps_revision_but_changed_code_invalidates_build() {
        let temp = Temp::new(); let state = temp.state();
        state.finish("build", 1, Ok(report())).unwrap();
        assert_eq!(state.save(FILES[0], TEMPLATES[0]).unwrap().built_revision, Some(1));
        let view = state.save(FILES[0], "void student_init(void) {}").unwrap();
        assert_eq!(view.revision, 2); assert_eq!(view.built_revision, None);
        assert!(state.begin("flashing").is_err());
    }

    #[test]
    fn restore_uses_the_embedded_template_and_invalidates_build() {
        let temp = Temp::new(); let state = temp.state();
        state.save(FILES[0], "changed").unwrap(); state.save(FILES[1], "header").unwrap();
        let view = state.restore().unwrap();
        assert_eq!(view.built_revision, None);
        for (name, text) in FILES.iter().zip(TEMPLATES) { assert_eq!(state.read(name).unwrap().content, text); }
    }

    #[test]
    fn source_limits_and_nul_bytes_do_not_touch_original() {
        let temp = Temp::new(); let state = temp.state();
        for content in ["a".repeat(MAX_SOURCE + 1), "a\0b".into()] { assert!(state.save(FILES[0], &content).is_err()); }
        assert_eq!(state.read(FILES[0]).unwrap().content, TEMPLATES[0]);
    }

    #[test]
    fn an_abandoned_temporary_file_does_not_block_saving() {
        let temp = Temp::new(); let state = temp.state();
        fs::write(temp.0.join("workspace/student_decoder.c.pending"), "old partial write").unwrap();
        assert!(state.save(FILES[0], "new source").is_ok());
        assert_eq!(state.read(FILES[0]).unwrap().content, "new source");
    }

    #[cfg(windows)]
    #[test]
    fn partial_restore_invalidates_the_build_before_reporting_second_write_failure() {
        let temp = Temp::new(); let state = temp.state();
        state.save(FILES[0], "changed source").unwrap(); state.save(FILES[1], "changed header").unwrap();
        state.finish("build", 3, Ok(report())).unwrap();
        let header = temp.0.join("workspace").join(FILES[1]);
        let mut permissions = fs::metadata(&header).unwrap().permissions(); permissions.set_readonly(true); fs::set_permissions(&header, permissions).unwrap();
        let result = state.restore();
        let mut permissions = fs::metadata(&header).unwrap().permissions(); permissions.set_readonly(false); fs::set_permissions(&header, permissions).unwrap();
        assert!(result.is_err());
        assert_eq!(state.view().unwrap().revision, 4); assert_eq!(state.view().unwrap().built_revision, None);
        assert_eq!(state.read(FILES[0]).unwrap().content, TEMPLATES[0]);
    }

    #[test]
    fn busy_operations_block_writes_restore_and_other_jobs() {
        let temp = Temp::new(); let state = temp.state();
        state.begin("building").unwrap();
        assert!(state.save(FILES[0], "changed").is_err()); assert!(state.restore().is_err()); assert!(state.begin("building").is_err());
        state.finish("build", 1, Err("failure".into())).unwrap_err();
        assert!(!state.view().unwrap().busy); assert_eq!(state.view().unwrap().built_revision, None);
    }

    #[test]
    fn flashing_reserves_the_link_but_building_does_not() {
        let temp = Temp::new(); let state = temp.state();
        state.begin("building").unwrap(); assert!(state.with_link_available(|| ()).is_ok());
        state.finish("build", 1, Ok(report())).unwrap(); state.begin("flashing").unwrap();
        assert!(state.with_link_available(|| ()).is_err());
    }

    #[test]
    fn malformed_or_failed_tool_success_is_refused() {
        let temp = Temp::new(); let workspace = temp.0.join("workspace");
        assert!(decode_report(b"not json", "build", &workspace, &temp.0, true).is_err());
        let bytes = serde_json::to_vec(&report()).unwrap();
        assert!(decode_report(&bytes, "build", &workspace, &temp.0, false).is_err());
        assert!(decode_report(&bytes, "flash", &workspace, &temp.0, true).is_err());
        let mut missing = report(); missing.artifact = None;
        assert!(decode_report(&serde_json::to_vec(&missing).unwrap(), "build", &workspace, &temp.0, true).is_err());
    }

    #[test]
    fn compiler_diagnostics_use_source_aliases_and_hide_profile_paths() {
        let temp = Temp::new(); let workspace = temp.0.join("workspace"); let mut value = report();
        value.diagnostics.push(Diagnostic { severity: "error".into(), file: Some(workspace.join(FILES[0]).to_string_lossy().into()),
            line: Some(2), column: Some(3), message: format!("{}: error", workspace.display()) });
        let report = decode_report(&serde_json::to_vec(&value).unwrap(), "build", &workspace, &temp.0, true).unwrap();
        assert_eq!(report.diagnostics[0].file.as_deref(), Some(FILES[0]));
        assert!(!report.diagnostics[0].message.contains(&temp.0.to_string_lossy().to_string()));
    }

    #[test]
    fn diagnostics_remove_external_temp_paths_and_keep_web_links_and_error_details() {
        let message = "Erreur outil: 'C:\\Users\\Private Name\\AppData\\Local\\Temp\\n7rt-flash\\image.bin': accès refusé; voir https://example.org/sdk";
        let cleaned = redact_absolute_paths(message);
        assert!(!cleaned.contains("Users")); assert!(!cleaned.contains("Private Name"));
        assert!(cleaned.contains("accès refusé")); assert!(cleaned.contains("https://example.org/sdk"));
        let unix = redact_absolute_paths("/home/private/tmp/image.bin: erreur lecture");
        assert!(!unix.contains("/home")); assert!(unix.contains("erreur lecture"));
        assert!(!redact_absolute_paths("'\\\\server\\private\\source.c': absent").contains("server"));
        assert_eq!(redact_absolute_paths("student_decoder.c:12:4: error: '/' attendu"), "student_decoder.c:12:4: error: '/' attendu");
    }

    #[test]
    fn oversized_sources_already_on_disk_are_refused() {
        let temp = Temp::new(); let state = temp.state();
        fs::write(temp.0.join("workspace").join(FILES[0]), vec![b'a'; MAX_SOURCE + 1]).unwrap();
        assert!(state.read(FILES[0]).is_err()); assert!(state.begin("building").is_err());
        state.restore().unwrap();
        assert_eq!(state.read(FILES[0]).unwrap().content, TEMPLATES[0]);
    }

    #[test]
    fn helper_failures_remain_readable_without_a_compiler_diagnostic() {
        let temp = Temp::new();
        let bytes = br#"{"ok":false,"operation":"build","error":"GCC absent.","diagnostics":[],"checks":[]}"#;
        let report = decode_report(bytes, "build", &temp.0, &temp.0, false).unwrap();
        assert_eq!(report.message.as_deref(), Some("GCC absent."));
        assert_eq!(report.diagnostics[0].message, "GCC absent.");
    }

    #[test]
    fn flash_requires_a_connected_verified_board_and_no_active_session() {
        use n7rt_lab::discovery::PortInfo;
        use n7rt_lab::runtime::BoardIdentity;
        let artifact = report().artifact.unwrap();
        let mut status = Status::default();
        assert!(flash_target(&status, &artifact).is_err());
        status.state = LinkState::Ready;
        status.port = Some(PortInfo { port: "COM-test".into(), vid: Some(0x0483), pid: Some(0x374e), serial: Some("12345678".into()), product: None, probe: Some("STLINK-V3E") });
        status.board = Some(BoardIdentity {
            runtime: "0.1.0".into(), build_id: "test".into(), abi: "0.1".into(), proto: 0,
            uid: "00000001-00000002-00000003".into(), dev_id: 0x450, rev_id: 0, reset_flags: 0,
            reset_causes: vec![], student_status: 1, student_status_text: "code étudiant chargé", student_abi: "0.1".into(),
            student_crc32: "0x0000007B".into(), rx_window: 8, fault_pending: false, runtime_verified: true, compatible: true, problems: vec![],
        });
        let target = flash_target(&status, &artifact).unwrap();
        assert_eq!(target.serial, "12345678"); assert_eq!(target.uid, "00000001-00000002-00000003"); assert_eq!(target.crc, 123);
        status.board.as_mut().unwrap().runtime_verified = false;
        assert!(flash_target(&status, &artifact).is_err());
        status.board.as_mut().unwrap().runtime_verified = true;
        status.session = Some(n7rt_lab::model::SessionView { id: 1, phase: "active", mode: 0, tick_period_ms: 10, elapsed_ms: 0, bus_time_us: 0 });
        assert!(flash_target(&status, &artifact).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn symlink_sources_cannot_write_outside_the_workspace() {
        use std::os::unix::fs::symlink;
        let temp = Temp::new(); let state = temp.state(); let outside = temp.0.join("outside");
        fs::write(&outside, "keep").unwrap(); let source = temp.0.join("workspace").join(FILES[0]);
        fs::remove_file(&source).unwrap(); symlink(&outside, &source).unwrap();
        assert!(state.read(FILES[0]).is_err()); assert!(state.save(FILES[0], "overwrite").is_err()); assert!(state.restore().is_err());
        assert_eq!(fs::read_to_string(outside).unwrap(), "keep");
    }
}
