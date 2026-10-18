// Mission panel (views/mission.ts): rendering from a MissionView, exact ch. 24 §7.3 texts, unavailable missions
// disabled, text only (an innerHTML write throws in this fake DOM). Node has no DOM: a few elements and a document.

import cockpitSrc from "./cockpit.ts?raw";
import missionSrc from "./mission.ts?raw";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import type { CheckView, MissionEntry, MissionView, Status } from "../types";
import {
  BOARD_NOT_READY, CHECK_LABEL, COVER, MESSAGES, MissionPanel, TRAINING, TRAINING_LABEL, benchBanner, boardReady, checkMessage, libraryMeta,
  missionOptions, remainingSeconds, slotLabels, slotName, valueWithUnit, type MissionActions, type MissionElements,
} from "./mission";

type Listener = (e: { target: FakeNode }) => void;

class FakeNode {
  children: FakeNode[] = [];
  parent: FakeNode | null = null;
  className = "";
  hidden = false;
  disabled = false;
  title = "";
  type = "";
  dataset: Record<string, string> = {};
  style = { width: "" };
  private text = "";
  private listeners: Record<string, Listener[]> = {};
  protected own = "";
  constructor(readonly tagName: string) {}
  get value(): string {
    return this.own;
  }
  set value(v: string) {
    this.own = v;
  }
  get firstChild(): FakeNode | null {
    return this.children[0] ?? null;
  }
  appendChild(c: FakeNode): FakeNode {
    c.parent = this;
    this.children.push(c);
    return c;
  }
  removeChild(c: FakeNode): FakeNode {
    this.children.splice(this.children.indexOf(c), 1);
    c.parent = null;
    return c;
  }
  get textContent(): string {
    return this.children.length ? this.children.map((c) => c.textContent).join("") : this.text;
  }
  set textContent(v: string) {
    this.children = [];
    this.text = v;
  }
  set innerHTML(_v: string) {
    throw new Error("innerHTML is forbidden for dynamic data");
  }
  addEventListener(type: string, fn: Listener): void {
    (this.listeners[type] ??= []).push(fn);
  }
  /** Dispatches from `target` up to this node's listeners (bubbling as far as this test needs). */
  fire(type: string, target: FakeNode = this): void {
    for (let n: FakeNode | null = target; n; n = n.parent) for (const fn of n.listeners[type] ?? []) fn({ target });
  }
  closest(tag: string): FakeNode | null {
    for (let n: FakeNode | null = this; n; n = n.parent) if (n.tagName === tag) return n;
    return null;
  }
  all(tag: string): FakeNode[] {
    return this.children.flatMap((c) => [...(c.tagName === tag ? [c] : []), ...c.all(tag)]);
  }
}

/** A select: its value is the selected option's (the first one when none matches), as in the DOM. */
class FakeSelect extends FakeNode {
  get options(): FakeNode[] {
    return this.children;
  }
  get value(): string {
    const o = this.children.find((c) => c.value === this.own);
    return o ? o.value : this.children[0]?.value ?? "";
  }
  set value(v: string) {
    this.own = v;
  }
}

const fakeDocument = {
  createElement: (tag: string) => (tag === "select" ? new FakeSelect(tag) : new FakeNode(tag)),
};
let saved: unknown;

beforeAll(() => {
  saved = (globalThis as { document?: unknown }).document;
  (globalThis as { document?: unknown }).document = fakeDocument;
});

afterAll(() => {
  (globalThis as { document?: unknown }).document = saved;
});

const check = (over: Partial<CheckView> = {}): CheckView => ({
  state: "idle", elapsed_s: 0, total_s: 0, verdict: null, reason: null, run_file: null, ...over,
});

const LIST: MissionEntry[] = [
  { id: "apps", title: "APPS — pédale d'accélérateur", available: true, reason: null },
  { id: "wsf", title: "WSF — vitesse des roues", available: false, reason: "mission pas encore disponible dans cette version" },
];

