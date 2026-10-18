// End-to-end scenarios of the Physical Lab, without board (contract docs/physical-lab-v1.md §8.6, §9.1, §13):
// - "phys": the 3-minute manual test of §13 replayed with real gestures (pointer, wheel and key events positioned
//   by Editor.worldToClient, clicks and typing in the panels); the displayed values are compared with §9.1;
// - "phys-perf": the phases and metrics of §8.6 on the hidden demonstration perf-150-300;
//   --seconds > 0 adds editing churn, a narrow-canvas regression and that many seconds of sustained RUN.
// Each step reports its checks (expected, observed, pass or fail). Progress, shots and report go through the
// commands of the cockpit scenarios (e2e_progress, e2e_released, e2e_finish).

import { e2e } from "../bridge";
import { $ } from "../dom";
import { summarize, type Summary, type UiPerf } from "../perf";
import type { E2eConfig } from "../types";
import type { Editor } from "./editor";
import type { LabState } from "./state";
import { phys } from "./api";
import { type Hit, hitTest } from "./canvas/hit";
import { HIT_PX } from "./canvas/gestures";
import { SceneModel } from "./canvas/model";
import type { CompId, Component, Edit, Kind, KindDef, Measure, PinRef, Pt, Rect, Rot, ValuesDto, WireId } from "./types";
import { formatNumber, NBSP } from "./units";
import { physView, type FrameRecord, type PhysView } from "./view";
import { isolateSoakNavigation } from "./soak-navigation";

const sleep = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));
const nextFrame = () => new Promise<number>((r) => requestAnimationFrame(r));
const frames = async (n: number) => {
  for (let i = 0; i < n; i++) await nextFrame();
};

class Failure extends Error {}

interface Check {
  name: string;
  ok: boolean;
  expected: string;
  observed: string;
}

interface StepResult {
  name: string;
  ok: boolean;
  ms: number;
  checks: Check[];
  detail?: unknown;
  error?: string;
}

type CheckFn = (name: string, ok: boolean, expected: string, observed: string) => void;

export async function runPhysScenario(cfg: E2eConfig, perf: UiPerf): Promise<void> {
  const steps: StepResult[] = [];
  const report: Record<string, unknown> = {
    app: "N7RT CAN Lab",
    scenario: cfg.scenario,
    started_at: new Date().toISOString(),
    config: { ...cfg, instructor_code: cfg.instructor_code ? "(défini)" : null, group_code: cfg.group_code ? "(défini)" : null },
    steps,
  };
  let ok = false;
  try {
    const view = physView();
    if (!view) throw new Failure("vue Physical Lab absente");
    const runner = new Runner(cfg, view, steps, report);
    if (cfg.scenario === "phys") await runner.functional();
    else await runner.performance();
    ok = steps.length > 0 && steps.every((s) => s.ok) && report.budgets_ok !== false;
  } catch (err) {
    report.error = String(err);
  }
  report.ui = perf.summary();
  report.ok = ok;
  report.finished_at = new Date().toISOString();
  await e2e.progress(ok ? "done" : "failed");
  await e2e.finish(report, ok);
}

// ------------------------------------------------------------------------------------------- geometry

/** R90 : (x, y) → (−y, x), applied rot/90 times (contract §1.3). */
function rotate(p: Pt, rot: Rot): Pt {
  let [x, y] = p;
  for (let i = 0; i < rot / 90; i++) [x, y] = [-y, x];
  return [x, y];
}

const toWorld = (c: Component, local: Pt): Pt => {
  const [x, y] = rotate(local, c.rot);
  return [c.pos[0] + x, c.pos[1] + y];
};

const inside = (p: Pt, r: Rect) => p[0] >= r.x0 && p[0] <= r.x1 && p[1] >= r.y0 && p[1] <= r.y1;

/** A point of the body that is neither an actuator nor near a pin: grabbing it selects or drags the part. */
function grabLocal(def: KindDef): Pt {
  const b = def.body;
  const candidates: Pt[] = [
    [(b.x0 + b.x1) / 2, (b.y0 + b.y1) / 2],
    [b.x0 + 0.5, b.y1 - 0.5], [b.x1 - 0.5, b.y1 - 0.5], [b.x0 + 0.5, b.y0 + 0.5], [b.x1 - 0.5, b.y0 + 0.5],
  ];
  const ok = candidates.find((p) => inside(p, b) && !def.actuators.some((a) => inside(p, a.zone)) &&
    def.pins.every((pin) => Math.hypot(pin.pos[0] - p[0], pin.pos[1] - p[1]) > 0.6));
  return ok ?? candidates[0];
}

function zoneCentre(def: KindDef, kind: "toggle_switch" | "press_button" | "drag_pot"): Pt {
  const z = def.actuators.find((a) => a.kind === kind)?.zone;
  if (!z) throw new Failure(`${def.kind} sans actionneur ${kind}`);
  return [(z.x0 + z.x1) / 2, (z.y0 + z.y1) / 2];
}

// ------------------------------------------------------------------------------------------- gestures

const BUTTONS: Record<number, number> = { 0: 1, 1: 4, 2: 2 };

/** Synthetic input as a mouse produces it: pointer events, their mouse compatibility events, click. */
class Gestures {
  private pressed: Element | null = null;
  private buttons = 0;

  constructor(private readonly view: PhysView) {}

  editor(): Editor {
    const e = this.view.editor;
    if (!e) throw new Failure(`éditeur de schéma indisponible : ${this.view.error ?? "non créé"}`);
    return e;
  }

  at(p: Pt): { x: number; y: number } {
    return this.editor().worldToClient(p);
  }

  private targetAt(x: number, y: number): Element {
    const host = $("#phys-canvas");
    const t = document.elementFromPoint(x, y);
    if (!t || !host.contains(t)) throw new Failure(`point (${Math.round(x)}, ${Math.round(y)}) hors du schéma : ` +
      JSON.stringify({ canvas: host.getBoundingClientRect().toJSON(), viewport: [innerWidth, innerHeight],
        visibility: document.visibilityState, target: t ? `${t.tagName}#${t.id}.${t.className}` : null }));
    return t;
  }

  private fire(t: Element, type: string, x: number, y: number, button: number): void {
    const init = {
      bubbles: true, cancelable: true, composed: true, clientX: x, clientY: y, screenX: x, screenY: y, button,
      buttons: this.buttons, view: window,
    };
    t.dispatchEvent(new PointerEvent(type, { ...init, pointerId: 1, pointerType: "mouse", isPrimary: true, width: 1, height: 1, pressure: this.buttons ? 0.5 : 0 }));
    const mouse = ({ pointerdown: "mousedown", pointermove: "mousemove", pointerup: "mouseup" } as Record<string, string>)[type];
    if (mouse) t.dispatchEvent(new MouseEvent(mouse, init));
  }

  /** Client coordinates (CSS pixels). While a button is held the events go to the pressed element (capture). */
  moveTo(x: number, y: number): void {
    this.fire(this.pressed ?? this.targetAt(x, y), "pointermove", x, y, -1);
  }

  downAt(x: number, y: number, button = 0): void {
    const t = this.targetAt(x, y);
    this.pressed = t;
    this.buttons = BUTTONS[button] ?? 1;
    this.fire(t, "pointerdown", x, y, button);
  }

  upAt(x: number, y: number, button = 0): void {
    const t = this.pressed ?? this.targetAt(x, y);
    this.buttons = 0;
    this.fire(t, "pointerup", x, y, button);
    if (button === 0) t.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, clientX: x, clientY: y, button, view: window }));
    this.pressed = null;
  }

  /** Release an interrupted synthetic gesture through the same cancellation path as a lost pointer. */
  cancel(): void {
    const t = this.pressed;
    this.pressed = null;
    this.buttons = 0;
    try {
      if (t) this.fire(t, "pointercancel", 0, 0, -1);
    } finally {
      this.editor().cancelGesture();
    }
  }

  move(p: Pt): void {
    const { x, y } = this.at(p);
    this.moveTo(x, y);
  }

  async click(p: Pt): Promise<void> {
    const { x, y } = this.at(p);
    this.moveTo(x, y);
    await nextFrame();
    this.downAt(x, y);
    this.upAt(x, y);
    await nextFrame();
  }

  async drag(from: Pt, to: Pt, steps = 4): Promise<void> {
    const a = this.at(from);
    const b = this.at(to);
    this.moveTo(a.x, a.y);
    await nextFrame();
    this.downAt(a.x, a.y);
    for (let i = 1; i <= steps; i++) {
      this.moveTo(a.x + ((b.x - a.x) * i) / steps, a.y + ((b.y - a.y) * i) / steps);
      await nextFrame();
    }
    this.upAt(b.x, b.y);
    await nextFrame();
  }

  wheel(x: number, y: number, deltaY: number): void {
    this.targetAt(x, y).dispatchEvent(new WheelEvent("wheel", {
      bubbles: true, cancelable: true, composed: true, clientX: x, clientY: y, deltaY, deltaMode: 0, view: window,
    }));
  }

  key(key: string, init: KeyboardEventInit = {}): void {
    const t = document.activeElement instanceof HTMLElement ? document.activeElement : document.body;
    t.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true, ...init }));
    t.dispatchEvent(new KeyboardEvent("keyup", { key, bubbles: true, cancelable: true, ...init }));
  }
}

/** Number shown in a value cell or its tooltip: "1.499997000006 V", "−499.999 µA", "1228". */
const PREFIX_EXP: Record<string, number> = { f: -15, p: -12, n: -9, "µ": -6, m: -3, "": 0, k: 3, M: 6, G: 9, T: 12 };
export function parseShown(text: string): number | null {
  const t = text.replaceAll("−", "-").trim();
  const m = /^(-?\d+(?:\.\d+)?(?:e-?\d+)?)(?: ([fpnµmkMGT]?)(?:V|A|W|Ω|F|Hz|s|bit|%)?)?$/.exec(t);
  if (!m) return null;
  const exp = PREFIX_EXP[m[2] ?? ""];
  return Number(exp ? `${m[1]}e${exp}` : m[1]);
}

const isCode = (m: Measure) => m.name === "code" || /code/.test(m.name);

