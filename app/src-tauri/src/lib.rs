//! N7RT CAN Lab: Tauri shell around the `n7rt-lab` core.
//!
//! Threads: the Tauri main thread (window, WebView) never touches the serial port. Commands only
//! post to the core (`Lab::command`, non-blocking). The UI pulls what changed with `lab_drain`
//! every 50 ms (ui/src/main.ts): an `invoke` answered through the IPC protocol. Pushing with
//! `emit` instead (one script evaluation per event in the WebView) made the WebView renderer
//! grow by ~1.8 MB/s at 600 frames/s on WebView2; pulling also lets a slow or hidden window
//! simply ask less often, without any backlog.
//!
//! Automated end-to-end mode (`--e2e functional|perf --serial <SN> --report <file> [--coordinated]`): the UI
//! drives its own controls (ui/src/e2e.ts), reads back what it displayed and asks the core to
//! compare it with the frames and answers that really crossed the link. Used by
//! `stm32lab app-test`.
//!
//! The Physical Lab (`phys` module, docs/physical-lab-v1.md §8.3) has its own engine and commands; its
//! scenarios `--e2e phys` and `--e2e phys-perf` need no board.
//!
//! Missions (`mission` module, docs/apps-mission-slice.md §6, docs/distribution.md §2): the controller shares the link
//! core and the Physical Lab, in student mode (default) or instructor mode. `--e2e mission-apps [--expect-verdict
//! pass|fail|any]` logs in as instructor and plays the APPS mission with its reference pack on a board; `--e2e
//! student-apps` plays the student flow (lock, group pack, run file). Their secrets come from the environment, in e2e
//! mode only: N7RT_E2E_INSTRUCTOR_CODE, N7RT_E2E_GROUP_CODE (never written into the report), and the group pack file
//! N7RT_E2E_PACK; the instructor directory is N7RT_INSTRUCTOR_DIR as for any run.

mod ide;
mod mission;
mod phys;

use std::path::PathBuf;
use std::io::Read;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use n7rt_lab::access::base64_encode;
use n7rt_lab::e2e::{verify_display, DisplayedRow, Verification};
use n7rt_lab::model::UiBatch;
use n7rt_lab::sim::Stimuli;
use n7rt_lab::{Command, Lab, LabConfig, MissionConfig, Missions, SerialConnector};
use serde::Serialize;
use tauri::{Manager, State};

#[derive(Debug, Clone, Serialize, Default)]
pub struct E2eConfig {
    pub scenario: String,
    pub serial: Option<String>,
    pub report: PathBuf,
    pub seconds: u32,
    pub rate: u32,
    /// Steps synchronised with the harness (MCU halt through SWD, port held by another program).
    pub coordinated: bool,
    /// mission-apps: verdict the nominal check must give, "pass" | "fail" | "any".
    pub expect_verdict: String,
    /// Mission scenarios: N7RT_E2E_INSTRUCTOR_CODE (scrubbed from the report).
    pub instructor_code: Option<String>,
    /// student-apps: N7RT_E2E_GROUP_CODE (scrubbed from the report).
    pub group_code: Option<String>,
    /// student-apps: file name of N7RT_E2E_PACK (its bytes: `e2e_pack`).
    pub pack_name: Option<String>,
    /// progression-<id>: N7RT_E2E_STAGE, « full » (default), « until-incident » (stops in incident 1 and closes the
    /// application) or « resume » (reopens the mission and goes on).
    pub stage: Option<String>,
}

/// Environment variables of the mission scenarios.
const E2E_INSTRUCTOR_CODE: &str = "N7RT_E2E_INSTRUCTOR_CODE";
const E2E_GROUP_CODE: &str = "N7RT_E2E_GROUP_CODE";
const E2E_PACK: &str = "N7RT_E2E_PACK";
const E2E_ANSWERS: &str = "N7RT_E2E_ANSWERS";
const E2E_STAGE: &str = "N7RT_E2E_STAGE";
/// Bound of the answers file of the progression scenario.
const E2E_ANSWERS_MAX_BYTES: u64 = 1 << 20;
/// Report fields that never reach the report file.
const E2E_SECRETS: [&str; 2] = ["instructor_code", "group_code"];

struct AppState {
    lab: Arc<Lab>,
    e2e: Option<E2eConfig>,
}

