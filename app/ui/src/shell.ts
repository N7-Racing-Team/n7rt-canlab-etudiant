// IDE shell behaviour: show/hide the side and bottom panels, resize them with the splitters, switch the console
// tabs. One Shell per view, scoped to the view's workspace slot (root.querySelectorAll: its tabs, tab panels and
// bottom splitter). The frame around the views is shared (panel toggles, side splitters, the .app variables and
// classes): only the active view's shell drives it.
// Layout is remembered per user and per view (localStorage, best effort): the cockpit under n7rt-can-lab.layout.v1
// as before, the Physical Lab under n7rt-can-lab.layout.v1.phys. Resizing writes one CSS variable on .app, at most
// once per animation frame. A tall tab (the cockpit's « Fiche ») has its own dock height, a share of the workspace
// until the user resizes it; a hidden tab is never selected.
//
// ViewHost: one active view among several (cockpit, Physical Lab); only the active one gets the animation frames.

import { $ } from "./dom";

export type Panel = "left" | "right" | "bottom";
const PANELS: Panel[] = ["left", "bottom", "right"];

export interface Layout {
  left: number;
  right: number;
  /** Height of the bottom dock for the ordinary tabs. */
  bottom: number;
  hidden: string[];
  tab: string;
  /** Height of the dock for a tall tab (ShellOptions.tallTabs), once the user resized it; by tab name. */
  heights?: Record<string, number>;
}

export const COCKPIT_LAYOUT_KEY = "n7rt-can-lab.layout.v1";
export const PHYS_LAYOUT_KEY = "n7rt-can-lab.layout.v1.phys";

const LIMITS = { left: [220, 520], right: [220, 560], bottom: [96, 560] } as const;
/** A tall tab's dock: at least TALL_MIN px, and at least TALL_GAP px of the workspace left above it. */
const TALL_MIN = 240;
const TALL_GAP = 180;

/** The limits of a tall tab's dock in a workspace `ws` px high. */
export function tallLimits(ws: number): [number, number] {
  return [TALL_MIN, Math.max(TALL_MIN, ws - TALL_GAP)];
}

/** Height kept above the cockpit's « Fiche » dock on a short workspace: the session line, the metrics, the head of
 * the frames table and about four of its rows. */
export const FRAMES_ROOM = 212;
/** The default dock height of the cockpit's « Fiche »: 60 % of the workspace; on a short workspace (1366 x 768) more,
 * down to FRAMES_ROOM px left above it, up to 500 px. */
export function ficheShare(ws: number): number {
  return Math.round(Math.max(ws * 0.6, Math.min(ws - FRAMES_ROOM, 500)));
}

/** A tall tab's dock height: the remembered one, else its default share of the workspace, within tallLimits. */
export function tallHeight(stored: number | undefined, share: (ws: number) => number, ws: number): number {
  const [lo, hi] = tallLimits(ws);
  return Math.round(Math.max(lo, Math.min(hi, stored ?? share(ws))));
}

/** The tab to select: `wanted` when it exists and is shown, else `fallback` (a hidden tab is never selected). */
export function shownTab(tabs: { name: string; hidden: boolean }[], wanted: string, fallback: string): string {
  return tabs.some((t) => t.name === wanted && !t.hidden) ? wanted : fallback;
}

/** Cockpit, first run: on a narrow window the results panel starts hidden, so the frames keep room. */
export function cockpitDefaults(width: number): Layout {
  return { left: 296, right: 300, bottom: 210, hidden: width < 1280 ? ["right"] : [], tab: "logs" };
}

/** Physical Lab, first run: the inspector is wide enough for the pinout table, the library for a demonstration's
 * full title in its list (as wide as the cockpit's side panel). */
export function physDefaults(): Layout {
  return { left: 296, right: 380, bottom: 180, hidden: [], tab: "phys-diag" };
}

type LayoutStore = Pick<Storage, "getItem" | "setItem">;

