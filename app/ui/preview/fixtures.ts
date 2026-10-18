// Preview fixtures (dev only, see boot.ts): one named state of the cockpit and of the release 0.3 progression per
// entry of STATES, answered by the fake core as the Rust core would.
//
// Every value is invented for the preview: mission « FAN », group G07, identifiers 0x1A4 and 0x2B0, signals, bench
// designators, board serial. Nothing comes from a mission pack. The texts the core sends as they are (step states,
// work orders, verdict lines, refusals, public lists of the incident part, competence labels) are the public texts
// of crates/n7rt-lab/src/workflow/{session,incident,qualification}.rs, copied verbatim: the views show them unchanged.

import type {
  AccessView, BoardIdentity, CheckView, Choice, FrameRow, HeartbeatView, IncidentPart, IncidentView, LibraryItem, LogRow,
  MissionEntry, MissionView, NominalPart, PortInfo, ProgressionView, RuleView, SlotView, StatementView, Stats, Status, StepView,
  UiBatch,
} from "../src/types";
import type { DemoEntry, FileEntry, JournalEntry, KindDef, SnapshotDto, ValuesDto } from "../src/lab/types";
import type { Layout } from "../src/shell";
// Public DTO fixtures of the Physical Lab (written by crates/n7rt-lab/tests/phys_fixtures.rs).
import libraryJson from "../src/lab/__fixtures__/library.json?raw";
import snapshotJson from "../src/lab/__fixtures__/snapshot.json?raw";
import valuesJson from "../src/lab/__fixtures__/values_mission.json?raw";
import demosJson from "../src/lab/__fixtures__/demos.json?raw";
import filesJson from "../src/lab/__fixtures__/files.json?raw";
import journalJson from "../src/lab/__fixtures__/journal.json?raw";

/** One user action after the state is drawn (and after « Ouvrir la fiche »), as the e2e does it: `type` sets the value
 * then sends input and change, `change` sets the value then sends change, `click` presses. Selector, value. */
export interface Step {
  type?: [string, string];
  change?: [string, string];
  click?: string;
}

/** The core's answers of the Physical Lab (the mission bench, multimeter included). */
export interface PhysFixture {
  library: KindDef[];
  snapshot: SnapshotDto;
  values: ValuesDto;
  demos: DemoEntry[];
  files: FileEntry[];
  journal: JournalEntry[];
}

export interface Scenario {
  /** One line shown on the index page of the preview. */
  label: string;
  access: AccessView;
  list: MissionEntry[];
  mission: MissionView;
  fiche: NominalPart;
  incident: IncidentPart | null;
  batch: UiBatch;
  /** Selector of an element that is visible once the state is drawn. */
  ready: string;
  /** Presses « Ouvrir la fiche » once the state is drawn. */
  openFiche: boolean;
  /** Selector of the element brought to the top of its scrolling panel (null: panels left at the top). */
  scroll: string | null;
  /** Cockpit panel layout stored before the application starts (n7rt-can-lab.layout.v1); missing keys: defaults. */
  layout?: Partial<Layout>;
  /** User actions once the state is drawn, 100 ms apart. */
  after?: Step[];
  /** The view the application opens on (cockpit by default). */
  view?: "cockpit" | "phys";
  /** Physical Lab answers; without them its commands are not simulated. */
  phys?: PhysFixture;
}

// ------------------------------------------------------------------------------- public texts of the core

const STEP_NAMES = ["Essai nominal", "Incident 1", "Incident 2", "Incident 3", "Qualification finale"];
const ORDER_E0 =
  "Essai nominal. Identifiez les trames de votre ECU sur le bus, décodez-les, programmez la carte et documentez votre décodage dans la fiche. Lancez la vérification quand votre code et votre fiche sont prêts.";
const orderIncident = (k: number): string =>
  `Incident ${k}. Le véhicule revient d'essai avec une anomalie. Observez-la au banc, sur le bus et dans vos résultats ; consignez votre diagnostic dans la fiche ; réparez le véhicule ou votre code, ou les deux ; puis lancez « Vérifier la réparation ».`;
const ORDER_QUALIFICATION = "Qualification finale. Une séquence d'essais cachée évalue votre code. Le résultat est donné par compétence.";
const ORDER_CLOSED = "Mission clôturée. Votre fiche finale est disponible.";
const DETAIL_HIDDEN = "Le détail n'est pas communiqué. Reprenez vos mesures et vos essais.";
const EXPORTED = "La fiche finale est exportée dans le dossier Documents.";
const NOT_EXPORTED = "La fiche finale n'a pas pu être exportée : utilisez « Ouvrir la fiche finale » pour réessayer.";
/** The reason is invented for the preview; the format is the core's. */
const interrupted = (reason: string): string =>
  `Vérification interrompue (${reason}). Elle n'est pas comptée. Relancez quand le banc est prêt.`;
