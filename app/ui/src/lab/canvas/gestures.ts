// Gesture state machine of the schematic editor (contract §3.3, §8.4 "Clavier", §8.5 "Gestes"). It turns pointer
// and keyboard input, already converted to world coordinates, into LabState UI changes (selection, hover, view,
// message) and LabController calls: previews while a gesture runs, one edit at the end of the gesture, interactions
// for actuators. It never validates geometry (previews do) and never shows a state the core has not returned.
// Pure: no DOM, so that tests drive it with a fake controller.

import type { Hover, LabState, Outcome, PreviewResult, Tool } from "../state";
import type {
  ActuatorKind, CompId, Component, Edit, EditReply, End, Endpoint, Interact, Kind, Patch, PinRef, Pt, Refusal, Rot,
  ValueMap, WireId,
} from "../types";
import { type FPt, canonicalPath, inverseRot, rectOf, rotatePt, samePt, snapPt } from "./geom";
import { type Hit, hitTest, marqueeMode, marqueeSelect } from "./hit";
import type { SceneModel } from "./model";
import { alphaOfX, potPosition, SLIDER_KINDS } from "./symbols";
import { bendsOf, dragSegment, elbow, isHorizontal } from "./wiring";

/** The part of LabController the editor uses (LabController satisfies it). */
export interface EditorController {
  edit(edit: Edit): Promise<Outcome<EditReply>>;
  undo(): Promise<Outcome<EditReply>>;
  redo(): Promise<Outcome<EditReply>>;
  requestPreview(edit: Edit, sink: (r: PreviewResult, edit: Edit) => void): void;
  cancelPreview(): void;
  interact(interact: Interact): void;
}

export interface GestureEnv {
  /** Grid units per CSS pixel at the current zoom. */
  unitsPerPx(): number;
  fit(): void;
  /** Ladder step closest to the ratio (1 = 100 %). */
  zoomTo(ratio: number): void;
  /** Something the overlay shows changed. */
  changed(): void;
}

export interface Mods {
  shift: boolean;
  ctrl: boolean;
}

export interface KeyInput {
  key: string;
  code: string;
  shift: boolean;
  ctrl: boolean;
  alt: boolean;
  repeat: boolean;
}

export type Ghost =
  | { t: "place"; comp: Component }
  | { t: "move"; comps: CompId[]; wires: WireId[]; delta: Pt }
  | { t: "path"; path: Pt[] };

export interface GestureView {
  /** Local proposal of the gesture, drawn while its preview is pending or refused. */
  ghost: Ghost | null;
  status: "pending" | "ok" | "refused";
  /** Previewed patch of the current proposal (status ok). */
  patch: Patch | null;
  refusal: Refusal | null;
  marquee: { a: FPt; b: FPt; enclosed: boolean } | null;
}

/** Hit pointing distance and drag threshold, CSS pixels. */
export const HIT_PX = 5;
export const DRAG_PX = 3;

interface Target {
  ep: Endpoint;
  pt: Pt;
  /** Axis of the first leg of a wire leaving here; null: the dominant axis of the pointer. */
  axis: "h" | "v" | null;
}

interface PressPlan {
  drag: (() => void) | null;
  click: (() => void) | null;
}

interface WireG {
  t: "wire";
  a: Target;
  /** Fixed corners after the start; `legs` counts how many each click added (Backspace removes one leg). */
  points: Pt[];
  legs: number[];
  hFirst: boolean | null;
  cursor: Pt;
  target: Target | null;
  /** Started by dragging from a pin: the first release acts as a click. */
  viaDrag: boolean;
}

type G =
  | { t: "idle" }
  | { t: "press"; start: FPt; plan: PressPlan }
  | { t: "place"; kind: Kind; rot: Rot; pos: Pt | null }
  | WireG
  | { t: "move"; comps: CompId[]; wires: WireId[]; origin: Pt; delta: Pt }
  | { t: "bends"; wire: WireId; seg: number; path: Pt[]; origin: Pt; dt: number }
  | { t: "wire_end"; wire: WireId; end: End; path: Pt[]; cursor: Pt; target: Target | null }
  | { t: "marquee"; a: FPt; b: FPt; shift: boolean }
  | { t: "pot"; comp: CompId; alpha: number; sent: number | null }
  | { t: "button"; comp: CompId };

type Holder = "pointer" | "key";