const OPEN: MissionView = {
  id: "apps",
  title: "APPS — pédale d'accélérateur",
  open: true,
  kind: "reference",
  group: null,
  statement: {
    summary: "Résumé public.",
    requirements: ["Deux capteurs mesurent la course de la pédale.", "Coupure si l'implausibilité dure plus de 100 ms."],
    slots: [
      { slot: 0, code: "A1", label: "Course de la pédale vue par le capteur 1", unit: "%" },
      { slot: 3, code: "A4", label: "Implausibilité APPS active", unit: "0/1" },
    ],
  },
  library: [
    { title: "Accelerator Pedal Assembly", publisher: "CTS Corporation", reference: "Series 552", revision: "RevB_0524", date: "2024-05" },
    { title: "Formula Student Rules 2026", publisher: "FSG", reference: "Rev-15f99a7", revision: "Version 1.1", date: null },
  ],
  check: check(),
  tick_period_ms: 10,
  progression: null,
};

const CLOSED: MissionView = {
  id: null, title: null, open: false, kind: null, group: null, statement: null, library: [], check: check(), tick_period_ms: null, progression: null,
};
/** The same mission opened by a student with the group pack. */
const GROUP: MissionView = { ...OPEN, kind: "group", group: "G01" };

function readyStatus(over: Partial<Status> = {}): Status {
  return {
    state: "SESSION", stalled: false, student_busy: false, message: "", target: null,
    port: { port: "COM5", vid: null, pid: null, serial: "SN", product: null, probe: "STLINK-V3" },
    ports: [], session: null, heartbeat: null, fault: null,
    stimuli: { pot: 0, button: false, format: "classic", can_id: 0, dlc: 0, rate_hz: 0 }, stim_rev: 0, can_start: false, can_stop: true,
    board: {
      runtime: "", build_id: "", abi: "", proto: 0, uid: "", dev_id: 0, rev_id: 0, reset_flags: 0, reset_causes: [],
      student_status: 1, student_status_text: "", student_abi: "", student_crc32: "", rx_window: 32, fault_pending: false,
      runtime_verified: true, compatible: true, problems: [],
    },
    ...over,
  };
}

function elements(): Record<keyof MissionElements, FakeNode> {
  const n = (tag = "div") => new FakeNode(tag);
  return {
    select: new FakeSelect("select"), open: n("button"), close: n("button"), reset: n("button"), error: n("p"), stimuli: n("details"),
    section: n("details"), title: n("summary"), summary: n("p"), requirements: n("ul"), outputs: n("ul"), library: n("ul"),
    check: n("button"), result: n("div"), resultText: n("p"), folder: n("button"), progress: n("div"), bar: n("i"),
    frames: n(), framesCover: n(), framesCoverText: n("p"), framesCoverBar: n("i"), slots: n(), slotsCover: n("p"),
    progressRun: n(), progressRunBar: n("i"),
  };
}

