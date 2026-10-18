// Student and instructor modes in the cockpit (docs/distribution.md §2): the mode line and its button, the instructor
// code dialog, the student flow (choice of the session's mission with its confirmation, then the locked mission, the
// group pack import and the group code) and the instructor tools (reset, group packs of a cohort, judgement of a run
// file). The instructor's mission selector, the mission panel and the nominal check stay in views/mission.ts.
//
// Rules: text through textContent only (ui/DESIGN.md §8); every refusal is the core's French sentence, shown as it is;
// a code typed here is sent once and the field is emptied; the student never sees a verdict.

import { clear, el, setText } from "../dom";
import type { AccessView, GenRow, Judgement, MissionEntry, MissionView } from "../types";

export const MODE_LABEL = { student: "Mode étudiant", instructor: "Mode encadrant" } as const;
/** Instructor mode of a build without the oracle: everything but the verdicts (docs/distribution.md §1). */
export const PUBLIC_EDITION = "édition publique, sans verdict";

/** Text of the mode line. */
export function modeText(a: AccessView | null | undefined): string {
  if (a?.mode !== "instructor") return MODE_LABEL.student;
  return a.oracle ? MODE_LABEL.instructor : `${MODE_LABEL.instructor} — ${PUBLIC_EDITION}`;
}
export const ENTER_INSTRUCTOR = "Mode encadrant";
export const LEAVE_INSTRUCTOR = "Quitter le mode encadrant";
/** Shown before the session's mission is locked (written into the confirmation at mount). */
export const CONFIRM_CHOICE = "Ce choix est définitif pour la session : pour en changer, demandez à votre encadrant.";
export const RESET_LABEL = "Réinitialiser";
export const RESET_CONFIRM = "Confirmer la réinitialisation";
/** Two-click confirmation (ui/DESIGN.md, Confirmation): 5 s at most. */
export const CONFIRM_MS = 5000;
export const NO_PACK = "Aucun pack importé.";
/** Why « Ouvrir » or « Fermer » of the student flow is disabled (their title). */
export const CHECK_RUNNING = "Vérification en cours : attendez la fin de l'essai.";
export const NEED_PACK = "Importez d'abord le pack du groupe.";
export const NEED_CODE = "Saisissez le code du groupe.";
export const PACK_OPEN = "Le pack du groupe est déjà ouvert.";
export const NOTHING_OPEN = "Aucune mission ouverte.";
export const COMMAND_RUNNING = "Commande en cours…";
/** Bounds of the files sent to the core, checked on File.size before reading (the core checks them again): the same
 * values as n7rt_tool::MAX_PACK_BYTES and MAX_RUN_BYTES (a release 0.3 pack holds its hidden tests, 3 to 4 MiB). */
export const MAX_PACK_BYTES = 8 << 20;
export const MAX_RUN_BYTES = 8 << 20;
/** The core's sentences for a file over its bound. */
export const PACK_TOO_LARGE = "Fichier trop volumineux pour un pack de groupe.";
export const RUN_TOO_LARGE = "Fichier trop volumineux pour un fichier d'essai.";

/** « G01 — cohorte promo-2026 ». */
export function packLabel(a: AccessView): string {
  const p = a.student.pack;
  return p ? `${p.group} — cohorte ${p.cohort}` : NO_PACK;
}

/** Groups typed as « G01, G02 G03 ; G04 ». */
export function parseGroups(text: string): string[] {
  const out: string[] = [];
  for (const g of text.split(/[\s,;]+/)) if (g && !out.includes(g)) out.push(g);
  return out;
}

/** « Essai jugé : réussi » / « Essai jugé : non réussi » (instructor only). */
export function judgementText(j: Judgement): string {
  return `Essai jugé : ${j.verdict === "pass" ? "réussi" : "non réussi"}`;
}