const keyOf = (edit: Edit): string => JSON.stringify(edit);
const isZero = (p: Pt): boolean => p[0] === 0 && p[1] === 0;
const dist = (a: FPt, b: FPt): number => Math.hypot(a[0] - b[0], a[1] - b[1]);
const byId = (a: number, b: number): number => a - b;

function sameEndpoint(a: Endpoint, b: Endpoint): boolean {
  if (a.type === "pin" && b.type === "pin") return a.comp === b.comp && a.pin === b.pin;
  if (a.type === "on_wire" && b.type === "on_wire") return a.wire === b.wire && samePt(a.at, b.at);
  return false;
}

export class Gestures {
  /** World point of the hovered pin or wire point: where the hovered net's live value is shown. */
  hoverAt: FPt | null = null;
  private toolValue: Tool = "select";
  private g: G = { t: "idle" };
  private wantKey: string | null = null;
  private last: { key: string; r: PreviewResult } | null = null;
  private committed: GestureView | null = null;
  private hoverHit: Hit | null = null;
  private readonly holders = new Map<CompId, Set<Holder>>();
  private readonly potKeys = new Map<CompId, { v: number; t: number }>();
  private readonly sink = (r: PreviewResult, edit: Edit): void => {
    this.last = { key: keyOf(edit), r };
    this.env.changed();
  };

  constructor(
    private readonly state: LabState,
    private readonly ctl: EditorController,
    private readonly model: SceneModel,
    private readonly env: GestureEnv,
    private readonly now: () => number = () => performance.now(),
  ) {}

  get tool(): Tool {
    return this.toolValue;
  }

  /** Current gesture ("idle", "place", "wire", "move", …). */
  get gesture(): G["t"] {
    return this.g.t;
  }

  /** Kind whose placement is armed (library highlight), or null. */
  get placing(): Kind | null {
    return this.g.t === "place" ? this.g.kind : null;
  }

  /** The gesture lives only while the primary button is held. */
  get pointerBound(): boolean {
    const g = this.g;
    return g.t === "press" || g.t === "move" || g.t === "bends" || g.t === "wire_end" || g.t === "marquee" ||
      g.t === "pot" || g.t === "button" || (g.t === "wire" && g.viaDrag);
  }

  // ------------------------------------------------------------------------------------------- pointer

  /** Primary button pressed at world point p. */
  down(p: FPt, mods: Mods): void {
    this.committed = null;
    // A drag whose release was lost (outside the window, no capture) is dropped: never committed by this click.
    if (this.pointerBound) this.cancelGesture();
    const g = this.g;
    if (g.t === "place") {
      this.placeAt(p);
      this.commitPlacement();
      return;
    }
    if (g.t === "wire") {
      this.wireClick(g, p);
      return;
    }
    if (g.t !== "idle") return;
    const hit = hitTest(this.model, p, this.tol());
    if (hit?.t === "actuator") {
      this.actuate(hit.comp, hit.kind, p);
      return;
    }
    if (this.toolValue === "wire") {
      const tg = this.targetOf(hit);
      if (tg) this.startWire(tg, p, false);
      return;
    }
    this.press(hit, p, mods);
  }

  move(p: FPt): void {
    const g = this.g;
    switch (g.t) {
      case "idle":
        this.setHover(hitTest(this.model, p, this.tol()));
        return;
      case "press":
        if (!g.plan.drag || dist(p, g.start) < DRAG_PX * this.env.unitsPerPx()) return;
        this.g = { t: "idle" };
        g.plan.drag();
        if (this.g.t !== "idle") this.move(p);
        return;
      case "place":
        this.placeAt(p);
        return;
      case "wire":
        this.updateWire(g, p);
        return;
      case "move": {
        const s = snapPt(p);
        const d: Pt = [s[0] - g.origin[0], s[1] - g.origin[1]];
        if (samePt(d, g.delta)) return;
        g.delta = d;
        this.preview(isZero(d) ? null : this.moveEdit(g));
        return;
      }
      case "bends": {
        const s = snapPt(p);
        const dt = isHorizontal(g.path[g.seg], g.path[g.seg + 1]) ? s[1] - g.origin[1] : s[0] - g.origin[0];
        if (dt === g.dt) return;
        g.dt = dt;
        this.preview(dt === 0 ? null : this.bendsEdit(g));
        return;
      }
      case "wire_end":
        this.updateWireEnd(g, p);
        return;
      case "marquee":
        g.b = p;
        this.env.changed();
        return;
      case "pot":
        g.alpha = this.alphaAt(g.comp, p);
        return;
      default:
        return;
    }
  }