describe("mission panel", () => {
  let e: Record<keyof MissionElements, FakeNode>;
  let calls: string[];
  let panel: MissionPanel;

  beforeEach(() => {
    e = elements();
    calls = [];
    const actions: MissionActions = {
      open: (id) => calls.push(`open:${id}`),
      close: () => calls.push("close"),
      reset: () => calls.push("reset"),
      check: () => calls.push("check"),
      openDocument: (i) => calls.push(`doc:${i}`),
      openRunsFolder: () => calls.push("folder"),
    };
    panel = new MissionPanel(e as unknown as MissionElements, actions);
    panel.mount();
    panel.setList(LIST);
  });

  it("opens each of the four available instructor missions and permits bench reset only for references", () => {
    const list = ["apps", "wsf", "sas", "bps"].map((id) => ({ id, title: id.toUpperCase(), available: true, reason: null }));
    panel.setList(list);
    for (const id of list.map((m) => m.id)) {
      panel.setView(CLOSED);
      e.select.value = id;
      e.select.fire("change");
      expect(e.open.disabled).toBe(false);
      e.open.fire("click");
      expect(calls.at(-1)).toBe(`open:${id}`);
      panel.setView({ ...OPEN, id });
      expect(e.reset.hidden || e.reset.disabled).toBe(false);
      e.reset.fire("click");
      expect(calls.at(-1)).toBe("reset");
    }
    panel.setBusy(true);
    const before = calls.length;
    e.reset.fire("click");
    expect(calls).toHaveLength(before);
    panel.setBusy(false);
    panel.setView(GROUP);
    expect(e.reset.hidden && e.reset.disabled).toBe(true);
    e.reset.fire("click");
    expect(calls).toHaveLength(before);
    panel.setView({ ...OPEN, check: { ...OPEN.check, state: "running" } });
    e.reset.fire("click");
    expect(calls).toHaveLength(before);
  });

  it("lists the training cockpit first and disables an unavailable mission with its reason", () => {
    const opts = (e.select as unknown as FakeSelect).options;
    expect(opts.map((o) => [o.value, o.textContent, o.disabled])).toEqual([
      [TRAINING, TRAINING_LABEL, false],
      ["apps", "APPS — pédale d'accélérateur", false],
      ["wsf", "WSF — vitesse des roues", true],
    ]);
    expect(opts[2].title).toBe("Indisponible : mission pas encore disponible dans cette version");
    expect(opts[1].title).toBe("");
    // Nothing open: the training stimuli stay, Ouvrir waits for a mission choice, Fermer has nothing to close.
    panel.setView(CLOSED);
    expect([e.stimuli.hidden, e.section.hidden]).toEqual([false, true]);
    expect([e.open.disabled, e.close.disabled]).toEqual([true, true]);
    e.select.value = "apps";
    e.select.fire("change");
    expect(e.open.disabled).toBe(false);
    e.open.fire("click");
    expect(calls).toEqual(["open:apps"]);
  });

  it("replaces the stimuli by the statement and the library of the open mission", () => {
    panel.setView(OPEN);
    expect([e.stimuli.hidden, e.section.hidden]).toEqual([true, false]);
    expect(e.title.textContent).toBe("Mission APPS");
    expect(e.summary.textContent).toBe("Résumé public.");
    expect(e.requirements.all("li").map((li) => li.textContent)).toEqual(OPEN.statement!.requirements);
    // every output with the slot the student code publishes, before anything is published
    expect(e.outputs.all("li").map((li) => li.textContent)).toEqual([
      "Slot 0 — A1 · Course de la pédale vue par le capteur 1 (%)", "Slot 3 — A4 · Implausibilité APPS active (0/1)"]);
    const docs = e.library.all("button");
    expect(docs.map((b) => b.dataset.index)).toEqual(["0", "1"]);
    expect(docs.map((b) => b.className)).toEqual(["lib-doc", "lib-doc"]);
    expect(docs[0].children.map((c) => c.textContent)).toEqual(["Accelerator Pedal Assembly", "CTS Corporation · Series 552 · RevB_0524 · 2024-05"]);
    expect(docs[1].children[1].textContent).toBe("FSG · Rev-15f99a7 · Version 1.1");
    // A click anywhere in an entry opens that document.
    e.library.fire("click", docs[1].children[0]);
    expect(calls).toEqual(["doc:1"]);
    expect(e.select.value).toBe("apps");
    expect([e.open.disabled, e.close.disabled]).toEqual([true, false]);
    e.close.fire("click");
    expect(calls).toEqual(["doc:1", "close"]);
  });

  it("lets the nominal check start only with a ready board", () => {
    panel.setView(OPEN);
    expect(e.check.disabled).toBe(true);
    expect(e.check.title).toBe(BOARD_NOT_READY);
    panel.setBoard(readyStatus());
    expect(e.check.disabled).toBe(false);
    e.check.fire("click");
    expect(calls).toEqual(["check"]);
    panel.setBoard(readyStatus({ stalled: true }));
    expect(e.check.disabled).toBe(true);
    panel.setBoard(readyStatus());
    panel.setCheckError(BOARD_NOT_READY);
    expect([e.resultText.textContent, e.result.className, e.result.hidden]).toEqual([BOARD_NOT_READY, "mission-result warn", false]);
  });

  it("covers the bus and the slots during a check, then shows the exact verdict", () => {
    panel.setBoard(readyStatus());
    panel.setView({ ...OPEN, check: check({ state: "running", elapsed_s: 2.2, total_s: 9 }) });
    const running = "Essai en cours — 7 s. Ne débranchez pas la carte et ne fermez pas l'application.";
    expect(e.resultText.textContent).toBe(running);
    expect([e.frames.hidden, e.framesCover.hidden, e.slots.hidden, e.slotsCover.hidden]).toEqual([true, false, true, false]);
    expect([e.framesCoverText.textContent, e.slotsCover.textContent]).toEqual([running, running]);
    expect([e.progress.hidden, e.bar.style.width, e.framesCoverBar.style.width]).toEqual([false, "24.4%", "24.4%"]);
    // the run bar of the progression's action bar, at the same proportion
    expect([e.progressRun.hidden, e.progressRunBar.style.width]).toEqual([false, "24.4%"]);
    expect([e.select.disabled, e.open.disabled, e.close.disabled, e.check.disabled]).toEqual([true, true, true, true]);

    panel.setView({ ...OPEN, check: check({ state: "done", elapsed_s: 9, total_s: 9, verdict: "pass" }) });
    expect([e.resultText.textContent, e.result.className]).toEqual(["Essai nominal : réussi", "mission-result ok"]);
    expect([e.frames.hidden, e.framesCover.hidden, e.slots.hidden, e.slotsCover.hidden, e.progress.hidden]).toEqual([false, true, false, true, true]);
    expect(e.progressRun.hidden).toBe(true);
    expect(e.check.disabled).toBe(false);

    panel.setView({ ...OPEN, check: check({ state: "done", elapsed_s: 9, total_s: 9, verdict: "fail" }) });
    expect(e.resultText.textContent).toBe("Essai nominal : non réussi. Le détail n'est pas communiqué. Reprenez vos essais.");
    expect(e.result.className).toBe("mission-result err");

    panel.setView({ ...OPEN, check: check({ state: "cancelled", total_s: 9, reason: "carte déconnectée" }) });
    expect(e.resultText.textContent).toBe(
      "Vérification interrompue (carte déconnectée). Elle n'est pas comptée. Relancez quand le banc est prêt.",
    );
    panel.setView(CLOSED);
    expect([e.stimuli.hidden, e.section.hidden, e.result.hidden]).toEqual([false, true, true]);
    expect(e.select.value).toBe(TRAINING);
  });

  it("says the countdown once for a progression: the covers only say what is hidden until the verdict", () => {
    panel.setBoard(readyStatus());
    const running = check({ state: "running", elapsed_s: 2.2, total_s: 9 });
    const progression = (current: number) => ({ current }) as unknown as NonNullable<MissionView["progression"]>;
    panel.setView({ ...GROUP, check: running, progression: progression(1) });
    expect([e.framesCover.hidden, e.slotsCover.hidden]).toEqual([false, false]);
    expect([e.framesCoverText.textContent, e.slotsCover.textContent]).toEqual([COVER.bus(false), COVER.slots(false)]);
    expect(COVER.bus(false)).toBe("Essai en cours : la vue du bus est masquée jusqu'au verdict.");
    panel.setView({ ...GROUP, check: running, progression: progression(4) });
    expect(e.framesCoverText.textContent).toBe("Qualification en cours : la vue du bus est masquée jusqu'au verdict.");
    expect(e.framesCoverBar.style.width).toBe("24.4%");
  });

  it("records a student run: the run file name, the folder button, never a verdict", () => {
    panel.setBoard(readyStatus());
    panel.setView(OPEN);
    expect(e.check.textContent).toBe("Vérifier l'essai nominal");
    panel.setView(GROUP);
    expect(e.title.textContent).toBe("Mission APPS — groupe G01");
    expect(e.check.textContent).toBe(CHECK_LABEL.group);
    expect(e.check.title).toBe("Joue l'essai nominal sur votre carte et enregistre le fichier d'essai à transmettre à votre encadrant.");
    expect(e.folder.hidden).toBe(true);
    panel.setView({ ...GROUP, check: check({ state: "running", elapsed_s: 1, total_s: 9 }) });
    expect(e.resultText.textContent).toBe("Essai en cours — 8 s. Ne débranchez pas la carte et ne fermez pas l'application.");
    const name = "essai-apps-G01-20261006-142233.n7rtrun";
    panel.setView({ ...GROUP, check: check({ state: "recorded", elapsed_s: 9, total_s: 9, run_file: name }) });
    expect(e.resultText.textContent).toBe(`Essai enregistré : ${name}. Transmettez ce fichier à votre encadrant.`);
    expect([e.result.className, e.result.hidden, e.folder.hidden]).toEqual(["mission-result ok", false, false]);
    expect(e.resultText.textContent).not.toMatch(/réussi|Essai nominal/);
    e.folder.fire("click");
    expect(calls).toEqual(["folder"]);
    panel.setView({ ...GROUP, check: check({ state: "cancelled", total_s: 9, reason: "fichier d'essai non enregistré" }) });
    expect(e.folder.hidden).toBe(true);
    expect(e.resultText.textContent).toBe(MESSAGES.cancelled("fichier d'essai non enregistré"));
  });

  it("writes text only: the panel source has no HTML sink", () => {
    // Spelled in pieces: the source guard (scripts/check-ui.mjs) reads this file too.
    const sinks = new RegExp(["inner" + "HTML", "outer" + "HTML", "insertAdjacent" + "HTML", "document" + "\\.write"].join("|"));
    for (const src of [missionSrc, cockpitSrc]) {
      expect(src.length).toBeGreaterThan(1000);
      expect(src).not.toMatch(sinks);
    }
  });
});

