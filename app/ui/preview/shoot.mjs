#!/usr/bin/env node
// Screenshots of the preview states (dev only, see boot.ts). Starts vite (port 5199 by default, --strictPort), opens
// every state of fixtures.ts at every size in headless Chrome, waits until the page says the state is drawn
// (<html data-preview-ready="1">, read through the DevTools protocol), writes <out>/<state>-<w>x<h>.png, then stops
// vite and Chrome. Re-run it after a UI change to get the same shots again.
//
//   node ui/preview/shoot.mjs --out <folder> [--sizes 1702x1114,1366x768] [--states e0-start,e1-incident]
//                             [--port 5199] [--url http://127.0.0.1:5199] [--chrome <chrome.exe>] [--scrollbars] [--cli]
//                             [--lint]
//
//   --sizes  shot sizes in pixels; WxH@scale emulates Windows display scaling (1702x1114@1.25: the page sees a
//            1362x891 viewport, as in the application on a 125 % screen). Files: <state>-<w>x<h>[@<scale>x].png
//   --url    a server already running: vite is neither started nor stopped.
//   --scrollbars  keep the scrollbars (hidden by default with --hide-scrollbars; the application shows them).
//   --cli    one plain `chrome --headless=new --screenshot` per shot (virtual-time budget) instead of the DevTools
//            protocol: no readiness check, no console report. Fallback only.
//   --lint   after each shot, runs the page's lint (window.__previewLint, lint.ts: checks L1–L7) and writes
//            <state>-<w>x<h>[@<scale>x].lint.json next to the PNG; a failed check fails the shot. Not with --cli.
//
// Node 22 or later (global fetch and WebSocket). Exit code 1 when a state did not draw, reported a warning, an
// exception or a command the fake core does not know, or (--lint) failed a check; its shot is written anyway.

import { spawn, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const APP_DIR = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const tail = (text, n = 20) => text.split(/\r?\n/).slice(-n).join("\n");

const USAGE = `usage: node ui/preview/shoot.mjs --out <folder> [--sizes 1702x1114,1366x768] [--states a,b]
                                [--port 5199] [--url http://127.0.0.1:5199] [--chrome <path>] [--scrollbars] [--cli]
                                [--timeout 20000] [--lint]`;

function parseArgs(argv) {
  const o = {
    out: null, sizes: "1702x1114,1366x768", states: null, port: 5199, url: null, chrome: null, cli: false, scrollbars: false, timeout: 20000,
    lint: false,
  };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    const value = () => {
      const v = argv[++i];
      if (v === undefined) throw new Error(`${a}: missing value\n${USAGE}`);
      return v;
    };
    if (a === "--out") o.out = resolve(value());
    else if (a === "--sizes") o.sizes = value();
    else if (a === "--states") o.states = value().split(",").map((s) => s.trim()).filter(Boolean);
    else if (a === "--port") o.port = Number(value());
    else if (a === "--url") o.url = value().replace(/\/+$/, "");
    else if (a === "--chrome") o.chrome = value();
    else if (a === "--timeout") o.timeout = Number(value());
    else if (a === "--cli") o.cli = true;
    else if (a === "--scrollbars") o.scrollbars = true;
    else if (a === "--lint") o.lint = true;
    else if (a === "-h" || a === "--help") {
      console.log(USAGE);
      process.exit(0);
    } else throw new Error(`unknown option ${a}\n${USAGE}`);
  }
  if (!o.out) throw new Error(`--out is required\n${USAGE}`);
  if (o.lint && o.cli) throw new Error("--lint needs the DevTools protocol: it cannot be combined with --cli");
  o.sizes = o.sizes.split(",").map((s) => {
    const m = /^(\d+)x(\d+)(?:@(\d+(?:\.\d+)?))?$/.exec(s.trim());
    if (!m) throw new Error(`bad size "${s}" (expected WxH or WxH@scale, e.g. 1366x768 or 1702x1114@1.25)`);
    const scale = m[3] ? Number(m[3]) : 1;
    if (!(scale >= 0.5 && scale <= 4)) throw new Error(`bad scale in "${s}"`);
    return { w: Number(m[1]), h: Number(m[2]), scale };
  });
  if (!Number.isInteger(o.port) || o.port <= 0) throw new Error(`bad port ${o.port}`);
  return o;
}

function findChrome(explicit) {
  const local = process.env.LOCALAPPDATA ? join(process.env.LOCALAPPDATA, "Google", "Chrome", "Application", "chrome.exe") : null;
  const candidates = [
    explicit, process.env.CHROME_PATH,
    "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe", "C:\\Program Files (x86)\\Google\\Chrome\\Application\\chrome.exe", local,
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome", "/usr/bin/google-chrome", "/usr/bin/chromium",
  ].filter(Boolean);
  const found = candidates.find((p) => existsSync(p));
  if (!found) throw new Error("Chrome not found: pass --chrome <path> or set CHROME_PATH");
  return found;
}

