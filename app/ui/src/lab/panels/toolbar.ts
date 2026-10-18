// Workspace toolbar of the Physical Lab (contract docs/physical-lab-v1.md §8.4): tools Sélection / Fil, undo, redo,
// zoom, « Simulation : RUN / STOP / RESET » (the simulation only, never the STM32 session), simulated time,
// « ralenti » indicator and the message line (reason of the last refusal). Written in the animation frame only.

import { $, setText } from "../../dom";
import type { Editor } from "../editor";
import type { LabController, LabState } from "../state";
import { simTimeText, zoomText } from "../units";

export interface ToolbarHost {
  readonly state: LabState;
  readonly controller: LabController;
  /** Null while the editor is unavailable. */
  editor(): Editor | null;
  /** Library and document loaded. */
  ready(): boolean;
}

export class LabToolbar {
  private dirty = true;
  private readonly b: Record<"select" | "wire" | "undo" | "redo" | "zoomOut" | "zoomIn" | "fit" | "run" | "stop" | "reset", HTMLButtonElement>;
  private readonly zoom: HTMLElement;
  private readonly time: HTMLElement;
  private readonly slow: HTMLElement;
  private readonly messageRow: HTMLElement;
  private readonly message: HTMLElement;
  /** One-line banner of a mission bench (mission mode: internal quantities withheld by the core). */
  private readonly mission: HTMLElement;

  constructor(root: HTMLElement, private readonly host: ToolbarHost) {
    const btn = (id: string) => $<HTMLButtonElement>(`#${id}`, root);
    this.b = {
      select: btn("phys-tool-select"), wire: btn("phys-tool-wire"), undo: btn("phys-undo"), redo: btn("phys-redo"),
      zoomOut: btn("phys-zoom-out"), zoomIn: btn("phys-zoom-in"), fit: btn("phys-fit"),
      run: btn("phys-run"), stop: btn("phys-stop"), reset: btn("phys-reset"),
    };
    this.zoom = $("#phys-zoom", root);
    this.time = $("#phys-time", root);
    this.slow = $("#phys-slow", root);
    this.messageRow = $("#phys-message-row", root);
    this.message = $("#phys-message", root);
    this.mission = $("#phys-mission", root);
  }

  mount(): void {
    const { state, controller } = this.host;
    const ed = (fn: (e: Editor) => void) => () => {
      const e = this.host.editor();
      if (e) fn(e);
    };
    this.b.select.addEventListener("click", ed((e) => {
      e.cancelGesture();
      e.setTool("select");
    }));
    this.b.wire.addEventListener("click", ed((e) => e.setTool("wire")));
    this.b.undo.addEventListener("click", () => void controller.undo());
    this.b.redo.addEventListener("click", () => void controller.redo());
    this.b.zoomOut.addEventListener("click", ed((e) => e.zoomStep(-1)));
    this.b.zoomIn.addEventListener("click", ed((e) => e.zoomStep(1)));
    this.b.fit.addEventListener("click", ed((e) => e.fit()));
    this.b.run.addEventListener("click", () => void controller.runControl("run"));
    this.b.stop.addEventListener("click", () => void controller.runControl("stop"));
    this.b.reset.addEventListener("click", () => void controller.runControl("reset"));
    const mark = () => {
      this.dirty = true;
    };
    state.onStatus.on(mark);
    state.onView.on(mark);
    state.onMessage.on(mark);
    state.onValues.on(mark);
  }

  invalidate(): void {
    this.dirty = true;
  }

  render(): void {
    if (!this.dirty) return;
    this.dirty = false;
    const s = this.host.state;
    const ready = this.host.ready();
    const editor = this.host.editor() !== null;
    const set = (b: HTMLButtonElement, disabled: boolean) => {
      if (b.disabled !== disabled) b.disabled = disabled;
    };
    const pressed = (b: HTMLButtonElement, on: boolean) => {
      if (b.getAttribute("aria-pressed") !== String(on)) b.setAttribute("aria-pressed", String(on));
    };
    pressed(this.b.select, s.view.tool === "select");
    pressed(this.b.wire, s.view.tool === "wire");
    for (const b of [this.b.select, this.b.wire, this.b.zoomOut, this.b.zoomIn, this.b.fit]) set(b, !ready || !editor);
    set(this.b.undo, !ready || !s.canUndo);
    set(this.b.redo, !ready || !s.canRedo);
    set(this.b.run, !ready || s.run === "run");
    set(this.b.stop, !ready || s.run === "stop");
    set(this.b.reset, !ready);
    setText(this.zoom, zoomText(s.view.zoomPercent));
    setText(this.time, simTimeText(s.simTimeNs));
    const preview = s.run === "stop" && s.values?.preview === true;
    this.time.classList.toggle("preview", preview);
    const title = preview ? "Temps simulé (STOP : valeurs de l'aperçu, temps figé)" : "Temps simulé";
    if (this.time.title !== title) this.time.title = title;
    this.slow.hidden = !(s.values?.engine.slow ?? false);
    const m = s.message;
    setText(this.message, m?.text ?? "");
    const cls = m?.kind === "error" ? "message err" : "message";
    if (this.messageRow.className !== cls) this.messageRow.className = cls;
    const mission = s.disclosure === "mission";
    if (this.mission.hidden === mission) this.mission.hidden = !mission;
  }
}
