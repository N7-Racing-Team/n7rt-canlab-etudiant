// Student progression of a release 0.3 group pack (private spec ch. 24 §7, adapted without server): the six step
// boxes (Essai nominal, Incident 1–3, Qualification finale, Clôture) with their exact states, the work order of the
// current step, its button, the message that blocks it, the lines of the last verdict, and the nominal fiche.
//
// Rules: every text comes from the core as it is (states, orders, messages) or is a label of the public fiche schema;
// text through textContent only (ui/DESIGN.md §8); no score, no counter, no percentage (ch. 24 §7.1); nothing here
// knows a frame, a signal or an expected value: the form only checks shapes (hexadecimal on 11 bits, integers,
// finite numbers), the core checks the rest at the verification.

import { clear, el, setText } from "../dom";
import type {
  BitField, BitOrder, Decision, FrameEntry, IncidentPart, NominalPart, ProgressionView, Sign, SignalEntry, StatusEntry, StepView,
} from "../types";
import { captureFocus, restoreFocus } from "./focus";
import { DIRTY, field, FIX_FIELDS, FORMAT, isInvalid, mark, revealField, SAVED_FIRST, type FormUi } from "./form";
import { emptyIncident, IncidentForm } from "./incident";

export const ORDERS: BitOrder[] = ["INTEL", "MOTOROLA"];
export const SIGNS: Sign[] = ["NON_SIGNÉ", "COMPLÉMENT_À_DEUX"];
/** DD-1…DD-4 and the values each one accepts (public schema, ch. 24 §3.3). */
export const DECISIONS: { label: string; values: Decision[] }[] = [
  { label: "DD-1 Valeur égale au code invalide", values: ["REJETER", "PUBLIER"] },
  { label: "DD-2 Trame à protection non conforme", values: ["IGNORER_SES_DONNÉES", "UTILISER_SES_DONNÉES"] },
  { label: "DD-3 Valeur hors plage sous statut OK", values: ["REJETER", "BORNER", "PUBLIER"] },
  { label: "DD-4 Donnée plus vieille que la péremption", values: ["REJETER", "PUBLIER_LA_DERNIÈRE"] },
];
export const FRAME_FORMAT = "STANDARD_11_BITS";
export const FREE_TEXT_NOTE = "non évaluée automatiquement";

/** CSS class of a step box by its displayed state (neutral constant colours, ch. 24 §7.1). */
export function stepClass(s: StepView): string {
  const state = {
    "À FAIRE": "todo",
    "EN COURS": "open",
    "VALIDÉ": "passed",
    "ÉCHEC À CORRIGER": "failed",
  }[s.state] ?? "todo";
  return `step step-${state}${s.current ? " step-current" : ""}`;
}

// ------------------------------------------------------------------------------------------------- form values

/** An empty field is `null`; a value that is not of the expected shape is `undefined` (refused before saving). */
export function parseIdentifier(text: string): number | null | undefined {
  const t = text.trim();
  if (!t) return null;
  const m = /^0x([0-9a-f]{1,3})$/i.exec(t);
  if (!m) return undefined;
  const v = parseInt(m[1], 16);
  return v <= 0x7ff ? v : undefined;
}

export function formatIdentifier(v: number | null): string {
  return v === null ? "" : `0x${v.toString(16).toUpperCase().padStart(3, "0")}`;
}

export function parseInteger(text: string, min: number, max: number): number | null | undefined {
  const t = text.trim();
  if (!t) return null;
  if (!/^-?\d+$/.test(t)) return undefined;
  const v = Number(t);
  return Number.isSafeInteger(v) && v >= min && v <= max ? v : undefined;
}

export function parseNumber(text: string): number | null | undefined {
  const t = text.trim().replace(",", ".");
  if (!t) return null;
  const v = Number(t);
  return Number.isFinite(v) && /^[-+]?(\d+\.?\d*|\.\d+)(e[-+]?\d+)?$/i.test(t) ? v : undefined;
}

/** « 0, 1 » → [0, 1]; empty → []; anything else is refused. */
export function parseSlots(text: string): number[] | undefined {
  const t = text.trim();
  if (!t) return [];
  const parts = t.split(/[\s,;]+/).filter(Boolean);
  const out = parts.map((p) => parseInteger(p, 0, 31));
  return out.every((v): v is number => typeof v === "number") ? out : undefined;
}

/** `code_invalide`: an integer, « AUCUN », or empty. */
export function parseInvalidCode(text: string): number | "AUCUN" | null | undefined {
  const t = text.trim();
  if (!t) return null;
  if (t.toUpperCase() === "AUCUN") return "AUCUN";
  if (/^0x[0-9a-f]+$/i.test(t)) {
    const v = parseInt(t.slice(2), 16);
    return Number.isSafeInteger(v) ? v : undefined;
  }
  return parseInteger(t, -(2 ** 52), 2 ** 52);
}

export function emptyBitField(): BitField {
  return { ordre: null, lsb_bit: null, longueur: null };
}

export function emptyFrame(): FrameEntry {
  return {
    identifiant: null, format: null, dlc: null, periode_ms: null,
    protection: { presente: null, crc: emptyBitField(), compteur: emptyBitField() },
    compteur_de_vie: emptyBitField(),
  };
}

export function emptySignal(): SignalEntry {
  return {
    nom: "", identifiant: null, slots: [], ordre: null, lsb_bit: null, longueur: null, signe: null, facteur: null, offset: null,
    code_invalide: null,
  };
}

export function emptyStatus(): StatusEntry {
  return { identifiant: null, slots: [], ordre: null, lsb_bit: null, longueur: null, code_ok: null };
}

export function emptyPart(): NominalPart {
  return {
    documents_utilises: [], trames: [], signaux: [], statuts: [],
    supervision: { premier_timeout_ms: null, timeout_ms: null, peremption_ms: null, pertes_consecutives_tolerees: null },
    decisions: [null, null, null, null], justification: "",
  };
}

// --------------------------------------------------------------------------------------------- the verdict

// Public texts of the core's verdict lines (workflow/session.rs), pinned by progress.test.ts: a rewording there fails
// here first. They only pick a presentation (kind, tone); the lines are always shown exactly as given.
export const PASSED_PREFIX = "Vérification réussie.";
export const FAILED_HEAD = "Échec à corriger.";
export const INTERRUPTED_PREFIX = "Vérification interrompue";
export const DETAIL_HIDDEN = "Le détail n'est pas communiqué. Reprenez vos mesures et vos essais.";
export const CLOSED_LINE = "Mission clôturée.";
export const EXPORTED = "La fiche finale est exportée dans le dossier Documents.";
export const NOT_EXPORTED = "La fiche finale n'a pas pu être exportée : utilisez « Ouvrir la fiche finale » pour réessayer.";
export const NOT_SAVED = "La progression n'a pas pu être enregistrée sur ce poste : prévenez l'encadrant.";
/** The labels of the « label : value » lines of a failure (E0, then E1–E3). */
const PART_LABELS = ["Fiche nominale", "Essai nominal", "Diagnostic (fiche)", "Réparation (essai caché)"];
const CP_LINE = /^(CP-\d+ — .+) : (PASS|FAIL)$/;
const NEXT_LINE = /^(Incident [1-3] ouvert\. |Qualification finale ouverte\.$)/;
const PART_SEP = " : ";

export type Tone = "ok" | "warn" | "info" | "err";
export type LineKind = "head" | "part" | "next" | "hint" | "closure" | "export" | "cp" | "text";
export interface FeedbackLine {
  text: string;
  kind: LineKind;
  tone?: Tone;
  /** label, separator, value: their concatenation is `text`. */
  parts?: [string, string, string];
}
export interface FeedbackModel {
  /** The block's tone; null when nothing tells it (unknown text stays neutral, never a wrong colour). */
  tone: Tone | null;
  lines: FeedbackLine[];
}

