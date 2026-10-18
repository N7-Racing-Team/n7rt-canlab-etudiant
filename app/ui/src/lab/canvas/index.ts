// Schematic editor (lot C), contract docs/physical-lab-v1.md §8.5 and ui/src/lab/editor.ts. Two stacked canvases
// in the host: the scene (opaque: grid, wires, symbols, squares, texts) and the overlay (hover, selection, states,
// previews, live values). Input handlers only change state and mark layers dirty; frame(now), called by the view's
// single rAF loop, draws each dirty layer at most once. The editor owns no document data: it draws LabState and
// turns gestures into LabController calls (gestures.ts).

import type { CreateEditor, Editor, EditorMetrics } from "../editor";
import type { DocEvent, LabController, LabState, Tool } from "../state";
import type { Kind, Pt } from "../types";
import { DirtyRegion } from "./dirty";
import type { FPt } from "./geom";
import { Gestures, type KeyInput, type Mods } from "./gestures";
import {
  type DiagMarker, type LiveItem, diagMarkers, diagSignature, liveItems, liveSignature, stateSignature,
} from "./live";
import { SceneModel } from "./model";
import { Renderer, TEXT_PX, readPalette } from "./render";
import { TextWidthCache } from "./text-width-cache";
import { Viewport, stepNearest, zoomPercent } from "./view";

/** Frames kept in EditorMetrics.renderMs. */
const RING = 600;
/** Canvas allocation slack: reallocated only when the host size varies by more than 25 %. */
const SLACK = 1.25;
const FONT_PROBE = '400 11px "IBM Plex Mono"';
/** The first draw waits for the font, at most this long (a load that never settles must not blank the editor). */
const FONT_WAIT_MS = 1000;

const mods = (e: MouseEvent): Mods => ({ shift: e.shiftKey, ctrl: e.ctrlKey || e.metaKey });

const keyInput = (e: KeyboardEvent): KeyInput =>
  ({ key: e.key, code: e.code, shift: e.shiftKey, ctrl: e.ctrlKey || e.metaKey, alt: e.altKey, repeat: e.repeat });

function isFormField(t: EventTarget | null): boolean {
  return t instanceof HTMLElement && (t.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(t.tagName));
}

class SchematicEditor implements Editor {
  readonly metrics: EditorMetrics = { renderMs: [], fullRepaints: 0, partialRepaints: 0, overlayRepaints: 0 };
  private readonly scene: HTMLCanvasElement;
  private readonly overlay: HTMLCanvasElement;
  private readonly sctx: CanvasRenderingContext2D | null;
  private readonly octx: CanvasRenderingContext2D | null;
  private readonly vp = new Viewport();
  private readonly model: SceneModel;
  private readonly renderer: Renderer;
  private readonly gestures: Gestures;
  private readonly dirty = new DirtyRegion();
  private readonly widths = new TextWidthCache();
  private readonly ro: ResizeObserver;
  private readonly offs: (() => void)[] = [];
  private overlayDirty = true;
  private active = true;
  private destroyed = false;
  private sizePending = true;
  private positioned = false;
  /** Host CSS size at the last canvas allocation. */
  private allocBase: [number, number] = [0, 0];
  private fitPending = true;
  private docName: string;
  private live: LiveItem[] = [];
  private liveSig = "";
  private diags: DiagMarker[] = [];
  private diagSig = "";
  private stateSig = "";
  private pan: { id: number; x: number; y: number; fx: number; fy: number } | null = null;
  private spacePan = false;
  private captured: number | null = null;
  private wheelAcc = 0;
  private cursor = "";
  private mq: MediaQueryList | null = null;
  private fontTimer: number | null = null;

