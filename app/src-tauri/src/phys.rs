//! Tauri commands of the Physical Lab (docs/physical-lab-v1.md §8.3): thin async wrappers around
//! `n7rt_lab::phys::PhysLab`, independent of the N7RT-Link session of the cockpit.
//!
//! JSON arguments are camelCase on the JS side (`baseRev`, `sinceSeq`), snake_case here (Tauri's default
//! mapping). Rejections are a `PhysError` (`refused`, `file`, `engine_unavailable`). Nothing is emitted: the UI
//! pulls values with `phys_values`, as the cockpit pulls with `lab_drain`.

use std::path::PathBuf;
use std::sync::Arc;

use n7rt_lab::phys::{
    CommandReply, DemoEntry, FileEntry, JournalEntry, PhysCommand, PhysConfig, PhysError, PhysLab, PreviewReply, SnapshotDto,
    ValuesDto,
};
use serde::Serialize;
use tauri::State;

/// The engine handle managed by Tauri.
pub struct PhysState(Arc<PhysLab>);

impl PhysState {
    /// Starts the `phys` thread; circuits are stored under `data_dir` (the application data directory).
    pub fn start(data_dir: Option<PathBuf>) -> PhysState {
        PhysState(Arc::new(PhysLab::start(PhysConfig { data_dir })))
    }

    pub fn shutdown(&self) {
        self.0.shutdown();
    }

    /// The engine handle, shared with the mission controller (it loads the mission bench and reads the probe).
    pub fn handle(&self) -> Arc<PhysLab> {
        self.0.clone()
    }

    /// Every phys_* command answers `engine_unavailable` once the core panicked more than 3 times (contract
    /// §8.1). The handle's infallible calls (snapshot, values, journal, new) cannot say it, so they are guarded
    /// here: a preview at an impossible revision is refused as stale by a live core, without any effect.
    fn alive(&self) -> Result<(), PhysError> {
        let probe = serde_json::json!({ "op": "delete", "components": [], "wires": [] });
        match serde_json::from_value(probe) {
            Ok(edit) => match self.0.preview(u64::MAX, &edit) {
                Err(PhysError::EngineUnavailable) => Err(PhysError::EngineUnavailable),
                _ => Ok(()),
            },
            Err(_) => Ok(()),
        }
    }
}

/// `--e2e phys` and `--e2e phys-perf` drive the Physical Lab only, `--e2e pack-<id>` opens a release 0.3 pack: no
/// board, no serial number, no link record.
pub fn board_free(scenario: &str) -> bool {
    matches!(scenario, "phys" | "phys-perf") || scenario.starts_with("pack-")
}

/// Rejection of `phys_preview`: the contract error, or a malformed edit (as Tauri reports a malformed
/// argument: a message; the UI maps it to engine_unavailable).
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum PreviewRejection {
    Phys(PhysError),
    Malformed(String),
}

#[tauri::command]
pub async fn phys_library(state: State<'_, PhysState>) -> Result<serde_json::Value, PhysError> {
    state.alive()?;
    // KindDef is a type of n7rt-circuit, which this crate does not name: sent as its JSON value.
    serde_json::to_value(state.0.library()).map_err(|_| PhysError::EngineUnavailable)
}

#[tauri::command]
pub async fn phys_snapshot(state: State<'_, PhysState>) -> Result<SnapshotDto, PhysError> {
    state.alive()?;
    Ok(state.0.snapshot())
}

#[tauri::command]
pub async fn phys_preview(state: State<'_, PhysState>, base_rev: u64, edit: serde_json::Value) -> Result<PreviewReply, PreviewRejection> {
    let edit = serde_json::from_value(edit).map_err(|e| PreviewRejection::Malformed(format!("édition illisible : {e}")))?;
    state.0.preview(base_rev, &edit).map_err(PreviewRejection::Phys)
}

#[tauri::command]
pub async fn phys_command(state: State<'_, PhysState>, command: PhysCommand) -> Result<CommandReply, PhysError> {
    state.0.command(command)
}

#[tauri::command]
pub async fn phys_values(state: State<'_, PhysState>, since_seq: u64) -> Result<Option<ValuesDto>, PhysError> {
    match state.0.values(since_seq) {
        Some(v) => Ok(Some(v)),
        None => state.alive().map(|()| None),
    }
}

#[tauri::command]
pub async fn phys_journal(state: State<'_, PhysState>, n: usize) -> Result<Vec<JournalEntry>, PhysError> {
    state.alive()?;
    Ok(state.0.journal_tail(n))
}

#[tauri::command]
pub async fn phys_new(state: State<'_, PhysState>) -> Result<SnapshotDto, PhysError> {
    state.alive()?;
    Ok(state.0.new_circuit())
}

#[tauri::command]
pub async fn phys_save(state: State<'_, PhysState>, name: String, overwrite: bool) -> Result<SnapshotDto, PhysError> {
    state.0.save(&name, overwrite)
}

#[tauri::command]
pub async fn phys_load(state: State<'_, PhysState>, name: String) -> Result<SnapshotDto, PhysError> {
    state.0.load(&name)
}

#[tauri::command]
pub async fn phys_list(state: State<'_, PhysState>) -> Result<Vec<FileEntry>, PhysError> {
    state.0.list()
}

#[tauri::command]
pub async fn phys_demos(state: State<'_, PhysState>) -> Result<Vec<DemoEntry>, PhysError> {
    state.alive()?;
    Ok(state.0.demos())
}

#[tauri::command]
pub async fn phys_load_demo(state: State<'_, PhysState>, id: String) -> Result<SnapshotDto, PhysError> {
    state.0.load_demo(&id)
}