function feedbackLine(text: string, first: boolean): FeedbackLine {
  const cp = CP_LINE.exec(text);
  if (cp) return { text, kind: "cp", tone: cp[2] === "PASS" ? "ok" : "warn", parts: [cp[1], PART_SEP, cp[2]] };
  if (text === CLOSED_LINE) return { text, kind: "closure", tone: "ok" };
  if (text === EXPORTED) return { text, kind: "export", tone: "ok" };
  if (text === NOT_EXPORTED) return { text, kind: "export", tone: "err" };
  if (text === NOT_SAVED) return { text, kind: "text", tone: "err" };
  if (text === DETAIL_HIDDEN) return { text, kind: "hint" };
  if (first && text.startsWith(PASSED_PREFIX)) return { text, kind: "head", tone: "ok" };
  if (first && text === FAILED_HEAD) return { text, kind: "head", tone: "warn" };
  if (first && text.startsWith(INTERRUPTED_PREFIX)) return { text, kind: "head", tone: "info" };
  const label = PART_LABELS.find((l) => text.startsWith(l + PART_SEP));
  if (label) {
    const value = text.slice(label.length + PART_SEP.length);
    return { text, kind: "part", tone: value.startsWith("non ") ? "warn" : "ok", parts: [label, PART_SEP, value] };
  }
  if (NEXT_LINE.test(text)) return { text, kind: "next" };
  return { text, kind: "text" };
}

/** How the verdict lines are presented. The block's tone: an unsaved progression is an error and an interruption is
 * not a verdict; then the step data (the current step to correct, or every step validated); then the public headline. */
export function feedbackModel(lines: string[], steps: StepView[], current: number | null): FeedbackModel {
  const model = lines.map((t, i) => feedbackLine(t, i === 0));
  const cps = model.filter((l) => l.kind === "cp");
  let tone: Tone | null = null;
  if (lines.includes(NOT_SAVED)) tone = "err";
  else if (lines[0]?.startsWith(INTERRUPTED_PREFIX)) tone = "info";
  else if (current !== null && steps[current]?.state === "ÉCHEC À CORRIGER") tone = "warn";
  else if (current === null && steps.length > 0 && steps.every((s) => s.state === "VALIDÉ")) tone = "ok";
  else if (lines[0]?.startsWith(PASSED_PREFIX)) tone = "ok";
  else if (lines[0] === FAILED_HEAD) tone = "warn";
  else if (cps.length) tone = cps.every((l) => l.tone === "ok") && lines.includes(CLOSED_LINE) ? "ok" : "warn";
  else if (lines.includes(NOT_EXPORTED)) tone = "err";
  return { tone, lines: model };
}

/** The status-bar segment of the progression (shown in every view): the current step and its state, in its tone. */
export function progressStatus(v: ProgressionView | null): { text: string; tone: "" | "ok" | "warn" | "accent"; title: string } | null {
  if (!v) return null;
  const c = v.current;
  const step = c === null ? null : v.steps[c];
  const text = c === null ? "E5 · Clôture" : `E${c} · ${step?.name ?? ""} — ${step?.state ?? ""}`;
  const tone = v.running ? "accent" : step?.state === "ÉCHEC À CORRIGER" ? "warn" : c === null ? "ok" : "";
  return { text, tone, title: v.blocked ?? "" };
}

// ---------------------------------------------------------------------------------------------- the panel

export interface ProgressElements {
  section: HTMLElement;
  steps: HTMLElement;
  order: HTMLElement;
  verify: HTMLButtonElement;
  blocked: HTMLElement;
  feedback: HTMLElement;
  ficheToggle: HTMLButtonElement;
  fiche: HTMLElement;
  /** « Ouvrir la fiche finale », once the mission is closed. */
  final: HTMLButtonElement;
  /** « Reproduire l'anomalie » (R-11) and its confirmation (M-18), during an incident step. */
  reproduce: HTMLButtonElement;
  reproduceDialog: HTMLDialogElement;
  reproduceConfirm: HTMLButtonElement;
  reproduceCancel: HTMLButtonElement;
  /** « E1 · Incident 1 » in the head of the consigne. */
  briefStep?: HTMLElement;
  /** The « Fiche » tab of the bottom dock: shown while a progression is open. */
  ficheTab?: HTMLElement;
  /** Under the button, after the core's message: what the fiche form means for the verification (FIX_FIELDS,
   * SAVED_FIRST). */
  hint?: HTMLElement;
  /** E5, the export of the final fiche failed: the alert and its « Réessayer l'export ». */
  exportAlert?: HTMLElement;
  exportRetry?: HTMLButtonElement;
}

export interface ProgressActions {
  /** Starts the verification; rejects with the core's refusal (M-06, M-08, …). */
  verify(): Promise<void>;
  loadFiche(): Promise<NominalPart>;
  saveFiche(part: NominalPart): Promise<void>;
  /** The incident part `rank` (1–3), once its incident is active. */
  loadIncident(rank: number): Promise<IncidentPart | null>;
  saveIncident(part: IncidentPart): Promise<void>;
  /** « Consigner la mesure » of the incident in progress. */
  consign(): Promise<void>;
  /** « Reproduire l'anomalie » (R-11), confirmed. */
  reproduce(): Promise<void>;
  /** Opens the final fiche of the closed mission (the core exports it again when needed: mission_fiche_open is also
   * the retry of a failed export, « Réessayer l'export »). */
  openFinal(): Promise<void>;
  /** The fiche was shown: its dock tab comes forward (and the dock back, if it was hidden). */
  showFiche?(): void;
  /** The fiche was closed: the dock goes back to the tab it showed before. */
  hideFiche?(): void;
  /** Brings an element into view, and focuses it when asked (the panel itself never scrolls nor focuses); `start`:
   * its top just under a sticky head. */
  reveal?(el: HTMLElement, focus: boolean, start?: boolean): void;
}

export const SHAPE_REFUSED = "Certains champs ne sont pas au bon format : corrigez-les avant d'enregistrer.";
/** The fiche box's state of a closed mission whose export failed (workflow/session.rs fiche_state). */
export const EXPORT_PENDING = "Clôturée, exportation à reprendre";
/** The two sentences that open the core's missing-fields message (workflow/session.rs, verification preconditions):
 * its list « A, B, C. » is drawn as a list, its text unchanged. */
export const MISSING_LEAD = "Complétez la fiche de cette étape avant la vérification. ";
export const MISSING_LABEL = "Champs à renseigner : ";

/** The fields named by the core's missing-fields message, or null for any other message. */
export function missingFields(message: string): string[] | null {
  const head = MISSING_LEAD + MISSING_LABEL;
  if (!message.startsWith(head) || !message.endsWith(".") || message.length <= head.length + 1) return null;
  return message.slice(head.length, -1).split(", ");
}

/** What a field name shares with its neighbours in the list: « Supervision : », « Trame 2 : », « Décision ». */
function fieldGroup(name: string): string {
  const colon = name.indexOf(" : ");
  if (colon >= 0) return name.slice(0, colon + 3);
  const space = name.lastIndexOf(" ");
  return space > 0 ? name.slice(0, space + 1) : "";
}

/** The missing-fields message as a list: one line per group of fields (« Supervision : premier timeout, timeout »),
 * the repeated group names and the separators kept in visually hidden spans, so that the text stays the core's. */
function drawMissing(target: HTMLElement, fields: string[]): void {
  target.appendChild(el("span", "pb-lead", MISSING_LEAD));
  target.appendChild(el("span", "pb-label", MISSING_LABEL));
  const groups: { name: string; fields: string[] }[] = [];
  for (const f of fields) {
    const g = fieldGroup(f);
    const last = groups[groups.length - 1];
    if (last && g && last.name === g) last.fields.push(f);
    else groups.push({ name: g, fields: [f] });
  }
  const list = el("ul", "pb-list");
  groups.forEach((g, i) => {
    const item = el("li", "pb-item");
    g.fields.forEach((f, k) => {
      if (k === 0) {
        item.appendChild(el("span", undefined, f));
        return;
      }
      item.appendChild(el("span", undefined, ", "));
      item.appendChild(el("span", "sr", g.name));
      item.appendChild(el("span", undefined, f.slice(g.name.length)));
    });
    item.appendChild(el("span", "sr", i < groups.length - 1 ? ", " : "."));
    list.appendChild(item);
  });
  target.appendChild(list);
}