const BUTTON = { nominal: "Lancer la vérification", incident: "Vérifier la réparation", qualification: "Lancer la qualification" };
const FICHE_DRAFT = "En cours de rédaction";
const interval = (s: number): string => `Prochaine vérification possible dans ${Math.floor(s / 60)} min ${s % 60} s.`;
const attempt = (left: number): string => `Essai en cours — ${left} s. Ne débranchez pas la carte et ne fermez pas l'application.`;
const qualifying = (left: number): string =>
  `Qualification en cours — ${left} s. Ne débranchez pas la carte et ne fermez pas l'application.`;
const incomplete = (fields: string[]): string =>
  `Complétez la fiche de cette étape avant la vérification. Champs à renseigner : ${fields.join(", ")}.`;
const passed = (since: string): string => `Vérification réussie. Étape validée ${since}.`;
const CP_LABELS = [
  "Recevoir et filtrer les trames de l'ECU",
  "Décoder les valeurs nominales",
  "Respecter les limites et les valeurs invalides",
  "Contrôler l'intégrité des messages",
  "Superviser fraîcheur et séquence",
  "Traiter un défaut signalé par la source",
  "Appliquer la logique de sécurité de la mission",
  "Reprendre après un défaut",
];
const competences = (fails: number[]): string[] =>
  CP_LABELS.map((label, k) => `CP-${k + 1} — ${label} : ${fails.includes(k + 1) ? "FAIL" : "PASS"}`);

const MEASURE_HINT =
  "Relevez cette grandeur dans des conditions où l'anomalie se manifeste, sans bouger les curseurs avant d'appuyer sur Consigner.";
const CATEGORIES: Choice[] = [
  { value: "SENSOR/PHYSICAL", definition: "Défaut du capteur, de son faisceau, de son montage ou de la grandeur physique. Les trames de l'ECU source restent conformes à leur protection et à leur rythme" },
  { value: "SOURCE_ECU/APPLICATION", definition: "Défaut de l'ECU source lui-même (logique, configuration, étalonnage, alimentation de l'ECU). Ses trames restent conformes à leur protection ; leur contenu ou leur présence est faux" },
  { value: "MESSAGE/E2E", definition: "Le message ne passe pas le contrôle de protection de bout en bout, ou un autre émetteur utilise le même identifiant" },
  { value: "CAN_BUS/PROTOCOL", definition: "Défaut de la couche CAN : trames d'erreur, acquittement, bus-off, débit" },
];
const OBSERVED: Choice[] = [
  { value: "TRAMES_ABSENTES", definition: "Les trames n'apparaissent plus sur le bus" },
  { value: "TRAMES_FIGÉES", definition: "Présentes à leur rythme, contenu et compteur identiques d'une trame à l'autre" },
  { value: "PROTECTION_NON_CONFORME", definition: "Au moins une trame sur 100 a une protection non conforme" },
  { value: "SÉQUENCE_NON_CONFORME", definition: "Protection conforme, mais compteur discontinu ou trames en surnombre" },
  { value: "DÉFAUT_SIGNALÉ_PAR_LA_SOURCE", definition: "Trames conformes ; la source signale un défaut (statut différent de « OK » ou code d'invalidité)" },
  { value: "VALEUR_HORS_PLAGE", definition: "Trames conformes, statut OK, valeur hors de la plage codable ou code d'invalidité sous statut OK" },
  { value: "VALEUR_INCOHÉRENTE", definition: "Trames conformes, statut OK, valeur dans la plage mais contraire à la redondance, à la grandeur imposée au banc ou à une autre grandeur transmise" },
  { value: "AUCUNE_ANOMALIE_SUR_LE_BUS", definition: "Rien de ce qui précède" },
];
const UNITS_RATE = ["trames/s"];
const UNITS_NONE = ["sans unité"];
const UNITS_PROPORTION = ["%", "fraction"];
const UNITS_VOLTAGE = ["V", "mV"];
const UNITS_VALUE = ["%", "fraction", "tr/min", "Hz", "°", "rad", "bar", "kPa", "0/1"];
const same = (state: string, text: string, units: string[]): RuleView => ({ state, component: text, other: text, units });
const RULES: RuleView[] = [
  {
    state: "TRAMES_ABSENTES",
    component: "Nombre de trames de la trame DATA de l'ECU cible reçues en 1,000 s",
    other: "Nombre de trames de l'identifiant cible reçues en 1,000 s",
    units: UNITS_RATE,
  },
  same("TRAMES_FIGÉES", "Incrément du compteur entre deux trames consécutives de la trame concernée", UNITS_NONE),
  same("PROTECTION_NON_CONFORME", "Proportion des trames concernées dont la protection est conforme, sur 100 trames consécutives", UNITS_PROPORTION),
  same("SÉQUENCE_NON_CONFORME", "Nombre de trames concernées reçues en 1,000 s", UNITS_RATE),
  {
    state: "DÉFAUT_SIGNALÉ_PAR_LA_SOURCE",
    component: "Tension continue, par rapport à la broche de masse de l'ECU source, à la broche de l'ECU source reliée à la sortie de la cible",
    other: "Tension continue, par rapport à la broche de masse de l'ECU source, à la broche de l'ECU source où aboutit la cible",
    units: UNITS_VOLTAGE,
  },
  same("VALEUR_HORS_PLAGE", "Valeur brute (entier) du signal concerné", UNITS_NONE),
  same(
    "VALEUR_INCOHÉRENTE",
    "Valeur physique transmise par l'ECU source pour le signal qui dépend de la cible, moyenne sur 1 s, aux conditions capturées",
    UNITS_VALUE,
  ),
  same("AUCUNE_ANOMALIE_SUR_LE_BUS", "Tension à la broche LV_IN de l'ECU source", UNITS_VOLTAGE),
];