/** What a view switch must leave untouched in the cockpit: console tab, panels, layout, panel toggles, slots. */
function cockpitShell(): Record<string, string> {
  const app = $("#app");
  const root = $('.workspace[data-view="cockpit"]');
  const panels = ["left", "bottom", "right"];
  return {
    onglet: root.querySelector<HTMLElement>('.tab[aria-selected="true"]')?.dataset.tab ?? "",
    panneau_console: Array.from(root.querySelectorAll<HTMLElement>(".tab-panel[data-panel]")).filter((p) => !p.hidden).map((p) => p.dataset.panel).join(","),
    panneaux_masques: panels.filter((p) => app.classList.contains(`no-${p}`)).join(","),
    dimensions: ["--w-left", "--w-right", "--h-bottom"].map((v) => app.style.getPropertyValue(v)).join(" "),
    bascules: panels.map((p) => `${$(`#toggle-${p}`).getAttribute("aria-label")}:${$(`#toggle-${p}`).getAttribute("aria-pressed")}`).join(", "),
    emplacements: Array.from(document.querySelectorAll<HTMLElement>('.workbench > [data-view="cockpit"]')).map((e) => (e.hidden ? "masqué" : "visible")).join(","),
    filtres: [$<HTMLInputElement>("#frame-filter").value, $<HTMLInputElement>("#console-filter").value].join("|"),
  };
}

const toggleTitles = () => ["left", "bottom", "right"].map((p) => $(`#toggle-${p}`).getAttribute("aria-label") ?? "").join(", ");

// ------------------------------------------------------------------------------------------- the runner

class Runner {
  private readonly g: Gestures;
  private readonly state: LabState;

  constructor(private readonly cfg: E2eConfig, private readonly view: PhysView, private readonly steps: StepResult[],
    private readonly report: Record<string, unknown>) {
    this.g = new Gestures(view);
    this.state = view.state;
  }

  private async step(name: string, fn: (check: CheckFn) => Promise<unknown>): Promise<void> {
    await e2e.progress(name);
    const t0 = performance.now();
    const checks: Check[] = [];
    const check: CheckFn = (n, ok, expected, observed) => checks.push({ name: n, ok, expected, observed });
    try {
      const detail = await fn(check);
      this.steps.push({ name, ok: checks.every((c) => c.ok), ms: Math.round(performance.now() - t0), checks, detail });
    } catch (err) {
      this.steps.push({ name, ok: false, ms: Math.round(performance.now() - t0), checks, error: String(err) });
      throw err;
    }
  }

  private async until(what: string, pred: () => boolean, timeoutMs: number): Promise<number> {
    const t0 = performance.now();
    while (!pred()) {
      if (performance.now() - t0 > timeoutMs) {
        throw new Failure(`délai dépassé (${timeoutMs} ms) en attendant : ${what} — message « ${this.state.message?.text ?? ""} »`);
      }
      await sleep(15);
    }
    return performance.now() - t0;
  }

  private async released(gate: string, timeoutMs: number): Promise<void> {
    const t0 = performance.now();
    while (!(await e2e.released(gate))) {
      if (performance.now() - t0 > timeoutMs) throw new Failure(`le banc n'a pas libéré l'étape ${gate}`);
      await sleep(50);
    }
  }

  /** The harness captures the window at shot:<name>; it releases the step when coordinated. */
  private async shot(name: string): Promise<void> {
    await sleep(400);
    await e2e.progress(`shot:${name}`);
    if (this.cfg.coordinated) await this.released(`shot-${name}`, 20000);
    else await sleep(300);
  }

  private async clickButton(sel: string): Promise<void> {
    const b = $<HTMLButtonElement>(sel);
    await this.until(`bouton ${sel} actif`, () => !b.disabled, 3000);
    b.click();
  }

  private async openView(): Promise<{ ready_ms: number }> {
    const t0 = performance.now();
    await this.clickButton('.activity button[data-show-view="phys"]');
    await this.until("vue Physical Lab affichée", () => !$("#phys-main").hidden, 2000);
    await this.view.whenReady();
    if (!this.view.ready) throw new Failure(this.view.error ?? "Physical Lab indisponible");
    this.g.editor();
    await frames(3);
    return { ready_ms: Math.round(performance.now() - t0) };
  }

  private def(id: CompId): KindDef {
    const c = this.state.components.get(id);
    if (!c) throw new Failure(`composant #${id} absent`);
    return this.state.kindDef(c.kind);
  }

  private comp(id: CompId): Component {
    const c = this.state.components.get(id);
    if (!c) throw new Failure(`composant #${id} absent`);
    return c;
  }

  /** Fit has a zoom floor: pan the actual view until these actuator points can be hit, without changing zoom. */
  private async ensureVisible(points: Pt[]): Promise<void> {
    const host = $("#phys-canvas");
    const margin = 12;
    for (let attempt = 0; attempt <= 3; attempt++) {
      // ResizeObserver only marks sizePending; the editor applies it in its next real frame.
      await frames(3);
      const r = host.getBoundingClientRect();
      const client = points.map((p) => this.g.at(p));
      if (!client.length || client.some((p) => !Number.isFinite(p.x) || !Number.isFinite(p.y))
          || r.width <= 2 * margin || r.height <= 2 * margin) {
        throw new Failure("géométrie indisponible pour recadrer les actionneurs");
      }
      const x0 = Math.min(...client.map((p) => p.x)), x1 = Math.max(...client.map((p) => p.x));
      const y0 = Math.min(...client.map((p) => p.y)), y1 = Math.max(...client.map((p) => p.y));
      const hits = client.every((p) => {
        const target = document.elementFromPoint(p.x, p.y);
        return p.x >= r.left + margin && p.x <= r.right - margin
          && p.y >= r.top + margin && p.y <= r.bottom - margin && target !== null && host.contains(target);
      });
      if (hits) return;
      if (attempt === 3 || x1 - x0 > r.width - 2 * margin || y1 - y0 > r.height - 2 * margin) {
        throw new Failure("actionneurs non visibles après recadrage borné : " +
          JSON.stringify({ canvas: r.toJSON(), points: client, viewport: [innerWidth, innerHeight] }));
      }
      const cx = r.left + r.width / 2, cy = r.top + r.height / 2;
      const centreTarget = document.elementFromPoint(cx, cy);
      if (!centreTarget || !host.contains(centreTarget)) throw new Failure("centre du canvas masqué pendant le recadrage");
      const dx = cx - (x0 + x1) / 2, dy = cy - (y0 + y1) / 2;
      try {
        this.g.downAt(cx, cy, 1);
        this.g.moveTo(cx + dx, cy + dy);
        this.g.upAt(cx + dx, cy + dy, 1);
      } finally {
        this.g.cancel();
      }
    }
  }

  /** Each await is followed by a fresh transform: no client coordinate survives a pan or resize. */
  private async actuate(pot: CompId, sw: CompId, alpha: number): Promise<void> {
    const s = this.state;
    const closed = s.states.get(sw)?.closed === true;
    const switchPoint = toWorld(this.comp(sw), zoneCentre(this.def(sw), "toggle_switch"));
    await this.ensureVisible([switchPoint]);
    const lame = this.g.at(switchPoint);
    try {
      this.g.moveTo(lame.x, lame.y);
      this.g.downAt(lame.x, lame.y);
      this.g.upAt(lame.x, lame.y);
    } finally {
      this.g.cancel();
    }
    const potY = zoneCentre(this.def(pot), "drag_pot")[1];
    const position = Number(s.states.get(pot)?.position ?? 0.5);
    const fromWorld = toWorld(this.comp(pot), [-2 + 4 * position, potY]);
    const toWorldPoint = toWorld(this.comp(pot), [-2 + 4 * alpha, potY]);
    await this.ensureVisible([fromWorld, toWorldPoint]);
    const from = this.g.at(fromWorld), to = this.g.at(toWorldPoint);
    try {
      this.g.moveTo(from.x, from.y);
      this.g.downAt(from.x, from.y);
      this.g.moveTo(to.x, to.y);
      this.g.upAt(to.x, to.y);
    } finally {
      this.g.cancel();
    }
    await this.until("stimuli appliqués par les actionneurs visibles", () =>
      (s.states.get(sw)?.closed === true) !== closed
      && Math.abs(Number(s.states.get(pot)?.position) - alpha) < 0.02, 2000);
  }

  private grab(id: CompId): Pt {
    return toWorld(this.comp(id), grabLocal(this.def(id)));
  }

  private async useTool(tool: "select" | "wire"): Promise<void> {
    if (this.state.view.tool === tool) return;
    await this.clickButton(tool === "select" ? "#phys-tool-select" : "#phys-tool-wire");
    await this.until(`outil ${tool}`, () => this.state.view.tool === tool, 2000);
  }

  private async select(id: CompId): Promise<void> {
    const sel = this.state.selection;
    if (sel.components.size === 1 && sel.components.has(id) && sel.wires.size === 0) return;
    await this.useTool("select");
    await this.g.click(this.grab(id));
    await this.until(`sélection de ${this.comp(id).label || this.comp(id).kind}`,
      () => sel.components.size === 1 && sel.components.has(id) && sel.wires.size === 0, 2000);
    await frames(2);
  }

  /** Integer offset that centres the plan box in the schematic, zooming out until it fits. */
  private async frameBox(min: Pt, max: Pt): Promise<Pt> {
    const host = $("#phys-canvas").getBoundingClientRect();
    const margin = 32;
    for (let i = 0; i < 7; i++) {
      const o = this.g.at([0, 0]);
      const u = this.g.at([1, 0]);
      const v = this.g.at([0, 1]);
      const sx = u.x - o.x;
      const sy = v.y - o.y;
      if (!(sx > 0 && sy > 0)) throw new Failure(`repère du schéma inattendu (${sx}, ${sy})`);
      if ((max[0] - min[0]) * sx > host.width - 2 * margin || (max[1] - min[1]) * sy > host.height - 2 * margin) {
        this.g.editor().zoomStep(-1);
        await frames(3);
        continue;
      }
      const cx = (host.left + host.width / 2 - o.x) / sx;
      const cy = (host.top + host.height / 2 - o.y) / sy;
      return [Math.round(cx - (min[0] + max[0]) / 2), Math.round(cy - (min[1] + max[1]) / 2)];
    }
    throw new Failure("schéma trop petit pour le circuit de référence, même dézoomé");
  }

  /** Library press, then a click in the schematic; a part anchored elsewhere than its origin is dragged in place. */
  private async place(kind: Kind, pos: Pt): Promise<CompId> {
    const before = new Set(this.state.components.keys());
    const item = $<HTMLButtonElement>(`#phys-palette .lab-item[data-kind="${kind}"]`);
    const r = item.getBoundingClientRect();
    const init = { bubbles: true, cancelable: true, clientX: r.left + 4, clientY: r.top + 4, button: 0, pointerId: 1, pointerType: "mouse", isPrimary: true };
    item.dispatchEvent(new PointerEvent("pointerdown", { ...init, buttons: 1 }));
    item.dispatchEvent(new PointerEvent("pointerup", { ...init, buttons: 0 }));
    await nextFrame();
    this.g.move([pos[0] + 1, pos[1]]);
    await nextFrame();
    this.g.move(pos);
    await sleep(150);
    await this.g.click(pos);
    const fresh = () => [...this.state.components.values()].find((c) => !before.has(c.id) && c.kind === kind);
    await this.until(`pose de ${kind} en (${pos[0]}, ${pos[1]})`, () => fresh() !== undefined, 3000);
    this.g.key("Escape");
    const c = fresh()!;
    if (c.pos[0] !== pos[0] || c.pos[1] !== pos[1]) {
      const from = this.grab(c.id);
      await this.useTool("select");
      await this.g.drag(from, [from[0] + pos[0] - c.pos[0], from[1] + pos[1] - c.pos[1]]);
      await this.until(`${kind} déplacé en (${pos[0]}, ${pos[1]})`, () => this.comp(c.id).pos[0] === pos[0] && this.comp(c.id).pos[1] === pos[1], 3000);
    }
    return c.id;
  }

  /** Wire tool: click the start pin, the corners, then the end pin or a point of a wire. */
  private async wire(points: Pt[], what: string): Promise<void> {
    await this.useTool("wire");
    const rev = this.state.rev;
    const before = this.state.wires.size;
    for (const p of points) {
      this.g.move(p);
      await nextFrame();
      await this.g.click(p);
      await sleep(40);
    }
    await this.until(`fil ${what}`, () => this.state.rev > rev && this.state.wires.size > before, 3000);
  }

  private async typeParam(id: CompId, name: string, text: string, expect: (v: unknown) => boolean): Promise<void> {
    await this.select(id);
    const input = $<HTMLInputElement>(`#phys-inspector tr[data-param="${name}"] input`);
    input.focus();
    input.value = text;
    input.dispatchEvent(new Event("input", { bubbles: true }));
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }));
    await this.until(`${name} = ${text}`, () => expect(this.comp(id).params[name]), 3000);
    input.blur();
  }

  /** Potentiometer position typed in the inspector (field next to the slider). */
  private async typePosition(id: CompId, position: number): Promise<void> {
    await this.select(id);
    const input = $<HTMLInputElement>('#phys-inspector input[data-state="position"]');
    input.focus();
    input.value = String(position).replace(".", ",");
    input.dispatchEvent(new Event("input", { bubbles: true }));
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }));
    await this.until(`curseur à ${position}`, () => this.state.states.get(id)?.position === position, 3000);
    input.blur();
  }

  // -------------------------------------------------------------------------------- readings

  private pinValue(id: CompId, pin: string): number | null {
    const r = this.state.compValues.get(id)?.pins.find((p) => p.pin === pin)?.voltage;
    return r?.k === "valid" ? r.v : null;
  }

  private code(id: CompId): number | null {
    const m = this.state.compValues.get(id)?.measures.find(isCode);
    return m && (m.reading.k === "valid" || m.reading.k === "saturated") ? m.reading.v : null;
  }

  /** Publication counter before an action: pass it to fresh() so that values published while the action was still
   * being acknowledged count (in STOP nothing is published afterwards). */
  private mark(): number {
    return this.state.values?.seq ?? -1;
  }

  /** Values computed for the current document, published after `since` (default: now). */
  private async fresh(since = this.mark()): Promise<void> {
    const rev = this.state.rev;
    await this.until("valeurs à jour", () => (this.state.values?.rev ?? -1) >= rev && (this.state.values?.seq ?? -1) > since, 3000);
    await frames(2);
  }

  /** Text and full value of a pin voltage as the inspector shows it. */
  private async shownPin(id: CompId, pin: string, col: "v" | "i"): Promise<{ text: string; value: number | null }> {
    await this.select(id);
    const cell = $(`#phys-inspector tr[data-pin="${pin}"] td[data-col="${col}"]`);
    return { text: cell.textContent ?? "", value: parseShown(cell.title) };
  }

  private async shownMeasure(id: CompId, pick: (name: string) => boolean): Promise<{ text: string; value: number | null }> {
    await this.select(id);
    const row = Array.from(document.querySelectorAll<HTMLElement>("#phys-inspector tr[data-measure]")).find((r) => pick(r.dataset.measure ?? ""));
    const cell = row?.querySelector<HTMLElement>('[data-col="value"]');
    if (!cell) throw new Failure(`mesure absente de l'inspecteur (${this.comp(id).label})`);
    const text = cell.textContent ?? "";
    return { text, value: parseShown(text.replace(/^saturé \((.*)\)$/, "$1")) };
  }

  private async checkVolts(check: CheckFn, name: string, id: CompId, pin: string, expected: number, tol: number): Promise<number | null> {
    const shown = await this.shownPin(id, pin, "v");
    const ok = shown.value !== null && Math.abs(shown.value - expected) <= tol;
    check(name, ok, `${expected} V à ${tol} V près`, `${shown.value ?? "—"} (affiché « ${shown.text} »)`);
    if (shown.value !== null) {
      const text = formatNumber(shown.value, "V", "reading");
      check(`${name} : texte affiché`, shown.text === text, text, shown.text);
    }
    return shown.value;
  }

  private async checkCode(check: CheckFn, name: string, id: CompId, expected: number): Promise<void> {
    const shown = await this.shownMeasure(id, (n) => /code/.test(n));
    check(name, shown.value === expected, String(expected), shown.text);
  }

  // -------------------------------------------------------------------------------- transients

  /**
   * Records V(X) (pin 1 of C1, input of ADC2) while running, applies `change`, waits `taus` time constants of
   * simulated time and fits τ. A 1e−6 V check on V(W) needs about 13 τ after a step of a few hundred millivolts.
   */
  private async transient(c1: CompId, change: () => Promise<void>, tauMs: number, taus: number):
    Promise<{ tau_ms: number | null; samples: number; final: number | null }> {
    const samples: { t: number; x: number }[] = [];
    const off = this.state.onValues.on((v: ValuesDto) => {
      if (v.run !== "run" || v.preview) return;
      const x = this.pinValue(c1, "p1");
      if (x !== null) samples.push({ t: v.sim_time_ns, x });
    });
    try {
      await change();
      const t0 = this.state.simTimeNs;
      const waitMs = tauMs * taus;
      await this.until(`${Math.round(waitMs)} ms de temps simulé`, () => this.state.simTimeNs - t0 >= waitMs * 1e6, waitMs * 4 + 3000);
      await sleep(120);
      return fitTau(samples.filter((s) => s.t > t0 - 2e6));
    } finally {
      off();
    }
  }

  // -------------------------------------------------------------------------------- functional

  async functional(): Promise<void> {
    const s = this.state;
    const ids: Record<string, CompId> = {};
    // Plan in grid units (all at rotation 0): the reference case of §9.1 with a junction on the wiper wire.
    const PLAN: [string, Kind, Pt][] = [
      ["V1", "dc_supply", [0, 0]], ["S1", "switch", [8, -8]], ["RV1", "potentiometer", [20, 0]], ["R1", "resistor", [26, -12]],
      ["C1", "capacitor", [44, -12]], ["ADC1", "adc_input", [34, -6]], ["ADC2", "adc_input", [44, -4]],
      ["G1", "ground", [0, 8]], ["G2", "ground", [14, 3]], ["G3", "ground", [34, 0]], ["G4", "ground", [50, -12]], ["G5", "ground", [44, 2]],
    ];
    let off: Pt = [0, 0];
    const at = (p: Pt): Pt => [p[0] + off[0], p[1] + off[1]];
    // The e2e start view is the cockpit: its shell before any switch.
    const cockpitBefore = cockpitShell();

    await this.step("vue", async (check) => {
      const r = await this.openView();
      const titles = toggleTitles();
      check("bascules de panneaux du Physical Lab", titles === "Bibliothèque, Console, Inspecteur", "Bibliothèque, Console, Inspecteur", titles);
      return r;
    });

    await this.step("nouveau", async (check) => {
      if (s.components.size > 0 || s.dirty) {
        await this.clickButton("#phys-new");
        if (s.dirty) await this.clickButton("#phys-new");
        await this.until("circuit vide", () => s.components.size === 0, 3000);
      }
      if (s.run === "run") await this.clickButton("#phys-stop");
      await this.until("STOP", () => s.run === "stop", 2000);
      check("circuit vide", s.components.size === 0 && s.wires.size === 0, "0 composant, 0 fil", `${s.components.size}, ${s.wires.size}`);
      off = await this.frameBox([-3, -15], [53, 11]);
      return { offset: off, zoom: s.view.zoomPercent };
    });

    await this.step("placement", async (check) => {
      for (const [key, kind, pos] of PLAN) ids[key] = await this.place(kind, at(pos));
      for (const key of ["V1", "S1", "RV1", "R1", "C1", "ADC1", "ADC2"]) {
        check(`désignateur ${key}`, this.comp(ids[key]).label === key, key, this.comp(ids[key]).label);
      }
      return { components: s.components.size };
    });

    const pin = (key: string, p: string): Pt => toWorld(this.comp(ids[key]), this.def(ids[key]).pins.find((d) => d.id === p)!.pos);
    await this.step("cablage", async (check) => {
      await this.wire([pin("V1", "pos"), at([0, -8]), pin("S1", "p1")], "V1.+ → S1.1");
      await this.wire([pin("S1", "p2"), at([11, 7]), at([25, 7]), at([25, 0]), pin("RV1", "b")], "S1.2 → RV1.B");
      await this.wire([pin("RV1", "a"), at([14, 0]), pin("G2", "gnd")], "RV1.A → masse");
      await this.wire([pin("V1", "neg"), pin("G1", "gnd")], "V1.− → masse");
      await this.wire([pin("RV1", "w"), at([20, -6]), pin("ADC1", "in")], "RV1.W → ADC1.IN");
      await this.wire([pin("R1", "p1"), at([23, -6])], "R1.1 → jonction sur le fil du curseur");
      await this.wire([pin("R1", "p2"), pin("C1", "p1")], "R1.2 → C1.1 (X)");
      await this.wire([pin("C1", "p2"), pin("G4", "gnd")], "C1.2 → masse");
      await this.wire([pin("ADC2", "in"), pin("C1", "p1")], "ADC2.IN → X");
      await this.wire([pin("ADC1", "ref"), pin("G3", "gnd")], "ADC1.REF → masse");
      await this.wire([pin("ADC2", "ref"), pin("G5", "gnd")], "ADC2.REF → masse");
      this.g.key("Escape");
      await this.useTool("select");
      const net = (key: string, p: string) => s.netOf({ comp: ids[key], pin: p } as PinRef);
      const same = (members: [string, string][]) => {
        const first = net(...members[0]);
        return first !== undefined && members.every((m) => net(...m) === first);
      };
      const groups: Record<string, [string, string][]> = {
        GND: [["V1", "neg"], ["RV1", "a"], ["C1", "p2"], ["ADC1", "ref"], ["ADC2", "ref"], ["G1", "gnd"], ["G2", "gnd"], ["G3", "gnd"], ["G4", "gnd"], ["G5", "gnd"]],
        alimentation: [["V1", "pos"], ["S1", "p1"]],
        B: [["S1", "p2"], ["RV1", "b"]],
        W: [["RV1", "w"], ["ADC1", "in"], ["R1", "p1"]],
        X: [["R1", "p2"], ["C1", "p1"], ["ADC2", "in"]],
      };
      for (const [name, members] of Object.entries(groups)) check(`net ${name}`, same(members), members.map((m) => m.join(".")).join(", "), net(...members[0])?.name ?? "—");
      const distinct = new Set(Object.values(groups).map((m) => net(...m[0])));
      check("5 nets distincts", distinct.size === 5, "5", String(distinct.size));
      check("masse", net("V1", "neg")?.ground === true, "net GND", net("V1", "neg")?.name ?? "—");
      const junctions = [...s.components.values()].filter((c) => c.kind === "junction").length;
      check("jonction sur le fil du curseur", junctions === 1, "1", String(junctions));
      return { wires: s.wires.size, nets: s.nets.length };
    });

    await this.step("parametres", async (check) => {
      for (const key of ["ADC1", "ADC2"]) await this.typeParam(ids[key], "vref", "5", (v) => v === 5);
      await this.typePosition(ids.RV1, 0.3);
      // The switch is closed by its actuator in the schematic (a press actuates, in every tool).
      await this.g.click(toWorld(this.comp(ids.S1), zoneCentre(this.def(ids.S1), "toggle_switch")));
      let viaCanvas = true;
      try {
        await this.until("S1 fermé", () => s.states.get(ids.S1)?.closed === true, 2000);
      } catch {
        viaCanvas = false;
        await this.select(ids.S1);
        const box = $<HTMLInputElement>('#phys-inspector input[data-state="closed"]');
        box.checked = true;
        box.dispatchEvent(new Event("change", { bubbles: true }));
        await this.until("S1 fermé (inspecteur)", () => s.states.get(ids.S1)?.closed === true, 2000);
      }
      check("S1 fermé par son actionneur", viaCanvas, "clic sur la lame", viaCanvas ? "fermé" : "sans effet, fermé par l'inspecteur");
      check("vref ADC1, ADC2", this.comp(ids.ADC1).params.vref === 5 && this.comp(ids.ADC2).params.vref === 5, "5 V", `${this.comp(ids.ADC1).params.vref}, ${this.comp(ids.ADC2).params.vref}`);
      await this.shot("phys-editor");
      return {};
    });

    // STOP: the preview holds the capacitors (C1 discharged at t = 0, §5.6).
    await this.step("apercu-stop", async (check) => {
      const since = this.mark();
      await this.clickButton("#phys-reset");
      await this.fresh(since);
      check("aperçu publié en STOP", s.values?.preview === true && s.run === "stop", "preview, STOP", `${s.values?.preview}, ${s.run}`);
      await this.checkVolts(check, "V(W) à 0+, C1 déchargé", ids.RV1, "w", 1.2396668, 1e-6);
      return {};
    });

    const running = async () => {
      await this.clickButton("#phys-run");
      await this.until("RUN", () => s.run === "run", 2000);
    };
    const tauCheck = (check: CheckFn, name: string, t: { tau_ms: number | null; samples: number }, expected: number) =>
      check(name, t.tau_ms !== null && Math.abs(t.tau_ms - expected) <= expected * 0.03, `${expected} ms à 3 % près`,
        `${t.tau_ms?.toFixed(2) ?? "—"} ms (${t.samples} points)`);

    await this.step("run", async (check) => {
      const t = await this.transient(ids.C1, running, 121, 13);
      tauCheck(check, "τ de la charge de C1 (ADC2 monte vers 1228)", t, 121);
      await this.checkVolts(check, "V(W)", ids.RV1, "w", 1.499997, 1e-6);
      await this.checkCode(check, "ADC1", ids.ADC1, 1228);
      await this.checkCode(check, "ADC2", ids.ADC2, 1228);
      await this.select(ids.RV1);
      await this.shot("phys-run");
      return { tau_ms: t.tau_ms };
    });

    await this.step("curseur-0.7", async (check) => {
      await this.clickButton("#phys-stop");
      await this.until("STOP", () => s.run === "stop", 2000);
      const since = this.mark();
      await this.typePosition(ids.RV1, 0.7);
      await this.fresh(since);
      await this.checkVolts(check, "V(W) à 0+ (aperçu)", ids.RV1, "w", 3.1528849, 1e-6);
      await this.checkCode(check, "ADC1 à 0+ (aperçu)", ids.ADC1, 2582);
      const t = await this.transient(ids.C1, running, 121, 13);
      tauCheck(check, "τ, ADC2 suit", t, 121);
      await this.checkVolts(check, "V(W)", ids.RV1, "w", 3.499993, 1e-6);
      await this.checkCode(check, "ADC1", ids.ADC1, 2867);
      await this.checkCode(check, "ADC2", ids.ADC2, 2867);
      return { tau_ms: t.tau_ms };
    });

    await this.step("interrupteur", async (check) => {
      const lame = toWorld(this.comp(ids.S1), zoneCentre(this.def(ids.S1), "toggle_switch"));
      await this.clickButton("#phys-stop");
      await this.until("STOP", () => s.run === "stop", 2000);
      const since = this.mark();
      await this.g.click(lame);
      await this.until("S1 ouvert", () => s.states.get(ids.S1)?.closed === false, 2000);
      await this.fresh(since);
      await this.checkVolts(check, "V(W) à 0+, S1 ouvert (aperçu)", ids.RV1, "w", 1.4411882, 1e-6);
      await this.checkCode(check, "ADC1 à 0+, S1 ouvert (aperçu)", ids.ADC1, 1180);
      const open = await this.transient(ids.C1, running, 169.9995, 8);
      tauCheck(check, "τ, S1 ouvert", open, 169.9995);
      await this.checkCode(check, "ADC1 vers 0", ids.ADC1, 0);
      const closed = await this.transient(ids.C1, async () => {
        await this.g.click(lame);
        await this.until("S1 refermé", () => s.states.get(ids.S1)?.closed === true, 2000);
      }, 121, 10);
      tauCheck(check, "τ, S1 refermé", closed, 121);
      await this.checkCode(check, "ADC1 retour", ids.ADC1, 2867);
      return { tau_open_ms: open.tau_ms, tau_closed_ms: closed.tau_ms };
    });

    await this.step("r1-22k", async (check) => {
      const t0 = s.simTimeNs;
      await this.typeParam(ids.R1, "resistance", "22k", (v) => v === 22000);
      const t = await this.transient(ids.C1, () => this.typePosition(ids.RV1, 0.3), 241, 13);
      tauCheck(check, "τ avec R1 = 22 kΩ, en RUN", t, 241);
      check("sans redémarrage", s.run === "run" && s.simTimeNs > t0, "RUN continu, temps croissant", `${s.run}, t = ${(s.simTimeNs / 1e9).toFixed(3)} s`);
      await this.checkVolts(check, "V(W), valeur finale inchangée", ids.RV1, "w", 1.499997, 1e-6);
      await this.checkCode(check, "ADC1", ids.ADC1, 1228);
      return { tau_ms: t.tau_ms };
    });

    await this.step("c1-4u7", async (check) => {
      const before = this.code(ids.ADC2);
      const codes: number[] = [];
      const offValues = s.onValues.on(() => {
        const c = this.code(ids.ADC2);
        if (c !== null) codes.push(c);
      });
      await this.typeParam(ids.C1, "capacitance", "4,7µ", (v) => v === 4.7e-6);
      await sleep(300);
      offValues();
      check("sans saut d'ADC2", before !== null && codes.length > 0 && codes.every((c) => c === before), `ADC2 = ${before}`, codes.slice(0, 8).join(", ") || "aucune valeur");
      const t = await this.transient(ids.C1, () => this.typePosition(ids.RV1, 0.7), 113.27, 10);
      tauCheck(check, "τ avec C1 = 4,7 µF", t, 113.27);
      await this.checkCode(check, "ADC2", ids.ADC2, 2867);
      return { tau_ms: t.tau_ms };
    });

    await this.step("inspecteur-rv1", async (check) => {
      await this.transient(ids.C1, () => this.typePosition(ids.RV1, 0.3), 113.27, 13);
      const vb = await this.shownPin(ids.RV1, "b", "v");
      check("V(B)", vb.text === `4.99999${NBSP}V`, `4.99999${NBSP}V`, vb.text);
      const ib = await this.shownPin(ids.RV1, "b", "i");
      check("courant de RV1", ib.text.replace("−", "") === `499.999${NBSP}µA`, `499.999${NBSP}µA`, ib.text);
      const p = await this.shownMeasure(ids.RV1, (n) => n === "power");
      check("puissance de RV1", p.text === `2.49999${NBSP}mW`, `2.49999${NBSP}mW`, p.text);
      const op = $("#phys-inspector .lab-op").textContent ?? "";
      check("état du curseur", op.includes("0.300"), "curseur à 0.300", op);
      await this.shot("phys-inspector");
      await this.clickButton('.tab[data-tab="phys-nets"]');
      await frames(3);
      const count = $("#phys-count-nets").textContent ?? "";
      const first = document.querySelector("#phys-nets .lab-row:not([hidden]) span")?.textContent ?? "";
      check("onglet Nets", count === String(s.nets.length) && s.nets.some((n) => n.name === first), `${s.nets.length} nets listés`, `${count}, première ligne « ${first} »`);
      await this.clickButton('.tab[data-tab="phys-diag"]');
      await frames(3);
      const errors = (s.values?.diagnostics ?? []).filter((d) => d.severity === "error");
      check("aucun diagnostic d'erreur", errors.length === 0, "0", errors.map((d) => d.code).join(", ") || "0");
      return { diagnostics: (s.values?.diagnostics ?? []).map((d) => `${d.severity}:${d.code}`) };
    });

    await this.step("enregistrer-recharger", async (check) => {
      await this.clickButton("#phys-stop");
      await this.until("STOP", () => s.run === "stop", 2000);
      const name = "e2e-phys";
      const field = $<HTMLInputElement>("#phys-file-name");
      field.value = name;
      field.dispatchEvent(new Event("input", { bubbles: true }));
      await this.clickButton("#phys-save");
      try {
        await this.until("enregistré", () => s.name === name && !s.dirty, 2000);
      } catch {
        // name_exists: the button asks to overwrite, a second click confirms.
        await this.until("demande d'écrasement", () => $("#phys-save").classList.contains("btn--danger"), 2000);
        await this.clickButton("#phys-save");
        await this.until("enregistré (écrasement)", () => s.name === name && !s.dirty, 3000);
      }
      const saved = circuitText(s);
      await this.clickButton("#phys-new");
      await this.until("nouveau circuit", () => s.components.size === 0, 3000);
      const select = $<HTMLSelectElement>("#phys-open-select");
      select.dispatchEvent(new Event("focus"));
      await this.until("circuit dans la liste", () => Array.from(select.options).some((o) => o.value === name), 3000);
      select.value = name;
      select.dispatchEvent(new Event("change", { bubbles: true }));
      await frames(2);
      await this.clickButton("#phys-open");
      await this.until("circuit rechargé", () => s.name === name && s.components.size > 0, 3000);
      const loaded = circuitText(s);
      check("circuit identique après rechargement", loaded === saved, `${saved.length} caractères`, loaded === saved ? "identique" : firstDifference(saved, loaded));
      return { components: s.components.size, wires: s.wires.size };
    });

    // cockpit → phys → cockpit: the cockpit comes back exactly as it was; the Physical Lab keeps its document.
    await this.step("bascule-cockpit", async (check) => {
      const doc = circuitText(s);
      await this.clickButton('.activity button[data-show-view="cockpit"]');
      await this.until("cockpit affiché", () => !$('.workspace[data-view="cockpit"]').hidden, 2000);
      await frames(3);
      const after = cockpitShell();
      for (const [k, v] of Object.entries(cockpitBefore)) check(`cockpit, ${k}`, after[k] === v, v, after[k]);
      const physSlots = Array.from(document.querySelectorAll<HTMLElement>('.workbench > [data-view="phys"]'));
      check("emplacements du Physical Lab masqués", physSlots.every((e) => e.hidden), "masqués", physSlots.map((e) => (e.hidden ? "masqué" : "visible")).join(","));
      await this.clickButton('.activity button[data-show-view="phys"]');
      await this.until("Physical Lab réaffiché", () => !$("#phys-main").hidden, 2000);
      await frames(3);
      check("circuit conservé", circuitText(s) === doc, "identique", circuitText(s) === doc ? "identique" : firstDifference(doc, circuitText(s)));
      const titles = toggleTitles();
      check("bascules du Physical Lab rétablies", titles === "Bibliothèque, Console, Inspecteur", "Bibliothèque, Console, Inspecteur", titles);
      return {};
    });

    // The two embedded demonstrations (§9), as the student opens them: loaded, run for 3 s of simulated time (25 τ of
    // C1 in ref-pot-rc, τ = 121 ms: settled to 1e−6 V), then captured with a component selected so that the
    // inspector shows its pinout. No error or warning diagnostic is expected.
    const DEMOS: [string, string, string, [string, string, number][]][] = [
      ["ref-pot-rc", "phys-demo-ref-pot-rc", "RV1", [["RV1", "w", 1.4999970]]],
      ["tp1-capteur-can", "phys-demo-tp1-capteur-can", "U2", [["U2", "canh", 2.4997483], ["U2", "canl", 2.4997483], ["U2", "vdd", 4.9989934]]],
    ];
    for (const [id, shotName, focus, expected] of DEMOS) {
      await this.step(`demo-${id}`, async (check) => {
        if (s.run === "run") {
          await this.clickButton("#phys-stop");
          await this.until("STOP", () => s.run === "stop", 2000);
        }
        const r = await this.view.controller.loadDemo(id);
        if (!r.ok) throw new Failure(`${id} : ${JSON.stringify(r.error)}`);
        const byLabel = (label: string): CompId => {
          const c = Array.from(s.components.values()).find((x) => x.label === label);
          if (!c) throw new Failure(`${id} : composant ${label} absent`);
          return c.id;
        };
        await this.clickButton("#phys-run");
        const t0 = s.simTimeNs;
        await this.until("3 s de temps simulé", () => s.run === "run" && s.simTimeNs - t0 >= 3e9, 15000);
        await this.fresh();
        for (const [label, pin, volts] of expected) {
          const v = this.pinValue(byLabel(label), pin);
          check(`${label}.${pin}`, v !== null && Math.abs(v - volts) <= 1e-6, `${volts} V à 0.000001 V près`, String(v));
        }
        const bad = (s.values?.diagnostics ?? []).filter((d) => d.severity !== "info");
        check("aucun diagnostic d'erreur ni d'avertissement", bad.length === 0, "0", bad.map((d) => d.code).join(", ") || "0");
        await this.select(byLabel(focus));
        await frames(4);
        await this.shot(shotName);
        await this.clickButton("#phys-stop");
        return {};
      });
    }
  }

  // -------------------------------------------------------------------------------- performance

  async performance(): Promise<void> {
    const s = this.state;
    const rec = new FrameLog(this.view);
    const freezes = new Freezes();
    const budgets: Record<string, { value: number; limit: number; ok: boolean }> = {};
    this.report.budgets = budgets;
    const budget = (name: string, value: number, limit: number) => {
      budgets[name] = { value: Math.round(value * 100) / 100, limit: Math.round(limit * 100) / 100, ok: value <= limit };
    };
    let T = 1000 / 60;

    // Memory marks (§8.6): at mem:<label> the harness waits for the private memory of the process tree to settle,
    // then releases the gate. mem:cockpit (the cockpit alone) is the reference of the budget; mem:lab (the perf
    // document loaded, at rest) and mem:fin (after the phases, at rest) separate the footprint from the caches.
    const memGate = async (name: string, label: string) => {
      if (!this.cfg.coordinated) return;
      await this.step(name, async () => {
        const t0 = performance.now();
        await e2e.progress(`mem:${label}`);
        await this.released(`mem-${label}`, 30000);
        return { wait_ms: Math.round(performance.now() - t0), js_heap_mb: { used: heapMb(), committed: heapMb("totalJSHeapSize") } };
      });
    };
    await memGate("memoire-cockpit", "cockpit");
    await this.step("vue", () => this.openView());
    await this.step("chargement", async (check) => {
      if (s.run === "run") await this.clickButton("#phys-stop");
      rec.start();
      const t0 = performance.now();
      const r = await this.view.controller.loadDemo("perf-150-300");
      if (!r.ok) throw new Failure(`perf-150-300 : ${JSON.stringify(r.error)}`);
      // First image of the loaded circuit: the first frame that drew with the snapshot's revision.
      const first = await rec.first((f) => f.drew && f.rev === r.reply.rev, 5000);
      const ms = first.end - t0;
      budget("chargement_ms", ms, 200);
      const comps = [...s.components.values()];
      const junctions = comps.filter((c) => c.kind === "junction").length;
      check("gabarit", comps.length - junctions >= 150 && junctions >= 50 && s.wires.size >= 300,
        "≥ 150 composants, ≥ 50 jonctions, ≥ 300 fils", `${comps.length - junctions}, ${junctions}, ${s.wires.size}`);
      return { load_ms: Math.round(ms), components: comps.length - junctions, junctions, wires: s.wires.size };
    });

    await this.step("repos", async () => {
      this.g.editor().fit();
      await sleep(300);
      rec.start();
      await sleep(2000);
      const gaps = rec.gaps();
      T = gaps.length ? summarize(gaps).p50 : T;
      return { T_ms: Math.round(T * 100) / 100, frames: gaps.length };
    });
    await memGate("memoire-lab", "lab");

    const centre = () => {
      const r = $("#phys-canvas").getBoundingClientRect();
      return { x: r.left + r.width / 2, y: r.top + r.height / 2 };
    };
    /**
     * 10 s, one input per frame. input_to_draw counts only the inputs that change what is drawn (the driver marks
     * them right after dispatching); `edits`: every release sends an edit (edit_to_values); `run`: engine lag.
     */
    const phase = async (name: string, drive: Driver, edits = false, run = false) => {
      await this.step(name, async (check) => {
        const lags: number[] = [];
        const offValues = s.onValues.on((v) => lags.push(v.engine.lag_frames));
        const heap0 = heapMb();
        rec.start();
        freezes.reset();
        const inputs: number[] = [];
        const editTimes: number[] = [];
        const marks: Marks = { input: () => inputs.push(performance.now()), edit: () => editTimes.push(performance.now()) };
        const t0 = performance.now();
        let i = 0;
        while (performance.now() - t0 < PHASE_MS) {
          await nextFrame();
          const out = drive(i++, marks);
          if (out) await out;
        }
        const end = drive(-1, marks);
        if (end) await end;
        await sleep(300);
        offValues();
        drive.verdict?.(check);
        const gaps = summarize(rec.gaps());
        const render = summarize(rec.renderMs());
        const i2d = summarize(rec.inputToDraw(inputs));
        const e2v = summarize(rec.editToValues(editTimes));
        const lag = summarize(lags);
        const frozen = freezes.count(rec.gaps());
        budget(`${name}.frame_gap_p95_ms`, gaps.p95, 1.1 * T);
        // One missed vsync plus timestamp jitter (§8.6): at 144 Hz, 2·T alone fails on 0.1 ms of jitter.
        budget(`${name}.frame_gap_p99_ms`, gaps.p99, 2 * T + 1);
        budget(`${name}.frame_gap_max_ms`, gaps.max, 50);
        budget(`${name}.render_ms_p95`, render.p95, 8);
        budget(`${name}.render_ms_p99`, render.p99, 12);
        budget(`${name}.gels`, frozen, 0);
        budget(`${name}.input_to_draw_p95_ms`, i2d.p95, 50);
        if (edits) budget(`${name}.edit_to_values_p95_ms`, e2v.p95, 100);
        if (run) budget(`${name}.retard_moteur_p99_ms`, lag.p99, 20);
        const m = this.g.editor().metrics;
        const e = s.values?.engine;
        return {
          frame_gap: round(gaps), render_ms: round(render), input_to_draw: round(i2d), edit_to_values: round(e2v),
          gels: frozen, gels_source: freezes.source, gels_detail: freezes.long, long_gaps: rec.longGaps(t0),
          engine_lag_frames: round(lag),
          solver: e ? { substeps_last: e.substeps_last, frame_us_p95: e.frame_us_p95, islands: e.islands, min_dt_ns: e.min_dt_ns, slow: e.slow, restarts: e.restarts } : null,
          editor: { full: m.fullRepaints, partial: m.partialRepaints, overlay: m.overlayRepaints },
          heap_mb: { start: heap0, end: heapMb(), committed_end: heapMb("totalJSHeapSize") },
          ...(drive.detail ? { drive: drive.detail() } : {}),
        };
      });
    };

    // Pan: middle button held, one move per frame, back and forth; every move shifts the view.
    const pan: Driver = (i, m) => {
      const c = centre();
      if (i < 0) {
        this.g.upAt(c.x, c.y, 1);
        return;
      }
      if (i === 0) this.g.downAt(c.x, c.y, 1);
      const k = i % 120;
      const dx = (k < 60 ? k : 120 - k) * 3;
      const before = this.g.at([0, 0]);
      this.g.moveTo(c.x + dx, c.y + dx / 3);
      const after = this.g.at([0, 0]);
      if (after.x !== before.x || after.y !== before.y) m.input();
    };
    this.g.editor().fit();
    await frames(3);
    await phase("pan-vue-ajustee", pan);
    this.g.editor().zoomTo(1);
    await frames(3);
    await phase("pan-zoom-1", pan);
    // Wheel: one notch per frame around the centre, twelve in, twelve out; a notch past the end of the ladder
    // changes nothing and is not an input to draw.
    await phase("zoom-molette", (i, m) => {
      if (i < 0) return;
      const c = centre();
      const z = s.view.zoomPercent;
      this.g.wheel(c.x, c.y, Math.floor(i / 12) % 2 === 0 ? -100 : 100);
      if (s.view.zoomPercent !== z) m.input();
    });
    this.g.editor().fit();
    await frames(3);

    const ecu = [...s.components.values()].find((c) => c.kind === "ecu_stm32");
    if (!ecu) throw new Failure("aucune ECU dans perf-150-300");
    await this.useTool("select");
    s.setSelection([], []);
    await frames(2);
    await phase("glisser-ecu", this.dragDriver(await this.dragPlan([ecu.id])), true);
    const twenty = [...s.components.values()].filter((c) => c.kind !== "junction" && c.kind !== "ground").slice(0, 20).map((c) => c.id);
    s.setSelection(twenty, []);
    await frames(2);
    await phase("glisser-selection-20", this.dragDriver(await this.dragPlan(twenty)), true);
    s.setSelection([], []);

    // RUN: the slider of a potentiometer dragged, a switch toggled every 500 ms (the slider released meanwhile).
    const pot = [...s.components.values()].find((c) => c.kind === "potentiometer");
    const sw = [...s.components.values()].find((c) => c.kind === "switch");
    if (!pot || !sw) throw new Failure("perf-150-300 sans potentiomètre ou sans interrupteur");
    const potY = zoneCentre(this.def(pot.id), "drag_pot")[1];
    // Selection dragging may have moved these parts beyond the fitted view. Frame their real input range.
    await this.ensureVisible([
      toWorld(this.comp(sw.id), zoneCentre(this.def(sw.id), "toggle_switch")),
      toWorld(this.comp(pot.id), [-2 + 4 * 0.1, potY]),
      toWorld(this.comp(pot.id), [-2 + 4 * 0.9, potY]),
    ]);
    await this.clickButton("#phys-run");
    await this.until("RUN", () => s.run === "run", 2000);
    const slider = (alpha: number) => this.g.at(toWorld(this.comp(pot.id), [-2 + 4 * alpha, potY]));
    let sliding = false;
    let alpha = 0.5;
    await phase("run-curseur-interrupteur", (i, m) => {
      const k = i < 0 ? 0 : i % 30;
      if (k === 0) {
        if (sliding) {
          const p = slider(alpha);
          this.g.upAt(p.x, p.y);
          sliding = false;
        }
        if (i < 0) return;
        const lame = this.g.at(toWorld(this.comp(sw.id), zoneCentre(this.def(sw.id), "toggle_switch")));
        this.g.moveTo(lame.x, lame.y);
        this.g.downAt(lame.x, lame.y);
        m.input();
        this.g.upAt(lame.x, lame.y);
        return;
      }
      alpha = 0.2 + 0.6 * (k / 29);
      const p = slider(alpha);
      if (!sliding) {
        this.g.moveTo(p.x, p.y);
        this.g.downAt(p.x, p.y);
        sliding = true;
      } else this.g.moveTo(p.x, p.y);
      m.input();
    }, false, true);
    freezes.disconnect();
    // FrameLog records every frame for the short timing phases. It must not retain two hours of test
    // instrumentation: the soak records counters and one bounded checkpoint per minute instead.
    rec.stop();
    if (this.cfg.seconds > 0) {
      await this.step("edition-churn", async (check) => {
        // Exercise production document transitions before the sustained RUN. Temporary parts never
        // replace the representative circuit, and no instrumentation retains deleted components.
        const sinceStop = this.mark();
        await this.clickButton("#phys-stop");
        await this.until("STOP pour le churn édition", () => s.run === "stop", 2000);
        await this.fresh(sinceStop);
        const original = circuitText(s);
        const originalComponents = s.components.size;
        const originalWires = s.wires.size;
        const restarts = s.values?.engine.restarts ?? 0;
        const channels0 = this.view.controller.retainedPotChannels;
        let channelsPeak = channels0;
        const pos: Pt = [Math.max(...[...s.components.values()].map((c) => c.pos[0])) + 20,
          Math.min(...[...s.components.values()].map((c) => c.pos[1]))];
        let cycles = 0;
        let labels = 0;
        let temporary: CompId | null = null;
        try {
          for (let i = 0; i < 100; i++) {
            let since = this.mark();
            const added = await this.view.controller.edit({ op: "add_component", kind: "potentiometer", pos, rot: 0 });
            if (!added.ok) throw new Failure(`churn add : ${JSON.stringify(added.error)}`);
            temporary = added.reply.patch.components.find(([, before, after]) => before === null && after?.kind === "potentiometer")?.[0] ?? null;
            if (temporary === null) throw new Failure("potentiomètre temporaire absent du patch churn");
            await this.fresh(since);
            this.g.editor().fit();
            await frames(2);
            for (let rename = 0; rename < 2; rename++) {
              since = this.mark();
              const labelled = await this.view.controller.edit({ op: "set_label", comp: temporary, label: `SC_${i}_${rename}` });
              if (!labelled.ok) throw new Failure(`churn label : ${JSON.stringify(labelled.error)}`);
              await this.fresh(since);
              await frames(2); // Rebuild/draw the actual label geometry and text-width cache.
              labels++;
            }
            since = this.mark();
            const position = i % 2 === 0 ? 0.2 : 0.8;
            this.view.controller.interact({ op: "set_pot", comp: temporary, position });
            await this.until("set_pot temporaire appliqué", () => s.states.get(temporary!)?.position === position, 2000);
            await this.fresh(since);
            channelsPeak = Math.max(channelsPeak, this.view.controller.retainedPotChannels);
            since = this.mark();
            const deleted = await this.view.controller.edit({ op: "delete", components: [temporary], wires: [] });
            if (!deleted.ok) throw new Failure(`churn delete : ${JSON.stringify(deleted.error)}`);
            await this.fresh(since);
            if (s.components.has(temporary) || s.states.has(temporary) || s.compValues.has(temporary)
                || this.view.controller.retainedPotChannels !== channels0) {
              throw new Failure("composant, état, valeurs ou channel temporaire retenus après suppression");
            }
            temporary = null;
            cycles++;
          }
        } finally {
          if (temporary !== null && s.components.has(temporary)) {
            await this.view.controller.edit({ op: "delete", components: [temporary], wires: [] });
          }
        }
        const restored = circuitText(s) === original;
        check("churn complet", cycles === 100 && labels === 200, "100 créations/set_pot/suppressions, 200 labels", `${cycles}, ${labels}`);
        check("topologie et états originaux restaurés", restored, "document original identique", String(restored));
        check("aucun redémarrage moteur pendant churn", s.values?.engine.restarts === restarts, String(restarts), String(s.values?.engine.restarts));
        check("channels temporaires libérés", this.view.controller.retainedPotChannels === channels0,
          String(channels0), String(this.view.controller.retainedPotChannels));
        if (!restored) throw new Failure("document original différent après le churn édition");
        this.g.editor().fit();
        await frames(3);
        // In STOP with settled topology, fitting the same bounds must keep the camera and retained scene.
        const fitFullBefore = this.g.editor().metrics.fullRepaints;
        const fitOriginBefore = this.g.at([0, 0]);
        const fitZoomBefore = s.view.zoomPercent;
        for (let repeat = 0; repeat < 3; repeat++) {
          this.g.editor().fit();
          await frames(2);
        }
        const fitFullAfter = this.g.editor().metrics.fullRepaints;
        const fitOriginAfter = this.g.at([0, 0]);
        const fitZoomAfter = s.view.zoomPercent;
        const fitRetained = fitFullAfter === fitFullBefore;
        const fitUnchanged = fitOriginAfter.x === fitOriginBefore.x && fitOriginAfter.y === fitOriginBefore.y
          && fitZoomAfter === fitZoomBefore;
        check("fit répété sans repaint complet", fitRetained, "0 repaint complet supplémentaire", String(fitFullAfter - fitFullBefore));
        check("fit répété sans déplacement ni zoom", fitUnchanged, "caméra inchangée", String(fitUnchanged));
        if (!fitRetained || !fitUnchanged) throw new Failure("fit répété invalide le rendu ou déplace la caméra en STOP");
        await this.clickButton("#phys-run");
        await this.until("RUN après churn édition", () => s.run === "run", 2000);
        await this.fresh();
        return { cycles, created: cycles, label_updates: labels, pot_interactions: cycles, deleted: cycles,
          deleted_state_checks: cycles, topology_restored: restored, original_components: originalComponents,
          original_wires: originalWires, engine_restarts: (s.values?.engine.restarts ?? 0) - restarts,
          retained_pot_channels_before: channels0, retained_pot_channels_after: this.view.controller.retainedPotChannels,
          retained_pot_channels_peak: channelsPeak,
          repeated_fit: { repeats: 3, run: "stop", full_before: fitFullBefore, full_after: fitFullAfter,
            additional_full: fitFullAfter - fitFullBefore, zoom_before: fitZoomBefore, zoom_after: fitZoomAfter,
            origin_before: fitOriginBefore, origin_after: fitOriginAfter },
          scope: "production Controller edit/interact and real redraws; STOP transitions before sustained RUN" };
      });
      await this.step("actionneurs-canvas-etroit", async (check) => {
        const host = $("#phys-canvas");
        const width = host.style.getPropertyValue("max-width");
        const priority = host.style.getPropertyPriority("max-width");
        const original = circuitText(s, false);
        const components = s.components.size, wires = s.wires.size;
        const restarts = s.values?.engine.restarts ?? 0;
        const beforeClosed = s.states.get(sw.id)?.closed === true;
        const beforePot = Number(s.states.get(pot.id)?.position ?? 0.5);
        const target = beforePot < 0.5 ? 0.8 : 0.2;
        try {
          await this.clickButton('[data-tab="phys-journal"]');
          host.style.setProperty("max-width", "346px");
          await frames(3);
          this.g.editor().fit();
          await frames(3);
          const narrow = canvasGeometry();
          const since = this.mark();
          await this.actuate(pot.id, sw.id, target);
          await this.fresh(since);
          check("canvas étroit réel", host.getBoundingClientRect().width <= 346.5, "≤ 346,5 px",
            String(host.getBoundingClientRect().width));
          check("interrupteur actionné dans le canvas étroit", (s.states.get(sw.id)?.closed === true) !== beforeClosed,
            String(!beforeClosed), String(s.states.get(sw.id)?.closed === true));
          check("potentiomètre actionné dans le canvas étroit", Math.abs(Number(s.states.get(pot.id)?.position) - target) < 0.02,
            String(target), String(s.states.get(pot.id)?.position));
          check("charge et document conservés", circuitText(s, false) === original && s.components.size === components && s.wires.size === wires,
            "document et topologie inchangés", String(circuitText(s, false) === original));
          check("RUN sans redémarrage moteur", s.run === "run" && s.values?.engine.restarts === restarts,
            "RUN, même moteur", String(s.run === "run" && s.values?.engine.restarts === restarts));
          check("journal visible pendant la régression", $('[data-tab="phys-journal"]').getAttribute("aria-selected") === "true"
            && !$("#phys-journal").hidden, "sélectionné et visible", String(!$("#phys-journal").hidden));
          return { canvas: narrow, components, wires, gestures: 2, scope: "real middle-pan, switch click and potentiometer drag" };
        } finally {
          try {
            this.g.cancel();
          } finally {
            if (width) host.style.setProperty("max-width", width, priority);
            else host.style.removeProperty("max-width");
            await frames(3);
            this.g.editor().fit();
            await frames(3);
          }
        }
      });
      await this.step("run-soak", async (check) => {
        const durationMs = this.cfg.seconds * 1000;
        let values = 0;
        let cycles = 0;
        let lagMax = 0;
        let journalIncomplete = false;
        let nextCheckpoint = 0;
        const checkpointLimit = Math.ceil(this.cfg.seconds / 60) + 2;
        const checkpoints: { elapsed_s: number; js_heap_mib: number | null; js_heap_committed_mib: number | null;
          canvas: ReturnType<typeof canvasGeometry>; simulated_s: number; values: number; cycles: number }[] = [];
        let t0: number | null = null;
        let sim0: number | null = null;
        let restarts0: number | null = null;
        const soak = {
          completed: false, failure: null as string | null, requested_seconds: this.cfg.seconds,
          elapsed_seconds: 0, simulated_seconds: 0, values: 0, cycles: 0, actuator_gestures: 0, lag_frames_max: 0,
          engine_restarts: null as number | null, heap_start_mib: null as number | null, heap_end_mib: null as number | null,
          heap_committed_start_mib: null as number | null, heap_committed_end_mib: null as number | null,
          canvas_geometry_start: null as ReturnType<typeof canvasGeometry> | null,
          canvas_geometry_end: null as ReturnType<typeof canvasGeometry> | null, checkpoints,
          checkpoint_limit: checkpointLimit, frame_recording: "disabled during soak",
          journal: "production journal requested visible and active; final checks record actual state", journal_incomplete: false,
        };
        // Keep bounded partial evidence even when a gesture, setup or final check fails.
        this.report.soak = soak;
        const recordJournalIntegrity = () => {
          if (s.values?.doc_id === s.docId && s.values.diagnostics.some((d) => d.code === "journal_incomplete")) journalIncomplete = true;
        };
        const snapshot = (checkpoint = false) => {
          recordJournalIntegrity();
          const elapsedMs = t0 === null ? 0 : performance.now() - t0;
          const simulatedS = sim0 === null ? 0 : (s.simTimeNs - sim0) / 1e9;
          Object.assign(soak, {
            elapsed_seconds: elapsedMs / 1000, simulated_seconds: simulatedS, values, cycles,
            actuator_gestures: cycles * 2, lag_frames_max: lagMax,
            engine_restarts: restarts0 === null ? null : (s.values?.engine.restarts ?? 0) - restarts0,
            heap_end_mib: heapMb(), heap_committed_end_mib: heapMb("totalJSHeapSize"),
            canvas_geometry_end: canvasGeometry(), journal_incomplete: journalIncomplete,
          });
          if (checkpoint && elapsedMs >= nextCheckpoint && checkpoints.length < checkpointLimit) {
            checkpoints.push({ elapsed_s: Math.round(elapsedMs / 1000), js_heap_mib: soak.heap_end_mib,
              js_heap_committed_mib: soak.heap_committed_end_mib, canvas: canvasGeometry(),
              simulated_s: Math.round(simulatedS), values, cycles });
            nextCheckpoint = Math.floor(elapsedMs / 60000 + 1) * 60000;
          }
        };
        const offValues = s.onValues.on((v) => {
          values++; lagMax = Math.max(lagMax, v.engine.lag_frames); recordJournalIntegrity();
        });
        let navigation: ReturnType<typeof isolateSoakNavigation> | null = null;
        try {
          await this.clickButton('[data-tab="phys-journal"]');
          await frames(3);
          this.g.editor().fit();
          await frames(3);
          await this.fresh();
          recordJournalIntegrity();
          const firstJournal = await phys.journal(1);
          soak.heap_start_mib = heapMb();
          soak.heap_committed_start_mib = heapMb("totalJSHeapSize");
          soak.canvas_geometry_start = canvasGeometry();
          t0 = performance.now();
          sim0 = s.simTimeNs;
          restarts0 = s.values?.engine.restarts ?? 0;
          values = 0;
          lagMax = 0;
          navigation = isolateSoakNavigation();
          this.report.soak_navigation = { scope: "view navigation isolated only during sustained RUN; restored in finally", events: navigation.events };
          while (performance.now() - t0 < durationMs) {
            if (s.run !== "run") throw new Failure("RUN interrompu pendant le soak");
            alpha = 0.2 + 0.6 * ((cycles % 16) / 15);
            await this.actuate(pot.id, sw.id, alpha);
            cycles++;
            snapshot(true);
            await sleep(Math.min(500, Math.max(0, durationMs - (performance.now() - t0))));
          }
          await this.fresh();
          const elapsedMs = performance.now() - t0;
          const simulatedS = (s.simTimeNs - sim0) / 1e9;
          let checksOk = true;
          const finalCheck: CheckFn = (name, ok, expected, observed) => {
            checksOk &&= ok;
            check(name, ok, expected, observed);
          };
          finalCheck("durée RUN complète", elapsedMs >= durationMs && s.run === "run", `${this.cfg.seconds} s RUN`, `${Math.round(elapsedMs / 1000)} s`);
          finalCheck("valeurs actives", values >= this.cfg.seconds * 5, `≥ ${this.cfg.seconds * 5}`, String(values));
          finalCheck("simulation active", simulatedS >= this.cfg.seconds / 2, `≥ ${this.cfg.seconds / 2} s`, String(Math.round(simulatedS)));
          finalCheck("aucun redémarrage moteur", s.values?.engine.restarts === restarts0, String(restarts0), String(s.values?.engine.restarts));
          finalCheck("journal affiché", Number($("#phys-count-journal").textContent) > 0, "> 0 lignes", $("#phys-count-journal").textContent ?? "");
          const lastJournal = await phys.journal(1);
          finalCheck("journal actif", lastJournal.length > 0 && lastJournal[0].frame > (firstJournal[0]?.frame ?? -1),
            "trame finale > initiale", `${firstJournal[0]?.frame ?? -1} → ${lastJournal[0]?.frame ?? -1}`);
          finalCheck("onglet journal actif", $('[data-tab="phys-journal"]').getAttribute("aria-selected") === "true" && !$("#phys-journal").hidden,
            "sélectionné et visible", String(!$("#phys-journal").hidden));
          recordJournalIntegrity();
          finalCheck("journal sans perte", s.values?.doc_id === s.docId && !journalIncomplete,
            "aucun diagnostic journal_incomplete pendant RUN", String(journalIncomplete));
          soak.completed = checksOk;
          if (!checksOk) soak.failure = "critères RUN non validés";
          return soak;
        } catch (err) {
          soak.failure = String(err);
          throw err;
        } finally {
          try {
            snapshot(true);
          } finally {
            navigation?.restore();
            offValues();
            this.g.cancel();
          }
        }
      });
    }
    await this.clickButton("#phys-stop");
    await memGate("memoire-fin", "fin");
    this.report.budgets_ok = Object.values(budgets).every((b) => b.ok);
    this.report.T_ms = T;
  }

  /**
   * Where to take the parts and how far to move them, checked before the phase so that every release commits: an
   * integer point deep in the body of one of the parts (pointing there finds that body, not a pin, a wire or an
   * actuator, here and at the destination), and a delta the core's dry run accepts.
   */
  private async dragPlan(ids: CompId[]): Promise<DragPlan> {
    const s = this.state;
    const o = this.g.at([0, 0]);
    const uCss = this.g.at([1, 0]).x - o.x;
    if (!(uCss > 0)) throw new Failure(`repère du schéma inattendu (${uCss} px par unité)`);
    // The editor's pointing tolerance, plus the error of a pointer on whole CSS pixels (half a pixel).
    const tol = HIT_PX / uCss;
    const reach = tol + 0.5 / uCss;
    const model = new SceneModel((k) => s.library.get(k), (t) => t.length * 6.6);
    model.rebuild(s.components.values(), s.wires.values());
    const moved = new Set(ids);
    // At the destination only the moved parts, their pins and their own wires (rerouted with them) may be pointed.
    const ownWire = (id: WireId) => {
      const x = s.wires.get(id);
      return x !== undefined && (moved.has(x.a.comp) || moved.has(x.b.comp));
    };
    const free = (h: Hit | null) => h === null || ((h.t === "body" || h.t === "pin" || h.t === "actuator") && moved.has(h.comp)) ||
      (h.t === "wire" && ownWire(h.wire));
    const clear = (p: Pt, id: CompId): boolean => {
      const h = hitTest(model, p, reach);
      return h?.t === "body" && h.comp === id;
    };
    // Larger bodies first: their centre is far from every lead and wire.
    const margin = (id: CompId) => {
      const b = this.def(id).body;
      return Math.min(b.x1 - b.x0, b.y1 - b.y0);
    };
    const order = [...ids].filter((id) => this.comp(id).kind !== "junction").sort((a, b) => margin(b) - margin(a) || a - b);
    // The pointer stays on the canvas, a few pixels inside its edges, at the start and at the destination.
    const view = $("#phys-canvas").getBoundingClientRect();
    const inView = (p: Pt) => {
      const c = this.g.at(p);
      return c.x >= view.left + 8 && c.x <= view.right - 8 && c.y >= view.top + 8 && c.y <= view.bottom - 8;
    };
    const grabs: { id: CompId; local: Pt }[] = [];
    for (const id of order) {
      const b = this.def(id).body;
      const cx = (b.x0 + b.x1) / 2;
      const cy = (b.y0 + b.y1) / 2;
      const pts: Pt[] = [];
      // Whole points at least half a unit inside the body: a pointer off by its rounding still finds the body.
      for (let x = Math.ceil(b.x0 + 0.5); x <= Math.floor(b.x1 - 0.5); x++) {
        for (let y = Math.ceil(b.y0 + 0.5); y <= Math.floor(b.y1 - 0.5); y++) pts.push([x, y]);
      }
      pts.sort((p, q) => Math.hypot(p[0] - cx, p[1] - cy) - Math.hypot(q[0] - cx, q[1] - cy) || p[0] - q[0] || p[1] - q[1]);
      const local = pts.find((p) => {
        const w = toWorld(this.comp(id), p);
        return inView(w) && clear(w, id);
      });
      if (local) grabs.push({ id, local });
    }
    if (!grabs.length) throw new Failure(`aucun point de prise sur le corps des composants déplacés (tolérance ${tol.toFixed(2)} unité)`);
    const tried: string[] = [];
    // The dry run depends on the delta only (the same parts move whatever the grab).
    const accepted = new Map<string, boolean>();
    for (const grab of grabs) {
      const w = toWorld(this.comp(grab.id), grab.local);
      for (const mag of [6, 8, 5, 10, 4, 12]) {
        for (const delta of [[mag, 0], [-mag, 0], [0, mag], [0, -mag]] as Pt[]) {
          const to: Pt = [w[0] + delta[0], w[1] + delta[1]];
          const at = `#${grab.id} ${delta.join(",")}`;
          if (!inView(to)) {
            tried.push(`${at} : hors de la vue`);
            continue;
          }
          const dest = hitTest(model, to, reach);
          if (!free(dest)) {
            tried.push(`${at} : ${dest?.t}`);
            continue;
          }
          const key = delta.join(",");
          let ok = accepted.get(key);
          if (ok === undefined) {
            const edit: Edit = { op: "move", components: [...ids].sort((a, b) => a - b), wires: [], delta };
            ok = await phys.preview(s.rev, edit).then(() => true, (e) => {
              tried.push(`${at} : ${JSON.stringify(e).slice(0, 80)}`);
              return false;
            });
            accepted.set(key, ok);
          }
          if (ok) return { ids, grab: grab.id, local: grab.local, delta, uCss, tried };
        }
      }
    }
    throw new Failure(`aucun déplacement accepté par l'essai à blanc : ${tried.slice(0, 6).join(" ; ")}`);
  }

  /**
   * Drag cycles of 60 frames: press on the planned point, 58 moves of one grid unit each (the ghost changes at
   * every move, between 2 units and the planned distance), release at the planned delta: the move is committed;
   * the next cycle takes the parts where they landed and brings them back. Each release must commit.
   */
  private dragDriver(plan: DragPlan): Driver {
    const s = this.state;
    const M = Math.abs(plan.delta[0]) + Math.abs(plan.delta[1]);
    const e: Pt = [plan.delta[0] / M, plan.delta[1] / M];
    const period = 2 * (M - 2);
    /** Distance at move k (1…MOVES), a triangle wave between 2 and M that ends at M. */
    const wave = (k: number) => {
      const r = (MOVES - k) % period;
      return M - (r <= M - 2 ? r : period - r);
    };
    const report = { releases: 0, committed: 0, failures: [] as string[] };
    let sign = 1;
    let cycle: { start: Pt; base: Pt; k: number; n: number } | null = null;
    const client = (base: Pt, n: number) => {
      const p = this.g.at([base[0] + sign * e[0] * n, base[1] + sign * e[1] * n]);
      return { x: Math.round(p.x), y: Math.round(p.y) };
    };
    const release = (c: NonNullable<typeof cycle>, m: Marks): Promise<void> => {
      const p = client(c.base, c.n);
      const rev = s.rev;
      const want: Pt = [c.start[0] + sign * plan.delta[0], c.start[1] + sign * plan.delta[1]];
      this.g.upAt(p.x, p.y);
      m.edit();
      report.releases++;
      cycle = null;
      sign = -sign;
      const landed = () => {
        const pos = this.comp(plan.grab).pos;
        return pos[0] === want[0] && pos[1] === want[1];
      };
      return this.until("déplacement validé", () => s.rev > rev && landed(), 1500).then(() => {
        report.committed++;
      }, () => {
        if (report.failures.length >= 4) return;
        const pos = this.comp(plan.grab).pos;
        report.failures.push(`relâcher ${report.releases} : révision ${s.rev > rev ? "changée" : "inchangée"}, #${plan.grab} en ` +
          `(${pos.join(", ")}) au lieu de (${want.join(", ")}), ${s.selection.components.size} sélectionné(s), « ${s.message?.text ?? ""} »`);
      });
    };
    const drive: Driver = (i, m) => {
      if (i >= 0 && cycle === null) {
        const start = [...this.comp(plan.grab).pos] as Pt;
        const base = toWorld(this.comp(plan.grab), plan.local);
        cycle = { start, base, k: 0, n: 0 };
        const p = client(base, 0);
        this.g.moveTo(p.x, p.y);
        this.g.downAt(p.x, p.y);
        return;
      }
      const c = cycle;
      if (c === null) return;
      if (i < 0 || c.k === MOVES) {
        if (c.n !== M) {
          c.n = M;
          const p = client(c.base, M);
          this.g.moveTo(p.x, p.y);
        }
        return release(c, m);
      }
      c.k++;
      const n = wave(c.k);
      const changed = n !== c.n;
      c.n = n;
      const p = client(c.base, n);
      this.g.moveTo(p.x, p.y);
      if (changed) m.input();
      return;
    };
    drive.verdict = (check) => {
      check("chaque relâcher valide le déplacement prévu", report.releases > 0 && report.committed === report.releases,
        `${report.releases} déplacement(s) de ${plan.delta.join(", ")} validé(s)`,
        `${report.committed}/${report.releases}${report.failures.length ? ` — ${report.failures.join(" ; ")}` : ""}`);
    };
    drive.detail = () => ({
      grab: plan.grab, local: plan.local, delta: plan.delta, u_css: +plan.uCss.toFixed(3), parts: plan.ids.length,
      releases: report.releases, committed: report.committed, refused_plans: plan.tried.slice(0, 6),
    });
    return drive;
  }
}

