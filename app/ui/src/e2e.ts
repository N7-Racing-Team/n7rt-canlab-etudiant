// Automated end-to-end scenario (app started with --e2e). It drives the real controls (buttons,
// selects, sliders), waits for what the window displays, reads the rendered rows back and asks
// the core to compare them with the frames and answers that really crossed the link.
// With --coordinated, steps that need the outside world (halting the MCU through SWD, holding
// the serial port) are synchronised with the harness through <report>.progress / .<step>.go.
// Scenarios mission-<id>, for id = apps, wsf, sas or bps (docs/apps-mission-slice.md §7): instructor login (code
// N7RT_E2E_INSTRUCTOR_CODE), the mission with its reference pack on the board, its raw bus, the bench in the Physical
// Lab view, then « Vérifier l'essai nominal » and its verdict (--expect-verdict pass|fail|any).
// Scenarios student-<id> (docs/distribution.md §2.1): instructor login, reset and logout, then the student flow: choice
// of the mission with its confirmation (other missions are then refused by the core), import of the group pack
// N7RT_E2E_PACK, opening with the group code N7RT_E2E_GROUP_CODE, the raw bus, « Enregistrer l'essai nominal » and the
// run file, whose path is reported for the harness to judge with n7rt-tool. No verdict is ever shown to the student.
// Scenarios progression-<id>: the same student prelude with a release 0.3 pack, then its progression E0 to E5 on the
// board (ui/src/e2e-progression.ts); N7RT_E2E_STAGE « resume » skips the prelude and reopens the mission. Scenarios
// pack-<id>: the prelude and the opening of a release 0.3 pack without any board, then its initial progression.
// Both flows fail if the board reboots between the session start and the end of the nominal check.
// The codes never enter the report.

import { access, e2e, lab, mission } from "./bridge";
import { $ } from "./dom";
import { ideScenario, isBoardReboot, missionScenario } from "./e2e-missions";
import type { UiPerf } from "./perf";
import type { Store } from "./store";
import type { AccessView, DisplayedRow, E2eConfig, FrameFormat, FrameRow, MissionView, UiBatch, Verification } from "./types";
import { CONFIRM_CHOICE, modeText } from "./views/access";
import { MESSAGES, benchBanner } from "./views/mission";

interface StepResult {
  name: string;
  ok: boolean;
  ms: number;
  detail?: unknown;
  error?: string;
}

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

class Failure extends Error {}

