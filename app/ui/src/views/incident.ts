// The incident part of the fiche (private spec ch. 24 §3.4–§3.6, §7.4, adapted without server): target designated
// on the bench (two pins of a conductor, or a component) or on the bus (an identifier), category and observed state
// from the public lists with their definitions, the key measure (quantity imposed by the public rule, value and unit
// given by the student, conditions and evidence recorded by the application at « Consigner la mesure »), and the free
// justification.
//
// Rules: the lists, definitions and the rule come from the core (same for the four missions); nothing here knows a
// target, a value or what the verification expects; the form checks shapes only (11-bit hexadecimal, finite number,
// « désignateur.broche »); text through textContent only (ui/DESIGN.md §8).

import { clear, el, setText } from "../dom";
import type { IncidentPart, IncidentView, TargetKind } from "../types";
import { captureFocus, restoreFocus } from "./focus";
import { DIRTY, field, FORMAT, isInvalid, mark, revealField, type FormUi } from "./form";
import { FREE_TEXT_NOTE, parseIdentifier, parseNumber, RUNNING_NOTE, SHAPE_REFUSED } from "./progress";

export const TARGET_KINDS: TargetKind[] = ["CONDUCTEUR", "COMPOSANT", "IDENTIFIANT"];
const TARGET_LABELS: Record<TargetKind, string> = {
  CONDUCTEUR: "Conducteur (ses deux broches)",
  COMPOSANT: "Composant (son désignateur)",
  IDENTIFIANT: "Identifiant (trame du bus)",
};

/** The key measure's sentences, shared with the Physical Lab's multimeter (lab/panels/meter.ts). */
export const CHOOSE_STATE = "Choisissez l'état observé : il impose la grandeur à relever.";
export const imposedText = (quantity: string): string => `Grandeur imposée : ${quantity}.`;
export const consignedText = (consigned: string | null): string =>
  consigned ? `Mesure consignée ${consigned}.` : "Mesure non consignée.";
/** The note of a validated incident part (the running note is the nominal part's). */
export const VALIDATED_INCIDENT = "Partie validée : la fiche n'est pas modifiable.";

export function emptyIncident(rank: number): IncidentPart {
  return {
    rang: rank,
    cible: { type: null, conducteur: { a: null, b: null }, composant: null, identifiant: null },
    categorie: null,
    etat_observe: null,
    mesure_cle: { valeur: null, unite: null, consignation: null },
    justification: "",
  };
}

export interface IncidentActions {
  save(part: IncidentPart): Promise<void>;
  consign(): Promise<void>;
}

/** A list of choices; a saved value missing from the list is kept as a choice of its own, never wiped. */
function choice(cls: string, values: { value: string; label: string; title?: string }[], current: string | null): HTMLSelectElement {
  const s = el("select", `select ${cls}`);
  const none = el("option", undefined, "—");
  none.value = "";
  s.appendChild(none);
  const all = current && !values.some((v) => v.value === current) ? [...values, { value: current, label: `${current} (absent du banc)` }] : values;
  for (const v of all) {
    const o = el("option", undefined, v.label);
    o.value = v.value;
    if (v.title) o.title = v.title;
    s.appendChild(o);
  }
  s.value = current ?? "";
  return s;
}

/** The quantity of the public rule for a state and a target type. */
export function imposedQuantity(view: IncidentView, state: string | null, kind: TargetKind | null): { text: string; units: string[] } | null {
  const rule = view.rules.find((r) => r.state === state);
  if (!rule) return null;
  return { text: kind === "COMPOSANT" ? rule.component : rule.other, units: rule.units };
}

/** What the lists and the rule of the view are: while they stay, a new view is patched in place, never redrawn. */
const listsKey = (v: IncidentView): string => JSON.stringify([v.rank, v.pins, v.parts, v.categories, v.states, v.rules]);

type Control = HTMLInputElement | HTMLSelectElement | HTMLTextAreaElement;