interface Marks {
  /** The input just dispatched changes what is drawn (input_to_draw). */
  input(): void;
  /** The release just dispatched sends an edit (edit_to_values). */
  edit(): void;
}

/** One input per frame; i = -1 at the end (release what is held). */
interface Driver {
  (i: number, marks: Marks): void | Promise<void>;
  /** Checks of the phase (e.g. every release committed). */
  verdict?: (check: CheckFn) => void;
  /** Extra detail for the report. */
  detail?: () => Record<string, unknown>;
}

interface DragPlan {
  ids: CompId[];
  grab: CompId;
  local: Pt;
  delta: Pt;
  uCss: number;
  tried: string[];
}

const PHASE_MS = 10000;
/** Moves per drag cycle (then the release: 60 frames). */
const MOVES = 58;

const round = (s: Summary) => ({ count: s.count, p50: +s.p50.toFixed(2), p95: +s.p95.toFixed(2), p99: +s.p99.toFixed(2), max: +s.max.toFixed(2) });

/** JS heap estimate exposed by the page (Chromium), MiB. This is not process private memory. */
function heapMb(field: "usedJSHeapSize" | "totalJSHeapSize" = "usedJSHeapSize"): number | null {
  const m = (performance as unknown as { memory?: Record<string, number> }).memory;
  return m && Number.isFinite(m[field]) && m[field] >= 0 ? Math.round((m[field] / 1048576) * 10) / 10 : null;
}