fn parse_args() -> Option<E2eConfig> {
    let args: Vec<String> = std::env::args().collect();
    let value = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
    let scenario = value("--e2e")?;
    let board = !phys::board_free(&scenario);
    let env = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    Some(E2eConfig {
        scenario,
        serial: if board { value("--serial") } else { None },
        report: PathBuf::from(value("--report").unwrap_or_else(|| "n7rt-can-lab-e2e.json".into())),
        seconds: value("--seconds").and_then(|s| s.parse().ok()).unwrap_or(60),
        rate: value("--rate").and_then(|s| s.parse().ok()).unwrap_or(600),
        coordinated: args.iter().any(|a| a == "--coordinated"),
        expect_verdict: value("--expect-verdict").filter(|v| matches!(v.as_str(), "pass" | "fail")).unwrap_or_else(|| "any".into()),
        instructor_code: env(E2E_INSTRUCTOR_CODE),
        group_code: env(E2E_GROUP_CODE),
        pack_name: env(E2E_PACK).and_then(|p| PathBuf::from(p).file_name().map(|n| n.to_string_lossy().into_owned())),
        stage: env(E2E_STAGE),
    })
}

#[tauri::command(rename_all = "snake_case")]
fn lab_snapshot(state: State<AppState>) -> UiBatch {
    state.lab.snapshot()
}

/// Everything new since the previous call (rows, logs, errors, status), or null.
#[tauri::command(rename_all = "snake_case")]
fn lab_drain(state: State<AppState>) -> Option<UiBatch> {
    state.lab.drain()
}

#[tauri::command(rename_all = "snake_case")]
fn lab_refresh(state: State<AppState>) {
    state.lab.command(Command::RefreshPorts);
}

#[tauri::command(rename_all = "snake_case")]
fn lab_connect(state: State<AppState>, ide: State<ide::IdeState>, port: Option<String>, serial: Option<String>) -> Result<(), String> {
    ide.with_link_available(|| state.lab.command(Command::Connect { port, serial }))
}

#[tauri::command(rename_all = "snake_case")]
fn lab_disconnect(state: State<AppState>, ide: State<ide::IdeState>) -> Result<(), String> {
    ide.with_link_available(|| state.lab.command(Command::Disconnect))
}

#[tauri::command(rename_all = "snake_case")]
fn lab_start(state: State<AppState>, ide: State<ide::IdeState>, tick_period_ms: u16) -> Result<(), String> {
    ide.with_link_available(|| state.lab.command(Command::StartSession { tick_period_ms }))
}

#[tauri::command(rename_all = "snake_case")]
fn lab_stop(state: State<AppState>) {
    state.lab.command(Command::StopSession);
}

#[tauri::command(rename_all = "snake_case")]
fn lab_set_stimuli(state: State<AppState>, stimuli: Stimuli, rev: u64) {
    state.lab.command(Command::SetStimuli { stimuli, rev });
}

#[tauri::command(rename_all = "snake_case")]
fn lab_send_one(state: State<AppState>) {
    state.lab.command(Command::SendOne);
}

#[tauri::command(rename_all = "snake_case")]
fn lab_reset_board(state: State<AppState>, ide: State<ide::IdeState>) -> Result<(), String> {
    ide.with_link_available(|| state.lab.command(Command::ResetBoard))
}

#[tauri::command(rename_all = "snake_case")]
fn e2e_config(state: State<AppState>) -> Option<E2eConfig> {
    state.e2e.clone()
}

/// Exercise the native close request, including the frontend's unsaved/busy guard.
/// `close` emits CloseRequested; unlike `destroy`, it can be prevented by the listener.
#[tauri::command(rename_all = "snake_case")]
fn e2e_request_close(app: tauri::AppHandle, state: State<AppState>) -> Result<(), String> {
    state.e2e.as_ref().ok_or("mode e2e inactif")?;
    app.get_webview_window("main").ok_or("Fenêtre de test indisponible.")?
        .close().map_err(|_| "Demande de fermeture de test impossible.".into())
}

fn e2e_path(state: &AppState, suffix: &str) -> Result<PathBuf, String> {
    let cfg = state.e2e.as_ref().ok_or("mode e2e inactif")?;
    let mut p = cfg.report.clone().into_os_string();
    p.push(suffix);
    Ok(PathBuf::from(p))
}

/// Publishes the current step for the external harness (`<report>.progress`).
#[tauri::command(rename_all = "snake_case")]
fn e2e_progress(state: State<AppState>, step: String) -> Result<(), String> {
    std::fs::write(e2e_path(&state, ".progress")?, step).map_err(|e| e.to_string())
}

/// True once the harness created `<report>.<step>.go`.
#[tauri::command(rename_all = "snake_case")]
fn e2e_released(state: State<AppState>, step: String) -> Result<bool, String> {
    Ok(e2e_path(&state, &format!(".{step}.go"))?.exists())
}

