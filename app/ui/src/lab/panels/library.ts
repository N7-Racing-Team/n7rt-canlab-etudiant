// Left panel of the Physical Lab (contract docs/physical-lab-v1.md §8.4): the library (placeable kinds; pressing one
// arms the placement in the editor) and the file section (name, save with overwrite confirmation, open, new,
// demonstrations, modification marker). Abandoning unsaved changes, or overwriting a file, takes a second click on
// the same button turned into a .btn--danger.

import { $, clear, el, setOptions, setText } from "../../dom";
import { asPhysError } from "../api";
import type { LabController, LabState } from "../state";
import type { FileEntry, Kind } from "../types";

export interface LibraryHost {
  readonly state: LabState;
  readonly controller: LabController;
  ready(): boolean;
  /** Arms the placement of a kind in the editor; false when the editor is unavailable. */
  armPlacement(kind: Kind): boolean;
  /** Kind whose placement is armed, if any. */
  placing(): Kind | null;
}

const CONFIRM_MS = 5000;

/** One pending confirmation at a time: the button shows the question in danger style until confirmed or expired. */
class Confirmation {
  private armed: { btn: HTMLButtonElement; key: string; text: string; cls: string; timer: number } | null = null;

  /** This button asks this question (key) right now: a click on it confirms. */
  isArmed(btn: HTMLButtonElement, key: string): boolean {
    return this.armed !== null && this.armed.btn === btn && this.armed.key === key;
  }

  arm(btn: HTMLButtonElement, key: string, question: string): void {
    this.cancel();
    this.armed = { btn, key, text: btn.textContent ?? "", cls: btn.className, timer: window.setTimeout(() => this.cancel(), CONFIRM_MS) };
    btn.textContent = question;
    btn.className = "btn btn--danger";
  }

  cancel(): void {
    const a = this.armed;
    if (!a) return;
    this.armed = null;
    window.clearTimeout(a.timer);
    a.btn.textContent = a.text;
    a.btn.className = a.cls;
  }
}

export class LibraryPanel {
  private paletteShown: unknown = null;
  private armedShown: Kind | null | undefined = undefined;
  private demosShown: unknown = null;
  private files: FileEntry[] = [];
  private filesNote = "Aucun circuit enregistré";
  private filesDirty = true;
  private dirty = true;
  private docName = "";
  /** File this document was opened from or last saved to: saving it again under that name needs no confirmation. */
  private fileName: string | null = null;
  private readonly confirm = new Confirmation();
  private readonly palette: HTMLElement;
  private readonly nameInput: HTMLInputElement;
  private readonly docLabel: HTMLElement;
  private readonly dirtyBadge: HTMLElement;
  private readonly save: HTMLButtonElement;
  private readonly fresh: HTMLButtonElement;
  private readonly openSelect: HTMLSelectElement;
  private readonly open: HTMLButtonElement;
  private readonly demoSelect: HTMLSelectElement;
  private readonly demo: HTMLButtonElement;
  private readonly demoSummary: HTMLElement;

  constructor(root: HTMLElement, private readonly host: LibraryHost) {
    this.palette = $("#phys-palette", root);
    this.nameInput = $<HTMLInputElement>("#phys-file-name", root);
    this.docLabel = $("#phys-doc-name", root);
    this.dirtyBadge = $("#phys-dirty", root);
    this.save = $<HTMLButtonElement>("#phys-save", root);
    this.fresh = $<HTMLButtonElement>("#phys-new", root);
    this.openSelect = $<HTMLSelectElement>("#phys-open-select", root);
    this.open = $<HTMLButtonElement>("#phys-open", root);
    this.demoSelect = $<HTMLSelectElement>("#phys-demo-select", root);
    this.demo = $<HTMLButtonElement>("#phys-demo", root);
    this.demoSummary = $("#phys-demo-summary", root);
  }