export class ProgressPanel {
  private view: ProgressionView | null = null;
  private identity = "";
  private stepsKey = "";
  private form: FicheForm | null = null;
  private incidentForm: IncidentForm | null = null;
  private busy = false;
  private refusal = "";
  /** The verdict lines drawn (null before the first view): they are rebuilt only when they change. */
  private feedbackKey: string | null = null;
  private wasRunning = false;
  /** A fiche being loaded for its dock tab: a second request waits for it. */
  private opening = false;
  /** « Réessayer l'export » on its way. */
  private exporting = false;
  /** The message drawn under the button (rebuilt only when it changes). */
  private blockedText: string | null = null;

  constructor(private e: ProgressElements, private actions: ProgressActions) {}

  /** What the forms tell the panel: they follow the verification button. */
  private formUi(): FormUi {
    return { reveal: this.actions.reveal, changed: () => this.render() };
  }

  /** The open form (nominal or incident) holds a wrongly shaped field. */
  private get formErrors(): boolean {
    return !!(this.form?.hasErrors || this.incidentForm?.hasErrors);
  }

  /** The open form holds unsaved changes. */
  private get formChanges(): boolean {
    return !!(this.form?.hasChanges || this.incidentForm?.hasChanges);
  }

  /** The open form's unsaved changes, which closing the window or the mission must not lose (views/unsaved.ts). */
  get unsaved(): boolean {
    return this.formChanges;
  }

  /** Saves the open form as « Enregistrer la fiche » does; true when nothing unsaved is left. */
  async saveUnsaved(): Promise<boolean> {
    for (const f of [this.form, this.incidentForm]) if (f?.hasChanges && !(await f.saveNow())) return false;
    return !this.formChanges;
  }

  /** E5 and the final fiche not exported (the core's export line, or the fiche box after a restart). */
  private exportFailed(v: ProgressionView): boolean {
    return v.current === null && (v.feedback.includes(NOT_EXPORTED) || v.fiche_state === EXPORT_PENDING);
  }

  mount(): void {
    this.e.verify.addEventListener("click", () => void this.verify());
    this.e.ficheToggle.addEventListener("click", () => void this.toggleFiche());
    this.e.reproduce.addEventListener("click", () => {
      if (!this.e.reproduceDialog.open) this.e.reproduceDialog.showModal();
    });
    this.e.reproduceCancel.addEventListener("click", () => this.e.reproduceDialog.close());
    this.e.reproduceConfirm.addEventListener("click", () => {
      this.e.reproduceDialog.close();
      this.actions
        .reproduce()
        .then(() => (this.refusal = ""))
        .catch((err) => (this.refusal = String(err)))
        .finally(() => this.render());
    });
    this.e.final.addEventListener("click", () => {
      this.actions.openFinal().catch((err) => {
        this.refusal = String(err);
        this.render();
      });
    });
    // the core has no export-only command: opening the final fiche exports it again when it is missing
    this.e.exportRetry?.addEventListener("click", () => {
      if (this.exporting) return;
      this.exporting = true;
      this.render();
      this.actions
        .openFinal()
        .then(() => (this.refusal = ""))
        .catch((err) => (this.refusal = String(err)))
        .finally(() => {
          this.exporting = false;
          this.render();
        });
    });
    this.render();
  }

  /** `identity` names the open pack (mission and group): another pack closes the fiche, whose edits belong to it. */
  setView(v: ProgressionView | null, identity = ""): void {
    if (!v || identity !== this.identity) this.closeFiche();
    // the open form belongs to a step: another step (an incident opened or validated) closes it
    const formStep = this.incidentForm ? this.incidentForm.rank : this.form ? 0 : null;
    const viewStep = v?.incident ? v.incident.rank : 0;
    if (formStep !== null && formStep !== viewStep) this.closeFiche();
    if (JSON.stringify(v) !== JSON.stringify(this.view)) this.refusal = "";
    this.identity = identity;
    this.view = v;
    this.render();
    // once the mission is closed (E5) the whole fiche is read-only: the core refuses every save
    this.form?.setEditable(!!v?.nominal_editable && !v?.running, !v?.nominal_editable, !!v?.running, v?.current === null);
    this.form?.setDocuments(v?.documents ?? []);
    if (v?.incident) this.incidentForm?.setView(v.incident, !!v.running);
  }

  /** The verification judges the saved fiche: an open form is read and saved first (a wrong shape stops here). */
  private async verify(): Promise<void> {
    if (!this.view?.ready || this.busy || this.formErrors) return;
    this.busy = true;
    this.render();
    try {
      // a form hidden with « Fermer la fiche » keeps its edits: they are saved too, the verification judges them
      // (a wrongly shaped field is marked in the form, which brings it forward when it is shown)
      const shown = !this.e.fiche.hidden;
      if (this.form) {
        if (!this.form.read()) {
          this.form.refuse(shown);
          this.refusal = SHAPE_REFUSED;
          return;
        }
        const rev = this.form.revision;
        await this.actions.saveFiche(this.form.current());
        this.form.markSavedAt(rev);
      }
      if (this.incidentForm) {
        if (!this.incidentForm.read()) {
          this.incidentForm.refuse(shown);
          this.refusal = SHAPE_REFUSED;
          return;
        }
        const rev = this.incidentForm.revision;
        await this.actions.saveIncident(this.incidentForm.current());
        this.incidentForm.markSavedAt(rev);
      }
      await this.actions.verify();
      this.refusal = "";
    } catch (err) {
      this.refusal = String(err);
    } finally {
      this.busy = false;
      this.render();
    }
  }

  /** The form is destroyed (another pack, another step, no progression); a shown fiche gives its dock tab back. */
  private closeFiche(): void {
    const shown = !this.e.fiche.hidden;
    this.form?.dispose();
    this.form = null;
    this.incidentForm = null;
    clear(this.e.fiche);
    this.e.fiche.hidden = true;
    setText(this.e.ficheToggle, "Ouvrir la fiche");
    if (shown) this.actions.hideFiche?.();
  }

  private async toggleFiche(): Promise<void> {
    if (!this.e.fiche.hidden) {
      // the form is kept: closing it is not losing its edits
      this.e.fiche.hidden = true;
      setText(this.e.ficheToggle, "Ouvrir la fiche");
      this.emphasis();
      this.actions.hideFiche?.();
      return;
    }
    const v = this.view;
    // a part that cannot be read is never shown empty: the next verification would save the empty form over it
    const refuse = (err: unknown): void => {
      this.refusal = String(err);
      this.render();
      // the dock tab, if it was chosen to show the fiche, gives way
      this.actions.hideFiche?.();
    };
    this.opening = true;
    try {
      if (v?.incident && !this.incidentForm) {
        const rank = v.incident.rank;
        let saved: IncidentPart | null;
        try {
          saved = await this.actions.loadIncident(rank);
        } catch (err) {
          refuse(err);
          return;
        }
        this.incidentForm = new IncidentForm(
          this.e.fiche,
          { save: (p) => this.actions.saveIncident(p), consign: () => this.actions.consign() },
          this.formUi(),
        );
        this.incidentForm.render(saved ?? emptyIncident(rank), v.incident, v.running);
      } else if (!v?.incident && !this.form) {
        let part: NominalPart;
        try {
          part = await this.actions.loadFiche();
        } catch (err) {
          refuse(err);
          return;
        }
        this.form = new FicheForm(this.e.fiche, (p) => this.actions.saveFiche(p), this.formUi());
        const editable = !!v?.nominal_editable && !v?.running;
        this.form.render(part, v?.documents ?? [], editable, !v?.nominal_editable);
        this.form.setEditable(editable, !v?.nominal_editable, !!v?.running, v?.current === null);
      }
    } finally {
      this.opening = false;
    }
    // shown and labelled before the dock is asked to show it: the tab change that follows finds it open
    this.e.fiche.hidden = false;
    setText(this.e.ficheToggle, "Fermer la fiche");
    this.emphasis();
    this.actions.showFiche?.();
  }