export function judgementMeta(j: Judgement): string {
  const parts = [`Groupe ${j.group}`, `cohorte ${j.cohort}`, `mission ${j.mission.toUpperCase()}`];
  if (j.started_utc) parts.push(`essai du ${j.started_utc}`);
  return parts.join(" · ");
}

/** Bytes → standard base64 (the transport of files to the core). */
export function bytesToBase64(b: Uint8Array): string {
  let s = "";
  const chunk = 0x8000;
  for (let i = 0; i < b.length; i += chunk) s += String.fromCharCode(...b.subarray(i, i + chunk));
  return btoa(s);
}

export async function fileToBase64(f: Blob): Promise<string> {
  return bytesToBase64(new Uint8Array(await f.arrayBuffer()));
}

export interface AccessElements {
  modeLine: HTMLElement;
  modeButton: HTMLButtonElement;
  error: HTMLElement;
  // student
  studentFlow: HTMLElement;
  choice: HTMLElement;
  studentSelect: HTMLSelectElement;
  choose: HTMLButtonElement;
  confirm: HTMLElement;
  confirmText: HTMLElement;
  confirmYes: HTMLButtonElement;
  confirmNo: HTMLButtonElement;
  locked: HTMLElement;
  lockedTitle: HTMLElement;
  pack: HTMLElement;
  importButton: HTMLButtonElement;
  packFile: HTMLInputElement;
  code: HTMLInputElement;
  open: HTMLButtonElement;
  close: HTMLButtonElement;
  // instructor
  instructorFlow: HTMLElement;
  reset: HTMLButtonElement;
  packs: HTMLButtonElement;
  judge: HTMLButtonElement;
  runFile: HTMLInputElement;
  judgeResult: HTMLElement;
  judgeVerdict: HTMLElement;
  judgeMeta: HTMLElement;
  judgeDetail: HTMLElement;
  // code dialog
  codeDialog: HTMLDialogElement;
  codeInput: HTMLInputElement;
  codeSubmit: HTMLButtonElement;
  codeCancel: HTMLButtonElement;
  codeError: HTMLElement;
  // group packs dialog
  packsDialog: HTMLDialogElement;
  packsMission: HTMLSelectElement;
  packsCohort: HTMLInputElement;
  packsGroups: HTMLInputElement;
  /** « Régénérer les groupes déjà présents » (new code, new pack). */
  packsReplace: HTMLInputElement;
  packsGenerate: HTMLButtonElement;
  packsClose: HTMLButtonElement;
  packsError: HTMLElement;
  packsTable: HTMLElement;
  packsRows: HTMLElement;
}

export interface AccessActions {
  login(code: string): Promise<AccessView>;
  logout(): Promise<AccessView>;
  reset(): Promise<AccessView>;
  choose(id: string): Promise<AccessView>;
  importPack(data: string): Promise<AccessView>;
  open(code: string): Promise<MissionView>;
  close(): Promise<MissionView>;
  genPacks(mission: string, cohort: string, groups: string[], replace: boolean): Promise<GenRow[]>;
  /** `name`: the chosen file's name (the judgement is kept under it). */
  judge(name: string, data: string): Promise<Judgement>;
  /** A mission view came back (student open or close). */
  mission(v: MissionView): void;
  /** The mode or the student state changed: the mission list and state are read again. */
  changed(): void;
}

export class AccessPanel {
  private access: AccessView | null = null;
  private mission: MissionView | null = null;
  private list: MissionEntry[] = [];
  private busy = false;
  private resetArmed: ReturnType<typeof setTimeout> | null = null;

  constructor(private e: AccessElements, private actions: AccessActions) {}