  /** Primary button released at world point p. */
  up(p: FPt): void {
    const g = this.g;
    switch (g.t) {
      case "press":
        this.g = { t: "idle" };
        g.plan.click?.();
        this.env.changed();
        return;
      case "wire":
        if (!g.viaDrag) return;
        g.viaDrag = false;
        this.updateWire(g, p);
        if (!g.target && samePt(g.cursor, g.a.pt)) this.cancelGesture();
        else this.wireClick(g, p);
        return;
      case "move":
        this.finish(isZero(g.delta) ? null : this.moveEdit(g));
        return;
      case "bends":
        this.finish(g.dt === 0 ? null : this.bendsEdit(g));
        return;
      case "wire_end":
        this.finish(g.target && !this.sameEnd(g) ? this.wireEndEdit(g) : null);
        return;
      case "marquee":
        this.finishMarquee(g);
        return;
      case "pot":
        g.alpha = this.alphaAt(g.comp, p);
        this.flushPot();
        this.g = { t: "idle" };
        this.env.changed();
        return;
      case "button":
        this.g = { t: "idle" };
        this.release(g.comp, "pointer");
        this.env.changed();
        return;
      default:
        return;
    }
  }

  /** pointercancel or lostpointercapture: gestures bound to the pointer stop, nothing is sent. */
  cancelPointer(): void {
    if (this.pointerBound) this.cancelGesture();
  }

  /** The pointer left the editor. */
  leave(): void {
    if (this.g.t === "place" && this.g.pos) {
      this.g.pos = null;
      this.preview(null);
    }
    if (this.g.t === "idle") this.setHover(null);
  }

  /** Window lost the focus: held buttons are released, pointer gestures stop. */
  blur(): void {
    this.cancelPointer();
    this.releaseAll();
  }

  // ------------------------------------------------------------------------------------------- commands

  setTool(tool: Tool): void {
    if (this.g.t !== "idle") this.cancelGesture();
    this.toolValue = tool;
    if (this.state.view.tool !== tool) this.state.setView({ tool });
    this.env.changed();
  }

  armPlacement(kind: Kind): void {
    this.cancelGesture();
    if (!this.state.library.get(kind)?.placeable) return;
    this.g = { t: "place", kind, rot: 0, pos: null };
    this.env.changed();
  }

  /** Cancels the gesture in progress; nothing is sent. */
  cancelGesture(): void {
    const g = this.g;
    this.g = { t: "idle" };
    if (g.t === "button") this.release(g.comp, "pointer");
    this.preview(null);
    this.committed = null;
    this.env.changed();
  }

  /**
   * A snapshot replaced the document mirror (file opened, new circuit, reload): a gesture built on the previous
   * document is dropped. An armed placement stays armed; actuators end with their pointer release.
   */
  docReplaced(): void {
    const t = this.g.t;
    if (t !== "idle" && t !== "place" && t !== "pot" && t !== "button") this.cancelGesture();
  }

  /** Gesture cancelled and every held push button released (view hidden, editor destroyed). */
  cancel(): void {
    this.cancelGesture();
    this.releaseAll();
  }

  /** Once per animation frame: the wiper position is sent at most once per frame (latest only). */
  frame(): void {
    if (this.g.t === "pot") this.flushPot();
  }

  // ------------------------------------------------------------------------------------------- keyboard