// ------------------------------------------------------------------------------------ invented mission

const MISSION_ID = "fan";
const MISSION_TITLE = "FAN — régulation du ventilateur moteur";
const GROUP = "G07";
const COHORT = "promo-2026";

const LIST: MissionEntry[] = [
  { id: MISSION_ID, title: MISSION_TITLE, available: true, reason: null },
  { id: "tpm", title: "TPM — surveillance de la pression des pneus", available: true, reason: null },
  { id: "lux", title: "LUX — allumage automatique des feux", available: true, reason: null },
  { id: "hvb", title: "HVB — tension de la batterie de traction", available: false, reason: "pack de mission absent de ce poste" },
];

const STATEMENT: StatementView = {
  summary:
    "Le calculateur moteur publie la température du liquide de refroidissement et la demande de climatisation ; le boîtier batterie publie la tension du réseau de bord. Votre nœud décode ces trames et publie la consigne du ventilateur.",
  requirements: [
    "Publier la température du liquide (A1) en °C et la demande de climatisation (A2).",
    "Calculer la consigne du ventilateur (A3) en % à chaque tick.",
    "Ne jamais publier une valeur égale au code invalide.",
    "Signaler l'absence de la trame moteur (A5) au-delà du timeout.",
  ],
  slots: [
    { slot: 0, code: "A1", label: "Température liquide moteur", unit: "°C" },
    { slot: 1, code: "A2", label: "Demande de climatisation", unit: "0/1" },
    { slot: 2, code: "A3", label: "Consigne ventilateur", unit: "%" },
    { slot: 3, code: "A4", label: "Statut capteur température", unit: "0/1" },
    { slot: 4, code: "A5", label: "Trame moteur absente", unit: "0/1" },
    { slot: 5, code: "A6", label: "Tension réseau de bord", unit: "V" },
  ],
};

/** Sorted by title, as the core sends it. */
const LIBRARY: LibraryItem[] = [
  { title: "Guide de la protection de bout en bout", publisher: "N7RT", reference: "E2E-G", revision: "A", date: null },
  { title: "Messagerie CAN du calculateur moteur", publisher: "N7RT", reference: "FAN-MSG", revision: "B", date: "2026-09-01" },
  { title: "Notice du banc CAN Lab", publisher: "N7RT", reference: "BANC-01", revision: "3", date: "2026-06-15" },
];
const DOCUMENTS = LIBRARY.map((d, i) => ({ id: `lib-${i + 1}`, title: d.title }));

const EMPTY_FICHE: NominalPart = {
  documents_utilises: [], trames: [], signaux: [], statuts: [],
  supervision: { premier_timeout_ms: null, timeout_ms: null, peremption_ms: null, pertes_consecutives_tolerees: null },
  decisions: [null, null, null, null], justification: "",
};
const EMPTY_FICHE_FIELDS = [
  "Documents utilisés : au moins un document", "Trames : au moins une trame", "Signaux : au moins un signal",
  "Statuts : au moins un statut", "Supervision : premier timeout", "Supervision : timeout", "Supervision : péremption",
  "Supervision : pertes consécutives tolérées", "Décision DD-1", "Décision DD-2", "Décision DD-3", "Décision DD-4",
];

const noField = () => ({ ordre: null, lsb_bit: null, longueur: null });

