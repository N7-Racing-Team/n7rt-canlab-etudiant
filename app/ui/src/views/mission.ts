// Mission panel of the cockpit (docs/apps-mission-slice.md §6): the instructor's mission selector, then, once a
// mission is open, the public statement, the library, the nominal check and its result, in place of the training
// stimuli. With the reference pack (instructor) the check is « Vérifier l'essai nominal » and ends with a verdict;
// with a group pack (student, docs/distribution.md §2.1) it is « Enregistrer l'essai nominal » and ends with the name
// of the run file to hand in, never a verdict. During a check the bus view and the slots are covered by the progress
// message (ch. 24 §7.5). The modes and the student flow are in views/access.ts.
//
// Rules: text through textContent only (ui/DESIGN.md §8); the library is one flat list sorted by title, every
// document drawn and opened the same way (ch. 23 §2.3 rule 4); messages are the exact texts of ch. 24 §7.3; nothing
// here knows a frame, a signal or an expected value.

import { clear, el, setText } from "../dom";
import type { CheckView, LibraryItem, MissionEntry, MissionView, SlotLabel, Status } from "../types";

export const TRAINING = "training";
export const TRAINING_LABEL = "Entraînement (ECU virtuel)";
/** ch. 24 §7.3 M-06. */
export const BOARD_NOT_READY = "Connectez la carte et vérifiez que votre programme est chargé.";

/** ch. 24 §7.3: M-10 (during a check), the nominal verdict (M-03n), M-11 (cancelled). */
export const MESSAGES = {
  running: (n: number) => `Essai en cours — ${n} s. Ne débranchez pas la carte et ne fermez pas l'application.`,
  /** The covers during the final qualification (ch. 24 §6.1). */
  qualifying: (n: number) => `Qualification en cours — ${n} s. Ne débranchez pas la carte et ne fermez pas l'application.`,
  pass: "Essai nominal : réussi",
  fail: "Essai nominal : non réussi. Le détail n'est pas communiqué. Reprenez vos essais.",
  cancelled: (why: string) => `Vérification interrompue (${why}). Elle n'est pas comptée. Relancez quand le banc est prêt.`,
  /** Student: the run file was written (docs/distribution.md §2.1). */
  recorded: (name: string) => `Essai enregistré : ${name}. Transmettez ce fichier à votre encadrant.`,
};

/** The covers of a release 0.3 progression's attempt: the countdown (M-10) is the progression's message under its
 * button, said once; the covers only say what is hidden until the verdict. */
export const COVER = {
  bus: (qualifying: boolean) => `${qualifying ? "Qualification" : "Essai"} en cours : la vue du bus est masquée jusqu'au verdict.`,
  slots: (qualifying: boolean) => `${qualifying ? "Qualification" : "Essai"} en cours : les résultats sont masqués jusqu'au verdict.`,
};

export const CHECK_LABEL = { reference: "Vérifier l'essai nominal", group: "Enregistrer l'essai nominal" } as const;
const CHECK_TITLE = {
  reference: "Lance l'essai nominal sur votre carte : verdict réussi ou non réussi, sans détail.",
  group: "Joue l'essai nominal sur votre carte et enregistre le fichier d'essai à transmettre à votre encadrant.",
};

export interface SelectOption {
  value: string;
  label: string;
  disabled: boolean;
  /** Why the mission is unavailable (tooltip). */
  title: string;
}

export function missionOptions(list: MissionEntry[]): SelectOption[] {
  return [
    { value: TRAINING, label: TRAINING_LABEL, disabled: false, title: "" },
    ...list.map((m) => ({
      value: m.id,
      label: m.title,
      disabled: !m.available,
      title: m.available ? "" : `Indisponible : ${m.reason ?? "raison inconnue"}`,
    })),
  ];
}

/** Seconds left in a running check, rounded up. */
export function remainingSeconds(c: CheckView): number {
  return Math.max(0, Math.ceil(c.total_s - c.elapsed_s - 1e-9));
}

export type Tone = "" | "ok" | "warn" | "err";

export function checkMessage(c: CheckView): { text: string; tone: Tone } {
  switch (c.state) {
    case "running":
      return { text: MESSAGES.running(remainingSeconds(c)), tone: "" };
    case "done":
      return c.verdict === "pass" ? { text: MESSAGES.pass, tone: "ok" } : { text: MESSAGES.fail, tone: "err" };
    case "recorded":
      return { text: MESSAGES.recorded(c.run_file ?? "fichier d'essai"), tone: "ok" };
    case "cancelled":
      return { text: MESSAGES.cancelled(c.reason ?? "essai incomplet"), tone: "warn" };
    default:
      return { text: "", tone: "" };
  }
}

/** Public slot names of the open mission, by slot index (none without a mission). */
export function slotLabels(view: MissionView | null): Map<number, SlotLabel> {
  const m = new Map<number, SlotLabel>();
  if (view?.open && view.statement) for (const s of view.statement.slots) m.set(s.slot, s);
  return m;
}