  /** keydown; true when consumed. */
  key(k: KeyInput): boolean {
    const g = this.g;
    const key = k.key.length === 1 ? k.key.toLowerCase() : k.key;
    if (k.ctrl && !k.alt) {
      if (key === "z" || key === "y") {
        this.cancelGesture();
        void (key === "y" || k.shift ? this.ctl.redo() : this.ctl.undo());
        return true;
      }
      if (key === "a") {
        if (g.t === "idle") this.state.setSelection([...this.state.components.keys()].sort(byId), [...this.state.wires.keys()].sort(byId));
        return true;
      }
      return false;
    }
    if (k.alt) return false;
    switch (key) {
      case "Escape":
        if (g.t !== "idle") this.cancelGesture();
        else if (this.toolValue === "wire") this.setTool("select");
        else if (this.hasSelection()) this.state.setSelection([], []);
        else return false;
        return true;
      case "Backspace":
        if (g.t !== "wire") return false;
        this.removeLeg(g);
        return true;
      case "/":
        if (g.t !== "wire") return false;
        g.hFirst = !this.legHFirst(g, this.lastPoint(g), g.cursor);
        this.preview(g.target ? this.wireEdit(g, g.target) : null);
        return true;
      case "r":
        if (g.t === "place") {
          g.rot = ((g.rot + 90) % 360) as Rot;
          if (g.pos) this.preview(this.placeEdit(g.kind, g.pos, g.rot));
          else this.env.changed();
          return true;
        }
        // Mid-gesture the key is swallowed: nothing may change the document under a drag.
        if (g.t !== "idle") return true;
        if (!this.hasSelection()) return false;
        void this.ctl.edit({ op: "rotate", ...this.selectedIds() });
        return true;
      case "w":
        if (g.t !== "wire") this.setTool("wire");
        return true;
      case "f":
        this.env.fit();
        return true;
      case "Delete":
        if (g.t !== "idle") return true;
        if (!this.hasSelection()) return false;
        void this.ctl.edit({ op: "delete", ...this.selectedIds() });
        return true;
      case " ":
        return this.spaceDown(k.repeat);
      case "ArrowLeft":
      case "ArrowRight":
      case "ArrowUp":
      case "ArrowDown":
        return this.potKey(key, k.shift);
      default:
        break;
    }
    if (key === "1" || k.code === "Digit1" || k.code === "Numpad1") {
      this.env.zoomTo(1);
      return true;
    }
    return false;
  }

  /** keyup; true when consumed. */
  keyUp(k: KeyInput): boolean {
    if (k.key !== " " && k.code !== "Space") return false;
    let any = false;
    for (const [comp, by] of [...this.holders]) {
      if (by.has("key")) {
        this.release(comp, "key");
        any = true;
      }
    }
    return any;
  }

  // ------------------------------------------------------------------------------------------- rendering

  /** What the overlay draws for the gesture: ghost and preview, or marquee. */
  view(): GestureView | null {
    if (this.committed) return this.committed;
    const g = this.g;
    if (g.t === "marquee") {
      return { ghost: null, status: "pending", patch: null, refusal: null, marquee: { a: g.a, b: g.b, enclosed: marqueeMode(g.a, g.b) === "enclosed" } };
    }
    const ghost = this.ghost();
    if (!ghost) return null;
    const last = this.last;
    if (this.wantKey !== null && last && last.key === this.wantKey) {
      return last.r.ok
        ? { ghost, status: "ok", patch: last.r.reply.patch, refusal: null, marquee: null }
        : { ghost, status: "refused", patch: null, refusal: last.r.refusal, marquee: null };
    }
    return { ghost, status: "pending", patch: null, refusal: null, marquee: null };
  }

  cursor(): string {
    const t = this.g.t;
    if (t === "pot" || t === "button") return "pointer";
    if (t === "move") return "move";
    if (t === "place" || t === "wire") return "crosshair";
    if (this.hoverHit?.t === "actuator") return "pointer";
    return this.toolValue === "wire" ? "crosshair" : "default";
  }

  // ------------------------------------------------------------------------------------------- internals

  private tol(): number {
    return HIT_PX * this.env.unitsPerPx();
  }

  private hasSelection(): boolean {
    return this.state.selection.components.size > 0 || this.state.selection.wires.size > 0;
  }

  private selectedIds(): { components: CompId[]; wires: WireId[] } {
    return {
      components: [...this.state.selection.components].sort(byId),
      wires: [...this.state.selection.wires].sort(byId),
    };
  }

  private selectedOfKind(kind: Kind): CompId[] {
    return [...this.state.selection.components].filter((id) => this.state.components.get(id)?.kind === kind).sort(byId);
  }

  private toggle(comps: CompId[], wires: WireId[]): void {
    const c = new Set(this.state.selection.components);
    const w = new Set(this.state.selection.wires);
    for (const id of comps) if (!c.delete(id)) c.add(id);
    for (const id of wires) if (!w.delete(id)) w.add(id);
    this.state.setSelection(c, w);
  }

  /** Requests the dry run of the current proposal (only when it changed); null drops the preview. */
  private preview(edit: Edit | null): void {
    if (!edit) {
      if (this.wantKey !== null) {
        this.wantKey = null;
        this.last = null;
        this.ctl.cancelPreview();
      }
    } else {
      const key = keyOf(edit);
      if (key !== this.wantKey) {
        this.wantKey = key;
        this.ctl.requestPreview(edit, this.sink);
      }
    }
    this.env.changed();
  }

