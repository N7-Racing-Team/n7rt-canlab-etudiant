// Preview harness (dev only: vite builds ui/index.html alone, this page never ships in the application). It boots the
// real UI against a fake core, in a browser, for design reviews and before/after screenshots:
//   1. window.__TAURI_INTERNALS__ is installed (official @tauri-apps/api/mocks) before any application module loads;
//      every command is answered from the fixtures of the state named by ?state=… (fixtures.ts);
//   2. the markup is the real ui/index.html: its stylesheets are linked and its body adopted, so a redesign of
//      index.html or of the styles shows here unchanged;
//   3. src/main.ts is imported, exactly as the application starts;
//   4. the state's finishing touches: « Ouvrir la fiche » pressed, its user actions (Scenario.after), the relevant
//      panel scrolled into view. A state may also set the panel layout and the view before the application starts.
// When everything is drawn, <html data-preview-ready="1"> (shoot.mjs waits for it); data-preview-error and
// data-preview-warn report what went wrong, data-preview-unhandled the commands the fake core does not know.
//
// Run (from app/):  npx vite --port 5199 --strictPort   then open http://127.0.0.1:5199/preview/?state=e0-fiche-open
// Every state at every size:  node ui/preview/shoot.mjs --out <folder>   (see shoot.mjs)
// Without ?state the page lists the states. &scroll=none leaves the panels at their top (as the application opens);
// &keepLayout=1 keeps the panel layout stored by a previous visit (otherwise the default layout of the window width).
// Type check: npx tsc --noEmit -p ui/preview (the fixtures are typed after src/types.ts).
// Lint: window.__previewLint() returns the acceptance checks L1–L7 of the drawn page (lint.ts; shoot.mjs --lint).

import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { scenario, STATE_NAMES, type Scenario, type Step } from "./fixtures";
import { lint, type LintReport } from "./lint";
import type { IncidentPart, NominalPart } from "../src/types";

declare global {
  interface Window {
    __previewLint?: () => LintReport;
  }
}
window.__previewLint = lint;

/** Keys of the application (main.ts VIEW_KEY, shell.ts layouts): the preview starts on the cockpit, default layout. */
const STORAGE_PREFIX = "n7rt-can-lab.";
const VIEW_KEY = "n7rt-can-lab.view.v1";
const COCKPIT_LAYOUT_KEY = "n7rt-can-lab.layout.v1";
const FICHE_TOGGLE = "#progress-fiche-toggle";
const FICHE_DRAWN = "#progress-fiche .fiche-title";
const TIMEOUT_MS = 10_000;

const root = document.documentElement;
const params = new URLSearchParams(location.search);
const unhandled = new Set<string>();
const warnings: string[] = [];

function warn(text: string): void {
  warnings.push(text);
  root.dataset.previewWarn = warnings.join(" | ");
  console.warn(`[preview] ${text}`);
}

// ---------------------------------------------------------------------------------------------- fake core

/** The core's answers for one state. Values are copied, as the IPC serialises them; saved parts are kept. */
function fakeCore(s: Scenario): (cmd: string, payload?: unknown) => unknown {
  const copy = <T>(v: T): T => structuredClone(v);
  return (cmd, payload) => {
    const args = (payload ?? {}) as Record<string, unknown>;
    switch (cmd) {
      case "lab_snapshot":
        return copy(s.batch);
      case "lab_drain":
        return null;
      case "lab_refresh": case "lab_connect": case "lab_disconnect": case "lab_start": case "lab_stop": case "lab_set_stimuli":
      case "lab_send_one": case "lab_reset_board": case "library_open": case "runs_open_folder":
        return null;
      case "mission_list":
        return copy(s.list);
      case "mission_state": case "mission_open": case "mission_close": case "mission_reset": case "mission_check":
      case "mission_consign": case "mission_reproduce": case "mission_fiche_open": case "mission_verify": case "student_open":
        return copy(s.mission);
      case "mission_fiche_get":
        return copy(s.fiche);
      case "mission_fiche_set":
        s.fiche = copy(args.part as NominalPart);
        return copy(s.mission);
      case "mission_incident_get":
        return s.incident && s.incident.rang === args.rank ? copy(s.incident) : null;
      case "mission_incident_set":
        s.incident = copy(args.part as IncidentPart);
        return copy(s.mission);
      case "access_state": case "instructor_logout": case "instructor_reset": case "student_choose": case "student_import_pack":
        return copy(s.access);
      case "e2e_config":
        return null;
      case "ide_state":
        return {
          available: false, reason: "Aperçu : l'IDE n'est pas simulé.", busy: false, phase: "idle", revision: 0, built_revision: null,
          files: ["student_decoder.c", "student_decoder.h"], diagnostics: [], build: null, flash: null,
        };
      // Physical Lab: only the states that carry a bench (Scenario.phys); the values never change
      case "phys_library": case "phys_snapshot": case "phys_values": case "phys_demos": case "phys_list": case "phys_journal":
      case "phys_command": case "phys_preview":
        if (s.phys) return physAnswer(s, cmd, args);
        unhandled.add(cmd);
        root.dataset.previewUnhandled = [...unhandled].join(",");
        throw `Aperçu : la commande ${cmd} n'est pas simulée.`;
      default:
        unhandled.add(cmd);
        root.dataset.previewUnhandled = [...unhandled].join(",");
        throw `Aperçu : la commande ${cmd} n'est pas simulée.`;
    }
  };
}

