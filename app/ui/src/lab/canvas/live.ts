// What the overlay shows from the values (contract §8.5 "Valeurs vivantes", "Marqueurs de diagnostic"): live
// values only for the pins of the selected component and for the hovered net, diagnostic markers on the items in
// cause. Their signatures let the editor redraw the overlay only when a shown string changes. Pure: no DOM.

import { type LabState, pinKey } from "../state";
import type { CompId, Dir, Rect, Severity } from "../types";
import { formatReading } from "./format";
import { type FPt, intersects } from "./geom";
import type { SceneModel } from "./model";
import { DYNAMIC_KINDS } from "./symbols";

export interface LiveItem {
  text: string;
  at: FPt;
  /** Pin direction: the box goes beyond the pin; null: next to the hovered point. */
  dir: Dir | null;
}

export interface DiagMarker {
  at: FPt;
  severity: Exclude<Severity, "info">;
}

export function liveItems(state: LabState, model: SceneModel, hoverAt: FPt | null): LiveItem[] {
  const out: LiveItem[] = [];
  const sel = state.selection;
  if (sel.components.size === 1 && sel.wires.size === 0) {
    const id: CompId = sel.components.values().next().value as CompId;
    const g = model.comps.get(id);
    const cv = state.compValues.get(id);
    if (g && cv && g.comp.kind !== "junction") {
      for (const pv of cv.pins) {
        const pin = g.pins.find((p) => p.id === pv.pin);
        if (pin) out.push({ text: `${pin.short} ${formatReading(pv.voltage, "V")}`, at: pin.pos, dir: pin.dir });
      }
    }
  }
  const net = state.hover.net;
  if (net && hoverAt) {
    const nv = state.netValues.get(pinKey(net));
    if (nv) out.push({ text: `${nv.name} ${formatReading(nv.voltage, "V")}`, at: hoverAt, dir: null });
  }
  return out;
}

export const liveSignature = (items: readonly LiveItem[]): string =>
  items.map((i) => `${i.text}@${i.at[0]},${i.at[1]},${i.dir ?? ""}`).join("|");

/** Top-left corner of a live box, in device pixels from its anchor; `moved`: not at the first place tried. */
export interface LivePlace {
  dx: number;
  dy: number;
  moved: boolean;
}

/** Shifts tried across the lead, in half box heights (0, −1, +1, −2, … ±12), and outwards, in box heights. */
const ACROSS = [0, ...Array.from({ length: 12 }, (_, k) => [-(k + 1), k + 1]).flat()];
const OUTWARD = [0, 1, 2, 3];
/** Symbol tests per layout: a crowded view (many pins, zoomed out) stays cheap; the rest take their first place. */
const MAX_TESTS = 800;

interface Candidate {
  dx: number;
  dy: number;
  cost: number;
}

function candidates(dir: Dir, w: number, h: number, gap: number): Candidate[] {
  const out: Candidate[] = [];
  const horizontal = dir === "l" || dir === "r";
  // Across a vertical lead the box moves sideways by quarter widths (at least half a height).
  const step = horizontal ? h / 2 : Math.max(h / 2, w / 4);
  for (const i of OUTWARD) {
    for (const j of ACROSS) {
      const away = gap + i * h;
      const across = j * step;
      let dx: number;
      let dy: number;
      switch (dir) {
        case "l": dx = -away - w; dy = -h / 2 + across; break;
        case "r": dx = away; dy = -h / 2 + across; break;
        case "u": dx = -w / 2 + across; dy = -away - h; break;
        default: dx = -w / 2 + across; dy = away;
      }
      out.push({ dx: Math.round(dx), dy: Math.round(dy), cost: Math.abs(across) + i * h });
    }
  }
  // Stable: equal costs keep the order above (nearer the lead first, then above or left first).
  return out.map((c, k) => ({ c, k })).sort((a, b) => a.c.cost - b.c.cost || a.k - b.k).map((e) => e.c);
}

/**
 * Places the live boxes (device pixels, `u` device pixels per grid unit): each pin's box beyond the pin, along its
 * outward direction. When that place is taken (`covers`, world rectangle: a symbol or a diagnostic marker) or by an
 * earlier box, the nearest free place among shifts across the lead and further out is taken; with none free, the
 * nearest place clear of symbols and markers, else the first place. Deterministic, independent of the pan (offsets from the anchor) and bounded (MAX_TESTS).
 * Boxes without a direction (hovered net) stay next to their point.
 */
