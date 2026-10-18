"""`stm32lab app-test`: the N7RT CAN Lab application (app/) driven end to end.

The application runs its own end-to-end scenario (started with --e2e): it clicks its real controls, reads
back what its window displays and checks it, against the frames and answers that really crossed the link
(board scenarios, app/ui/src/e2e.ts, S1.4) or against the analytic expectations of the Physical Lab
(app/ui/src/lab/e2e-phys.ts, docs/physical-lab-v1.md §8.6 and §13). The board scenario mission-<id> (id = apps,
wsf, sas or bps) logs in as instructor and plays that mission with its reference pack (docs/apps-mission-slice.md §7):
raw bus of the mission vehicle, bench, then the hidden nominal check, whose verdict is reported and, with
--expect-verdict pass|fail, judged. The board scenario student-<id> plays the student flow (docs/distribution.md
§2.1): reset by the instructor, choice of the mission, group pack, group code, raw bus, then the nominal run recorded
as a run file, which this harness judges with `n7rt-tool judge` (exit code 0 = pass, 1 = fail, anything else = not
judged). Both fail if the board reboots during the mission, and both accept the ide- prefix (integrated IDE build and
flash of N7RT_E2E_DECODER_C first). They need, in the harness's environment (passed on to the application):
N7RT_INSTRUCTOR_DIR, N7RT_E2E_INSTRUCTOR_CODE; student-<id> also N7RT_E2E_PACK (group pack file) and
N7RT_E2E_GROUP_CODE. The board scenario progression-<id> plays a release 0.3 group pack from E0 to E5 (app/ui/src/
e2e-progression.ts): the student prelude, then every step through the real controls, its controlled failures,
« Reproduire l'anomalie » and, with ide-, the student code changed and flashed again; it needs N7RT_E2E_PACK,
N7RT_E2E_GROUP_CODE and the operator's answers file N7RT_E2E_ANSWERS (private tooling: never in a report), and
N7RT_E2E_STAGE = full, until-incident (the application is closed in incident 1) or resume (it is reopened there).
The harness then reads the final fiche the application exported (Documents/N7RT CAN Lab/fiches) and keeps only what
its checks need: states, verdicts, CRC-32, counts, the SHA-256 check. No code is ever written into a report. This
harness:
- board scenarios: resolves the board from the registry (guard: probe serial + MCU UID), never a COM
  number, and plays the outside world for the coordinated steps: halts the MCU over SWD for ~6 s like a
  debugger breakpoint, then holds the serial port like another program would;
- Physical Lab scenarios (phys, phys-perf): no board, no ST-LINK, no SWD step, no --serial;
- samples CPU and memory of the whole process tree (application + WebView processes) and, for the
  Physical Lab, the CPU of the core's `phys` thread;
- writes data/logs/app_<scenario>_[<board>_]<date>.json and evaluates the budgets.

Gates. The scenario publishes its current step in <report>.progress; a gated step waits for
<report>.<gate>.go, written once the harness has played its part:
  halt:ready  -> halt.go         MCU halted, then resumed, over SWD (board, --coordinated)
  busy:ready  -> busy.go         serial port held by the harness until busy:seen (board, --coordinated)
  shot:<name> -> shot-<name>.go  window captured to <report stem>_<name>.png
  mem:<label> -> mem-<label>.go  memory reference: the harness waits until the private memory of the tree
                                 is stable (at most MEM_SETTLE_TIMEOUT_S). mem:cockpit, published while the
                                 cockpit alone is shown, is the reference of the Physical Lab memory budget;
                                 mem:lab and mem:fin (phys-perf, at rest before and after the phases) are
                                 reported beside it.
Names are [A-Za-z0-9_-]. Physical Lab runs pass --coordinated: a harness is there to release the gates.
"""
from __future__ import annotations

import functools
import hashlib
import json
import math
import os
import re
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
APP_DIR = ROOT / "app"
EXE_NAME = "n7rt-can-lab.exe" if os.name == "nt" else "n7rt-can-lab"
WINDOW_TITLE = "N7RT CAN Lab"
# Tauri's main window has logical minima 1024x640. Use a conservative physical
# lower bound to reject minimized titlebars without depending on monitor DPI.
CAPTURE_MIN_WIDTH = 640
CAPTURE_MIN_HEIGHT = 400
SAMPLE_PERIOD_S = 0.5

# docs/can-lab-architecture.md section 14 (RTT measured with the example student code).
BUDGET_RTT_P99_MS = 10.0
BUDGET_CPU_PERCENT = 25.0
BUDGET_MEMORY_MB = 300.0
BUDGET_DISPLAY_LATENCY_MS = 100.0

# Missions (docs/apps-mission-slice.md §7, docs/distribution.md §2): board scenarios, one implementation for the four
# missions (ui/src/e2e-missions.ts). mission-<id> plays the instructor flow (the application's verdict), student-<id>
# the student flow (n7rt-tool's verdict on the run file); ide-<scenario> builds and flashes N7RT_E2E_DECODER_C with the
# integrated IDE first. The verdict is judged against --expect-verdict unless "any".
MISSIONS = ("apps", "wsf", "sas", "bps")
MISSION_SCENARIOS = tuple(f"{ide}{flow}-{mission}" for mission in MISSIONS for ide in ("", "ide-")
                          for flow in ("mission", "student", "progression"))
VERDICTS = ("pass", "fail", "any")
MISSION_SHOTS = ("mission-bus", "mission-bench", "mission-verdict")
STUDENT_SHOTS = ("student-locked", "student-recorded")
MISSION_MIN_IDENTIFIERS = 17
ENV_INSTRUCTOR_DIR = "N7RT_INSTRUCTOR_DIR"
ENV_INSTRUCTOR_CODE = "N7RT_E2E_INSTRUCTOR_CODE"
ENV_PACK = "N7RT_E2E_PACK"
ENV_GROUP_CODE = "N7RT_E2E_GROUP_CODE"
ENV_DECODER = "N7RT_E2E_DECODER_C"
ENV_ANSWERS = "N7RT_E2E_ANSWERS"
ENV_STAGE = "N7RT_E2E_STAGE"
PROGRESSION_STAGES = ("full", "until-incident", "resume")
# The tutorial captures (docs/tutoriel-etudiant-0.3.md), by stage: 01 to 08 before the application is closed in
# incident 1, 09 to 11 after it is reopened; a full run takes them all.
PROGRESSION_SHOTS = ("01-ouvrir", "02-progression", "03-fiche-nominale", "04-verdict-e0", "05-multimetre",
                     "06-fiche-incident", "07-consigner", "08-verdict-incident", "09-reproduire", "10-qualification",
                     "11-cloture")
# E0 to E5 with two or three attempts per step, 120 s apart (300 s in E4), plus the IDE flashes.
PROGRESSION_TIMEOUT_S = 5400.0
# Every verdict line the progression may show (workflow/session.rs): fixed texts, a date, a CP label. Anything else
# would be a detail the student must not get.
FEEDBACK_LINES = tuple(re.compile(p) for p in (
    r"Vérification réussie\. Étape validée le \d{2}/\d{2}/\d{4} à \d{2}:\d{2}\.",
    r"Incident [1-3] ouvert\. Le véhicule revient d'essai avec une anomalie\.",
    r"Qualification finale ouverte\.", r"Mission clôturée\.", r"La fiche finale est exportée dans le dossier Documents\.",
    r"Échec à corriger\.", r"Fiche nominale : (conforme|non conforme)", r"Essai nominal : (réussi|non réussi)",
    r"Diagnostic \(fiche\) : (conforme|non conforme)", r"Réparation \(essai caché\) : (vérifiée|non vérifiée)",
    r"Le détail n'est pas communiqué\. Reprenez vos mesures et vos essais\.",
    r"CP-[1-8] — (Recevoir et filtrer les trames de l'ECU|Décoder les valeurs nominales|Respecter les limites et les "
    r"valeurs invalides|Contrôler l'intégrité des messages|Superviser fraîcheur et séquence|Traiter un défaut signalé "
    r"par la source|Appliquer la logique de sécurité de la mission|Reprendre après un défaut) : (PASS|FAIL)",
))
PASSED_LINE = "Vérification réussie. Étape validée le "
# pack-<id>: the student application opens a release 0.3 pack on a machine without board (packaging smoke).
PACK_SCENARIOS = tuple(f"pack-{mission}" for mission in MISSIONS)


def is_student(scenario: str) -> bool:
    """student-<id> and ide-student-<id>: the student flow, whose run file n7rt-tool judges."""
    return scenario in MISSION_SCENARIOS and scenario.removeprefix("ide-").startswith("student-")


def is_progression(scenario: str) -> bool:
    """progression-<id> and ide-progression-<id>: a release 0.3 pack from E0 to E5."""
    return scenario in MISSION_SCENARIOS and scenario.removeprefix("ide-").startswith("progression-")


def _mission_env(scenario: str) -> tuple[str, ...]:
    needed = (ENV_INSTRUCTOR_DIR, ENV_INSTRUCTOR_CODE)
    if is_student(scenario):
        needed += (ENV_PACK, ENV_GROUP_CODE)
    if is_progression(scenario):
        needed += (ENV_PACK, ENV_GROUP_CODE, ENV_ANSWERS)
    if scenario.startswith("ide-"):
        needed += (ENV_DECODER,)
    return needed


