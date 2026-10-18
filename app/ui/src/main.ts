// Entry point: the UI pulls what changed from the core every POLL_MS (no push events: see
// src-tauri/src/lib.rs), the store absorbs it, the active view renders on animation frames when dirty.
// Two views share the window (ViewHost, ui/src/shell.ts): the cockpit and the Physical Lab, one active at a time.
// The cockpit store keeps absorbing batches while hidden and is fully re-rendered when shown again; the Physical
// Lab polls its values only while shown. In e2e mode the start view is the cockpit.
// Missions (views/mission.ts): the mission state is pulled every MISSION_POLL_MS; the panel lives in the cockpit.
// Modes (views/access.ts, docs/distribution.md §2): the mode and the student state are read after each command that
// changes them; the mission list depends on the mode.

import { access, e2e, lab, mission } from "./bridge";
import { $, setText } from "./dom";
import { PhysView } from "./lab/view";
import { UiPerf } from "./perf";
import { COCKPIT_LAYOUT_KEY, PHYS_LAYOUT_KEY, Shell, ViewHost, cockpitDefaults, ficheShare, physDefaults } from "./shell";
import { Store } from "./store";
import { Cockpit } from "./views/cockpit";
import { AccessPanel } from "./views/access";
import { MissionPanel, benchBanner } from "./views/mission";
import { ProgressPanel, progressStatus } from "./views/progress";
import { closeDialog, settleFiche, watchFiche } from "./views/unsaved";
import type { MissionView } from "./types";
import { StudentIde } from "./ide/view";

type ViewName = "cockpit" | "phys" | "code";
declare const __APP_VERSION__: string;
const VIEW_KEY = "n7rt-can-lab.view.v1";

const store = new Store();
const perf = new UiPerf();
const shell = new Shell({
  root: $('.workspace[data-view="cockpit"]'),
  key: COCKPIT_LAYOUT_KEY,
  defaults: cockpitDefaults(window.innerWidth),
  titles: { left: "Panneau latéral", bottom: "Console", right: "Résultats" },
  // the fiche gets 60 % of the workspace (more on a short one); about four rows of frames stay readable above it
  tallTabs: { fiche: ficheShare },
});
const physShell = new Shell({
  root: $('.workspace[data-view="phys"]'),
  key: PHYS_LAYOUT_KEY,
  defaults: physDefaults(),
  titles: { left: "Bibliothèque", bottom: "Console", right: "Inspecteur" },
});
const cockpit = new Cockpit(perf, shell, store);
const phys = new PhysView();
const codeShell = new Shell({
  root: $('.workspace[data-view="code"]'), key: "n7rt-can-lab.layout.v1.code",
  defaults: { left: 248, right: 256, bottom: 180, hidden: [], tab: "ide-console" },
  titles: { left: "Explorateur de projet", right: "Plan du code et carte", bottom: "Console de compilation" },
});
const code = new StudentIde((name) => host.show(name));
/** A mission closes once the fiche's unsaved changes are saved or discarded on purpose (views/unsaved.ts). */
const settleBeforeMissionClose = () =>
  settleFiche((message, offer) => closeDialog("fiche-close", offer ? "Fiche non enregistrée" : "Fermeture suspendue", message, offer, "Revenir à la mission"));
const POLL_MS = 50;
const MISSION_POLL_MS = 250;

