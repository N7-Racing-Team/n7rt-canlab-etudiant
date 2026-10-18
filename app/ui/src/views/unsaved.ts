// Unsaved fiche work that closing the window or the mission would lose: the open form of the progression, which saves
// nothing by itself (« Modifications non enregistrées »). The progression panel registers itself here; the window's
// close guard (ide/view.ts) and the « Fermer » of the mission ask before closing: save, close without saving, or go back.

import { el } from "../dom";

export interface UnsavedWork {
  /** Something typed since the last « Enregistrer la fiche ». */
  readonly unsaved: boolean;
  /** Saves it as « Enregistrer la fiche » does; true when nothing unsaved is left (false: a wrongly shaped field or a
   * refusal, shown in the fiche). */
  saveUnsaved(): Promise<boolean>;
}

export type CloseChoice = "cancel" | "save" | "discard";

export const FICHE_UNSAVED = "La fiche contient des modifications non enregistrées. Que souhaitez-vous faire ?";
export const FICHE_NOT_SAVED = "La fiche n'a pas pu être enregistrée : la raison est indiquée au bas de la fiche (Cockpit, onglet « Fiche »). Corrigez-la et réessayez, ou fermez à nouveau et choisissez « Fermer sans enregistrer ».";

let work: UnsavedWork | null = null;

/** The fiche whose unsaved changes a close must not lose (null: none). */
export function watchFiche(w: UnsavedWork | null): void {
  work = w;
}

/** The watched fiche holds unsaved changes (the reload guard of ide/view.ts asks it too). */
export function ficheUnsaved(): boolean {
  return !!work?.unsaved;
}

/** Before a close: true when it may go on (nothing unsaved, saved, or discarded on purpose). `ask` shows the choice
 * (`offer`: save and discard are offered; otherwise only the way back). */
export async function settleFiche(ask: (message: string, offer: boolean) => Promise<CloseChoice>): Promise<boolean> {
  if (!work?.unsaved) return true;
  const choice = await ask(FICHE_UNSAVED, true);
  if (choice === "cancel") return false;
  if (choice === "discard") return true;
  if (await work.saveUnsaved()) return true;
  await ask(FICHE_NOT_SAVED, false);
  return false;
}

/** The modal choice before a close: « Enregistrer et fermer » and « Fermer sans enregistrer » when `offer`, and the
 * way back (`back`), focused first. `id`: the prefix of the ids of the dialog and its buttons. */
export function closeDialog(id: string, title: string, message: string, offer: boolean, back: string): Promise<CloseChoice> {
  const dialog = el("dialog", "dialog");
  dialog.id = `${id}-dialog`;
  dialog.setAttribute("aria-labelledby", `${id}-title`);
  const head = el("p", "dialog-title", title);
  head.id = `${id}-title`;
  const detail = el("p", undefined, message);
  const actions = el("div", "field-row dialog-actions");
  const cancel = el("button", "btn", back);
  cancel.type = "button"; cancel.id = `${id}-cancel`;
  cancel.addEventListener("click", () => dialog.close("cancel"));
  if (offer) {
    const discard = el("button", "btn btn--danger", "Fermer sans enregistrer");
    discard.type = "button"; discard.id = `${id}-discard`;
    // the second click of a double click on a previous dialog (same size, same buttons) never discards this one
    discard.disabled = true;
    setTimeout(() => { discard.disabled = false; }, 400);
    discard.addEventListener("click", () => dialog.close("discard"));
    const save = el("button", "btn", "Enregistrer et fermer");
    save.type = "button"; save.id = `${id}-save`;
    save.addEventListener("click", () => dialog.close("save"));
    actions.append(save, discard);
  }
  actions.append(cancel);
  dialog.append(head, detail, actions);
  document.body.append(dialog);
  return new Promise((resolve) => {
    dialog.addEventListener("close", () => {
      const choice = dialog.returnValue;
      dialog.remove();
      resolve(choice === "save" || choice === "discard" ? choice : "cancel");
    }, { once: true });
    dialog.showModal();
    cancel.focus();
  });
}