  constructor(private readonly host: HTMLElement, private readonly state: LabState, controller: LabController) {
    this.scene = document.createElement("canvas");
    this.overlay = document.createElement("canvas");
    for (const c of [this.scene, this.overlay]) {
      c.width = 0;
      c.height = 0;
      c.style.position = "absolute";
      c.style.left = "0";
      c.style.top = "0";
      c.style.width = "0px";
      c.style.height = "0px";
      c.setAttribute("aria-hidden", "true");
    }
    this.overlay.style.touchAction = "none";
    host.style.overflow = "hidden";
    if (host.tabIndex < 0) host.tabIndex = 0;
    if (!host.hasAttribute("aria-label")) host.setAttribute("aria-label", "Éditeur de schéma");
    host.append(this.scene, this.overlay);
    this.sctx = this.scene.getContext("2d", { alpha: false });
    this.octx = this.overlay.getContext("2d");

    this.vp.dpr = window.devicePixelRatio || 1;
    // measure() reads the renderer's font when the model first lays texts out (rebuild below).
    this.model = new SceneModel((k) => state.library.get(k), (t) => this.measure(t));
    this.renderer = new Renderer(this.vp, this.model, state, readPalette(host));
    this.gestures = new Gestures(state, controller, this.model, {
      unitsPerPx: () => 1 / this.vp.uCss,
      fit: () => this.fit(),
      zoomTo: (r) => this.zoomTo(r),
      changed: () => {
        this.overlayDirty = true;
      },
    });
    this.model.rebuild(state.components.values(), state.wires.values());
    this.docName = state.name;
    if (this.state.view.tool !== this.gestures.tool) this.gestures.setTool(this.state.view.tool);

    this.overlay.addEventListener("pointerdown", this.onPointerDown);
    this.overlay.addEventListener("pointermove", this.onPointerMove);
    this.overlay.addEventListener("pointerup", this.onPointerUp);
    this.overlay.addEventListener("pointercancel", this.onPointerCancel);
    this.overlay.addEventListener("lostpointercapture", this.onPointerCancel);
    this.overlay.addEventListener("pointerleave", this.onPointerLeave);
    this.overlay.addEventListener("wheel", this.onWheel, { passive: false });
    this.overlay.addEventListener("contextmenu", this.onContextMenu);
    host.addEventListener("keydown", this.onKeyDown);
    host.addEventListener("keyup", this.onKeyUp);
    window.addEventListener("blur", this.onBlur);

    this.offs.push(
      state.onDoc.on((e) => this.onDoc(e)),
      state.onSelection.on(() => this.overlayChanged()),
      state.onHover.on(() => this.overlayChanged()),
      state.onValues.on(() => this.refreshValues()),
      state.onView.on(() => {
        if (this.state.view.tool !== this.gestures.tool) this.gestures.setTool(this.state.view.tool);
      }),
    );

    this.ro = new ResizeObserver(() => {
      this.sizePending = true;
    });
    this.ro.observe(host);
    this.watchDpr();

    const fonts = document.fonts;
    if (fonts) {
      fonts.addEventListener("loadingdone", this.onFontsLoaded);
      fonts.load(FONT_PROBE).then(this.onFontsReady, this.onFontsReady);
      this.fontTimer = window.setTimeout(this.onFontsReady, FONT_WAIT_MS);
    } else {
      this.onFontsReady();
    }
    this.refreshValues();
  }

  // ------------------------------------------------------------------------------------------- Editor

  /** Kind whose placement is armed, or null (see armedKind). */
  get armedKind(): Kind | null {
    return this.destroyed ? null : this.gestures.placing;
  }

  frame(_now: number): void {
    if (!this.active || this.destroyed) return;
    const t0 = performance.now();
    if (this.sizePending) this.applySize();
    if (this.vp.w <= 0 || this.vp.h <= 0 || !this.sctx || !this.octx) return;
    if (this.fitPending) {
      this.fitPending = false;
      this.vp.fit(this.model.bounds());
      this.viewChanged();
    }
    // Contract §8.5: nothing is drawn before the schematic font is loaded (layers stay dirty meanwhile); the view
    // above is already in place, so worldToClient is right from the first frame.
    if (!this.renderer.textsReady) return;
    this.gestures.frame();
    let drew = false;
    const rep = this.dirty.take(this.vp.w, this.vp.h);
    if (rep.kind !== "none") {
      this.renderer.paintScene(this.sctx, rep.kind === "full" ? null : rep.rect);
      if (rep.kind === "full") this.metrics.fullRepaints++;
      else this.metrics.partialRepaints++;
      drew = true;
    }
    if (this.overlayDirty) {
      this.overlayDirty = false;
      this.renderer.paintOverlay(this.octx, this.gestures.view(), this.live, this.diags);
      this.metrics.overlayRepaints++;
      drew = true;
    }
    if (drew) {
      const ms = this.metrics.renderMs;
      ms.push(performance.now() - t0);
      if (ms.length > RING) ms.splice(0, ms.length - RING);
    }
  }