  /** The dock switched tab: the « Fiche » tab opens the fiche (loading it if needed); another tab hides it, keeping
   * the form and its edits, without asking the dock anything back. */
  tabShown(fiche: boolean): void {
    if (fiche && this.e.fiche.hidden) {
      if (this.view && !this.opening) void this.toggleFiche();
    } else if (!fiche && !this.e.fiche.hidden) {
      this.e.fiche.hidden = true;
      setText(this.e.ficheToggle, "Ouvrir la fiche");
      this.emphasis();
    }
  }

  /** « Consigner la mesure » from the Physical Lab's multimeter: like the fiche's button (the open incident form is
   * read and saved first), else the measure only. Rejects with the refusal, which the form shows too. */
  consignFromBench(): Promise<void> {
    return this.incidentForm ? this.incidentForm.consignExternal() : this.actions.consign();
  }

  /** One violet at a time: when the verification cannot run now (not ready, not running, an editable part, fiche
   * closed), working on the fiche is the useful action, so the toggle takes the primary emphasis. */
  private emphasis(): void {
    const v = this.view;
    if (!v) return;
    // (not while a verification is being requested: the toggle would flash violet under the click)
    const unavailable = v.current !== null && !v.running && (!v.ready || this.formErrors);
    const editable = v.incident ? v.incident.editable : v.nominal_editable;
    const want = unavailable && this.e.fiche.hidden && editable ? "btn btn--primary ico ico--doc" : "btn ico ico--doc";
    if (this.e.ficheToggle.className !== want) this.e.ficheToggle.className = want;
  }

  private render(): void {
    const e = this.e;
    const v = this.view;
    e.section.hidden = !v;
    if (e.ficheTab) e.ficheTab.hidden = !v;
    if (!v) {
      this.feedbackKey = null;
      this.wasRunning = false;
      return;
    }
    const key = JSON.stringify([v.steps, v.fiche_state]);
    if (key !== this.stepsKey) {
      this.stepsKey = key;
      clear(e.steps);
      for (const s of v.steps) {
        const box = el("div", stepClass(s));
        box.role = "listitem";
        if (s.current) box.ariaCurrent = "step";
        box.appendChild(el("div", "step-name", s.name));
        box.appendChild(el("div", "step-state", s.state));
        if (s.since) box.appendChild(el("div", "step-since", s.since));
        e.steps.appendChild(box);
      }
      // the sixth box of ch. 24 §7.1: the fiche and its state
      const fiche = el("div", "step step-fiche");
      fiche.role = "listitem";
      fiche.appendChild(el("div", "step-name", "Fiche"));
      fiche.appendChild(el("div", "step-state", v.fiche_state));
      e.steps.appendChild(fiche);
    }
    if (e.briefStep) setText(e.briefStep, v.current === null ? "E5 · Clôture" : `E${v.current} · ${v.steps[v.current]?.name ?? ""}`);
    setText(e.order, v.order);
    setText(e.verify, v.button);
    // the form gates the button too: a wrongly shaped field would stop the verification anyway (SHAPE_REFUSED)
    const errors = this.formErrors;
    // a format refusal holds while the form still shows a wrong field, not once it is corrected
    if (this.refusal === SHAPE_REFUSED && !errors) this.refusal = "";
    e.verify.disabled = !v.ready || this.busy || errors;
    e.verify.hidden = v.current === null;
    // the mission is closed once no step is current (E5): its final fiche can be opened
    e.final.hidden = v.current !== null || !v.readable;
    // a failed export: the alert and « Réessayer l'export » take the emphasis, « Ouvrir la fiche finale » stays (the
    // core's export line names it) without the violet
    const exportFailed = !!e.exportAlert && this.exportFailed(v);
    if (e.exportAlert) e.exportAlert.hidden = !exportFailed;
    if (e.exportRetry) e.exportRetry.disabled = this.exporting;
    const finalClass = exportFailed ? "btn ico ico--doc" : "btn btn--primary ico ico--doc";
    if (e.final.className !== finalClass) e.final.className = finalClass;
    // R-11 in the three incident steps alike (DC-24-29: its presence never tells the nature of the incident)
    e.reproduce.hidden = !v.incident;
    e.reproduce.disabled = v.running || this.busy;
    const blocked = this.refusal || v.blocked || "";
    this.drawBlocked(blocked);
    e.blocked.hidden = !blocked;
    // during a run the message is the countdown (M-10): shown, not read out every second
    const blockedClass = `progress-blocked ${v.running ? "is-run" : "is-warn"}`;
    if (e.blocked.className !== blockedClass) e.blocked.className = blockedClass;
    const live = v.running ? "off" : "polite";
    if (e.blocked.ariaLive !== live) e.blocked.ariaLive = live;
    // what the open form means for the verification: a wrong field to correct first, or unsaved changes saved by it
    const step = v.current !== null && !v.running;
    const hint = !step || blocked === SHAPE_REFUSED ? "" : errors ? FIX_FIELDS : v.ready && !this.busy && this.formChanges ? SAVED_FIRST : "";
    if (e.hint) {
      setText(e.hint, hint);
      e.hint.hidden = !hint;
      const hintClass = hint === FIX_FIELDS ? "progress-hint is-warn" : "progress-hint";
      if (e.hint.className !== hintClass) e.hint.className = hintClass;
    }
    // a disabled button always says why (its title mirrors the message under it)
    const verifyTitle = e.verify.disabled ? blocked || (hint === FIX_FIELDS ? hint : "") : "";
    if (e.verify.title !== verifyTitle) e.verify.title = verifyTitle;
    const reproduceTitle = e.reproduce.disabled ? blocked : "";
    if (e.reproduce.title !== reproduceTitle) e.reproduce.title = reproduceTitle;
    this.renderFeedback(v);
    this.emphasis();
    this.wasRunning = v.running;
  }

  /** The message under the button, as given; the missing-fields message is drawn as a list of its fields. Rebuilt
   * only when it changes (the core's view comes every 250 ms). */
  private drawBlocked(text: string): void {
    if (text === this.blockedText) return;
    this.blockedText = text;
    const fields = missingFields(text);
    if (!fields) {
      setText(this.e.blocked, text);
      return;
    }
    clear(this.e.blocked);
    drawMissing(this.e.blocked, fields);
  }

  /** The verdict block, rebuilt only when its lines change; a new verdict is brought into view (and focused when it
   * ends a run). During a run the previous verdict is stale: hidden until the next one. */
  private renderFeedback(v: ProgressionView): void {
    const e = this.e;
    const key = JSON.stringify(v.feedback);
    e.feedback.hidden = v.feedback.length === 0 || v.running;
    if (key === this.feedbackKey) {
      // the same lines again after a run (the same verdict twice): shown again, brought into view
      if (this.wasRunning && !v.running && v.feedback.length) this.actions.reveal?.(e.feedback, true);
      return;
    }
    const first = this.feedbackKey === null;
    this.feedbackKey = key;
    const model = feedbackModel(v.feedback, v.steps, v.current);
    const cls = model.tone ? `progress-feedback is-${model.tone}` : "progress-feedback";
    if (e.feedback.className !== cls) e.feedback.className = cls;
    clear(e.feedback);
    for (const line of model.lines) {
      const p = el("p", "feedback-line");
      p.dataset.kind = line.kind;
      if (line.tone) p.dataset.tone = line.tone;
      if (line.parts) {
        const [label, sep, value] = line.parts;
        p.appendChild(el("span", "fl-label", label));
        p.appendChild(el("span", "fl-sep sr", sep));
        p.appendChild(el("span", line.kind === "cp" ? `fl-value badge badge--${line.tone}` : "fl-value", value));
      } else {
        p.textContent = line.text;
      }
      e.feedback.appendChild(p);
    }
    if (!first && v.feedback.length && !v.running) this.actions.reveal?.(e.feedback, this.wasRunning);
  }
}