/** Stops a process and its children (vite keeps an esbuild child; Chrome has many). */
function killTree(child) {
  if (!child || child.exitCode !== null || child.signalCode !== null) return;
  if (process.platform === "win32") spawnSync("taskkill", ["/pid", String(child.pid), "/T", "/F"], { stdio: "ignore", windowsHide: true });
  else child.kill("SIGTERM");
}

function waitExit(child, ms) {
  if (child.exitCode !== null || child.signalCode !== null) return Promise.resolve();
  return Promise.race([new Promise((r) => child.once("exit", r)), sleep(ms)]);
}

// ------------------------------------------------------------------------------------------------- vite

async function startVite(port) {
  const bin = join(APP_DIR, "node_modules", "vite", "bin", "vite.js");
  if (!existsSync(bin)) throw new Error(`vite is not installed (${bin}): run npm install in app/`);
  const child = spawn(process.execPath, [bin, "--port", String(port), "--strictPort", "--host", "127.0.0.1"], {
    cwd: APP_DIR, stdio: ["ignore", "pipe", "pipe"], windowsHide: true,
  });
  let log = "";
  child.stdout.on("data", (d) => (log += d));
  child.stderr.on("data", (d) => (log += d));
  const base = `http://127.0.0.1:${port}`;
  const end = Date.now() + 60_000;
  while (Date.now() < end) {
    if (child.exitCode !== null) throw new Error(`vite exited with code ${child.exitCode} (port ${port} busy?):\n${tail(log)}`);
    try {
      const r = await fetch(`${base}/preview/index.html`);
      if (r.ok) return { base, child, log: () => log };
    } catch {
      /* not listening yet */
    }
    await sleep(250);
  }
  killTree(child);
  throw new Error(`vite did not answer on ${base} within 60 s:\n${tail(log)}`);
}

// ------------------------------------------------------------------------------------ DevTools protocol

class Cdp {
  constructor(ws) {
    this.ws = ws;
    this.seq = 0;
    this.pending = new Map();
    this.listeners = new Set();
    ws.addEventListener("message", (ev) => {
      const msg = JSON.parse(typeof ev.data === "string" ? ev.data : Buffer.from(ev.data).toString("utf8"));
      const p = msg.id !== undefined ? this.pending.get(msg.id) : undefined;
      if (p) {
        this.pending.delete(msg.id);
        if (msg.error) p.reject(new Error(`${p.method}: ${msg.error.message}`));
        else p.resolve(msg.result);
      } else if (msg.method) for (const l of this.listeners) l(msg);
    });
    ws.addEventListener("close", () => {
      for (const p of this.pending.values()) p.reject(new Error(`${p.method}: DevTools connection closed`));
      this.pending.clear();
    });
  }

  static async connect(url) {
    const ws = new WebSocket(url);
    await new Promise((res, rej) => {
      ws.addEventListener("open", res, { once: true });
      ws.addEventListener("error", () => rej(new Error(`cannot connect to ${url}`)), { once: true });
    });
    return new Cdp(ws);
  }

  send(method, params = {}, sessionId = undefined) {
    const id = ++this.seq;
    this.ws.send(JSON.stringify(sessionId ? { id, method, params, sessionId } : { id, method, params }));
    return new Promise((resolve, reject) => this.pending.set(id, { resolve, reject, method }));
  }

