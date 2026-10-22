// Cockpit view: board and link state, session, stimuli, counters, raw frames with the student's
// answers, published values, student logs, link events, status bar. It shows raw CAN data and
// what the student's code produced, never a decoding of the frames. With a mission open (views/mission.ts)
// the slots carry their public names and units, the tick period is the mission's, and during a check the
// session and connection controls are locked.
//
// Rendering rules (ui/DESIGN.md): DOM touched only on animation frames and only for what changed;
// text through textContent; the frames table recycles a fixed set of rows.

import { lab } from "../bridge";
import { $, clear, el, setOptions, setText } from "../dom";
import {
  STATE_CLASS, STATE_TEXT, dataLength, dataText, durationText, f32Text, idText, kindText, msText,
  publicationsText, resultText, sessionText,
} from "../format";
import type { UiPerf } from "../perf";
import type { Shell } from "../shell";
import { ERRORS_KEPT, LOGS_KEPT, ROWS_KEPT, type Store } from "../store";
import type { ErrorRow, FrameFormat, FrameRow, LogRow, MissionView, PortInfo, SlotLabel, Stimuli } from "../types";
import { slotLabels, slotName, valueWithUnit } from "./mission";

export interface View {
  mount(): void;
  render(store: Store, now: number): void;
}

const FORMATS: FrameFormat[] = ["classic", "extended", "fd", "fd_brs", "remote"];
/** Columns of the frames table, in display order (`data-col` is read back by the e2e check). */
const COLS = ["session", "seq", "t", "id", "kind", "dlc", "data", "result", "publications", "cycles", "rtt"];
const NUMERIC = new Set(["seq", "t", "dlc", "cycles", "rtt"]);
const ROW_HEIGHT = 21; // --h-row + 1 px separator

function portValue(p: PortInfo): string {
  return p.serial ? `sn:${p.serial}` : `port:${p.port}`;
}

function portLabel(p: PortInfo): string {
  const probe = p.probe ?? p.product ?? "port série";
  return p.serial ? `${p.port} · ${probe} · ${p.serial}` : `${p.port} · ${probe}`;
}

function rowKey(r: FrameRow): string {
  return `${r.session_id}:${r.frame_seq}`;
}

// Why a toolbar or stimuli button is disabled (its title): short, in the order the reasons apply.
const BUSY_CHECKING = "Vérification en cours : la carte et la session sont réservées à l'essai.";
const NO_BOARD = "Aucune carte détectée : branchez-la, puis rechercher les cartes.";
const ALREADY_CONNECTED = "Cette carte est déjà connectée.";
const NOT_CONNECTED = "Aucune carte connectée.";
const SESSION_RUNNING = "Une session est déjà en cours.";
const START_NEEDS_BOARD = "Connectez une carte prête pour démarrer une session.";
const NO_SESSION = "Aucune session en cours.";
const SEND_NEEDS_SESSION = "Démarrez une session pour envoyer une trame.";
const MISSION_STIMULI = "Pendant une mission, le bus est celui du banc : pas de trame manuelle.";

/** Enables a button, or disables it with its reason as title (an empty reason enables it). */
function gate(selector: string, reason: string): void {
  const b = $<HTMLButtonElement>(selector);
  b.disabled = reason !== "";
  if (b.title !== reason) b.title = reason;
}

/** « trame 18432 » or « tick 1250 ms » · « 12 mises à jour »: where a slot's value came from, as the row's title. */
function slotOrigin(source: string, at: number, updates: number): string {
  const from = source === "frame" ? `trame ${at}` : `tick ${msText(at, 0)}`;
  return `${from} · ${updates} ${updates > 1 ? "mises" : "mise"} à jour`;
}