  /**
   * Sends the edit unless its preview was refused: then nothing is sent and the reason goes to the message line.
   * The ghost stays visible until the reply has been applied to the mirror.
   */
  private tryCommit(edit: Edit): boolean {
    const last = this.last;
    if (last && last.key === keyOf(edit) && !last.r.ok && last.r.refusal) {
      this.state.setMessage({ kind: "error", text: last.r.refusal.message_fr });
      return false;
    }
    const view = this.view();
    this.preview(null);
    this.committed = view;
    const settle = (): void => {
      if (this.committed !== view) return;
      this.committed = null;
      this.env.changed();
    };
    this.ctl.edit(edit).then(settle, settle);
    return true;
  }

  /** Ends a drag gesture with its edit, or restores the scene (null or refused). */
  private finish(edit: Edit | null): void {
    if (edit) this.tryCommit(edit);
    this.g = { t: "idle" };
    this.preview(null);
  }

  private setHover(hit: Hit | null): void {
    this.hoverHit = hit;
    let h: Hover = {};
    let at: FPt | null = null;
    if (hit?.t === "pin" || hit?.t === "junction") {
      const ref: PinRef = { comp: hit.comp, pin: hit.pin };
      const net = this.state.netOf(ref)?.key;
      h = hit.t === "pin" ? { comp: hit.comp, pin: ref, net } : { pin: ref, net };
      at = hit.pos;
    } else if (hit?.t === "wire") {
      const w = this.state.wires.get(hit.wire);
      h = { wire: hit.wire, net: w ? this.state.netOf(w.a)?.key : undefined };
      at = hit.at;
    } else if (hit) {
      h = { comp: hit.comp };
    }
    const moved = !(at === this.hoverAt || (at && this.hoverAt && samePt(at, this.hoverAt)));
    this.hoverAt = at;
    this.state.setHover(h);
    if (moved) this.env.changed();
  }

  private press(hit: Hit | null, p: FPt, mods: Mods): void {
    const s = snapPt(p);
    const sel = this.state.selection;
    let plan: PressPlan;
    if (hit?.t === "pin") {
      const comp = hit.comp;
      const end = this.selectedWireEnd({ comp, pin: hit.pin });
      const tg = this.targetOf(hit);
      const click = (): void => this.clickComp(comp, mods.shift);
      plan = end
        ? { drag: () => this.startWireEnd(end.wire, end.end), click }
        : { drag: tg ? () => this.startWire(tg, p, true) : null, click };
    } else if (hit?.t === "junction") {
      const j = hit.comp;
      const end = this.selectedWireEnd({ comp: j, pin: hit.pin });
      if (end) {
        plan = { drag: () => this.startWireEnd(end.wire, end.end), click: () => this.clickJunction(j, mods.shift) };
      } else {
        this.clickJunction(j, mods.shift);
        plan = { drag: () => this.startMove([j], [], s), click: null };
      }
    } else if (hit?.t === "wire") {
      const { wire, seg } = hit;
      let click: (() => void) | null = null;
      if (!sel.wires.has(wire)) {
        if (mods.shift) this.state.setSelection(sel.components, [...sel.wires, wire]);
        else this.state.setSelection([], [wire]);
      } else {
        click = mods.shift ? () => this.toggle([], [wire]) : () => this.state.setSelection([], [wire]);
      }
      plan = {
        drag: () => {
          const now = this.state.selection;
          if (now.components.size === 0 && now.wires.size === 1 && now.wires.has(wire)) this.startBends(wire, seg, s);
          else this.startMove(this.selectedIds().components, this.selectedIds().wires, s);
        },
        click,
      };
    } else if (hit?.t === "body") {
      const comp = hit.comp;
      let click: (() => void) | null = null;
      if (!sel.components.has(comp)) {
        if (mods.shift) this.state.setSelection([...sel.components, comp], sel.wires);
        else this.state.setSelection([comp], []);
      } else {
        click = mods.shift ? () => this.toggle([comp], []) : () => this.state.setSelection([comp], []);
      }
      plan = { drag: () => this.startMove(this.selectedIds().components, this.selectedIds().wires, s), click };
    } else {
      plan = {
        drag: () => {
          this.g = { t: "marquee", a: p, b: p, shift: mods.shift };
        },
        click: mods.shift ? null : () => this.state.setSelection([], []),
      };
    }
    this.g = { t: "press", start: p, plan };
  }

  private clickComp(comp: CompId, shift: boolean): void {
    if (shift) this.toggle([comp], []);
    else this.state.setSelection([comp], []);
  }