function storage(): LayoutStore | null {
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

/** The remembered layout over the defaults; a field of the wrong type falls back to its default. */
export function readLayout(store: LayoutStore | null, key: string, defaults: Layout): Layout {
  try {
    const raw = store?.getItem(key);
    const p = raw ? (JSON.parse(raw) as Partial<Record<keyof Layout, unknown>>) : null;
    if (!p || typeof p !== "object") return { ...defaults };
    const num = (v: unknown, d: number) => (typeof v === "number" && Number.isFinite(v) ? v : d);
    const h = p.heights;
    const heights = h && typeof h === "object" && !Array.isArray(h) && Object.values(h).every((v) => typeof v === "number" && Number.isFinite(v))
      ? { ...(h as Record<string, number>) }
      : null;
    return {
      left: num(p.left, defaults.left),
      right: num(p.right, defaults.right),
      bottom: num(p.bottom, defaults.bottom),
      hidden: Array.isArray(p.hidden) ? p.hidden.filter((x): x is string => typeof x === "string") : [...defaults.hidden],
      tab: typeof p.tab === "string" ? p.tab : defaults.tab,
      ...(heights ? { heights } : {}),
    };
  } catch {
    return { ...defaults };
  }
}

export function writeLayout(store: LayoutStore | null, key: string, layout: Layout): void {
  try {
    store?.setItem(key, JSON.stringify(layout));
  } catch {
    /* storage unavailable: the layout is simply not remembered */
  }
}

export interface ShellOptions {
  /** The view's workspace slot: its console tabs, tab panels and bottom splitter. */
  root: ParentNode;
  key: string;
  defaults: Layout;
  /** Titles and accessible names of the panel toggles while this view is active. */
  titles: Record<Panel, string>;
  /** Tabs whose dock is taller than the others (the cockpit's « Fiche »): their default height from the workspace
   * height; a resized height is remembered per tab (Layout.heights), the other tabs keep Layout.bottom. */
  tallTabs?: Record<string, (workspace: number) => number>;
}

export class Shell {
  private layout: Layout;
  private readonly app = $("#app");
  private active = false;
  /** Called when the visible console tab changes (the view refreshes its counts). */
  onTab: (tab: string) => void = () => {};

  constructor(private readonly opts: ShellOptions) {
    this.layout = readLayout(storage(), opts.key, opts.defaults);
  }

  mount(): void {
    for (const panel of PANELS) {
      $(`#toggle-${panel}`).addEventListener("click", () => {
        if (this.active) this.toggle(panel);
      });
    }
    for (const tab of this.tabs()) {
      tab.addEventListener("click", () => this.selectTab(tab.dataset.tab ?? ""));
    }
    // The bottom splitter belongs to the view; the side splitters are shared by the views.
    const own = Array.from(this.opts.root.querySelectorAll<HTMLElement>(".splitter[data-split]"));
    const shared = Array.from(document.querySelectorAll<HTMLElement>(".workbench > .splitter[data-split]"));
    for (const sp of [...own, ...shared]) this.splitter(sp, sp.dataset.split as Panel);
    // a tall tab follows the window's height (its default share, and its limits)
    window.addEventListener("resize", () => {
      if (this.active && this.opts.tallTabs?.[this.layout.tab]) this.app.style.setProperty("--h-bottom", `${this.bottomFor()}px`);
    });
    this.selectTab(this.layout.tab);
  }

  get tab(): string {
    return this.layout.tab;
  }

  /** The dock height of `tab`: a tall tab's own (remembered, else its share of the workspace), else Layout.bottom. */
  private bottomFor(tab = this.layout.tab): number {
    const share = this.opts.tallTabs?.[tab];
    if (!share) return this.layout.bottom;
    return tallHeight(this.layout.heights?.[tab], share, (this.opts.root as HTMLElement).clientHeight || 0);
  }

  /** The view became visible: its layout takes over the shared frame. */
  activate(): void {
    this.active = true;
    this.apply();
  }

  deactivate(): void {
    this.active = false;
  }

  private apply(): void {
    const s = this.app.style;
    s.setProperty("--w-left", `${this.layout.left}px`);
    s.setProperty("--w-right", `${this.layout.right}px`);
    s.setProperty("--h-bottom", `${this.bottomFor()}px`);
    for (const p of PANELS) {
      const hidden = this.layout.hidden.includes(p);
      this.app.classList.toggle(`no-${p}`, hidden);
      const btn = $(`#toggle-${p}`);
      btn.setAttribute("aria-pressed", String(!hidden));
      const title = this.opts.titles[p];
      if (btn.title !== title) btn.title = title;
      if (btn.getAttribute("aria-label") !== title) btn.setAttribute("aria-label", title);
    }
  }

  toggle(panel: string): void {
    const h = this.layout.hidden;
    this.layout.hidden = h.includes(panel) ? h.filter((x) => x !== panel) : [...h, panel];
    if (this.active) this.apply();
    writeLayout(storage(), this.opts.key, this.layout);
  }

  /** Selects a console tab; a hidden one (« Fiche » without a progression, even remembered) gives the default tab. */
  selectTab(tab: string): void {
    const tabs = this.tabs();
    tab = shownTab(tabs.map((t) => ({ name: t.dataset.tab ?? "", hidden: t.hidden })), tab, this.opts.defaults.tab);
    this.layout.tab = tab;
    for (const t of tabs) t.setAttribute("aria-selected", String(t.dataset.tab === tab));
    for (const p of Array.from(this.opts.root.querySelectorAll<HTMLElement>(".tab-panel[data-panel]"))) {
      p.hidden = p.dataset.panel !== tab;
    }
    if (this.active) this.app.style.setProperty("--h-bottom", `${this.bottomFor()}px`);
    writeLayout(storage(), this.opts.key, this.layout);
    this.onTab(tab);
  }

  private tabs(): HTMLElement[] {
    return Array.from(this.opts.root.querySelectorAll<HTMLElement>(".tab[data-tab]"));
  }

  private splitter(el: HTMLElement, which: Panel): void {
    el.addEventListener("pointerdown", (down) => {
      if (!this.active) return;
      down.preventDefault();
      el.setPointerCapture(down.pointerId);
      el.classList.add("dragging");
      // a tall tab's dock has its own height and limits; the other tabs share Layout.bottom
      const tall = which === "bottom" && this.opts.tallTabs?.[this.layout.tab] ? this.layout.tab : null;
      const start = { x: down.clientX, y: down.clientY, v: which === "bottom" ? this.bottomFor() : this.layout[which] };
      const [lo, hi] = tall ? tallLimits((this.opts.root as HTMLElement).clientHeight || 0) : LIMITS[which];
      let pending = 0;
      let frame = 0;
      const move = (e: PointerEvent) => {
        const delta = which === "left" ? e.clientX - start.x : which === "right" ? start.x - e.clientX : start.y - e.clientY;
        pending = Math.max(lo, Math.min(hi, start.v + delta));
        if (!frame) {
          frame = requestAnimationFrame(() => {
            frame = 0;
            if (tall) this.layout.heights = { ...(this.layout.heights ?? {}), [tall]: pending };
            else this.layout[which] = pending;
            this.app.style.setProperty(which === "bottom" ? "--h-bottom" : `--w-${which}`, `${pending}px`);
          });
        }
      };
      const up = () => {
        el.classList.remove("dragging");
        el.removeEventListener("pointermove", move);
        el.removeEventListener("pointerup", up);
        el.removeEventListener("pointercancel", up);
        writeLayout(storage(), this.opts.key, this.layout);
      };
      el.addEventListener("pointermove", move);
      el.addEventListener("pointerup", up);
      el.addEventListener("pointercancel", up);
    });
  }
}

// ----------------------------------------------------------------------------------------------- views

export interface HostedView {
  /** Shows the view's slots and resumes its work (rendering, polling). */
  show(): void;
  /** Hides the view's slots and suspends its work; its state is kept. */
  hide(): void;
  /** One animation frame, only while the view is active. */
  frame(now: number): void;
}

/**
 * One active view among several. The first show() hides every other view; later ones hide the active view, then
 * show the new one. A switch requested while switching is ignored. Errors of a view are reported, never left to
 * break the host or the animation loop.
 */
export class ViewHost<N extends string> {
  private current: N | null = null;
  private switching = false;
  private frameErrors = 0;
  /** After each switch. */
  onChange: (active: N) => void = () => {};
  onError: (where: string, err: unknown) => void = (where, err) => console.error(where, err);

  constructor(private readonly views: Readonly<Record<N, HostedView>>) {}

  get active(): N | null {
    return this.current;
  }

  has(name: string): name is N {
    return Object.prototype.hasOwnProperty.call(this.views, name);
  }

  show(name: string): boolean {
    if (!this.has(name) || this.switching || name === this.current) return false;
    this.switching = true;
    try {
      const others = this.current === null ? (Object.keys(this.views) as N[]).filter((n) => n !== name) : [this.current];
      for (const n of others) this.guard(`hide:${n}`, () => this.views[n].hide());
      this.current = name;
      this.frameErrors = 0;
      this.guard(`show:${name}`, () => this.views[name].show());
      this.guard("change", () => this.onChange(name));
    } finally {
      this.switching = false;
    }
    return true;
  }

  /** The animation frame goes to the active view only. */
  frame(now: number): void {
    const name = this.current;
    if (name === null) return;
    try {
      this.views[name].frame(now);
    } catch (err) {
      // Reported a few times only: a broken view must not flood the console at 60 Hz.
      if (this.frameErrors++ < 3) this.onError(`frame:${name}`, err);
    }
  }

  private guard(where: string, fn: () => void): void {
    try {
      fn();
    } catch (err) {
      this.onError(where, err);
    }
  }
}