/** Bounded scalar measurements, with an RGBA8 size estimate only (GPU copies/driver allocations are opaque). */
function canvasGeometry(): { viewport_css: [number, number]; host_css: [number, number]; dpr: number;
  surfaces: { backing_px: [number, number]; css: [number, number] }[]; rgba8_backing_bytes_estimate: number } | null {
  const host = $("#phys-canvas");
  const rect = host.getBoundingClientRect();
  const surfaces = Array.from(host.querySelectorAll("canvas")).slice(0, 4).map((canvas) => {
    const box = canvas.getBoundingClientRect();
    return { backing_px: [canvas.width, canvas.height] as [number, number], css: [box.width, box.height] as [number, number] };
  });
  const scalars = [innerWidth, innerHeight, rect.width, rect.height, devicePixelRatio,
    ...surfaces.flatMap((surface) => [...surface.backing_px, ...surface.css])];
  if (!surfaces.length || !scalars.every((value) => Number.isFinite(value) && value > 0)) return null;
  return { viewport_css: [innerWidth, innerHeight], host_css: [rect.width, rect.height], dpr: devicePixelRatio,
    surfaces, rgba8_backing_bytes_estimate: surfaces.reduce((sum, surface) => sum + surface.backing_px[0] * surface.backing_px[1] * 4, 0) };
}