  /** A junction selects its wires (contract §8.5). */
  private clickJunction(j: CompId, shift: boolean): void {
    const ws = this.model.wiresOf(j);
    if (!shift) {
      this.state.setSelection([], ws);
      return;
    }
    const sel = this.state.selection.wires;
    if (ws.every((w) => sel.has(w))) this.toggle([], ws);
    else this.toggle([], ws.filter((w) => !sel.has(w)));
  }

  /** The only selected wire that ends on this pin, with that end. */
  private selectedWireEnd(ref: PinRef): { wire: WireId; end: End } | null {
    const found: { wire: WireId; end: End }[] = [];
    for (const id of this.state.selection.wires) {
      const w = this.state.wires.get(id);
      if (!w) continue;
      if (w.a.comp === ref.comp && w.a.pin === ref.pin) found.push({ wire: id, end: "a" });
      else if (w.b.comp === ref.comp && w.b.pin === ref.pin) found.push({ wire: id, end: "b" });
    }
    return found.length === 1 ? found[0] : null;
  }

  private targetOf(hit: Hit | null): Target | null {
    if (hit?.t === "pin") {
      const dir = this.model.pin(hit)?.dir;
      return { ep: { type: "pin", comp: hit.comp, pin: hit.pin }, pt: hit.pos, axis: dir ? (dir === "l" || dir === "r" ? "h" : "v") : null };
    }
    if (hit?.t === "junction") return { ep: { type: "pin", comp: hit.comp, pin: hit.pin }, pt: hit.pos, axis: null };
    if (hit?.t === "wire") {
      const g = this.model.wires.get(hit.wire);
      if (!g) return null;
      // Leaving a wire along it would overlap it: the first leg is perpendicular.
      const axis = isHorizontal(g.path[hit.seg], g.path[hit.seg + 1]) ? "v" : "h";
      return { ep: { type: "on_wire", wire: hit.wire, at: hit.at }, pt: hit.at, axis };
    }
    return null;
  }

  // ---------------------------------------------------------------------------- placement

  private placeEdit(kind: Kind, pos: Pt, rot: Rot): Edit {
    return { op: "add_component", kind, pos, rot };
  }

  private placeAt(p: FPt): void {
    const g = this.g;
    if (g.t !== "place") return;
    const s = snapPt(p);
    if (g.pos && samePt(g.pos, s)) return;
    g.pos = s;
    this.preview(this.placeEdit(g.kind, s, g.rot));
  }

  private commitPlacement(): void {
    const g = this.g;
    if (g.t !== "place" || !g.pos) return;
    if (this.tryCommit(this.placeEdit(g.kind, g.pos, g.rot))) this.g = { t: "idle" };
  }

  // ---------------------------------------------------------------------------- wire drawing

  private startWire(a: Target, p: FPt, viaDrag: boolean): void {
    const g: WireG = {
      t: "wire", a, points: [], legs: [], hFirst: a.axis === null ? null : a.axis === "h",
      cursor: a.pt, target: null, viaDrag,
    };
    this.g = g;
    this.updateWire(g, p);
  }

  private lastPoint(g: WireG): Pt {
    return g.points.length ? g.points[g.points.length - 1] : g.a.pt;
  }

  private legHFirst(g: WireG, from: Pt, to: Pt): boolean {
    return g.hFirst ?? Math.abs(to[0] - from[0]) >= Math.abs(to[1] - from[1]);
  }

  /** [start, fixed corners…, L corner, end], canonical. */
  private wirePath(g: WireG): Pt[] {
    const from = this.lastPoint(g);
    return canonicalPath([g.a.pt, ...g.points, elbow(from, g.cursor, this.legHFirst(g, from, g.cursor)), g.cursor]);
  }

  private wireEdit(g: WireG, b: Target): Edit {
    return { op: "add_wire", a: g.a.ep, b: b.ep, bends: bendsOf(this.wirePath(g)) };
  }

  private updateWire(g: WireG, p: FPt): void {
    const hit = hitTest(this.model, p, this.tol(), { actuators: false, bodies: false });
    let tg = this.targetOf(hit);
    if (tg && sameEndpoint(tg.ep, g.a.ep)) tg = null;
    g.target = tg;
    g.cursor = tg ? tg.pt : snapPt(p);
    this.setHover(hit);
    this.preview(tg ? this.wireEdit(g, tg) : null);
  }