MISSION_ENV = {scenario: _mission_env(scenario) for scenario in MISSION_SCENARIOS}
MISSION_ENV.update({scenario: (ENV_INSTRUCTOR_DIR, ENV_INSTRUCTOR_CODE, ENV_PACK, ENV_GROUP_CODE) for scenario in PACK_SCENARIOS})
TOOL_NAME = "n7rt-tool.exe" if os.name == "nt" else "n7rt-tool"
JUDGE_TIMEOUT_S = 180.0

# Physical Lab, docs/physical-lab-v1.md §8.6, without a board. T = median rAF gap over 2 s at rest.
PHYS_SCENARIOS = ("phys", "phys-perf")
PHYS_THREAD = "phys"                    # the core's thread (§8.1)
PHYS_TIMEOUT_S = 300.0
BUDGET_PHYS_LOAD_MS = 200.0             # perf-150-300 loaded -> first frame
BUDGET_FRAME_GAP_P95_T = 1.1
BUDGET_FRAME_GAP_P99_T = 2.0
BUDGET_FRAME_GAP_P99_SLACK_MS = 1.0    # p99 <= 2 T + 1 ms: one missed vsync plus timestamp jitter (§8.6)
BUDGET_FRAME_GAP_MAX_MS = 50.0
BUDGET_RENDER_P95_MS = 8.0
BUDGET_RENDER_P99_MS = 12.0
LONG_FRAME_MS = 50.0                    # none allowed
BUDGET_INPUT_TO_DRAW_P95_MS = 50.0
BUDGET_EDIT_TO_VALUES_P95_MS = 100.0
BUDGET_ENGINE_LAG_P99_MS = 20.0
BUDGET_PHYS_MEMORY_DELTA_MB = 40.0      # vs the cockpit alone (§1.8)
BUDGET_PHYS_THREAD_CPU_PERCENT = 5.0    # share of the machine, proposed
# Memory reference (mem:<label>): private memory within the tolerance over the whole window.
MEM_SETTLE_WINDOW_S = 2.0
MEM_SETTLE_TOLERANCE_MB = 2.0
MEM_SETTLE_TIMEOUT_S = 15.0
# Idle time before a mark (seconds); with MEM_SETTLE_TIMEOUT_S it stays under the scenario's 30 s gate timeout.
MEM_IDLE_BEFORE_S = {"fin": 12.0}

_GATE_NAME = re.compile(r"[A-Za-z0-9_-]{1,64}")
_CAPTURED = re.compile(r"screenshot \S+\.png")


def mission_env_missing(scenario: str, environ=None) -> list[str]:
    """The environment variables a mission scenario needs and that are not set (names only, never values)."""
    env = os.environ if environ is None else environ
    return [k for k in MISSION_ENV.get(scenario, ()) if not env.get(k)]


def tool_exe(app: Path, explicit: Path | None = None) -> Path:
    """`explicit` (--tool: the instructor tools archive's n7rt-tool, built with the oracle), else n7rt-tool next to
    the application (same cargo profile). A public n7rt-tool cannot judge (docs/distribution.md §1)."""
    return explicit.resolve() if explicit is not None else app.parent / TOOL_NAME


def mission_of(scenario: str) -> str:
    """Mission played by a mission scenario: mission-wsf, ide-student-bps -> wsf, bps."""
    return scenario.rsplit("-", 1)[-1]


def report_dir(scenario: str, default: Path, environ=None) -> Path:
    """Where a scenario writes its report and screenshots. A mission scenario shows the reference variant's bus and
    verdicts: it writes under the instructor directory (missions/<mission>/reports/app-test/, gitignored there), never
    into the student tree. Other scenarios: `default` (data/logs)."""
    env = os.environ if environ is None else environ
    if (scenario in MISSION_SCENARIOS or scenario in PACK_SCENARIOS) and env.get(ENV_INSTRUCTOR_DIR):
        return Path(env[ENV_INSTRUCTOR_DIR]) / "missions" / mission_of(scenario) / "reports" / "app-test"
    return default


def judge_run_file(tool: Path, instructor_dir: str, run_file: str, code: str, *,
                   timeout_s: float = JUDGE_TIMEOUT_S) -> dict:
    """`n7rt-tool judge <instructor dir> <run file> --code-stdin`, the code on its standard input. The tool's output
    (private detail) is not kept; only its exit code: 0 = pass, 1 = fail, anything else = not judged (the last line of
    its error output, which names no secret, is kept then)."""
    if not Path(tool).is_file():
        return {"verdict": None, "exit_code": None, "error": f"{Path(tool).name} not found: cargo build -p n7rt-tool"}
    try:
        done = subprocess.run([str(tool), "judge", str(instructor_dir), str(run_file), "--code-stdin"],
                              input=code + "\n", capture_output=True, text=True, timeout=timeout_s, cwd=APP_DIR)
    except subprocess.TimeoutExpired:
        return {"verdict": None, "exit_code": None, "error": f"n7rt-tool judge: no answer after {timeout_s:.0f} s"}
    out = {"exit_code": done.returncode, "verdict": {0: "pass", 1: "fail"}.get(done.returncode)}
    if out["verdict"] is None:
        lines = [ln.strip() for ln in (done.stderr or "").splitlines() if ln.strip()]
        out["error"] = (lines[-1][:200] if lines else "no message") + f" (exit code {done.returncode})"
    return out


def documents_dir() -> Path:
    """The user's Documents folder as the application finds it (known folder, possibly redirected to OneDrive)."""
    if os.name == "nt":
        import ctypes
        import uuid
        from ctypes import wintypes

        class Guid(ctypes.Structure):
            _fields_ = [("data1", wintypes.DWORD), ("data2", wintypes.WORD), ("data3", wintypes.WORD),
                        ("data4", ctypes.c_ubyte * 8)]

        guid = Guid()
        ctypes.memmove(ctypes.byref(guid), uuid.UUID("{FDD39AD0-238F-46AF-ADB4-6C85480369C7}").bytes_le, 16)
        path = ctypes.c_wchar_p()
        if ctypes.windll.shell32.SHGetKnownFolderPath(ctypes.byref(guid), 0, None, ctypes.byref(path)) == 0:
            try:
                return Path(path.value)
            finally:
                ctypes.windll.ole32.CoTaskMemFree(path)
    return Path.home() / "Documents"


def _safe_name(s: str) -> str:
    """workflow/export.rs folder(): the characters kept in a folder name."""
    return "".join(c for c in s if c.isascii() and (c.isalnum() or c in "-_"))[:40]


def read_export(mission: str, group: str, since: float, root: Path | None = None) -> dict:
    """The final fiche exported for (mission, group) since `since` (Unix time): what the checks need only. The fiche
    holds the group's answers, which never enter a report: states, verdicts, CRC-32, counts and the SHA-256 check."""
    base = (root or documents_dir()) / "N7RT CAN Lab" / "fiches"
    prefix = f"{_safe_name(mission)}-{_safe_name(group)}-"
    folders = sorted((p for p in base.glob(prefix + "*") if p.is_dir() and p.stat().st_mtime >= since - 5),
                     key=lambda p: p.stat().st_mtime)
    if not folders:
        return {}
    folder = folders[-1]
    raw = (folder / "fiche.json").read_bytes()
    doc = json.loads(raw)
    page = folder / "fiche.html"
    sha = hashlib.sha256(raw).hexdigest()
    incidents = [i for i in _list(doc.get("incidents")) if isinstance(i, dict)]
    return {
        "folder_name": folder.name,
        "complete": page.is_file() and (folder / "scellés" / "fiche-finale.sealed").is_file(),
        "sha256_ok": page.is_file() and f"Empreinte SHA-256 de fiche.json : {sha}" in page.read_text(encoding="utf-8"),
        "etapes": [_dict(e).get("etat") for e in _list(doc.get("etapes"))],
        "temps_actif": doc.get("temps_actif"),
        "reproductions": len(_list(doc.get("reproductions"))),
        "tentatives": [{k: _dict(t).get(k) for k in ("etape", "verdict", "diagnostic", "reparation", "crc32", "annulation")}
                       for t in _list(doc.get("tentatives"))],
        "reparations": [{"rang": i.get("rang"), "crc32_activation": _dict(i.get("reparation")).get("crc32_activation"),
                         "crc32_validation": _dict(i.get("reparation")).get("crc32_validation"),
                         "circuit_lines": len(_list(_dict(i.get("reparation")).get("circuit")))} for i in incidents],
        "competences": _dict(doc.get("qualification")).get("competences"),
    }


def app_exe(release: bool, explicit: Path | None = None) -> Path:
    """`explicit` (--exe, e.g. the instructor edition from the instructor tools archive), else the build of this
    repository (app/target/release or debug)."""
    exe = explicit.resolve() if explicit is not None else APP_DIR / "target" / ("release" if release else "debug") / EXE_NAME
    if not exe.is_file():
        raise FileNotFoundError(f"{exe} absent: build the application first (app/README.md)")
    return exe