/** Frames of the view, through PhysView.frameObserver. */
class FrameLog {
  private records: FrameRecord[] = [];
  private waiters: { pred: (f: FrameRecord) => boolean; resolve: (f: FrameRecord) => void }[] = [];

  constructor(private readonly view: PhysView) {
    view.frameObserver = (f) => {
      this.records.push(f);
      this.waiters = this.waiters.filter((w) => {
        if (!w.pred(f)) return true;
        w.resolve(f);
        return false;
      });
    };
  }

  start(): void {
    this.records = [];
  }

  stop(): void {
    this.view.frameObserver = null;
    this.records = [];
    this.waiters = [];
  }

  /** The first frame recorded since start() that matches, already seen or to come. */
  first(pred: (f: FrameRecord) => boolean, timeoutMs: number): Promise<FrameRecord> {
    const seen = this.records.find(pred);
    if (seen) return Promise.resolve(seen);
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Failure(`aucune image dessinée en ${timeoutMs} ms`)), timeoutMs);
      this.waiters.push({ pred, resolve: (f) => {
        clearTimeout(timer);
        resolve(f);
      } });
    });
  }

  gaps(): number[] {
    const out: number[] = [];
    for (let i = 1; i < this.records.length; i++) out.push(this.records[i].now - this.records[i - 1].now);
    return out;
  }

  /** Gaps over 50 ms: when (ms after t0), how long, and the view's own time in the frames around. */
  longGaps(t0: number): { at_ms: number; gap_ms: number; render_before_ms: number; render_after_ms: number; drew_after: boolean }[] {
    const out = [];
    for (let i = 1; i < this.records.length && out.length < 5; i++) {
      const a = this.records[i - 1];
      const b = this.records[i];
      if (b.now - a.now <= 50) continue;
      out.push({
        at_ms: Math.round(a.now - t0), gap_ms: +(b.now - a.now).toFixed(1), render_before_ms: +(a.end - a.start).toFixed(1),
        render_after_ms: +(b.end - b.start).toFixed(1), drew_after: b.drew,
      });
    }
    return out;
  }

  renderMs(): number[] {
    return this.records.map((r) => r.end - r.start);
  }

  /** Each input that changes the drawing → end of the first frame that drew after it. */
  inputToDraw(inputs: number[]): number[] {
    const out: number[] = [];
    let j = 0;
    for (const t of inputs) {
      while (j < this.records.length && !(this.records[j].drew && this.records[j].start >= t)) j++;
      if (j < this.records.length) out.push(this.records[j].end - t);
    }
    return out;
  }

  /**
   * Each release (the edit is sent at once) → end of the first frame drawing values of a revision at least that of
   * the edit. The edit's revision is the first one mirrored after the release; a release that changed nothing
   * (refused or empty move) has none and is not counted.
   */
  editToValues(times: number[]): number[] {
    const out: number[] = [];
    for (const t of times) {
      let r0 = -1;
      for (const r of this.records) if (r.start < t) r0 = r.rev;
      const after = this.records.filter((r) => r.start >= t);
      const editRev = after.find((r) => r.rev > r0)?.rev;
      if (editRev === undefined) continue;
      const shown = after.find((r) => r.valuesRev >= editRev);
      if (shown) out.push(shown.end - t);
    }
    return out;
  }
}

