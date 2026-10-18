// Physical Lab view (contract docs/physical-lab-v1.md §8.4): the schematic editor of ./canvas (lot C) and the panels
// around it — library and file (left), workspace toolbar and console (centre), inspector (right). Hosted by the
// ViewHost of ui/src/main.ts: show() and hide() follow the activity bar; frame() runs in the single animation loop
// and is the only place where the view writes the DOM and the editor draws. Values are polled only while shown.
// The editor may be unavailable (createEditor throws, or a frame fails): the view then says so in the workspace and
// the rest keeps working.

import { $ } from "../dom";
import type { HostedView } from "../shell";
import type { ProgressionView } from "../types";
import { armedKind, createEditor } from "./canvas/index";
import type { Editor } from "./editor";
import { isField, keepFocusOnPress } from "./focus";
import { LabConsole } from "./panels/console";
import { Inspector } from "./panels/inspector";
import { MeterPanel } from "./panels/meter";
import { LibraryPanel } from "./panels/library";
import { LabToolbar } from "./panels/toolbar";
import { LabController, LabState } from "./state";
import type { CompId, Kind } from "./types";

/** One animation frame of the view, for the performance scenario (e2e-phys.ts). */
export interface FrameRecord {
  /** requestAnimationFrame timestamp. */
  now: number;
  /** performance.now() at the start and at the end of the view's frame callback. */
  start: number;
  end: number;
  /** The editor repainted a layer during this frame. */
  drew: boolean;
  /** Document revision mirrored, and revision of the values shown. */
  rev: number;
  valuesRev: number;
}

let instance: PhysView | null = null;

/** The Physical Lab view of this window (end-to-end scenarios). */
export function physView(): PhysView | null {
  return instance;
}

function describe(e: unknown): string {
  if (e instanceof Error) return e.message;
  if (typeof e === "string") return e;
  if (typeof e === "object" && e !== null && "code" in e) {
    const c = (e as { code: unknown }).code;
    if (c === "refused" && "refusal" in e) return (e as { refusal: { message_fr: string } }).refusal.message_fr;
    return String(c);
  }
  return String(e);
}

export class PhysView implements HostedView {
  readonly state = new LabState();
  readonly controller = new LabController(this.state);
  /** Null until created, or when unavailable. */
  editor: Editor | null = null;
  /** Library and document loaded from the core. */
  ready = false;
  /** Why the view or the editor is unavailable (French); null when everything works. */
  error: string | null = null;
  /** Called at the end of every frame while set (performance scenario). */
  frameObserver: ((r: FrameRecord) => void) | null = null;

  private active = false;
  private starting: Promise<void> | null = null;
  private editorFailed = false;
  private spaceHeld: CompId | null = null;
  /** Copies of key events given to the editor; ignored when they bubble back to the document. */
  private readonly forwarded = new WeakSet<Event>();
  /** Last position requested by the arrow keys without editor, until the core confirms it (1 s at most). */
  private potTarget: { id: CompId; position: number; at: number } | null = null;
  private readonly canvasHost: HTMLElement;
  private readonly stageError: HTMLElement;
  private readonly stageErrorText: HTMLElement;
  private readonly retry: HTMLButtonElement;
  private readonly toolbar: LabToolbar;
  private readonly library: LibraryPanel;
  private readonly inspector: Inspector;
  private readonly meter: MeterPanel;
  private readonly console: LabConsole;

  constructor() {
    const main = $("#phys-main");
    this.canvasHost = $("#phys-canvas", main);
    this.stageError = $("#phys-stage-error", main);
    this.stageErrorText = $("#phys-stage-error-text", main);
    this.retry = $<HTMLButtonElement>("#phys-retry", main);
    const host = {
      state: this.state,
      controller: this.controller,
      editor: () => (this.editorFailed ? null : this.editor),
      ready: () => this.ready,
      armPlacement: (kind: Kind) => this.armPlacement(kind),
      placing: () => this.placing(),
    };
    this.toolbar = new LabToolbar(main, host);
    this.library = new LibraryPanel($("#phys-left"), host);
    this.inspector = new Inspector($("#phys-inspector"), $("#phys-insp-what"), this.state, this.controller);
    this.meter = new MeterPanel($("#phys-meter"), this.state, this.controller);
    this.console = new LabConsole(main, this.state);
    instance = this;
  }

  mount(): void {
    this.toolbar.mount();
    this.library.mount();
    this.inspector.mount();
    this.meter.mount();
    this.console.mount();
    // Space and Enter stay with the schematic after a mouse press on any button the view owns (focus.ts).
    document.addEventListener("mousedown", (e) => {
      if (this.active && this.owns(e.target)) keepFocusOnPress(e);
    });
    document.addEventListener("keydown", (e) => this.onKey(e));
    document.addEventListener("keyup", (e) => this.onKey(e));
    window.addEventListener("blur", () => this.releaseSpace());
    this.retry.addEventListener("click", () => {
      if (this.ready) return;
      this.starting = null;
      if (this.active) this.show();
    });
    this.state.onDoc.on((e) => {
      const t = this.potTarget;
      if (e.kind === "state" && t?.id === e.comp && this.state.states.get(e.comp)?.position === t.position) this.potTarget = null;
    });
  }