// ------------------------------------------------------------------------------------------- the fiche form

type Reader = () => boolean;

// Every control carries its design-system class (.input, .select, .textarea, .btn: ui/DESIGN.md §5) next to its
// semantic one; scripts/check-ui.mjs refuses a control created without it.
function input(cls: string, value: string, placeholder = ""): HTMLInputElement {
  const i = el("input", `input ${cls}`);
  i.type = "text";
  i.value = value;
  if (placeholder) i.placeholder = placeholder;
  return i;
}

function select(cls: string, values: string[], current: string | null, empty = "—"): HTMLSelectElement {
  const s = el("select", `select ${cls}`);
  const none = el("option", undefined, empty);
  none.value = "";
  s.appendChild(none);
  for (const v of values) {
    const o = el("option", undefined, v);
    o.value = v;
    s.appendChild(o);
  }
  s.value = current ?? "";
  return s;
}

const text = (v: number | null): string => (v === null ? "" : String(v));

/** One line of fields in a record (styles: .fiche-line, a grid that wraps, never wider than the fiche). */
function line(body: HTMLElement, variant = ""): HTMLElement {
  const l = el("div", variant ? `fiche-line fiche-line--${variant}` : "fiche-line");
  body.appendChild(l);
  return l;
}

/** The core's refusal of any save once the mission is closed (workflow/session.rs CLOSED_FICHE): the note of the
 * read-only fiche at E5. */
export const CLOSED_FICHE = "La mission est clôturée : la fiche n'est plus modifiable.";
export const RUNNING_NOTE = "Vérification en cours : la fiche n'est pas modifiable.";
const VALIDATED_NOTE = "Partie validée : seuls la justification, les pages et les noms des signaux restent modifiables.";

/** The seven sections of the nominal part in the tutorial's order (§5.1): key, heading, entry of the index. */
const SECTIONS: [string, string, string][] = [
  ["documents", "Documents utilisés", "Documents utilisés"],
  ["trames", "Trames", "Trames"],
  ["signaux", "Signaux", "Signaux"],
  ["statuts", "Statuts", "Statuts"],
  ["supervision", "Supervision de la trame de données", "Supervision de la trame de données"],
  ["decisions", "Décisions de conception", "Décisions de conception"],
  ["justification", `Justification (${FREE_TEXT_NOTE})`, "Justification"],
];

/** The records of the lists, named as the core names them in its missing-fields message (« Trame 2 : DLC »). */
const RECORDS = {
  documents: { name: "Document", the: "le document", of: "du document" },
  trames: { name: "Trame", the: "la trame", of: "de la trame" },
  signaux: { name: "Signal", the: "le signal", of: "du signal" },
  statuts: { name: "Statut", the: "le statut", of: "du statut" },
};
type RecordKind = keyof typeof RECORDS;
type Control = HTMLInputElement | HTMLSelectElement | HTMLTextAreaElement;

/** How long « Confirmer la suppression » waits for its second click. */
const CONFIRM_MS = 5000;

/** The nominal part as a document: a head (title, note, index of the seven sections), the numbered sections with one
 * record per document, frame, signal and status, every control a labelled field, and a foot (« Enregistrer la fiche »,
 * the unsaved-changes marker, the status). Values are read back on « Enregistrer la fiche » or before a
 * verification; a wrongly shaped field is marked under itself and nothing is saved. */
export class FicheForm {
  private readers: Reader[] = [];
  private part: NominalPart = emptyPart();
  private documents: { id: string; title: string }[] = [];
  private editable = true;
  private validated = false;
  /** An attempt runs: nothing can be typed or saved, the free texts neither. */
  private running = false;
  /** The mission is closed (E5): the whole fiche is read-only, the core refuses every save. */
  private closed = false;
  private status: HTMLElement | null = null;
  private statusText = "";
  private statusTone: "" | "ok" | "err" = "";
  private saveButton: HTMLButtonElement | null = null;
  private dirtyMark: HTMLElement | null = null;
  /** A field changed since the last save (shown next to « Enregistrer la fiche »; nothing is saved by itself). */
  private dirty = false;
  private edits = 0;
  private nav: HTMLElement | null = null;
  private navItems: { section: string; item: HTMLElement; block: HTMLElement }[] = [];
  /** The section in view, marked in the index (IntersectionObserver of the real page; absent in the tests). */
  private watcher: IntersectionObserver | null = null;
  private inView = "";
  /** What `changed` last told the page (dirty, wrongly shaped). */
  private told = "";
  /** The section being drawn: the first part of the key of its controls. */
  private section = "";
  /** The shape check of each parsed field, in the order of the page: run when it changes and at every read. */
  private checks = new Map<HTMLElement, () => boolean>();
  /** What was typed in each wrongly shaped field, by key: a redraw shows it again, still marked. */
  private bad = new Map<string, string>();
  /** The field focused after the next redraw: the first field of a record just added. */
  private pendingFocus: string | null = null;
  /** The « Supprimer » waiting for its second click. */
  private armed: { button: HTMLButtonElement; label: string; timer: ReturnType<typeof setTimeout> } | null = null;

  constructor(private root: HTMLElement, private save: (p: NominalPart) => Promise<void>, private ui: FormUi = {}) {}

  /** `on`: the controlled fields can be edited; `validated`: the part passed (the note says why it is read-only);
   * `running`: an attempt runs, the free texts and « Enregistrer » are read-only too; `closed`: the mission is closed,
   * everything is read-only. */
  setEditable(on: boolean, validated = false, running = false, closed = false): void {
    if (on !== this.editable || validated !== this.validated || running !== this.running || closed !== this.closed) {
      this.read();
      this.running = running;
      this.closed = closed;
      this.render(this.part, this.documents, on, validated);
    }
  }

  /** The free texts (justification, pages, signal names) and the save button. */
  private get free(): boolean {
    return !this.running && !this.closed && (this.editable || this.validated);
  }

  /** The controlled fields are frozen. */
  private get locked(): boolean {
    return !this.editable || this.closed;
  }

  setDocuments(docs: { id: string; title: string }[]): void {
    if (JSON.stringify(docs) !== JSON.stringify(this.documents)) {
      this.read();
      this.render(this.part, docs, this.editable, this.validated);
    }
  }

  render(part: NominalPart, documents: { id: string; title: string }[], editable: boolean, validated = false): void {
    const memo = captureFocus(this.root);
    this.disarm();
    this.part = structuredClone(part);
    this.documents = documents;
    this.editable = editable;
    this.validated = validated;
    this.readers = [];
    this.checks = new Map();
    this.navItems = [];
    const r = this.root;
    clear(r);
    const head = el("header", "fiche-head");
    head.appendChild(el("h3", "fiche-title", "Fiche — partie nominale"));
    const note = this.closed ? CLOSED_FICHE : this.running ? RUNNING_NOTE : validated ? VALIDATED_NOTE : "";
    if (note) head.appendChild(el("p", "fiche-note", note));
    this.nav = el("nav", "fiche-nav");
    this.nav.ariaLabel = "Sections de la fiche";
    head.appendChild(this.nav);
    r.appendChild(head);
    const body = el("div", "fiche-body");
    r.appendChild(body);
    this.documentsSection(this.block(body, 0));
    this.framesSection(this.block(body, 1));
    this.signalsSection(this.block(body, 2));
    this.statusesSection(this.block(body, 3));
    this.supervisionSection(this.block(body, 4));
    this.decisionsSection(this.block(body, 5));
    this.justificationSection(this.block(body, 6));
    r.appendChild(this.foot());
    // a wrongly shaped field stays marked across the redraw, with what was typed in it
    for (const [c, check] of this.checks) if (this.bad.has(c.dataset.key ?? "")) check();
    this.refreshNav();
    this.watchSections();
    const focus = this.pendingFocus;
    this.pendingFocus = null;
    restoreFocus(r, focus ? { key: focus, start: null, end: null } : memo, focus !== null);
  }