/** A complete nominal part: 2 frames, 4 signals, 2 statuses, supervision, the four decisions. */
const FICHE: NominalPart = {
  documents_utilises: [
    { document: "lib-2", pages: "p. 4 à 7" },
    { document: "lib-1", pages: "§ 2.3" },
  ],
  trames: [
    {
      identifiant: 0x1a4, format: "STANDARD_11_BITS", dlc: 8, periode_ms: 20,
      protection: { presente: true, crc: { ordre: "INTEL", lsb_bit: 56, longueur: 8 }, compteur: { ordre: "INTEL", lsb_bit: 48, longueur: 4 } },
      compteur_de_vie: noField(),
    },
    {
      identifiant: 0x2b0, format: "STANDARD_11_BITS", dlc: 4, periode_ms: 100,
      protection: { presente: false, crc: noField(), compteur: noField() },
      compteur_de_vie: { ordre: "INTEL", lsb_bit: 28, longueur: 4 },
    },
  ],
  signaux: [
    { nom: "Température liquide", identifiant: 0x1a4, slots: [0], ordre: "INTEL", lsb_bit: 0, longueur: 10, signe: "NON_SIGNÉ", facteur: 0.25, offset: -40, code_invalide: 1023 },
    { nom: "Demande clim", identifiant: 0x1a4, slots: [1], ordre: "INTEL", lsb_bit: 12, longueur: 1, signe: "NON_SIGNÉ", facteur: 1, offset: 0, code_invalide: "AUCUN" },
    { nom: "Couple résistant", identifiant: 0x1a4, slots: [2], ordre: "INTEL", lsb_bit: 16, longueur: 8, signe: "COMPLÉMENT_À_DEUX", facteur: 0.5, offset: 0, code_invalide: -128 },
    { nom: "Tension batterie", identifiant: 0x2b0, slots: [5], ordre: "MOTOROLA", lsb_bit: 7, longueur: 12, signe: "NON_SIGNÉ", facteur: 0.01, offset: 0, code_invalide: 4095 },
  ],
  statuts: [
    { identifiant: 0x1a4, slots: [3], ordre: "INTEL", lsb_bit: 13, longueur: 2, code_ok: 0 },
    { identifiant: 0x2b0, slots: [4], ordre: "INTEL", lsb_bit: 24, longueur: 2, code_ok: 1 },
  ],
  supervision: { premier_timeout_ms: 500, timeout_ms: 60, peremption_ms: 200, pertes_consecutives_tolerees: 2 },
  decisions: ["REJETER", "IGNORER_SES_DONNÉES", "BORNER", "PUBLIER_LA_DERNIÈRE"],
  justification:
    "Identifiants, positions et facteurs relevés dans la messagerie (p. 4 à 7) puis confirmés au banc en faisant varier la température imposée. Le CRC est contrôlé avant tout décodage.",
};

const PARTS = ["ECU1", "ECU2", "R3", "R4", "C5", "J3"];
const PINS = ["ECU1.1", "ECU1.2", "ECU1.3", "ECU1.4", "ECU2.5", "ECU2.6", "ECU2.7", "ECU2.8", "J3.1", "J3.2", "J3.3", "R3.1", "R3.2"];

const INCIDENT_1: IncidentPart = {
  rang: 1,
  cible: { type: "CONDUCTEUR", conducteur: { a: "J3.2", b: "ECU2.7" }, composant: null, identifiant: null },
  categorie: "SENSOR/PHYSICAL",
  etat_observe: "VALEUR_INCOHÉRENTE",
  mesure_cle: {
    valeur: 87.5,
    unite: "%",
    consignation: {
      consignee_s: 1_792_067_520,
      grandeur: RULES[6].other,
      conditions: { apercu: true },
      preuve_sha256: "5d2f8a41".repeat(8),
    },
  },
  justification:
    "La consigne publiée ne suit plus la température imposée au banc. Au connecteur J3, le fil de la sonde vers l'ECU2 présente une coupure franche.",
};

/** The incident target declared on the bus: an identifier, a proportion to measure, nothing recorded yet. */
const INCIDENT_1_BUS: IncidentPart = {
  rang: 1,
  cible: { type: "IDENTIFIANT", conducteur: { a: null, b: null }, composant: null, identifiant: "0x2B0" },
  categorie: "MESSAGE/E2E",
  etat_observe: "PROTECTION_NON_CONFORME",
  mesure_cle: { valeur: null, unite: null, consignation: null },
  justification: "",
};

function incidentView(editable: boolean, rule: RuleView = RULES[6], consigned: string | null = "le 15/10/2026 à 14:32"): IncidentView {
  return {
    rank: 1, editable, grandeur: rule.other, units: rule.units, hint: MEASURE_HINT, consigned,
    categories: CATEGORIES, states: OBSERVED, rules: RULES, parts: PARTS, pins: PINS,
  };
}

// --------------------------------------------------------------------------------- Physical Lab bench

/** The public fixture circuit as the mission bench: internal values withheld, the multimeter on RV1 a → w. */
function physBench(): PhysFixture {
  const snapshot = { ...(JSON.parse(snapshotJson) as SnapshotDto), name: "banc-fan", disclosure: "mission" as const };
  const sample = JSON.parse(valuesJson) as ValuesDto;
  const values: ValuesDto = {
    ...sample,
    seq: 1, doc_id: snapshot.doc_id, rev: snapshot.rev, run: "run", preview: false, disclosure: "mission",
    nets: [], comps: [], diagnostics: [],
    meter: { com: { comp: 3, pin: "a" }, vohm: { comp: 3, pin: "w" }, function: "v_dc", display: "4,155 V" },
  };
  return {
    library: JSON.parse(libraryJson) as KindDef[],
    snapshot,
    values,
    demos: JSON.parse(demosJson) as DemoEntry[],
    files: JSON.parse(filesJson) as FileEntry[],
    journal: JSON.parse(journalJson) as JournalEntry[],
  };
}

// ------------------------------------------------------------------------------------- board and bus