  mount(): void {
    const e = this.e;
    setText(e.confirmText, CONFIRM_CHOICE);
    e.modeButton.addEventListener("click", () => {
      if (this.access?.mode === "instructor") void this.run(() => this.actions.logout(), (a) => this.applyAccess(a, true));
      else this.openCodeDialog();
    });
    e.codeSubmit.addEventListener("click", () => void this.submitCode());
    e.codeInput.addEventListener("keydown", (ev) => {
      if ((ev as KeyboardEvent).key === "Enter") void this.submitCode();
    });
    e.codeCancel.addEventListener("click", () => this.closeCodeDialog());
    e.choose.addEventListener("click", () => {
      if (!e.studentSelect.value) return;
      e.confirm.hidden = false;
      this.controls();
    });
    e.confirmNo.addEventListener("click", () => {
      e.confirm.hidden = true;
      this.controls();
    });
    e.confirmYes.addEventListener("click", () => {
      const id = e.studentSelect.value;
      e.confirm.hidden = true;
      void this.run(() => this.actions.choose(id), (a) => this.applyAccess(a, true));
    });
    e.studentSelect.addEventListener("change", () => {
      e.confirm.hidden = true;
      this.controls();
    });
    e.importButton.addEventListener("click", () => e.packFile.click());
    e.packFile.addEventListener("change", () => void this.importSelected());
    e.code.addEventListener("keydown", (ev) => {
      if ((ev as KeyboardEvent).key === "Enter") this.openGroup();
    });
    e.code.addEventListener("input", () => this.controls());
    e.open.addEventListener("click", () => this.openGroup());
    e.close.addEventListener("click", () => void this.run(() => this.actions.close(), (v) => this.actions.mission(v)));
    e.reset.addEventListener("click", () => this.resetClicked());
    e.packs.addEventListener("click", () => this.openPacksDialog());
    e.packsGenerate.addEventListener("click", () => void this.generate());
    e.packsClose.addEventListener("click", () => this.e.packsDialog.close());
    e.judge.addEventListener("click", () => e.runFile.click());
    e.runFile.addEventListener("change", () => void this.judgeSelected());
    this.render();
  }

  setAccess(a: AccessView): void {
    this.access = a;
    if (a.mode !== "instructor") {
      this.e.judgeResult.hidden = true;
      this.disarmReset();
    }
    this.render();
  }

  setMission(v: MissionView): void {
    this.mission = v;
    this.controls();
  }

  /** The core's list (student mode: the four missions, the playable ones can be chosen). */
  setList(list: MissionEntry[]): void {
    this.list = list;
    const sel = this.e.studentSelect;
    const keep = sel.value;
    clear(sel);
    for (const m of list) {
      const opt = el("option", undefined, m.title);
      opt.value = m.id;
      opt.disabled = !m.available;
      if (!m.available) opt.title = `Indisponible : ${m.reason ?? "raison inconnue"}`;
      sel.appendChild(opt);
    }
    const first = list.find((m) => m.available)?.id ?? "";
    sel.value = list.some((m) => m.id === keep && m.available) ? keep : first;
    this.fillPacksMissions();
    this.controls();
  }

  setError(text: string): void {
    setText(this.e.error, text);
  }

  // ------------------------------------------------------------------------------------------------ render

  private render(): void {
    const e = this.e;
    const a = this.access;
    const instructor = a?.mode === "instructor";
    setText(e.modeLine, modeText(a));
    const badge = instructor ? "badge badge--accent" : "badge";
    if (e.modeLine.className !== badge) e.modeLine.className = badge;
    setText(e.modeButton, instructor ? LEAVE_INSTRUCTOR : ENTER_INSTRUCTOR);
    e.studentFlow.hidden = instructor;
    e.instructorFlow.hidden = !instructor;
    const locked = a?.student.locked ?? null;
    e.choice.hidden = !!locked;
    e.locked.hidden = !locked;
    if (locked) e.confirm.hidden = true;
    setText(e.lockedTitle, a?.student.locked_title ?? (locked ? locked.toUpperCase() : "—"));
    setText(e.pack, a ? packLabel(a) : NO_PACK);
    this.controls();
  }

  private running(): boolean {
    return this.mission?.check.state === "running";
  }