  /** A field holds a wrongly shaped value (marked under it): the verification waits for it. */
  get hasErrors(): boolean {
    return this.bad.size > 0;
  }

  /** A field changed since the last save. */
  get hasChanges(): boolean {
    return this.dirty;
  }

  /** « Enregistrer la fiche » asked from outside (before the window or the mission closes): true when nothing unsaved
   * is left. */
  async saveNow(): Promise<boolean> {
    await this.submit();
    return !this.dirty;
  }

  /** The form is discarded (closed with its pack or its step): it stops watching the scroll. */
  dispose(): void {
    this.watcher?.disconnect();
    this.watcher = null;
    this.disarm();
  }

  /** Marks in the index the first section seen under the sticky head (the real page only: the tests' DOM has no
   * IntersectionObserver). */
  private watchSections(): void {
    this.watcher?.disconnect();
    this.watcher = null;
    if (typeof IntersectionObserver === "undefined") return;
    const seen = new Map<Element, boolean>();
    const watcher = new IntersectionObserver(
      (entries) => {
        for (const en of entries) seen.set(en.target, en.isIntersecting);
        const first = this.navItems.find((n) => seen.get(n.block));
        if (first) this.markInView(first.section);
      },
      // the band under the head (about 90 px) down to the middle of the fiche
      { root: this.root, rootMargin: "-90px 0px -50% 0px" },
    );
    for (const { block } of this.navItems) watcher.observe(block);
    this.watcher = watcher;
    if (this.inView) this.markInView(this.inView);
  }

  private markInView(section: string): void {
    this.inView = section;
    for (const { section: s, item } of this.navItems) {
      const want = s === section ? "true" : null;
      if (item.ariaCurrent !== want) item.ariaCurrent = want;
    }
  }

  /** Tells the page when the form turns dirty or clean, valid or wrongly shaped. */
  private notify(): void {
    const now = `${this.dirty}|${this.bad.size > 0}`;
    if (now === this.told) return;
    this.told = now;
    this.ui.changed?.();
  }

  /** Reads every field back; false when a field has a wrong shape (nothing saved then; the field is marked). */
  read(): boolean {
    return this.readers.map((f) => f()).every(Boolean);
  }

  current(): NominalPart {
    return structuredClone(this.part);
  }

  /** The part on screen was saved (here, or before a verification): no unsaved change left. */
  markSaved(): void {
    this.dirty = false;
    this.showDirty();
  }

  /** Edits typed so far: read before a save made from outside the form (the verification). */
  get revision(): number {
    return this.edits;
  }

  /** Saved from outside: clean only when nothing was typed since `revision` was read. */
  markSavedAt(revision: number): void {
    if (this.edits === revision) this.markSaved();
  }

  /** A wrong shape was refused: the status says so, the index marks its section, and the first wrongly shaped field
   * comes into view with the focus (`reveal`, when the fiche is shown). */
  refuse(reveal = true): void {
    this.setStatus(SHAPE_REFUSED, "err");
    this.refreshNav();
    const first = [...this.checks.keys()].find((c) => isInvalid(c));
    if (first && reveal) revealField(this.ui, first);
  }

  private async submit(): Promise<void> {
    if (!this.free) return;
    if (!this.read()) {
      this.refuse();
      return;
    }
    const edits = this.edits;
    try {
      await this.save(this.current());
      // (a field changed while the save was on its way stays unsaved)
      if (this.edits === edits) this.markSaved();
      this.setStatus("Enregistré", "ok");
    } catch (err) {
      this.setStatus(String(err), "err");
    }
  }

  // ------------------------------------------------------------------------------- structure

  /** Section `k`: a numbered block with its heading, and its entry in the index of the head. */
  private block(body: HTMLElement, k: number): HTMLElement {
    const [key, title, entry] = SECTIONS[k];
    this.section = key;
    const block = el("section", "fiche-block");
    block.dataset.section = key;
    block.appendChild(el("h4", "fiche-section", title));
    body.appendChild(block);
    const item = el("button", "btn btn--ghost fiche-nav-item", entry);
    item.type = "button";
    item.addEventListener("click", () => {
      this.markInView(key);
      this.ui.reveal?.(block, false, true);
    });
    this.nav!.appendChild(item);
    this.navItems.push({ section: key, item, block });
    return block;
  }

  /** « Enregistrer la fiche », the unsaved-changes marker, the status of the last save; hidden once the mission is
   * closed (the fiche is read-only, the note says so). */
  private foot(): HTMLElement {
    const foot = el("footer", "fiche-foot");
    foot.hidden = this.closed;
    const save = el("button", "btn fiche-save", "Enregistrer la fiche");
    save.type = "button";
    save.disabled = !this.free;
    save.addEventListener("click", () => void this.submit());
    foot.appendChild(save);
    this.saveButton = save;
    this.dirtyMark = el("span", "fiche-dirty", DIRTY);
    foot.appendChild(this.dirtyMark);
    this.status = el("p", "fiche-status", "");
    this.status.role = "status";
    foot.appendChild(this.status);
    this.showDirty();
    this.showStatus();
    return foot;
  }

  /** A record of a list: its head (« Trame 2 », the identifier as typed, « Supprimer »), then its body of lines. */
  private record(block: HTMLElement, kind: RecordKind, index: number, remove: () => void, id?: HTMLInputElement): HTMLElement {
    const names = RECORDS[kind];
    const name = `${names.name} ${index + 1}`;
    const rec = el("div", "fiche-record");
    rec.role = "group";
    rec.ariaLabel = name;
    const head = el("div", "fiche-record-head");
    head.appendChild(el("span", "fiche-record-name", name));
    if (id) {
      const shown = el("span", "fiche-record-id", id.value.trim() || "—");
      id.addEventListener("input", () => setText(shown, id.value.trim() || "—"));
      head.appendChild(shown);
    }
    if (!this.locked) head.appendChild(this.removeButton(`${names.the} ${index + 1}`, `${names.of} ${index + 1}`, remove));
    rec.appendChild(head);
    const body = el("div", "fiche-record-body");
    rec.appendChild(body);
    block.appendChild(rec);
    return body;
  }

  /** « Supprimer », in two clicks (ui/DESIGN.md §5): the first asks « Confirmer la suppression » for 5 s. */
  private removeButton(the: string, of: string, remove: () => void): HTMLButtonElement {
    const label = `Supprimer ${the}`;
    const del = el("button", "btn btn--ghost fiche-remove", "Supprimer");
    del.type = "button";
    del.ariaLabel = label;
    del.addEventListener("click", () => {
      // a redraw keeps only what reads back: a wrongly shaped field stops it instead of losing its row
      if (!this.read()) {
        this.refuse();
        return;
      }
      if (this.armed?.button !== del) {
        this.disarm();
        del.className = "btn btn--danger fiche-remove";
        setText(del, "Confirmer la suppression");
        del.ariaLabel = `Confirmer la suppression ${of}`;
        this.armed = { button: del, label, timer: setTimeout(() => this.disarm(), CONFIRM_MS) };
        return;
      }
      this.disarm();
      remove();
      this.edited();
      this.render(this.part, this.documents, this.editable, this.validated);
    });
    return del;
  }

  private disarm(): void {
    const a = this.armed;
    if (!a) return;
    this.armed = null;
    clearTimeout(a.timer);
    a.button.className = "btn btn--ghost fiche-remove";
    setText(a.button, "Supprimer");
    a.button.ariaLabel = a.label;
  }