let missionState: MissionView | null = null;
const missionPanel = new MissionPanel(
  {
    select: $<HTMLSelectElement>("#mission-select"),
    open: $<HTMLButtonElement>("#mission-open"),
    close: $<HTMLButtonElement>("#mission-close"),
    reset: $<HTMLButtonElement>("#mission-reset"),
    error: $("#mission-error"),
    stimuli: $("#stim-section"),
    section: $("#mission-panel"),
    title: $("#mission-title"),
    summary: $("#mission-summary"),
    requirements: $("#mission-requirements"),
    outputs: $("#mission-outputs"),
    library: $("#mission-library"),
    check: $<HTMLButtonElement>("#mission-check"),
    result: $("#mission-result"),
    resultText: $("#mission-result-text"),
    folder: $<HTMLButtonElement>("#mission-runs-folder"),
    progress: $("#mission-progress"),
    bar: $("#mission-progress-bar"),
    frames: $("#frames-wrap"),
    framesCover: $("#check-cover"),
    framesCoverText: $("#check-cover-text"),
    framesCoverBar: $("#check-cover-bar"),
    slots: $("#slots-area"),
    slotsCover: $("#slots-cover"),
    progressRun: $("#progress-run"),
    progressRunBar: $("#progress-run-bar"),
  },
  {
    open: (id) => void settleBeforeMissionClose().then((go) => { if (go) void missionCommand(() => mission.open(id)); }),
    close: () => void settleBeforeMissionClose().then((go) => { if (go) void missionCommand(() => mission.close()); }),
    reset: () => void missionCommand(() => mission.reset()),
    check: () =>
      void mission.check().then(applyMission, (err) => {
        missionPanel.setCheckError(String(err));
      }),
    openDocument: (index) =>
      void mission.openDocument(index).then(
        () => missionPanel.setError(""),
        (err) => missionPanel.setError(String(err)),
      ),
    openRunsFolder: () =>
      void mission.openRunsFolder().then(
        () => missionPanel.setError(""),
        (err) => missionPanel.setError(String(err)),
      ),
  },
);

/** The dock tab shown before the fiche took the dock: the fiche gives it back when it closes. */
let tabBeforeFiche: string | null = null;

/** Brings an element into view inside its scrolling panel, and focuses it without a second scroll when asked;
 * `start`: its top at the top of the panel (under a sticky head, by the panel's scroll-padding). */
function reveal(el: HTMLElement, focus: boolean, start = false): void {
  el.scrollIntoView({ block: start ? "start" : "nearest" });
  if (focus) el.focus({ preventScroll: true });
}

/** Student progression of a release 0.3 group pack (steps, work order, verification, fiche). */
const progressPanel = new ProgressPanel(
  {
    section: $("#progress-section"),
    steps: $("#progress-steps"),
    order: $("#progress-order"),
    verify: $<HTMLButtonElement>("#progress-verify"),
    blocked: $("#progress-blocked"),
    feedback: $("#progress-feedback"),
    ficheToggle: $<HTMLButtonElement>("#progress-fiche-toggle"),
    fiche: $("#progress-fiche"),
    final: $<HTMLButtonElement>("#progress-final"),
    reproduce: $<HTMLButtonElement>("#progress-reproduce"),
    reproduceDialog: $<HTMLDialogElement>("#reproduce-dialog"),
    reproduceConfirm: $<HTMLButtonElement>("#reproduce-confirm"),
    reproduceCancel: $<HTMLButtonElement>("#reproduce-cancel"),
    briefStep: $("#progress-brief-step"),
    ficheTab: $("#tab-fiche"),
    hint: $("#progress-hint"),
    exportAlert: $("#progress-export"),
    exportRetry: $<HTMLButtonElement>("#progress-export-retry"),
  },
  {
    // the fiche lives in the cockpit's bottom dock: its tab comes forward, the dock comes back if it was hidden
    showFiche: () => {
      $("#tab-fiche").hidden = false;
      if ($("#app").classList.contains("no-bottom")) shell.toggle("bottom");
      if (shell.tab !== "fiche") {
        tabBeforeFiche = shell.tab;
        shell.selectTab("fiche");
      }
    },
    hideFiche: () => {
      if (shell.tab === "fiche") shell.selectTab(tabBeforeFiche ?? "logs");
    },
    reveal,
    verify: async () => {
      applyMission(await mission.verify());
    },
    loadFiche: () => mission.ficheGet(),
    saveFiche: async (part) => {
      applyMission(await mission.ficheSet(part));
    },
    loadIncident: (rank) => mission.incidentGet(rank),
    saveIncident: async (part) => {
      applyMission(await mission.incidentSet(part));
    },
    consign: async () => {
      applyMission(await mission.consign());
    },
    reproduce: async () => {
      applyMission(await mission.reproduce());
    },
    openFinal: async () => {
      applyMission(await mission.ficheOpen());
    },
  },
);
// closing the window (ide/view.ts) or the mission asks before the fiche's unsaved changes are lost
watchFiche(progressPanel);