  private controls(): void {
    const e = this.e;
    const a = this.access;
    const busy = this.busy || this.running();
    const groupOpen = !!this.mission?.open && this.mission.kind === "group";
    e.modeButton.disabled = busy || !a;
    e.studentSelect.disabled = busy || !e.confirm.hidden;
    e.choose.disabled = busy || !e.studentSelect.value || !e.confirm.hidden;
    e.confirmYes.disabled = busy;
    e.importButton.disabled = busy || !a?.student.locked;
    e.code.disabled = busy || !a?.student.pack;
    e.open.disabled = busy || !a?.student.pack || !e.code.value || groupOpen;
    e.close.disabled = busy || !this.mission?.open;
    // a disabled button says why (title)
    const waiting = this.running() ? CHECK_RUNNING : "";
    const openWhy = !e.open.disabled ? "" : waiting || (!a?.student.pack ? NEED_PACK : groupOpen ? PACK_OPEN : !e.code.value ? NEED_CODE : COMMAND_RUNNING);
    if (e.open.title !== openWhy) e.open.title = openWhy;
    const closeWhy = !e.close.disabled ? "" : waiting || (!this.mission?.open ? NOTHING_OPEN : COMMAND_RUNNING);
    if (e.close.title !== closeWhy) e.close.title = closeWhy;
    // once the group pack is open, the setup steps (import, code, « Ouvrir ») give way to « Fermer »
    const cls = groupOpen ? "mission-block student-locked is-open" : "mission-block student-locked";
    if (e.locked.className !== cls) e.locked.className = cls;
    e.reset.disabled = busy;
    e.packs.disabled = busy;
    e.judge.disabled = busy;
  }

  /** One command at a time; a refusal is shown under the panel. */
  private async run<T>(cmd: () => Promise<T>, done: (v: T) => void): Promise<boolean> {
    this.busy = true;
    this.controls();
    try {
      done(await cmd());
      this.setError("");
      return true;
    } catch (err) {
      this.setError(String(err));
      return false;
    } finally {
      this.busy = false;
      this.controls();
    }
  }

  private applyAccess(a: AccessView, changed: boolean): void {
    this.setAccess(a);
    if (changed) this.actions.changed();
  }

  // ------------------------------------------------------------------------------------------------ code dialog

  private openCodeDialog(): void {
    const e = this.e;
    e.codeInput.value = "";
    setText(e.codeError, "");
    e.codeSubmit.disabled = false;
    if (!e.codeDialog.open) e.codeDialog.showModal();
    e.codeInput.focus();
  }

  private closeCodeDialog(): void {
    this.e.codeInput.value = "";
    if (this.e.codeDialog.open) this.e.codeDialog.close();
  }

  private async submitCode(): Promise<void> {
    const e = this.e;
    const code = e.codeInput.value;
    if (!code || e.codeSubmit.disabled) return;
    e.codeInput.value = "";
    e.codeSubmit.disabled = true;
    setText(e.codeError, "");
    try {
      const a = await this.actions.login(code);
      this.closeCodeDialog();
      this.applyAccess(a, true);
      this.setError("");
    } catch (err) {
      setText(e.codeError, String(err));
      e.codeInput.focus();
    } finally {
      e.codeSubmit.disabled = false;
    }
  }

  // ------------------------------------------------------------------------------------------------ student

  private async importSelected(): Promise<void> {
    const input = this.e.packFile;
    const f = input.files?.[0];
    if (!f) return;
    try {
      if (f.size > MAX_PACK_BYTES) {
        this.setError(PACK_TOO_LARGE);
        return;
      }
      const data = await fileToBase64(f);
      await this.run(() => this.actions.importPack(data), (a) => this.applyAccess(a, false));
    } catch (err) {
      this.setError(String(err));
    } finally {
      input.value = "";
    }
  }

  private openGroup(): void {
    const code = this.e.code.value;
    if (!code || this.e.open.disabled) return;
    void this.run(() => this.actions.open(code), (v) => {
      this.e.code.value = "";
      this.actions.mission(v);
    });
  }

