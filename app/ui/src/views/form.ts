// The field system shared by the two parts of the fiche (views/progress.ts nominal part, views/incident.ts incident
// part): a labelled field (label.ff: its label, the control, its format error under it), the marking of a wrongly
// shaped field, the texts of the format errors and of the unsaved-changes marker.
//
// Same rules as the forms (they run in the fake DOM of progress.test.ts): className strings, textContent, ARIA
// reflection properties and dataset only; no DOM query, no focus, no scroll (an injected `reveal` does those).

import { el, setText } from "../dom";

/** What a form asks of the page around it. */
export interface FormUi {
  /** Brings an element into view inside the fiche, and focuses it when asked; `start`: its top under the head. */
  reveal?(el: HTMLElement, focus: boolean, start?: boolean): void;
  /** The form became dirty or clean, valid or wrongly shaped (the verification button follows it). */
  changed?(): void;
}

/** French integer with its thousands separator (« 100 000 »). */
const n = (v: number): string => v.toLocaleString("fr-FR");

/** The format error shown under a wrongly shaped field (the form checks shapes only, never the truth). */
export const FORMAT = {
  identifier: "0x suivi de 1 à 3 chiffres hexadécimaux, au plus 0x7FF.",
  integer: (min: number, max: number): string => `Entier de ${n(min)} à ${n(max)}.`,
  whole: "Nombre entier.",
  number: "Nombre décimal (virgule ou point).",
  slots: "Numéros de 0 à 31 séparés par des virgules.",
  invalidCode: "Entier, 0x… ou AUCUN.",
  value: "Nombre décimal (virgule ou point), sans unité.",
};

/** Next to « Enregistrer la fiche » while a field changed since the last save. */
export const DIRTY = "Modifications non enregistrées";
/** Under the verification button: a field of the fiche is wrongly shaped (the button waits for it). */
export const FIX_FIELDS = "Corrigez les champs signalés dans la fiche.";
/** Under the verification button: unsaved changes, saved by the verification first. */
export const SAVED_FIRST = "La fiche sera enregistrée avant la vérification.";

const errors = new WeakMap<HTMLElement, HTMLElement>();
const boxes = new WeakMap<HTMLElement, HTMLElement>();

/** A labelled field: label.ff > span.ff-label + the control + span.ff-error (hidden while empty). The label gives the
 * control its accessible name. `wide` spans two columns of the record's line, `full` the whole line. */
export function field(label: string, control: HTMLElement, opts: { wide?: boolean; full?: boolean } = {}): HTMLLabelElement {
  const f = el("label", `ff${opts.wide ? " ff--wide" : ""}${opts.full ? " ff--full" : ""}`);
  f.appendChild(el("span", "ff-label", label));
  f.appendChild(control);
  const err = el("span", "ff-error", "");
  err.hidden = true;
  f.appendChild(err);
  errors.set(control, err);
  boxes.set(control, f);
  return f;
}

/** Brings a wrongly shaped field into view with its label and message (`reveal` of the page), then focuses it. */
export function revealField(ui: FormUi, control: HTMLElement): void {
  const box = boxes.get(control);
  if (box) ui.reveal?.(box, false);
  ui.reveal?.(control, true);
}

export function isInvalid(control: HTMLElement): boolean {
  return control.className.split(" ").includes("invalid");
}

/** Marks a field as well or wrongly shaped: the `invalid` class token (.input.invalid), aria-invalid, and its message
 * under it. */
export function mark(control: HTMLElement, ok: boolean, message: string): void {
  const tokens = control.className.split(" ").filter((t) => t && t !== "invalid");
  if (!ok) tokens.push("invalid");
  const cls = tokens.join(" ");
  if (control.className !== cls) control.className = cls;
  const aria = ok ? null : "true";
  if (control.ariaInvalid !== aria) control.ariaInvalid = aria;
  const err = errors.get(control);
  if (err) {
    setText(err, ok ? "" : message);
    err.hidden = ok;
  }
}