  /** Resolves once the first start attempt is over (ready, or failed with `error` set). */
  whenReady(): Promise<void> {
    return this.starting ?? Promise.resolve();
  }

  show(): void {
    this.active = true;
    if (!this.starting) this.starting = this.start();
    else if (this.ready) this.resume();
    this.invalidatePanels();
  }

  hide(): void {
    this.active = false;
    this.controller.stopPolling();
    this.releaseSpace();
    this.inspector.releaseHold();
    this.callEditor((e) => {
      e.cancelGesture();
      e.setActive(false);
    });
  }

  frame(now: number): void {
    const start = performance.now();
    let drew = false;
    const ed = this.editorFailed ? null : this.editor;
    if (ed) {
      try {
        const before = ed.metrics.fullRepaints + ed.metrics.partialRepaints + ed.metrics.overlayRepaints;
        ed.frame(now);
        drew = ed.metrics.fullRepaints + ed.metrics.partialRepaints + ed.metrics.overlayRepaints !== before;
      } catch (e) {
        this.editorFailed = true;
        // A schematic that no longer draws must not take input either: its listeners and canvases go away.
        try {
          ed.destroy();
        } catch {
          /* already broken: nothing more to release */
        }
        this.editor = null;
        this.showError(`Erreur de l'éditeur de schéma, affichage arrêté : ${describe(e)}`, false);
        // The toolbar disables the editor's tools and the library drops its armed item in this same frame.
        this.invalidatePanels();
        console.error(e);
      }
    }
    this.toolbar.render();
    this.library.render();
    this.inspector.render();
    this.meter.render();
    this.console.render(now);
    this.frameObserver?.({ now, start, end: performance.now(), drew, rev: this.state.rev, valuesRev: this.state.values?.rev ?? -1 });
  }

  /** The shell switched the console tab. */
  tabShown(tab: string): void {
    this.console.tabShown(tab);
  }

  /** The progression of the open pack: the multimeter's « Mesure clé » during an incident, its probes locked during a
   * verification (lab/panels/meter.ts). */
  setMission(p: ProgressionView | null, consign: () => Promise<void>): void {
    this.meter.setMission(p, consign);
  }

  /** Arms the placement of a kind (library); false when the editor is unavailable. */
  armPlacement(kind: Kind): boolean {
    let ok = false;
    this.callEditor((e) => {
      e.armPlacement(kind);
      ok = true;
    });
    return ok;
  }

  /** Kind whose placement is armed in the editor: the editor alone ends it (click, Escape, tool, undo, hide). */
  private placing(): Kind | null {
    const ed = this.editorFailed ? null : this.editor;
    return ed ? armedKind(ed) : null;
  }

  // ------------------------------------------------------------------ lifecycle

  private async start(): Promise<void> {
    this.showError(null, false);
    try {
      await this.controller.init();
    } catch (e) {
      this.ready = false;
      this.showError(`Physical Lab indisponible : ${describe(e)}`, true);
      return;
    }
    this.ready = true;
    try {
      this.editor = createEditor(this.canvasHost, this.state, this.controller);
      this.editorFailed = false;
    } catch (e) {
      this.editor = null;
      this.showError(`Éditeur de schéma indisponible : ${describe(e)}. Les panneaux, la simulation et les fichiers restent utilisables.`, false);
    }
    this.library.refreshFiles();
    // Hidden while starting: the editor (created active) must not keep its canvases until the view is shown.
    if (this.active) this.resume();
    else this.callEditor((e) => e.setActive(false));
    this.invalidatePanels();
  }

  private resume(): void {
    this.controller.startPolling();
    this.callEditor((e) => {
      e.setActive(true);
      e.invalidate();
    });
  }

  private invalidatePanels(): void {
    this.toolbar.invalidate();
    this.library.invalidate();
    this.inspector.invalidate();
    this.console.invalidate();
  }

  private showError(text: string | null, retry: boolean): void {
    this.error = text;
    this.stageError.hidden = text === null;
    this.stageErrorText.textContent = text ?? "";
    this.retry.hidden = !retry;
  }

  private callEditor(fn: (e: Editor) => void): void {
    const ed = this.editorFailed ? null : this.editor;
    if (!ed) return;
    try {
      fn(ed);
    } catch (e) {
      this.state.setMessage({ kind: "error", text: `Éditeur de schéma : ${describe(e)}` });
      console.error(e);
    }
  }

  // ------------------------------------------------------------------ keyboard (§8.4)
  //
  // The editor implements the keys of §8.4 on its host (Suppr, Ctrl+Z/Y, R, W, Échap, F, 1, Ctrl+A, Espace,
  // flèches) and consumes what it handles. Keys typed elsewhere in the view (library, toolbar, inspector buttons,
  // no focus) are given to it, so that one implementation decides. Without an editor, the view handles what still
  // makes sense (history, deletion, rotation, push button, potentiometer).