/** The Physical Lab of a state with a bench: a static circuit, the multimeter probes follow the commands. */
function physAnswer(s: Scenario, cmd: string, args: Record<string, unknown>): unknown {
  const p = s.phys!;
  const copy = <T>(v: T): T => structuredClone(v);
  const run = { type: "run", run: p.values.run, sim_time_ns: p.values.sim_time_ns };
  switch (cmd) {
    case "phys_library": return copy(p.library);
    case "phys_snapshot": return copy(p.snapshot);
    case "phys_values": return Number(args.sinceSeq ?? -1) >= p.values.seq ? null : copy(p.values);
    case "phys_demos": return copy(p.demos);
    case "phys_list": return copy(p.files);
    case "phys_journal": return copy(p.journal);
    case "phys_preview": return { base_rev: p.snapshot.rev, patch: { components: [], wires: [] } };
    default: {
      const c = (args.command ?? {}) as { cmd?: string; com?: unknown; vohm?: unknown; function?: unknown };
      if (c.cmd === "meter") {
        p.values = { ...p.values, seq: p.values.seq + 1, meter: { ...p.values.meter, ...copy({ com: c.com, vohm: c.vohm, function: c.function }) } as typeof p.values.meter };
      }
      return run;
    }
  }
}

// ------------------------------------------------------------------------------------------------ helpers

function visible(selector: string): boolean {
  const el = document.querySelector<HTMLElement>(selector);
  return !!el && !el.closest("[hidden]") && el.getClientRects().length > 0;
}

async function waitFor(test: () => boolean, what: string): Promise<boolean> {
  const end = performance.now() + TIMEOUT_MS;
  while (performance.now() < end) {
    if (test()) return true;
    await new Promise((r) => setTimeout(r, 25));
  }
  warn(`${what} : absent après ${TIMEOUT_MS / 1000} s`);
  return false;
}

const frame = (): Promise<void> => new Promise((r) => requestAnimationFrame(() => r()));

/** The real ui/index.html: stylesheets and named metas into this head, body adopted (scripts left out). */
async function adoptMarkup(): Promise<string> {
  const url = new URL("../index.html", import.meta.url);
  const res = await fetch(url, { cache: "no-store" });
  if (!res.ok) throw new Error(`ui/index.html : HTTP ${res.status}`);
  const doc = new DOMParser().parseFromString(await res.text(), "text/html");
  const loads: Promise<void>[] = [];
  for (const el of Array.from(doc.head.querySelectorAll('link[rel~="stylesheet"], style, meta[name]'))) {
    if (el instanceof HTMLMetaElement && (el.name === "viewport" || document.head.querySelector(`meta[name="${el.name}"]`))) continue;
    if (el instanceof HTMLLinkElement) {
      const link = document.createElement("link");
      link.rel = "stylesheet";
      link.href = new URL(el.getAttribute("href") ?? "", url).href;
      loads.push(new Promise((r) => { link.onload = link.onerror = () => r(); }));
      document.head.appendChild(link);
    } else {
      document.head.appendChild(document.importNode(el, true));
    }
  }
  for (const a of Array.from(doc.body.attributes)) document.body.setAttribute(a.name, a.value);
  document.body.replaceChildren(
    ...Array.from(doc.body.childNodes).filter((n) => n.nodeName !== "SCRIPT").map((n) => document.importNode(n, true)),
  );
  await Promise.all(loads);
  return doc.title;
}