const accessPanel = new AccessPanel(
  {
    modeLine: $("#mode-line"),
    modeButton: $<HTMLButtonElement>("#mode-btn"),
    error: $("#mission-error"),
    studentFlow: $("#student-flow"),
    choice: $("#student-choice"),
    studentSelect: $<HTMLSelectElement>("#student-mission"),
    choose: $<HTMLButtonElement>("#student-choose"),
    confirm: $("#student-confirm"),
    confirmText: $("#student-confirm-text"),
    confirmYes: $<HTMLButtonElement>("#student-confirm-yes"),
    confirmNo: $<HTMLButtonElement>("#student-confirm-no"),
    locked: $("#student-locked"),
    lockedTitle: $("#student-locked-title"),
    pack: $("#student-pack"),
    importButton: $<HTMLButtonElement>("#student-import"),
    packFile: $<HTMLInputElement>("#student-pack-file"),
    code: $<HTMLInputElement>("#student-code"),
    open: $<HTMLButtonElement>("#student-open"),
    close: $<HTMLButtonElement>("#student-close"),
    instructorFlow: $("#instructor-flow"),
    reset: $<HTMLButtonElement>("#instructor-reset"),
    packs: $<HTMLButtonElement>("#instructor-packs"),
    judge: $<HTMLButtonElement>("#instructor-judge"),
    runFile: $<HTMLInputElement>("#instructor-run-file"),
    judgeResult: $("#judge-result"),
    judgeVerdict: $("#judge-verdict"),
    judgeMeta: $("#judge-meta"),
    judgeDetail: $("#judge-detail"),
    codeDialog: $<HTMLDialogElement>("#code-dialog"),
    codeInput: $<HTMLInputElement>("#code-input"),
    codeSubmit: $<HTMLButtonElement>("#code-submit"),
    codeCancel: $<HTMLButtonElement>("#code-cancel"),
    codeError: $("#code-error"),
    packsDialog: $<HTMLDialogElement>("#packs-dialog"),
    packsMission: $<HTMLSelectElement>("#packs-mission"),
    packsCohort: $<HTMLInputElement>("#packs-cohort"),
    packsGroups: $<HTMLInputElement>("#packs-groups"),
    packsReplace: $<HTMLInputElement>("#packs-replace"),
    packsGenerate: $<HTMLButtonElement>("#packs-generate"),
    packsClose: $<HTMLButtonElement>("#packs-close"),
    packsError: $("#packs-error"),
    packsTable: $("#packs-table"),
    packsRows: $("#packs-rows"),
  },
  {
    login: (code) => access.login(code),
    logout: () => access.logout(),
    reset: () => access.reset(),
    choose: (id) => access.choose(id),
    importPack: (data) => access.importPack(data),
    open: (code) => access.open(code),
    close: async () => ((await settleBeforeMissionClose()) ? mission.close() : mission.state()),
    genPacks: (m, cohort, groups, replace) => access.genPacks(m, cohort, groups, replace),
    judge: (name, data) => access.judge(name, data),
    mission: (v) => applyMission(v),
    changed: () => void refreshAccess(),
  },
);

/** Open state of the Carte and Session sections before a progression folded them (null: no progression open). */
let foldedSections: boolean[] | null = null;
const FOLDED = ["#board-section", "#session-section"];