  /** « Ajouter … » under the last record; `add` returns the key of the new record's first field, focused after. */
  private addButton(block: HTMLElement, label: string, add: () => string): void {
    if (this.locked) return;
    const b = el("button", "btn btn--add fiche-add ico ico--plus", label);
    b.type = "button";
    b.addEventListener("click", () => {
      if (!this.read()) {
        this.refuse();
        return;
      }
      this.pendingFocus = add();
      this.edited();
      this.render(this.part, this.documents, this.editable, this.validated);
    });
    block.appendChild(b);
  }

  // --------------------------------------------------------------------------------- fields

  /** A control of the form: its key (« trames.1.dlc », to keep the focus across a redraw), and its edits: typing
   * clears its error, leaving it checks its shape; both mark the fiche as changed. */
  private control<T extends Control>(c: T, key: string): T {
    c.dataset.key = `${this.section}.${key}`;
    c.addEventListener("input", () => {
      this.edited();
      if (this.checks.has(c) && isInvalid(c)) this.marked(c, true, "");
    });
    c.addEventListener("change", () => {
      this.edited();
      this.checks.get(c)?.();
    });
    return c;
  }

  private text(cls: string, key: string, value: string, placeholder = ""): HTMLInputElement {
    return this.control(input(cls, this.bad.get(`${this.section}.${key}`) ?? value, placeholder), key);
  }

  private choice(cls: string, key: string, values: string[], current: string | null): HTMLSelectElement {
    return this.control(select(cls, values, current), key);
  }

  /** A parsed text field: its shape is checked (and marked) when it changes and at every read; the reader returns
   * the value, undefined for a wrong shape. */
  private parsed<T>(c: HTMLInputElement, parse: (t: string) => T | undefined, message: string): () => T | undefined {
    let value: T | undefined;
    const check = (): boolean => {
      value = parse(c.value);
      this.marked(c, value !== undefined, message);
      return value !== undefined;
    };
    this.checks.set(c, check);
    return () => (check(), value);
  }

  private marked(c: HTMLElement, ok: boolean, message: string): void {
    mark(c, ok, message);
    const key = c.dataset.key ?? "";
    if (ok) this.bad.delete(key);
    else this.bad.set(key, (c as HTMLInputElement).value);
    this.refreshNav();
  }

  /** The entries of the index whose section holds a wrongly shaped field. */
  private refreshNav(): void {
    const wrong = new Set([...this.bad.keys()].map((k) => k.split(".")[0]));
    for (const { section, item } of this.navItems) {
      const want = wrong.has(section);
      if ((item.dataset.invalid === "true") === want) continue;
      if (want) item.dataset.invalid = "true";
      else delete item.dataset.invalid;
    }
    this.notify();
  }

  private edited(): void {
    this.edits++;
    if (!this.dirty) {
      this.dirty = true;
      this.showDirty();
    }
    // « Enregistré » or a refusal no longer describes what is on screen
    if (this.statusText) this.setStatus("", "");
  }

  /** While dirty: the marker shows and « Enregistrer la fiche » takes the emphasis of the fiche. */
  private showDirty(): void {
    if (this.dirtyMark) this.dirtyMark.hidden = !this.dirty;
    const cls = this.dirty ? "btn btn--primary fiche-save" : "btn fiche-save";
    if (this.saveButton && this.saveButton.className !== cls) this.saveButton.className = cls;
    this.notify();
  }

  private setStatus(text: string, tone: "" | "ok" | "err"): void {
    this.statusText = text;
    this.statusTone = tone;
    this.showStatus();
  }

  private showStatus(): void {
    if (!this.status) return;
    setText(this.status, this.statusText);
    const cls = this.statusTone ? `fiche-status ${this.statusTone}` : "fiche-status";
    if (this.status.className !== cls) this.status.className = cls;
  }

  /** A bit field: order, LSB position and length under the core's name of the field (« CRC de protection »). */
  private bits(target: HTMLElement, legend: string, key: string, f: BitField, set: (b: BitField) => void): Reader {
    const o = this.choice("fiche-order", `${key}.ordre`, ORDERS, f.ordre);
    const l = this.text("fiche-int", `${key}.lsb`, text(f.lsb_bit), "LSB");
    l.title = "Position du bit de poids faible";
    const n = this.text("fiche-int", `${key}.longueur`, text(f.longueur), "long.");
    for (const x of [o, l, n]) x.disabled = this.locked;
    const group = el("fieldset", "ff-bits");
    group.appendChild(el("legend", "ff-legend", legend));
    const cells = el("div", "ff-bits-cells");
    cells.appendChild(field("Ordre", o));
    cells.appendChild(field("LSB", l));
    cells.appendChild(field("Longueur", n));
    group.appendChild(cells);
    target.appendChild(group);
    const readLsb = this.parsed(l, (t) => parseInteger(t, 0, 63), FORMAT.integer(0, 63));
    const readLength = this.parsed(n, (t) => parseInteger(t, 1, 64), FORMAT.integer(1, 64));
    return () => {
      const lsb = readLsb();
      const len = readLength();
      if (lsb === undefined || len === undefined) return false;
      set({ ordre: (o.value || null) as BitOrder | null, lsb_bit: lsb, longueur: len });
      return true;
    };
  }

  // ------------------------------------------------------------------------------- sections

  private documentsSection(block: HTMLElement): void {
    this.part.documents_utilises.forEach((d, i) => {
      const s = this.control(el("select", "select fiche-document"), `${i}.document`);
      const none = el("option", undefined, "—");
      none.value = "";
      s.appendChild(none);
      for (const doc of this.documents) {
        const o = el("option", undefined, doc.title);
        o.value = doc.id;
        s.appendChild(o);
      }
      s.value = d.document ?? "";
      s.disabled = this.locked;
      // the whole title on hover, should a narrow fiche cut it
      const title = (): void => {
        s.title = this.documents.find((x) => x.id === s.value)?.title ?? "";
      };
      title();
      s.addEventListener("change", title);
      const pages = this.text("fiche-pages", `${i}.pages`, d.pages, "p. …");
      pages.disabled = !this.free;
      const l = line(this.record(block, "documents", i, () => this.part.documents_utilises.splice(i, 1)), "doc");
      l.appendChild(field("Document", s));
      l.appendChild(field(`Pages (${FREE_TEXT_NOTE})`, pages));
      this.readers.push(() => {
        // a choice missing from the list (library not loaded yet) is kept, never wiped
        const listed = this.documents.some((x) => x.id === d.document);
        d.document = s.value || (listed ? null : d.document);
        d.pages = pages.value;
        return true;
      });
    });
    this.addButton(block, "Ajouter un document", () => {
      this.part.documents_utilises.push({ document: null, pages: "" });
      return `documents.${this.part.documents_utilises.length - 1}.document`;
    });
  }