  /** A click while drawing: on a pin, junction or wire it ends the wire; in the void it fixes a corner. */
  private wireClick(g: WireG, p: FPt): void {
    this.updateWire(g, p);
    if (g.target) {
      if (this.tryCommit(this.wireEdit(g, g.target))) this.g = { t: "idle" };
      return;
    }
    const from = this.lastPoint(g);
    const to = g.cursor;
    if (samePt(from, to)) return;
    const corner = elbow(from, to, this.legHFirst(g, from, to));
    const added: Pt[] = [];
    if (!samePt(corner, from) && !samePt(corner, to)) added.push(corner);
    added.push([to[0], to[1]]);
    g.points.push(...added);
    g.legs.push(added.length);
    g.hFirst = this.nextAxis(g);
    this.env.changed();
  }

  /** The next leg leaves perpendicular to the last fixed segment. */
  private nextAxis(g: WireG): boolean | null {
    const pts = [g.a.pt, ...g.points];
    if (pts.length < 2) return g.a.axis === null ? null : g.a.axis === "h";
    return !isHorizontal(pts[pts.length - 2], pts[pts.length - 1]);
  }

  private removeLeg(g: WireG): void {
    const n = g.legs.pop();
    if (n === undefined) return;
    g.points.splice(g.points.length - n, n);
    g.hFirst = this.nextAxis(g);
    this.preview(g.target ? this.wireEdit(g, g.target) : null);
  }

  // ---------------------------------------------------------------------------- wire end, segment, move

  private startWireEnd(wire: WireId, end: End): void {
    const g = this.model.wires.get(wire);
    if (!g) return;
    const path = g.path.map((q): Pt => [q[0], q[1]]);
    this.g = { t: "wire_end", wire, end, path, cursor: end === "a" ? path[0] : path[path.length - 1], target: null };
  }

  private updateWireEnd(g: Extract<G, { t: "wire_end" }>, p: FPt): void {
    const hit = hitTest(this.model, p, this.tol(), { actuators: false, bodies: false, excludeWire: g.wire });
    const tg = this.targetOf(hit);
    g.target = tg;
    g.cursor = tg ? tg.pt : snapPt(p);
    this.setHover(hit);
    this.preview(tg && !this.sameEnd(g) ? this.wireEndEdit(g) : null);
  }

  /** Dropped back on the pin it already ends on. */
  private sameEnd(g: Extract<G, { t: "wire_end" }>): boolean {
    const w = this.state.wires.get(g.wire);
    if (!w || !g.target || g.target.ep.type !== "pin") return false;
    const cur = g.end === "a" ? w.a : w.b;
    return g.target.ep.comp === cur.comp && g.target.ep.pin === cur.pin;
  }

  private wireEndEdit(g: Extract<G, { t: "wire_end" }>): Edit {
    return { op: "move_wire_end", wire: g.wire, end: g.end, to: g.target!.ep };
  }

  /** Local proposal: the moving end follows the pointer, its last segment keeps its orientation. */
  private wireEndPath(g: Extract<G, { t: "wire_end" }>): Pt[] {
    const path = g.end === "b" ? g.path : [...g.path].reverse();
    const n = path.length;
    const fixed = path.slice(0, n - 1);
    const from = fixed[fixed.length - 1];
    const lastHorizontal = n >= 2 ? isHorizontal(path[n - 2], path[n - 1]) : true;
    const out = canonicalPath([...fixed, elbow(from, g.cursor, !lastHorizontal), g.cursor]);
    return g.end === "b" ? out : out.reverse();
  }

  private startBends(wire: WireId, seg: number, origin: Pt): void {
    const g = this.model.wires.get(wire);
    if (!g || seg + 1 >= g.path.length) return;
    this.g = { t: "bends", wire, seg, path: g.path.map((q): Pt => [q[0], q[1]]), origin, dt: 0 };
  }

  private bendsEdit(g: Extract<G, { t: "bends" }>): Edit {
    return { op: "set_bends", wire: g.wire, bends: bendsOf(dragSegment(g.path, g.seg, g.dt)) };
  }

  private startMove(comps: CompId[], wires: WireId[], origin: Pt): void {
    if (comps.length === 0 && wires.length === 0) return;
    this.g = { t: "move", comps, wires, origin, delta: [0, 0] };
  }

  private moveEdit(g: Extract<G, { t: "move" }>): Edit {
    return { op: "move", components: g.comps, wires: g.wires, delta: g.delta };
  }

  private finishMarquee(g: Extract<G, { t: "marquee" }>): void {
    const picked = marqueeSelect(this.model, rectOf(g.a, g.b), marqueeMode(g.a, g.b));
    this.g = { t: "idle" };
    if (g.shift) this.toggle(picked.comps, picked.wires);
    else this.state.setSelection(picked.comps, picked.wires);
    this.env.changed();
  }