export class Cockpit implements View {
  private stimRev = 0;
  private stimInitialised = false;
  private selectedPort = "";
  private idFilter: number | null = null;
  private consoleFilter = "";
  private selected: FrameRow | null = null;
  /** The row object shown by each <tr> (recycled rows keep their latest content). */
  private shown = new WeakMap<HTMLTableRowElement, FrameRow>();
  private mission: MissionView | null = null;
  private missionKey = "";
  private labels = new Map<number, SlotLabel>();
  /** The session whose frames are shown normally: the running one, else the one of the newest frame. */
  private current: number | null = null;

  constructor(private perf: UiPerf, private shell: Shell, private store: Store) {}

  mount(): void {
    const dlc = $<HTMLSelectElement>("#stim-dlc");
    for (let i = 0; i <= 15; i++) {
      const o = el("option");
      o.value = String(i);
      dlc.appendChild(o);
    }
    this.refreshDlcLabels();
    dlc.value = "8";

    $("#btn-refresh").addEventListener("click", () => void lab.refresh());
    $<HTMLSelectElement>("#port-select").addEventListener("change", (e) => {
      this.selectedPort = (e.target as HTMLSelectElement).value;
    });
    $("#btn-connect").addEventListener("click", () => {
      const v = $<HTMLSelectElement>("#port-select").value;
      if (!v) return;
      const [kind, ...rest] = v.split(":");
      const val = rest.join(":");
      void lab.connect(kind === "port" ? val : null, kind === "sn" ? val : null);
    });
    $("#btn-disconnect").addEventListener("click", () => void lab.disconnect());
    $("#btn-start").addEventListener("click", () => void lab.start(Number($<HTMLSelectElement>("#tick-select").value)));
    $("#btn-stop").addEventListener("click", () => void lab.stop());
    $("#btn-send-one").addEventListener("click", () => void lab.sendOne());

    for (const id of ["#stim-pot", "#stim-rate", "#stim-id"]) {
      $(id).addEventListener("input", () => this.stimuliChanged());
    }
    for (const id of ["#stim-button", "#stim-format", "#stim-dlc"]) {
      $(id).addEventListener("change", () => this.stimuliChanged());
    }

    $<HTMLInputElement>("#frame-filter").addEventListener("input", (e) => this.setIdFilter((e.target as HTMLInputElement).value));
    $<HTMLInputElement>("#console-filter").addEventListener("input", (e) => {
      this.consoleFilter = (e.target as HTMLInputElement).value.trim().toLowerCase();
      this.applyConsoleFilter($("#logs"));
      this.applyConsoleFilter($("#errors"));
    });
    $("#frames tbody").addEventListener("click", (e) => {
      const tr = (e.target as Element).closest("tr");
      const row = tr ? this.shown.get(tr as HTMLTableRowElement) : undefined;
      if (row) this.select(row);
    });
    this.framesEmpty();
  }

  /** No frame received yet: the table says how to get some (a filter that hides every row is not « no frame »). */
  private framesEmpty(): void {
    const empty = this.store.rows.length === 0 && $("#frames tbody").children.length === 0;
    const p = $("#frames-empty");
    if (p.hidden !== !empty) p.hidden = !empty;
  }

  // ------------------------------------------------------------------ mission

  /** The mission state changed: the controls and the slots are drawn again when it matters to them. */
  setMission(view: MissionView): void {
    this.mission = view;
    const key = JSON.stringify([view.open, view.id, view.tick_period_ms, view.check.state, view.statement?.slots ?? null]);
    if (key === this.missionKey) return;
    this.missionKey = key;
    this.labels = slotLabels(view);
    this.store.dirty.status = this.store.status !== null;
    this.store.dirty.slots = true;
  }

  private checking(): boolean {
    return this.mission?.check.state === "running";
  }

  // ------------------------------------------------------------------ stimuli

  private refreshDlcLabels(): void {
    const format = $<HTMLSelectElement>("#stim-format").value;
    const dlc = $<HTMLSelectElement>("#stim-dlc");
    for (const o of Array.from(dlc.options)) {
      setText(o, `${o.value} (${dataLength(Number(o.value), format)}\u00a0o)`);
    }
  }