class TreeSampler:
    """CPU (share of the whole machine) and memory of a process and all its descendants; optionally the CPU of
    one named thread of the root process (the Physical Lab core's `phys` thread)."""

    def __init__(self, pid: int, thread: str | None = None) -> None:
        import psutil
        self.psutil = psutil
        self.root = psutil.Process(pid)
        self.known: dict[int, object] = {}
        self.samples: list[dict] = []
        self.ncpu = psutil.cpu_count() or 1
        self.thread = thread
        self.primed = 0  # processes seen for the first time by the last sample (not measured yet)
        # "<name>:<pid>" -> role: app, or the WebView2 process type (browser, renderer, gpu-process, utility:...)
        self.roles: dict[str, str] = {}
        self._tid: int | None = None
        self._thread_last: tuple[float, float] | None = None  # (t, CPU seconds) of the thread

    def sample(self, t: float) -> None:
        ps = self.psutil
        try:
            procs = [self.root, *self.root.children(recursive=True)]
        except ps.Error:
            return
        cpu = ws = private = 0.0
        names: dict[str, int] = {}
        per: dict[str, float] = {}
        self.primed = 0
        for p in procs:
            try:
                if p.pid not in self.known:
                    p.cpu_percent(None)  # first call primes the counter
                    self.known[p.pid] = p
                    self.roles[f"{p.name()}:{p.pid}"] = _process_role(p, self.root.pid)
                    self.primed += 1
                    continue
                cpu += self.known[p.pid].cpu_percent(None)
                mem = p.memory_info()
                ws += mem.rss
                private += getattr(mem, "private", mem.rss)
                names[p.name()] = names.get(p.name(), 0) + 1
                per[f"{p.name()}:{p.pid}"] = round(getattr(mem, "private", mem.rss) / 2**20, 1)
            except ps.Error:
                continue
        row = {"t": round(t, 2), "cpu_percent": round(cpu / self.ncpu, 2),
               "working_set_mb": round(ws / 2**20, 1), "private_mb": round(private / 2**20, 1),
               "processes": names, "per_process_private_mb": per}
        if self.thread is not None:
            share = self._thread_share(t)
            row["thread_cpu_percent"] = None if share is None else round(share, 2)
        self.samples.append(row)

    def _thread_share(self, t: float) -> float | None:
        """CPU of the named thread since the previous sample, % of the whole machine (None until measured)."""
        try:
            times = {th.id: th.user_time + th.system_time for th in self.root.threads()}
        except self.psutil.Error:
            return None
        pid = self.root.pid
        if self._tid not in times or thread_name(pid, self._tid) != self.thread:
            self._tid = next((tid for tid in sorted(times) if thread_name(pid, tid) == self.thread), None)
            self._thread_last = None
            if self._tid is None:
                return None
        last, self._thread_last = self._thread_last, (t, times[self._tid])
        if last is None or t <= last[0]:
            return None
        return max(0.0, (times[self._tid] - last[1]) / (t - last[0]) / self.ncpu * 100)

    def settle(self, elapsed, *, window_s: float, tolerance_mb: float, timeout_s: float) -> dict:
        """Samples until the private memory of the tree stays within tolerance_mb over window_s, then returns that
        reference (e.g. the cockpit alone). A process appearing restarts the window: its memory is not counted
        before its second sample."""
        start = elapsed()
        run: list[dict] = []
        while True:
            now = elapsed()
            before = len(self.samples)
            self.sample(now)
            fresh = self.samples[before:]
            if fresh and fresh[-1]["processes"] and not self.primed:
                run.append(fresh[-1])
            else:
                run = []
            if run and run[-1]["t"] - run[0]["t"] >= window_s:
                window = [s for s in run if s["t"] >= run[-1]["t"] - window_s]
                spread = max(s["private_mb"] for s in window) - min(s["private_mb"] for s in window)
                if spread <= tolerance_mb:
                    return _mark(run[-1], True, spread)
            if now - start >= timeout_s or not self._alive():
                return _mark(run[-1] if run else None, False, None)
            time.sleep(SAMPLE_PERIOD_S)

    def _alive(self) -> bool:
        try:
            return self.root.is_running()
        except self.psutil.Error:
            return False

    def summary(self, after: float = 0.0) -> dict:
        rows = [s for s in self.samples if s["t"] >= after and s["processes"]]
        if not rows:
            return {}
        cpu = [s["cpu_percent"] for s in rows]
        out = {
            "samples": len(rows),
            "cpu_percent_mean": round(sum(cpu) / len(cpu), 2),
            "cpu_percent_max": max(cpu),
            "working_set_mb_max": max(s["working_set_mb"] for s in rows),
            "private_mb_max": max(s["private_mb"] for s in rows),
            "private_mb_first": rows[0]["private_mb"],
            "private_mb_last": rows[-1]["private_mb"],
            "processes": rows[-1]["processes"],
            "per_process_private_mb_first": rows[0].get("per_process_private_mb"),
            "per_process_private_mb_last": rows[-1].get("per_process_private_mb"),
        }
        peak = max(rows, key=lambda s: s["private_mb"])
        out["peak"] = {"t": peak["t"], "private_mb": peak["private_mb"],
                       "per_process_private_mb": peak.get("per_process_private_mb")}
        if self.thread is not None:
            shares = [s["thread_cpu_percent"] for s in rows if s.get("thread_cpu_percent") is not None]
            out.update({"thread": self.thread,
                        "thread_cpu_percent_mean": round(sum(shares) / len(shares), 2) if shares else None,
                        "thread_cpu_percent_max": max(shares) if shares else None})
        return out


def _mark(row: dict | None, settled: bool, spread: float | None) -> dict:
    if row is None:
        return {"t": None, "private_mb": None, "working_set_mb": None, "processes": {}, "settled": False,
                "spread_mb": None}
    return {"t": row["t"], "private_mb": row["private_mb"], "working_set_mb": row["working_set_mb"],
            "processes": row["processes"], "settled": settled,
            "spread_mb": None if spread is None else round(spread, 1),
            "per_process_private_mb": row.get("per_process_private_mb")}


def _process_role(p, root_pid: int) -> str:
    """What a process of the tree is: the application itself, or a WebView2 (Chromium) process by its --type
    (no --type: the browser process); a utility process is named by its service."""
    if p.pid == root_pid:
        return "app"
    try:
        args = p.cmdline()
    except Exception:
        return p.name()
    kind = next((a.split("=", 1)[1] for a in args if a.startswith("--type=")), None)
    if kind is None:
        return "browser" if p.name().lower().startswith("msedgewebview2") else p.name()
    if kind == "utility":
        sub = next((a.split("=", 1)[1] for a in args if a.startswith("--utility-sub-type=")), "")
        return f"utility:{sub.rsplit('.', 1)[-1]}" if sub else kind
    return kind


def thread_name(pid: int, tid: int) -> str | None:
    """OS name of a thread: its description on Windows (set by Rust's thread::Builder::name), comm on Linux."""
    if os.name == "nt":
        return _windows_thread_description(tid)
    if sys.platform.startswith("linux"):
        try:
            return Path(f"/proc/{pid}/task/{tid}/comm").read_text(encoding="utf-8", errors="replace").strip()
        except OSError:
            return None
    return None


@functools.lru_cache(maxsize=1)
def _kernel32():
    import ctypes
    from ctypes import wintypes
    k = ctypes.WinDLL("kernel32", use_last_error=True)
    k.OpenThread.restype = wintypes.HANDLE
    k.OpenThread.argtypes = (wintypes.DWORD, wintypes.BOOL, wintypes.DWORD)
    k.GetThreadDescription.restype = ctypes.c_long  # HRESULT
    k.GetThreadDescription.argtypes = (wintypes.HANDLE, ctypes.POINTER(ctypes.c_void_p))
    k.LocalFree.restype = ctypes.c_void_p
    k.LocalFree.argtypes = (ctypes.c_void_p,)
    k.CloseHandle.restype = wintypes.BOOL
    k.CloseHandle.argtypes = (wintypes.HANDLE,)
    return k


def _windows_thread_description(tid: int) -> str | None:
    import ctypes
    try:
        k = _kernel32()
    except (AttributeError, OSError):  # GetThreadDescription: Windows 10 1607 and later
        return None
    handle = k.OpenThread(0x0800, False, tid)  # THREAD_QUERY_LIMITED_INFORMATION
    if not handle:
        return None
    try:
        text = ctypes.c_void_p()
        if k.GetThreadDescription(handle, ctypes.byref(text)) < 0 or not text.value:
            return None
        try:
            return ctypes.wstring_at(text.value)
        finally:
            k.LocalFree(text)
    finally:
        k.CloseHandle(handle)