/** The elements a new view of the core changes. */
interface Drawn {
  controls: Control[];
  note: HTMLElement;
  hint: HTMLElement;
  consign: HTMLButtonElement;
  consigned: HTMLElement;
  save: HTMLButtonElement;
  dirty: HTMLElement;
  status: HTMLElement;
}

export class IncidentForm {
  private part: IncidentPart = emptyIncident(1);
  private view: IncidentView | null = null;
  /** A verification runs (the part is read-only for it; the note says so). */
  private running = false;
  private drawn: Drawn | null = null;
  private readers: (() => boolean)[] = [];
  /** The shape check of the identifier and of the value, in the order of the page. */
  private checks = new Map<HTMLElement, () => boolean>();
  /** What was typed in the value and identifier fields, kept as typed across a redraw (even a wrong shape). */
  private raw: { value: string | null; identifier: string | null } = { value: null, identifier: null };
  private statusText = "";
  private statusTone: "" | "ok" | "err" = "";
  /** A field changed since the last save (shown next to « Enregistrer la fiche »; nothing is saved by itself). */
  private dirty = false;
  private edits = 0;
  /** What `changed` last told the page (dirty, wrongly shaped). */
  private told = "";

  constructor(private root: HTMLElement, private actions: IncidentActions, private ui: FormUi = {}) {}

  get rank(): number {
    return this.part.rang;
  }