  setTool(tool: Tool): void {
    this.gestures.setTool(tool);
    this.updateCursor();
  }

  armPlacement(kind: Kind): void {
    this.gestures.armPlacement(kind);
    this.host.focus({ preventScroll: true });
    this.updateCursor();
  }

  cancelGesture(): void {
    this.gestures.cancel();
    this.updateCursor();
  }

  fit(): void {
    // Hidden, or shown but not measured yet: the last size may be stale (a mission bench opened from the cockpit
    // would be fitted to the view's old size, then drawn small in a corner, UX-12). The next shown frame fits it.
    if (this.active && !this.sizePending && this.vp.w > 0 && this.vp.h > 0) {
      const { k, ox, oy } = this.vp;
      this.vp.fit(this.model.bounds());
      // A repeated fit with unchanged camera needs no full repaint. Document and value changes
      // retain their own dirty regions; a resize, pan or different fit still invalidates the view.
      if (this.vp.k !== k || this.vp.ox !== ox || this.vp.oy !== oy) this.viewChanged();
    } else {
      this.fitPending = true;
    }
  }

  zoomTo(percent: number): void {
    // Contract: 1 = 100 %. A value above the ladder's top (3.81) is read as a percentage.
    const ratio = percent > 8 ? percent / 100 : percent;
    this.zoomAround(stepNearest(ratio), this.vp.w / 2, this.vp.h / 2);
  }

  zoomStep(direction: 1 | -1): void {
    this.zoomAround(this.vp.k + direction, this.vp.w / 2, this.vp.h / 2);
  }

  worldToClient(p: Pt): { x: number; y: number } {
    const r = this.overlay.getBoundingClientRect();
    const [x, y] = this.vp.toDevice(p);
    return { x: r.left + x / this.vp.dpr, y: r.top + y / this.vp.dpr };
  }

  invalidate(): void {
    const pal = readPalette(this.host);
    if (pal.font !== this.renderer.pal.font) {
      this.widths.clear();
      this.model.relayout();
    }
    this.renderer.pal = pal;
    this.renderer.invalidateCaches();
    this.dirty.markFull();
    this.overlayDirty = true;
    this.sizePending = true;
  }

  setActive(active: boolean): void {
    if (this.destroyed || active === this.active) return;
    this.active = active;
    if (active) {
      this.invalidate();
      return;
    }
    this.releaseCapture();
    this.pan = null;
    this.spacePan = false;
    this.gestures.cancel();
    this.state.setHover({});
    // Hidden: no backing store at all (memory budget, contract §8.6).
    for (const c of [this.scene, this.overlay]) {
      c.width = 0;
      c.height = 0;
      c.style.width = "0px";
      c.style.height = "0px";
    }
    this.allocBase = [0, 0];
    this.vp.resize(0, 0);
  }

  destroy(): void {
    if (this.destroyed) return;
    this.gestures.cancel();
    this.destroyed = true;
    if (this.fontTimer !== null) window.clearTimeout(this.fontTimer);
    this.fontTimer = null;
    this.releaseCapture();
    this.overlay.removeEventListener("pointerdown", this.onPointerDown);
    this.overlay.removeEventListener("pointermove", this.onPointerMove);
    this.overlay.removeEventListener("pointerup", this.onPointerUp);
    this.overlay.removeEventListener("pointercancel", this.onPointerCancel);
    this.overlay.removeEventListener("lostpointercapture", this.onPointerCancel);
    this.overlay.removeEventListener("pointerleave", this.onPointerLeave);
    this.overlay.removeEventListener("wheel", this.onWheel);
    this.overlay.removeEventListener("contextmenu", this.onContextMenu);
    this.host.removeEventListener("keydown", this.onKeyDown);
    this.host.removeEventListener("keyup", this.onKeyUp);
    window.removeEventListener("blur", this.onBlur);
    document.fonts?.removeEventListener("loadingdone", this.onFontsLoaded);
    this.mq?.removeEventListener("change", this.onDpr);
    this.ro.disconnect();
    for (const off of this.offs) off();
    this.widths.clear();
    this.scene.remove();
    this.overlay.remove();
  }