  /** Reads the stimuli controls; null (and an explanation) when the identifier is invalid. */
  private readStimuli(): Stimuli | null {
    const format = $<HTMLSelectElement>("#stim-format").value as FrameFormat;
    const text = $<HTMLInputElement>("#stim-id").value.trim();
    const idOk = /^(0x)?[0-9a-fA-F]{1,8}$/.test(text);
    const can_id = idOk ? parseInt(text.replace(/^0x/i, ""), 16) : NaN;
    const max = format === "extended" ? 0x1fffffff : 0x7ff;
    const err = $("#stim-error");
    const idInput = $<HTMLInputElement>("#stim-id");
    if (!idOk || can_id > max) {
      idInput.classList.add("invalid");
      setText(err, `Identifiant invalide (0 à 0x${max.toString(16).toUpperCase()}).`);
      return null;
    }
    idInput.classList.remove("invalid");
    setText(err, "");
    return {
      pot: Number($<HTMLInputElement>("#stim-pot").value),
      button: $<HTMLInputElement>("#stim-button").checked,
      format,
      can_id,
      dlc: Number($<HTMLSelectElement>("#stim-dlc").value),
      rate_hz: Number($<HTMLInputElement>("#stim-rate").value),
    };
  }

  private stimuliChanged(): void {
    this.refreshDlcLabels();
    setText($("#stim-pot-out"), $<HTMLInputElement>("#stim-pot").value);
    setText($("#stim-rate-out"), $<HTMLInputElement>("#stim-rate").value);
    const s = this.readStimuli();
    if (!s) return;
    this.stimRev += 1;
    this.perf.probe(this.stimRev);
    void lab.setStimuli(s, this.stimRev);
  }

  /** The core's stimuli become the controls' initial values once. */
  private initStimuli(s: Stimuli, rev: number): void {
    this.stimInitialised = true;
    this.stimRev = Math.max(this.stimRev, rev);
    $<HTMLInputElement>("#stim-pot").value = String(s.pot);
    $<HTMLInputElement>("#stim-button").checked = s.button;
    if (FORMATS.includes(s.format)) $<HTMLSelectElement>("#stim-format").value = s.format;
    $<HTMLInputElement>("#stim-id").value = idText(s.can_id, s.format === "extended" ? 1 : 0);
    $<HTMLSelectElement>("#stim-dlc").value = String(s.dlc);
    $<HTMLInputElement>("#stim-rate").value = String(s.rate_hz);
    this.refreshDlcLabels();
    setText($("#stim-pot-out"), String(s.pot));
    setText($("#stim-rate-out"), String(s.rate_hz));
  }

  // ------------------------------------------------------------------ render loop

  render(store: Store, now: number): void {
    const t0 = performance.now();
    if (store.dirty.status && store.status) this.renderStatus(store);
    if (store.dirty.stats && store.stats) this.renderStats(store);
    if (store.dirty.rows) this.renderRows(store.newRows);
    if (store.dirty.status || store.dirty.rows) this.markPastRows(store);
    if (store.dirty.slots) this.renderSlots(store);
    if (store.dirty.logs) this.renderLogs(store);
    if (store.dirty.errors) this.renderErrors(store);
    this.perf.onRender(performance.now() - t0);
    if (store.dirty.rows) this.perf.displayed(store.newRows, now);
  }

