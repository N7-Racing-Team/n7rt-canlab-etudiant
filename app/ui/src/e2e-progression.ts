// Release 0.3 student progression on a real board (scenarios progression-<id> and ide-progression-<id>,
// tools/stm32/n7rt_app.py): E0 to E5 through the real controls of the progression panel, the incident form, the
// multimeter, the Physical Lab edits and the integrated IDE. What a student would know at each step (the fiche, where
// to measure, what to repair) comes from the test operator's answers file N7RT_E2E_ANSWERS (e2e_answers), produced at
// test time from the test pack by the private tooling: nothing of it is embedded in the application, and neither the
// answers nor the values read enter the report (only verdict lines, states, counts and digests).
//
// Every incident is played as: one attempt with a wrong category and no repair (both lines fail, except the repair
// line of an incident with nothing to repair), one attempt with the real repair and a wrong value (the repair alone
// passes), « Reproduire l'anomalie » and the repair again, then the passing attempt. An incident without a physical
// repair is played with the student code instead when the IDE is there: the template flashed fails the hidden test,
// the reference flashed again passes it. Stages (N7RT_E2E_STAGE): « full », « until-incident » (stops after the
// first attempt of incident 1: the harness closes the application) and « resume » (reopens and goes on).

import { e2e, mission } from "./bridge";
import { $ } from "./dom";
import { imageFor } from "./ide/api";
import { activeIde } from "./ide/view";
import { physView } from "./lab/view";
import type { Edit, WorldQuantity } from "./lab/types";
import type { Store } from "./store";
import type { E2eConfig, FrameRow, IncidentPart, MissionView, NominalPart, ProgressionView } from "./types";

const sleep = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));

class ProgressionFailure extends Error {}

function fail(message: string): never {
  throw new ProgressionFailure(message);
}

// ------------------------------------------------------------------------------------------------- answers file

type Measure =
  | { kind: "voltage"; pin: string; reference: string; unit: string; tol_abs: number; tol_rel: number }
  | { kind: "proportion"; id: number; data_ids: number[]; unit: string; samples: { data: string; good: boolean }[] }
  | {
      kind: "value"; id: number; positions: number[]; signed: boolean; factor: number; offset: number; invalid: number | null;
      unit: string; tol_abs: number; tol_rel: number; samples: { data: string; value: number | null }[];
    };

type Repair =
  | { kind: "wire"; a: string; b: string }
  | { kind: "param"; designator: string; param: string; value: number }
  | { kind: "none" };

interface IncidentAnswer {
  rank: number;
  /** The perfect incident part, without the value (read at runtime) and its unit. */
  part: IncidentPart;
  measure: Measure;
  /** World conditions where the anomaly shows (the inspector's mechanical controls). */
  conditions: { designator: string; quantity: WorldQuantity; value: number }[];
  repair: Repair;
}

interface Answers {
  mission: string;
  /** The perfect nominal part; its first document is the first entry of the mission library. */
  nominal: NominalPart;
  incidents: IncidentAnswer[];
  /** Two pins of the bench to show the multimeter on, away from every incident (tutorial capture). */
  neutral_meter: { com: string; vohm: string };
}

function checkAnswers(a: unknown, id: string): Answers {
  const x = a as Answers;
  if (!x || typeof x !== "object" || x.mission !== id || !x.nominal || !Array.isArray(x.incidents) || !x.neutral_meter) {
    fail(`réponses de test N7RT_E2E_ANSWERS : forme inattendue ou autre mission que ${id}`);
  }
  x.incidents.forEach((inc, k) => {
    if (inc.rank !== k + 1 || !inc.part || !inc.measure || !inc.repair || !Array.isArray(inc.conditions)) {
      fail(`réponses de test : incident ${k + 1} mal formé`);
    }
  });
  return x;
}

// ------------------------------------------------------------------------------------------------- bus readings

function bytesOf(hex: string): number[] {
  const h = hex.replace(/[^0-9a-f]/gi, "");
  const out: number[] = [];
  for (let i = 0; i + 1 < h.length; i += 2) out.push(parseInt(h.slice(i, i + 2), 16));
  return out;
}

/** Crc_CalculateCRC8H2F update (polynomial 0x2F). */
function crc8h2f(reg: number, data: number[]): number {
  let crc = reg;
  for (const b of data) {
    crc ^= b;
    for (let k = 0; k < 8; k++) crc = crc & 0x80 ? ((crc << 1) ^ 0x2f) & 0xff : (crc << 1) & 0xff;
  }
  return crc;
}

/** Profile 22: the CRC of byte 0 over bytes 1… then the Data ID of the counter (low nibble of byte 1). */
export function p22Conforms(d: number[], dataIds: number[]): boolean {
  if (d.length < 2) return false;
  const crc = crc8h2f(crc8h2f(0xff, d.slice(1)), [dataIds[d[1] & 0x0f]]) ^ 0xff;
  return d[0] === crc;
}

