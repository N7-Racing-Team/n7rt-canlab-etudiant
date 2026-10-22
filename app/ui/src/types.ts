// Mirror of crates/n7rt-lab/src/model.rs (serde JSON). Keep both in step.

export type LinkState =
  | "DISCONNECTED" | "ABSENT" | "PORT_BUSY" | "HANDSHAKE" | "READY"
  | "NO_STUDENT" | "STUDENT_DISABLED" | "INCOMPATIBLE" | "SESSION" | "LOST";

export type FrameFormat = "classic" | "extended" | "fd" | "fd_brs" | "remote";

export interface Stimuli {
  pot: number;
  button: boolean;
  format: FrameFormat;
  can_id: number;
  dlc: number;
  rate_hz: number;
}

export interface PortInfo {
  port: string;
  vid: number | null;
  pid: number | null;
  serial: string | null;
  product: string | null;
  probe: string | null;
}

export interface BoardIdentity {
  /** "NUCLEO-H753ZI", "NUCLEO-G0B1RE", or "" for an unsupported microcontroller. */
  board: string;
  /** "h753", "g0b1", or "". */
  family: string;
  runtime: string;
  build_id: string;
  abi: string;
  proto: number;
  uid: string;
  dev_id: number;
  rev_id: number;
  reset_flags: number;
  reset_causes: string[];
  student_status: number;
  student_status_text: string;
  student_abi: string;
  student_crc32: string;
  rx_window: number;
  fault_pending: boolean;
  runtime_verified: boolean;
  compatible: boolean;
  problems: string[];
}

export interface SessionView {
  id: number;
  phase: "starting" | "active" | "stopping" | "closing";
  mode: number;
  tick_period_ms: number;
  elapsed_ms: number;
  bus_time_us: number;
}

export interface HeartbeatView {
  uptime_ms: number;
  acked_frame_seq: number;
  hook_running_ms: number;
  state: number;
  student_disabled: boolean;
  rx_frames: number;
  rx_seq_gaps: number;
  rx_framing_errors: number;
  rx_crc_errors: number;
  rx_overruns: number;
  log_dropped: number;
  queue_high_water: number;
  max_cycles: number;
}

export interface Status {
  state: LinkState;
  stalled: boolean;
  student_busy: boolean;
  message: string;
  target: string | null;
  port: PortInfo | null;
  ports: PortInfo[];
  board: BoardIdentity | null;
  session: SessionView | null;
  heartbeat: HeartbeatView | null;
  fault: string | null;
  stimuli: Stimuli;
  stim_rev: number;
  can_start: boolean;
  can_stop: boolean;
}

export interface QueueStats {
  sim_depth: number;
  sim_high_water: number;
  sim_capacity: number;
  sim_paused_ms: number;
  sim_dropped: number;
  sim_generated: number;
  sim_max_step_us: number;
  ui_rows_capacity: number;
  ui_rows_not_shown: number;
  ui_logs_dropped: number;
  ui_errors_dropped: number;
  link_loop_max_us: number;
  write_max_us: number;
  read_chunk_max: number;
  tx_backlog_max: number;
}

export interface Stats {
  frames_sent: number;
  results: number;
  nacks: number;
  nack_codes: Record<string, number>;
  lost: number;
  late: number;
  aborted: number;
  in_flight: number;
  in_flight_max: number;
  window: number;
  credit_waits: number;
  tx_fps: number;
  rx_fps: number;
  rtt_count: number;
  rtt_p50_us: number;
  rtt_p95_us: number;
  rtt_p99_us: number;
  rtt_max_us: number;
  rtt_mean_us: number;
  sim_time_sent: number;
  tick_results: number;
  logs: number;
  rx_framing: Record<string, number>;
  rx_rejects: number;
  board_seq_gaps: number;
  pre_sync_discarded: number;
  bytes_tx: number;
  bytes_rx: number;
  queues: QueueStats;
}

export interface EntryView {
  slot: number;
  kind: number;
  bits: number;
}

export interface FrameRow {
  session_id: number;
  frame_seq: number;
  t_us: number;
  can_id: number;
  flags: number;
  dlc: number;
  data: string;
  outcome: "result" | "nack" | "lost" | "aborted";
  status: number;
  result_flags: number;
  cycles: number;
  entries: EntryView[];
  nack: string | null;
  rtt_us: number | null;
  stim_rev: number;
}