/** A long animation frame as the report shows it: timing and script attribution. */
interface LongFrame {
  at_ms: number;
  duration_ms: number;
  blocking_ms?: number;
  render_start_ms?: number;
  style_layout_start_ms?: number;
  scripts?: { invoker: string; type: string; source: string; fn: string; char: number; ms: number; layout_ms: number }[];
}

interface LoafEntry extends PerformanceEntry {
  blockingDuration?: number;
  renderStart?: number;
  styleAndLayoutStart?: number;
  scripts?: {
    invoker: string; invokerType: string; sourceURL: string; sourceFunctionName: string; sourceCharPosition: number;
    startTime: number; duration: number; forcedStyleAndLayoutDuration: number;
  }[];
}

const rel = (t: number | undefined, t0: number) => (t ? +(t - t0).toFixed(1) : undefined);

/** Freezes: long-animation-frame entries over 50 ms, else long tasks, else animation frame gaps over 50 ms. */
class Freezes {
  readonly source: "long-animation-frame" | "longtask" | "raf";
  /** The first long frames of the phase, with their attribution. */
  readonly long: LongFrame[] = [];
  private entries = 0;
  private t0 = 0;
  private readonly observer: PerformanceObserver | null = null;

  constructor() {
    const types = PerformanceObserver.supportedEntryTypes ?? [];
    this.source = types.includes("long-animation-frame") ? "long-animation-frame" : types.includes("longtask") ? "longtask" : "raf";
    if (this.source !== "raf") {
      this.observer = new PerformanceObserver((list) => {
        for (const e of list.getEntries() as LoafEntry[]) {
          // Entries are delivered after their frame: one that started before the phase is not the phase's.
          if (e.duration <= 50 || e.startTime < this.t0) continue;
          this.entries++;
          if (this.long.length < 5) this.long.push(longFrame(e, this.t0));
        }
      });
      this.observer.observe({ type: this.source, buffered: false });
    }
  }