/** Physical value of a field given by its bit positions (LSB first, bit = byte × 8 + bit in byte); null = SNA. */
export function fieldValue(d: number[], m: { positions: number[]; signed: boolean; factor: number; offset: number; invalid: number | null }): number | null {
  let raw = 0;
  m.positions.forEach((p, i) => {
    if (((d[p >> 3] ?? 0) >> (p & 7)) & 1) raw += 2 ** i;
  });
  if (m.signed && raw >= 2 ** (m.positions.length - 1)) raw -= 2 ** m.positions.length;
  // the SNA code is a raw value as read (sign extended), codec.rs
  if (m.invalid !== null && raw === m.invalid) return null;
  return raw * m.factor + m.offset;
}

/** The frame quantities of measure.rs on the rows of one session: the last 100 frames (proportion), the mean over
 * the last second (value). */
export function frameQuantity(m: Measure, rows: FrameRow[]): number | null {
  if (m.kind === "voltage") return null;
  const of = rows.filter((r) => r.outcome === "result" && r.can_id === m.id);
  if (m.kind === "proportion") {
    if (of.length < 100) return null;
    const good = of.slice(-100).filter((r) => p22Conforms(bytesOf(r.data), m.data_ids)).length;
    return good;
  }
  const end = of[of.length - 1]?.t_us;
  if (end === undefined) return null;
  const values = of.filter((r) => r.t_us + 1_000_000 > end).map((r) => fieldValue(bytesOf(r.data), m)).filter((v): v is number => v !== null);
  return values.length ? values.reduce((s, v) => s + v, 0) / values.length : null;
}

/** The decoding of the answers checked against the samples the private tooling decoded with the pack. */
function selfCheck(m: Measure): void {
  if (m.kind === "proportion") {
    for (const s of m.samples) if (p22Conforms(bytesOf(s.data), m.data_ids) !== s.good) fail("contrôle P22 du banc de test ≠ celui du pack");
  } else if (m.kind === "value") {
    for (const s of m.samples) {
      const v = fieldValue(bytesOf(s.data), m);
      if ((v === null) !== (s.value === null) || (v !== null && Math.abs(v - (s.value as number)) > 1e-9 * Math.max(1, Math.abs(v)))) {
        fail("décodage du banc de test ≠ celui du pack");
      }
    }
  }
}

/** « 4,155 V », « 12,3 mV » → volts. */
export function meterVolts(text: string): number | null {
  const m = /(-?\d+(?:[.,]\d+)?)\s*(mV|V)\b/.exec(text.replace(/[  ]/g, " "));
  if (!m) return null;
  const v = Number(m[1].replace(",", "."));
  return m[2] === "mV" ? v / 1000 : v;
}

// ------------------------------------------------------------------------------------------------- the scenario

export interface ProgressionContext {
  cfg: E2eConfig;
  store: Store;
  id: string;
  step: (name: string, fn: () => Promise<unknown>) => Promise<void>;
  click: (sel: string) => Promise<void>;
  set: (sel: string, value: string) => void;
  until: (what: string, pred: () => boolean, timeoutMs: number) => Promise<number>;
  untilMission: (what: string, pred: (v: MissionView) => boolean, timeoutMs: number) => Promise<MissionView>;
  shot: (name: string, rects?: string[]) => Promise<void>;
  /** Re-bases the reboot watch (after a flash, which reboots the board on purpose). */
  watchBoard: () => void;
  noReboot: () => Promise<unknown>;
  /** Every delivered row since the scenario start (bounded), oldest first. */
  rows: () => FrameRow[];
  /** Opens the mission with the group code (the real controls). */
  openMission: () => Promise<unknown>;
  report: Record<string, unknown>;
}

interface Attempt {
  step: number;
  label: string;
  lines: string[];
}

const DETAIL_HIDDEN = "Le détail n'est pas communiqué. Reprenez vos mesures et vos essais.";
const PASSED = "Vérification réussie. Étape validée le ";
const INTERVAL_MS = 120_000;
const QUALIFICATION_INTERVAL_MS = 300_000;