/// Displayed rows vs. the frames and answers that really crossed the link, plus the oracle
/// check on every answered frame since start-up.
#[tauri::command(rename_all = "snake_case")]
fn e2e_verify(state: State<AppState>, rows: Vec<DisplayedRow>) -> Verification {
    let mut v = verify_display(&state.lab.record(), &rows);
    let (_, checked, mismatches) = state.lab.oracle();
    v.oracle_checked = checked;
    v.oracle_mismatches = mismatches;
    v
}

/// student-apps: the group pack N7RT_E2E_PACK, base64 (e2e mode only).
#[tauri::command(rename_all = "snake_case")]
fn e2e_pack(state: State<AppState>) -> Result<String, String> {
    state.e2e.as_ref().ok_or("mode e2e inactif")?;
    let path = std::env::var_os(E2E_PACK).ok_or("N7RT_E2E_PACK non défini")?;
    std::fs::read(path).map(|b| base64_encode(&b)).map_err(|_| "pack de groupe N7RT_E2E_PACK illisible".into())
}

/// progression-<id>: the answers file N7RT_E2E_ANSWERS of the test operator (e2e mode only; never embedded, never
/// reported): the fiche and the repairs the scenario plays, produced from the test pack by the private tooling.
#[tauri::command(rename_all = "snake_case")]
fn e2e_answers(state: State<AppState>) -> Result<serde_json::Value, String> {
    state.e2e.as_ref().ok_or("mode e2e inactif")?;
    const ERROR: &str = "réponses de test N7RT_E2E_ANSWERS illisibles ou hors limite";
    let path = std::env::var_os(E2E_ANSWERS).filter(|p| !p.is_empty()).ok_or("N7RT_E2E_ANSWERS non défini")?;
    let mut bytes = Vec::new();
    std::fs::File::open(path).map_err(|_| ERROR)?.take(E2E_ANSWERS_MAX_BYTES + 1).read_to_end(&mut bytes).map_err(|_| ERROR)?;
    if bytes.len() as u64 > E2E_ANSWERS_MAX_BYTES {
        return Err(ERROR.into());
    }
    serde_json::from_slice(&bytes).map_err(|_| ERROR.into())
}

#[derive(Serialize)]
struct E2eStudentSource {
    c: String,
    h: Option<String>,
}

fn read_e2e_student_source(path: PathBuf) -> Result<String, String> {
    const MAX_SOURCE_BYTES: usize = 256 * 1024;
    const ERROR: &str = "Source étudiante de test illisible ou hors limite.";
    let mut bytes = Vec::new();
    std::fs::File::open(path).map_err(|_| ERROR)?.take((MAX_SOURCE_BYTES + 1) as u64)
        .read_to_end(&mut bytes).map_err(|_| ERROR)?;
    if bytes.len() > MAX_SOURCE_BYTES || bytes.contains(&0) { return Err(ERROR.into()); }
    String::from_utf8(bytes).map_err(|_| ERROR.into())
}

/// Test-only input for the actual editor controls. Source text is never added to the report.
#[tauri::command(rename_all = "snake_case")]
fn e2e_student_source(state: State<AppState>) -> Result<E2eStudentSource, String> {
    state.e2e.as_ref().ok_or("mode e2e inactif")?;
    let c = std::env::var_os("N7RT_E2E_DECODER_C").filter(|p| !p.is_empty())
        .ok_or("Source étudiante de test absente.")?;
    let h = std::env::var_os("N7RT_E2E_DECODER_H").filter(|p| !p.is_empty())
        .map(|p| read_e2e_student_source(PathBuf::from(p))).transpose()?;
    Ok(E2eStudentSource { c: read_e2e_student_source(PathBuf::from(c))?, h })
}

/// student-apps: full path of the run file just written (e2e mode only; the UI shows its name).
#[tauri::command(rename_all = "snake_case")]
fn e2e_run_file(state: State<AppState>, missions: State<mission::MissionState>) -> Result<Option<String>, String> {
    state.e2e.as_ref().ok_or("mode e2e inactif")?;
    Ok(missions.0.last_run_path().map(|p| p.to_string_lossy().into_owned()))
}