export async function runScenario(cfg: E2eConfig, store: Store, perf: UiPerf): Promise<void> {
  const requestedScenario = cfg.scenario;
  let ideChecks: unknown[] | null = null;
  if (ideScenario(cfg.scenario)) {
    try {
      const { prepareIde } = await import("./ide/e2e");
      ideChecks = await prepareIde(cfg, store, cfg.scenario === "ide");
      cfg = { ...cfg, scenario: cfg.scenario === "ide" ? "functional" : cfg.scenario.slice(4) };
    } catch (err) {
      await e2e.finish({ scenario: requestedScenario, ok: false, error: String(err), steps: [{ name: "ide-workflow", ok: false, error: String(err) }] }, false);
      return;
    }
  }
  if (cfg.scenario === "phys" || cfg.scenario === "phys-perf") {
    // Physical Lab scenarios, without board: ui/src/lab/e2e-phys.ts.
    const { runPhysScenario } = await import("./lab/e2e-phys");
    await runPhysScenario(cfg, perf);
    return;
  }
  const steps: StepResult[] = [];
  const verifications: (Verification & { step: string })[] = [];
  const report: Record<string, unknown> = {
    app: "N7RT CAN Lab",
    scenario: requestedScenario,
    ide: ideChecks,
    started_at: new Date().toISOString(),
    // The codes are not part of the report (the core removes them too).
    config: { ...cfg, instructor_code: cfg.instructor_code ? "(défini)" : null, group_code: cfg.group_code ? "(défini)" : null },
    steps,
    verifications,
  };

  const status = () => store.status!;
  const until = async (what: string, pred: () => boolean, timeoutMs: number) => {
    const t0 = performance.now();
    while (!pred()) {
      if (performance.now() - t0 > timeoutMs) {
        throw new Failure(`délai dépassé (${timeoutMs} ms) en attendant : ${what} — état ${status()?.state} « ${status()?.message} »`);
      }
      await sleep(20);
    }
    return performance.now() - t0;
  };
  // Buttons follow the status on the next animation frame: wait until enabled, like a user.
  const click = async (sel: string) => {
    const b = $<HTMLButtonElement>(sel);
    await until(`bouton ${sel} actif`, () => !b.disabled, 3000);
    b.click();
  };
  const set = (sel: string, value: string) => {
    const e = $<HTMLInputElement | HTMLSelectElement>(sel);
    e.value = value;
    e.dispatchEvent(new Event("input", { bubbles: true }));
    e.dispatchEvent(new Event("change", { bubbles: true }));
  };
  const check = (sel: string, on: boolean) => {
    const e = $<HTMLInputElement>(sel);
    e.checked = on;
    e.dispatchEvent(new Event("change", { bubbles: true }));
  };
  const step = async (name: string, fn: () => Promise<unknown>) => {
    await e2e.progress(name);
    const t0 = performance.now();
    try {
      const detail = await fn();
      steps.push({ name, ok: true, ms: Math.round(performance.now() - t0), detail });
    } catch (err) {
      steps.push({ name, ok: false, ms: Math.round(performance.now() - t0), error: String(err) });
      throw err;
    }
  };
  const released = async (gate: string, timeoutMs: number) => {
    const t0 = performance.now();
    while (!(await e2e.released(gate))) {
      if (performance.now() - t0 > timeoutMs) throw new Failure(`le banc n'a pas libéré l'étape ${gate}`);
      await sleep(50);
    }
  };

  const displayedRows = (): DisplayedRow[] =>
    Array.from(document.querySelectorAll<HTMLTableRowElement>("#frames tbody tr")).map((tr) => {
      const c = (col: string) => tr.querySelector(`[data-col="${col}"]`)?.textContent ?? "";
      return {
        session: c("session"), seq: c("seq"), id: c("id"), kind: c("kind"), dlc: c("dlc"),
        data: c("data"), result: c("result"), publications: c("publications"),
      };
    });
  /** With `oracle` false (missions), only the display is checked: the SDK example model does not apply. */
  const verify = async (name: string, oracle = true) => {
    const v = await e2e.verify(displayedRows());
    if (!oracle) {
      v.oracle_checked = 0;
      v.oracle_mismatches = [];
    }
    verifications.push({ step: name, ...v });
    if (!v.displayed || v.mismatches.length || v.oracle_mismatches.length) {
      throw new Failure(`affichage ≠ réponses de la carte : ${[...v.mismatches, ...v.oracle_mismatches].slice(0, 5).join(" | ")}`);
    }
    return { displayed: v.displayed, matched: v.matched, oracle_checked: v.oracle_checked };
  };
  /** Waits for a mission state (pulled from the core, as the panel does). */
  const untilMission = async (what: string, pred: (v: MissionView) => boolean, timeoutMs: number) => {
    const t0 = performance.now();
    for (;;) {
      const v = await mission.state();
      if (pred(v)) return v;
      if (performance.now() - t0 > timeoutMs) {
        throw new Failure(`délai dépassé (${timeoutMs} ms) en attendant : ${what} — essai ${JSON.stringify(v.check)}`);
      }
      await sleep(100);
    }
  };
  const sessionId = () => status().session?.id ?? 0;
  const active = () => status().session?.phase === "active";
  const drained = () => (store.stats?.in_flight ?? 1) === 0;

  const selectBoard = async () => {
    const sel = $<HTMLSelectElement>("#port-select");
    const wanted = () => (cfg.serial ? `sn:${cfg.serial}` : Array.from(sel.options).find((o) => o.value.startsWith("sn:"))?.value);
    // The list is re-rendered on the next animation frame after the status update.
    await until("carte dans la liste affichée", () => Array.from(sel.options).some((o) => o.value === wanted()), 5000);
    set("#port-select", wanted()!);
  };
  const connect = async () => {
    await until("carte listée", () => status().ports.some((p) => !cfg.serial || p.serial === cfg.serial), 10000);
    await selectBoard();
    const t0 = performance.now();
    const alreadyReady = status().state === "READY" && (!cfg.serial || status().port?.serial === cfg.serial);
    if (!alreadyReady) await click("#btn-connect");
    await until("carte prête", () => status().state === "READY", 8000);
    const b = status().board!;
    if (!b.runtime_verified || !b.compatible || b.student_status !== 1) throw new Failure(`carte non conforme : ${JSON.stringify(b)}`);
    return { handshake_ms: Math.round(performance.now() - t0), board: b, port: status().port };
  };
  const startSession = async () => {
    const before = store.logsReceived;
    set("#tick-select", "10");
    await click("#btn-start");
    await until("session active", active, 5000);
    await until("log student_init affiché", () => store.logsReceived > before &&
      Array.from(document.querySelectorAll("#logs li")).some((li) => li.textContent?.includes("student_decoder ready")), 3000);
    return { session: sessionId().toString(16).toUpperCase() };
  };
  const stimulate = (format: FrameFormat, id: string, dlc: number, pot: number, button: boolean, rate: number) => {
    set("#stim-format", format);
    set("#stim-id", id);
    set("#stim-dlc", String(dlc));
    set("#stim-pot", String(pot));
    check("#stim-button", button);
    set("#stim-rate", String(rate));
  };
  const sendFrames = async (n: number) => {
    for (let i = 0; i < n; i++) {
      const before = store.rowsReceived;
      await click("#btn-send-one");
      await until("réponse affichée", () => store.rowsReceived > before && !store.dirty.rows, 3000);
    }
  };

  // Same variants as the S1.2/S1.3 smoke test (tools/n7rt_link/runtime_check.py), through the
  // stimuli panel: classic, extended, remote, DLC 0, FD+BRS 64 bytes, FD 12 bytes, DLC 15 classic.
  const VARIANTS: [FrameFormat, string, number, number, boolean][] = [
    ["classic", "0x123", 8, 0x11, false],
    ["extended", "0x1ABCDEF", 2, 0xab, true],
    ["remote", "0x321", 4, 0x00, false],
    ["classic", "0x7FF", 0, 0x42, false],
    ["fd_brs", "0x456", 15, 0xfe, true],
    ["fd", "0x0FA", 9, 0x5a, false],
    ["classic", "0x200", 15, 0xff, true],
    ["classic", "0x000", 1, 0x00, false],
  ];

  let ok = false;
  const played = missionScenario(cfg.scenario);
  try {
    if (cfg.scenario === "functional") {
      await step("connexion", connect);
      await step("session", startSession);
      await step("variantes", async () => {
        for (const [format, id, dlc, pot, button] of VARIANTS) {
          stimulate(format, id, dlc, pot, button, 0);
          await sendFrames(1);
        }
        return { variants: VARIANTS.length };
      });
      await step("rafale-100fps", async () => {
        stimulate("classic", "0x125", 8, 10, false, 100);
        for (let i = 0; i < 8; i++) {
          await sleep(250);
          set("#stim-pot", String(20 + i * 25));
        }
        set("#stim-rate", "0");
        await until("plus de trame en vol", drained, 3000);
        await sleep(300);
        return { frames_sent: store.stats!.frames_sent, lost: store.stats!.lost };
      });
      await step("verification-1", () => verify("verification-1"));
      await step("stop-start", async () => {
        const first = sessionId();
        await click("#btn-stop");
        await until("session fermée", () => status().session === null && status().state === "READY", 5000);
        await startSession();
        if (sessionId() === first) throw new Failure("même session après Stop/Start");
        stimulate("extended", "0x18FF1234", 8, 77, true, 0);
        await sendFrames(3);
        return { previous: first.toString(16), next: sessionId().toString(16) };
      });
      await step("verification-2", () => verify("verification-2"));
      await step("deconnexion-reconnexion", async () => {
        await click("#btn-disconnect");
        await until("déconnecté", () => status().state === "DISCONNECTED" && status().port === null, 5000);
        const c = await connect();
        await startSession();
        stimulate("classic", "0x124", 8, 99, false, 0);
        await sendFrames(3);
        return { handshake_ms: c.handshake_ms };
      });
      await step("reset-carte", async () => {
        const before = sessionId();
        const t0 = performance.now();
        await lab.resetBoard();
        await until("session relancée après redémarrage", () => active() && sessionId() !== before, 8000);
        const resumeMs = Math.round(performance.now() - t0);
        stimulate("classic", "0x125", 8, 123, true, 0);
        await sendFrames(3);
        return { resume_ms: resumeMs, reset_causes: status().board?.reset_causes };
      });
      await step("verification-3", () => verify("verification-3"));
      if (cfg.coordinated) {
        await step("halt", async () => {
          stimulate("classic", "0x126", 8, 50, false, 50);
          await sleep(500);
          await e2e.progress("halt:ready");
          const tStall = await until("carte muette détectée (STALLED)", () => status().stalled, 20000);
          const tResume = await until("reprise après point d'arrêt", () => !status().stalled, 20000);
          await released("halt", 20000);
          await sleep(1500);
          set("#stim-rate", "0");
          await until("plus de trame en vol", drained, 8000);
          const st = store.stats!;
          return { stall_detect_ms: Math.round(tStall), resume_wait_ms: Math.round(tResume), lost_counted: st.lost, results: st.results };
        });
        await step("verification-halt", () => verify("verification-halt"));
        await step("port-occupe", async () => {
          await click("#btn-disconnect");
          await until("déconnecté", () => status().state === "DISCONNECTED", 5000);
          await e2e.progress("busy:ready");
          await released("busy", 20000);
          await selectBoard();
          await click("#btn-connect");
          await until("conflit de port signalé", () => status().state === "PORT_BUSY", 8000);
          const message = status().message;
          if (!message.includes(status().ports.find((p) => p.serial === cfg.serial)?.port ?? "§")) throw new Failure(`message sans nom de port : ${message}`);
          await e2e.progress("busy:seen");
          const t0 = performance.now();
          await until("connexion automatique après libération du port", () => status().state === "READY", 15000);
          return { message, recover_ms: Math.round(performance.now() - t0) };
        });
      }
      await step("fin", async () => {
        if (!active()) await startSession();
        await click("#btn-stop");
        await until("session fermée", () => status().session === null, 5000);
        return {};
      });
      await step("verification-finale", () => verify("verification-finale"));
    } else if (cfg.scenario === "perf") {
      await step("connexion", connect);
      await step("session", startSession);
      await step("charge", async () => {
        perf.reset();
        stimulate("classic", "0x123", 8, 0, false, cfg.rate);
        const t0 = performance.now();
        let k = 0;
        const samples: { t: number; tx: number; rx: number; inflight: number; loop_ms: number }[] = [];
        while (performance.now() - t0 < cfg.seconds * 1000) {
          await sleep(500);
          k += 1;
          set("#stim-pot", String((k * 37) % 256));
          const st = store.stats!;
          samples.push({ t: Math.round(performance.now() - t0), tx: st.tx_fps, rx: st.rx_fps, inflight: st.in_flight, loop_ms: st.queues.link_loop_max_us / 1000 });
        }
        set("#stim-rate", "0");
        await until("plus de trame en vol", drained, 5000);
        await sleep(500);
        report.samples = samples;
        report.ui = perf.summary();
        const st = store.stats!;
        return { frames_sent: st.frames_sent, results: st.results, lost: st.lost, nacks: st.nacks };
      });
      await step("verification", () => verify("verification"));
      await step("arret", async () => {
        await click("#btn-stop");
        await until("session fermée", () => status().session === null, 5000);
        return {};
      });
    } else if (cfg.scenario === "showcase") {
      // Screens for the documentation: the harness captures the window at each "shot:" step.
      const shot = async (name: string) => {
        await sleep(400);
        await e2e.progress(`shot:${name}`);
        await released(`shot-${name}`, 20000);
      };
      await step("connexion", connect);
      await shot("ready");
      await step("session", startSession);
      await step("trafic", async () => {
        stimulate("classic", "0x123", 8, 96, false, 0);
        await sendFrames(1);
        stimulate("fd_brs", "0x456", 15, 200, true, 0);
        await sendFrames(1);
        stimulate("remote", "0x321", 4, 0, false, 0);
        await sendFrames(1);
        stimulate("extended", "0x18FF1234", 8, 64, false, 40);
        await sleep(2500);
        return {};
      });
      await shot("session");
      await step("selection", async () => {
        const row = Array.from(document.querySelectorAll<HTMLTableRowElement>("#frames tbody tr"))
          .find((tr) => tr.querySelector('[data-col="kind"]')?.textContent?.includes("FD"));
        (row ?? document.querySelector<HTMLTableRowElement>("#frames tbody tr"))?.click();
        set("#stim-rate", "0");
        return {};
      });
      await shot("selected");
      await step("onglet-liaison", async () => {
        await click('.tab[data-tab="link"]');
        return {};
      });
      await shot("link");
      await step("onglet-diagnostic", async () => {
        await click('.tab[data-tab="diag"]');
        return {};
      });
      await shot("diag");
      await step("arret", async () => {
        await click('.tab[data-tab="logs"]');
        await click("#btn-stop");
        await until("session fermée", () => status().session === null, 5000);
        await click("#btn-disconnect");
        await until("déconnecté", () => status().state === "DISCONNECTED", 5000);
        return {};
      });
      await shot("disconnected");
    } else if (played) {
      const student = played.student;
      // Each capture names the elements to annotate: their rectangles (CSS pixels of the window's client area); « sel@k »
      // is the k-th match of sel.
      const shots: Record<string, unknown> = {};
      report.shots = shots;
      const shot = async (name: string, rects: string[] = []) => {
        await sleep(400);
        const rect = (spec: string) => {
          const [sel, k] = spec.split("@");
          const b = document.querySelectorAll(sel)[Number(k ?? 0)]?.getBoundingClientRect();
          return b && b.width > 0 ? [b.x, b.y, b.width, b.height].map((x) => Math.round(x)) : null;
        };
        shots[name] = { dpr: window.devicePixelRatio, inner: [window.innerWidth, window.innerHeight], rects: Object.fromEntries(rects.map((s) => [s, rect(s)])) };
        await e2e.progress(`shot:${name}`);
        await released(`shot-${name}`, 20000);
      };
      // Every delivered frame and every board reboot the core hands over, before the store keeps only the last rows;
      // the progression also keeps the last rows for its frame measures.
      const ids = new Set<number>();
      let reboots = 0;
      const kept: FrameRow[] = [];
      const KEPT_ROWS = 30000;
      const apply = store.apply.bind(store);
      store.apply = (b: UiBatch) => {
        for (const r of b.rows) if (r.outcome === "result") ids.add(r.can_id);
        if (played.progression) {
          kept.push(...b.rows);
          if (kept.length > KEPT_ROWS) kept.splice(0, kept.length - KEPT_ROWS);
        }
        reboots += b.errors.filter(isBoardReboot).length;
        apply(b);
      };
      // The IDE flash reboots the board before the session; from the session start, a reboot fails the scenario.
      let watch: { reboots: number; uptime_ms: number | null } | null = null;
      const watchBoard = () => {
        watch = { reboots, uptime_ms: status().heartbeat?.uptime_ms ?? null };
      };
      const noReboot = async () => {
        if (!watch) throw new Failure("surveillance des redémarrages non démarrée");
        const w: { reboots: number; uptime_ms: number | null } = watch;
        await sleep(1200); // one more heartbeat after the nominal check
        const uptime = status().heartbeat?.uptime_ms ?? null;
        const detail = { reboots: reboots - w.reboots, uptime_start_ms: w.uptime_ms, uptime_end_ms: uptime };
        if (detail.reboots) throw new Failure(`la carte a redémarré ${detail.reboots} fois pendant la mission`);
        if (w.uptime_ms !== null && uptime !== null && uptime <= w.uptime_ms) {
          throw new Failure(`uptime de la carte revenu en arrière (${w.uptime_ms} → ${uptime} ms) : redémarrage`);
        }
        return detail;
      };
      /** Waits for a mode or student state (pulled from the core). */
      const untilAccess = async (what: string, pred: (a: AccessView) => boolean, timeoutMs: number) => {
        const t0 = performance.now();
        for (;;) {
          const a = await access.state();
          if (pred(a)) return a;
          if (performance.now() - t0 > timeoutMs) {
            throw new Failure(`délai dépassé (${timeoutMs} ms) en attendant : ${what} — ${JSON.stringify(a)} « ${$("#mission-error").textContent} »`);
          }
          await sleep(100);
        }
      };
      const login = async () => {
        if (!cfg.instructor_code) throw new Failure("N7RT_E2E_INSTRUCTOR_CODE non défini");
        await click("#mode-btn");
        await until("dialogue du code encadrant", () => $<HTMLDialogElement>("#code-dialog").open, 3000);
        set("#code-input", cfg.instructor_code);
        await click("#code-submit");
        const t0 = performance.now();
        for (;;) {
          const a = await access.state();
          if (a.mode === "instructor" && !$<HTMLDialogElement>("#code-dialog").open) break;
          const shown = $("#code-error").textContent ?? "";
          if (shown) throw new Failure(`connexion encadrant refusée : « ${shown} »`);
          if (performance.now() - t0 > 15000) throw new Failure("connexion encadrant : délai dépassé");
          await sleep(100);
        }
        const expectedMode = modeText(await access.state());
        await until("ligne du mode encadrant et de son édition", () => $("#mode-line").textContent === expectedMode, 3000);
        return { mode: "instructor", code_field_empty: $<HTMLInputElement>("#code-input").value === "" };
      };
      const logout = async () => {
        await click("#mode-btn");
        await untilAccess("mode étudiant", (a) => a.mode === "student", 5000);
        await until("ligne « Mode étudiant »", () => $("#mode-line").textContent === "Mode étudiant", 3000);
        return { mode: "student" };
      };
      const traffic = async () => {
        const ticks0 = store.stats?.tick_results ?? 0;
        ids.clear();
        await sleep(3000);
        const st = store.stats!;
        const tickSlots = store.slots.filter((x) => x.source === "tick").length;
        const detail = {
          frames_sent: st.frames_sent, results: st.results, lost: st.lost, nacks: st.nacks, identifiers: ids.size,
          tick_results: st.tick_results - ticks0, slots_from_ticks: tickSlots,
        };
        if (!st.frames_sent || !st.results) throw new Failure(`aucune trame livrée : ${JSON.stringify(detail)}`);
        if (st.lost || st.nacks) throw new Failure(`${st.lost} trame(s) perdue(s), ${st.nacks} NACK`);
        if (ids.size < 17) throw new Failure(`${ids.size} identifiants distincts livrés, 17 au moins attendus`);
        if (detail.tick_results < 200) throw new Failure(`${detail.tick_results} TICK_RESULT en 3 s`);
        if (cfg.expect_verdict === "pass" && tickSlots === 0) throw new Failure("aucun slot mis à jour par les ticks");
        return detail;
      };
      const session = async () => {
        await click("#btn-start");
        await until("session active", active, 5000);
        watchBoard();
        return { session: sessionId().toString(16).toUpperCase(), tick_period_ms: status().session?.tick_period_ms };
      };
      /** Runs the nominal check and waits until its final message is shown. */
      const nominal = async (finalText: (shown: string) => boolean) => {
        await click('.activity button[data-show-view="cockpit"]');
        await until("cockpit affiché", () => !$('.workspace[data-view="cockpit"]').hidden, 3000);
        await click("#mission-check");
        const started = await untilMission("essai démarré", (m) => m.check.state !== "idle", 5000);
        const total = started.check.total_s || 9;
        const v = await untilMission("fin de l'essai", (m) => ["done", "recorded", "cancelled"].includes(m.check.state), (total + 30) * 1000);
        // The panel follows the core through its own poll: wait for the final text, not just any text (the running
        // message « Essai en cours — 0 s » can still be shown when the core already has the result).
        await until("message final affiché", () => !$("#mission-result").hidden && finalText($("#mission-result-text").textContent ?? ""), 5000);
        return v;
      };
      const stopAndClose = async () => {
        if (status().session) {
          await click("#btn-stop");
          await until("session fermée", () => status().session === null, 5000);
        }
        await click(student ? "#student-close" : "#mission-close");
        await untilMission("mission fermée", (m) => !m.open, 10000);
        return {};
      };

      if (played.board) await step("connexion", connect);
      if (!student) {
        await step("mode-encadrant", login);
        await step("ouverture-mission", async () => {
          const sel = $<HTMLSelectElement>("#mission-select");
          const option = () => Array.from(sel.options).find((o) => o.value === played.id);
          await until(`mission ${played.label} dans la liste`, () => option() !== undefined && !option()!.disabled, 5000).catch(() => {
            throw new Failure(`mission ${played.label} indisponible : ${option()?.title ?? "absente de la liste"}`);
          });
          set("#mission-select", played.id);
          await click("#mission-open");
          const v = await untilMission("mission ouverte", (m) => m.open && m.id === played.id && m.kind === "reference", 10000);
          await until(`panneau Mission ${played.label} affiché`, () => !$("#mission-panel").hidden && $("#stim-section").hidden, 3000);
          return { mission: v.id, title: v.title, slots: v.statement?.slots.length, library: v.library.length, tick_period_ms: v.tick_period_ms };
        });
        await step("session", session);
        await step("trafic", traffic);
        await step("verification-affichage", () => verify("verification-affichage", false));
        await shot("mission-bus");
        await step("vue-banc", async () => {
          await click('.activity button[data-show-view="phys"]');
          await until("vue Physical Lab affichée", () => !$("#phys-main").hidden, 3000);
          await sleep(1500);
          // The bench banner names the mission really open (it once said APPS for every bench).
          const banner = $("#phys-mission");
          const want = benchBanner(await mission.state());
          await until("bandeau du banc de la mission", () => !banner.hidden && banner.textContent === want, 3000).catch(() => {
            throw new Failure(`bandeau « ${banner.textContent} »${banner.hidden ? " masqué" : ""}, attendu « ${want} »`);
          });
          return { banner: banner.textContent };
        });
        await shot("mission-bench");
        await step("essai-nominal", async () => {
          const v = await nominal((t) => t.startsWith("Essai nominal") || t.startsWith("Vérification interrompue"));
          report.mission = {
            state: v.check.state, verdict: v.check.verdict ?? "cancelled", reason: v.check.reason, total_s: v.check.total_s,
            shown: $("#mission-result-text").textContent,
          };
          return report.mission;
        });
        await shot("mission-verdict");
        await step("verdict", async () => {
          const m = report.mission as { state: string; verdict: string; reason: string | null; shown: string };
          if (m.state === "cancelled") throw new Failure(`vérification interrompue (${m.reason}) : « ${m.shown} »`);
          const want = m.verdict === "pass" ? MESSAGES.pass : MESSAGES.fail;
          if (m.shown !== want) throw new Failure(`message affiché « ${m.shown} », attendu « ${want} »`);
          if (cfg.expect_verdict !== "any" && m.verdict !== cfg.expect_verdict) {
            throw new Failure(`verdict ${m.verdict}, attendu ${cfg.expect_verdict}`);
          }
          return { verdict: m.verdict, expected: cfg.expect_verdict };
        });
        await step("sans-redemarrage", noReboot);
        await step("arret", stopAndClose);
        await step("fin-mode-encadrant", logout);
      } else {
        const stage = played.progression ? cfg.stage ?? "full" : "full";
        const openMission = async () => {
          set("#student-code", cfg.group_code!);
          await click("#student-open");
          const v = await untilMission("mission ouverte avec le pack du groupe", (m) => m.open && m.id === played.id && m.kind === "group", 20000)
            .catch((err) => {
              throw new Failure(`${String(err)} « ${$("#mission-error").textContent} »`);
            });
          await until(`panneau Mission ${played.label} affiché`, () => !$("#mission-panel").hidden && $("#stim-section").hidden, 3000);
          if ($<HTMLInputElement>("#student-code").value !== "") throw new Failure("le code du groupe reste dans le champ");
          if (played.progression) {
            // a release 0.3 pack verifies through its progression only
            await until("panneau de progression affiché", () => !$("#progress-section").hidden && $("#mission-check").hidden, 5000);
          } else if ($("#mission-check").textContent !== "Enregistrer l'essai nominal") throw new Failure(`bouton « ${$("#mission-check").textContent} »`);
          return { group: v.group, slots: v.statement?.slots.length, tick_period_ms: v.tick_period_ms };
        };
        if (stage !== "resume") {
          await step("reinitialisation", async () => {
            const before = await access.state();
            await login();
            await click("#instructor-reset");
            await until("confirmation de la réinitialisation", () => $("#instructor-reset").classList.contains("btn--danger"), 2000);
            await click("#instructor-reset");
            await untilAccess("verrou et pack effacés", (a) => a.mode === "instructor" && a.student.locked === null && a.student.pack === null, 5000);
            await logout();
            return { locked_before: before.student.locked, pack_before: before.student.pack !== null };
          });
          await step("choix-mission", async () => {
            await until("missions de la session listées", () => $<HTMLSelectElement>("#student-mission").options.length === 4, 5000);
            set("#student-mission", played.id);
            await click("#student-choose");
            await until("confirmation affichée", () => !$("#student-confirm").hidden, 2000);
            const shown = $("#student-confirm-text").textContent;
            const want = CONFIRM_CHOICE;
            if (shown !== want) throw new Failure(`confirmation « ${shown} », attendue « ${want} »`);
            await click("#student-confirm-yes");
            const a = await untilAccess("mission verrouillée", (x) => x.student.locked === played.id, 5000);
            await until("mission verrouillée affichée", () => !$("#student-locked").hidden && $("#student-choice").hidden, 3000);
            return { locked: a.student.locked, title: $("#student-locked-title").textContent };
          });
          await step("autre-mission-refusee", async () => {
            const want = `La mission de cette session est déjà choisie : ${played.label}.`;
            const refused = async (what: string, run: () => Promise<unknown>) => {
              try {
                await run();
              } catch (err) {
                const got = String(err);
                if (got !== want) throw new Failure(`${what} : refus « ${got} », attendu « ${want} »`);
                return got;
              }
              throw new Failure(`${what} accepté(e) après le verrou`);
            };
            const [other, another] = played.others;
            const open = await refused(`ouverture de ${other.toUpperCase()}`, () => mission.open(other));
            const choose = await refused(`choix de ${another.toUpperCase()}`, () => access.choose(another));
            if ((await mission.state()).open) throw new Failure("une mission s'est ouverte");
            return { open, choose, refused_open: other, refused_choice: another };
          });
          await step("import-pack", async () => {
            if (!cfg.group_code) throw new Failure("N7RT_E2E_GROUP_CODE non défini");
            const b64 = await e2e.pack();
            const bytes = Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
            // The real control: a file given to the file input, as the file picker does.
            const input = $<HTMLInputElement>("#student-pack-file");
            const dt = new DataTransfer();
            dt.items.add(new File([bytes], cfg.pack_name ?? "pack.n7rtpack"));
            input.files = dt.files;
            input.dispatchEvent(new Event("change", { bubbles: true }));
            const a = await untilAccess("pack importé", (x) => x.student.pack !== null, 10000);
            await until("pack affiché", () => ($("#student-pack").textContent ?? "").startsWith(a.student.pack!.group), 3000);
            return { bytes: bytes.length, cohort: a.student.pack!.cohort, group: a.student.pack!.group };
          });
        }
        if (played.progression && stage !== "resume") {
          await shot("01-ouvrir", ["#student-locked", "#student-locked-title", "#student-import", "#student-code", "#student-open"]);
        }
        await step("ouverture-mission", openMission);
        await shot("student-locked");
        if (played.board) await step("session", session);
        if (!played.board) {
          await step("progression-initiale", async () => {
            const v = await untilMission("progression du pack", (m) => !!m.progression?.readable, 10000);
            const p = v.progression!;
            const states = p.steps.map((s) => s.state);
            if (p.current !== 0 || JSON.stringify(states) !== JSON.stringify(["EN COURS", "À FAIRE", "À FAIRE", "À FAIRE", "À FAIRE"])) {
              throw new Failure(`progression initiale ${JSON.stringify(states)} (étape ${p.current})`);
            }
            if (p.ready || p.incident !== null || !p.documents.length) throw new Failure("progression initiale inattendue");
            await until("panneau de progression affiché", () => !$("#progress-section").hidden && !$("#progress-blocked").hidden, 3000);
            return { steps: states, documents: p.documents.length, button: p.button };
          });
          await step("arret", stopAndClose);
        } else if (played.progression) {
          await step("trafic", traffic);
          await step("verification-affichage", () => verify("verification-affichage", false));
          const { runProgression } = await import("./e2e-progression");
          await runProgression({
            cfg, store, id: played.id, step, click, set, until, untilMission, shot, watchBoard, noReboot,
            rows: () => kept,
            openMission: () => step("reouverture-mission", openMission),
            report,
          });
          if (stage === "until-incident") {
            // the student closes the application in the middle of the incident: mission open, vehicle running
            await step("sans-redemarrage", noReboot);
          } else {
            await step("arret", stopAndClose);
          }
        } else {
          await step("trafic", traffic);
          await step("verification-affichage", () => verify("verification-affichage", false));
          await step("essai-nominal", async () => {
            const v = await nominal((t) => t.startsWith("Essai enregistré") || t.startsWith("Vérification interrompue"));
            const shown = $("#mission-result-text").textContent ?? "";
            report.mission = {
              state: v.check.state, run_file: v.check.run_file, reason: v.check.reason, total_s: v.check.total_s, shown,
            };
            report.run_file = await e2e.runFile();
            return report.mission;
          });
          await shot("student-recorded");
          await step("fichier-essai", async () => {
            const m = report.mission as { state: string; run_file: string | null; reason: string | null; shown: string };
            if (m.state !== "recorded") throw new Failure(`essai non enregistré (${m.reason ?? m.state}) : « ${m.shown} »`);
            const want = MESSAGES.recorded(m.run_file ?? "");
            if (m.shown !== want) throw new Failure(`message affiché « ${m.shown} », attendu « ${want} »`);
            if (/réussi|Essai nominal :/.test(m.shown)) throw new Failure("un verdict est montré à l'étudiant");
            if ($("#mission-runs-folder").hidden) throw new Failure("bouton « Ouvrir le dossier » absent");
            const path = report.run_file as string | null;
            if (!path || !m.run_file || !path.endsWith(m.run_file)) throw new Failure(`fichier d'essai ${path} ≠ ${m.run_file}`);
            return { run_file: path };
          });
          await step("sans-redemarrage", noReboot);
          await step("arret", stopAndClose);
        }
      }
    } else {
      throw new Failure(`scénario inconnu : ${cfg.scenario}`);
    }
    ok = steps.every((s) => s.ok);
  } catch (err) {
    report.error = String(err);
  }
  report.ui = report.ui ?? perf.summary();
  report.events = store.errors.map((e) => `${(e.at_ms / 1000).toFixed(2)} s [${e.kind}] ${e.text}`);
  report.ok = ok;
  report.finished_at = new Date().toISOString();
  await e2e.progress(ok ? "done" : "failed");
  await e2e.finish(report, ok);
}