  private renderStatus(store: Store): void {
    const s = store.status!;
    if (!this.stimInitialised) this.initStimuli(s.stimuli, s.stim_rev);
    const variant = STATE_CLASS[s.state] ?? "";
    const badge = $("#state-badge");
    setText(badge, STATE_TEXT[s.state] ?? s.state);
    badge.className = variant ? `badge badge--${variant}` : "badge";
    $("#overlay-stalled").hidden = !s.stalled;
    $("#overlay-busy").hidden = !s.student_busy;
    setText($("#state-message"), s.message);
    const msgClass = s.stalled || variant === "warn" ? "message warn" : variant === "err" ? "message err" : "message";
    if ($("#state-message-row").className !== msgClass) $("#state-message-row").className = msgClass;

    const options = s.ports.map((p) => ({ value: portValue(p), label: portLabel(p) }));
    if (!options.length) options.push({ value: "", label: "Aucun port série détecté" });
    const current = s.port ? portValue(s.port) : "";
    const firstProbe = s.ports.find((p) => p.probe);
    const keep = this.selectedPort || current || (firstProbe ? portValue(firstProbe) : "");
    setOptions($<HTMLSelectElement>("#port-select"), options, keep);
    // A running nominal check owns the board and the session: nothing may interrupt it from here.
    const checking = this.checking();
    const chosen = $<HTMLSelectElement>("#port-select").value;
    // a disabled button always says why (its title), in the order the reasons apply
    const busy = checking ? BUSY_CHECKING : "";
    gate("#btn-connect", busy || (!chosen ? NO_BOARD : !!current && chosen === current ? ALREADY_CONNECTED : ""));
    gate("#btn-disconnect", busy || (s.target === null && s.port === null ? NOT_CONNECTED : ""));
    gate("#btn-start", busy || (s.can_start ? "" : s.session ? SESSION_RUNNING : START_NEEDS_BOARD));
    gate("#btn-stop", busy || (s.can_stop ? "" : NO_SESSION));
    gate("#btn-send-one", this.mission?.open ? MISSION_STIMULI : s.session?.phase !== "active" ? SEND_NEEDS_SESSION : "");
    $<HTMLSelectElement>("#port-select").disabled = checking;
    const tick = $<HTMLSelectElement>("#tick-select");
    const missionTick = this.mission?.open ? this.mission.tick_period_ms : null;
    if (missionTick !== null && tick.value !== String(missionTick)) tick.value = String(missionTick);
    tick.disabled = missionTick !== null;
    tick.title = missionTick !== null ? "Période de tick fixée par la mission" : "";

    const info = $("#board-info");
    const set = (k: string, v: string, cls = "") => {
      const dd = $(`[data-k="${k}"]`, info);
      setText(dd, v);
      if (dd.title !== v) dd.title = v;
      if (dd.className !== cls) dd.className = cls;
    };
    const b = s.board;
    set("port", s.port ? s.port.port : "—");
    set("board", b ? b.board || `microcontrôleur 0x${b.dev_id.toString(16).toUpperCase()} non pris en charge` : "—",
      b ? (b.board ? "" : "err") : "");
    set("probe", s.port ? `${s.port.probe ?? "?"} ${s.port.serial ?? ""}`.trim() : "—");
    set("runtime", b ? `${b.runtime} · ${b.build_id} · ABI ${b.abi}` : "—");
    set("verified", b ? (b.runtime_verified ? "défi SHA-256 vérifié" : "NON vérifié") + (b.compatible ? "" : " — incompatible") : "—",
      b ? (b.runtime_verified && b.compatible ? "ok" : "err") : "");
    set("student", b ? b.student_status_text : "—", b ? (b.student_status === 1 ? "ok" : "err") : "");
    set("crc", b && b.student_status === 1 ? `${b.student_crc32} · ABI ${b.student_abi}` : "—");
    set("uid", b ? b.uid : "—");
    set("reset", b ? b.reset_causes.join(", ") || "—" : "—");
    const fault = $("#board-fault");
    fault.hidden = !s.fault;
    setText(fault, s.fault ? `Dernière faute : ${s.fault}` : "");

    const si = $("#session-info");
    const ss = (k: string, v: string) => setText($(`[data-k="${k}"]`, si), v);
    const sess = s.session;
    ss("id", sess ? sessionText(sess.id) : "—");
    ss("phase", sess ? { starting: "ouverture…", active: "active", stopping: "arrêt…", closing: "fermeture…" }[sess.phase] : "aucune");
    ss("elapsed", sess ? durationText(sess.elapsed_ms) : "—");
    ss("bus", sess ? `${(sess.bus_time_us / 1e6).toFixed(3)}\u00a0s · tick ${sess.tick_period_ms || "—"}\u00a0ms` : "—");

    const hb = s.heartbeat;
    const sc = (k: string, v: number | undefined) => {
      const e = $(`[data-c="${k}"]`);
      setText(e, v === undefined ? "—" : String(v));
      e.classList.toggle("nonzero", (v ?? 0) > 0);
    };
    sc("rx_crc_errors", hb?.rx_crc_errors);
    sc("rx_framing_errors", hb?.rx_framing_errors);
    sc("rx_seq_gaps", hb?.rx_seq_gaps);
    sc("rx_overruns", hb?.rx_overruns);
    sc("log_dropped", hb?.log_dropped);

    // Status bar.
    const sbState = $("#sb-state");
    const sbClass = s.stalled ? "warn" : variant;
    if (sbState.className !== sbClass) sbState.className = sbClass;
    setText($(`[data-s="state"]`), STATE_TEXT[s.state] + (s.stalled ? " · carte muette" : "") + (s.student_busy ? " · code occupé" : ""));
    setText($(`[data-s="port"]`), s.port ? `${s.port.port} · ${s.port.serial ?? ""}` : "aucun port");
    setText($(`[data-s="runtime"]`), b ? `runtime ${b.runtime} ${b.runtime_verified ? "vérifié" : "NON vérifié"} · CRC ${b.student_crc32}` : "—");
    setText($(`[data-s="session"]`), sess ? `session ${sessionText(sess.id)} · ${durationText(sess.elapsed_ms)}` : "pas de session");
  }