  /**
   * Keys reach the view from its own slots, from the shared frame (activity bar, panel toggles) or with no focused
   * element; never from a form field or from the cockpit's slots.
   */
  private owns(t: EventTarget | null): boolean {
    if (!(t instanceof Element) || t === document.body || t === document.documentElement) return true;
    return t.closest('.workbench > [data-view="cockpit"]') === null && !isField(t);
  }

  /** The only selected item, if it is a component of this kind. */
  private single(kind: Kind): CompId | null {
    const { components, wires } = this.state.selection;
    if (components.size !== 1 || wires.size !== 0) return null;
    const id = components.values().next().value as CompId;
    return this.state.components.get(id)?.kind === kind ? id : null;
  }

  /** The only selected item, if it is driven by set_pot (potentiometer, pedal sensor). */
  private slider(): CompId | null {
    return this.single("potentiometer") ?? this.single("mlx90377");
  }

  private onKey(e: KeyboardEvent): void {
    if (!this.active || !this.ready || e.defaultPrevented || e.isComposing || this.forwarded.has(e) || !this.owns(e.target)) return;
    const ed = this.editorFailed ? null : this.editor;
    if (ed) {
      if (e.target instanceof Node && this.canvasHost.contains(e.target)) return; // already the editor's
      // A focused button keeps its own action; its keyup still goes to the editor, which may hold a push button
      // since a Space pressed elsewhere (a keyup it does not expect is a no-op). Only the keyboard focuses a button
      // here: mouse presses leave the focus where it was (keepFocusOnPress).
      if (e.type === "keydown" && (e.key === " " || e.key === "Enter") && e.target instanceof HTMLButtonElement) return;
      const copy = new KeyboardEvent(e.type, {
        key: e.key, code: e.code, ctrlKey: e.ctrlKey, shiftKey: e.shiftKey, altKey: e.altKey, metaKey: e.metaKey,
        repeat: e.repeat, bubbles: true, cancelable: true,
      });
      this.forwarded.add(copy);
      this.canvasHost.dispatchEvent(copy);
      if (copy.defaultPrevented) e.preventDefault();
      return;
    }
    if (e.type === "keyup") {
      if (e.key === " " && this.spaceHeld !== null) {
        e.preventDefault();
        this.releaseSpace();
      }
      return;
    }
    if (this.fallbackKey(e)) e.preventDefault();
  }

  /** Keys without an editor; true when handled. */
  private fallbackKey(e: KeyboardEvent): boolean {
    const k = e.key.length === 1 ? e.key.toLowerCase() : e.key;
    const sel = this.state.selection;
    const items = { components: [...sel.components], wires: [...sel.wires] };
    const some = items.components.length + items.wires.length > 0;
    if ((e.ctrlKey || e.metaKey) && !e.altKey) {
      if (k === "z") void (e.shiftKey ? this.controller.redo() : this.controller.undo());
      else if (k === "y") void this.controller.redo();
      else if (k === "a") this.state.setSelection(this.state.components.keys(), this.state.wires.keys());
      else return false;
      return true;
    }
    if (e.altKey || e.ctrlKey || e.metaKey) return false;
    if (k === "Delete" && some) void this.controller.edit({ op: "delete", ...items });
    else if (k === "r" && some) void this.controller.edit({ op: "rotate", ...items });
    else if (k === "Escape" && some) this.state.setSelection([], []);
    else if (k === " " && !(e.target instanceof HTMLButtonElement) && this.single("push_button") !== null) {
      const id = this.single("push_button")!;
      if (!e.repeat && this.spaceHeld === null) {
        this.spaceHeld = id;
        this.controller.interact({ op: "set_button", comp: id, pressed: true });
      }
    } else if ((k === "ArrowLeft" || k === "ArrowRight" || k === "ArrowUp" || k === "ArrowDown") && this.slider() !== null) {
      this.stepPot(this.slider()!, k === "ArrowRight" || k === "ArrowUp" ? 1 : -1, e.shiftKey);
    } else return false;
    return true;
  }

  private releaseSpace(): void {
    const id = this.spaceHeld;
    if (id === null) return;
    this.spaceHeld = null;
    if (this.state.components.has(id)) this.controller.interact({ op: "set_button", comp: id, pressed: false });
  }

  /** ±0.01 (Shift ±0.1), from the position requested last so that held keys accumulate before the replies. */
  private stepPot(id: CompId, direction: 1 | -1, coarse: boolean): void {
    const current = this.state.states.get(id)?.position;
    const t = this.potTarget;
    const pending = t !== null && t.id === id && performance.now() - t.at < 1000;
    const base = pending ? t.position : typeof current === "number" ? current : null;
    if (base === null) return;
    const next = Math.min(1, Math.max(0, Math.round((base + direction * (coarse ? 0.1 : 0.01)) * 1000) / 1000));
    if (next === base) return;
    this.potTarget = { id, position: next, at: performance.now() };
    this.controller.interact({ op: "set_pot", comp: id, position: next });
  }
}