/** A progression opens: Carte and Session fold, so that it is the first thing in the side panel; it closes: they are
 * as they were. */
function foldForProgression(open: boolean): void {
  const sections = FOLDED.map((s) => $<HTMLDetailsElement>(s));
  if (open && !foldedSections) {
    foldedSections = sections.map((d) => d.open);
    for (const d of sections) d.open = false;
  } else if (!open && foldedSections) {
    sections.forEach((d, i) => (d.open = foldedSections![i]));
    foldedSections = null;
  }
}

/** The status-bar segment of the progression: step and state, its tone, the blocking message as its title. */
function renderProgressStatus(v: MissionView): void {
  const sb = $<HTMLButtonElement>("#sb-progress");
  const st = progressStatus(v.progression);
  sb.hidden = !st;
  if (!st) return;
  setText($('[data-s="progress"]', sb), st.text);
  const cls = st.tone ? `sb-progress ${st.tone}` : "sb-progress";
  if (sb.className !== cls) sb.className = cls;
  if (sb.title !== st.title) sb.title = st.title;
}

function applyMission(v: MissionView): void {
  missionState = v;
  missionPanel.setView(v);
  foldForProgression(!!v.progression);
  progressPanel.setView(v.progression, `${v.id ?? ""}|${v.group ?? ""}`);
  accessPanel.setMission(v);
  cockpit.setMission(v);
  renderProgressStatus(v);
  // the multimeter records the key measure where it is taken (the open incident form is saved first)
  phys.setMission(v.progression, () => progressPanel.consignFromBench());
  // The Physical Lab shows the banner in mission mode; its text names the open mission.
  setText($("#phys-mission"), benchBanner(v));
}

/** After a mode change, a reset or a choice: the mode-dependent list, the mission and the student state again. */
async function refreshAccess(): Promise<void> {
  const list = await mission.list().catch(() => []);
  missionPanel.setList(list);
  accessPanel.setList(list);
  try {
    applyMission(await mission.state());
    accessPanel.setAccess(await access.state());
  } catch (err) {
    console.error(err);
  }
}

/** Open or close: one command at a time; a refusal is shown under the selector and the list is read again. */
async function missionCommand(run: () => Promise<MissionView>): Promise<void> {
  missionPanel.setBusy(true);
  try {
    applyMission(await run());
    missionPanel.setError("");
  } catch (err) {
    missionPanel.setError(String(err));
    const list = await mission.list().catch(() => []);
    missionPanel.setList(list);
    accessPanel.setList(list);
  } finally {
    missionPanel.setBusy(false);
  }
}

/** The mission state, at most one request in flight. */
async function pollMission(): Promise<void> {
  try {
    const v = await mission.state();
    if (JSON.stringify(v) !== JSON.stringify(missionState)) applyMission(v);
  } catch (err) {
    console.error(err);
  }
  setTimeout(() => void pollMission(), MISSION_POLL_MS);
}

const IDE_BUSY = "Compilation ou flashage en cours : la carte est occupée.";

/** While the IDE builds or flashes, the board belongs to it: connect, disconnect, start and the check wait (and say why). */
function lockWhileIdeBusy(): void {
  if (!code.busy) return;
  for (const id of ["btn-connect", "btn-disconnect", "btn-start", "mission-check"]) {
    const b = $<HTMLButtonElement>(`#${id}`);
    b.disabled = true;
    if (id !== "mission-check" && b.title !== IDE_BUSY) b.title = IDE_BUSY;
  }
}

/** Shows or hides the grid slots of a view (the inactive ones are hidden, never removed). */
function slots(view: ViewName, visible: boolean): void {
  for (const el of Array.from(document.querySelectorAll<HTMLElement>(`.workbench > [data-view="${view}"]`))) el.hidden = !visible;
}