  private renderStats(store: Store): void {
    const st = store.stats!;
    const m = (k: string, v: string, bad = false) => {
      const e = $(`[data-m="${k}"]`);
      setText(e, v);
      e.classList.toggle("nonzero", bad);
    };
    m("tx_fps", st.tx_fps.toFixed(0));
    m("rx_fps", st.rx_fps.toFixed(0));
    m("frames_sent", String(st.frames_sent));
    m("results", String(st.results));
    m("in_flight", `${st.in_flight} / ${st.window}`);
    m("rtt", st.rtt_count ? `${msText(st.rtt_p50_us, 1)} / ${msText(st.rtt_p99_us, 1)}` : "—");
    m("nacks", String(st.nacks), st.nacks > 0);
    m("lost", String(st.lost), st.lost > 0);
    const pcErrors = Object.values(st.rx_framing).reduce((a, b) => a + b, 0) + st.rx_rejects + st.board_seq_gaps;
    const pe = $(`[data-c="pc_errors"]`);
    setText(pe, String(pcErrors));
    pe.classList.toggle("nonzero", pcErrors > 0);

    setText($(`[data-s="rates"]`), `${st.tx_fps.toFixed(0)} → ${st.rx_fps.toFixed(0)}\u00a0trames/s`);
    setText($(`[data-s="rtt"]`), st.rtt_count ? `A/R ${msText(st.rtt_p50_us, 2)}\u00a0ms` : "A/R —");
    const problems = st.nacks + st.lost + pcErrors;
    const sbErr = $(`[data-s="errors"]`);
    setText(sbErr, problems ? `${problems} anomalie${problems > 1 ? "s" : ""}` : "0 anomalie");
    sbErr.classList.toggle("alert", problems > 0);

    const q = st.queues;
    const ui = this.perf.summary();
    const diag: [string, string][] = [
      ["File sim → liaison", `${q.sim_depth} / ${q.sim_capacity} (max ${q.sim_high_water})`],
      ["Bus en pause (contre-pression)", `${q.sim_paused_ms}\u00a0ms`],
      ["Trames générées / rejetées", `${q.sim_generated} / ${q.sim_dropped}`],
      ["Pas du générateur max", `${(q.sim_max_step_us / 1000).toFixed(1)}\u00a0ms`],
      ["En vol max / attentes de crédit", `${st.in_flight_max} / ${st.credit_waits}`],
      ["Boucle liaison max (50 ms)", `${(q.link_loop_max_us / 1000).toFixed(1)}\u00a0ms`],
      ["Écriture série max", `${(q.write_max_us / 1000).toFixed(1)}\u00a0ms`],
      ["Lecture série max", `${q.read_chunk_max}\u00a0o`],
      ["Attente d'écriture série max", `${q.tx_backlog_max}\u00a0o`],
      ["Lignes non affichées", `${q.ui_rows_not_shown}`],
      ["Octets émis / reçus", `${st.bytes_tx} / ${st.bytes_rx}`],
      ["SIM_TIME / TICK_RESULT", `${st.sim_time_sent} / ${st.tick_results}`],
      ["A/R p95 / max", `${msText(st.rtt_p95_us, 2)} / ${msText(st.rtt_max_us, 2)}\u00a0ms`],
      ["Lots UI, intervalle p95", `${ui.batch_interval_ms.p95.toFixed(0)}\u00a0ms`],
      ["Rendu UI p95 / max", `${ui.render_ms.p95.toFixed(2)} / ${ui.render_ms.max.toFixed(1)}\u00a0ms`],
      ["Stimulus → affichage p50 / p95", `${ui.stimulus_to_display_ms.p50.toFixed(0)} / ${ui.stimulus_to_display_ms.p95.toFixed(0)}\u00a0ms`],
    ];
    const dl = $("#diag");
    if (dl.children.length !== diag.length) {
      clear(dl);
      for (const [k] of diag) {
        const row = el("div");
        row.appendChild(el("dt", undefined, k));
        row.appendChild(el("dd"));
        dl.appendChild(row);
      }
    }
    diag.forEach(([, v], i) => setText(dl.children[i].lastElementChild!, v));
    setText($("#frames-note"), q.ui_rows_not_shown ? `${q.ui_rows_not_shown} non affichées (débit > affichage)` : "");
  }