  // ------------------------------------------------------------------------------------------- state

  private onDoc(e: DocEvent): void {
    if (e.kind === "snapshot") {
      const wasEmpty = this.model.comps.size === 0;
      this.gestures.docReplaced();
      this.updateCursor();
      this.widths.clear();
      this.model.rebuild(this.state.components.values(), this.state.wires.values());
      this.dirty.markFull();
      // A newly opened circuit is fitted; a reload of the same one (stale revision) keeps the view.
      if (wasEmpty || this.state.name !== this.docName) this.fit();
      this.docName = this.state.name;
    } else if (e.kind === "patch") {
      for (const r of this.model.applyPatch(e.patch)) this.dirty.add(this.renderer.dirtyOf(r));
    }
    this.overlayDirty = true;
    this.refreshValues();
  }

  private overlayChanged(): void {
    this.overlayDirty = true;
    this.refreshLive();
  }

  private refreshLive(): void {
    const items = liveItems(this.state, this.model, this.gestures.hoverAt);
    const sig = liveSignature(items);
    this.live = items;
    if (sig !== this.liveSig) {
      this.liveSig = sig;
      this.overlayDirty = true;
    }
  }

  /** A steady state redraws nothing: only a changed live string, marker set or drawn state dirties the overlay. */
  private refreshValues(): void {
    this.refreshLive();
    const markers = diagMarkers(this.state, this.model);
    const sig = diagSignature(markers);
    this.diags = markers;
    if (sig !== this.diagSig) {
      this.diagSig = sig;
      this.overlayDirty = true;
    }
    const states = stateSignature(this.state);
    if (states !== this.stateSig) {
      this.stateSig = states;
      this.overlayDirty = true;
    }
  }

  // ------------------------------------------------------------------------------------------- view

  private viewChanged(): void {
    this.dirty.markFull();
    this.overlayDirty = true;
    const pct = zoomPercent(this.vp.k);
    if (this.state.view.zoomPercent !== pct) this.state.setView({ zoomPercent: pct });
  }

  private zoomAround(k: number, x: number, y: number): void {
    if (this.vp.zoomAt(k, x, y)) this.viewChanged();
  }

  private panBy(dx: number, dy: number): void {
    if (this.vp.panBy(dx, dy)) this.viewChanged();
  }

  private applySize(): void {
    this.sizePending = false;
    const host = this.host;
    if (!this.positioned && host.isConnected) {
      // First time in the document: position the canvases, and read the tokens (empty while detached).
      if (getComputedStyle(host).position === "static") host.style.position = "relative";
      this.positioned = true;
      this.renderer.pal = readPalette(host);
      this.renderer.invalidateCaches();
      this.widths.clear();
      this.model.relayout();
      this.dirty.markFull();
    }
    const cssW = host.clientWidth;
    const cssH = host.clientHeight;
    if (cssW <= 0 || cssH <= 0) {
      this.vp.resize(0, 0);
      return;
    }
    const dpr = window.devicePixelRatio || 1;
    const dprChanged = dpr !== this.vp.dpr;
    const [bw, bh] = this.allocBase;
    const realloc = dprChanged || this.scene.width === 0 ||
      cssW > bw * SLACK || cssH > bh * SLACK || cssW < bw * (2 - SLACK) || cssH < bh * (2 - SLACK);
    if (realloc) {
      this.allocBase = [cssW, cssH];
      const aw = Math.ceil(cssW * SLACK);
      const ah = Math.ceil(cssH * SLACK);
      for (const c of [this.scene, this.overlay]) {
        c.width = Math.round(aw * dpr);
        c.height = Math.round(ah * dpr);
        c.style.width = `${c.width / dpr}px`;
        c.style.height = `${c.height / dpr}px`;
      }
    }
    const w = Math.min(this.scene.width, Math.round(cssW * dpr));
    const h = Math.min(this.scene.height, Math.round(cssH * dpr));
    if (dprChanged) {
      this.vp.setDpr(dpr, w, h);
      this.renderer.invalidateCaches();
    } else {
      this.vp.resize(w, h);
    }
    this.viewChanged();
  }