def _render_window(hwnd: int):
    """The window's own pixels through PrintWindow(PW_RENDERFULLCONTENT): WebView2 content included, whatever
    covers the window on screen. Returns (image, window rect) or None."""
    import ctypes
    from ctypes import wintypes
    from PIL import Image
    user32, gdi32 = ctypes.WinDLL("user32"), ctypes.WinDLL("gdi32")
    user32.IsIconic.argtypes = [wintypes.HWND]
    user32.IsIconic.restype = wintypes.BOOL
    if user32.IsIconic(hwnd):
        return None
    user32.GetWindowRect.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.RECT)]
    user32.GetWindowDC.argtypes = [wintypes.HWND]
    user32.GetWindowDC.restype = wintypes.HDC
    user32.ReleaseDC.argtypes = [wintypes.HWND, wintypes.HDC]
    user32.PrintWindow.argtypes = [wintypes.HWND, wintypes.HDC, wintypes.UINT]
    gdi32.CreateCompatibleDC.argtypes = [wintypes.HDC]
    gdi32.CreateCompatibleDC.restype = wintypes.HDC
    gdi32.CreateCompatibleBitmap.argtypes = [wintypes.HDC, ctypes.c_int, ctypes.c_int]
    gdi32.CreateCompatibleBitmap.restype = wintypes.HBITMAP
    gdi32.SelectObject.argtypes = [wintypes.HDC, wintypes.HGDIOBJ]
    gdi32.SelectObject.restype = wintypes.HGDIOBJ
    gdi32.DeleteObject.argtypes = [wintypes.HGDIOBJ]
    gdi32.DeleteDC.argtypes = [wintypes.HDC]
    gdi32.GetDIBits.argtypes = [wintypes.HDC, wintypes.HBITMAP, wintypes.UINT, wintypes.UINT, ctypes.c_void_p,
                                ctypes.c_void_p, wintypes.UINT]

    class BitmapInfoHeader(ctypes.Structure):
        _fields_ = [("biSize", wintypes.DWORD), ("biWidth", wintypes.LONG), ("biHeight", wintypes.LONG),
                    ("biPlanes", wintypes.WORD), ("biBitCount", wintypes.WORD), ("biCompression", wintypes.DWORD),
                    ("biSizeImage", wintypes.DWORD), ("biXPelsPerMeter", wintypes.LONG),
                    ("biYPelsPerMeter", wintypes.LONG), ("biClrUsed", wintypes.DWORD),
                    ("biClrImportant", wintypes.DWORD)]

    wr = wintypes.RECT()
    if not user32.GetWindowRect(hwnd, ctypes.byref(wr)):
        return None
    w, h = wr.right - wr.left, wr.bottom - wr.top
    if w < CAPTURE_MIN_WIDTH or h < CAPTURE_MIN_HEIGHT:
        return None
    hdc = user32.GetWindowDC(hwnd)
    mem = gdi32.CreateCompatibleDC(hdc)
    bmp = gdi32.CreateCompatibleBitmap(hdc, w, h)
    old = gdi32.SelectObject(mem, bmp)
    try:
        if not user32.PrintWindow(hwnd, mem, 2):  # PW_RENDERFULLCONTENT
            return None
        bi = BitmapInfoHeader(ctypes.sizeof(BitmapInfoHeader), w, -h, 1, 32, 0, 0, 0, 0, 0, 0)  # top-down BGRX
        buf = ctypes.create_string_buffer(w * h * 4)
        if gdi32.GetDIBits(mem, bmp, 0, h, buf, ctypes.byref(bi), 0) != h:
            return None
        img = Image.frombuffer("RGB", (w, h), buf, "raw", "BGRX", 0, 1)
        return (img, wr) if img.getbbox() else None  # all black: nothing rendered
    finally:
        gdi32.SelectObject(mem, old)
        gdi32.DeleteObject(bmp)
        gdi32.DeleteDC(mem)
        user32.ReleaseDC(hwnd, hdc)


def capture_window(title: str, out: Path) -> bool:
    """Screenshot of the application window (Windows). The window renders itself, so another window covering it
    never ends up in the capture; a screen grab of its rectangle is the fallback."""
    if os.name != "nt":
        return False
    import ctypes
    from ctypes import wintypes
    from PIL import ImageGrab
    user32 = ctypes.windll.user32
    try:
        user32.SetProcessDpiAwarenessContext(ctypes.c_void_p(-4))  # per-monitor v2: physical pixels
    except (AttributeError, OSError):
        user32.SetProcessDPIAware()
    hwnd = user32.FindWindowW(None, title)
    if not hwnd or user32.IsIconic(hwnd):
        return False
    rect = wintypes.RECT()
    # Visible frame: GetWindowRect also covers the invisible resize borders (desktop pixels).
    if ctypes.windll.dwmapi.DwmGetWindowAttribute(hwnd, 9, ctypes.byref(rect), ctypes.sizeof(rect)) != 0:
        if not user32.GetWindowRect(hwnd, ctypes.byref(rect)):
            return False
    if rect.right - rect.left < CAPTURE_MIN_WIDTH or rect.bottom - rect.top < CAPTURE_MIN_HEIGHT:
        return False
    rendered = _render_window(hwnd)
    # A user can minimize the window between geometry discovery and PrintWindow.
    if user32.IsIconic(hwnd):
        return False
    if rendered:
        img, wr = rendered
        img.crop((rect.left - wr.left, rect.top - wr.top, rect.right - wr.left, rect.bottom - wr.top)).save(out)
        return True
    ImageGrab.grab(bbox=(rect.left, rect.top, rect.right, rect.bottom), all_screens=True).save(out)
    return True


def _read(path: Path) -> str:
    try:
        return path.read_text(encoding="utf-8").strip()
    except OSError:
        return ""


def _load_report(report: Path) -> dict:
    """The application's report, or a failed result saying why there is none."""
    if not report.is_file():
        return {"ok": False, "error": "no report"}
    try:
        result = json.loads(report.read_text(encoding="utf-8"))
    except (OSError, ValueError) as exc:
        return {"ok": False, "error": f"unreadable report: {exc}"}
    return result if isinstance(result, dict) else {"ok": False, "error": "report is not a JSON object"}


def _release(report: Path, gate: str) -> None:
    Path(str(report) + f".{gate}.go").write_text("go", encoding="utf-8")


def _gate_name(step: str, elapsed, events: list[dict]) -> str | None:
    """The name after 'shot:' or 'mem:' when it is safe in a file name; otherwise the step is reported and never
    released (the scenario then fails on its own timeout)."""
    name = step.split(":", 1)[1]
    if _GATE_NAME.fullmatch(name):
        return name
    events.append({"t": round(elapsed(), 2), "event": f"step {step!r} ignored: invalid name"})
    return None


def _screenshot(report: Path, step: str, elapsed, events: list[dict]) -> None:
    name = _gate_name(step, elapsed, events)
    if name is None:
        return
    shot = report.parent / f"{report.stem}_{name}.png"
    try:
        text = f"screenshot {shot.name}" if capture_window(WINDOW_TITLE, shot) else "screenshot failed"
    except Exception as exc:  # noqa: BLE001 - a failed capture is reported, it never blocks the scenario
        text = f"screenshot failed: {exc}"
    events.append({"t": round(elapsed(), 2), "event": text})
    _release(report, f"shot-{name}")


# Chromium slows down, then stops, a window that is covered (another application in front) or in the background:
# timers throttled to one per minute, no frames. A scenario run must not depend on what the operator does with the
# other windows. Tauri's own defaults are kept (the variable replaces the arguments the application passes).
WEBVIEW_ARGS = ("--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection,CalculateNativeWinOcclusion "
                "--disable-background-timer-throttling --disable-renderer-backgrounding --disable-backgrounding-occluded-windows")


def webview_env(base) -> dict:
    """The environment of an application run by the harness: WebView2 never throttles a covered window."""
    env = dict(base)
    env["WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS"] = WEBVIEW_ARGS
    return env


