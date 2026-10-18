//! Tauri commands of the missions (docs/apps-mission-slice.md §6) and of the student and instructor modes
//! (docs/distribution.md §2): thin async wrappers around `n7rt_lab::mission::Missions`. Errors are French sentences
//! for the UI; none carries a code, a key, a mission value or a local path. Files chosen in the UI (group pack, run
//! file) arrive as base64 text, refused over their bound before decoding; nothing here logs a code.

use n7rt_lab::access::{decode_upload, MAX_PACK_BYTES, MAX_RUN_BYTES, MSG_PACK_TOO_LARGE, MSG_RUN_TOO_LARGE};
use n7rt_lab::cohort::{GenRow, Judgement};
use n7rt_lab::mission::{AccessView, MissionEntry, MissionView, Missions, StudentView};
use n7rt_lab::workflow::fiche::NominalPart;
use n7rt_lab::workflow::incident::IncidentPart;
use tauri::State;

/// The mission controller managed by Tauri.
pub struct MissionState(pub Missions);

#[tauri::command(rename_all = "snake_case")]
pub async fn mission_list(state: State<'_, MissionState>) -> Result<Vec<MissionEntry>, String> {
    Ok(state.0.list())
}

/// Instructor: opens a mission with its reference pack. Student: refused (the locked mission opens with the group
/// code, `student_open`).
#[tauri::command(rename_all = "snake_case")]
pub async fn mission_open(state: State<'_, MissionState>, ide: State<'_, crate::ide::IdeState>, id: String) -> Result<MissionView, String> {
    ide.with_link_available(|| state.0.open(&id))?
}

#[tauri::command(rename_all = "snake_case")]
pub async fn mission_close(state: State<'_, MissionState>, ide: State<'_, crate::ide::IdeState>) -> Result<MissionView, String> {
    ide.with_link_available(|| state.0.close())?
}

#[tauri::command(rename_all = "snake_case")]
pub async fn mission_reset(state: State<'_, MissionState>, ide: State<'_, crate::ide::IdeState>) -> Result<MissionView, String> {
    ide.with_link_available(|| state.0.reset())?
}

#[tauri::command(rename_all = "snake_case")]
pub async fn mission_state(state: State<'_, MissionState>) -> Result<MissionView, String> {
    Ok(state.0.view())
}

/// The nominal check: a verdict with a reference pack (instructor), a run file with a group pack (student).
#[tauri::command(rename_all = "snake_case")]
pub async fn mission_check(state: State<'_, MissionState>, ide: State<'_, crate::ide::IdeState>) -> Result<MissionView, String> {
    ide.with_link_available(|| state.0.check())?
}

/// Release 0.3 progression: the nominal part of the fiche of the open group pack.
#[tauri::command(rename_all = "snake_case")]
pub async fn mission_fiche_get(state: State<'_, MissionState>) -> Result<NominalPart, String> {
    state.0.fiche_get()
}

/// Release 0.3 progression: records the nominal part of the fiche (shape checks only).
#[tauri::command(rename_all = "snake_case")]
pub async fn mission_fiche_set(state: State<'_, MissionState>, part: NominalPart) -> Result<MissionView, String> {
    state.0.fiche_set(part)
}

/// Release 0.3 progression: the incident part `rank` (1–3) of the fiche, once its incident is active.
#[tauri::command(rename_all = "snake_case")]
pub async fn mission_incident_get(state: State<'_, MissionState>, rank: usize) -> Result<Option<IncidentPart>, String> {
    state.0.incident_get(rank)
}

/// Release 0.3 progression: records an incident part of the fiche (shape checks only).
#[tauri::command(rename_all = "snake_case")]
pub async fn mission_incident_set(state: State<'_, MissionState>, part: IncidentPart) -> Result<MissionView, String> {
    state.0.incident_set(part)
}

/// Release 0.3 progression: « Consigner la mesure » of the incident in progress.
#[tauri::command(rename_all = "snake_case")]
pub async fn mission_consign(state: State<'_, MissionState>) -> Result<MissionView, String> {
    state.0.consign()
}