  private ghost(): Ghost | null {
    const g = this.g;
    switch (g.t) {
      case "place": {
        const def = this.state.library.get(g.kind);
        if (!g.pos || !def) return null;
        const params: ValueMap = {};
        for (const p of def.params) params[p.name] = p.default;
        return { t: "place", comp: { id: -1, kind: g.kind, label: "", pos: g.pos, rot: g.rot, params } };
      }
      case "move":
        return isZero(g.delta) ? null : { t: "move", comps: g.comps, wires: g.wires, delta: g.delta };
      case "wire":
        return { t: "path", path: this.wirePath(g) };
      case "bends":
        return g.dt === 0 ? null : { t: "path", path: canonicalPath(dragSegment(g.path, g.seg, g.dt)) };
      case "wire_end":
        return { t: "path", path: this.wireEndPath(g) };
      default:
        return null;
    }
  }

  // ---------------------------------------------------------------------------- actuators

  private actuate(comp: CompId, kind: ActuatorKind, p: FPt): void {
    if (kind === "toggle_switch") {
      this.ctl.interact({ op: "set_switch", comp, closed: this.state.states.get(comp)?.closed !== true });
    } else if (kind === "press_button") {
      this.g = { t: "button", comp };
      this.hold(comp, "pointer");
    } else {
      this.potKeys.delete(comp);
      this.g = { t: "pot", comp, alpha: this.alphaAt(comp, p), sent: null };
    }
    this.env.changed();
  }

  /** α of a pointer: brought into the symbol frame, α = clamp((x − x_A)/(x_B − x_A), 0, 1) (§8.5). */
  private alphaAt(comp: CompId, p: FPt): number {
    const c = this.state.components.get(comp);
    if (!c) return 0.5;
    const local = rotatePt([p[0] - c.pos[0], p[1] - c.pos[1]], inverseRot(c.rot));
    return alphaOfX(local[0]);
  }

  private flushPot(): void {
    const g = this.g;
    if (g.t !== "pot" || g.sent === g.alpha) return;
    g.sent = g.alpha;
    this.ctl.interact({ op: "set_pot", comp: g.comp, position: g.alpha });
  }

  private hold(comp: CompId, by: Holder): void {
    let set = this.holders.get(comp);
    if (!set) {
      set = new Set();
      this.holders.set(comp, set);
    }
    const first = set.size === 0;
    set.add(by);
    if (first) this.ctl.interact({ op: "set_button", comp, pressed: true });
  }

  private release(comp: CompId, by: Holder): void {
    const set = this.holders.get(comp);
    if (!set || !set.delete(by)) return;
    if (set.size > 0) return;
    this.holders.delete(comp);
    this.ctl.interact({ op: "set_button", comp, pressed: false });
  }

  private releaseAll(): void {
    for (const comp of [...this.holders.keys()]) {
      this.holders.delete(comp);
      this.ctl.interact({ op: "set_button", comp, pressed: false });
    }
  }

  /** Space presses the selected push buttons; repeats are ignored. */
  private spaceDown(repeat: boolean): boolean {
    const buttons = this.selectedOfKind("push_button");
    if (buttons.length === 0) return false;
    if (!repeat) for (const c of buttons) this.hold(c, "key");
    return true;
  }

  /** Arrows on the selected potentiometers and pedal sensors: ±0.01, Shift ±0.1. */
  private potKey(key: string, big: boolean): boolean {
    const pots = [...this.state.selection.components]
      .filter((id) => SLIDER_KINDS.has(this.state.components.get(id)?.kind ?? "junction"))
      .sort(byId);
    if (pots.length === 0) return false;
    const step = (big ? 0.1 : 0.01) * (key === "ArrowLeft" || key === "ArrowDown" ? -1 : 1);
    const t = this.now();
    for (const comp of pots) {
      // Consecutive presses build on the previous request while its reply may still be in flight.
      const prev = this.potKeys.get(comp);
      const base = prev && t - prev.t < 1000 ? prev.v : potPosition(this.state.states.get(comp), this.state.components.get(comp)?.kind === "mlx90377" ? 0 : 0.5);
      const v = Math.round(Math.min(1, Math.max(0, base + step)) * 1000) / 1000;
      this.potKeys.set(comp, { v, t });
      this.ctl.interact({ op: "set_pot", comp, position: v });
    }
    return true;
  }
}