#[tauri::command(rename_all = "snake_case")]
fn e2e_finish(app: tauri::AppHandle, state: State<AppState>, report: serde_json::Value, ok: bool) -> Result<(), String> {
    let cfg = state.e2e.as_ref().ok_or("mode e2e inactif")?;
    let mut report = report;
    if let Some(c) = report.get_mut("config").and_then(|c| c.as_object_mut()) {
        for k in E2E_SECRETS {
            c.remove(k);
        }
    }
    if let Some(obj) = report.as_object_mut() {
        let (frames, _, _) = state.lab.oracle();
        obj.insert("record_frames".into(), frames.into());
        obj.insert("record_logs".into(), state.lab.log_record().len().into());
        obj.insert("final".into(), serde_json::to_value(state.lab.snapshot()).unwrap_or_default());
    }
    let text = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
    std::fs::write(&cfg.report, text).map_err(|e| e.to_string())?;
    let app2 = app.clone();
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(200));
        app2.exit(if ok { 0 } else { 1 });
    });
    Ok(())
}

pub fn run() {
    let e2e = parse_args();
    let cfg = LabConfig {
        // In e2e mode the scenario itself selects the board and clicks "Connecter".
        auto_connect: e2e.is_none(),
        serial: None,
        record: e2e.as_ref().is_some_and(|c| !phys::board_free(&c.scenario)),
        ..LabConfig::default()
    };
    let lab = Arc::new(Lab::start(cfg, Box::new(SerialConnector)));
    let automated = e2e.is_some();
    let lab_for_missions = lab.clone();
    tauri::Builder::default()
        .manage(AppState { lab: lab.clone(), e2e })
        .setup(move |app| {
            // Circuits are saved under the application data directory (circuits/<name>.json).
            let data_dir = app.path().app_data_dir().ok();
            app.manage(ide::IdeState::start(data_dir.clone()));
            let phys = phys::PhysState::start(data_dir.clone());
            // Student state under the application data directory, run files under the user's documents.
            let cfg = MissionConfig {
                app_version: app.package_info().version.to_string(),
                ..MissionConfig::discover(data_dir, app.path().document_dir().ok())
            };
            let missions = Missions::start(lab_for_missions.clone(), phys.handle(), cfg);
            app.manage(phys);
            app.manage(mission::MissionState(missions));
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.set_title("N7RT CAN Lab");
                if automated {
                    // A covered window gets no animation frames: keep it visible so that the
                    // measured display latency is the application's, not the desktop's.
                    let _ = w.set_always_on_top(true);
                    let _ = w.set_focus();
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            lab_snapshot,
            lab_drain,
            lab_refresh,
            lab_connect,
            lab_disconnect,
            lab_start,
            lab_stop,
            lab_set_stimuli,
            lab_send_one,
            lab_reset_board,
            ide::ide_state,
            ide::ide_read,
            ide::ide_save,
            ide::ide_restore,
            ide::ide_build,
            ide::ide_flash,
            e2e_config,
            e2e_request_close,
            e2e_progress,
            e2e_released,
            e2e_verify,
            e2e_finish,
            e2e_pack,
            e2e_answers,
            e2e_student_source,
            e2e_run_file,
            phys::phys_library,
            phys::phys_snapshot,
            phys::phys_preview,
            phys::phys_command,
            phys::phys_values,
            phys::phys_journal,
            phys::phys_new,
            phys::phys_save,
            phys::phys_load,
            phys::phys_list,
            phys::phys_demos,
            phys::phys_load_demo,
            mission::mission_list,
            mission::mission_open,
            mission::mission_close,
            mission::mission_reset,
            mission::mission_state,
            mission::mission_check,
            mission::mission_fiche_get,
            mission::mission_fiche_set,
            mission::mission_incident_get,
            mission::mission_incident_set,
            mission::mission_consign,
            mission::mission_reproduce,
            mission::mission_fiche_open,
            mission::mission_verify,
            mission::library_open,
            mission::access_state,
            mission::student_state,
            mission::instructor_login,
            mission::instructor_logout,
            mission::instructor_reset,
            mission::instructor_gen_packs,
            mission::instructor_judge,
            mission::student_choose,
            mission::student_import_pack,
            mission::student_open,
            mission::runs_open_folder,
        ])
        .build(tauri::generate_context!())
        .expect("N7RT CAN Lab: initialisation de Tauri impossible")
        .run(move |app, event| {
            if let tauri::RunEvent::Exit = event {
                // A running nominal check ends as « application fermée ».
                if let Some(m) = app.try_state::<mission::MissionState>() {
                    m.0.shutdown();
                }
                // SESSION_STOP if a session is open, then close the port and stop the threads.
                lab.shutdown();
                if let Some(phys) = app.try_state::<phys::PhysState>() {
                    phys.shutdown();
                }
            }
        });
}