/// Release 0.3 progression: « Reproduire l'anomalie » of the incident in progress (R-11).
#[tauri::command(rename_all = "snake_case")]
pub async fn mission_reproduce(state: State<'_, MissionState>) -> Result<MissionView, String> {
    state.0.reproduce()
}

/// Release 0.3 progression: opens the final fiche of the closed mission.
#[tauri::command(rename_all = "snake_case")]
pub async fn mission_fiche_open(state: State<'_, MissionState>) -> Result<MissionView, String> {
    state.0.fiche_open()
}

/// Release 0.3 progression: verification of the current step (hidden test replayed to the board, fiche checked).
#[tauri::command(rename_all = "snake_case")]
pub async fn mission_verify(state: State<'_, MissionState>, ide: State<'_, crate::ide::IdeState>) -> Result<MissionView, String> {
    ide.with_link_available(|| state.0.verify())?
}

/// Opens the `index`-th document of the open mission's library (local copy with the default viewer, or the
/// publisher's address in the default browser).
#[tauri::command(rename_all = "snake_case")]
pub async fn library_open(state: State<'_, MissionState>, index: usize) -> Result<(), String> {
    state.0.library_open(index)
}

// ------------------------------------------------------------------------------------------------------- modes

/// Mode of the application and the student's session (locked mission, imported pack, last run file).
#[tauri::command(rename_all = "snake_case")]
pub async fn access_state(state: State<'_, MissionState>) -> Result<AccessView, String> {
    Ok(state.0.access())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn student_state(state: State<'_, MissionState>) -> Result<StudentView, String> {
    Ok(state.0.student_state())
}

/// « Code encadrant incorrect. » or « Ressources encadrant introuvables sur ce poste. » after 1 s on failure.
#[tauri::command(rename_all = "snake_case")]
pub async fn instructor_login(state: State<'_, MissionState>, code: String) -> Result<AccessView, String> {
    state.0.instructor_login(&code)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn instructor_logout(state: State<'_, MissionState>, ide: State<'_, crate::ide::IdeState>) -> Result<AccessView, String> {
    ide.with_link_available(|| state.0.instructor_logout())?
}

#[tauri::command(rename_all = "snake_case")]
pub async fn instructor_reset(state: State<'_, MissionState>, ide: State<'_, crate::ide::IdeState>) -> Result<AccessView, String> {
    ide.with_link_available(|| state.0.instructor_reset())?
}

/// `replace`: regenerate the groups already generated (new code, new pack).
#[tauri::command(rename_all = "snake_case")]
pub async fn instructor_gen_packs(
    state: State<'_, MissionState>,
    mission: String,
    cohort: String,
    groups: Vec<String>,
    replace: bool,
) -> Result<Vec<GenRow>, String> {
    state.0.instructor_gen_packs(&mission, &cohort, &groups, replace)
}

/// `name`: the file name chosen by the instructor; `data`: the run file (`.n7rtrun`, 8 MiB at most), base64.
#[tauri::command(rename_all = "snake_case")]
pub async fn instructor_judge(state: State<'_, MissionState>, name: String, data: String) -> Result<Judgement, String> {
    state.0.instructor_judge(&name, &decode_upload(&data, MAX_RUN_BYTES, MSG_RUN_TOO_LARGE)?)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn student_choose(state: State<'_, MissionState>, id: String) -> Result<AccessView, String> {
    state.0.student_choose(&id)
}

/// `data`: the group pack (`.n7rtpack`, 1 MiB at most), base64; stored as it is.
#[tauri::command(rename_all = "snake_case")]
pub async fn student_import_pack(state: State<'_, MissionState>, data: String) -> Result<AccessView, String> {
    state.0.student_import_pack(&decode_upload(&data, MAX_PACK_BYTES, MSG_PACK_TOO_LARGE)?)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn student_open(state: State<'_, MissionState>, ide: State<'_, crate::ide::IdeState>, code: String) -> Result<MissionView, String> {
    ide.with_link_available(|| state.0.student_open(&code))?
}

/// Opens `Documents\N7RT CAN Lab\essais` in the file explorer.
#[tauri::command(rename_all = "snake_case")]
pub async fn runs_open_folder(state: State<'_, MissionState>) -> Result<(), String> {
    state.0.open_runs_folder()
}
