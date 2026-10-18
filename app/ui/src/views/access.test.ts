// Modes (views/access.ts, docs/distribution.md §2): the mode line, the instructor code dialog, the student choice with
// its confirmation, the pack import and the group code, the instructor tools. Text only (an innerHTML write throws in
// this fake DOM). Node has no DOM: a few elements and a document.

import accessSrc from "./access.ts?raw";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import type { AccessView, GenRow, Judgement, MissionEntry, MissionView } from "../types";
import {
  AccessPanel, CONFIRM_CHOICE, LEAVE_INSTRUCTOR, MAX_PACK_BYTES, MAX_RUN_BYTES, MODE_LABEL, NO_PACK, NOTHING_OPEN, PACK_OPEN, PACK_TOO_LARGE,
  PUBLIC_EDITION, RESET_CONFIRM, RESET_LABEL, RUN_TOO_LARGE, bytesToBase64, judgementMeta, judgementText, modeText,
  packLabel, parseGroups, type AccessActions, type AccessElements,
} from "./access";

type Listener = (e: Record<string, unknown>) => void;

class FakeNode {
  children: FakeNode[] = [];
  parent: FakeNode | null = null;
  className = "";
  hidden = false;
  disabled = false;
  title = "";
  type = "";
  open = false;
  checked = false;
  files: Blob[] | null = null;
  clicks = 0;
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
  fire(type: string, extra: Record<string, unknown> = {}): void {
    for (const fn of this.listeners[type] ?? []) fn({ target: this, ...extra });
  }
  focus(): void {}
  click(): void {
    this.clicks += 1;
  }
  showModal(): void {
    this.open = true;
  }
  close(): void {
    this.open = false;
  }
  all(tag: string): FakeNode[] {
    return this.children.flatMap((c) => [...(c.tagName === tag ? [c] : []), ...c.all(tag)]);
  }
}

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

const fakeDocument = { createElement: (tag: string) => (tag === "select" ? new FakeSelect(tag) : new FakeNode(tag)) };
let saved: unknown;

beforeAll(() => {
  saved = (globalThis as { document?: unknown }).document;
  (globalThis as { document?: unknown }).document = fakeDocument;
});

afterAll(() => {
  (globalThis as { document?: unknown }).document = saved;
});

const flush = async () => {
  for (let i = 0; i < 5; i++) await new Promise((r) => setTimeout(r, 0));
};

const LIST: MissionEntry[] = [
  { id: "apps", title: "APPS — pédale d'accélérateur", available: true, reason: null },
  { id: "wsf", title: "WSF — vitesse des roues", available: false, reason: "mission pas encore disponible dans cette version" },
  { id: "sas", title: "SAS — angle du volant", available: false, reason: "mission pas encore disponible dans cette version" },
  { id: "bps", title: "BPS — pression de freinage", available: false, reason: "mission pas encore disponible dans cette version" },
];

const STUDENT: AccessView = {
  mode: "student", student: { locked: null, locked_title: null, pack: null, last_run: null }, edition: "édition encadrant", oracle: true,
};
const LOCKED: AccessView = { ...STUDENT, student: { ...STUDENT.student, locked: "apps", locked_title: "APPS — pédale d'accélérateur" } };
const WITH_PACK: AccessView = { ...STUDENT, student: { ...LOCKED.student, pack: { cohort: "promo-2026", group: "G01" } } };
const INSTRUCTOR: AccessView = { ...STUDENT, mode: "instructor" };

const idle = { state: "idle", elapsed_s: 0, total_s: 0, verdict: null, reason: null, run_file: null } as const;
const CLOSED: MissionView = {
  id: null, title: null, open: false, kind: null, group: null, statement: null, library: [], check: { ...idle }, tick_period_ms: null, progression: null,
};
const GROUP_OPEN: MissionView = { ...CLOSED, id: "apps", title: "APPS", open: true, kind: "group", group: "G01", tick_period_ms: 10 };

