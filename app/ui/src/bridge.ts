// Typed access to the Rust core. Every command returns immediately: the core only queues it.

import { invoke } from "@tauri-apps/api/core";
import type {
  AccessView, DisplayedRow, E2eConfig, GenRow, IncidentPart, Judgement, MissionEntry, MissionView, NominalPart, Stimuli, UiBatch,
  Verification,
} from "./types";

export const lab = {
  snapshot: () => invoke<UiBatch>("lab_snapshot"),
  /** What changed since the previous call, or null. */
  drain: () => invoke<UiBatch | null>("lab_drain"),
  refresh: () => invoke<void>("lab_refresh"),
  connect: (port: string | null, serial: string | null) => invoke<void>("lab_connect", { port, serial }),
  disconnect: () => invoke<void>("lab_disconnect"),
  start: (tick_period_ms: number) => invoke<void>("lab_start", { tick_period_ms }),
  stop: () => invoke<void>("lab_stop"),
  setStimuli: (stimuli: Stimuli, rev: number) => invoke<void>("lab_set_stimuli", { stimuli, rev }),
  sendOne: () => invoke<void>("lab_send_one"),
  resetBoard: () => invoke<void>("lab_reset_board"),
};

/** Missions (docs/apps-mission-slice.md §6). Errors are French sentences, shown as they are. */
export const mission = {
  list: () => invoke<MissionEntry[]>("mission_list"),
  open: (id: string) => invoke<MissionView>("mission_open", { id }),
  close: () => invoke<MissionView>("mission_close"),
  reset: () => invoke<MissionView>("mission_reset"),
  state: () => invoke<MissionView>("mission_state"),
  check: () => invoke<MissionView>("mission_check"),
  ficheGet: () => invoke<NominalPart>("mission_fiche_get"),
  ficheSet: (part: NominalPart) => invoke<MissionView>("mission_fiche_set", { part }),
  incidentGet: (rank: number) => invoke<IncidentPart | null>("mission_incident_get", { rank }),
  incidentSet: (part: IncidentPart) => invoke<MissionView>("mission_incident_set", { part }),
  consign: () => invoke<MissionView>("mission_consign"),
  reproduce: () => invoke<MissionView>("mission_reproduce"),
  ficheOpen: () => invoke<MissionView>("mission_fiche_open"),
  verify: () => invoke<MissionView>("mission_verify"),
  /** Opens a document of the open mission's library (default viewer or browser). */
  openDocument: (index: number) => invoke<void>("library_open", { index }),
  /** Opens the folder of the run files (Documents\N7RT CAN Lab\essais). */
  openRunsFolder: () => invoke<void>("runs_open_folder"),
};

/** Student and instructor modes (docs/distribution.md §2). Files travel as base64; codes are never kept. */
export const access = {
  state: () => invoke<AccessView>("access_state"),
  login: (code: string) => invoke<AccessView>("instructor_login", { code }),
  logout: () => invoke<AccessView>("instructor_logout"),
  reset: () => invoke<AccessView>("instructor_reset"),
  genPacks: (mission: string, cohort: string, groups: string[], replace: boolean) =>
    invoke<GenRow[]>("instructor_gen_packs", { mission, cohort, groups, replace }),
  judge: (name: string, data: string) => invoke<Judgement>("instructor_judge", { name, data }),
  choose: (id: string) => invoke<AccessView>("student_choose", { id }),
  importPack: (data: string) => invoke<AccessView>("student_import_pack", { data }),
  open: (code: string) => invoke<MissionView>("student_open", { code }),
};

export const e2e = {
  config: () => invoke<E2eConfig | null>("e2e_config"),
  /** E2E only: the native request follows the same close event as the window button. */
  requestClose: () => invoke<void>("e2e_request_close"),
  progress: (step: string) => invoke<void>("e2e_progress", { step }),
  released: (step: string) => invoke<boolean>("e2e_released", { step }),
  verify: (rows: DisplayedRow[]) => invoke<Verification>("e2e_verify", { rows }),
  finish: (report: unknown, ok: boolean) => invoke<void>("e2e_finish", { report, ok }),
  /** student-apps: the group pack N7RT_E2E_PACK, base64. */
  pack: () => invoke<string>("e2e_pack"),
  /** student-apps: full path of the run file just written. */
  runFile: () => invoke<string | null>("e2e_run_file"),
  /** Integration test only: sources supplied by the test operator, never embedded or reported. */
  studentSource: () => invoke<{ c: string; h: string | null }>("e2e_student_source"),
  /** progression-<id>: the test operator's answers file (fiche and repairs played by the scenario), never reported. */
  answers: () => invoke<unknown>("e2e_answers"),
};