  // ------------------------------------------------------------------ frames table

  private setIdFilter(text: string): void {
    const t = text.trim().replace(/^0x/i, "");
    this.idFilter = /^[0-9a-fA-F]{1,8}$/.test(t) ? parseInt(t, 16) : null;
    $<HTMLInputElement>("#frame-filter").classList.toggle("invalid", t !== "" && this.idFilter === null);
    // Rebuild the visible rows from the store (at most ROWS_KEPT).
    const tbody = $("#frames tbody");
    clear(tbody);
    this.renderRows(this.store.rows);
    this.framesEmpty();
  }

  private passes(r: FrameRow): boolean {
    return this.idFilter === null || r.can_id === this.idFilter;
  }

  /**
   * At most ROWS_KEPT <tr> ever exist: once the table is full, the oldest row is moved to the
   * top and refilled. No element is created or destroyed at high frame rates. When the reader
   * has scrolled down, the view is held still (no jump as rows arrive above).
   */
  private renderRows(rows: FrameRow[]): void {
    if (!rows.length) return;
    this.framesEmpty();
    const tbody = $("#frames tbody");
    const wrap = $("#frames-wrap");
    const fresh = (rows.length > ROWS_KEPT ? rows.slice(rows.length - ROWS_KEPT) : rows).filter((r) => this.passes(r));
    if (!fresh.length) return;
    const anchored = wrap.scrollTop > 0;
    for (const r of fresh) {
      const tr = tbody.children.length >= ROWS_KEPT ? (tbody.lastElementChild as HTMLTableRowElement) : this.newRow();
      this.fillRow(tr, r);
      tbody.insertBefore(tr, tbody.firstChild);
    }
    if (anchored) wrap.scrollTop += fresh.length * ROW_HEIGHT;
  }

  private newRow(): HTMLTableRowElement {
    const tr = el("tr");
    for (const col of COLS) {
      const td = el("td", NUMERIC.has(col) ? "num" : undefined);
      td.dataset.col = col;
      tr.appendChild(td);
    }
    return tr;
  }