const host = new ViewHost<ViewName>({
  code: {
    show() { slots("code", true); codeShell.activate(); code.show(); },
    hide() { code.hide(); codeShell.deactivate(); slots("code", false); },
    frame(now) {
      if (store.anyDirty()) {
        cockpit.render(store, now);
        store.clean();
      }
      code.setBoard(store.status, missionState);
      code.frame(now);
      lockWhileIdeBusy();
    },
  },
  cockpit: {
    show() {
      slots("cockpit", true);
      shell.activate();
      // Batches absorbed while hidden: everything is drawn again on the next frame.
      store.dirty = { status: true, stats: true, rows: true, logs: true, errors: true, slots: true };
    },
    hide() {
      shell.deactivate();
      slots("cockpit", false);
    },
    frame(now) {
      if (store.anyDirty()) {
        if (store.dirty.status) missionPanel.setBoard(store.status);
        cockpit.render(store, now);
        store.clean();
      }
    },
  },
  phys: {
    show() {
      slots("phys", true);
      physShell.activate();
      phys.show();
    },
    hide() {
      phys.hide();
      physShell.deactivate();
      slots("phys", false);
    },
    frame(now) {
      phys.frame(now);
    },
  },
});

host.onChange = (name) => {
  for (const b of Array.from(document.querySelectorAll<HTMLButtonElement>(".activity button[data-show-view]"))) {
    if (b.dataset.showView === name) b.setAttribute("aria-current", "page");
    else b.removeAttribute("aria-current");
  }
  try {
    localStorage.setItem(VIEW_KEY, name);
  } catch {
    /* storage unavailable: the view is simply not remembered */
  }
};

function savedView(): ViewName {
  try {
    const name = localStorage.getItem(VIEW_KEY);
    return name === "phys" || name === "cockpit" ? name : "code";
  } catch {
    return "code";
  }
}

/** One request in flight at most; the next one waits for the previous answer. */
async function poll(): Promise<void> {
  try {
    const b = await lab.drain();
    if (b) {
      perf.onBatch(performance.now());
      store.apply(b);
    }
  } catch (err) {
    console.error(err);
  }
  setTimeout(() => void poll(), POLL_MS);
}

function frame(now: number): void {
  perf.onAnimationFrame(now);
  host.frame(now);
  lockWhileIdeBusy();
  requestAnimationFrame(frame);
}

async function main(): Promise<void> {
  $("#app-version").textContent = `v${__APP_VERSION__}`;
  const initialView = savedView();
  shell.onTab = (tab) => {
    cockpit.tabShown(tab);
    progressPanel.tabShown(tab === "fiche");
  };
  physShell.onTab = (tab) => phys.tabShown(tab);
  shell.mount();
  physShell.mount();
  codeShell.mount();
  cockpit.mount();
  missionPanel.mount();
  progressPanel.mount();
  accessPanel.mount();
  phys.mount();
  code.mount();
  await code.protectNativeClose();
  $("#mission-code").addEventListener("click", () => host.show("code"));
  $("#mission-bench").addEventListener("click", () => host.show("phys"));
  // the progression's segment of the status bar, from any view: back to the cockpit, the progression in view
  $("#sb-progress").addEventListener("click", () => {
    host.show("cockpit");
    reveal($("#progress-section"), false);
  });
  for (const b of Array.from(document.querySelectorAll<HTMLButtonElement>(".activity button[data-show-view]"))) {
    b.addEventListener("click", () => host.show(b.dataset.showView ?? ""));
  }
  host.show("cockpit");
  store.apply(await lab.snapshot());
  void poll();
  await refreshAccess();
  void pollMission();
  requestAnimationFrame(frame);
  const cfg = await e2e.config();
  if (cfg) {
    const { runScenario } = await import("./e2e");
    await runScenario(cfg, store, perf);
  } else {
    host.show(initialView);
  }
}

main().catch((err) => {
  const msg = document.getElementById("state-message");
  if (msg) msg.textContent = `Erreur de l'interface : ${String(err)}`;
  console.error(err);
});