export interface SlotView {
  slot: number;
  kind: number;
  bits: number;
  source: "frame" | "tick";
  at: number;
  updates: number;
}

export interface LogRow {
  session_id: number;
  t_us: number;
  text: string;
}

export interface ErrorRow {
  at_ms: number;
  kind: string;
  text: string;
}

export interface UiBatch {
  rev: number;
  status: Status;
  stats: Stats;
  rows: FrameRow[];
  logs: LogRow[];
  errors: ErrorRow[];
  slots: SlotView[];
}

export interface E2eConfig {
  scenario: string;
  serial: string | null;
  report: string;
  seconds: number;
  rate: number;
  coordinated: boolean;
  /** mission-apps: verdict the nominal check must give. */
  expect_verdict: "pass" | "fail" | "any";
  /** Mission scenarios: N7RT_E2E_INSTRUCTOR_CODE (never written into the report). */
  instructor_code: string | null;
  /** student-apps: N7RT_E2E_GROUP_CODE (never written into the report). */
  group_code: string | null;
  /** student-apps: file name of N7RT_E2E_PACK. */
  pack_name: string | null;
  /** progression-<id>: « full », « until-incident » or « resume ». */
  stage?: string | null;
}

export interface DisplayedRow {
  session: string;
  seq: string;
  id: string;
  kind: string;
  dlc: string;
  data: string;
  result: string;
  publications: string;
}

export interface Verification {
  displayed: number;
  matched: number;
  mismatches: string[];
  oracle_checked: number;
  oracle_mismatches: string[];
}

// Missions: mirror of crates/n7rt-lab/src/mission.rs (serde JSON). Keep both in step.

/** An entry of the mission selector. */
export interface MissionEntry {
  id: string;
  title: string;
  available: boolean;
  /** Why the mission cannot be opened, when unavailable. */
  reason: string | null;
}

/** A public slot of the statement (ch. 10 §7.2): `slot` is the index the student code publishes. */
export interface SlotLabel {
  slot: number;
  code: string;
  label: string;
  unit: string;
}

export interface StatementView {
  summary: string;
  requirements: string[];
  slots: SlotLabel[];
}

/** Bibliographic fields only: no role, no file, no address. */
export interface LibraryItem {
  title: string;
  publisher: string;
  reference: string;
  revision: string;
  date: string | null;
}

export interface CheckView {
  /** "done": instructor verdict; "recorded": student run file. */
  state: "idle" | "running" | "done" | "recorded" | "cancelled";
  elapsed_s: number;
  total_s: number;
  verdict: "pass" | "fail" | null;
  /** Why a check was cancelled: « carte déconnectée », « application fermée », « essai incomplet ». */
  reason: string | null;
  /** Name of the run file once recorded (student). */
  run_file: string | null;
}

export interface MissionView {
  id: string | null;
  title: string | null;
  open: boolean;
  /** "reference" (instructor) | "group" (student pack), when open. */
  kind: "reference" | "group" | null;
  /** Group of the open student pack. */
  group: string | null;
  statement: StatementView | null;
  /** Sorted by title; library_open(index) opens an entry. */
  library: LibraryItem[];
  check: CheckView;
  tick_period_ms: number | null;
  /** Release 0.3 group pack: the student progression; null for a 0.2 pack or the reference pack. */
  progression: ProgressionView | null;
}

// Student progression (release 0.3): mirror of crates/n7rt-lab/src/workflow/session.rs and fiche.rs.

export interface StepView {
  name: string;
  /** « À FAIRE », « EN COURS », « VALIDÉ », « ÉCHEC À CORRIGER ». */
  state: string;
  /** « le JJ/MM/AAAA à HH:MM » once validated. */
  since: string | null;
  current: boolean;
}

export interface ProgressionView {
  readable: boolean;
  steps: StepView[];
  current: number | null;
  /** Work order of the current step (exact text). */
  order: string;
  button: string;
  ready: boolean;
  /** An attempt is running (the fiche is read-only for it). */
  running: boolean;
  /** Why the verification cannot start (exact message), or the running attempt. */
  blocked: string | null;
  /** Lines of the last verdict. */
  feedback: string[];
  nominal_editable: boolean;
  fiche_state: string;
  /** Choices of « Documents utilisés »: the mission library, same order. */
  documents: { id: string; title: string }[];
  /** The incident part of the current step E1–E3. */
  incident: IncidentView | null;
}

export interface Choice {
  value: string;
  definition: string;
}