/** « A1 · Course de la pédale vue par le capteur 1 ». */
export function slotName(s: SlotLabel): string {
  return `${s.code} · ${s.label}`;
}

/** The unit after a published value; logical outputs (0/1) stay bare. */
export function valueWithUnit(value: string, unit: string): string {
  return unit && unit !== "0/1" ? `${value} ${unit}` : value;
}

/** « Éditeur · référence · révision · date », the fields that exist. */
export function libraryMeta(d: LibraryItem): string {
  return [d.publisher, d.reference, d.revision, d.date ?? ""].filter((x) => x).join(" · ");
}

/** Board able to run a check: connected, runtime verified, student code loaded, not mute. */
/** Banner of the Physical Lab on a mission bench (mission mode: the core withholds the internal quantities). */
export function benchBanner(v: MissionView | null): string {
  const id = v?.open ? (v.id ?? "") : "";
  return `Banc de la mission${id ? ` ${id.toUpperCase()}` : ""} — mode mission : les grandeurs internes ne sont pas affichées.`;
}

export function boardReady(s: Status | null): boolean {
  if (!s || !s.port || s.stalled || !(s.state === "READY" || s.state === "SESSION")) return false;
  const b = s.board;
  return !!b && b.runtime_verified && b.compatible && b.student_status === 1;
}

export interface MissionElements {
  select: HTMLSelectElement;
  open: HTMLButtonElement;
  close: HTMLButtonElement;
  reset: HTMLButtonElement;
  error: HTMLElement;
  /** The « Stimuli — ECU virtuel » section, replaced by the mission panel. */
  stimuli: HTMLElement;
  section: HTMLElement;
  title: HTMLElement;
  summary: HTMLElement;
  requirements: HTMLElement;
  /** « Sorties à publier »: the slot of each output of the statement, the number the student code publishes. */
  outputs: HTMLElement;
  library: HTMLElement;
  check: HTMLButtonElement;
  result: HTMLElement;
  resultText: HTMLElement;
  /** « Ouvrir le dossier » of the run files, shown once a student run is recorded. */
  folder: HTMLButtonElement;
  progress: HTMLElement;
  bar: HTMLElement;
  /** Covers of the bus view and of the slots during a check. */
  frames: HTMLElement;
  framesCover: HTMLElement;
  framesCoverText: HTMLElement;
  framesCoverBar: HTMLElement;
  slots: HTMLElement;
  slotsCover: HTMLElement;
  /** The run bar under « Lancer la vérification » in the progression's action bar (same proportion as the covers). */
  progressRun?: HTMLElement;
  progressRunBar?: HTMLElement;
}

export interface MissionActions {
  open(id: string): void;
  close(): void;
  reset(): void;
  check(): void;
  openDocument(index: number): void;
  openRunsFolder(): void;
}

export class MissionPanel {
  private view: MissionView | null = null;
  private shownMission: string | null = null;
  private contentKey = "";
  private ready = false;
  private busy = false;
  private checkError = "";

  constructor(private e: MissionElements, private actions: MissionActions) {}

  mount(): void {
    const e = this.e;
    e.open.addEventListener("click", () => {
      const id = e.select.value;
      if (id === TRAINING) this.actions.close();
      else this.actions.open(id);
    });
    e.close.addEventListener("click", () => this.actions.close());
    e.reset.addEventListener("click", () => {
      if (this.view?.open && this.view.kind === "reference" && !this.running() && !this.busy) this.actions.reset();
    });
    e.check.addEventListener("click", () => {
      this.checkError = "";
      this.actions.check();
    });
    e.select.addEventListener("change", () => this.controls());
    e.folder.addEventListener("click", () => this.actions.openRunsFolder());
    e.library.addEventListener("click", (ev) => {
      const b = (ev.target as Element | null)?.closest?.("button");
      const i = b?.dataset.index;
      if (i !== undefined) this.actions.openDocument(Number(i));
    });
    this.render();
  }

  setList(list: MissionEntry[]): void {
    const sel = this.e.select;
    const keep = sel.value || this.view?.id || TRAINING;
    clear(sel);
    for (const o of missionOptions(list)) {
      const opt = el("option", undefined, o.label);
      opt.value = o.value;
      opt.disabled = o.disabled;
      if (o.title) opt.title = o.title;
      sel.appendChild(opt);
    }
    sel.value = missionOptions(list).some((o) => o.value === keep && !o.disabled) ? keep : TRAINING;
    this.controls();
  }

  setView(view: MissionView): void {
    if (view.check.state !== this.view?.check.state) this.checkError = "";
    this.view = view;
    this.render();
  }

  setBoard(status: Status | null): void {
    const ready = boardReady(status);
    if (ready !== this.ready) {
      this.ready = ready;
      this.controls();
    }
  }

  setBusy(busy: boolean): void {
    this.busy = busy;
    this.controls();
  }

  setError(text: string): void {
    setText(this.e.error, text);
  }

  /** A check that could not start (M-06…): shown in the result area until the next check state. */
  setCheckError(text: string): void {
    this.checkError = text;
    this.renderResult();
  }