const PORT: PortInfo = { port: "COM7", vid: 0x0483, pid: 0x3754, serial: "0047002B5136470C20303539", product: "STLINK-V3", probe: "STLINK-V3E" };
const BOARD: BoardIdentity = {
  runtime: "0.1.0", build_id: "5e0c41a9d2b7", abi: "1.0", proto: 1, uid: "0047002B-5136470C-20303539", dev_id: 0x469, rev_id: 0x2001,
  reset_flags: 0x0c, reset_causes: ["reset logiciel", "broche NRST"], student_status: 1, student_status_text: "code étudiant chargé",
  student_abi: "1.0", student_crc32: "0x7B21E4C9", rx_window: 32, fault_pending: false, runtime_verified: true, compatible: true,
  problems: [],
};
const SESSION_ID = 0x5c3a91e2;
const LAST_SEQ = 18432;

function f32Bits(v: number): number {
  const d = new DataView(new ArrayBuffer(4));
  d.setFloat32(0, v, true);
  return d.getUint32(0, true);
}

/** Deterministic bytes: the same shots every time. */
function random(seed: number): () => number {
  let s = seed >>> 0;
  return () => {
    s = (Math.imul(s, 1664525) + 1013904223) >>> 0;
    return s / 2 ** 32;
  };
}

const BUS = [
  { id: 0x0c8, dlc: 8 }, { id: 0x1a4, dlc: 8 }, { id: 0x2b0, dlc: 4 }, { id: 0x3e8, dlc: 8 }, { id: 0x1a4, dlc: 8 },
  { id: 0x418, dlc: 6 }, { id: 0x5f0, dlc: 3 }, { id: 0x1a4, dlc: 8 },
];

function frames(count: number): FrameRow[] {
  const next = random(7);
  const rows: FrameRow[] = [];
  for (let i = 0; i < count; i++) {
    const seq = LAST_SEQ - count + 1 + i;
    const bus = BUS[seq % BUS.length];
    let data = "";
    for (let b = 0; b < bus.dlc; b++) data += Math.floor(next() * 256).toString(16).padStart(2, "0");
    const entries =
      bus.id === 0x1a4 ? [{ slot: 0, kind: 0, bits: f32Bits(92.5) }, { slot: 1, kind: 0, bits: f32Bits(1) }]
      : bus.id === 0x2b0 ? [{ slot: 5, kind: 0, bits: f32Bits(13.8) }]
      : [];
    rows.push({
      session_id: SESSION_ID, frame_seq: seq, t_us: seq * 10_000, can_id: bus.id, flags: 0, dlc: bus.dlc, data, outcome: "result",
      status: 0, result_flags: 0, cycles: 900 + Math.floor(next() * 700), entries, nack: null,
      rtt_us: 2400 + Math.floor(next() * 700), stim_rev: 0,
    });
  }
  return rows;
}

function slots(): SlotView[] {
  const values = [92.5, 1, 65, 0, 0, 13.8];
  return values.map((v, slot) => ({
    slot, kind: 0, bits: f32Bits(v), source: slot === 2 ? "tick" : "frame", at: slot === 2 ? 1843 : LAST_SEQ - slot, updates: 1843,
  }));
}

function stats(active: boolean): Stats {
  const n = (v: number): number => (active ? v : 0);
  return {
    frames_sent: n(18432), results: n(18430), nacks: 0, nack_codes: {}, lost: 0, late: 0, aborted: 0, in_flight: n(2),
    in_flight_max: n(9), window: 32, credit_waits: 0, tx_fps: n(474), rx_fps: n(473), rtt_count: n(18430), rtt_p50_us: n(2750),
    rtt_p95_us: n(4400), rtt_p99_us: n(5300), rtt_max_us: n(6900), rtt_mean_us: n(2810), sim_time_sent: n(1843),
    tick_results: n(1843), logs: n(3), rx_framing: {}, rx_rejects: 0, board_seq_gaps: 0, pre_sync_discarded: 0,
    bytes_tx: active ? 845_120 : 270, bytes_rx: active ? 762_304 : 829,
    queues: {
      sim_depth: n(3), sim_high_water: n(12), sim_capacity: 1024, sim_paused_ms: 0, sim_dropped: 0, sim_generated: n(18432),
      sim_max_step_us: n(8900), ui_rows_capacity: 4096, ui_rows_not_shown: 0, ui_logs_dropped: 0, ui_errors_dropped: 0,
      link_loop_max_us: active ? 13_400 : 15_700, write_max_us: active ? 3800 : 600, read_chunk_max: active ? 225 : 24,
      tx_backlog_max: 0,
    },
  };
}