  reset(): void {
    this.entries = 0;
    this.long.length = 0;
    this.t0 = performance.now();
  }

  count(gaps: number[]): number {
    return this.source === "raf" ? gaps.filter((g) => g > 50).length : this.entries;
  }

  disconnect(): void {
    this.observer?.disconnect();
  }
}

function longFrame(e: LoafEntry, t0: number): LongFrame {
  return {
    at_ms: Math.round(e.startTime - t0), duration_ms: +e.duration.toFixed(1),
    blocking_ms: e.blockingDuration !== undefined ? +e.blockingDuration.toFixed(1) : undefined,
    render_start_ms: rel(e.renderStart, e.startTime), style_layout_start_ms: rel(e.styleAndLayoutStart, e.startTime),
    scripts: e.scripts?.slice(0, 6).map((x) => ({
      invoker: x.invoker, type: x.invokerType, source: x.sourceURL.split("/").pop() ?? "", fn: x.sourceFunctionName,
      char: x.sourceCharPosition, ms: +x.duration.toFixed(1), layout_ms: +x.forcedStyleAndLayoutDuration.toFixed(1),
    })),
  };
}

/** τ by least squares on ln|V − V∞|, V∞ the last sample, over the part of the transient between 3 % and 95 %. */
function fitTau(samples: { t: number; x: number }[]): { tau_ms: number | null; samples: number; final: number | null } {
  if (samples.length < 4) return { tau_ms: null, samples: samples.length, final: null };
  const final = samples[samples.length - 1].x;
  const r0 = Math.max(...samples.map((s) => Math.abs(s.x - final)));
  const pts = samples.filter((s) => {
    const r = Math.abs(s.x - final);
    return r <= 0.95 * r0 && r >= 0.03 * r0;
  }).map((s) => ({ t: s.t / 1e6, l: Math.log(Math.abs(s.x - final)) }));
  if (pts.length < 3) return { tau_ms: null, samples: pts.length, final };
  const n = pts.length;
  const mt = pts.reduce((a, p) => a + p.t, 0) / n;
  const ml = pts.reduce((a, p) => a + p.l, 0) / n;
  let num = 0;
  let den = 0;
  for (const p of pts) {
    num += (p.t - mt) * (p.l - ml);
    den += (p.t - mt) * (p.t - mt);
  }
  const slope = den > 0 ? num / den : 0;
  return { tau_ms: slope < 0 ? -1 / slope : null, samples: n, final };
}

/** Canonical text of the mirrored document (components, wires, persistent states), for the reload check. */
function circuitText(s: LabState, includeState = true): string {
  const sortKeys = (o: Record<string, unknown>) => Object.fromEntries(Object.entries(o).sort(([a], [b]) => a.localeCompare(b)));
  const comps = [...s.components.values()].sort((a, b) => a.id - b.id).map((c) => ({
    id: c.id, kind: c.kind, label: c.label, pos: c.pos, rot: c.rot, params: sortKeys(c.params),
    state: includeState ? sortKeys(Object.fromEntries(Object.entries(s.states.get(c.id) ?? {}).filter(([k]) => k !== "pressed"))) : undefined,
  }));
  const wires = [...s.wires.values()].sort((a, b) => a.id - b.id).map((w) => ({ id: w.id, a: w.a, b: w.b, bends: w.bends }));
  return JSON.stringify({ comps, wires });
}

function firstDifference(a: string, b: string): string {
  let i = 0;
  while (i < a.length && i < b.length && a[i] === b[i]) i++;
  return `différence au caractère ${i} : « ${a.slice(i, i + 60)} » / « ${b.slice(i, i + 60)} »`;
}