  on(listener) {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  close() {
    this.ws.close();
  }
}

/** The flags of every Chrome run (the task's reference command: --headless=new --disable-gpu --hide-scrollbars). */
function chromeFlags(o, profile) {
  return ["--headless=new", "--disable-gpu", ...(o.scrollbars ? [] : ["--hide-scrollbars"]), `--user-data-dir=${profile}`,
    "--no-first-run", "--no-default-browser-check", "--disable-extensions"];
}

async function launchChrome(o, chromePath, profile) {
  const child = spawn(chromePath, [...chromeFlags(o, profile), "--remote-debugging-port=0", "--window-size=1702,1114", "about:blank"], {
    stdio: ["ignore", "ignore", "pipe"], windowsHide: true,
  });
  let err = "";
  const ws = await new Promise((res, rej) => {
    const timer = setTimeout(() => rej(new Error(`Chrome did not open DevTools within 30 s:\n${tail(err)}`)), 30_000);
    child.stderr.on("data", (d) => {
      err += d;
      const m = /DevTools listening on (ws:\/\/\S+)/.exec(err);
      if (m) {
        clearTimeout(timer);
        res(m[1]);
      }
    });
    child.once("exit", (code) => {
      clearTimeout(timer);
      rej(new Error(`Chrome exited with code ${code}:\n${tail(err)}`));
    });
  });
  return { child, ws };
}

const READY = `(() => {
  const d = document.documentElement.dataset;
  return { ready: d.previewReady === "1", error: d.previewError || "", warn: d.previewWarn || "",
           unhandled: d.previewUnhandled || "", states: d.previewStates || "" };
})()`;
const LINT = `(() => window.__previewLint ? window.__previewLint() : { missing: true })()`;

/** Opens `url` in a fresh browser context (own storage), waits for the preview, optionally shoots it. The shot is
 * w×h pixels; with a scale (Windows display scaling, e.g. 1.25) the page sees a viewport of w/scale × h/scale.
 * With `lintFile`, the page's lint report is written there and returned as `lint`. */
async function visit(cdp, url, { w, h, scale }, timeout, file, lintFile = null) {
  const { browserContextId } = await cdp.send("Target.createBrowserContext");
  const { targetId } = await cdp.send("Target.createTarget", { url: "about:blank", browserContextId });
  const { sessionId } = await cdp.send("Target.attachToTarget", { targetId, flatten: true });
  const problems = [];
  const off = cdp.on((m) => {
    if (m.sessionId !== sessionId) return;
    if (m.method === "Runtime.exceptionThrown") {
      const d = m.params.exceptionDetails;
      problems.push(`exception: ${(d.exception && d.exception.description) || d.text}`.split("\n")[0]);
    } else if (m.method === "Runtime.consoleAPICalled" && m.params.type === "error") {
      problems.push(`console.error: ${m.params.args.map((a) => a.value ?? a.description ?? "").join(" ")}`.split("\n")[0]);
    }
  });
  try {
    await cdp.send("Runtime.enable", {}, sessionId);
    await cdp.send("Page.enable", {}, sessionId);
    await cdp.send("Emulation.setDeviceMetricsOverride", {
      width: Math.round(w / scale), height: Math.round(h / scale), deviceScaleFactor: scale, mobile: false,
    }, sessionId);
    await cdp.send("Emulation.setFocusEmulationEnabled", { enabled: true }, sessionId);
    await cdp.send("Page.navigate", { url }, sessionId);
    const end = Date.now() + timeout;
    let state = null;
    while (Date.now() < end) {
      try {
        const r = await cdp.send("Runtime.evaluate", { expression: READY, returnByValue: true }, sessionId);
        if (r.result && r.result.value && r.result.value.ready) {
          state = r.result.value;
          break;
        }
      } catch {
        /* the document is being replaced */
      }
      await sleep(100);
    }
    state ??= { ready: false, error: `not drawn within ${timeout / 1000} s`, warn: "", unhandled: "", states: "" };
    if (file) {
      const { data } = await cdp.send("Page.captureScreenshot", { format: "png" }, sessionId);
      writeFileSync(file, Buffer.from(data, "base64"));
    }
    let lint = null;
    if (lintFile && state.ready) {
      const r = await cdp.send("Runtime.evaluate", { expression: LINT, returnByValue: true }, sessionId);
      lint = r.exceptionDetails
        ? { missing: true, error: (r.exceptionDetails.exception && r.exceptionDetails.exception.description) || r.exceptionDetails.text }
        : r.result.value;
      writeFileSync(lintFile, JSON.stringify({ url, size: { w, h, scale }, ...lint }, null, 1) + "\n");
    }
    return { ...state, problems, lint };
  } finally {
    off();
    await cdp.send("Target.closeTarget", { targetId }).catch(() => {});
    await cdp.send("Target.disposeBrowserContext", { browserContextId }).catch(() => {});
  }
}

// ------------------------------------------------------------------------------------------------- main

const pageUrl = (base, state) => `${base}/preview/index.html${state ? `?state=${encodeURIComponent(state)}` : ""}`;
const sizeText = ({ w, h, scale }) => `${w}x${h}${scale === 1 ? "" : `@${scale}x`}`;
const shotFile = (out, state, size) => join(out, `${state}-${sizeText(size)}.png`);
const lintFileOf = (out, state, size) => join(out, `${state}-${sizeText(size)}.lint.json`);

/** One line per lint: the failure count of every rule, then the first failures. */
function lintNotes(lint) {
  if (!lint) return { ok: false, notes: ["lint: not run (state not drawn)"] };
  if (lint.missing) return { ok: false, notes: [`lint: window.__previewLint unavailable${lint.error ? ` (${lint.error.split("\n")[0]})` : ""}`] };
  const counts = Object.entries(lint.counts).map(([rule, n]) => `${rule} ${n ? n : "ok"}`).join(", ");
  const first = lint.failures.slice(0, 3).map((f) => `  ${f.rule} ${f.target}${f.text ? ` « ${f.text} »` : ""}: ${f.detail}`);
  return { ok: lint.pass, notes: [`lint: ${counts}`, ...first] };
}

function report(state, size, file, r, linting = false) {
  const notes = [];
  if (!r.ready) notes.push("NOT READY");
  if (r.error) notes.push(`error: ${r.error.split("\n")[0]}`);
  if (r.warn) notes.push(`warn: ${r.warn}`);
  if (r.unhandled) notes.push(`unhandled commands: ${r.unhandled}`);
  notes.push(...r.problems.slice(0, 3));
  let linted = true;
  if (linting) {
    const l = lintNotes(r.lint);
    linted = l.ok;
    notes.push(...l.notes);
  }
  const ok = r.ready && !r.error && !r.warn && !r.unhandled && r.problems.length === 0 && linted;
  console.log(`${ok ? "ok  " : "FAIL"} ${state} ${sizeText(size)} -> ${file}${notes.length ? `\n     ${notes.join("\n     ")}` : ""}`);
  return ok;
}

async function runCdp(o, chromePath, base, profile) {
  const chrome = await launchChrome(o, chromePath, profile);
  const cdp = await Cdp.connect(chrome.ws);
  let failures = 0;
  try {
    // the list of states comes from the preview itself: fixtures.ts stays the only list
    const index = await visit(cdp, pageUrl(base, null), o.sizes[0], o.timeout, null);
    const states = o.states ?? index.states.split(",").filter(Boolean);
    if (!states.length) throw new Error(`no state listed by ${pageUrl(base, null)}`);
    // warm-up: the first load compiles every module and lets vite finish optimizing dependencies (it may reload)
    await visit(cdp, pageUrl(base, states[0]), o.sizes[0], o.timeout * 3, null);
    for (const state of states) {
      for (const size of o.sizes) {
        const file = shotFile(o.out, state, size);
        const lintFile = o.lint ? lintFileOf(o.out, state, size) : null;
        if (!report(state, size, file, await visit(cdp, pageUrl(base, state), size, o.timeout, file, lintFile), o.lint)) failures++;
      }
    }
  } finally {
    await cdp.send("Browser.close").catch(() => {});
    cdp.close();
    await waitExit(chrome.child, 5000);
    killTree(chrome.child);
  }
  return failures;
}

function runCli(o, chromePath, base, profile) {
  const common = chromeFlags(o, profile);
  let states = o.states;
  if (!states) {
    const dom = spawnSync(chromePath, [...common, "--dump-dom", "--virtual-time-budget=5000", pageUrl(base, null)], {
      encoding: "utf8", timeout: 60_000, windowsHide: true,
    });
    const m = /data-preview-states="([^"]*)"/.exec(dom.stdout ?? "");
    if (!m) throw new Error("--cli: cannot read the list of states; pass --states");
    states = m[1].split(",").filter(Boolean);
  }
  let failures = 0;
  for (const state of states) {
    for (const size of o.sizes) {
      const file = shotFile(o.out, state, size);
      const { w, h, scale } = size;
      rmSync(file, { force: true });
      spawnSync(chromePath, [
        ...common, `--window-size=${Math.round(w / scale)},${Math.round(h / scale)}`, `--force-device-scale-factor=${scale}`,
        `--screenshot=${file}`, "--virtual-time-budget=8000", pageUrl(base, state),
      ], { stdio: "ignore", timeout: 60_000, windowsHide: true });
      const ok = existsSync(file);
      if (!ok) failures++;
      console.log(`${ok ? "ok  " : "FAIL"} ${state} ${sizeText(size)} -> ${file} (cli: readiness not checked)`);
    }
  }
  return failures;
}

async function main() {
  const o = parseArgs(process.argv.slice(2));
  mkdirSync(o.out, { recursive: true });
  const chromePath = findChrome(o.chrome);
  const vite = o.url ? null : await startVite(o.port);
  const base = o.url ?? vite.base;
  const profile = mkdtempSync(join(tmpdir(), "n7rt-preview-chrome-"));
  let failures = 0;
  try {
    console.log(`preview: ${pageUrl(base, null)}${vite ? " (vite started by this script)" : ""}`);
    failures = o.cli ? runCli(o, chromePath, base, profile) : await runCdp(o, chromePath, base, profile);
  } finally {
    if (vite) {
      killTree(vite.child);
      await waitExit(vite.child, 5000);
    }
    rmSync(profile, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
  }
  console.log(failures ? `${failures} shot(s) with problems` : "all shots ok");
  process.exitCode = failures ? 1 : 0;
}

main().catch((err) => {
  console.error(`shoot: ${err instanceof Error ? err.message : err}`);
  process.exitCode = 1;
});