/** `active`: a session runs (frames, results, slots); otherwise the board is ready, no session. */
function batch(active: boolean): UiBatch {
  const heartbeat: HeartbeatView = {
    uptime_ms: 912_000, acked_frame_seq: active ? LAST_SEQ : 0, hook_running_ms: 0, state: active ? 2 : 1, student_disabled: false,
    rx_frames: active ? 18430 : 0, rx_seq_gaps: 0, rx_framing_errors: 0, rx_crc_errors: 0, rx_overruns: 0, log_dropped: 0,
    queue_high_water: active ? 9 : 0, max_cycles: active ? 1640 : 0,
  };
  const status: Status = {
    state: active ? "SESSION" : "READY",
    stalled: false,
    student_busy: false,
    message: active
      ? "Session 5C3A91E2 active : le bus virtuel tourne."
      : "Carte prête : runtime 0.1.0 vérifié, code étudiant chargé (CRC 0x7B21E4C9).",
    target: PORT.port,
    port: PORT,
    ports: [PORT],
    board: BOARD,
    session: active ? { id: SESSION_ID, phase: "active", mode: 1, tick_period_ms: 10, elapsed_ms: 184_000, bus_time_us: 184_320_000 } : null,
    heartbeat,
    fault: null,
    stimuli: { pot: 128, button: false, format: "classic", can_id: 0x123, dlc: 8, rate_hz: 10 },
    stim_rev: 0,
    can_start: !active,
    can_stop: active,
  };
  const logs: LogRow[] = active
    ? [
        { session_id: SESSION_ID, t_us: 40_000, text: "décodeur prêt : 2 trames suivies" },
        { session_id: SESSION_ID, t_us: 520_000, text: "trame 0x2B0 reçue, supervision armée" },
        { session_id: SESSION_ID, t_us: 61_250_000, text: "consigne ventilateur 65 %" },
      ]
    : [];
  return { rev: 1, status, stats: stats(active), rows: active ? frames(60) : [], logs, errors: [], slots: active ? slots() : [] };
}

// ----------------------------------------------------------------------------------- mission views

const IDLE: CheckView = { state: "idle", elapsed_s: 0, total_s: 0, verdict: null, reason: null, run_file: null };

const CLOSED: MissionView = {
  id: null, title: null, open: false, kind: null, group: null, statement: null, library: [], check: IDLE, tick_period_ms: null,
  progression: null,
};

function opened(progression: ProgressionView, check: CheckView = IDLE): MissionView {
  return {
    id: MISSION_ID, title: MISSION_TITLE, open: true, kind: "group", group: GROUP, statement: STATEMENT, library: LIBRARY, check,
    tick_period_ms: 10, progression,
  };
}

type StepSpec = ["À FAIRE" | "EN COURS" | "VALIDÉ" | "ÉCHEC À CORRIGER", string | null];

function steps(specs: StepSpec[], current: number | null): StepView[] {
  return STEP_NAMES.map((name, k) => {
    const [state, since] = specs[k] ?? ["À FAIRE", null];
    return { name, state, since, current: current === k };
  });
}

function progression(p: Pick<ProgressionView, "steps" | "current" | "order" | "button"> & Partial<ProgressionView>): ProgressionView {
  return {
    readable: true, ready: false, running: false, blocked: null, feedback: [], nominal_editable: true, fiche_state: FICHE_DRAFT,
    documents: DOCUMENTS, incident: null, ...p,
  };
}

const V = {
  e0: "le 15/10/2026 à 14:05",
  e1: "le 15/10/2026 à 14:48",
  e2: "le 15/10/2026 à 15:20",
  e3: "le 15/10/2026 à 15:57",
  e4: "le 15/10/2026 à 16:41",
};

const STUDENT_NEW: AccessView = {
  mode: "student", student: { locked: null, locked_title: null, pack: null, last_run: null }, edition: "édition publique", oracle: false,
};
const STUDENT_LOCKED: AccessView = {
  mode: "student",
  student: { locked: MISSION_ID, locked_title: MISSION_TITLE, pack: { cohort: COHORT, group: GROUP }, last_run: null },
  edition: "édition publique",
  oracle: false,
};

const STUDENT_PANEL = "details:has(#student-flow)";
const MISSION_PANEL = "#mission-panel";
/** Brought to the top when the fiche is open: the fiche lives in the cockpit's bottom dock (tab « Fiche »), so the side
 * panel shows the progression from its top. */
const FICHE_TOP = "#mission-panel";
const E1_PASSED_FEEDBACK = [passed(V.e0), "Incident 1 ouvert. Le véhicule revient d'essai avec une anomalie."];
const E5_STEPS: StepSpec[] = [["VALIDÉ", V.e0], ["VALIDÉ", V.e1], ["VALIDÉ", V.e2], ["VALIDÉ", V.e3], ["VALIDÉ", V.e4]];

function base(p: Partial<Scenario> & Pick<Scenario, "label" | "mission">): Scenario {
  return {
    access: STUDENT_LOCKED, list: LIST, fiche: FICHE, incident: null, batch: batch(true), ready: "#progress-section", openFiche: false,
    scroll: MISSION_PANEL, ...p,
  };
}

const E0_OPEN: StepSpec[] = [["EN COURS", null]];
const E1_OPEN: StepSpec[] = [["VALIDÉ", V.e0], ["EN COURS", null]];