  private fillRow(tr: HTMLTableRowElement, r: FrameRow): void {
    const c = tr.children;
    c[0].textContent = sessionText(r.session_id);
    c[1].textContent = String(r.frame_seq);
    c[2].textContent = msText(r.t_us);
    c[3].textContent = idText(r.can_id, r.flags);
    c[4].textContent = kindText(r.flags);
    c[5].textContent = String(r.dlc);
    c[6].textContent = dataText(r.data);
    c[7].textContent = resultText(r);
    c[7].className = r.outcome === "result" && r.status === 0 && r.result_flags === 0 ? "r-ok" : `r-${r.outcome}`;
    c[8].textContent = publicationsText(r.entries);
    c[9].textContent = r.outcome === "result" ? String(r.cycles) : "";
    c[10].textContent = r.rtt_us !== null ? msText(r.rtt_us, 2) : "";
    this.shown.set(tr, r);
    this.markPast(tr, r);
    const sel = this.selected !== null && rowKey(this.selected) === rowKey(r);
    if (tr.classList.contains("selected") !== sel) tr.classList.toggle("selected", sel);
  }

  /** A frame of another session than the current one is greyed, with its session as title: the session column is
   * hidden on narrow windows, and a new session does not clear the table. */
  private markPast(tr: HTMLTableRowElement, r: FrameRow): void {
    const past = this.current !== null && r.session_id !== this.current;
    if (tr.classList.contains("past") !== past) tr.classList.toggle("past", past);
    const title = past ? `Session précédente (${sessionText(r.session_id)})` : "";
    if (tr.title !== title) tr.title = title;
  }

  private markPastRows(store: Store): void {
    const newest = store.rows.length ? store.rows[store.rows.length - 1].session_id : null;
    const current = store.status?.session?.id ?? newest;
    if (current !== this.current) {
      this.current = current;
      for (const tr of Array.from($("#frames tbody").children) as HTMLTableRowElement[]) {
        const row = this.shown.get(tr);
        if (row) this.markPast(tr, row);
      }
    }
    // sessions never interleave: the oldest kept frame tells whether an earlier session is still in the table
    const past = current !== null && store.rows.length > 0 && store.rows[0].session_id !== current;
    const legend = $("#frames-past");
    if (legend.hidden !== !past) legend.hidden = !past;
  }

  private select(r: FrameRow): void {
    this.selected = r;
    for (const tr of Array.from($("#frames tbody").children) as HTMLTableRowElement[]) {
      const row = this.shown.get(tr);
      tr.classList.toggle("selected", row !== undefined && rowKey(row) === rowKey(r));
    }
    const d = $("#frame-detail");
    clear(d);
    const kv = el("dl", "kv");
    const add = (k: string, v: string) => {
      kv.appendChild(el("dt", undefined, k));
      kv.appendChild(el("dd", undefined, v));
    };
    add("Session", sessionText(r.session_id));
    add("Trame", String(r.frame_seq));
    add("Temps bus", `${msText(r.t_us)}\u00a0ms`);
    add("ID", `${idText(r.can_id, r.flags)} · ${kindText(r.flags)}`);
    add("DLC / octets", `${r.dlc} / ${r.data.length / 2}`);
    add("Résultat", resultText(r));
    add("Cycles", r.outcome === "result" ? String(r.cycles) : "—");
    add("Aller-retour", r.rtt_us !== null ? `${msText(r.rtt_us, 2)}\u00a0ms` : "—");
    d.appendChild(kv);
    d.appendChild(el("p", "hint", "Données"));
    d.appendChild(el("p", "bytes", dataText(r.data)));
    d.appendChild(el("p", "hint", "Publications"));
    d.appendChild(el("p", "bytes", publicationsText(r.entries)));
  }

  // ------------------------------------------------------------------ side panel, console