  private watchDpr(): void {
    this.mq?.removeEventListener("change", this.onDpr);
    this.mq = window.matchMedia(`(resolution: ${window.devicePixelRatio || 1}dppx)`);
    this.mq.addEventListener("change", this.onDpr);
  }

  private readonly onDpr = (): void => {
    this.sizePending = true;
    this.watchDpr();
  };

  /** Text widths of 400 11px Plex Mono, bounded and invalidated with the document/font context. */
  private measure(text: string): number {
    return this.widths.measure(text, () => {
      const ctx = this.octx;
      if (ctx) {
        ctx.save();
        ctx.font = `400 ${TEXT_PX}px ${this.renderer.pal.font}`;
        const width = ctx.measureText(text).width;
        ctx.restore();
        return width;
      }
      return text.length * TEXT_PX * 0.6;
    });
  }

  private readonly onFontsReady = (): void => {
    if (this.fontTimer !== null) window.clearTimeout(this.fontTimer);
    this.fontTimer = null;
    if (this.destroyed || this.renderer.textsReady) return;
    this.renderer.textsReady = true;
    this.onFontsLoaded();
  };

  private readonly onFontsLoaded = (): void => {
    if (this.destroyed) return;
    this.widths.clear();
    this.model.relayout();
    this.dirty.markFull();
    this.overlayDirty = true;
  };

  // ------------------------------------------------------------------------------------------- input

  private devOf(e: { clientX: number; clientY: number }): FPt {
    const r = this.overlay.getBoundingClientRect();
    return [(e.clientX - r.left) * this.vp.dpr, (e.clientY - r.top) * this.vp.dpr];
  }

  private worldOf(e: { clientX: number; clientY: number }): FPt {
    const [x, y] = this.devOf(e);
    return this.vp.toWorld(x, y);
  }

  private capture(id: number): void {
    try {
      this.overlay.setPointerCapture(id);
      this.captured = id;
    } catch {
      this.captured = null;
    }
  }

  private releaseCapture(): void {
    const id = this.captured;
    this.captured = null;
    if (id !== null && this.overlay.hasPointerCapture(id)) this.overlay.releasePointerCapture(id);
  }

  private updateCursor(): void {
    const c = this.pan ? "grabbing" : this.spacePan ? "grab" : this.gestures.cursor();
    if (c === this.cursor) return;
    this.cursor = c;
    this.overlay.style.cursor = c;
  }

  private afterInput(): void {
    this.refreshLive();
    this.updateCursor();
  }

  private readonly onPointerDown = (e: PointerEvent): void => {
    if (!this.active || this.vp.w <= 0) return;
    this.host.focus({ preventScroll: true });
    if (e.button === 1 || (e.button === 0 && this.spacePan)) {
      e.preventDefault();
      this.pan = { id: e.pointerId, x: e.clientX, y: e.clientY, fx: 0, fy: 0 };
      this.capture(e.pointerId);
      this.updateCursor();
      return;
    }
    if (e.button !== 0) return;
    e.preventDefault();
    this.capture(e.pointerId);
    this.gestures.down(this.worldOf(e), mods(e));
    this.afterInput();
  };