describe("mission helpers", () => {
  it("names the open mission in the Physical Lab bench banner, never a fixed one", () => {
    const tail = " — mode mission : les grandeurs internes ne sont pas affichées.";
    for (const id of ["apps", "wsf", "sas", "bps"]) {
      expect(benchBanner({ ...OPEN, id })).toBe(`Banc de la mission ${id.toUpperCase()}${tail}`);
      expect(benchBanner({ ...GROUP, id })).toBe(`Banc de la mission ${id.toUpperCase()}${tail}`);
    }
    expect(benchBanner(CLOSED)).toBe(`Banc de la mission${tail}`);
    expect(benchBanner(null)).toBe(`Banc de la mission${tail}`);
  });

  it("formats the ch. 24 §7.3 messages", () => {
    expect(MESSAGES.running(9)).toBe("Essai en cours — 9 s. Ne débranchez pas la carte et ne fermez pas l'application.");
    expect(checkMessage(check())).toEqual({ text: "", tone: "" });
    expect(checkMessage(check({ state: "cancelled" })).text).toBe(MESSAGES.cancelled("essai incomplet"));
    expect(checkMessage(check({ state: "recorded", run_file: "essai-apps-G01-x.n7rtrun" }))).toEqual({
      text: "Essai enregistré : essai-apps-G01-x.n7rtrun. Transmettez ce fichier à votre encadrant.", tone: "ok",
    });
    expect(remainingSeconds(check({ state: "running", elapsed_s: 9, total_s: 9 }))).toBe(0);
    expect(remainingSeconds(check({ state: "running", elapsed_s: 0.05, total_s: 9 }))).toBe(9);
    expect(remainingSeconds(check({ state: "running", elapsed_s: 4, total_s: 9 }))).toBe(5);
  });

  it("names the slots of an open mission only", () => {
    expect(slotLabels(CLOSED).size).toBe(0);
    const labels = slotLabels(OPEN);
    expect(slotName(labels.get(0)!)).toBe("A1 · Course de la pédale vue par le capteur 1");
    expect(labels.get(3)!.code).toBe("A4");
    expect(valueWithUnit("42.5", "%")).toBe("42.5 %");
    expect(valueWithUnit("1", "0/1")).toBe("1");
    expect(libraryMeta({ title: "t", publisher: "P", reference: "R", revision: "1", date: null })).toBe("P · R · 1");
  });

  it("keeps the order of the core and the training entry first", () => {
    expect(missionOptions([]).map((o) => o.value)).toEqual([TRAINING]);
    expect(missionOptions(LIST).map((o) => o.disabled)).toEqual([false, false, true]);
  });

  it("knows when a board can run a check", () => {
    expect(boardReady(null)).toBe(false);
    expect(boardReady(readyStatus())).toBe(true);
    expect(boardReady(readyStatus({ state: "READY" }))).toBe(true);
    expect(boardReady(readyStatus({ port: null }))).toBe(false);
    expect(boardReady(readyStatus({ state: "LOST" }))).toBe(false);
    expect(boardReady(readyStatus({ board: { ...readyStatus().board!, student_status: 0 } }))).toBe(false);
  });
});