function elements(): Record<keyof AccessElements, FakeNode> {
  const n = (tag = "div") => new FakeNode(tag);
  return {
    modeLine: n("span"), modeButton: n("button"), error: n("p"),
    studentFlow: n(), choice: n(), studentSelect: new FakeSelect("select"), choose: n("button"), confirm: n(),
    confirmText: n("p"), confirmYes: n("button"), confirmNo: n("button"), locked: n(), lockedTitle: n("dd"), pack: n("dd"),
    importButton: n("button"), packFile: n("input"), code: n("input"), open: n("button"), close: n("button"),
    instructorFlow: n(), reset: n("button"), packs: n("button"), judge: n("button"), runFile: n("input"),
    judgeResult: n(), judgeVerdict: n("p"), judgeMeta: n("p"), judgeDetail: n("p"),
    codeDialog: n("dialog"), codeInput: n("input"), codeSubmit: n("button"), codeCancel: n("button"), codeError: n("p"),
    packsDialog: n("dialog"), packsMission: new FakeSelect("select"), packsCohort: n("input"), packsGroups: n("input"),
    packsReplace: n("input"), packsGenerate: n("button"), packsClose: n("button"), packsError: n("p"), packsTable: n(), packsRows: n("tbody"),
  };
}

describe("access panel", () => {
  let e: Record<keyof AccessElements, FakeNode>;
  let calls: string[];
  let panel: AccessPanel;
  let next: { login?: () => Promise<AccessView>; open?: () => Promise<MissionView>; judge?: () => Promise<Judgement> };

  beforeEach(() => {
    e = elements();
    e.reset.className = "btn";
    e.confirm.hidden = true;
    e.judgeResult.hidden = true;
    calls = [];
    next = {};
    const rows: GenRow[] = [
      { group: "G01", code: "7K4Q-M2ZD-9XRT-4WBC", file: "cohorts/promo-2026/apps/packs/G01.n7rtpack" },
      { group: "G02", code: "A1B2-C3D4-E5F6-G7H8", file: "cohorts/promo-2026/apps/packs/G02.n7rtpack" },
    ];
    const actions: AccessActions = {
      login: (code) => (calls.push(`login:${code}`), next.login ? next.login() : Promise.resolve(INSTRUCTOR)),
      logout: () => (calls.push("logout"), Promise.resolve(STUDENT)),
      reset: () => (calls.push("reset"), Promise.resolve({ ...INSTRUCTOR })),
      choose: (id) => (calls.push(`choose:${id}`), Promise.resolve(LOCKED)),
      importPack: (data) => (calls.push(`import:${data}`), Promise.resolve(WITH_PACK)),
      open: (code) => (calls.push(`open:${code}`), next.open ? next.open() : Promise.resolve(GROUP_OPEN)),
      close: () => (calls.push("close"), Promise.resolve(CLOSED)),
      genPacks: (m, cohort, groups, replace) => (calls.push(`gen:${m}:${cohort}:${groups.join("+")}:${replace}`), Promise.resolve(rows)),
      judge: (name, data) => (calls.push(`judge:${name}:${data}`), next.judge!()),
      mission: (v) => calls.push(`mission:${v.open ? v.kind : "closed"}`),
      changed: () => calls.push("changed"),
    };
    panel = new AccessPanel(e as unknown as AccessElements, actions);
    panel.mount();
    panel.setList(LIST);
    panel.setMission(CLOSED);
  });

  it("shows the student mode and opens the code dialog; a wrong code keeps it open and empties the field", async () => {
    panel.setAccess(STUDENT);
    expect([e.modeLine.textContent, e.modeLine.className, e.modeButton.textContent]).toEqual(["Mode étudiant", "badge", "Mode encadrant"]);
    expect([e.studentFlow.hidden, e.instructorFlow.hidden]).toEqual([false, true]);
    e.modeButton.fire("click");
    expect(e.codeDialog.open).toBe(true);
    next.login = () => Promise.reject("Code encadrant incorrect.");
    e.codeInput.value = "BAD-CODE";
    e.codeSubmit.fire("click");
    await flush();
    expect(calls).toEqual(["login:BAD-CODE"]);
    expect([e.codeError.textContent, e.codeDialog.open, e.codeInput.value]).toEqual(["Code encadrant incorrect.", true, ""]);
    // Enter submits; the right code switches the mode and asks the page to read the list again.
    next.login = undefined;
    e.codeInput.value = "GOOD-CODE";
    e.codeInput.fire("keydown", { key: "Enter" });
    await flush();
    expect(calls).toEqual(["login:BAD-CODE", "login:GOOD-CODE", "changed"]);
    expect([e.codeDialog.open, e.codeInput.value]).toEqual([false, ""]);
    expect([e.modeLine.textContent, e.modeLine.className, e.modeButton.textContent]).toEqual(["Mode encadrant", "badge badge--accent", LEAVE_INSTRUCTOR]);
    expect([e.studentFlow.hidden, e.instructorFlow.hidden]).toEqual([true, false]);
    // The same button leaves the instructor mode.
    e.modeButton.fire("click");
    await flush();
    expect(calls.slice(-2)).toEqual(["logout", "changed"]);
    expect(e.modeLine.textContent).toBe("Mode étudiant");
  });

  it("asks for confirmation before locking the session's mission", async () => {
    panel.setAccess(STUDENT);
    const opts = (e.studentSelect as unknown as FakeSelect).options;
    expect(opts.map((o) => [o.value, o.disabled])).toEqual([["apps", false], ["wsf", true], ["sas", true], ["bps", true]]);
    expect(opts[1].title).toBe("Indisponible : mission pas encore disponible dans cette version");
    expect(e.studentSelect.value).toBe("apps");
    expect([e.choice.hidden, e.locked.hidden, e.choose.disabled]).toEqual([false, true, false]);
    e.choose.fire("click");
    expect(e.confirm.hidden).toBe(false);
    expect(CONFIRM_CHOICE).toBe("Ce choix est définitif pour la session : pour en changer, demandez à votre encadrant.");
    expect(e.confirmText.textContent).toBe(CONFIRM_CHOICE);
    expect([e.choose.disabled, e.studentSelect.disabled]).toEqual([true, true]);
    e.confirmNo.fire("click");
    expect([e.confirm.hidden, e.choose.disabled]).toEqual([true, false]);
    expect(calls).toEqual([]);
    e.choose.fire("click");
    e.confirmYes.fire("click");
    await flush();
    expect(calls).toEqual(["choose:apps", "changed"]);
    expect([e.choice.hidden, e.locked.hidden, e.confirm.hidden]).toEqual([true, false, true]);
    expect([e.lockedTitle.textContent, e.pack.textContent]).toEqual(["APPS — pédale d'accélérateur", NO_PACK]);
    expect([e.importButton.disabled, e.code.disabled, e.open.disabled]).toEqual([false, true, true]);
  });

  it("imports the pack as base64 and opens with the group code, which is then emptied", async () => {
    panel.setAccess(LOCKED);
    e.importButton.fire("click");
    expect(e.packFile.clicks).toBe(1);
    e.packFile.files = [new Blob([new Uint8Array([0x4d, 0x61, 0x6e, 0x00, 0xff])])];
    e.packFile.value = "C:\\fakepath\\apps-G01.n7rtpack";
    e.packFile.fire("change");
    await flush();
    expect(calls).toEqual(["import:TWFuAP8="]);
    expect(e.packFile.value).toBe("");
    expect(e.pack.textContent).toBe("G01 — cohorte promo-2026");
    expect([e.code.disabled, e.open.disabled]).toEqual([false, true]);
    // A refused code stays in the field (typo), the core's sentence is shown.
    next.open = () => Promise.reject("Code de groupe incorrect.");
    e.code.value = "7K4Q-M2ZD";
    e.code.fire("input");
    expect(e.open.disabled).toBe(false);
    e.open.fire("click");
    await flush();
    expect([e.error.textContent, e.code.value]).toEqual(["Code de groupe incorrect.", "7K4Q-M2ZD"]);
    next.open = undefined;
    e.code.fire("keydown", { key: "Enter" });
    await flush();
    expect(calls.slice(-2)).toEqual(["open:7K4Q-M2ZD", "mission:group"]);
    expect([e.code.value, e.error.textContent]).toEqual(["", ""]);
    panel.setMission(GROUP_OPEN);
    expect([e.open.disabled, e.close.disabled]).toEqual([true, false]);
    // once open, the setup steps give way to « Fermer » (CSS on is-open); a disabled button says why
    expect(e.locked.className).toBe("mission-block student-locked is-open");
    expect([e.open.title, e.close.title]).toEqual([PACK_OPEN, ""]);
    e.close.fire("click");
    await flush();
    expect(calls.slice(-2)).toEqual(["close", "mission:closed"]);
    panel.setMission(CLOSED);
    expect(e.locked.className).toBe("mission-block student-locked");
    expect([e.close.disabled, e.close.title]).toEqual([true, NOTHING_OPEN]);
    // Nothing moves while a check runs.
    panel.setMission({ ...GROUP_OPEN, check: { ...idle, state: "running", total_s: 9 } });
    expect([e.modeButton.disabled, e.importButton.disabled, e.close.disabled]).toEqual([true, true, true]);
  });

  it("resets only after a second click", async () => {
    panel.setAccess(INSTRUCTOR);
    e.reset.fire("click");
    expect([e.reset.textContent, e.reset.className]).toEqual([RESET_CONFIRM, "btn btn--danger"]);
    expect(calls).toEqual([]);
    e.reset.fire("click");
    await flush();
    expect(calls).toEqual(["reset", "changed"]);
    expect([e.reset.textContent, e.reset.className]).toEqual([RESET_LABEL, "btn"]);
  });

  it("generates the group packs of a cohort and lists their codes", async () => {
    panel.setAccess(INSTRUCTOR);
    e.packs.fire("click");
    expect(e.packsDialog.open).toBe(true);
    expect((e.packsMission as unknown as FakeSelect).options.map((o) => [o.value, o.disabled])[0]).toEqual(["apps", false]);
    e.packsGenerate.fire("click");
    await flush();
    expect(e.packsError.textContent).toBe("Indiquez la mission, la cohorte et au moins un groupe.");
    e.packsCohort.value = " promo-2026 ";
    e.packsGroups.value = "G01, G02 G01";
    e.packsGenerate.fire("click");
    await flush();
    expect(calls).toEqual(["gen:apps:promo-2026:G01+G02:false"]);
    expect(e.packsTable.hidden).toBe(false);
    const rows = e.packsRows.all("tr").map((tr) => tr.children.map((td) => td.textContent));
    expect(rows[0]).toEqual(["G01", "7K4Q-M2ZD-9XRT-4WBC", "cohorts/promo-2026/apps/packs/G01.n7rtpack"]);
    expect(rows.length).toBe(2);
    e.packsReplace.checked = true;
    e.packsGroups.value = "G02";
    e.packsGenerate.fire("click");
    await flush();
    expect(calls.at(-1)).toBe("gen:apps:promo-2026:G02:true");
    e.packsClose.fire("click");
    expect(e.packsDialog.open).toBe(false);
  });

  it("judges a run file for the instructor and hides the verdict when leaving the mode", async () => {
    panel.setAccess(INSTRUCTOR);
    next.judge = () => Promise.resolve({
      mission: "apps", cohort: "promo-2026", group: "G01", started_utc: "20261006T142233Z", verdict: "fail",
      detail: "detail prive", judged_file: "cohorts/promo-2026/apps/judged/G01-20261006T142233Z.json",
    });
    e.judge.fire("click");
    expect(e.runFile.clicks).toBe(1);
    e.runFile.files = [new File([new Uint8Array([1, 2, 3])], "essai-apps-G01-20261006-142233.n7rtrun")];
    e.runFile.fire("change");
    await flush();
    expect(calls).toEqual(["judge:essai-apps-G01-20261006-142233.n7rtrun:AQID"]);
    expect([e.judgeResult.hidden, e.judgeResult.className]).toEqual([false, "mission-result err"]);
    expect(e.judgeVerdict.textContent).toBe("Essai jugé : non réussi");
    expect(e.judgeMeta.textContent).toBe(
      "Groupe G01 · cohorte promo-2026 · mission APPS · essai du 20261006T142233Z · conservé dans cohorts/promo-2026/apps/judged/G01-20261006T142233Z.json",
    );
    expect(e.judgeDetail.textContent).toBe("detail prive");
    panel.setAccess(STUDENT);
    expect([e.judgeResult.hidden, e.instructorFlow.hidden]).toEqual([true, true]);
  });

  it("refuses a file over its bound before reading it", async () => {
    expect([MAX_PACK_BYTES, MAX_RUN_BYTES]).toEqual([8 * 1024 * 1024, 8 * 1024 * 1024]);
    let reads = 0;
    const file = (size: number, name: string) =>
      ({ size, name, arrayBuffer: () => (reads++, Promise.resolve(new ArrayBuffer(0))) }) as unknown as Blob;
    panel.setAccess(LOCKED);
    e.packFile.files = [file(MAX_PACK_BYTES + 1, "apps-G01.n7rtpack")];
    e.packFile.value = "C:\\fakepath\\apps-G01.n7rtpack";
    e.packFile.fire("change");
    await flush();
    expect([e.error.textContent, e.packFile.value, reads, calls]).toEqual([PACK_TOO_LARGE, "", 0, []]);
    panel.setAccess(INSTRUCTOR);
    e.runFile.files = [file(MAX_RUN_BYTES + 1, "essai-apps-G01-20261006-142233.n7rtrun")];
    e.runFile.value = "C:\\fakepath\\essai-apps-G01-20261006-142233.n7rtrun";
    e.runFile.fire("change");
    await flush();
    expect([e.error.textContent, e.runFile.value, reads, calls]).toEqual([RUN_TOO_LARGE, "", 0, []]);
    // At the bound, the file is read and sent.
    next.judge = () => Promise.resolve({
      mission: "apps", cohort: "c", group: "G01", started_utc: "", verdict: "pass", detail: "", judged_file: null,
    });
    e.runFile.files = [file(MAX_RUN_BYTES, "essai-apps-G01-20261006-142233.n7rtrun")];
    e.runFile.fire("change");
    await flush();
    expect([e.error.textContent, reads, calls]).toEqual(["", 1, ["judge:essai-apps-G01-20261006-142233.n7rtrun:"]]);
  });

  it("writes text only: the panel source has no HTML sink", () => {
    const sinks = new RegExp(["inner" + "HTML", "outer" + "HTML", "insertAdjacent" + "HTML", "document" + "\\.write"].join("|"));
    expect(accessSrc.length).toBeGreaterThan(1000);
    expect(accessSrc).not.toMatch(sinks);
  });
});