def _drive(args: list[str], report: Path, timeout_s: float, on_step, *, thread: str | None = None,
           cleanup=None) -> tuple[dict, TreeSampler, list[dict], int | None, float]:
    """Launches the application, samples its process tree and hands each new progress step to
    on_step(step, now, elapsed, sampler, events). Returns (report or failure, sampler, events, exit code, duration)."""
    for p in report.parent.glob(report.name + "*"):
        p.unlink()
    progress_file = Path(str(report) + ".progress")
    events: list[dict] = []
    t0 = time.monotonic()

    def elapsed() -> float:
        return time.monotonic() - t0

    # Exercise an installed release: never let the source working directory supply missing resources.
    proc = subprocess.Popen(args, cwd=Path(args[0]).resolve().parent, env=webview_env(os.environ),
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        sampler = TreeSampler(proc.pid, thread=thread)
        last = ""
        while proc.poll() is None:
            now = elapsed()
            if now > timeout_s:
                proc.kill()
                events.append({"t": round(now, 2), "event": "timeout"})
                break
            sampler.sample(now)
            step = _read(progress_file)
            if step and step != last:
                events.append({"t": round(now, 2), "step": step})
                last = step
                on_step(step, now, elapsed, sampler, events)
            time.sleep(SAMPLE_PERIOD_S)
    except BaseException:
        proc.kill()  # the harness itself failed (SWD, serial port, Ctrl+C): never leave the window behind
        raise
    finally:
        if cleanup is not None:
            cleanup()
    duration = elapsed()
    try:
        proc.wait(timeout=10)  # reaps a killed application: its exit code is then known
    except subprocess.TimeoutExpired:
        pass
    return _load_report(report), sampler, events, proc.returncode, duration


def run(exe: Path, scenario: str, sn: str, port: str, report: Path, *, seconds: int, rate: int,
        coordinated: bool, halt, resume, timeout_s: float, expect_verdict: str = "any",
        tool: Path | None = None) -> dict:
    """Launches the application in e2e mode on a board, plays the coordinated steps, returns the result."""
    if expect_verdict not in VERDICTS:
        raise ValueError(f"expect_verdict: {expect_verdict} (not in {VERDICTS})")
    missing = mission_env_missing(scenario)
    if missing:
        raise ValueError(f"{scenario}: environment variables not set: {', '.join(missing)}")
    args = [str(exe), "--e2e", scenario, "--serial", sn, "--report", str(report),
            "--seconds", str(seconds), "--rate", str(rate)]
    if coordinated:
        args.append("--coordinated")
    if scenario in MISSION_SCENARIOS:
        args += ["--expect-verdict", expect_verdict]
    if is_progression(scenario) and os.environ.get(ENV_STAGE, "full") not in PROGRESSION_STAGES:
        raise ValueError(f"{ENV_STAGE}: {os.environ.get(ENV_STAGE)} (not in {PROGRESSION_STAGES})")
    started = time.time()
    held = None

    def on_step(step: str, now: float, elapsed, _sampler: TreeSampler, events: list[dict]) -> None:
        nonlocal held
        if step == "halt:ready":
            events.append({"t": round(now, 2), "event": "MCU halted (SWD)"})
            halt()
            resume()
            events.append({"t": round(elapsed(), 2), "event": "MCU resumed"})
            _release(report, "halt")
        elif step == "busy:ready":
            import serial
            held = serial.Serial(port, 921600, timeout=0.1)
            events.append({"t": round(elapsed(), 2), "event": f"{port} held by the harness"})
            _release(report, "busy")
        elif step.startswith("shot:"):
            _screenshot(report, step, elapsed, events)
        elif step == "busy:seen" and held is not None:
            held.close()
            held = None
            events.append({"t": round(elapsed(), 2), "event": f"{port} released"})

    def release_port() -> None:
        if held is not None:
            held.close()

    result, sampler, events, exit_code, duration = _drive(args, report, timeout_s, on_step, cleanup=release_port)
    if is_student(scenario):
        run_file = result.get("run_file")
        if isinstance(run_file, str) and run_file:
            result["judge"] = judge_run_file(tool_exe(exe, tool), os.environ[ENV_INSTRUCTOR_DIR], run_file,
                                             os.environ[ENV_INSTRUCTOR_CODE])
        else:
            result["judge"] = {"verdict": None, "exit_code": None, "error": "no run file reported"}
        events.append({"t": round(duration, 2), "event": f"n7rt-tool judge: {result['judge'].get('verdict')}"})
    closed = _dict(_dict(result.get("progression")).get("closed"))
    if is_progression(scenario) and closed.get("mission") and closed.get("group"):
        try:
            result["export"] = read_export(str(closed["mission"]), str(closed["group"]), started)
        except (OSError, ValueError) as exc:
            result["export"] = {"error": f"final fiche unreadable: {exc}"}
    load_start = next((e["t"] for e in events if e.get("step") == "charge"), 0.0)
    result["harness"] = {
        "exe_sha256": hashlib.sha256(exe.read_bytes()).hexdigest(),
        "working_directory": str(exe.resolve().parent),
        "exit_code": exit_code,
        "duration_s": round(duration, 1),
        "events": events,
        "process_tree": sampler.summary(),
        "process_tree_under_load": sampler.summary(after=load_start + 2.0) if load_start else None,
        "samples": sampler.samples,
    }
    return result


def phys_args(exe: Path, scenario: str, report: Path, *, seconds: int = 0) -> list[str]:
    """Command line of a Physical Lab scenario: no --serial, no board; --coordinated: the harness releases the
    shot: and mem: gates."""
    if not 0 <= seconds <= 86400:
        raise ValueError("Physical Lab duration must be between 0 and 86400 seconds")
    args = [str(exe), "--e2e", scenario, "--report", str(report), "--coordinated"]
    if seconds:
        args += ["--seconds", str(seconds)]
    return args


def run_phys(exe: Path, scenario: str, report: Path, *, timeout_s: float = PHYS_TIMEOUT_S, seconds: int = 0) -> dict:
    """Launches a Physical Lab scenario without any board (no ST-LINK, no SWD, no serial port), plays the shot:
    and mem: gates, returns the result."""
    marks: dict[str, dict] = {}
    released_at: dict[str, float] = {}

    def on_step(step: str, _now: float, elapsed, sampler: TreeSampler, events: list[dict]) -> None:
        if step.startswith("shot:"):
            _screenshot(report, step, elapsed, events)
        elif step.startswith("mem:"):
            label = _gate_name(step, elapsed, events)
            if label is None:
                return
            # After the phases, Chromium trims its caches once idle (V8 memory reducer, GPU resource cleanup):
            # the mark is taken after that idle time.
            idle_end = elapsed() + MEM_IDLE_BEFORE_S.get(label, 0.0)
            while elapsed() < idle_end:
                sampler.sample(elapsed())
                time.sleep(SAMPLE_PERIOD_S)
            mark = sampler.settle(elapsed, window_s=MEM_SETTLE_WINDOW_S, tolerance_mb=MEM_SETTLE_TOLERANCE_MB,
                                  timeout_s=MEM_SETTLE_TIMEOUT_S)
            marks[label] = mark
            events.append({"t": round(elapsed(), 2),
                           "event": f"memory reference {label}: private {mark['private_mb']} MB"
                                    + ("" if mark["settled"] else " (not settled)")})
            _release(report, f"mem-{label}")
            released_at[label] = elapsed()

    args = phys_args(exe, scenario, report, seconds=seconds)
    result, sampler, events, exit_code, duration = _drive(args, report, timeout_s, on_step, thread=PHYS_THREAD)
    first_step = next((e["t"] for e in events if "step" in e), None)
    start = released_at.get("cockpit", first_step)
    result["harness"] = {
        "exe_sha256": hashlib.sha256(exe.read_bytes()).hexdigest(),
        "working_directory": str(exe.resolve().parent),
        "command": args,
        "exit_code": exit_code,
        "duration_s": round(duration, 1),
        "events": events,
        "memory_marks": marks,
        "process_tree": sampler.summary(),
        # The Physical Lab part of the run: after the cockpit reference, else after the first step.
        "process_tree_under_load": sampler.summary(after=start) if start is not None else None,
        "process_roles": sampler.roles,
        "samples": sampler.samples,
    }
    return result


def evaluate(result: dict, scenario: str, seconds: int = 0, rate: int = 0,
             expect_verdict: str = "any") -> list[tuple[str, bool, str]]:
    """Acceptance checks on one run: S1.4 for the board scenarios, docs/physical-lab-v1.md §8.6 for the
    Physical Lab ones, docs/apps-mission-slice.md §7 for the missions."""
    if scenario == "ide" or (scenario.startswith("ide-") and scenario in MISSION_SCENARIOS):
        workflow = result.get("ide") or []
        detail = f"{len(workflow)} integrated workflow checks"
        checks = [("IDE edit/build/flash/reconnect", bool(workflow) and all(c.get("ok") for c in workflow), detail)]
        if scenario == "ide" and (harness := _dict(result.get("harness"))):
            checks.append(_shots_check(harness))
        return checks + evaluate(result, "functional" if scenario == "ide" else scenario.removeprefix("ide-"), seconds, rate, expect_verdict)
    if scenario in PHYS_SCENARIOS:
        return _evaluate_phys(result, scenario)
    if scenario in PACK_SCENARIOS:
        harness = _dict(result.get("harness"))
        opened = _step_detail(result, "progression-initiale")
        return [_steps_check(result), _exit_check(harness),
                ("release 0.3 pack opened: E0 in progress, nothing else", bool(opened.get("steps")), str(opened.get("steps"))),
                _shots_check(harness)]
    if is_student(scenario):
        return _evaluate_student(result, expect_verdict)
    if is_progression(scenario):
        return _evaluate_progression(result)
    if scenario in MISSION_SCENARIOS:
        return _evaluate_mission(result, expect_verdict)
    checks: list[tuple[str, bool, str]] = []
    steps = result.get("steps", [])
    checks.append(("scenario steps", bool(result.get("ok")) and all(s["ok"] for s in steps),
                   ", ".join(f"{s['name']}{'' if s['ok'] else ' FAIL'}" for s in steps) + (f" — {result.get('error')}" if result.get("error") else "")))
    ver = result.get("verifications", [])
    if scenario == "showcase":
        return checks
    checks.append(("display = board answers", bool(ver) and all(not v["mismatches"] and not v["oracle_mismatches"] and v["displayed"] for v in ver),
                   "; ".join(f"{v['step']}: {v['matched']}/{v['displayed']} rows, oracle {v['oracle_checked']}" for v in ver)))
    final = result.get("final", {}).get("stats", {})
    link_errors = sum(final.get("rx_framing", {}).values()) + final.get("rx_rejects", 0) + final.get("board_seq_gaps", 0)
    checks.append(("no link error (framing, reject, seq gap)", link_errors == 0, f"{final.get('rx_framing')}, rejects {final.get('rx_rejects')}, gaps {final.get('board_seq_gaps')}"))
    if scenario == "perf":
        load = next((s.get("detail") or {} for s in steps if s["name"] == "charge"), {})
        sent, results, lost, nacks = (load.get(k, 0) for k in ("frames_sent", "results", "lost", "nacks"))
        checks.append((f"{rate} frames/s for {seconds} s", sent >= rate * seconds * 0.97, f"{sent} frames sent"))
        checks.append(("no loss, no NACK", results == sent and lost == 0 and nacks == 0, f"{results} RESULT, {lost} lost, {nacks} NACK"))
        checks.append((f"RTT p99 <= {BUDGET_RTT_P99_MS} ms", final.get("rtt_p99_us", 1e9) / 1000 <= BUDGET_RTT_P99_MS,
                       f"p50 {final.get('rtt_p50_us', 0) / 1000:.2f} ms, p99 {final.get('rtt_p99_us', 0) / 1000:.2f} ms, max {final.get('rtt_max_us', 0) / 1000:.2f} ms"))
        tree = result["harness"].get("process_tree_under_load") or {}
        checks.append((f"CPU <= {BUDGET_CPU_PERCENT} % of the machine", tree.get("cpu_percent_mean", 1e9) <= BUDGET_CPU_PERCENT,
                       f"mean {tree.get('cpu_percent_mean')} %, max {tree.get('cpu_percent_max')} %"))
        checks.append((f"memory <= {BUDGET_MEMORY_MB:.0f} MB", tree.get("private_mb_max", 1e9) <= BUDGET_MEMORY_MB,
                       f"private {tree.get('private_mb_max')} MB, working set {tree.get('working_set_mb_max')} MB, {tree.get('processes')}"))
        lat = (result.get("ui") or {}).get("stimulus_to_display_ms", {})
        checks.append((f"stimulus -> display p95 <= {BUDGET_DISPLAY_LATENCY_MS:.0f} ms", lat.get("count", 0) > 0 and lat.get("p95", 1e9) <= BUDGET_DISPLAY_LATENCY_MS,
                       f"p50 {lat.get('p50', 0):.0f} ms, p95 {lat.get('p95', 0):.0f} ms, max {lat.get('max', 0):.0f} ms over {lat.get('count')} changes"))
    return checks


# What a mission-apps report (ui/src/e2e.ts) carries, besides ok, error, steps and verifications:
#   step "trafic"   detail: frames_sent, results, lost, nacks, identifiers (distinct identifiers delivered over 3 s),
#                   tick_results (over the same 3 s), slots_from_ticks
#   mission         {state: done | cancelled, verdict: pass | fail | cancelled, reason, total_s, shown}
#   final           the core's last batch (link counters)
def _evaluate_mission(result: dict, expect_verdict: str) -> list[tuple[str, bool, str]]:
    harness = _dict(result.get("harness"))
    checks = [_steps_check(result)]
    if harness:
        checks.append(_exit_check(harness))
    checks += _bus_checks(result)
    checks.append(_reboot_check(result))
    mission = _dict(result.get("mission"))
    verdict = mission.get("verdict")
    detail = f"verdict {verdict or 'none'}"
    if verdict == "cancelled":
        detail += f" ({mission.get('reason')})"
    checks.append(_verdict_check("nominal check verdict", verdict, expect_verdict, detail))
    if harness:
        checks.append(_shots_check(harness))
    return checks


# What a student-apps report carries, besides the mission-apps fields of the bus (step "trafic", verifications, final):
#   mission         {state: recorded | cancelled, run_file (name), reason, total_s, shown}
#   run_file        full path of the run file (the harness judges it)
#   judge           added by the harness: {exit_code, verdict: pass | fail | None, error}
def _evaluate_student(result: dict, expect_verdict: str) -> list[tuple[str, bool, str]]:
    harness = _dict(result.get("harness"))
    checks = [_steps_check(result)]
    if harness:
        checks.append(_exit_check(harness))
    checks += _bus_checks(result)
    checks.append(_reboot_check(result))
    mission = _dict(result.get("mission"))
    run_file = result.get("run_file")
    recorded = mission.get("state") == "recorded" and isinstance(run_file, str) and bool(run_file)
    checks.append(("run file recorded, no verdict shown", recorded and "réussi" not in str(mission.get("shown", "")),
                   f"{mission.get('state') or 'no run'}: {mission.get('run_file') or mission.get('reason')}"))
    judge = _dict(result.get("judge"))
    verdict = judge.get("verdict")
    detail = f"n7rt-tool judge: {verdict or 'not judged'}"
    if judge.get("error"):
        detail += f" ({judge.get('error')})"
    checks.append(_verdict_check("run file verdict (n7rt-tool judge)", verdict, expect_verdict, detail))
    if harness:
        checks.append(_shots_check(harness))
    return checks


# What a progression-<id> report carries, besides the student fields of the bus (step "trafic", verifications, final):
#   progression     {stage, attempts: [{step, label, lines}], flashes: [{at, crc32}], reboot_checks,
#                    stopped_at: {step, attempts} (until-incident), resume_part_sha256 (until-incident and resume),
#                    closed: {mission, group, reproductions} (full and resume)}
#   shots           {<name>: {dpr, inner, rects}}: the elements of each capture to annotate
#   export          added by the harness from the final fiche (read_export)
def _evaluate_progression(result: dict) -> list[tuple[str, bool, str]]:
    harness = _dict(result.get("harness"))
    prog = _dict(result.get("progression"))
    stage = prog.get("stage") or "full"
    checks = [_steps_check(result)]
    if harness:
        checks.append(_exit_check(harness))
    checks += _bus_checks(result)
    checks.append(_reboot_check(result))
    attempts = [_dict(a) for a in _list(prog.get("attempts"))]
    lines = [str(line) for a in attempts for line in _list(a.get("lines"))]
    odd = [line for line in lines if not any(p.fullmatch(line) for p in FEEDBACK_LINES)]
    checks.append(("verdict lines are public texts only", bool(lines) and not odd,
                   f"{len(attempts)} attempts, {len(lines)} lines" + (f", unexpected: {odd[:3]}" if odd else "")))
    if stage == "until-incident":
        stopped = _dict(prog.get("stopped_at"))
        checks.append(("closed in incident 1 after its first attempt",
                       stopped.get("step") == 1 and stopped.get("attempts") == 1 and bool(prog.get("resume_part_sha256")),
                       f"stopped at {stopped or 'nothing'}"))
    else:
        closed = _dict(prog.get("closed"))
        checks.append(("mission closed (E5)", bool(closed.get("mission")), f"{closed.get('mission')} {closed.get('group')}"))
        checks += _export_checks(result, attempts)
    # the tutorial captures of this stage, each with every element it names on screen (tools/docs/tutorial_figures.py)
    shots = _dict(result.get("shots"))
    wanted = {"until-incident": PROGRESSION_SHOTS[:8], "resume": PROGRESSION_SHOTS[8:]}.get(stage, PROGRESSION_SHOTS)
    incomplete = [n for n in wanted if not _dict(shots.get(n)).get("rects") or not all(_dict(_dict(shots.get(n)).get("rects")).values())]
    checks.append(("tutorial captures, every element shown", not incomplete,
                   f"{len(wanted) - len(incomplete)}/{len(wanted)}" + (f", incomplete: {', '.join(incomplete)}" if incomplete else "")))
    if harness:
        checks.append(_shots_check(harness))
    return checks


def _crc(value) -> int | None:
    try:
        return int(str(value), 16)
    except ValueError:
        return None


def _played(attempt: dict) -> str:
    """The verdict an attempt showed: « Vérification réussie » for E0–E3, eight CP lines in PASS for E4."""
    lines = [str(line) for line in _list(attempt.get("lines"))]
    cps = [line for line in lines if line.startswith("CP-")]
    if cps:
        return "PASS" if len(cps) == 8 and all(line.endswith(" : PASS") for line in cps) else "FAIL"
    return "PASS" if lines and lines[0].startswith(PASSED_LINE) else "FAIL"


def _export_checks(result: dict, attempts: list[dict]) -> list[tuple[str, bool, str]]:
    """The exported final fiche against what the scenario played: steps, verdicts, CRC-32 of the flashed code,
    repairs, reproductions, active time, SHA-256."""
    ex = _dict(result.get("export"))
    if not ex or ex.get("error"):
        return [("final fiche exported", False, ex.get("error") or "no export found")]
    checks = [("final fiche exported (fiche.json, fiche.html, sealed copy)", ex.get("complete") is True, ex.get("folder_name")),
              ("SHA-256 printed = SHA-256 of fiche.json", ex.get("sha256_ok") is True, "footer of fiche.html")]
    etapes = _list(ex.get("etapes"))
    checks.append(("every step VALIDÉ in the fiche", len(etapes) >= 5 and all(e == "VALIDÉ" for e in etapes[:5]), str(etapes)))
    played = [_played(a) for a in attempts]
    recorded = [_dict(t) for t in _list(ex.get("tentatives"))]
    tail = recorded[-len(played):] if played else []
    checks.append(("attempts recorded = attempts played", [t.get("verdict") for t in tail] == played,
                   f"{len(recorded)} recorded, the last {len(played)} played here: {[t.get('verdict') for t in tail]} vs {played}"))
    # every CRC-32 recorded by this run's attempts is a binary this run flashed (IDE checks and progression flashes)
    flashed = {_crc(f.get("crc32")) for f in map(_dict, _list(_dict(result.get("progression")).get("flashes")))}
    flashed |= {_crc(c.get("crc")) for c in map(_dict, _list(result.get("ide"))) if c.get("crc")}
    flashed.discard(None)
    unknown = sorted({str(t.get("crc32")) for t in tail if _crc(t.get("crc32")) not in flashed})
    checks.append(("recorded CRC-32 = flashed binary", bool(flashed) and not unknown,
                   f"{len(flashed)} binaries flashed" + (f", unknown CRC {unknown}" if unknown else "")))
    repairs = [_dict(r) for r in _list(ex.get("reparations"))]
    validated = [r for r in repairs if r.get("crc32_validation")]
    checks.append(("repair recorded for the three incidents (CRC at activation and validation)",
                   len(validated) == 3 and all(r.get("crc32_activation") for r in validated),
                   "; ".join(f"E{r.get('rang')}: {r.get('crc32_activation')} -> {r.get('crc32_validation')}, "
                             f"{r.get('circuit_lines')} circuit change(s)" for r in repairs)))
    closed = _dict(_dict(result.get("progression")).get("closed"))
    checks.append(("reproductions recorded", ex.get("reproductions") == closed.get("reproductions"),
                   f"{ex.get('reproductions')} in the fiche, {closed.get('reproductions')} played"))
    m = re.fullmatch(r"(\d+) h (\d+) min", str(ex.get("temps_actif")))
    checks.append(("active time recorded", bool(m) and int(m[1]) * 60 + int(m[2]) > 0, str(ex.get("temps_actif"))))
    return checks


def _reboot_check(result: dict) -> tuple[str, bool, str]:
    """Step "sans-redemarrage" (ui/src/e2e.ts): no board boot from the session start to the end of the nominal check
    (the reboot of the IDE flash comes before the session)."""
    step = next((_dict(s) for s in _list(result.get("steps")) if _dict(s).get("name") == "sans-redemarrage"), None)
    if step is None:
        return "no runtime reboot during the mission", False, "step sans-redemarrage missing"
    detail = _dict(step.get("detail"))
    return ("no runtime reboot during the mission", step.get("ok") is True and detail.get("reboots") == 0,
            f"{detail.get('reboots', step.get('error'))} reboot(s), board uptime {detail.get('uptime_start_ms')} -> "
            f"{detail.get('uptime_end_ms')} ms")


def _verdict_check(label: str, verdict, expect_verdict: str, detail: str) -> tuple[str, bool, str]:
    if expect_verdict == "any":
        return f"{label} (reported)", verdict in ("pass", "fail"), detail
    return f"{label} = {expect_verdict}", verdict == expect_verdict, f"{detail}, expected {expect_verdict}"


def _bus_checks(result: dict) -> list[tuple[str, bool, str]]:
    """The mission bus over the 3 s of step "trafic", the display check and the link counters."""
    checks: list[tuple[str, bool, str]] = []
    traffic = _step_detail(result, "trafic")
    ids, lost, nacks = (_num(traffic.get(k)) for k in ("identifiers", "lost", "nacks"))
    ticks, sent, results = (_num(traffic.get(k)) for k in ("tick_results", "frames_sent", "results"))
    checks.append(("mission bus delivered, no loss, no NACK",
                   bool(sent) and bool(results) and lost == 0 and nacks == 0,
                   f"{sent} frames sent, {results} RESULT, {lost} lost, {nacks} NACK"))
    checks.append((f">= {MISSION_MIN_IDENTIFIERS} identifiers delivered", ids is not None and ids >= MISSION_MIN_IDENTIFIERS,
                   f"{ids} distinct identifiers over 3 s"))
    checks.append(("TICK_RESULT flowing", ticks is not None and ticks > 0,
                   f"{ticks} TICK_RESULT over 3 s, {traffic.get('slots_from_ticks')} slots written by ticks"))
    ver = _list(result.get("verifications"))
    checks.append(("display = frames sent", bool(ver) and all(_dict(v).get("displayed") and not _dict(v).get("mismatches")
                                                                for v in ver),
                   "; ".join(f"{_dict(v).get('step')}: {_dict(v).get('matched')}/{_dict(v).get('displayed')} rows"
                             for v in ver) or "no verification"))
    final = _dict(_dict(result.get("final")).get("stats"))
    link_errors = sum(_dict(final.get("rx_framing")).values()) + (_num(final.get("rx_rejects")) or 0) + \
        (_num(final.get("board_seq_gaps")) or 0)
    checks.append(("no link error (framing, reject, seq gap)", bool(final) and link_errors == 0,
                   f"{final.get('rx_framing')}, rejects {final.get('rx_rejects')}, gaps {final.get('board_seq_gaps')}"))
    return checks


# What a phys-perf report (ui/src/lab/e2e-phys.ts) carries, besides ok, error and steps[{name, ok, checks, detail}]:
#   step "chargement"   detail.load_ms: perf-150-300 loaded -> first frame drawn
#   step "repos"        detail.T_ms, detail.frames: T = median rAF gap over 2 s at rest, over `frames` gaps
#   each phase step     detail.frame_gap, render_ms, input_to_draw, edit_to_values (ms), engine_lag_frames (frames of
#                       1 ms): Summary {count, p50, p95, p99, max} as built by summarize() in ui/src/perf.ts;
#                       detail.gels (frames > 50 ms) and gels_source; detail.solver (EngineStats, null before any value)
#   budgets, budgets_ok the scenario's own judgement, folded into ok
#   ui                  the cockpit's UiPerf summary: not a Physical Lab measurement, never judged here
# The harness judges every budget again from the raw Summaries, phase by phase (the worst phase is reported).
# Memory and CPU come from the harness. A measurement missing from the report fails its line: no budget is ever
# skipped.
PHYS_PERF_PHASES = ("pan-vue-ajustee", "pan-zoom-1", "zoom-molette", "glisser-ecu", "glisser-selection-20",
                    "run-curseur-interrupteur")
PHYS_EDIT_PHASES = ("glisser-ecu", "glisser-selection-20")   # each release sends a move edit: edit_to_values
PHYS_RUN_PHASE = "run-curseur-interrupteur"                  # the engine in RUN: engine lag, solver
PHYS_LOAD_STEP = "chargement"
PHYS_IDLE_STEP = "repos"
FRAME_MS = 1.0                                               # one simulated frame (§1.2)


def _evaluate_phys(result: dict, scenario: str) -> list[tuple[str, bool, str]]:
    harness = _dict(result.get("harness"))
    checks = [_steps_check(result), _exit_check(harness), _shots_check(harness)]
    if scenario != "phys-perf":
        return checks
    phases = _phases(result)
    every, edits, run = list(phases), list(PHYS_EDIT_PHASES), [PHYS_RUN_PHASE]
    load = _num(_step_detail(result, PHYS_LOAD_STEP).get("load_ms"))
    checks += [_phases_check(phases),
               (f"load -> first frame <= {BUDGET_PHYS_LOAD_MS:.0f} ms",
                load is not None and load <= BUDGET_PHYS_LOAD_MS,
                f"{load:.0f} ms" if load is not None else f"absent from the report ({PHYS_LOAD_STEP}: load_ms)")]
    period = _idle_period(result)
    for stat, factor, slack in (("p95", BUDGET_FRAME_GAP_P95_T, 0.0),
                                ("p99", BUDGET_FRAME_GAP_P99_T, BUDGET_FRAME_GAP_P99_SLACK_MS)):
        bound = f"{factor:g} T" + (f" + {slack:g} ms" if slack else "")
        limit = factor * period + slack if period else None
        note = (f"limit {limit:.1f} ms = {bound}, T = {period:.1f} ms" if period
                else f"T not measured ({PHYS_IDLE_STEP}: T_ms over frames > 0)")
        checks.append(_phase_check(f"frame gap {stat} <= {bound}", phases, every, "frame_gap", stat, limit,
                                   note=note))
    checks += [
        _phase_check(f"frame gap max <= {BUDGET_FRAME_GAP_MAX_MS:.0f} ms", phases, every, "frame_gap", "max",
                     BUDGET_FRAME_GAP_MAX_MS),
        _phase_check(f"render p95 <= {BUDGET_RENDER_P95_MS:.0f} ms", phases, every, "render_ms", "p95",
                     BUDGET_RENDER_P95_MS),
        _phase_check(f"render p99 <= {BUDGET_RENDER_P99_MS:.0f} ms", phases, every, "render_ms", "p99",
                     BUDGET_RENDER_P99_MS),
        _long_frames_check(phases),
        _phase_check(f"input -> draw p95 <= {BUDGET_INPUT_TO_DRAW_P95_MS:.0f} ms", phases, every, "input_to_draw",
                     "p95", BUDGET_INPUT_TO_DRAW_P95_MS),
        _phase_check(f"edit -> values p95 <= {BUDGET_EDIT_TO_VALUES_P95_MS:.0f} ms", phases, edits,
                     "edit_to_values", "p95", BUDGET_EDIT_TO_VALUES_P95_MS),
        _phase_check(f"engine lag p99 <= {BUDGET_ENGINE_LAG_P99_MS:.0f} ms", phases, run, "engine_lag_frames", "p99",
                     BUDGET_ENGINE_LAG_P99_MS, scale=FRAME_MS),
        _memory_check(harness),
        _cpu_check(harness),
        _solver_check(phases),
    ]
    return checks


def _dict(value) -> dict:
    return value if isinstance(value, dict) else {}


def _list(value) -> list:
    return value if isinstance(value, list) else []


def _steps(result: dict) -> list[dict]:
    return [s for s in _list(result.get("steps")) if isinstance(s, dict)]


def _step_detail(result: dict, name: str) -> dict:
    return next((_dict(s.get("detail")) for s in _steps(result) if s.get("name") == name), {})


def _phases(result: dict) -> dict[str, dict]:
    """Measured phases, name -> detail, in report order: the steps of §8.6 that returned a detail, and any other
    step whose detail carries a frame_gap Summary."""
    out: dict[str, dict] = {}
    for s in _steps(result):
        name, detail = s.get("name"), s.get("detail")
        if isinstance(name, str) and isinstance(detail, dict) and (
                name in PHYS_PERF_PHASES or isinstance(detail.get("frame_gap"), dict)):
            out[name] = detail
    return out


def _num(value) -> float | None:
    """A finite number from a report, else None (booleans and strings are not numbers)."""
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        return None
    value = float(value)
    return value if math.isfinite(value) else None


def _idle_period(result: dict) -> float | None:
    """T: the median rAF gap over 2 s at rest, as measured by the scenario (None when no gap was measured: the
    scenario then falls back on 60 Hz, a value the harness does not judge with)."""
    idle = _step_detail(result, PHYS_IDLE_STEP)
    period = _num(idle.get("T_ms"))
    frames = _num(idle.get("frames"))
    return period if period is not None and period > 0 and frames else None


def _phases_check(phases: dict[str, dict]) -> tuple[str, bool, str]:
    missing = [n for n in PHYS_PERF_PHASES if n not in phases]
    detail = ", ".join(phases) or "none"
    return (f"{len(PHYS_PERF_PHASES)} phases measured", not missing,
            detail + (f"; missing: {', '.join(missing)}" if missing else ""))


def _phase_check(label: str, phases: dict[str, dict], wanted: list[str], key: str, stat: str,
                 limit: float | None, *, scale: float = 1.0, note: str = "") -> tuple[str, bool, str]:
    """One statistic of a Summary, in each wanted phase, against its limit. Every wanted phase must have measured it
    (present, count > 0); the worst phase is reported. scale converts the Summary to ms."""
    values: dict[str, float] = {}
    missing: list[str] = []
    for name in wanted:
        summary = _dict(phases.get(name, {}).get(key))
        count, value = _num(summary.get("count")), _num(summary.get(stat))
        if not count or value is None:
            missing.append(name)
        else:
            values[name] = value * scale
    if not values:
        where = f" in {', '.join(missing)}" if missing else ": no phase measured"
        return label, False, f"no measurement ({key}.{stat} absent or count 0{where})"
    worst = max(values, key=values.__getitem__)
    over = [n for n, v in values.items() if limit is not None and v > limit]
    detail = f"{stat} {values[worst]:.1f} ms in {worst}" + (f", {note}" if note else "") + f"; {len(values)} phase(s)"
    if over:
        detail += "; over the limit: " + ", ".join(f"{n} {values[n]:.1f} ms" for n in over)
    if missing:
        detail += f"; not measured: {', '.join(missing)}"
    return label, limit is not None and not over and not missing, detail


def _steps_check(result: dict) -> tuple[str, bool, str]:
    """The scenario's own verdict: ok, at least one step, every step ok. The detail names the failed checks and the
    scenario's own budgets over their limit."""
    raw = _list(result.get("steps"))
    steps = _steps(result)
    ok = (result.get("ok") is True and bool(steps) and len(steps) == len(raw)
          and all(s.get("ok") is True for s in steps))
    parts = []
    for s in steps:
        part = str(s.get("name", "?"))
        if s.get("ok") is not True:
            failed = [c for c in _list(s.get("checks")) if isinstance(c, dict) and c.get("ok") is not True]
            why = [f"{c.get('name', '?')}: expected {c.get('expected', '?')}, observed {c.get('observed', '?')}"
                   for c in failed[:3]]
            if s.get("error"):
                why.append(str(s["error"]))
            part += " FAIL" + (f" ({'; '.join(why)})" if why else "")
        parts.append(part)
    detail = ", ".join(parts) or "no step"
    over = [f"{name} {b.get('value')} > {b.get('limit')}" for name, b in _dict(result.get("budgets")).items()
            if isinstance(b, dict) and b.get("ok") is not True]
    if over:
        detail += f"; scenario budgets over the limit: {', '.join(over[:5])}" + (" ..." if len(over) > 5 else "")
    if result.get("error"):
        detail += f" — {result['error']}"
    return "scenario steps", ok, detail


def _events(harness: dict) -> list[dict]:
    events = harness.get("events")
    return [e for e in events if isinstance(e, dict)] if isinstance(events, list) else []


def _exit_check(harness: dict) -> tuple[str, bool, str]:
    code = harness.get("exit_code")
    timed_out = any(e.get("event") == "timeout" for e in _events(harness))
    detail = f"exit code {code}" + (", killed by the harness after its timeout" if timed_out else "")
    return "application exit code 0", _num(code) == 0 and not timed_out, detail


def _shots_check(harness: dict) -> tuple[str, bool, str]:
    events = _events(harness)
    wanted = [str(e["step"])[5:] for e in events if str(e.get("step", "")).startswith("shot:")]
    if not wanted:
        return "screenshots", True, "no shot: step in this scenario"
    captured = sum(1 for e in events if _CAPTURED.fullmatch(str(e.get("event", ""))))
    return "screenshots", captured >= len(wanted), f"{captured}/{len(wanted)} captured ({', '.join(wanted)})"


def _long_frames_check(phases: dict[str, dict]) -> tuple[str, bool, str]:
    """Freezes counted by the scenario in every phase (long-animation-frame entries, else longtask, else rAF gaps)."""
    label = f"no long frame > {LONG_FRAME_MS:.0f} ms"
    counts: dict[str, float] = {}
    missing: list[str] = []
    for name, detail in phases.items():
        count = _num(detail.get("gels"))
        if count is None or count < 0:
            missing.append(name)
        else:
            counts[name] = count
    if not counts:
        return label, False, "no measurement (gels absent" + (f" in {', '.join(missing)})" if missing else
                                                              ": no phase measured)")
    sources = sorted({str(d["gels_source"]) for d in phases.values() if d.get("gels_source")})
    detail = (f"{sum(counts.values()):.0f} long frame(s) over {len(counts)} phase(s), "
              f"source {', '.join(sources) or '?'}")
    frozen = [f"{n} {c:.0f}" for n, c in counts.items() if c]
    if frozen:
        detail += f": {', '.join(frozen)}"
    if missing:
        detail += f"; not measured: {', '.join(missing)}"
    return label, not frozen and not missing, detail


def _memory_check(harness: dict) -> tuple[str, bool, str]:
    label = f"memory vs cockpit alone <= +{BUDGET_PHYS_MEMORY_DELTA_MB:.0f} MB"
    reference = _dict(_dict(harness.get("memory_marks")).get("cockpit"))
    cockpit = _num(reference.get("private_mb"))
    tree = _dict(harness.get("process_tree_under_load"))
    peak = _num(tree.get("private_mb_max"))
    if cockpit is None:
        measured = f"; private {peak:.1f} MB max with the Physical Lab" if peak is not None else ""
        return label, False, f"no cockpit reference (the scenario did not play the mem:cockpit gate){measured}"
    if peak is None:
        return label, False, "no sample with the Physical Lab active"
    delta = peak - cockpit
    settled = "" if reference.get("settled") is True else " (reference not settled)"
    detail = (f"{delta:+.1f} MB: private {peak:.1f} MB max with the Physical Lab, {cockpit:.1f} MB cockpit alone"
              f"{settled}; working set {tree.get('working_set_mb_max')} MB")
    last = _num(tree.get("private_mb_last"))
    if last is not None:
        detail += f"; {last - cockpit:+.1f} MB at the end"
    # Settled marks of the scenario at rest: the document loaded (mem:lab), after the phases (mem:fin).
    for gate, what in (("lab", "document loaded, at rest"),
                       ("fin", f"after the phases and {MEM_IDLE_BEFORE_S.get('fin', 0.0):.0f} s idle")):
        mark = _dict(_dict(harness.get("memory_marks")).get(gate))
        mb = _num(mark.get("private_mb"))
        if mb is not None:
            detail += (f"; {mb - cockpit:+.1f} MB {what} (mem:{gate}"
                       + ("" if mark.get("settled") is True else ", not settled") + ")")
    # Where the peak is: growth of each kind of process over the reference (WebView2 GPU process, renderer...).
    roles = _dict(harness.get("process_roles"))
    at_peak = _dict(_dict(tree.get("peak")).get("per_process_private_mb"))
    at_ref = _dict(reference.get("per_process_private_mb"))
    if roles and at_peak:
        def by_role(per: dict) -> dict[str, float]:
            out: dict[str, float] = {}
            for key, mb in per.items():
                if _num(mb) is not None:
                    role = roles.get(key, str(key).split(":", 1)[0])
                    out[role] = out.get(role, 0.0) + float(mb)
            return out
        grown = by_role(at_peak)
        base = by_role(at_ref)
        parts = sorted(((r, mb - base.get(r, 0.0)) for r, mb in grown.items()), key=lambda x: -abs(x[1]))
        detail += (f"; growth at the peak (t {_dict(tree.get('peak')).get('t')} s) by process: "
                   + ", ".join(f"{r} {d:+.1f}" for r, d in parts if abs(d) >= 0.5))
    return label, delta <= BUDGET_PHYS_MEMORY_DELTA_MB, (
        detail + "; drift over 10 min of RUN not measured by this scenario")


def _cpu_check(harness: dict) -> tuple[str, bool, str]:
    label = f"CPU: phys thread <= {BUDGET_PHYS_THREAD_CPU_PERCENT:.0f} % of the machine (proposed)"
    tree = _dict(harness.get("process_tree_under_load"))
    if not tree:
        return label, False, "no sample of the process tree"
    processes = (f"process tree mean {tree.get('cpu_percent_mean')} %, max {tree.get('cpu_percent_max')} %, "
                 f"{tree.get('processes')}")
    mean = _num(tree.get("thread_cpu_percent_mean"))
    if mean is None:
        return label, False, f"thread '{PHYS_THREAD}' not found or not measured; {processes}"
    return (label, mean <= BUDGET_PHYS_THREAD_CPU_PERCENT,
            f"phys thread mean {mean:.2f} %, max {tree.get('thread_cpu_percent_max')} %; {processes}")


def _solver_check(phases: dict[str, dict]) -> tuple[str, bool, str]:
    """§8.6 solver metrics (sub-steps, µs per frame p95, islands) as seen at the end of the RUN phase, and no engine
    restart."""
    label = "solver reported (no engine restart)"
    solver = phases.get(PHYS_RUN_PHASE, {}).get("solver")
    if not isinstance(solver, dict) or not solver:
        return label, False, f"absent from the report ({PHYS_RUN_PHASE}: solver)"
    missing = [k for k in ("frame_us_p95", "substeps_last", "islands", "restarts") if _num(solver.get(k)) is None]
    restarts = _num(solver.get("restarts"))
    detail = ", ".join(f"{k} {v}" for k, v in sorted(solver.items()))
    if missing:
        detail += f"; missing {', '.join(missing)}"
    return label, not missing and restarts == 0, detail


if __name__ == "__main__":
    sys.exit("use: python tools/stm32/stm32lab.py app-test <board> | app-test --phys")