  private readonly onPointerMove = (e: PointerEvent): void => {
    if (!this.active || this.vp.w <= 0) return;
    const pan = this.pan;
    if (pan && e.pointerId === pan.id) {
      pan.fx += (e.clientX - pan.x) * this.vp.dpr;
      pan.fy += (e.clientY - pan.y) * this.vp.dpr;
      pan.x = e.clientX;
      pan.y = e.clientY;
      const ix = Math.round(pan.fx);
      const iy = Math.round(pan.fy);
      pan.fx -= ix;
      pan.fy -= iy;
      this.panBy(ix, iy);
      return;
    }
    // Primary button up during a drag: its release was lost (no capture); the drag stops, nothing is sent.
    if ((e.buttons & 1) === 0 && this.gestures.pointerBound) this.gestures.cancelPointer();
    this.gestures.move(this.worldOf(e));
    this.afterInput();
  };

  private readonly onPointerUp = (e: PointerEvent): void => {
    if (this.pan && e.pointerId === this.pan.id) {
      this.pan = null;
      this.releaseCapture();
      this.updateCursor();
      return;
    }
    if (!this.active || e.button !== 0) return;
    this.gestures.up(this.worldOf(e));
    this.afterInput();
  };

  /** pointercancel and lostpointercapture (also fired after a normal release, then a no-op). */
  private readonly onPointerCancel = (e: PointerEvent): void => {
    if (this.pan && e.pointerId === this.pan.id) {
      this.pan = null;
      this.updateCursor();
      return;
    }
    if (this.captured === e.pointerId) this.captured = null;
    this.gestures.cancelPointer();
    this.afterInput();
  };

  private readonly onPointerLeave = (): void => {
    if (this.captured !== null || this.pan) return;
    this.gestures.leave();
    this.afterInput();
  };

  private readonly onWheel = (e: WheelEvent): void => {
    e.preventDefault();
    if (!this.active || this.vp.w <= 0) return;
    const scale = e.deltaMode === 1 ? 16 : e.deltaMode === 2 ? 400 : 1;
    const dx = e.deltaX * scale;
    const dy = e.deltaY * scale;
    const dpr = this.vp.dpr;
    if (e.shiftKey && !e.ctrlKey) {
      this.panBy(-(dy || dx) * dpr, 0);
      return;
    }
    if (!e.ctrlKey && Math.abs(dx) > Math.abs(dy)) {
      this.panBy(-dx * dpr, 0);
      return;
    }
    // One ladder step per wheel notch; small touchpad deltas accumulate to a notch.
    let step = 0;
    if (Math.abs(dy) >= 50) {
      step = dy < 0 ? 1 : -1;
      this.wheelAcc = 0;
    } else {
      this.wheelAcc += dy;
      if (Math.abs(this.wheelAcc) >= 50) {
        step = this.wheelAcc < 0 ? 1 : -1;
        this.wheelAcc = 0;
      }
    }
    if (step === 0) return;
    const [x, y] = this.devOf(e);
    this.zoomAround(this.vp.k + step, x, y);
  };

  private readonly onContextMenu = (e: MouseEvent): void => {
    e.preventDefault();
  };

  private readonly onKeyDown = (e: KeyboardEvent): void => {
    if (!this.active || isFormField(e.target)) return;
    if (this.gestures.key(keyInput(e))) {
      e.preventDefault();
      e.stopPropagation();
      this.afterInput();
      return;
    }
    if ((e.key === " " || e.code === "Space") && !e.ctrlKey && !e.altKey) {
      e.preventDefault();
      this.spacePan = true;
      this.updateCursor();
    }
  };

  private readonly onKeyUp = (e: KeyboardEvent): void => {
    if (isFormField(e.target)) return;
    if (e.key === " " || e.code === "Space") this.spacePan = false;
    if (this.gestures.keyUp(keyInput(e))) {
      e.preventDefault();
      e.stopPropagation();
    }
    this.afterInput();
  };

  private readonly onBlur = (): void => {
    this.spacePan = false;
    this.pan = null;
    this.gestures.blur();
    this.afterInput();
  };
}

export const createEditor: CreateEditor = (host, state, controller) => new SchematicEditor(host, state, controller);

/**
 * Kind whose placement is armed in an editor made by createEditor, or null: the library highlights it. The placement
 * ends inside the editor (click, Escape, tool change, undo), so the view reads it here instead of tracking it.
 */
export function armedKind(editor: Editor): Kind | null {
  return editor instanceof SchematicEditor ? editor.armedKind : null;
}