  /** A parsed field (identifier, value) is marked wrongly shaped: the verification waits for it. */
  get hasErrors(): boolean {
    return [...this.checks.keys()].some((c) => isInvalid(c));
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

  /** Tells the page when the form turns dirty or clean, valid or wrongly shaped. */
  private notify(): void {
    const now = `${this.dirty}|${this.hasErrors}`;
    if (now === this.told) return;
    this.told = now;
    this.ui.changed?.();
  }

  /** The core's view changed. The usual change (editable, recorded measure, hint, a run starting or ending) is patched
   * in place: the focus and what is being typed stay. New lists or another rule redraw the form, keeping every edit,
   * a wrongly shaped one included. */
  setView(view: IncidentView, running = false): void {
    const old = this.view;
    if (old && running === this.running && JSON.stringify(view) === JSON.stringify(old)) return;
    if (old && this.drawn && listsKey(view) === listsKey(old)) {
      this.view = view;
      this.running = running;
      this.patch();
      return;
    }
    this.read();
    this.running = running;
    this.draw(view);
  }

  /** Draws `part` as loaded (what was typed before is forgotten). */
  render(part: IncidentPart, view: IncidentView, running = false): void {
    this.raw = { value: null, identifier: null };
    this.part = structuredClone(part);
    this.running = running;
    this.draw(view);
  }

  private draw(view: IncidentView): void {
    const memo = captureFocus(this.root);
    this.view = view;
    this.readers = [];
    this.checks = new Map();
    const r = this.root;
    clear(r);
    const editable = view.editable;
    const controls: Control[] = [];
    // every control: a key (the focus is kept across a redraw), frozen with the part, and its edits mark the fiche
    // as changed; typing clears an error, leaving the field checks its shape
    const ctl = <T extends Control>(c: T, key: string): T => {
      c.dataset.key = `incident.${key}`;
      c.disabled = !editable;
      c.addEventListener("input", () => {
        if (this.checks.has(c) && isInvalid(c)) mark(c, true, "");
        this.edited();
      });
      c.addEventListener("change", () => {
        this.edited();
        this.checks.get(c)?.();
      });
      controls.push(c);
      return c;
    };

    const head = el("header", "fiche-head");
    head.appendChild(el("h3", "fiche-title", `Fiche — incident ${view.rank}`));
    const note = el("p", "fiche-note", "");
    head.appendChild(note);
    r.appendChild(head);
    // five numbered sections; from 640 px of fiche on, two columns: Cible, Catégorie, État observé on the left, Mesure
    // clé and Justification on the right (tab order = this order)
    const body = el("div", "fiche-body incident-grid");
    r.appendChild(body);
    const block = (area: string, title: string): HTMLElement => {
      const b = el("section", `fiche-block incident-${area}`);
      b.appendChild(el("h4", "fiche-section", title));
      body.appendChild(b);
      return b;
    };

    // Cible: its type, then what designates it on the bench or on the bus
    const cible = block("cible", "Cible");
    const kind = ctl(choice("incident-target-kind", TARGET_KINDS.map((k) => ({ value: k, label: TARGET_LABELS[k] })), this.part.cible.type), "cible");
    kind.ariaLabel = "Cible";
    cible.appendChild(kind);
    const designation = el("div", "incident-designation");
    cible.appendChild(designation);
    const pins = view.pins.map((p) => ({ value: p, label: p }));
    const pinA = ctl(choice("incident-pin-a", pins, this.part.cible.conducteur.a), "broche-a");
    const pinB = ctl(choice("incident-pin-b", pins, this.part.cible.conducteur.b), "broche-b");
    const component = ctl(choice("incident-part", view.parts.map((p) => ({ value: p, label: p })), this.part.cible.composant), "composant");
    const ident = ctl(el("input", "input incident-identifier"), "identifiant");
    ident.type = "text";
    ident.placeholder = "0x…";
    ident.value = this.raw.identifier ?? this.part.cible.identifiant ?? "";
    const designators: Record<TargetKind, HTMLElement[]> = {
      CONDUCTEUR: [field("Broche A", pinA), field("Broche B", pinB)],
      COMPOSANT: [field("Composant", component)],
      IDENTIFIANT: [field("Identifiant", ident)],
    };
    // synchronous on the change of the type: the matching fields are there right after it
    const showDesignation = (): void => {
      clear(designation);
      const k = (kind.value || null) as TargetKind | null;
      for (const f of k ? designators[k] : []) designation.appendChild(f);
    };
    showDesignation();

    // Catégorie, état observé (public definitions shown under each list)
    const categorie = block("categorie", "Catégorie");
    const cat = ctl(choice("incident-category", view.categories.map((c) => ({ value: c.value, label: c.value, title: c.definition })), this.part.categorie), "categorie");
    cat.ariaLabel = "Catégorie";
    categorie.appendChild(cat);
    const catDef = el("p", "incident-definition", "");
    categorie.appendChild(catDef);
    const etat = block("etat", "État observé");
    const state = ctl(choice("incident-state", view.states.map((s) => ({ value: s.value, label: s.value, title: s.definition })), this.part.etat_observe), "etat");
    state.ariaLabel = "État observé";
    etat.appendChild(state);
    const stateDef = el("p", "incident-definition", "");
    etat.appendChild(stateDef);

    // Mesure clé: the imposed quantity and the rule, « Consigner la mesure » and its record, then value and unit
    const mesure = block("mesure", "Mesure clé");
    const quantity = el("p", "incident-quantity", "");
    mesure.appendChild(quantity);
    const hint = el("p", "incident-hint", view.hint);
    mesure.appendChild(hint);
    const consignRow = el("div", "incident-consign-row");
    const consign = el("button", "btn incident-consign ico ico--meter", "Consigner la mesure");
    consign.type = "button";
    consign.disabled = !editable;
    consign.addEventListener("click", () => {
      // (refused or failed: the status says why)
      if (this.view?.editable) this.consign(true).catch(() => {});
    });
    consignRow.appendChild(consign);
    const consigned = el("p", "incident-consigned", "");
    consignRow.appendChild(consigned);
    mesure.appendChild(consignRow);
    const valueRow = el("div", "incident-value-row");
    const value = ctl(el("input", "input incident-value"), "valeur");
    value.type = "text";
    value.placeholder = "valeur";
    value.value = this.raw.value ?? (this.part.mesure_cle.valeur === null ? "" : String(this.part.mesure_cle.valeur));
    const unit = ctl(el("select", "select incident-unit"), "unite");
    valueRow.appendChild(field("Valeur", value));
    valueRow.appendChild(field("Unité", unit));
    mesure.appendChild(valueRow);

    const refresh = (): void => {
      setText(catDef, view.categories.find((c) => c.value === cat.value)?.definition ?? "");
      setText(stateDef, view.states.find((s) => s.value === state.value)?.definition ?? "");
      const imposed = imposedQuantity(view, state.value || null, (kind.value || null) as TargetKind | null);
      setText(quantity, imposed ? imposedText(imposed.text) : CHOOSE_STATE);
      const keep = unit.value || this.part.mesure_cle.unite;
      clear(unit);
      const none = el("option", undefined, "—");
      none.value = "";
      unit.appendChild(none);
      for (const u of imposed?.units ?? []) {
        const o = el("option", undefined, u);
        o.value = u;
        unit.appendChild(o);
      }
      unit.value = keep && (imposed?.units ?? []).includes(keep) ? keep : "";
    };
    kind.addEventListener("change", () => {
      showDesignation();
      refresh();
      // an identifier marked wrong no longer counts once the target is not an identifier
      if (isInvalid(ident)) checkId();
    });
    cat.addEventListener("change", refresh);
    state.addEventListener("change", refresh);
    refresh();

    // Justification
    const justif = block("justif", `Justification (${FREE_TEXT_NOTE})`);
    const just = ctl(el("textarea", "textarea fiche-justification"), "justification");
    just.value = this.part.justification;
    just.ariaLabel = "Justification";
    justif.appendChild(just);

    // the two parsed fields, marked under themselves when wrongly shaped (the identifier only when it is the target)
    const checkId = this.rule(ident, () => kind.value !== "IDENTIFIANT" || parseIdentifier(ident.value) !== undefined, FORMAT.identifier);
    const checkValue = this.rule(value, () => parseNumber(value.value) !== undefined, FORMAT.value);

    // every well-shaped field is kept, and the typed text of the two parsed fields too, so that a redraw loses nothing
    this.readers.push(() => {
      const k = (kind.value || null) as TargetKind | null;
      this.raw = { value: value.value, identifier: ident.value };
      const idOk = checkId();
      const valueOk = checkValue();
      const id = k === "IDENTIFIANT" ? parseIdentifier(ident.value) : null;
      const v = parseNumber(value.value);
      this.part.cible = {
        type: k,
        conducteur: { a: k === "CONDUCTEUR" ? pinA.value || null : null, b: k === "CONDUCTEUR" ? pinB.value || null : null },
        composant: k === "COMPOSANT" ? component.value || null : null,
        identifiant: id === undefined ? this.part.cible.identifiant : id === null ? null : `0x${id.toString(16).toUpperCase().padStart(3, "0")}`,
      };
      this.part.categorie = cat.value || null;
      this.part.etat_observe = state.value || null;
      this.part.mesure_cle = { ...this.part.mesure_cle, valeur: v === undefined ? this.part.mesure_cle.valeur : v, unite: unit.value || null };
      this.part.justification = just.value;
      return idOk && valueOk;
    });

    const foot = el("footer", "fiche-foot");
    const save = el("button", "btn fiche-save", "Enregistrer la fiche");
    save.type = "button";
    save.disabled = !editable;
    save.addEventListener("click", () => void this.submit());
    foot.appendChild(save);
    const dirty = el("span", "fiche-dirty", DIRTY);
    foot.appendChild(dirty);
    const status = el("p", "fiche-status", "");
    status.role = "status";
    foot.appendChild(status);
    r.appendChild(foot);

    this.drawn = { controls, note, hint, consign, consigned, save, dirty, status };
    this.patch();
    this.showDirty();
    this.showStatus();
    // a field typed with a wrong shape before the redraw stays marked
    if (this.raw.identifier !== null) checkId();
    if (this.raw.value !== null) checkValue();
    restoreFocus(r, memo);
  }

  /** What follows the core's view without a redraw: frozen or not, the note, the rule's hint, the recorded measure. */
  private patch(): void {
    const d = this.drawn;
    const v = this.view;
    if (!d || !v) return;
    const frozen = !v.editable;
    for (const c of d.controls) if (c.disabled !== frozen) c.disabled = frozen;
    d.consign.disabled = frozen;
    d.save.disabled = frozen;
    const note = !frozen ? "" : this.running ? RUNNING_NOTE : VALIDATED_INCIDENT;
    setText(d.note, note);
    d.note.hidden = !note;
    setText(d.hint, v.hint);
    setText(d.consigned, consignedText(v.consigned));
    // not recorded yet: an amber chip (something to do), the date in green once recorded
    const cls = v.consigned ? "incident-consigned ok" : "incident-consigned warn";
    if (d.consigned.className !== cls) d.consigned.className = cls;
  }

  /** Reads every field back; false when a field has a wrong shape (nothing saved then; the field is marked). */
  read(): boolean {
    return this.readers.map((f) => f()).every(Boolean);
  }

  current(): IncidentPart {
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

  /** A wrong shape was refused: the status says so and, when asked, the first wrongly shaped field comes into view
   * with the focus. */
  refuse(reveal = true): void {
    this.setStatus(SHAPE_REFUSED, "err");
    this.notify();
    const first = [...this.checks.keys()].find((c) => isInvalid(c));
    if (first && reveal) revealField(this.ui, first);
  }

  private rule(c: HTMLInputElement, ok: () => boolean, message: string): () => boolean {
    const check = (): boolean => {
      const good = ok();
      mark(c, good, message);
      this.notify();
      return good;
    };
    this.checks.set(c, check);
    return check;
  }

  private async submit(): Promise<void> {
    if (!this.view?.editable) return;
    if (!this.read()) {
      this.refuse();
      return;
    }
    const edits = this.edits;
    try {
      await this.actions.save(this.current());
      if (this.edits === edits) this.markSaved();
      this.setStatus("Enregistré", "ok");
    } catch (err) {
      this.setStatus(String(err), "err");
    }
  }

  /** « Consigner la mesure » from outside the form (the Physical Lab's multimeter): the same as the form's button, and
   * the refusal (a wrong shape or the core's text), shown in the form, is thrown back to the caller too. */
  consignExternal(): Promise<void> {
    return this.consign(false);
  }

  /** The declared state and target are saved first: the recorded quantity is the one they impose. */
  private async consign(reveal: boolean): Promise<void> {
    if (!this.read()) {
      this.refuse(reveal);
      throw SHAPE_REFUSED;
    }
    const edits = this.edits;
    try {
      await this.actions.save(this.current());
      if (this.edits === edits) this.markSaved();
      await this.actions.consign();
      this.setStatus("Mesure consignée", "ok");
    } catch (err) {
      this.setStatus(String(err), "err");
      throw err;
    }
  }

  private edited(): void {
    this.edits++;
    if (!this.dirty) {
      this.dirty = true;
      this.showDirty();
    }
    if (this.statusText) this.setStatus("", "");
    // (typing in a marked field has just cleared its mark)
    this.notify();
  }

  /** While dirty: the marker shows and « Enregistrer la fiche » takes the emphasis of the fiche. */
  private showDirty(): void {
    const d = this.drawn;
    if (!d) return;
    d.dirty.hidden = !this.dirty;
    const cls = this.dirty ? "btn btn--primary fiche-save" : "btn fiche-save";
    if (d.save.className !== cls) d.save.className = cls;
    this.notify();
  }

  private setStatus(text: string, tone: "" | "ok" | "err"): void {
    this.statusText = text;
    this.statusTone = tone;
    this.showStatus();
  }

  private showStatus(): void {
    const d = this.drawn;
    if (!d) return;
    setText(d.status, this.statusText);
    const cls = this.statusTone ? `fiche-status ${this.statusTone}` : "fiche-status";
    if (d.status.className !== cls) d.status.className = cls;
  }
}