  mount(): void {
    const { state } = this.host;
    // Pressing an item arms the placement (no HTML drag and drop: disabled by Tauri on Windows).
    this.palette.addEventListener("pointerdown", (e) => {
      const item = (e.target as Element).closest<HTMLButtonElement>(".lab-item");
      if (!item || e.button !== 0 || item.disabled) return;
      e.preventDefault();
      this.arm(item.dataset.kind as Kind);
    });
    this.palette.addEventListener("keydown", (e) => {
      const item = (e.target as Element).closest<HTMLButtonElement>(".lab-item");
      if (!item || (e.key !== "Enter" && e.key !== " ")) return;
      e.preventDefault();
      this.arm(item.dataset.kind as Kind);
    });
    this.save.addEventListener("click", () => void this.onSave());
    this.fresh.addEventListener("click", () => this.abandon(this.fresh, "new", () => this.host.controller.newCircuit(), "Nouveau circuit.", null));
    this.open.addEventListener("click", () => {
      const name = this.openSelect.value;
      if (name) this.abandon(this.open, `open:${name}`, () => this.host.controller.load(name), `Circuit « ${name} » ouvert.`, name);
    });
    this.demo.addEventListener("click", () => {
      const id = this.demoSelect.value;
      const title = state.demos.find((d) => d.id === id)?.title_fr ?? id;
      if (id) this.abandon(this.demo, `demo:${id}`, () => this.host.controller.loadDemo(id), `Démonstration « ${title} » chargée.`, null);
    });
    for (const select of [this.demoSelect, this.openSelect]) {
      select.addEventListener("change", () => {
        this.dirty = true;
      });
    }
    this.nameInput.addEventListener("input", () => this.nameInput.classList.remove("invalid"));
    // The list of saved circuits is refreshed when the reader opens it.
    this.openSelect.addEventListener("pointerdown", () => this.refreshFiles());
    this.openSelect.addEventListener("focus", () => this.refreshFiles());
    const mark = () => {
      this.dirty = true;
    };
    state.onStatus.on(mark);
    state.onDoc.on(mark);
  }

  invalidate(): void {
    this.dirty = true;
  }

  /** Re-reads the saved circuits (phys_list), newest first; an error empties the list and says why. */
  refreshFiles(): void {
    if (!this.host.ready()) return;
    this.host.controller.list().then(
      (files) => {
        this.files = [...files].sort((a, b) => b.modified_unix_ms - a.modified_unix_ms || a.name.localeCompare(b.name));
        this.filesNote = "Aucun circuit enregistré";
        this.filesDirty = true;
      },
      (e: unknown) => {
        const err = asPhysError(e);
        this.files = [];
        this.filesNote = err.code === "file" && err.error.code === "no_storage" ? "Aucun dossier de données" : "Liste indisponible";
        this.filesDirty = true;
      },
    );
  }

  private arm(kind: Kind): void {
    // A focused field would take the keys (R, Escape) meant for the placement.
    const active = document.activeElement;
    if (active instanceof HTMLInputElement || active instanceof HTMLSelectElement || active instanceof HTMLTextAreaElement) active.blur();
    if (!this.host.armPlacement(kind)) {
      this.host.state.setMessage({ kind: "error", text: "Éditeur de schéma indisponible : placement impossible." });
    }
    this.dirty = true;
  }

  private async onSave(): Promise<void> {
    const { state, controller } = this.host;
    const name = this.nameInput.value.trim() || state.name;
    if (!name) {
      this.nameInput.classList.add("invalid");
      state.setMessage({ kind: "error", text: "Donnez un nom au circuit avant de l'enregistrer." });
      return;
    }
    const key = `save:${name}`;
    const overwrite = this.confirm.isArmed(this.save, key) || name === this.fileName;
    this.confirm.cancel();
    const r = await controller.save(name, overwrite);
    if (r.ok) {
      this.fileName = r.reply.name;
      this.nameInput.classList.remove("invalid");
      state.setMessage({ kind: "info", text: `Circuit enregistré : ${r.reply.name}.` });
      this.refreshFiles();
      return;
    }
    const e = r.error;
    if (e.code === "file" && e.error.code === "name_exists") {
      // The controller explains it on the message line; the button now asks for the second click.
      this.confirm.arm(this.save, key, `Écraser « ${name} »`);
    } else if (e.code === "file" && e.error.code === "bad_name") {
      this.nameInput.classList.add("invalid");
    }
  }