export async function runProgression(ctx: ProgressionContext): Promise<void> {
  const { cfg, store, step, click, set, until, untilMission, shot } = ctx;
  const stage = cfg.stage ?? "full";
  if (!["full", "until-incident", "resume"].includes(stage)) fail(`N7RT_E2E_STAGE inconnu : ${stage}`);
  const answers = checkAnswers(await e2e.answers(), ctx.id);
  answers.incidents.forEach((i) => selfCheck(i.measure));
  const attempts: Attempt[] = [];
  const flashes: { at: string; crc32: string }[] = [];
  const reboots: unknown[] = [];
  let reproductions = 0;
  /** The student code is changed once, in the first incident without a physical repair. */
  let codeChanged = false;
  const progressionReport: Record<string, unknown> = { stage, attempts, flashes, reboot_checks: reboots };
  ctx.report.progression = progressionReport;

  const status = () => store.status!;
  const state = async (): Promise<ProgressionView> => (await mission.state()).progression ?? fail("pack sans progression");
  const untilProgress = async (what: string, pred: (p: ProgressionView) => boolean, timeoutMs: number) =>
    (await untilMission(what, (m) => !!m.progression && pred(m.progression), timeoutMs)).progression!;
  const lines = () => Array.from(document.querySelectorAll("#progress-feedback .feedback-line")).map((p) => p.textContent ?? "");
  const shown = (sel: string) => !$(sel).hidden;
  const fiche = () => $("#progress-fiche");
  const q = <T extends HTMLElement>(sel: string): T => fiche().querySelector<T>(sel) ?? fail(`champ ${sel} absent de la fiche`);

  // ----------------------------------------------------------------------------------------- bench and bus

  const lab = () => physView() ?? fail("vue Physical Lab absente");
  const comp = (designator: string) => [...lab().state.components.values()].find((c) => c.label === designator) ?? fail("un composant des réponses est absent du banc");
  const pinRef = (label: string) => {
    const i = label.lastIndexOf(".");
    return { comp: comp(label.slice(0, i)).id, pin: label.slice(i + 1) };
  };
  const showBench = async () => {
    await click('.activity button[data-show-view="phys"]');
    await until("vue Physical Lab affichée", () => !$("#phys-main").hidden, 3000);
  };
  const showCockpit = async () => {
    await click('.activity button[data-show-view="cockpit"]');
    await until("cockpit affiché", () => !$('.workspace[data-view="cockpit"]').hidden, 3000);
  };
  const busRunning = () => status().session?.phase === "active";
  const ensureBus = async () => {
    if (!busRunning()) {
      await showCockpit();
      await click("#btn-start");
      await until("session active", busRunning, 8000);
    }
    // the evidence of a measure: 100 frames of the slowest identifier and more (measure.rs)
    const sid = status().session!.id;
    await until("trafic de la session", () => ctx.rows().filter((r) => r.session_id === sid).length > 2000, 20000);
  };
  const stopBus = async () => {
    if (status().session) {
      await showCockpit();
      await click("#btn-stop");
      await until("session fermée", () => status().session === null, 8000);
    }
  };
  const sessionRows = () => {
    const sid = status().session?.id;
    return ctx.rows().filter((r) => r.session_id === sid);
  };
  /** The multimeter of the Physical Lab on two pins (the real selects), its display once steady. */
  const meter = async (com: string, vohm: string): Promise<number> => {
    await showBench();
    const option = (sel: string, label: string) => {
      const s = $<HTMLSelectElement>(sel);
      return Array.from(s.options).find((o) => o.textContent === label)?.value;
    };
    await until("broches de mesure au multimètre", () => !!option(".meter-com", com) && !!option(".meter-vohm", vohm), 5000);
    set(".meter-function", "v_dc");
    set(".meter-com", option(".meter-com", com)!);
    set(".meter-vohm", option(".meter-vohm", vohm)!);
    let last: number | null = null;
    const t0 = performance.now();
    for (;;) {
      await sleep(700);
      const v = meterVolts($(".meter-display").textContent ?? "");
      if (v !== null && last !== null && Math.abs(v - last) <= 0.005) return v;
      last = v;
      if (performance.now() - t0 > 15000) fail("multimètre instable ou illisible");
    }
  };
  /** What the student reads for the key measure now (canonical unit). */
  const observe = async (m: Measure): Promise<number> => {
    if (m.kind === "voltage") return meter(m.reference, m.pin);
    await ensureBus();
    await sleep(2500); // a full window of frames under the present conditions
    return frameQuantity(m, sessionRows()) ?? fail("mesure de trame impossible : trames insuffisantes");
  };
  const conditions = async (inc: IncidentAnswer) => {
    for (const c of inc.conditions) lab().controller.interact({ op: "set_world", comp: comp(c.designator).id, quantity: c.quantity, value: c.value });
    if (inc.conditions.length) await sleep(3000); // the vehicle reaches the conditions
  };
  const edit = async (what: string, e: Edit) => {
    const r = await lab().controller.edit(e);
    if (!r.ok) fail(`${what} refusé par le Physical Lab (${(r.error as { code?: string }).code ?? "erreur"})`);
  };
  /** The physical repair of the answers: a new wire for the wire between the two pins, or the setting back. */
  const repair = async (r: Repair) => {
    if (r.kind === "none") return;
    await showBench();
    if (r.kind === "param") {
      await edit("réglage", { op: "set_param", comp: comp(r.designator).id, name: r.param, value: r.value });
      return;
    }
    const [a, b] = [pinRef(r.a), pinRef(r.b)];
    const same = (x: { comp: number; pin: string }, y: { comp: number; pin: string }) => x.comp === y.comp && x.pin === y.pin;
    const wire = [...lab().state.wires.values()].find((w) => (same(w.a, a) && same(w.b, b)) || (same(w.a, b) && same(w.b, a)));
    if (!wire) fail("aucun fil entre les deux broches de la réparation");
    // the new wire follows the old one's path, from a to b: for some wires of the benches no L-shaped or automatic path
    // is free, and a student draws the bends by hand there, as the refusal says (oracle walk: workflow_flow::repair)
    const bends = same(wire!.a, a) ? [...wire!.bends] : [...wire!.bends].reverse();
    await edit("suppression du fil", { op: "delete", components: [], wires: [wire!.id] });
    await edit("nouveau fil", { op: "add_wire", a: { type: "pin", ...a }, b: { type: "pin", ...b }, bends });
  };

  /** A capture of the tutorial: the view that shows its elements first (the progression is in the cockpit's side
   * panel, the fiche in the cockpit's bottom dock, tab « Fiche », the multimeter in the Physical Lab), then every named
   * element must have a rectangle. */
  const capture = async (name: string, rects: string[], view: "cockpit" | "bench" | "as-is" = "cockpit") => {
    if (view === "cockpit") await showCockpit();
    if (view === "bench") await showBench();
    // the first element named is the panel the figure is about: in view (a long fiche sits below the fold)
    document.querySelector(rects[0]?.split("@")[0] ?? "")?.scrollIntoView({ block: "start" });
    await sleep(150);
    await shot(name, rects);
    const taken = ((ctx.report.shots as Record<string, { rects?: Record<string, unknown> }> | undefined)?.[name]?.rects) ?? {};
    const missing = rects.filter((r) => !taken[r]);
    if (missing.length) fail(`capture ${name} : élément(s) non affiché(s) ${missing.join(", ")}`);
  };

  // ----------------------------------------------------------------------------------------- the IDE

  const reflash = async (kind: "template" | "reference", revision: number) => {
    const editor = activeIde ?? fail("IDE intégré absent (scénario ide-progression-<id> attendu)");
    // the reboot watch is re-based after the flash: what happened before is checked now
    reboots.push(await ctx.noReboot());
    await stopBus();
    $('[data-show-view="code"]').click();
    await until("éditeur prêt", () => editor.ready, 10000);
    if (kind === "template") {
      await click("#ide-restore");
      await click("#ide-restore-confirm");
      await until("template restauré", () => !editor.busy, 30000);
    } else {
      const reference = await e2e.studentSource();
      // a revision changes the binary (a log line at init), never the decoding
      const marked = reference.c.includes('n7rt_log("student_decoder ready");')
        ? reference.c.replace('n7rt_log("student_decoder ready");', `n7rt_log("student_decoder ready");\n  n7rt_log("CAN Lab E2E r${revision}");`)
        : reference.c;
      editor.edit("student_decoder.c", marked);
      if (reference.h !== null) editor.edit("student_decoder.h", reference.h);
    }
    const operate = async (sel: string) => {
      await click(sel);
      await until("opération IDE démarrée", () => editor.busy, 5000).catch(() => undefined);
      await until("fin de l'opération IDE", () => !editor.busy, 180000);
    };
    await operate("#ide-build");
    if (!editor.state?.build?.ok) fail(editor.failure || "compilation IDE échouée");
    await operate("#ide-flash");
    if (!editor.state?.flash?.ok) fail(editor.failure || "programmation IDE échouée");
    const artifact = editor.state.build.artifact;
    await until("CRC de la carte = compilation", () => {
      // the image of the board as it reconnects: its family is unknown until its HELLO_ACK
      const board = status().board;
      const crc = board?.family ? imageFor(artifact, board.family)?.student_crc32 : undefined;
      return !!board?.student_crc32 && crc !== undefined && parseInt(board.student_crc32.replace(/^0x/i, ""), 16) === crc
        && status().state === "READY";
    }, 20000);
    await until("l'IDE confirme le CRC", () => $("#ide-match").textContent === "La carte exécute la compilation actuelle.", 5000);
    ctx.watchBoard();
    flashes.push({ at: kind === "template" ? "template" : `reference-r${revision}`, crc32: status().board!.student_crc32 });
    return status().board!.student_crc32;
  };

  // ----------------------------------------------------------------------------------------- verification

  const untilReady = async (what: string, intervalMs: number) => {
    const t0 = performance.now();
    for (;;) {
      const p = await state();
      if (p.ready) break;
      if (performance.now() - t0 > intervalMs + 30000) fail(`${what} : vérification toujours impossible — « ${p.blocked} »`);
      await sleep(500);
    }
    await until("bouton de vérification actif", () => !$<HTMLButtonElement>("#progress-verify").disabled, 5000);
  };
  /** Starts the verification with the real button and returns the verdict lines shown. */
  const verify = async (label: string, k: number, expect: (lines: string[]) => string | null): Promise<string[]> => {
    await untilReady(label, k === 4 ? QUALIFICATION_INTERVAL_MS : INTERVAL_MS);
    await click("#progress-verify");
    const running = k === 4 ? "Qualification en cours — " : "Essai en cours — ";
    try {
      await until(`« ${running}» affiché`, () => ($("#progress-blocked").textContent ?? "").startsWith(running), 15000);
    } catch {
      // what the panel says instead (a refusal of the verification is shown there)
      fail(`${label} : l'essai ne démarre pas — panneau « ${$("#progress-blocked").textContent ?? ""} », état ${JSON.stringify((await state()).blocked)}`);
    }
    if (k <= 3 && !fiche().hidden && fiche().querySelector<HTMLButtonElement>(".fiche-save")?.disabled === false) {
      fail("la fiche reste modifiable pendant l'essai");
    }
    const done = await untilProgress(`${label} : verdict`, (p) => !p.running, k === 4 ? 600000 : 300000);
    await until("verdict affiché", () => JSON.stringify(lines()) === JSON.stringify(done.feedback), 5000);
    attempts.push({ step: k, label, lines: done.feedback });
    const bad = expect(done.feedback);
    if (bad) fail(`${label} : ${bad} — affiché ${JSON.stringify(done.feedback)}`);
    return done.feedback;
  };
  const failed = (diagnostic: boolean, verified: boolean, nominal = false) => (got: string[]) => {
    const want = nominal
      ? ["Échec à corriger.", `Fiche nominale : ${diagnostic ? "conforme" : "non conforme"}`, `Essai nominal : ${verified ? "réussi" : "non réussi"}`, DETAIL_HIDDEN]
      : ["Échec à corriger.", `Diagnostic (fiche) : ${diagnostic ? "conforme" : "non conforme"}`, `Réparation (essai caché) : ${verified ? "vérifiée" : "non vérifiée"}`, DETAIL_HIDDEN];
    return JSON.stringify(got) === JSON.stringify(want) ? null : `attendu ${JSON.stringify(want)}`;
  };
  const passed = (next: string | null) => (got: string[]) => {
    if (!got[0]?.startsWith(PASSED)) return "pas de « Vérification réussie »";
    if (next !== null && got[1] !== next) return `ligne suivante attendue « ${next} »`;
    return null;
  };

  // ----------------------------------------------------------------------------------------- forms

  const nominalAnswer = (p: ProgressionView): NominalPart => {
    const part = structuredClone(answers.nominal);
    const doc = p.documents[0]?.id ?? fail("bibliothèque de la mission vide");
    part.documents_utilises = part.documents_utilises.map((d) => ({ ...d, document: doc }));
    return part;
  };
  const openFiche = async () => {
    if (fiche().hidden) await click("#progress-fiche-toggle");
    await until("fiche affichée", () => !fiche().hidden && !!fiche().querySelector(".fiche-save"), 5000);
  };
  const closeFiche = async () => {
    if (!fiche().hidden) await click("#progress-fiche-toggle");
    await until("fiche masquée", () => fiche().hidden, 3000);
  };
  const choose = (sel: string, value: string) => {
    const s = q<HTMLSelectElement>(sel);
    if (!Array.from(s.options).some((o) => o.value === value)) fail(`choix des réponses absent de ${sel}`);
    s.value = value;
    s.dispatchEvent(new Event("change", { bubbles: true }));
  };
  const type = (sel: string, value: string) => {
    const e = q<HTMLInputElement | HTMLTextAreaElement>(sel);
    e.value = value;
    e.dispatchEvent(new Event("input", { bubbles: true }));
    e.dispatchEvent(new Event("change", { bubbles: true }));
  };
  /** Types the incident part in the real form and saves it with « Enregistrer la fiche ». */
  const fillIncident = async (rank: number, part: IncidentPart) => {
    await openFiche();
    const kind = part.cible.type ?? fail("cible sans type dans les réponses");
    choose(".incident-target-kind", kind);
    if (kind === "CONDUCTEUR") {
      choose(".incident-pin-a", part.cible.conducteur.a ?? "");
      choose(".incident-pin-b", part.cible.conducteur.b ?? "");
    } else if (kind === "COMPOSANT") choose(".incident-part", part.cible.composant ?? "");
    else type(".incident-identifier", part.cible.identifiant ?? "");
    choose(".incident-category", part.categorie ?? "");
    choose(".incident-state", part.etat_observe ?? "");
    type(".incident-value", part.mesure_cle.valeur === null ? "" : String(part.mesure_cle.valeur).replace(".", ","));
    if (part.mesure_cle.unite) choose(".incident-unit", part.mesure_cle.unite);
    type(".fiche-justification", part.justification);
    await click("#progress-fiche .fiche-save");
    const want = JSON.stringify(canonical([part.cible, part.categorie, part.etat_observe, part.mesure_cle.valeur, part.mesure_cle.unite]));
    const t0 = performance.now();
    for (;;) {
      const saved = await mission.incidentGet(rank);
      if (saved && JSON.stringify(canonical([saved.cible, saved.categorie, saved.etat_observe, saved.mesure_cle.valeur, saved.mesure_cle.unite])) === want) return saved;
      // a save on a loaded machine can take seconds; a save that never lands still fails here
      if (performance.now() - t0 > 15000) fail(`fiche d'incident ${rank} non enregistrée : « ${fiche().querySelector(".fiche-status")?.textContent} »`);
      await sleep(100);
    }
  };
  // keys in a fixed order: the answers file (written by serde_json, keys sorted) and the application's view (keys in
  // field order) describe the same fiche
  const canonical = (v: unknown): unknown =>
    Array.isArray(v)
      ? v.map(canonical)
      : v !== null && typeof v === "object"
        ? Object.fromEntries(Object.keys(v).sort().map((k) => [k, canonical((v as Record<string, unknown>)[k])]))
        : v;
  const digest = async (value: unknown) => {
    const bytes = new TextEncoder().encode(JSON.stringify(canonical(value)));
    return Array.from(new Uint8Array(await crypto.subtle.digest("SHA-256", bytes)), (b) => b.toString(16).padStart(2, "0")).join("");
  };
  const refused = async (what: string, run: () => Promise<unknown>, want: string) => {
    try {
      await run();
    } catch (err) {
      if (String(err) !== want) fail(`${what} : refus « ${String(err)} », attendu « ${want} »`);
      return;
    }
    fail(`${what} accepté hors incident`);
  };

  // ----------------------------------------------------------------------------------------- E0

  const playE0 = async () => {
    await step("e0-etat-initial", async () => {
      const p = await untilProgress("progression affichée", (x) => x.readable, 10000);
      await until("panneau de progression affiché", () => shown("#progress-section"), 3000);
      const states = p.steps.map((s) => s.state);
      if (p.current !== 0 || JSON.stringify(states) !== JSON.stringify(["EN COURS", "À FAIRE", "À FAIRE", "À FAIRE", "À FAIRE"])) {
        fail(`état initial ${JSON.stringify(states)} (étape ${p.current})`);
      }
      if (p.button !== "Lancer la vérification" || shown("#progress-reproduce") || shown("#progress-final")) fail("boutons de l'E0 inattendus");
      await refused("consigner en E0", () => mission.consign(), "La mesure ne peut être consignée que pendant l'incident en cours, hors vérification.");
      await refused("reproduire en E0", () => mission.reproduce(), "« Reproduire l'anomalie » n'existe que pendant un incident non validé, hors vérification.");
      return { steps: states };
    });
    await capture("02-progression", ["#progress-section", "#progress-steps", "#progress-order", "#progress-fiche-toggle", "#progress-verify", "#progress-blocked"]);
    await step("e0-fiche-vide", async () => {
      await openFiche();
      await capture("03-fiche-nominale", ["#progress-fiche", ...[0, 1, 2, 3, 4, 5, 6].map((k) => `#progress-fiche .fiche-nav-item@${k}`), "#progress-fiche .fiche-save"]);
      // the empty form would be saved by the next verification: the mission is closed and reopened (the edits of an
      // open form belong to it), then the fiche is written through the command of its « Enregistrer »
      await closeFiche();
      await click("#student-close");
      await untilMission("mission fermée", (m) => !m.open, 10000);
      await ctx.openMission();
      return {};
    });
    await step("e0-echec-controle", async () => {
      const p = await state();
      const wrong = nominalAnswer(p);
      const d = wrong.decisions[0];
      wrong.decisions[0] = d === "REJETER" ? "PUBLIER" : "REJETER";
      await mission.ficheSet(wrong);
      await verify("E0 fiche fausse", 0, failed(false, true, true));
      await capture("04-verdict-e0", ["#progress-section", "#progress-feedback .feedback-line@0", "#progress-feedback .feedback-line@1", "#progress-feedback .feedback-line@2", "#progress-feedback .feedback-line@3"]);
      const after = await state();
      if (after.steps[0].state !== "ÉCHEC À CORRIGER") fail(`E0 après échec : ${after.steps[0].state}`);
      return { lines: attempts[attempts.length - 1].lines.length };
    });
    await step("e0-reussite", async () => {
      const p = await state();
      await mission.ficheSet(nominalAnswer(p));
      const saved = await mission.ficheGet();
      if ((await digest(saved)) !== (await digest(nominalAnswer(p)))) fail("fiche nominale relue ≠ fiche enregistrée");
      await verify("E0", 0, passed("Incident 1 ouvert. Le véhicule revient d'essai avec une anomalie."));
      const after = await state();
      if (after.steps[0].state !== "VALIDÉ" || after.current !== 1 || after.nominal_editable) fail("E0 non figée après validation");
      // validated: a controlled field (here DD-1) is frozen, the free texts stay editable (the same fiche saves again)
      const changed = nominalAnswer(p);
      changed.decisions[0] = changed.decisions[0] === "REJETER" ? "PUBLIER" : "REJETER";
      try {
        await mission.ficheSet(changed);
        fail("fiche nominale modifiable après validation");
      } catch (err) {
        if (err instanceof ProgressionFailure) throw err;
      }
      await mission.ficheSet(nominalAnswer(p));
      return {};
    });
  };

  // ----------------------------------------------------------------------------------------- E1–E3

  const wrongCategory = (p: ProgressionView, truth: string | null) =>
    p.incident?.categories.find((c) => c.value !== truth)?.value ?? fail("aucune autre catégorie");
  const nextLine = (k: number) => (k < 3 ? `Incident ${k + 1} ouvert. Le véhicule revient d'essai avec une anomalie.` : "Qualification finale ouverte.");
  /** The reading near the consigned value: the anomaly is in force. A voltage within three tolerances; a frame value
   * also within a quarter of itself (a mean over one second spreads from one second to the next). */
  const near = (inc: IncidentAnswer, got: number, consigned: number) => {
    const m = inc.measure;
    // the judge's tolerance at this value (MeasureSpec::tolerance)
    const judged = m.kind === "proportion" ? 1 : Math.max(m.tol_abs, m.tol_rel * Math.abs(consigned));
    const tol = m.kind === "proportion" ? 3 : m.kind === "voltage" ? 3 * judged : Math.max(3 * judged, 0.25 * Math.abs(consigned));
    return Math.abs(got - consigned) <= tol;
  };

  const firstAttempt = async (inc: IncidentAnswer) => {
    const k = inc.rank;
    let consigned = 0;
    await step(`e${k}-ouverture`, async () => {
      const p = await untilProgress(`incident ${k} en cours`, (x) => x.current === k, 10000);
      if (p.steps[k].state !== "EN COURS" || !p.order.startsWith(`Incident ${k}. Le véhicule revient d'essai avec une anomalie.`)) fail(`incident ${k} : ${p.order}`);
      if (p.button !== "Vérifier la réparation" || !(p.blocked ?? "").includes("Mesure clé : consigner la mesure")) fail(`incident ${k} : bouton ou blocage inattendu`);
      await until("bouton Reproduire affiché", () => shown("#progress-reproduce"), 3000);
      return { order: "Incident", steps: p.steps.map((s) => s.state) };
    });
    await ensureBus();
    if (k === 1) {
      await step("e1-multimetre", async () => {
        await meter(answers.neutral_meter.com, answers.neutral_meter.vohm);
        await capture("05-multimetre", [".meter-display", ".meter-function", ".meter-com", ".meter-vohm"], "bench");
        return {};
      });
    }
    await step(`e${k}-fiche`, async () => {
      await openFiche();
      if (k === 1) {
        await capture("06-fiche-incident", ["#progress-fiche", ...[".incident-target-kind", ".incident-category", ".incident-state", ".incident-value-row", ".fiche-justification"]
          .map((c) => `#progress-fiche ${c}`)]);
      }
      const p = await state();
      const part: IncidentPart = { ...structuredClone(inc.part), categorie: wrongCategory(p, inc.part.categorie) };
      part.mesure_cle = { valeur: null, unite: null, consignation: null };
      await fillIncident(k, part);
      const v = (await state()).incident!;
      if (!v.grandeur || !v.units.includes(inc.measure.unit)) fail("grandeur imposée absente ou sans l'unité des réponses");
      return {};
    });
    await step(`e${k}-consigner`, async () => {
      await conditions(inc);
      await ensureBus();
      await click("#progress-fiche .incident-consign");
      await until("mesure consignée affichée", () => (fiche().querySelector(".incident-consigned")?.textContent ?? "").startsWith("Mesure consignée le "), 10000);
      // the reading right after the recording, on the same frames or circuit
      consigned = inc.measure.kind === "voltage" ? await meter(inc.measure.reference, inc.measure.pin) : frameQuantity(inc.measure, sessionRows()) ?? fail("trames insuffisantes");
      if (k === 1) {
        await openFiche();
        await capture("07-consigner", ["#progress-fiche .incident-consign", "#progress-fiche .incident-consigned"]);
      }
      return {};
    });
    await step(`e${k}-echec-diagnostic`, async () => {
      const p = await state();
      const part: IncidentPart = { ...structuredClone(inc.part), categorie: wrongCategory(p, inc.part.categorie) };
      part.mesure_cle = { valeur: consigned, unite: inc.measure.unit, consignation: null };
      await fillIncident(k, part);
      // nothing repaired: a vehicle-side anomaly has nothing to repair and the hidden test passes with a right decoder
      await verify(`E${k} catégorie fausse, sans réparation`, k, failed(false, inc.repair.kind === "none"));
      if (k === 1) {
        await closeFiche();
        await capture("08-verdict-incident", ["#progress-section", "#progress-feedback .feedback-line@0", "#progress-feedback .feedback-line@1", "#progress-feedback .feedback-line@2", "#progress-feedback .feedback-line@3"]);
      }
      return {};
    });
    return consigned;
  };

  const finishIncident = async (inc: IncidentAnswer, consigned: number) => {
    const k = inc.rank;
    await ensureBus();
    if (inc.repair.kind !== "none") {
      await step(`e${k}-reparation-valeur-fausse`, async () => {
        await repair(inc.repair);
        const repaired = await observe(inc.measure);
        if (near(inc, repaired, consigned) && inc.measure.kind !== "proportion") fail("la réparation ne change pas la mesure clé");
        const part: IncidentPart = structuredClone(inc.part);
        part.mesure_cle = { valeur: consigned === 0 ? 1 : consigned * 3, unite: inc.measure.unit, consignation: null };
        await fillIncident(k, part);
        await verify(`E${k} réparé, valeur fausse`, k, failed(false, true));
        return {};
      });
      await step(`e${k}-reproduire`, async () => {
        await closeFiche();
        await showCockpit();
        await click("#progress-reproduce");
        await until("dialogue Reproduire ouvert", () => $<HTMLDialogElement>("#reproduce-dialog").open, 3000);
        if (k === 1) await capture("09-reproduire", ["#progress-reproduce", "#reproduce-dialog", "#reproduce-confirm"], "as-is");
        await click("#reproduce-confirm");
        reproductions += 1;
        await until("dialogue fermé", () => !$<HTMLDialogElement>("#reproduce-dialog").open, 3000);
        await sleep(1500);
        await conditions(inc);
        const again = await observe(inc.measure);
        if (!near(inc, again, consigned)) fail("l'anomalie n'est pas revenue après « Reproduire l'anomalie »");
        await repair(inc.repair);
        return { reproductions };
      });
    } else if (activeIde && !codeChanged) {
      codeChanged = true;
      await step(`e${k}-code-modele`, async () => {
        await reflash("template", 0);
        await showCockpit();
        const part: IncidentPart = structuredClone(inc.part);
        part.mesure_cle = { valeur: consigned, unite: inc.measure.unit, consignation: null };
        await fillIncident(k, part);
        await verify(`E${k} code du modèle`, k, failed(true, false));
        await reflash("reference", 2);
        await ensureBus();
        return { flashes: flashes.length };
      });
    }
    await step(`e${k}-reussite`, async () => {
      const part: IncidentPart = structuredClone(inc.part);
      // a voltage in millivolts: the unit is normalised before the comparison
      const milli = inc.measure.kind === "voltage" && (await state()).incident?.units.includes("mV");
      part.mesure_cle = { valeur: milli ? Math.round(consigned * 1000 * 10) / 10 : consigned, unite: milli ? "mV" : inc.measure.unit, consignation: null };
      await fillIncident(k, part);
      await verify(`E${k}`, k, passed(nextLine(k)));
      const after = await state();
      if (after.steps[k].state !== "VALIDÉ") fail(`E${k} non validée`);
      return { unit_normalised: !!milli };
    });
  };

  // ----------------------------------------------------------------------------------------- resume

  const resumeChecks = async (inc: IncidentAnswer) => {
    await step("reprise-etat", async () => {
      const p = await untilProgress("progression reprise", (x) => x.readable, 10000);
      const states = p.steps.map((s) => s.state);
      if (p.current !== 1 || states[0] !== "VALIDÉ" || states[1] !== "ÉCHEC À CORRIGER") fail(`reprise : ${JSON.stringify(states)} (étape ${p.current})`);
      if (!p.incident?.consigned) fail("reprise : la mesure consignée est perdue");
      const saved = await mission.incidentGet(1);
      if (!saved || saved.mesure_cle.valeur === null) fail("reprise : la fiche d'incident est perdue");
      progressionReport.resume_part_sha256 = await digest(saved);
      await ensureBus();
      await conditions(inc);
      const reading = await observe(inc.measure);
      if (!near(inc, reading, saved!.mesure_cle.valeur!)) fail("reprise : l'anomalie n'est plus sur le banc");
      return { steps: states, consigned: true };
    });
    return (await mission.incidentGet(1))!.mesure_cle.valeur!;
  };

  // ----------------------------------------------------------------------------------------- E4, E5

  const playE4E5 = async () => {
    await step("e4-qualification", async () => {
      const p = await untilProgress("qualification ouverte", (x) => x.current === 4, 10000);
      if (p.button !== "Lancer la qualification") fail(`bouton « ${p.button} »`);
      if (shown("#progress-reproduce")) fail("« Reproduire l'anomalie » en qualification");
      await ensureBus();
      const got = await verify("E4", 4, (l) => {
        const cps = l.filter((x) => x.startsWith("CP-"));
        if (cps.length !== 8) return `${cps.length} lignes CP, 8 attendues`;
        if (!cps.every((x) => x.endsWith(" : PASS"))) return "une compétence en FAIL";
        if (!l.includes("Mission clôturée.")) return "mission non clôturée";
        return null;
      });
      return { lines: got.length };
    });
    await capture("10-qualification", ["#progress-section", "#progress-feedback .feedback-line@0", "#progress-feedback .feedback-line@1", "#progress-feedback .feedback-line@2", "#progress-feedback .feedback-line@3", "#progress-feedback .feedback-line@4", "#progress-feedback .feedback-line@5", "#progress-feedback .feedback-line@6", "#progress-feedback .feedback-line@7", "#progress-feedback .feedback-line@8", "#progress-feedback .feedback-line@9"]);
    await step("e5-cloture", async () => {
      const p = await untilProgress("mission clôturée", (x) => x.current === null, 10000);
      if (!p.steps.every((s) => s.state === "VALIDÉ")) fail(`clôture : ${JSON.stringify(p.steps.map((s) => s.state))}`);
      if (!p.feedback.includes("La fiche finale est exportée dans le dossier Documents.")) fail("fiche finale non exportée");
      await until("bouton « Ouvrir la fiche finale »", () => shown("#progress-final") && !shown("#progress-verify"), 5000);
      try {
        await mission.ficheSet(answers.nominal);
        fail("fiche modifiable après la clôture");
      } catch (err) {
        if (err instanceof ProgressionFailure) throw err;
      }
      const m = await mission.state();
      progressionReport.closed = { mission: m.id, group: m.group, reproductions };
      return { steps: p.steps.map((s) => s.state) };
    });
    await capture("11-cloture", ["#progress-section", "#progress-steps", "#progress-feedback", "#progress-final"]);
  };

  // ----------------------------------------------------------------------------------------- the flow

  const [first, ...rest] = answers.incidents;
  if (!first) fail("réponses de test sans incident");
  let consigned: number;
  if (stage === "resume") {
    consigned = await resumeChecks(first);
  } else {
    await playE0();
    consigned = await firstAttempt(first);
    if (stage === "until-incident") {
      progressionReport.resume_part_sha256 = await digest(await mission.incidentGet(1));
      progressionReport.stopped_at = { step: 1, attempts: attempts.filter((a) => a.step === 1).length };
      await showCockpit();
      return;
    }
  }
  await finishIncident(first, consigned);
  for (const inc of rest) {
    const v = await firstAttempt(inc);
    await finishIncident(inc, v);
  }
  await playE4E5();
  await step("sans-redemarrage", ctx.noReboot);
  if (activeIde) {
    await step("carte-modele", async () => {
      await reflash("template", 0);
      await showCockpit();
      if (status().session) fail("session active après la remise du modèle");
      if (status().state !== "READY" || !status().board?.runtime_verified) fail(`carte : ${status().state}`);
      return { crc32: status().board!.student_crc32 };
    });
  }
}