describe("access helpers", () => {
  it("parses groups, encodes bytes and names packs and verdicts", () => {
    expect(parseGroups(" G01,G02 ;G03\n G02 ")).toEqual(["G01", "G02", "G03"]);
    expect(parseGroups("  ")).toEqual([]);
    expect(bytesToBase64(new Uint8Array([]))).toBe("");
    expect(bytesToBase64(new TextEncoder().encode("Man"))).toBe("TWFu");
    expect(bytesToBase64(new TextEncoder().encode("Ma"))).toBe("TWE=");
    const big = new Uint8Array(70_000).map((_, i) => i % 251);
    expect(Uint8Array.from(atob(bytesToBase64(big)), (c) => c.charCodeAt(0))).toEqual(big);
    expect(packLabel(STUDENT)).toBe("Aucun pack importé.");
    expect(packLabel(WITH_PACK)).toBe("G01 — cohorte promo-2026");
    const j: Judgement = { mission: "apps", cohort: "c", group: "G", started_utc: "", verdict: "pass", detail: "", judged_file: null };
    expect([judgementText(j), judgementMeta(j)]).toEqual(["Essai jugé : réussi", "Groupe G · cohorte c · mission APPS"]);
  });

  it("names the public edition in the instructor mode line only", () => {
    expect(modeText(null)).toBe(MODE_LABEL.student);
    expect(modeText({ ...STUDENT, oracle: false })).toBe(MODE_LABEL.student);
    expect(modeText(INSTRUCTOR)).toBe(MODE_LABEL.instructor);
    expect(modeText({ ...INSTRUCTOR, oracle: false, edition: "édition publique" })).toBe(`${MODE_LABEL.instructor} — ${PUBLIC_EDITION}`);
  });
});