  /**
   * Runs a document-replacing action, after a second click when there are unsaved changes. `file`: the saved circuit
   * the new document comes from (null: a new circuit or a demonstration, never saved).
   */
  private abandon(btn: HTMLButtonElement, key: string, action: () => Promise<{ ok: boolean }>, done: string, file: string | null): void {
    const { state } = this.host;
    if (!this.host.ready()) return;
    if (state.dirty && !this.confirm.isArmed(btn, key)) {
      this.confirm.arm(btn, key, "Abandonner les modifications ?");
      return;
    }
    this.confirm.cancel();
    void action().then((r) => {
      if (!r.ok) return;
      this.fileName = file;
      state.setMessage({ kind: "info", text: done });
    });
  }

  render(): void {
    const { state } = this.host;
    if (this.paletteShown !== state.palette) this.renderPalette();
    if (this.armedShown !== this.host.placing() || this.dirty) {
      this.armedShown = this.host.placing();
      for (const item of Array.from(this.palette.children) as HTMLElement[]) {
        const on = item.dataset.kind === this.armedShown;
        if (item.getAttribute("aria-pressed") !== String(on)) item.setAttribute("aria-pressed", String(on));
      }
    }
    if (this.demosShown !== state.demos) this.renderDemos();
    if (this.filesDirty) this.renderFiles();
    if (!this.dirty) return;
    this.dirty = false;
    const ready = this.host.ready();
    const name = state.name || "sans titre";
    setText(this.docLabel, name);
    if (this.docLabel.title !== name) this.docLabel.title = name;
    this.dirtyBadge.hidden = !state.dirty;
    // The save-as field follows the document name, unless the reader is typing in it.
    if (state.name !== this.docName) {
      this.docName = state.name;
      if (document.activeElement !== this.nameInput) this.nameInput.value = state.name;
    }
    // A mission bench is never saved (the core refuses it too: « Enregistrement indisponible en mission. »).
    const mission = state.disclosure === "mission";
    this.save.disabled = !ready || mission;
    const saveTitle = mission ? "Enregistrement indisponible en mission." : "";
    if (this.save.title !== saveTitle) this.save.title = saveTitle;
    this.fresh.disabled = !ready;
    this.open.disabled = !ready || !this.openSelect.value;
    this.demo.disabled = !ready || !this.demoSelect.value;
    for (const item of Array.from(this.palette.children) as HTMLButtonElement[]) item.disabled = !ready;
    setText(this.demoSummary, state.demos.find((d) => d.id === this.demoSelect.value)?.summary_fr ?? "");
  }

  private renderPalette(): void {
    const { state } = this.host;
    this.paletteShown = state.palette;
    clear(this.palette);
    for (const k of state.palette) {
      const item = el("button", "lab-item");
      item.type = "button";
      item.dataset.kind = k.kind;
      item.setAttribute("aria-pressed", "false");
      item.title = `${k.name_fr} : appuyer puis cliquer dans le schéma`;
      item.appendChild(el("span", undefined, k.name_fr));
      if (k.prefix) item.appendChild(el("span", "lab-prefix", k.prefix));
      this.palette.appendChild(item);
    }
    this.dirty = true;
  }

  private renderDemos(): void {
    const { state } = this.host;
    this.demosShown = state.demos;
    const options = state.demos.map((d) => ({ value: d.id, label: d.title_fr }));
    setOptions(this.demoSelect, options.length ? options : [{ value: "", label: "Aucune démonstration" }], this.demoSelect.value);
    this.dirty = true;
  }

  private renderFiles(): void {
    this.filesDirty = false;
    const options = this.files.map((f) => ({ value: f.name, label: f.name }));
    setOptions(this.openSelect, options.length ? options : [{ value: "", label: this.filesNote }], this.openSelect.value);
    this.dirty = true;
  }
}