  private running(): boolean {
    return this.view?.check.state === "running";
  }

  private controls(): void {
    const e = this.e;
    const v = this.view;
    const running = this.running();
    const current = v?.open ? v.id ?? TRAINING : TRAINING;
    e.select.disabled = running || this.busy;
    e.open.disabled = running || this.busy || e.select.value === current;
    e.close.disabled = running || this.busy || !v?.open;
    e.reset.hidden = v?.kind !== "reference";
    e.reset.disabled = running || this.busy || !v?.open || v.kind !== "reference";
    e.check.disabled = running || this.busy || !v?.open || !this.ready;
    // A release 0.3 pack verifies through its progression (views/progress.ts), never through the 0.2 nominal check.
    e.check.hidden = !!v?.progression;
    const kind = v?.kind === "group" ? "group" : "reference";
    if (e.check.textContent !== CHECK_LABEL[kind]) e.check.textContent = CHECK_LABEL[kind];
    const why = !v?.open || running ? "" : this.ready ? CHECK_TITLE[kind] : BOARD_NOT_READY;
    if (e.check.title !== why) e.check.title = why;
  }

  private render(): void {
    const e = this.e;
    const v = this.view;
    const open = !!v?.open;
    e.stimuli.hidden = open;
    e.section.hidden = !open;
    // The selector follows the open mission only when it changes (a choice not yet opened is kept).
    const openId = open ? v!.id : null;
    if (openId !== this.shownMission) {
      this.shownMission = openId;
      e.select.value = openId ?? TRAINING;
    }
    if (open) this.renderContent(v!);
    this.renderResult();
    this.renderCovers();
    this.controls();
  }

  private renderContent(v: MissionView): void {
    const st = v.statement;
    const key = JSON.stringify([v.id, v.group, st, v.library]);
    if (key === this.contentKey) return;
    this.contentKey = key;
    const e = this.e;
    setText(e.title, `Mission ${(v.id ?? "").toUpperCase()}${v.group ? ` — groupe ${v.group}` : ""}`);
    setText(e.summary, st?.summary ?? "");
    clear(e.requirements);
    for (const r of st?.requirements ?? []) e.requirements.appendChild(el("li", undefined, r));
    clear(e.outputs);
    for (const s of st?.slots ?? []) e.outputs.appendChild(el("li", undefined, `Slot ${s.slot} — ${slotName(s)}${s.unit ? ` (${s.unit})` : ""}`));
    clear(e.library);
    v.library.forEach((d, i) => {
      const li = el("li");
      const b = el("button", "lib-doc");
      b.type = "button";
      b.dataset.index = String(i);
      b.title = `Ouvrir « ${d.title} »`;
      b.appendChild(el("span", "lib-title", d.title));
      b.appendChild(el("span", "lib-meta", libraryMeta(d)));
      li.appendChild(b);
      e.library.appendChild(li);
    });
    if (!v.library.length) e.library.appendChild(el("li", "hint", "Bibliothèque introuvable sur ce poste."));
  }

  private renderResult(): void {
    const e = this.e;
    const c = this.view?.check;
    const msg = this.checkError ? { text: this.checkError, tone: "warn" as Tone } : c ? checkMessage(c) : { text: "", tone: "" as Tone };
    setText(e.resultText, msg.text);
    const cls = msg.tone ? `mission-result ${msg.tone}` : "mission-result";
    if (e.result.className !== cls) e.result.className = cls;
    // a release 0.3 pack words its verdicts and M-10 in the progression panel: one place, one message
    e.result.hidden = !msg.text || !!this.view?.progression;
    e.folder.hidden = !!this.checkError || c?.state !== "recorded";
    const running = c?.state === "running";
    e.progress.hidden = !running;
    if (running) this.setBar(e.bar, c!);
  }

  private renderCovers(): void {
    const e = this.e;
    const c = this.view?.check;
    const running = c?.state === "running";
    e.frames.hidden = running;
    e.framesCover.hidden = !running;
    e.slots.hidden = running;
    e.slotsCover.hidden = !running;
    if (e.progressRun) e.progressRun.hidden = !running;
    if (running && e.progressRunBar) this.setBar(e.progressRunBar, c!);
    if (running) {
      const progression = this.view?.progression;
      const qualifying = progression?.current === 4;
      if (progression) {
        setText(e.framesCoverText, COVER.bus(qualifying));
        setText(e.slotsCover, COVER.slots(qualifying));
      } else {
        const text = (qualifying ? MESSAGES.qualifying : MESSAGES.running)(remainingSeconds(c!));
        setText(e.framesCoverText, text);
        setText(e.slotsCover, text);
      }
      this.setBar(e.framesCoverBar, c!);
    }
  }

  private setBar(bar: HTMLElement, c: CheckView): void {
    const pct = c.total_s > 0 ? Math.min(100, Math.max(0, (100 * c.elapsed_s) / c.total_s)) : 0;
    const w = `${pct.toFixed(1)}%`;
    if (bar.style.width !== w) bar.style.width = w;
  }
}