  // ------------------------------------------------------------------------------------------------ instructor

  private disarmReset(): void {
    if (this.resetArmed !== null) clearTimeout(this.resetArmed);
    this.resetArmed = null;
    setText(this.e.reset, RESET_LABEL);
    if (this.e.reset.className !== "btn") this.e.reset.className = "btn";
  }

  private resetClicked(): void {
    if (this.resetArmed === null) {
      setText(this.e.reset, RESET_CONFIRM);
      this.e.reset.className = "btn btn--danger";
      this.e.reset.title = "Efface le choix de mission et les packs importés sur ce poste, ferme la mission.";
      this.resetArmed = setTimeout(() => this.disarmReset(), CONFIRM_MS);
      return;
    }
    this.disarmReset();
    void this.run(() => this.actions.reset(), (a) => this.applyAccess(a, true));
  }

  private fillPacksMissions(): void {
    const sel = this.e.packsMission;
    const keep = sel.value;
    clear(sel);
    for (const m of this.list) {
      const opt = el("option", undefined, m.title);
      opt.value = m.id;
      opt.disabled = !m.available;
      if (!m.available) opt.title = `Indisponible : ${m.reason ?? "raison inconnue"}`;
      sel.appendChild(opt);
    }
    const first = this.list.find((m) => m.available)?.id ?? "";
    sel.value = this.list.some((m) => m.id === keep && m.available) ? keep : first;
  }

  private openPacksDialog(): void {
    const e = this.e;
    setText(e.packsError, "");
    this.fillPacksMissions();
    if (!e.packsDialog.open) e.packsDialog.showModal();
  }

  private async generate(): Promise<void> {
    const e = this.e;
    const mission = e.packsMission.value;
    const cohort = e.packsCohort.value.trim();
    const groups = parseGroups(e.packsGroups.value);
    if (!mission || !cohort || !groups.length) {
      setText(e.packsError, "Indiquez la mission, la cohorte et au moins un groupe.");
      return;
    }
    e.packsGenerate.disabled = true;
    setText(e.packsError, "");
    try {
      this.showRows(await this.actions.genPacks(mission, cohort, groups, e.packsReplace.checked));
    } catch (err) {
      setText(e.packsError, String(err));
    } finally {
      e.packsGenerate.disabled = false;
    }
  }

  private showRows(rows: GenRow[]): void {
    const body = this.e.packsRows;
    clear(body);
    for (const r of rows) {
      const tr = el("tr");
      tr.appendChild(el("td", undefined, r.group));
      tr.appendChild(el("td", "code", r.code));
      tr.appendChild(el("td", "dim", r.file));
      body.appendChild(tr);
    }
    this.e.packsTable.hidden = rows.length === 0;
  }

  private async judgeSelected(): Promise<void> {
    const e = this.e;
    const f = e.runFile.files?.[0];
    if (!f) return;
    try {
      if (f.size > MAX_RUN_BYTES) {
        this.setError(RUN_TOO_LARGE);
        return;
      }
      const data = await fileToBase64(f);
      const name = (f as File).name ?? "essai.n7rtrun";
      await this.run(() => this.actions.judge(name, data), (j) => this.showJudgement(j));
    } catch (err) {
      this.setError(String(err));
    } finally {
      e.runFile.value = "";
    }
  }

  private showJudgement(j: Judgement): void {
    const e = this.e;
    setText(e.judgeVerdict, judgementText(j));
    setText(e.judgeMeta, judgementMeta(j) + (j.judged_file ? ` · conservé dans ${j.judged_file}` : ""));
    setText(e.judgeDetail, j.detail);
    const cls = `mission-result ${j.verdict === "pass" ? "ok" : "err"}`;
    if (e.judgeResult.className !== cls) e.judgeResult.className = cls;
    e.judgeResult.hidden = false;
  }
}