const STATE_TABLE: Record<string, () => Scenario> = {
  "mission-list": () =>
    base({
      label: "Mode étudiant, aucune mission choisie : liste des missions du poste",
      access: STUDENT_NEW, mission: CLOSED, fiche: EMPTY_FICHE, batch: batch(false), ready: "#student-choose", scroll: STUDENT_PANEL,
    }),
  "student-locked": () =>
    base({
      label: "Mission choisie (verrouillée), pack du groupe importé, code du groupe pas encore saisi",
      mission: CLOSED, fiche: EMPTY_FICHE, batch: batch(false), ready: "#student-locked", scroll: STUDENT_PANEL,
    }),
  "e0-start": () =>
    base({
      label: "E0 en cours, fiche vide et fermée : la vérification attend la fiche",
      fiche: EMPTY_FICHE,
      batch: batch(false),
      mission: opened(progression({
        steps: steps(E0_OPEN, 0), current: 0, order: ORDER_E0, button: BUTTON.nominal, blocked: incomplete(EMPTY_FICHE_FIELDS),
      })),
    }),
  "e0-fiche-open": () =>
    base({
      label: "E0 en cours, fiche nominale ouverte et remplie, vérification possible",
      openFiche: true,
      scroll: FICHE_TOP,
      mission: opened(progression({ steps: steps(E0_OPEN, 0), current: 0, order: ORDER_E0, button: BUTTON.nominal, ready: true })),
    }),
  "e0-fiche-records": () =>
    base({
      label: "E0, fiche remplie, défilée jusqu'aux trames : enregistrements, champs de bits, numérotation",
      openFiche: true,
      scroll: '#progress-fiche .fiche-block[data-section="trames"]',
      mission: opened(progression({ steps: steps(E0_OPEN, 0), current: 0, order: ORDER_E0, button: BUTTON.nominal, ready: true })),
    }),
  "e0-fiche-empty": () =>
    base({
      label: "E0 en cours, fiche nominale vide ouverte (le moment de la capture 03)",
      fiche: EMPTY_FICHE,
      batch: batch(false),
      openFiche: true,
      scroll: FICHE_TOP,
      mission: opened(progression({
        steps: steps(E0_OPEN, 0), current: 0, order: ORDER_E0, button: BUTTON.nominal, blocked: incomplete(EMPTY_FICHE_FIELDS),
      })),
    }),
  "e0-fiche-invalid": () =>
    base({
      label: "E0, fiche remplie, identifiant 0x900 tapé puis « Enregistrer la fiche » : format refusé",
      openFiche: true,
      scroll: FICHE_TOP,
      after: [{ type: ["#progress-fiche .fiche-id", "0x900"] }, { click: "#progress-fiche .fiche-save" }],
      mission: opened(progression({ steps: steps(E0_OPEN, 0), current: 0, order: ORDER_E0, button: BUTTON.nominal, ready: true })),
    }),
  "e0-fiche-dirty": () =>
    base({
      label: "E0, fiche remplie, période modifiée sans « Enregistrer la fiche » : la vérification l'enregistrera d'abord",
      openFiche: true,
      scroll: FICHE_TOP,
      after: [{ type: ['#progress-fiche [data-key="trames.0.periode"]', "20"] }],
      mission: opened(progression({ steps: steps(E0_OPEN, 0), current: 0, order: ORDER_E0, button: BUTTON.nominal, ready: true })),
    }),
  "e0-narrow": () =>
    base({
      label: "Comme e0-fiche-open, panneau latéral réduit à 220 px",
      openFiche: true,
      scroll: FICHE_TOP,
      layout: { left: 220 },
      mission: opened(progression({ steps: steps(E0_OPEN, 0), current: 0, order: ORDER_E0, button: BUTTON.nominal, ready: true })),
    }),
  "e0-console-hidden": () =>
    base({
      label: "Console du bas masquée, puis « Ouvrir la fiche »",
      openFiche: true,
      scroll: FICHE_TOP,
      layout: { hidden: ["bottom"] },
      mission: opened(progression({ steps: steps(E0_OPEN, 0), current: 0, order: ORDER_E0, button: BUTTON.nominal, ready: true })),
    }),
  "e0-failed": () =>
    base({
      label: "E0 en échec : verdict, trois lignes publiques, intervalle avant la prochaine vérification",
      mission: opened(progression({
        steps: steps([["ÉCHEC À CORRIGER", null]], 0), current: 0, order: ORDER_E0, button: BUTTON.nominal,
        blocked: interval(102),
        feedback: ["Échec à corriger.", "Fiche nominale : non conforme", "Essai nominal : réussi", DETAIL_HIDDEN],
      })),
    }),
  "e1-incident": () =>
    base({
      label: "E1 en cours, fiche incident ouverte (cible, catégorie, état, mesure consignée)",
      incident: INCIDENT_1,
      openFiche: true,
      scroll: FICHE_TOP,
      mission: opened(progression({
        steps: steps(E1_OPEN, 1), current: 1, order: orderIncident(1), button: BUTTON.incident, ready: true,
        feedback: E1_PASSED_FEEDBACK, nominal_editable: false, incident: incidentView(true),
      })),
    }),
  "e1-identifier": () =>
    base({
      label: "E1, cible « Identifiant », valeur « 4,1 V » tapée puis « Enregistrer la fiche » : format refusé",
      incident: INCIDENT_1_BUS,
      openFiche: true,
      scroll: FICHE_TOP,
      after: [{ type: ["#progress-fiche .incident-value", "4,1 V"] }, { click: "#progress-fiche .fiche-save" }],
      mission: opened(progression({
        steps: steps(E1_OPEN, 1), current: 1, order: orderIncident(1), button: BUTTON.incident, ready: true,
        feedback: E1_PASSED_FEEDBACK, nominal_editable: false, incident: incidentView(true, RULES[2], null),
      })),
    }),
  "e1-interrupted": () =>
    base({
      label: "E1 : vérification interrompue (non comptée), fiche fermée",
      incident: INCIDENT_1,
      mission: opened(progression({
        steps: steps(E1_OPEN, 1), current: 1, order: orderIncident(1), button: BUTTON.incident, ready: true,
        feedback: [interrupted("banc fermé")], nominal_editable: false, incident: incidentView(true),
      })),
    }),
  "e1-bench": () =>
    base({
      label: "E1 dans le Physical Lab : banc de la mission, multimètre (4,155 V)",
      incident: INCIDENT_1,
      view: "phys",
      scroll: null,
      ready: "#phys-meter .meter-display",
      phys: physBench(),
      mission: opened(progression({
        steps: steps(E1_OPEN, 1), current: 1, order: orderIncident(1), button: BUTTON.incident, ready: true,
        feedback: E1_PASSED_FEEDBACK, nominal_editable: false, incident: incidentView(true),
      })),
    }),
  "e1-bench-running": () =>
    base({
      label: "E1 dans le Physical Lab pendant la vérification : sondes verrouillées, message du cœur sous le multimètre",
      incident: INCIDENT_1,
      view: "phys",
      scroll: null,
      ready: "#phys-meter .meter-display",
      phys: physBench(),
      mission: opened(
        progression({
          steps: steps(E1_OPEN, 1), current: 1, order: orderIncident(1), button: BUTTON.incident, running: true, blocked: attempt(28),
          nominal_editable: false, incident: incidentView(false),
        }),
        { state: "running", elapsed_s: 12, total_s: 40, verdict: null, reason: null, run_file: null },
      ),
    }),
  "e1-verifying": () =>
    base({
      label: "E1 : vérification de la réparation en cours (fiche figée, bus et slots couverts, verdict précédent masqué)",
      incident: INCIDENT_1,
      openFiche: true,
      mission: opened(
        progression({
          steps: steps(E1_OPEN, 1), current: 1, order: orderIncident(1), button: BUTTON.incident, running: true, blocked: attempt(28),
          feedback: E1_PASSED_FEEDBACK, nominal_editable: false, incident: incidentView(false),
        }),
        { state: "running", elapsed_s: 12, total_s: 40, verdict: null, reason: null, run_file: null },
      ),
    }),
  "e4-qualification": () =>
    base({
      label: "E4 : verdict de la qualification par compétence (8 lignes CP, PASS et FAIL)",
      mission: opened(progression({
        steps: steps([["VALIDÉ", V.e0], ["VALIDÉ", V.e1], ["VALIDÉ", V.e2], ["VALIDÉ", V.e3], ["ÉCHEC À CORRIGER", null]], 4),
        current: 4, order: ORDER_QUALIFICATION, button: BUTTON.qualification, blocked: interval(252), feedback: competences([3, 5]),
        nominal_editable: false,
      })),
    }),
  "e4-running": () =>
    base({
      label: "E4 : qualification en cours (120 s sur 600), bus et slots couverts",
      mission: opened(
        progression({
          steps: steps([["VALIDÉ", V.e0], ["VALIDÉ", V.e1], ["VALIDÉ", V.e2], ["VALIDÉ", V.e3], ["EN COURS", null]], 4),
          current: 4, order: ORDER_QUALIFICATION, button: BUTTON.qualification, running: true, blocked: qualifying(480),
          feedback: [passed(V.e3), "Qualification finale ouverte."], nominal_editable: false,
        }),
        { state: "running", elapsed_s: 120, total_s: 600, verdict: null, reason: null, run_file: null },
      ),
    }),
  "e5-closed": () =>
    base({
      label: "E5 : mission clôturée, toutes les étapes validées, fiche finale exportée ; fiche ouverte en lecture seule",
      openFiche: true,
      mission: opened(progression({
        steps: steps(E5_STEPS, null),
        current: null, order: ORDER_CLOSED, button: BUTTON.qualification, feedback: [...competences([]), "Mission clôturée.", EXPORTED],
        nominal_editable: false, fiche_state: "Exportée le 15/10/2026 à 16:41",
      })),
    }),
  "e5-export-failed": () =>
    base({
      label: "E5 : mission clôturée, exportation de la fiche finale en échec",
      mission: opened(progression({
        steps: steps(E5_STEPS, null),
        current: null, order: ORDER_CLOSED, button: BUTTON.qualification, feedback: [...competences([]), "Mission clôturée.", NOT_EXPORTED],
        nominal_editable: false, fiche_state: "Clôturée, exportation à reprendre",
      })),
    }),
};

export const STATE_NAMES = Object.keys(STATE_TABLE);

/** A fresh copy of the named state, or null for an unknown name. */
export function scenario(name: string): Scenario | null {
  const make = STATE_TABLE[name];
  return make ? structuredClone(make()) : null;
}