  private renderSlots(store: Store): void {
    const tbody = $("#slots tbody");
    clear(tbody);
    const named = this.labels.size > 0;
    $("#slots").classList.toggle("named", named);
    for (const s of store.slots) {
      const tr = el("tr");
      const label = this.labels.get(s.slot);
      const rejected = s.kind === 1;
      const value = rejected ? "rejet" : f32Text(s.bits);
      if (label) {
        // named: the code, then the full public name under it (the text stays « A1 · name »), the value with its unit;
        // where it came from is the row's title
        const name = el("td", "slot-name");
        name.appendChild(el("span", "slot-code", label.code));
        name.appendChild(el("span", "slot-sep sr", " · "));
        name.appendChild(el("span", "slot-label", label.label));
        name.title = label.unit ? `${slotName(label)} (${label.unit})` : slotName(label);
        tr.appendChild(name);
        tr.appendChild(el("td", rejected ? "num rejected" : "num", rejected ? value : valueWithUnit(value, label.unit)));
        tr.title = slotOrigin(s.source, s.at, s.updates);
      } else {
        tr.appendChild(el("td", "slot-name", `s${s.slot}`));
        tr.appendChild(el("td", rejected ? "num rejected" : "num", value));
        tr.appendChild(el("td", "dim", s.source === "frame" ? `trame ${s.at}` : `tick ${msText(s.at, 0)}`));
        tr.appendChild(el("td", "num dim", String(s.updates)));
      }
      tbody.appendChild(tr);
    }
    $("#slots-empty").hidden = store.slots.length > 0;
  }

  private consoleLine(time: string, kind: string, kindClass: string, text: string): HTMLLIElement {
    const li = el("li");
    li.appendChild(el("span", "t", time));
    li.appendChild(el("span", `k ${kindClass}`, kind));
    li.appendChild(el("span", "m", text));
    if (this.consoleFilter && !text.toLowerCase().includes(this.consoleFilter)) li.classList.add("hide");
    return li;
  }

  private applyConsoleFilter(list: HTMLElement): void {
    for (const li of Array.from(list.children)) {
      const text = li.lastElementChild?.textContent?.toLowerCase() ?? "";
      li.classList.toggle("hide", this.consoleFilter !== "" && !text.includes(this.consoleFilter));
    }
  }

  private renderLogs(store: Store): void {
    const ol = $("#logs");
    const panel = ol.parentElement!;
    const follow = panel.scrollTop + panel.clientHeight >= panel.scrollHeight - 4;
    const frag = document.createDocumentFragment();
    for (const l of store.newLogs as LogRow[]) frag.appendChild(this.consoleLine(`${msText(l.t_us, 1)} ms`, "log", "log", l.text));
    ol.appendChild(frag);
    while (ol.children.length > LOGS_KEPT) ol.removeChild(ol.firstChild!);
    if (follow) panel.scrollTop = panel.scrollHeight;
    setText($("#count-logs"), String(store.logsReceived));
  }

  private renderErrors(store: Store): void {
    const ol = $("#errors");
    const frag = document.createDocumentFragment();
    // Newest on top, like the frames.
    for (let i = store.newErrors.length - 1; i >= 0; i--) {
      const e: ErrorRow = store.newErrors[i];
      frag.appendChild(this.consoleLine(`${(e.at_ms / 1000).toFixed(2)} s`, e.kind, e.kind, e.text));
    }
    ol.insertBefore(frag, ol.firstChild);
    while (ol.children.length > ERRORS_KEPT) ol.removeChild(ol.lastChild!);
    const count = $("#count-errors");
    setText(count, String(store.errorsReceived));
    const alarming = store.newErrors.some((e) => e.kind !== "info");
    if (alarming && this.shell.tab !== "link") count.classList.add("alert");
  }

  /** The shell switched tab: an inspected tab is no longer alerting. */
  tabShown(tab: string): void {
    if (tab === "link") $("#count-errors").classList.remove("alert");
  }
}