function prepareStorage(s: Scenario): void {
  try {
    if (params.get("keepLayout") !== "1") {
      for (const k of Object.keys(localStorage)) if (k.startsWith(STORAGE_PREFIX)) localStorage.removeItem(k);
      if (s.layout) localStorage.setItem(COCKPIT_LAYOUT_KEY, JSON.stringify(s.layout));
    }
    localStorage.setItem(VIEW_KEY, s.view ?? "cockpit");
  } catch {
    warn("localStorage indisponible : l'application démarre sur sa vue par défaut");
  }
}

/** Without a known ?state: the list of states, as links. */
function index(asked: string | null): void {
  document.title = "N7RT CAN Lab — aperçu";
  root.dataset.previewStates = STATE_NAMES.join(",");
  const b = document.body;
  Object.assign(b.style, { margin: "0", padding: "24px 32px", background: "#16181d", color: "#e6e8ee", font: "14px/1.6 system-ui, sans-serif" });
  const h = document.createElement("h1");
  h.textContent = "N7RT CAN Lab — états de l'aperçu";
  Object.assign(h.style, { fontSize: "18px", margin: "0 0 4px" });
  b.appendChild(h);
  const p = document.createElement("p");
  p.textContent = asked ? `État inconnu : « ${asked} ». ` : "Interface réelle, cœur simulé, données inventées. ";
  Object.assign(p.style, { margin: "0 0 16px", color: "#9aa0ae" });
  b.appendChild(p);
  const ul = document.createElement("ul");
  Object.assign(ul.style, { margin: "0", paddingLeft: "18px" });
  for (const name of STATE_NAMES) {
    const li = document.createElement("li");
    const a = document.createElement("a");
    a.href = `?state=${encodeURIComponent(name)}`;
    a.textContent = name;
    Object.assign(a.style, { color: "#8ab4ff", fontFamily: "ui-monospace, Consolas, monospace" });
    li.append(a, ` — ${scenario(name)?.label ?? ""}`);
    ul.appendChild(li);
  }
  b.appendChild(ul);
  root.dataset.previewReady = "1";
}

/** One user action, as the e2e drives the native controls (bubbling input and change events, then a click). */
async function act(step: Step): Promise<void> {
  const selector = step.type?.[0] ?? step.change?.[0] ?? step.click ?? "";
  if (!(await waitFor(() => visible(selector), `cible d'action ${selector}`))) return;
  const el = document.querySelector<HTMLElement>(selector)!;
  if (step.click) {
    el.click();
    return;
  }
  const [, value] = (step.type ?? step.change)!;
  (el as HTMLInputElement).value = value;
  if (step.type) el.dispatchEvent(new Event("input", { bubbles: true }));
  el.dispatchEvent(new Event("change", { bubbles: true }));
}

// ---------------------------------------------------------------------------------------------------- boot

async function boot(name: string, s: Scenario): Promise<void> {
  mockWindows("main");
  mockIPC(fakeCore(s), { shouldMockEvents: true });
  prepareStorage(s);
  const title = await adoptMarkup();
  document.title = `${title} — aperçu ${name}`;
  await import("../src/main");
  if (!(await waitFor(() => visible(s.ready), `élément d'état ${s.ready}`))) return;
  if (s.openFiche) {
    const toggle = document.querySelector<HTMLButtonElement>(FICHE_TOGGLE);
    if (!toggle) warn(`bouton ${FICHE_TOGGLE} introuvable`);
    else {
      toggle.click();
      await waitFor(() => visible(FICHE_DRAWN), `fiche ouverte ${FICHE_DRAWN}`);
    }
  }
  for (const step of s.after ?? []) {
    await act(step);
    await new Promise((r) => setTimeout(r, 100));
  }
  await document.fonts.ready;
  await frame();
  if (s.scroll && params.get("scroll") !== "none") {
    const target = document.querySelector(s.scroll);
    if (target) target.scrollIntoView({ block: "start" });
    else warn(`cible de défilement ${s.scroll} introuvable`);
  }
  for (let i = 0; i < 3; i++) await frame();
}

const asked = params.get("state");
const s = asked ? scenario(asked) : null;
if (!asked || !s) {
  index(asked);
} else {
  boot(asked, s)
    .catch((err) => {
      root.dataset.previewError = String(err instanceof Error ? err.stack ?? err.message : err);
      console.error(err);
    })
    .finally(() => {
      root.dataset.previewReady = "1";
    });
}