  private framesSection(block: HTMLElement): void {
    this.part.trames.forEach((f, i) => {
      const id = this.text("fiche-id", `${i}.id`, formatIdentifier(f.identifiant), "0x…");
      const format = this.choice("fiche-format", `${i}.format`, [FRAME_FORMAT], f.format);
      const dlc = this.text("fiche-int", `${i}.dlc`, text(f.dlc));
      const period = this.text("fiche-int", `${i}.periode`, text(f.periode_ms));
      const presence = f.protection.presente === null ? null : f.protection.presente ? "oui" : "non";
      const prot = this.choice("fiche-bool", `${i}.protection`, ["oui", "non"], presence);
      for (const x of [id, format, dlc, period, prot]) x.disabled = this.locked;
      const readId = this.parsed(id, parseIdentifier, FORMAT.identifier);
      const readDlc = this.parsed(dlc, (t) => parseInteger(t, 0, 8), FORMAT.integer(0, 8));
      const readPeriod = this.parsed(period, (t) => parseInteger(t, 0, 100_000), FORMAT.integer(0, 100_000));
      const body = this.record(block, "trames", i, () => this.part.trames.splice(i, 1), id);
      const l1 = line(body);
      l1.appendChild(field("Identifiant", id));
      l1.appendChild(field("Format", format, { wide: true }));
      l1.appendChild(field("DLC", dlc));
      l1.appendChild(field("Période (ms)", period));
      const l2 = line(body);
      l2.appendChild(field("Protection", prot));
      const readCrc = this.bits(l2, "CRC de protection", `${i}.crc`, f.protection.crc, (b) => (f.protection.crc = b));
      const readCounter = this.bits(l2, "Compteur de protection", `${i}.compteur`, f.protection.compteur, (b) => (f.protection.compteur = b));
      const readAlive = this.bits(line(body), "Compteur de vie", `${i}.vie`, f.compteur_de_vie, (b) => (f.compteur_de_vie = b));
      this.readers.push(() => {
        const ident = readId();
        const d = readDlc();
        const p = readPeriod();
        const ok = [readCrc(), readCounter(), readAlive()].every(Boolean);
        if (ident === undefined || d === undefined || p === undefined || !ok) return false;
        f.identifiant = ident;
        f.format = format.value || null;
        f.dlc = d;
        f.periode_ms = p;
        f.protection.presente = prot.value === "" ? null : prot.value === "oui";
        return true;
      });
    });
    this.addButton(block, "Ajouter une trame", () => {
      this.part.trames.push(emptyFrame());
      return `trames.${this.part.trames.length - 1}.id`;
    });
  }

  private signalsSection(block: HTMLElement): void {
    this.part.signaux.forEach((s, i) => {
      const name = this.text("fiche-name", `${i}.nom`, s.nom);
      name.disabled = !this.free;
      const id = this.text("fiche-id", `${i}.id`, formatIdentifier(s.identifiant), "0x…");
      const slots = this.text("fiche-slots", `${i}.slots`, s.slots.join(", "), "0, 1");
      const readId = this.parsed(id, parseIdentifier, FORMAT.identifier);
      const readSlots = this.parsed(slots, parseSlots, FORMAT.slots);
      const body = this.record(block, "signaux", i, () => this.part.signaux.splice(i, 1), id);
      const l1 = line(body);
      l1.appendChild(field(`Nom (${FREE_TEXT_NOTE})`, name, { wide: true }));
      l1.appendChild(field("Identifiant", id));
      l1.appendChild(field("Slots", slots));
      const l2 = line(body);
      const readField = this.bits(l2, "Position", `${i}.position`, { ordre: s.ordre, lsb_bit: s.lsb_bit, longueur: s.longueur }, (b) => {
        s.ordre = b.ordre;
        s.lsb_bit = b.lsb_bit;
        s.longueur = b.longueur;
      });
      const sign = this.choice("fiche-sign", `${i}.signe`, SIGNS, s.signe);
      l2.appendChild(field("Signe", sign, { wide: true }));
      const factor = this.text("fiche-num", `${i}.facteur`, text(s.facteur));
      const offset = this.text("fiche-num", `${i}.offset`, text(s.offset));
      const code = this.text("fiche-code", `${i}.code`, s.code_invalide === null ? "" : String(s.code_invalide), "entier ou AUCUN");
      const readFactor = this.parsed(factor, parseNumber, FORMAT.number);
      const readOffset = this.parsed(offset, parseNumber, FORMAT.number);
      const readCode = this.parsed(code, parseInvalidCode, FORMAT.invalidCode);
      const l3 = line(body);
      l3.appendChild(field("Facteur", factor));
      l3.appendChild(field("Offset", offset));
      l3.appendChild(field("Code invalide", code));
      for (const x of [id, slots, sign, factor, offset, code]) x.disabled = this.locked;
      this.readers.push(() => {
        const ident = readId();
        const sl = readSlots();
        const f = readFactor();
        const o = readOffset();
        const c = readCode();
        const ok = readField();
        if (ident === undefined || sl === undefined || f === undefined || o === undefined || c === undefined || !ok) return false;
        Object.assign(s, { nom: name.value, identifiant: ident, slots: sl, signe: (sign.value || null) as Sign | null, facteur: f, offset: o, code_invalide: c });
        return true;
      });
    });
    this.addButton(block, "Ajouter un signal", () => {
      this.part.signaux.push(emptySignal());
      return `signaux.${this.part.signaux.length - 1}.nom`;
    });
  }

  private statusesSection(block: HTMLElement): void {
    this.part.statuts.forEach((s, i) => {
      const id = this.text("fiche-id", `${i}.id`, formatIdentifier(s.identifiant), "0x…");
      const slots = this.text("fiche-slots", `${i}.slots`, s.slots.join(", "), "0, 1");
      const ok = this.text("fiche-int", `${i}.ok`, text(s.code_ok));
      for (const x of [id, slots, ok]) x.disabled = this.locked;
      const readId = this.parsed(id, parseIdentifier, FORMAT.identifier);
      const readSlots = this.parsed(slots, parseSlots, FORMAT.slots);
      const readOk = this.parsed(ok, (t) => parseInteger(t, -(2 ** 52), 2 ** 52), FORMAT.whole);
      const body = this.record(block, "statuts", i, () => this.part.statuts.splice(i, 1), id);
      const l1 = line(body);
      l1.appendChild(field("Identifiant", id));
      l1.appendChild(field("Slots", slots));
      l1.appendChild(field("Code OK", ok));
      const readField = this.bits(line(body), "Position", `${i}.position`, { ordre: s.ordre, lsb_bit: s.lsb_bit, longueur: s.longueur }, (b) => {
        s.ordre = b.ordre;
        s.lsb_bit = b.lsb_bit;
        s.longueur = b.longueur;
      });
      this.readers.push(() => {
        const ident = readId();
        const sl = readSlots();
        const code = readOk();
        const fieldOk = readField();
        if (ident === undefined || sl === undefined || code === undefined || !fieldOk) return false;
        Object.assign(s, { identifiant: ident, slots: sl, code_ok: code });
        return true;
      });
    });
    this.addButton(block, "Ajouter un statut", () => {
      this.part.statuts.push(emptyStatus());
      return `statuts.${this.part.statuts.length - 1}.id`;
    });
  }

  private supervisionSection(block: HTMLElement): void {
    const sv = this.part.supervision;
    const fields: [keyof typeof sv, string][] = [
      ["premier_timeout_ms", "Premier timeout (ms)"],
      ["timeout_ms", "Timeout (ms)"],
      ["peremption_ms", "Péremption (ms)"],
      ["pertes_consecutives_tolerees", "Pertes consécutives tolérées"],
    ];
    const l = line(block, "supervision");
    for (const [key, label] of fields) {
      const i = this.text("fiche-int", key, text(sv[key]));
      i.disabled = this.locked;
      const read = this.parsed(i, (t) => parseInteger(t, 0, 100_000), FORMAT.integer(0, 100_000));
      l.appendChild(field(label, i));
      this.readers.push(() => {
        const v = read();
        if (v === undefined) return false;
        sv[key] = v;
        return true;
      });
    }
  }

  private decisionsSection(block: HTMLElement): void {
    const l = line(block, "decisions");
    DECISIONS.forEach((d, i) => {
      const s = this.choice("fiche-decision", String(i), d.values, this.part.decisions[i] ?? null);
      s.disabled = this.locked;
      l.appendChild(field(d.label, s));
      this.readers.push(() => {
        this.part.decisions[i] = (s.value || null) as Decision | null;
        return true;
      });
    });
  }

  private justificationSection(block: HTMLElement): void {
    const just = this.control(el("textarea", "textarea fiche-justification"), "texte");
    just.value = this.part.justification;
    just.disabled = !this.free;
    just.ariaLabel = "Justification";
    block.appendChild(just);
    this.readers.push(() => {
      this.part.justification = just.value;
      return true;
    });
  }
}