/** One line of the public key-measure rule: the quantity for a component target, for the other targets, the units. */
export interface RuleView {
  state: string;
  component: string;
  other: string;
  units: string[];
}

export interface IncidentView {
  rank: number;
  editable: boolean;
  /** The quantity imposed by the saved observed state and target type. */
  grandeur: string | null;
  units: string[];
  hint: string;
  /** « le JJ/MM/AAAA à HH:MM » once the measure is recorded. */
  consigned: string | null;
  categories: Choice[];
  states: Choice[];
  rules: RuleView[];
  /** What can be designated on the live bench: designators, and pins as « désignateur.broche ». */
  parts: string[];
  pins: string[];
}

export type TargetKind = "CONDUCTEUR" | "COMPOSANT" | "IDENTIFIANT";

export interface Cible {
  type: TargetKind | null;
  conducteur: { a: string | null; b: string | null };
  composant: string | null;
  identifiant: string | null;
}

/** Written by the application at « Consigner la mesure » (the interface never writes it). */
export interface Consigned {
  consignee_s: number;
  grandeur: string;
  conditions: unknown;
  preuve_sha256: string;
}

export interface IncidentPart {
  rang: number;
  cible: Cible;
  categorie: string | null;
  etat_observe: string | null;
  mesure_cle: { valeur: number | null; unite: string | null; consignation: Consigned | null };
  justification: string;
}

export type BitOrder = "INTEL" | "MOTOROLA";
export type Sign = "NON_SIGNÉ" | "COMPLÉMENT_À_DEUX";
export type Decision =
  | "REJETER" | "PUBLIER" | "BORNER" | "IGNORER_SES_DONNÉES" | "UTILISER_SES_DONNÉES" | "PUBLIER_LA_DERNIÈRE";

export interface BitField {
  ordre: BitOrder | null;
  lsb_bit: number | null;
  longueur: number | null;
}

export interface FrameEntry {
  identifiant: number | null;
  format: string | null;
  dlc: number | null;
  periode_ms: number | null;
  protection: { presente: boolean | null; crc: BitField; compteur: BitField };
  compteur_de_vie: BitField;
}

export interface SignalEntry {
  nom: string;
  identifiant: number | null;
  slots: number[];
  ordre: BitOrder | null;
  lsb_bit: number | null;
  longueur: number | null;
  signe: Sign | null;
  facteur: number | null;
  offset: number | null;
  code_invalide: number | "AUCUN" | null;
}

export interface StatusEntry {
  identifiant: number | null;
  slots: number[];
  ordre: BitOrder | null;
  lsb_bit: number | null;
  longueur: number | null;
  code_ok: number | null;
}

export interface Supervision {
  premier_timeout_ms: number | null;
  timeout_ms: number | null;
  peremption_ms: number | null;
  pertes_consecutives_tolerees: number | null;
}

/** Nominal part of the fiche (public schema of the private spec ch. 24 §3.3). */
export interface NominalPart {
  documents_utilises: { document: string | null; pages: string }[];
  trames: FrameEntry[];
  signaux: SignalEntry[];
  statuts: StatusEntry[];
  supervision: Supervision;
  /** DD-1…DD-4 in order. */
  decisions: (Decision | null)[];
  justification: string;
}

// Modes (docs/distribution.md §2): mirror of crates/n7rt-lab/src/mission.rs and cohort.rs.

export type Mode = "student" | "instructor";

/** The imported group pack (clear header fields). */
export interface PackInfo {
  cohort: string;
  group: string;
}

export interface StudentView {
  /** Mission of the session, once chosen (locked). */
  locked: string | null;
  locked_title: string | null;
  pack: PackInfo | null;
  /** Name of the last run file written. */
  last_run: string | null;
}

export interface AccessView {
  mode: Mode;
  student: StudentView;
  /** « édition publique » or « édition encadrant […] » (docs/distribution.md §1). */
  edition: string;
  /** The build contains the oracle: instructor checks and judgements give a verdict. */
  oracle: boolean;
}

/** A generated group pack (instructor). */
export interface GenRow {
  group: string;
  code: string;
  /** Relative to the instructor directory. */
  file: string;
}

/** Verdict of a run file (instructor only: the detail is private). */
export interface Judgement {
  mission: string;
  cohort: string;
  group: string;
  started_utc: string;
  verdict: "pass" | "fail";
  detail: string;
  judged_file: string | null;
}