export function layoutLive(items: readonly LiveItem[], widths: readonly number[], h: number, gap: number, u: number,
  covers: (r: Rect) => boolean): LivePlace[] {
  const placed: Rect[] = [];
  const world = (it: LiveItem, c: Candidate, w: number, margin: number): Rect => ({
    x0: it.at[0] + (c.dx - margin) / u, y0: it.at[1] + (c.dy - margin) / u,
    x1: it.at[0] + (c.dx + w + margin) / u, y1: it.at[1] + (c.dy + h + margin) / u,
  });
  const out: LivePlace[] = items.map(() => ({ dx: Math.round(gap), dy: Math.round(-gap - h), moved: false }));
  // Side by side, along each side: neighbours stack in order.
  const along = (it: LiveItem): number => (it.dir === "l" || it.dir === "r" ? it.at[1] : it.at[0]);
  const order = items.map((_, k) => k).filter((k) => items[k].dir !== null)
    .sort((a, b) => (items[a].dir ?? "").localeCompare(items[b].dir ?? "") || along(items[a]) - along(items[b]) || a - b);
  let tests = 0;
  for (const k of order) {
    const it = items[k];
    const w = widths[k];
    const cands = candidates(it.dir as Dir, w, h, gap);
    let pick: Candidate | null = null;
    let clearOfSymbols: Candidate | null = null;
    for (const c of cands) {
      if (tests++ >= MAX_TESTS) break;
      if (covers(world(it, c, w, 0))) continue;
      clearOfSymbols ??= c;
      const r = world(it, c, w, 1);
      if (placed.some((p) => intersects(p, r))) continue;
      pick = c;
      break;
    }
    const c = pick ?? clearOfSymbols ?? cands[0];
    placed.push(world(it, c, w, 0));
    out[k] = { dx: c.dx, dy: c.dy, moved: c !== cands[0] };
  }
  return out;
}

/** One marker per place, an error winning over a warning; info diagnostics have none. */
export function diagMarkers(state: LabState, model: SceneModel): DiagMarker[] {
  const at = new Map<string, DiagMarker>();
  for (const d of state.values?.diagnostics ?? []) {
    if (d.severity === "info") continue;
    for (const it of d.items) {
      let p: FPt | null = null;
      if (it.type === "component") {
        const g = model.comps.get(it.id);
        if (g) p = g.comp.kind === "junction" ? g.pins[0]?.pos ?? null : [g.body.x1, g.body.y0];
      } else if (it.type === "wire") {
        const g = model.wires.get(it.id);
        if (g && g.path.length >= 2) {
          let k = 0;
          let best = -1;
          for (let i = 0; i + 1 < g.path.length; i++) {
            const len = Math.abs(g.path[i + 1][0] - g.path[i][0]) + Math.abs(g.path[i + 1][1] - g.path[i][1]);
            if (len > best) {
              best = len;
              k = i;
            }
          }
          p = [(g.path[k][0] + g.path[k + 1][0]) / 2, (g.path[k][1] + g.path[k + 1][1]) / 2];
        }
      } else {
        p = model.pinPos(it) ?? null;
      }
      if (!p) continue;
      const key = `${p[0]},${p[1]}`;
      const prev = at.get(key);
      if (!prev || (prev.severity === "warning" && d.severity === "error")) at.set(key, { at: p, severity: d.severity });
    }
  }
  return [...at.values()];
}

export const diagSignature = (markers: readonly DiagMarker[]): string =>
  markers.map((m) => `${m.at[0]},${m.at[1]}:${m.severity}`).sort().join("|");

/** States drawn on the overlay (blade, contact and cap, wiper): values can bring them as well as interaction replies. */
export function stateSignature(state: LabState): string {
  const parts: string[] = [];
  for (const c of state.components.values()) {
    if (!DYNAMIC_KINDS.has(c.kind)) continue;
    const s = state.states.get(c.id);
    parts.push(`${c.id}:${s?.closed}:${s?.pressed}:${s?.latched}:${s?.position}`);
  }
  return parts.join("|");
}
