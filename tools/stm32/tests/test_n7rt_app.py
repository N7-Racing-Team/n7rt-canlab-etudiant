"""N7RT CAN Lab application (app/): what the project suite can check without hardware.

- verdicts of `stm32lab app-test` (tools/stm32/n7rt_app.py) on synthetic reports: board scenarios (S1.4) and
  Physical Lab scenarios (docs/physical-lab-v1.md §8.6: a passing run, then one run per failing budget), on the
  report shape written by app/ui/src/lab/e2e-phys.ts (names checked against its source);
- the harness loop against a stand-in application: gates, no --serial without a board, timeout; the mission
  scenarios' environment (instructor directory, codes, group pack) and the judgement of the student's run file by a
  stand-in n7rt-tool;
- the CPU share of a named thread (the core's `phys` thread) and the memory reference of the mem: gate;
- `app-test` refuses to mix a board with the Physical Lab before touching anything;
- the UI safety check (app/scripts/check-ui.mjs) passes on the real UI and fails on a sink;
- `npm test` (vitest) in app/ when npm and app/node_modules are there;
- the Rust tests of n7rt-link, n7rt-lab, n7rt-circuit and n7rt-solver when N7RT_RUN_CARGO=1, in an
  environment where `cargo test` works (see app/README.md).
Real-board validation is `python tools/stm32/stm32lab.py app-test <board>`; the Physical Lab in the real
application is `python tools/stm32/stm32lab.py app-test --phys`.
"""
from __future__ import annotations

import json
import os
import re
import shutil
import subprocess
import sys
import threading
import time
import types
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[3]
APP = ROOT / "app"
STM32LAB = ROOT / "tools" / "stm32" / "stm32lab.py"
sys.path.insert(0, str(ROOT / "tools" / "stm32"))
import n7rt_app  # noqa: E402

NODE = shutil.which("node")
NPM = shutil.which("npm")
NAMED_THREADS = os.name == "nt" or sys.platform.startswith("linux")


def mock_capture_window(monkeypatch, size=(1024, 640), *, iconic=False, dwm_ok=True, rect_ok=True):
    """Only Win32 query mocks: no window restore, screen access, or application launch."""
    import ctypes
    from unittest.mock import MagicMock
    user32, dwmapi = MagicMock(), MagicMock()
    user32.FindWindowW.return_value = 71
    user32.IsIconic.return_value = iconic

    def set_rect(pointer):
        rect = pointer._obj
        rect.left, rect.top = 10, 20
        rect.right, rect.bottom = 10 + size[0], 20 + size[1]

    def window_rect(_hwnd, pointer):
        set_rect(pointer)
        return rect_ok

    def visible_rect(_hwnd, _attribute, pointer, _length):
        set_rect(pointer)
        return 0 if dwm_ok else 1

    user32.GetWindowRect.side_effect = window_rect
    dwmapi.DwmGetWindowAttribute.side_effect = visible_rect
    monkeypatch.setattr(n7rt_app, "os", types.SimpleNamespace(name="nt"))
    monkeypatch.setattr(ctypes, "windll", types.SimpleNamespace(user32=user32, dwmapi=dwmapi), raising=False)
    return user32


@pytest.mark.parametrize("size, iconic", [
    ((1024, 640), True), ((280, 34), False), ((639, 400), False), ((640, 399), False), ((0, 0), False),
])
def test_capture_window_rejects_minimized_or_small_window_without_capturing(tmp_path, monkeypatch, size, iconic):
    image_grab = pytest.importorskip("PIL.ImageGrab")
    user32 = mock_capture_window(monkeypatch, size, iconic=iconic)
    forbidden = lambda *a, **k: pytest.fail("Rejected window must not be rendered or grabbed")
    monkeypatch.setattr(n7rt_app, "_render_window", forbidden)
    monkeypatch.setattr(image_grab, "grab", forbidden)
    output = tmp_path / "invalid.png"
    assert n7rt_app.capture_window("N7RT CAN Lab", output) is False
    assert not output.exists()
    user32.ShowWindow.assert_not_called()
    user32.SetForegroundWindow.assert_not_called()


def test_capture_window_rejects_failed_geometry_lookup(tmp_path, monkeypatch):
    pytest.importorskip("PIL.ImageGrab")
    mock_capture_window(monkeypatch, dwm_ok=False, rect_ok=False)
    monkeypatch.setattr(n7rt_app, "_render_window", lambda *a: pytest.fail("No valid geometry"))
    assert n7rt_app.capture_window("N7RT CAN Lab", tmp_path / "invalid.png") is False


def test_capture_window_rejects_minimization_during_render(tmp_path, monkeypatch):
    image_grab = pytest.importorskip("PIL.ImageGrab")
    user32 = mock_capture_window(monkeypatch)
    user32.IsIconic.side_effect = [False, True]
    monkeypatch.setattr(n7rt_app, "_render_window", lambda *a: None)
    monkeypatch.setattr(image_grab, "grab", lambda *a, **k: pytest.fail("Minimized fallback must not grab"))
    assert n7rt_app.capture_window("N7RT CAN Lab", tmp_path / "invalid.png") is False


@pytest.mark.parametrize("rendered", [True, False])
def test_capture_window_accepts_normal_geometry_for_render_and_fallback(tmp_path, monkeypatch, rendered):
    from ctypes import wintypes
    image = pytest.importorskip("PIL.Image")
    image_grab = pytest.importorskip("PIL.ImageGrab")
    user32 = mock_capture_window(monkeypatch, (640, 400))
    pixels = image.new("RGB", (640, 400), "white")
    rect = wintypes.RECT(10, 20, 650, 420)
    monkeypatch.setattr(n7rt_app, "_render_window", lambda *a: (pixels, rect) if rendered else None)
    grabs = []
    monkeypatch.setattr(image_grab, "grab", lambda **k: grabs.append(k) or pixels)
    output = tmp_path / "valid.png"
    assert n7rt_app.capture_window("N7RT CAN Lab", output) is True
    with image.open(output) as captured:
        assert captured.size == (640, 400)
    assert len(grabs) == (0 if rendered else 1)
    user32.ShowWindow.assert_not_called()
    user32.SetForegroundWindow.assert_not_called()


@pytest.mark.parametrize("size, iconic", [((1024, 640), True), ((280, 34), False), ((640, 399), False)])
def test_render_window_rejects_invalid_geometry_before_allocating_gdi(monkeypatch, size, iconic):
    import ctypes
    from unittest.mock import MagicMock
    pytest.importorskip("PIL.Image")
    user32 = mock_capture_window(monkeypatch, size, iconic=iconic)
    gdi32 = MagicMock()
    monkeypatch.setattr(ctypes, "WinDLL", lambda name: user32 if name == "user32" else gdi32, raising=False)
    assert n7rt_app._render_window(71) is None
    user32.GetWindowDC.assert_not_called()
    gdi32.CreateCompatibleBitmap.assert_not_called()


def perf_result(**over) -> dict:
    load = {"frames_sent": 72000, "results": 72000, "lost": 0, "nacks": 0}
    load.update(over.pop("load", {}))
    result = {
        "ok": True,
        "steps": [{"name": "connexion", "ok": True}, {"name": "charge", "ok": True, "detail": load}],
        "verifications": [{"step": "verification", "displayed": 200, "matched": 200, "oracle_checked": 72000,
                           "mismatches": [], "oracle_mismatches": []}],
        "final": {"stats": {"rx_framing": {}, "rx_rejects": 0, "board_seq_gaps": 0,
                            "rtt_p50_us": 3100, "rtt_p99_us": 6500, "rtt_max_us": 9000}},
        "ui": {"stimulus_to_display_ms": {"count": 200, "p50": 40, "p95": 70, "max": 90}},
        "harness": {"process_tree_under_load": {"cpu_percent_mean": 6.0, "cpu_percent_max": 11.0,
                                                "private_mb_max": 280.0, "working_set_mb_max": 500.0,
                                                "processes": {}}},
    }
    result.update(over)
    return result


def verdict(result: dict) -> dict[str, bool]:
    return {name: ok for name, ok, _ in n7rt_app.evaluate(result, "perf", 120, 600)}


def test_perf_verdict_passes_on_a_clean_run():
    assert all(verdict(perf_result()).values())


@pytest.mark.parametrize("change, failing", [
    ({"load": {"lost": 1, "results": 71999}}, "no loss, no NACK"),
    ({"load": {"frames_sent": 1000, "results": 1000}}, "600 frames/s for 120 s"),
    ({"final": {"stats": {"rx_framing": {"E_CRC": 1}, "rx_rejects": 0, "board_seq_gaps": 0,
                          "rtt_p99_us": 6500}}}, "no link error (framing, reject, seq gap)"),
])
def test_perf_verdict_fails_on_each_defect(change, failing):
    v = verdict(perf_result(**change))
    assert v[failing] is False
    assert sum(not ok for ok in v.values()) >= 1


def test_display_mismatch_fails():
    bad = perf_result(verifications=[{"step": "verification", "displayed": 200, "matched": 199, "oracle_checked": 10,
                                      "mismatches": ["trame 7"], "oracle_mismatches": []}])
    assert verdict(bad)["display = board answers"] is False


def test_memory_budget():
    heavy = perf_result()
    heavy["harness"]["process_tree_under_load"]["private_mb_max"] = 546.0
    assert verdict(heavy)[f"memory <= {n7rt_app.BUDGET_MEMORY_MB:.0f} MB"] is False


# ------------------------------------------------------------ mission-apps (docs/apps-mission-slice.md §7)

def mission_result(verdict_: str = "fail", **over) -> dict:
    traffic = {"frames_sent": 2400, "results": 2390, "lost": 0, "nacks": 0, "identifiers": 18, "tick_results": 300,
               "slots_from_ticks": 0}
    traffic.update(over.pop("traffic", {}))
    shots = [{"t": 1.0 + i, "step": f"shot:{n}"} for i, n in enumerate(n7rt_app.MISSION_SHOTS)]
    captured = [{"t": 1.5 + i, "event": f"screenshot app_mission-apps_{n}.png"} for i, n in enumerate(n7rt_app.MISSION_SHOTS)]
    result = {
        "ok": True,
        "steps": [{"name": "connexion", "ok": True}, {"name": "trafic", "ok": True, "detail": traffic},
                  {"name": "verdict", "ok": True},
                  {"name": "sans-redemarrage", "ok": True,
                   "detail": {"reboots": 0, "uptime_start_ms": 10_000, "uptime_end_ms": 22_500}}],
        "verifications": [{"step": "verification-affichage", "displayed": 200, "matched": 200, "oracle_checked": 0,
                           "mismatches": [], "oracle_mismatches": []}],
        "mission": {"state": "cancelled" if verdict_ == "cancelled" else "done", "verdict": verdict_,
                    "reason": "carte déconnectée" if verdict_ == "cancelled" else None, "total_s": 9.0},
        "final": {"stats": {"rx_framing": {}, "rx_rejects": 0, "board_seq_gaps": 0}},
        "harness": {"exit_code": 0, "events": shots + captured},
    }
    result.update(over)
    return result


def mission_verdict(result: dict, expect: str = "any") -> dict[str, bool]:
    return {name: ok for name, ok, _ in n7rt_app.evaluate(result, "mission-apps", expect_verdict=expect)}


@pytest.mark.parametrize("verdict_, expect", [("fail", "any"), ("pass", "any"), ("pass", "pass"), ("fail", "fail")])
def test_mission_verdict_passes_when_reported_as_expected(verdict_, expect):
    v = mission_verdict(mission_result(verdict_), expect)
    assert all(v.values()), v
    label = "nominal check verdict (reported)" if expect == "any" else f"nominal check verdict = {expect}"
    assert label in v and "screenshots" in v and ">= 17 identifiers delivered" in v
    assert "no runtime reboot during the mission" in v


REBOOTED = {"name": "sans-redemarrage", "ok": False, "error": "Failure: la carte a redémarré 1 fois pendant la mission",
            "detail": None}


@pytest.mark.parametrize("steps", [
    [{"name": "verdict", "ok": True}, REBOOTED],
    [{"name": "verdict", "ok": True}],
    [{"name": "sans-redemarrage", "ok": True, "detail": {"reboots": 1}}],
])
def test_mission_verdict_fails_on_a_board_reboot_or_an_unchecked_one(steps):
    traffic = {"name": "trafic", "ok": True, "detail": mission_result()["steps"][1]["detail"]}
    v = mission_verdict(mission_result("pass", steps=[traffic, *steps]), "pass")
    assert v["no runtime reboot during the mission"] is False


@pytest.mark.parametrize("mission", n7rt_app.MISSIONS)
def test_the_four_missions_share_one_evaluation(mission):
    passed = {name: ok for name, ok, _ in n7rt_app.evaluate(mission_result("pass"), f"mission-{mission}", expect_verdict="pass")}
    assert all(passed.values()) and "nominal check verdict = pass" in passed, passed
    failed = {name: ok for name, ok, _ in n7rt_app.evaluate(mission_result("pass"), f"mission-{mission}", expect_verdict="fail")}
    assert [k for k, ok in failed.items() if not ok] == ["nominal check verdict = fail"]
    ide = n7rt_app.evaluate({**mission_result("fail"), "ide": [{"ok": True}]}, f"ide-mission-{mission}", expect_verdict="fail")
    assert ide[0][0] == "IDE edit/build/flash/reconnect" and all(ok for _, ok, _ in ide), ide


def test_mission_scenario_names_and_flows():
    assert len(n7rt_app.MISSION_SCENARIOS) == 24 == len(set(n7rt_app.MISSION_SCENARIOS))
    for mission in n7rt_app.MISSIONS:
        for scenario in (f"progression-{mission}", f"ide-progression-{mission}"):
            assert n7rt_app.is_progression(scenario) and not n7rt_app.is_student(scenario)
            assert n7rt_app.mission_of(scenario) == mission
    assert not n7rt_app.is_progression("student-apps") and not n7rt_app.is_progression("progression-xyz")
    assert n7rt_app.MISSION_ENV["ide-progression-sas"] == ("N7RT_INSTRUCTOR_DIR", "N7RT_E2E_INSTRUCTOR_CODE", "N7RT_E2E_PACK",
                                                           "N7RT_E2E_GROUP_CODE", "N7RT_E2E_ANSWERS", "N7RT_E2E_DECODER_C")
    for mission in n7rt_app.MISSIONS:
        for scenario in (f"mission-{mission}", f"ide-mission-{mission}"):
            assert scenario in n7rt_app.MISSION_SCENARIOS and not n7rt_app.is_student(scenario)
            assert n7rt_app.mission_of(scenario) == mission
        for scenario in (f"student-{mission}", f"ide-student-{mission}"):
            assert scenario in n7rt_app.MISSION_SCENARIOS and n7rt_app.is_student(scenario)
            assert n7rt_app.mission_of(scenario) == mission
    assert not n7rt_app.is_student("student-xyz") and not n7rt_app.is_student("functional")
    assert n7rt_app.MISSION_ENV["ide-student-wsf"] == ("N7RT_INSTRUCTOR_DIR", "N7RT_E2E_INSTRUCTOR_CODE", "N7RT_E2E_PACK",
                                                       "N7RT_E2E_GROUP_CODE", "N7RT_E2E_DECODER_C")
    assert n7rt_app.MISSION_ENV["mission-bps"] == ("N7RT_INSTRUCTOR_DIR", "N7RT_E2E_INSTRUCTOR_CODE")
    assert n7rt_app.MISSION_ENV["ide-mission-sas"] == ("N7RT_INSTRUCTOR_DIR", "N7RT_E2E_INSTRUCTOR_CODE", "N7RT_E2E_DECODER_C")


@pytest.mark.parametrize("verdict_, expect", [("fail", "pass"), ("pass", "fail"), ("cancelled", "any"),
                                              ("cancelled", "fail")])
def test_mission_verdict_fails_on_another_verdict_or_a_cancelled_check(verdict_, expect):
    v = mission_verdict(mission_result(verdict_), expect)
    failed = [k for k, ok in v.items() if not ok]
    assert failed == [k for k in v if k.startswith("nominal check verdict")], v


@pytest.mark.parametrize("change, failing", [
    ({"traffic": {"identifiers": 16}}, ">= 17 identifiers delivered"),
    ({"traffic": {"lost": 2}}, "mission bus delivered, no loss, no NACK"),
    ({"traffic": {"nacks": 1}}, "mission bus delivered, no loss, no NACK"),
    ({"traffic": {"tick_results": 0}}, "TICK_RESULT flowing"),
    ({"verifications": [{"step": "verification-affichage", "displayed": 200, "matched": 199, "mismatches": ["x"]}]},
     "display = frames sent"),
    ({"final": {"stats": {"rx_framing": {}, "rx_rejects": 0, "board_seq_gaps": 3}}},
     "no link error (framing, reject, seq gap)"),
    ({"ok": False}, "scenario steps"),
])
def test_mission_verdict_fails_on_each_defect(change, failing):
    assert mission_verdict(mission_result(**change))[failing] is False


def test_mission_malformed_report_fails_without_crashing():
    v = mission_verdict({"ok": False, "error": "no report"})
    assert not any(v.values())


# ------------------------------------------------------------ student-apps (docs/distribution.md §2.1)

RUN_NAME = "essai-apps-G01-20261006-142233.n7rtrun"


def student_result(judged: str | None = "fail", **over) -> dict:
    result = mission_result("fail")
    del result["mission"]
    shots = [{"t": 1.0 + i, "step": f"shot:{n}"} for i, n in enumerate(n7rt_app.STUDENT_SHOTS)]
    captured = [{"t": 1.5 + i, "event": f"screenshot app_student-apps_{n}.png"} for i, n in enumerate(n7rt_app.STUDENT_SHOTS)]
    result.update({
        "mission": {"state": "recorded", "run_file": RUN_NAME, "reason": None, "total_s": 9.0,
                    "shown": f"Essai enregistré : {RUN_NAME}. Transmettez ce fichier à votre encadrant."},
        "run_file": f"C:/Users/x/Documents/N7RT CAN Lab/essais/{RUN_NAME}",
        "judge": {"exit_code": {"pass": 0, "fail": 1}.get(judged, 2), "verdict": judged},
        "harness": {"exit_code": 0, "events": shots + captured},
    })
    result.update(over)
    return result


def student_verdict(result: dict, expect: str = "any") -> dict[str, bool]:
    return {name: ok for name, ok, _ in n7rt_app.evaluate(result, "student-apps", expect_verdict=expect)}


@pytest.mark.parametrize("judged, expect", [("fail", "any"), ("pass", "any"), ("pass", "pass"), ("fail", "fail")])
def test_student_verdict_is_the_tools_on_the_run_file(judged, expect):
    v = student_verdict(student_result(judged), expect)
    assert all(v.values()), v
    label = "run file verdict (n7rt-tool judge) " + ("(reported)" if expect == "any" else f"= {expect}")
    assert label in v and "run file recorded, no verdict shown" in v and ">= 17 identifiers delivered" in v
    assert "screenshots" in v


@pytest.mark.parametrize("change, failing", [
    ({"judge": {"exit_code": 2, "verdict": None, "error": "pack inconnu"}}, "run file verdict (n7rt-tool judge) (reported)"),
    ({"judge": {}}, "run file verdict (n7rt-tool judge) (reported)"),
    ({"run_file": None}, "run file recorded, no verdict shown"),
    ({"mission": {"state": "cancelled", "reason": "carte déconnectée", "shown": "Vérification interrompue"}},
     "run file recorded, no verdict shown"),
    ({"mission": {"state": "recorded", "run_file": RUN_NAME, "shown": "Essai nominal : réussi"}},
     "run file recorded, no verdict shown"),
    ({"traffic": {"identifiers": 3}}, ">= 17 identifiers delivered"),
])
def test_student_verdict_fails_on_each_defect(change, failing):
    if "traffic" in change:
        result = student_result()
        result["steps"][1]["detail"]["identifiers"] = change["traffic"]["identifiers"]
    else:
        result = student_result(**change)
    v = student_verdict(result)
    assert [k for k, ok in v.items() if not ok] == [failing], v


@pytest.mark.parametrize("mission", n7rt_app.MISSIONS)
def test_the_four_student_flows_share_one_evaluation(mission):
    for scenario in (f"student-{mission}", f"ide-student-{mission}"):
        result = {**student_result("pass"), "ide": [{"ok": True}]}
        v = {name: ok for name, ok, _ in n7rt_app.evaluate(result, scenario, expect_verdict="pass")}
        assert all(v.values()) and "run file verdict (n7rt-tool judge) = pass" in v, (scenario, v)
        assert "no runtime reboot during the mission" in v


def test_student_verdict_against_the_expected_one():
    assert student_verdict(student_result("fail"), "pass")["run file verdict (n7rt-tool judge) = pass"] is False
    assert not any(student_verdict({"ok": False, "error": "no report"}).values())


def test_mission_scenarios_name_their_missing_environment():
    full = {k: "x" for k in n7rt_app.MISSION_ENV["student-apps"]}
    assert n7rt_app.mission_env_missing("student-apps", full) == []
    assert n7rt_app.mission_env_missing("student-apps", {**full, "N7RT_E2E_PACK": ""}) == ["N7RT_E2E_PACK"]
    assert n7rt_app.mission_env_missing("mission-apps", {}) == ["N7RT_INSTRUCTOR_DIR", "N7RT_E2E_INSTRUCTOR_CODE"]
    assert n7rt_app.mission_env_missing("functional", {}) == []


FAKE_TOOL = '''\
"""Stand-in for n7rt-tool: `judge <dir> <run file> --code-stdin`, exit code from N7RT_FAKE_JUDGE_EXIT."""
import json, os, sys
from pathlib import Path

code = sys.stdin.readline().rstrip("\\n")
Path(os.environ["N7RT_FAKE_JUDGE_LOG"]).write_text(json.dumps({"argv": sys.argv[1:], "code_ok": code == os.environ.get(
    "N7RT_E2E_INSTRUCTOR_CODE")}), encoding="utf-8")
print("detail prive du jugement")
print("ligne d'erreur sans secret", file=sys.stderr)
sys.exit(int(os.environ.get("N7RT_FAKE_JUDGE_EXIT", "0")))
'''


@pytest.fixture
def fake_tool(tmp_path, monkeypatch):
    """A stand-in n7rt-tool next to the stand-in application (run through the Python interpreter)."""
    tool = tmp_path / n7rt_app.TOOL_NAME
    tool.write_text(FAKE_TOOL, encoding="utf-8")
    log = tmp_path / "judge.json"
    monkeypatch.setenv("N7RT_FAKE_JUDGE_LOG", str(log))
    monkeypatch.setattr(n7rt_app, "subprocess", types.SimpleNamespace(
        Popen=lambda args, **kw: subprocess.Popen([sys.executable, *args], **kw),
        run=lambda args, **kw: subprocess.run([sys.executable, *args], **kw),
        DEVNULL=subprocess.DEVNULL, TimeoutExpired=subprocess.TimeoutExpired))
    return tool, log


@pytest.mark.parametrize("exit_code, verdict", [(0, "pass"), (1, "fail"), (2, None)])
def test_judge_maps_the_tool_exit_code_and_keeps_no_output(tmp_path, fake_tool, monkeypatch, exit_code, verdict):
    tool, log = fake_tool
    monkeypatch.setenv("N7RT_FAKE_JUDGE_EXIT", str(exit_code))
    monkeypatch.setenv("N7RT_E2E_INSTRUCTOR_CODE", "CODE-1234")
    out = n7rt_app.judge_run_file(tool, str(tmp_path / "instr"), str(tmp_path / "run.n7rtrun"), "CODE-1234")
    assert out["exit_code"] == exit_code and out["verdict"] == verdict
    assert "detail prive" not in json.dumps(out)
    if verdict is None:
        assert out["error"] == "ligne d'erreur sans secret (exit code 2)"
    called = json.loads(log.read_text(encoding="utf-8"))
    assert called == {"argv": ["judge", str(tmp_path / "instr"), str(tmp_path / "run.n7rtrun"), "--code-stdin"],
                      "code_ok": True}
    missing = n7rt_app.judge_run_file(tmp_path / "absent-tool", "d", "r", "c")
    assert missing["verdict"] is None and "cargo build -p n7rt-tool" in missing["error"]


# ------------------------------------------------------------ Physical Lab verdicts (docs/physical-lab-v1.md §8.6)

STEPS, EXIT, SHOTS = "scenario steps", "application exit code 0", "screenshots"
PHASES = "6 phases measured"
LOAD = "load -> first frame <= 200 ms"
GAP_P95, GAP_P99, GAP_MAX = "frame gap p95 <= 1.1 T", "frame gap p99 <= 2 T + 1 ms", "frame gap max <= 50 ms"
RENDER_P95, RENDER_P99 = "render p95 <= 8 ms", "render p99 <= 12 ms"
LONG = "no long frame > 50 ms"
INPUT, EDIT, LAG = "input -> draw p95 <= 50 ms", "edit -> values p95 <= 100 ms", "engine lag p99 <= 20 ms"
MEMORY = "memory vs cockpit alone <= +40 MB"
CPU = "CPU: phys thread <= 5 % of the machine (proposed)"
SOLVER = "solver reported (no engine restart)"
PHYS_PERF_CHECKS = [STEPS, EXIT, SHOTS, PHASES, LOAD, GAP_P95, GAP_P99, GAP_MAX, RENDER_P95, RENDER_P99, LONG, INPUT,
                    EDIT, LAG, MEMORY, CPU, SOLVER]
REMOVE = object()

# The phases of §8.6 as ui/src/lab/e2e-phys.ts names them; the two drags send edits, the last one runs the engine.
PAN_FIT, PAN_ZOOM1, WHEEL, DRAG_ECU, DRAG_20, RUN = (
    "pan-vue-ajustee", "pan-zoom-1", "zoom-molette", "glisser-ecu", "glisser-selection-20", "run-curseur-interrupteur")
PERF_PHASES = [PAN_FIT, PAN_ZOOM1, WHEEL, DRAG_ECU, DRAG_20, RUN]
E2E_PHYS = APP / "ui" / "src" / "lab" / "e2e-phys.ts"


def summary(count: int, p50: float, p95: float, p99: float, mx: float) -> dict:
    return {"count": count, "p50": p50, "p95": p95, "p99": p99, "max": mx}


NONE = summary(0, 0, 0, 0, 0)   # what summarize([]) gives: nothing measured


def phase(name: str) -> dict:
    """The detail of one 10 s phase within every budget, on a 60 Hz display (T = 16.7 ms, so 1.1 T = 18.37 ms)."""
    return {
        "frame_gap": summary(600, 16.7, 17.9, 25.0, 33.4),
        "render_ms": summary(600, 2.1, 5.5, 9.0, 14.0),
        "input_to_draw": summary(600, 18.0, 30.0, 40.0, 45.0),
        "edit_to_values": summary(10, 35.0, 70.0, 85.0, 90.0) if name in (DRAG_ECU, DRAG_20) else NONE,
        "gels": 0, "gels_source": "long-animation-frame",
        "engine_lag_frames": summary(9000, 0.0, 1.0, 3.0, 6.0) if name == RUN else NONE,
        "solver": {"substeps_last": 12, "frame_us_p95": 180.0, "islands": 10, "min_dt_ns": 10000, "slow": False,
                   "restarts": 0},
        "editor": {"full": 1, "partial": 590, "overlay": 12},
        "heap_mb": {"start": 40.1, "end": 41.0},
    }


def phys_perf_result() -> dict:
    """A phys-perf report as ui/src/lab/e2e-phys.ts writes it, within every §8.6 budget."""
    steps = [
        {"name": "vue", "ok": True, "ms": 310, "checks": [], "detail": {"ready_ms": 250}},
        {"name": "chargement", "ok": True, "ms": 420,
         "checks": [{"name": "gabarit", "ok": True, "expected": "≥ 150 composants", "observed": "150, 60, 300"}],
         "detail": {"load_ms": 120, "components": 150, "junctions": 60, "wires": 300}},
        {"name": "repos", "ok": True, "ms": 2300, "checks": [], "detail": {"T_ms": 16.7, "frames": 120}},
    ]
    steps += [{"name": n, "ok": True, "ms": 10300, "checks": [], "detail": phase(n)} for n in PERF_PHASES]
    return {
        "ok": True,
        "scenario": "phys-perf",
        "steps": steps,
        "budgets": {"chargement_ms": {"value": 120, "limit": 200, "ok": True}},
        "budgets_ok": True,
        "T_ms": 16.7,
        # The cockpit's UiPerf summary over the whole run, load included: far over every budget, never judged.
        "ui": {"hidden_events": 0, "frame_gap_ms": summary(4000, 16.7, 60.0, 120.0, 400.0),
               "render_ms": summary(4000, 1.0, 30.0, 60.0, 90.0), "stimulus_to_display_ms": NONE},
        "harness": {
            "exit_code": 0,
            "events": [{"t": 1.0, "step": "mem:cockpit"},
                       {"t": 4.0, "event": "memory reference cockpit: private 260.0 MB"},
                       {"t": 9.0, "step": "shot:perf"},
                       {"t": 9.2, "event": "screenshot app_phys-perf_20261005-120000_perf.png"}],
            "memory_marks": {"cockpit": {"t": 3.5, "private_mb": 260.0, "settled": True}},
            "process_tree_under_load": {"cpu_percent_mean": 7.5, "cpu_percent_max": 14.0, "private_mb_max": 290.0,
                                        "working_set_mb_max": 520.0,
                                        "processes": {"n7rt-can-lab.exe": 1, "msedgewebview2.exe": 6},
                                        "thread": "phys", "thread_cpu_percent_mean": 1.2,
                                        "thread_cpu_percent_max": 3.0},
        },
    }


def edited(path: str, value) -> dict:
    """The passing phys-perf run with one field replaced, or removed when value is REMOVE. Dotted path; "@<name>"
    is the step of that name ("@glisser-ecu.detail.gels"; "@zoom-molette" alone removes the step)."""
    result = phys_perf_result()
    *parents, leaf = path.split(".")

    def step(name: str) -> dict:
        return next(s for s in result["steps"] if s["name"] == name)

    node = result
    for key in parents:
        node = step(key[1:]) if key.startswith("@") else node[key]
    if leaf.startswith("@"):
        assert value is REMOVE
        result["steps"].remove(step(leaf[1:]))
    elif value is REMOVE:
        del node[leaf]
    else:
        node[leaf] = value
    return result


def phys_verdict(result: dict, scenario: str = "phys-perf") -> dict[str, bool]:
    return {name: ok for name, ok, _ in n7rt_app.evaluate(result, scenario)}


def failing(v: dict[str, bool]) -> list[str]:
    return [name for name, ok in v.items() if not ok]


def test_phys_perf_passes_with_one_line_per_budget():
    checks = n7rt_app.evaluate(phys_perf_result(), "phys-perf")
    assert [name for name, _, _ in checks] == PHYS_PERF_CHECKS
    assert all(ok for _, ok, _ in checks), [c for c in checks if not c[1]]
    assert all(detail for _, _, detail in checks)


@pytest.mark.parametrize("path, value, budget", [
    ("@chargement.detail.load_ms", 250.0, LOAD),
    (f"@{PAN_ZOOM1}.detail.frame_gap", summary(600, 16.7, 18.5, 25.0, 33.4), GAP_P95),   # 18.5 > 1.1 x 16.7
    (f"@{WHEEL}.detail.frame_gap", summary(600, 16.7, 17.9, 34.5, 40.0), GAP_P99),       # 34.5 > 2 x 16.7 + 1
    (f"@{DRAG_ECU}.detail.frame_gap", summary(600, 16.7, 17.9, 25.0, 51.0), GAP_MAX),
    (f"@{PAN_FIT}.detail.render_ms", summary(600, 2.1, 8.5, 9.0, 14.0), RENDER_P95),
    (f"@{RUN}.detail.render_ms", summary(600, 2.1, 5.5, 12.5, 14.0), RENDER_P99),
    (f"@{DRAG_20}.detail.gels", 1, LONG),
    (f"@{WHEEL}.detail.input_to_draw", summary(600, 18.0, 55.0, 60.0, 70.0), INPUT),
    (f"@{PAN_ZOOM1}.detail.input_to_draw", NONE, INPUT),               # no input ever drawn
    (f"@{DRAG_20}.detail.edit_to_values", summary(10, 35.0, 120.0, 130.0, 140.0), EDIT),
    (f"@{DRAG_ECU}.detail.edit_to_values", NONE, EDIT),               # no edit ever reached the values
    (f"@{RUN}.detail.engine_lag_frames", summary(9000, 0.0, 1.0, 21.0, 25.0), LAG),   # 21 frames of 1 ms
    (f"@{RUN}.detail.engine_lag_frames", NONE, LAG),
    ("harness.process_tree_under_load.private_mb_max", 300.5, MEMORY),    # +40.5 MB over the cockpit
    ("harness.process_tree_under_load.thread_cpu_percent_mean", 5.5, CPU),
    (f"@{RUN}.detail.solver.restarts", 1, SOLVER),
    (f"@{RUN}.detail.solver", None, SOLVER),                          # no value seen in RUN
    ("ok", False, STEPS),
    (f"@{RUN}.ok", False, STEPS),
    ("harness.exit_code", 1, EXIT),
    ("harness.events", [{"t": 9.0, "step": "shot:perf"}, {"t": 9.2, "event": "screenshot failed"}], SHOTS),
    (f"@{WHEEL}", REMOVE, PHASES),                                     # a phase that never ran
])
def test_phys_perf_each_budget_fails_alone(path, value, budget):
    assert failing(phys_verdict(edited(path, value))) == [budget]


@pytest.mark.parametrize("path, budgets", [
    ("@chargement.detail.load_ms", [LOAD]),
    ("@chargement", [LOAD]),
    ("@repos.detail.T_ms", [GAP_P95, GAP_P99]),   # no T: the max is still judged
    (f"@{PAN_ZOOM1}.detail.frame_gap", [GAP_P95, GAP_P99, GAP_MAX]),
    (f"@{WHEEL}.detail.render_ms", [RENDER_P95, RENDER_P99]),
    (f"@{DRAG_ECU}.detail.gels", [LONG]),
    (f"@{PAN_FIT}.detail.input_to_draw", [INPUT]),
    (f"@{DRAG_20}.detail.edit_to_values", [EDIT]),
    (f"@{RUN}.detail.engine_lag_frames", [LAG]),
    (f"@{RUN}.detail.solver", [SOLVER]),
    (f"@{RUN}.detail.solver.islands", [SOLVER]),
    (f"@{RUN}.detail.solver.restarts", [SOLVER]),
    (f"@{DRAG_ECU}.detail", [PHASES, EDIT]),       # the step failed before returning its measurements
    (f"@{RUN}", [PHASES, LAG, SOLVER]),
    ("harness.memory_marks", [MEMORY]),
    ("harness.process_tree_under_load.thread_cpu_percent_mean", [CPU]),
    ("harness.process_tree_under_load", [MEMORY, CPU]),
    ("harness.exit_code", [EXIT]),
    ("steps", [STEPS, PHASES, LOAD, GAP_P95, GAP_P99, GAP_MAX, RENDER_P95, RENDER_P99, LONG, INPUT, EDIT, LAG,
               SOLVER]),
])
def test_phys_perf_missing_measurement_fails_its_lines(path, budgets):
    assert failing(phys_verdict(edited(path, REMOVE))) == budgets


def test_phys_perf_budgets_are_inclusive_limits():
    """Every measurement exactly at its limit passes; T = 10 ms keeps 1.1 T and 2 T + 1 ms exact."""
    result = phys_perf_result()
    details = {s["name"]: s["detail"] for s in result["steps"]}
    details["chargement"]["load_ms"] = 200.0
    details["repos"].update(T_ms=10.0, frames=200)
    for name in PERF_PHASES:
        d = details[name]
        d["frame_gap"] = summary(1000, 10.0, 11.0, 21.0, 50.0)
        d["render_ms"] = summary(1000, 3.0, 8.0, 12.0, 20.0)
        d["input_to_draw"] = summary(1000, 20.0, 50.0, 60.0, 70.0)
    for name in (DRAG_ECU, DRAG_20):
        details[name]["edit_to_values"] = summary(10, 40.0, 100.0, 110.0, 120.0)
    details[RUN]["engine_lag_frames"] = summary(9000, 0.0, 5.0, 20.0, 30.0)
    tree = result["harness"]["process_tree_under_load"]
    tree["private_mb_max"] = 300.0   # cockpit 260 MB + 40 MB
    tree["thread_cpu_percent_mean"] = 5.0
    assert failing(phys_verdict(result)) == []


def test_phys_perf_frame_gap_scales_with_the_display_period():
    """The same gaps pass on a 60 Hz display and fail on a 144 Hz one (T = 6.9 ms: 1.1 T = 7.6 ms)."""
    fast = edited("@repos.detail.T_ms", 6.9)
    assert failing(phys_verdict(fast)) == [GAP_P95, GAP_P99]


def test_phys_perf_never_judges_with_an_invented_period():
    """No gap measured at rest: the scenario falls back on 60 Hz, the harness reports T as not measured."""
    checks = n7rt_app.evaluate(edited("@repos.detail.frames", 0), "phys-perf")
    assert failing({name: ok for name, ok, _ in checks}) == [GAP_P95, GAP_P99]
    assert all("T not measured" in detail for name, _, detail in checks if name in (GAP_P95, GAP_P99))


def test_phys_perf_reports_the_worst_phase_and_every_phase_over_the_limit():
    result = edited(f"@{WHEEL}.detail.render_ms", summary(600, 2.1, 9.5, 11.0, 14.0))
    details = {s["name"]: s["detail"] for s in result["steps"]}
    details[DRAG_ECU]["render_ms"] = summary(600, 2.1, 8.2, 11.0, 14.0)
    detail = {name: d for name, _, d in n7rt_app.evaluate(result, "phys-perf")}[RENDER_P95]
    assert detail.startswith(f"p95 9.5 ms in {WHEEL}")
    assert f"over the limit: {WHEEL} 9.5 ms, {DRAG_ECU} 8.2 ms" in detail


def test_phys_perf_memory_line_says_which_processes_grew():
    """The peak over the cockpit reference is split by kind of process (WebView2 --type), and the end is shown."""
    result = edited("harness.process_tree_under_load.private_mb_max", 330.0)
    harness = result["harness"]
    harness["process_roles"] = {"n7rt-can-lab.exe:1": "app", "msedgewebview2.exe:2": "browser",
                                "msedgewebview2.exe:3": "gpu-process", "msedgewebview2.exe:4": "renderer"}
    harness["memory_marks"]["cockpit"]["per_process_private_mb"] = {
        "n7rt-can-lab.exe:1": 6.0, "msedgewebview2.exe:2": 40.0, "msedgewebview2.exe:3": 150.0,
        "msedgewebview2.exe:4": 64.0}
    harness["memory_marks"]["lab"] = {"t": 9.0, "private_mb": 301.0, "settled": True}
    harness["memory_marks"]["fin"] = {"t": 90.0, "private_mb": 280.5, "settled": False}
    tree = harness["process_tree_under_load"]
    tree["private_mb_last"] = 275.0
    tree["peak"] = {"t": 40.9, "private_mb": 330.0, "per_process_private_mb": {
        "n7rt-can-lab.exe:1": 8.0, "msedgewebview2.exe:2": 40.2, "msedgewebview2.exe:3": 200.0,
        "msedgewebview2.exe:4": 81.8, "msedgewebview2.exe:5": 0.0}}
    ok, detail = {name: (ok, d) for name, ok, d in n7rt_app.evaluate(result, "phys-perf")}[MEMORY]
    assert not ok
    assert detail.startswith("+70.0 MB: private 330.0 MB max")
    assert "+15.0 MB at the end" in detail
    assert "+41.0 MB document loaded, at rest (mem:lab)" in detail
    assert "+20.5 MB after the phases and 12 s idle (mem:fin, not settled)" in detail
    assert "growth at the peak (t 40.9 s) by process: gpu-process +50.0, renderer +17.8, app +2.0" in detail


@pytest.mark.parametrize("pid, name, args, role", [
    (1, "n7rt-can-lab.exe", ["n7rt-can-lab.exe", "--e2e", "phys-perf"], "app"),
    (2, "msedgewebview2.exe", ["msedgewebview2.exe", "--embedded-browser-webview=1"], "browser"),
    (3, "msedgewebview2.exe", ["msedgewebview2.exe", "--type=gpu-process", "--gpu-preferences=x"], "gpu-process"),
    (4, "msedgewebview2.exe", ["msedgewebview2.exe", "--type=renderer"], "renderer"),
    (5, "msedgewebview2.exe", ["msedgewebview2.exe", "--type=utility",
                               "--utility-sub-type=network.mojom.NetworkService"], "utility:NetworkService"),
    (6, "msedgewebview2.exe", None, "msedgewebview2.exe"),             # command line not readable
])
def test_process_role_names_the_webview2_process_types(pid, name, args, role):
    class Proc:
        def __init__(self):
            self.pid = pid

        def name(self):
            return name

        def cmdline(self):
            if args is None:
                raise PermissionError("access denied")
            return args

    assert n7rt_app._process_role(Proc(), 1) == role


def test_phys_perf_engine_lag_is_judged_in_run_only():
    """Out of RUN the engine does not advance: a lag counted there is not the §8.6 budget."""
    stopped = edited(f"@{PAN_ZOOM1}.detail.engine_lag_frames", summary(40, 0.0, 30.0, 50.0, 50.0))
    assert failing(phys_verdict(stopped)) == []


def test_phys_perf_scenario_budgets_are_named_in_its_verdict():
    """The scenario's own budgets fold into its ok; the line says which one failed."""
    result = phys_perf_result()
    result["ok"] = result["budgets_ok"] = False
    result["budgets"][f"{PAN_ZOOM1}.frame_gap_p95_ms"] = {"value": 19.1, "limit": 18.37, "ok": False}
    checks = {name: (ok, detail) for name, ok, detail in n7rt_app.evaluate(result, "phys-perf")}
    assert failing({name: ok for name, (ok, _) in checks.items()}) == [STEPS]
    assert f"{PAN_ZOOM1}.frame_gap_p95_ms 19.1 > 18.37" in checks[STEPS][1]


def test_harness_reads_what_the_phys_perf_scenario_writes():
    """Step and phase names and detail keys read by the evaluator are those ui/src/lab/e2e-phys.ts writes."""
    source = E2E_PHYS.read_text(encoding="utf-8")
    assert list(n7rt_app.PHYS_PERF_PHASES) == PERF_PHASES
    for name in PERF_PHASES:
        assert f'phase("{name}"' in source, name
    for name in (DRAG_ECU, DRAG_20):   # phases whose releases send edits
        assert re.search(rf'phase\("{name}",[^\n]*, true\);', source), name
    for name in ("chargement", "repos"):
        assert f'step("{name}"' in source, name
    for key in ("load_ms", "T_ms", "frames", "frame_gap", "render_ms", "input_to_draw", "edit_to_values", "gels",
                "gels_source", "engine_lag_frames", "solver", "frame_us_p95", "substeps_last", "islands", "restarts",
                "budgets_ok"):
        assert re.search(rf"\b{key}\b", source), key


@pytest.mark.parametrize("result", [
    {"ok": False, "error": "no report"},
    {"ok": True, "steps": "chargement", "ui": [], "harness": None, "budgets": []},
    {"ok": True,
     "steps": [None, 3, {"name": ["x"], "detail": {"frame_gap": {}}},
               {"name": PAN_ZOOM1, "ok": True, "detail": {"frame_gap": "fast", "gels": True, "render_ms": [],
                                                          "solver": "x"}},
               {"name": "repos", "ok": True, "detail": {"T_ms": "16.7", "frames": 3}},
               {"name": "chargement", "ok": True, "detail": {"load_ms": True}}],
     "harness": {"events": "x", "memory_marks": [], "process_tree_under_load": 7}},
    {"ok": "yes", "budgets": {"x": 3},
     "steps": [{"name": RUN, "ok": 1, "checks": "x",
                "detail": {"solver": {"restarts": "0"}, "engine_lag_frames": {"count": float("nan"), "p99": 1},
                           "gels": -1, "frame_gap": summary(10, 1.0, float("inf"), float("inf"), float("inf"))}},
               {"name": "repos", "ok": True, "detail": {"T_ms": -16.7, "frames": 120}},
               {"name": "chargement", "ok": True, "detail": {"load_ms": float("nan")}}]},
])
def test_phys_perf_malformed_report_fails_without_crashing(result):
    checks = n7rt_app.evaluate(result, "phys-perf")
    assert [name for name, _, _ in checks] == PHYS_PERF_CHECKS
    # Without any shot: step there is no screenshot to miss; every other line fails.
    assert failing({name: ok for name, ok, _ in checks}) == [c for c in PHYS_PERF_CHECKS if c != SHOTS]


def phys_result(**over) -> dict:
    result = {"ok": True, "scenario": "phys",
              "steps": [{"name": "vue", "ok": True}, {"name": "placement", "ok": True}, {"name": "run-adc", "ok": True}],
              "harness": {"exit_code": 0, "events": [{"t": 2.0, "step": "shot:circuit"},
                                                     {"t": 2.3, "event": "screenshot app_phys_x_circuit.png"}]}}
    result.update(over)
    return result


def test_phys_functional_judges_the_scenario_without_board_checks():
    assert phys_verdict(phys_result(), "phys") == {STEPS: True, EXIT: True, SHOTS: True}


@pytest.mark.parametrize("change", [
    {"ok": False, "error": "ADC2 = 1100, attendu 1228"},
    {"steps": []},   # a scenario without any step proves nothing
    {"steps": [{"name": "vue", "ok": True}, {"name": "run-adc", "ok": False, "error": "ADC1"}]},
])
def test_phys_functional_fails_on_its_own_verdict(change):
    assert failing(phys_verdict(phys_result(**change), "phys")) == [STEPS]


# ------------------------------------------------------------ harness loop, against a stand-in application

FAKE_APP = '''\
"""Stand-in for n7rt-can-lab --e2e: plays N7RT_FAKE_PLAN (progress steps, gates), then writes its report."""
import json, os, sys, threading, time
from pathlib import Path

argv = sys.argv[1:]
plan = json.loads(os.environ["N7RT_FAKE_PLAN"])
report = Path(argv[argv.index("--report") + 1])
released = []


def gate(step):
    kind, _, name = step.partition(":")
    if kind in ("halt", "busy"):
        return kind
    return f"{kind}-{name}" if kind in ("shot", "mem") else None


def name_thread(name):
    import ctypes
    if os.name == "nt":
        k = ctypes.WinDLL("kernel32")
        k.GetCurrentThread.restype = ctypes.c_void_p
        k.SetThreadDescription.argtypes = (ctypes.c_void_p, ctypes.c_wchar_p)
        k.SetThreadDescription(k.GetCurrentThread(), name)
    elif sys.platform.startswith("linux"):
        ctypes.CDLL(None).prctl(15, name.encode(), 0, 0, 0)


def finish(ok, error=None):
    steps = [{"name": s, "ok": True} for s, _ in plan]
    extra = json.loads(os.environ.get("N7RT_FAKE_EXTRA", "{}"))
    report.write_text(json.dumps({"ok": ok, "error": error, "argv": argv, "released": released, "steps": steps,
                                  **extra}), encoding="utf-8")
    sys.exit(0 if ok else 1)


threading.Thread(target=lambda: (name_thread("phys"), threading.Event().wait()), daemon=True).start()
for step, hold in plan:
    Path(str(report) + ".progress").write_text(step, encoding="utf-8")
    g = gate(step)
    if g is None:
        time.sleep(hold)
        continue
    t0 = time.monotonic()
    while not Path(str(report) + f".{g}.go").exists():
        if time.monotonic() - t0 > 30:
            finish(False, f"gate {g} never released")
        time.sleep(0.05)
    released.append(g)
finish(True)
'''


@pytest.fixture
def fake_app(tmp_path, monkeypatch):
    """The harness launches FAKE_APP (through the Python interpreter) instead of the application."""
    pytest.importorskip("psutil")
    app = tmp_path / "fake_app.py"
    app.write_text(FAKE_APP, encoding="utf-8")
    shim = types.SimpleNamespace(Popen=lambda args, **kw: subprocess.Popen([sys.executable, *args], **kw),
                                 DEVNULL=subprocess.DEVNULL, TimeoutExpired=subprocess.TimeoutExpired)
    monkeypatch.setattr(n7rt_app, "subprocess", shim)

    def plan(*steps: tuple[str, float]) -> Path:
        monkeypatch.setenv("N7RT_FAKE_PLAN", json.dumps([list(s) for s in steps]))
        return app
    return plan


def test_phys_run_needs_no_board_and_plays_its_gates(tmp_path, fake_app, monkeypatch):
    monkeypatch.setattr(n7rt_app, "MEM_SETTLE_WINDOW_S", 1.0)
    exe = fake_app(("vue", 0.7), ("mem:cockpit", 0), ("chargement", 0.7), ("shot:circuit", 0), ("run", 1.2))
    report = tmp_path / "app_phys-perf_test.json"
    result = n7rt_app.run_phys(exe, "phys-perf", report, timeout_s=60)
    harness = result["harness"]
    assert result["ok"] is True and harness["exit_code"] == 0
    assert result["argv"] == ["--e2e", "phys-perf", "--report", str(report), "--coordinated"]  # no board, no --serial
    assert result["released"] == ["mem-cockpit", "shot-circuit"]
    cockpit = harness["memory_marks"]["cockpit"]
    assert cockpit["settled"] is True and cockpit["private_mb"] > 0
    events = [e.get("event", "") for e in harness["events"]]
    assert any(e.startswith("memory reference cockpit: private") for e in events)
    assert any(e.startswith("screenshot") for e in events)
    tree = harness["process_tree_under_load"]
    assert tree["samples"] >= 2
    v = phys_verdict(result)
    assert v[STEPS] and v[EXIT] and v[MEMORY]   # the stand-in's memory does not move after the reference
    if NAMED_THREADS:
        assert tree["thread"] == "phys" and tree["thread_cpu_percent_mean"] is not None
        assert v[CPU]   # its phys thread only waits


def test_phys_soak_command_keeps_the_application_board_free(tmp_path):
    exe = tmp_path / "n7rt-can-lab.exe"
    report = tmp_path / "soak.json"
    args = n7rt_app.phys_args(exe, "phys-perf", report, seconds=7200)
    assert args[-2:] == ["--seconds", "7200"]
    assert "--serial" not in args and "--coordinated" in args
    for duration in (-1, 86401):
        with pytest.raises(ValueError, match="duration"):
            n7rt_app.phys_args(exe, "phys-perf", report, seconds=duration)


def test_board_run_still_plays_the_halt_gate(tmp_path, fake_app):
    calls = []
    exe = fake_app(("connexion", 0.7), ("charge", 2.6), ("halt:ready", 0), ("fin", 1.0))
    report = tmp_path / "app_functional_x_test.json"
    result = n7rt_app.run(exe, "functional", "SN0042", "COM99", report, seconds=7, rate=50, coordinated=True,
                          halt=lambda: calls.append("halt"), resume=lambda: calls.append("resume"), timeout_s=60)
    harness = result["harness"]
    assert result["ok"] is True and harness["exit_code"] == 0
    assert result["argv"] == ["--e2e", "functional", "--serial", "SN0042", "--report", str(report),
                              "--seconds", "7", "--rate", "50", "--coordinated"]
    assert calls == ["halt", "resume"] and result["released"] == ["halt"]
    assert [e["event"] for e in harness["events"] if "event" in e] == ["MCU halted (SWD)", "MCU resumed"]
    assert harness["process_tree_under_load"]   # sampled from 2 s after the start of "charge"


@pytest.fixture
def mission_env(tmp_path, monkeypatch):
    """The environment of the mission scenarios (stand-in values)."""
    monkeypatch.setenv("N7RT_INSTRUCTOR_DIR", str(tmp_path / "instructor"))
    monkeypatch.setenv("N7RT_E2E_INSTRUCTOR_CODE", "CODE-1234")
    monkeypatch.setenv("N7RT_E2E_PACK", str(tmp_path / "apps-G01.n7rtpack"))
    monkeypatch.setenv("N7RT_E2E_GROUP_CODE", "GROUP-5678")


def test_mission_runs_refuse_a_missing_environment(tmp_path, fake_app, monkeypatch):
    exe = fake_app(("connexion", 0.1))
    for k in n7rt_app.MISSION_ENV["student-apps"]:
        monkeypatch.delenv(k, raising=False)
    for scenario in n7rt_app.MISSION_SCENARIOS:
        with pytest.raises(ValueError, match="N7RT_INSTRUCTOR_DIR"):
            n7rt_app.run(exe, scenario, "SN0042", "COM99", tmp_path / "r.json", seconds=1, rate=1, coordinated=False,
                         halt=lambda: None, resume=lambda: None, timeout_s=5)


def test_student_run_plays_its_shots_then_judges_the_run_file(tmp_path, fake_app, fake_tool, mission_env, monkeypatch):
    tool, log = fake_tool
    monkeypatch.setattr(n7rt_app, "subprocess", types.SimpleNamespace(
        Popen=lambda args, **kw: subprocess.Popen([sys.executable, *args], **kw),
        run=lambda args, **kw: subprocess.run([sys.executable, *args], **kw),
        DEVNULL=subprocess.DEVNULL, TimeoutExpired=subprocess.TimeoutExpired))
    exe = fake_app(("connexion", 0.6), *[(f"shot:{n}", 0) for n in n7rt_app.STUDENT_SHOTS], ("fichier-essai", 0.6))
    run_file = str(tmp_path / RUN_NAME)
    monkeypatch.setenv("N7RT_FAKE_EXTRA", json.dumps({"run_file": run_file}))
    monkeypatch.setenv("N7RT_FAKE_JUDGE_EXIT", "1")
    assert exe.parent == tool.parent
    report = tmp_path / "app_student-apps_x_test.json"
    result = n7rt_app.run(exe, "student-apps", "SN0042", "COM99", report, seconds=120, rate=600, coordinated=False,
                          halt=lambda: None, resume=lambda: None, timeout_s=60, expect_verdict="fail")
    assert result["ok"] is True and result["harness"]["exit_code"] == 0
    assert result["argv"][:2] == ["--e2e", "student-apps"] and result["argv"][-2:] == ["--expect-verdict", "fail"]
    assert result["released"] == [f"shot-{n}" for n in n7rt_app.STUDENT_SHOTS]
    assert result["judge"] == {"exit_code": 1, "verdict": "fail"}
    called = json.loads(log.read_text(encoding="utf-8"))
    assert called["argv"] == ["judge", str(tmp_path / "instructor"), run_file, "--code-stdin"] and called["code_ok"]
    assert "CODE-1234" not in json.dumps(result) and "GROUP-5678" not in json.dumps(result)


def test_mission_run_asks_for_the_expected_verdict_and_plays_the_shots(tmp_path, fake_app, mission_env):
    exe = fake_app(("connexion", 0.6), *[(f"shot:{n}", 0) for n in n7rt_app.MISSION_SHOTS], ("verdict", 0.6))
    report = tmp_path / "app_mission-apps_x_test.json"
    result = n7rt_app.run(exe, "mission-apps", "SN0042", "COM99", report, seconds=120, rate=600, coordinated=False,
                          halt=lambda: None, resume=lambda: None, timeout_s=60, expect_verdict="fail")
    assert result["ok"] is True and result["harness"]["exit_code"] == 0
    assert result["argv"] == ["--e2e", "mission-apps", "--serial", "SN0042", "--report", str(report),
                              "--seconds", "120", "--rate", "600", "--expect-verdict", "fail"]
    assert result["released"] == [f"shot-{n}" for n in n7rt_app.MISSION_SHOTS]
    with pytest.raises(ValueError):
        n7rt_app.run(exe, "mission-apps", "SN0042", "COM99", report, seconds=1, rate=1, coordinated=False,
                     halt=lambda: None, resume=lambda: None, timeout_s=5, expect_verdict="maybe")


def test_other_board_scenarios_get_no_expected_verdict(tmp_path, fake_app):
    exe = fake_app(("connexion", 0.6))
    report = tmp_path / "app_showcase_x_test.json"
    result = n7rt_app.run(exe, "showcase", "SN0042", "COM99", report, seconds=1, rate=1, coordinated=False,
                          halt=lambda: None, resume=lambda: None, timeout_s=60)
    assert "--expect-verdict" not in result["argv"]


def test_run_kills_an_application_that_never_finishes(tmp_path, fake_app):
    exe = fake_app(("chargement", 60))
    report = tmp_path / "app_phys_test.json"
    t0 = time.monotonic()
    result = n7rt_app.run_phys(exe, "phys", report, timeout_s=1.5)
    assert time.monotonic() - t0 < 15
    harness = result["harness"]
    assert result["ok"] is False and result["error"] == "no report"
    assert "timeout" in [e.get("event") for e in harness["events"]]
    assert harness["exit_code"] not in (None, 0)
    assert failing(phys_verdict(result, "phys")) == [STEPS, EXIT]


# ------------------------------------------------------------ CPU of a named thread, memory reference

def name_current_thread(name: str) -> None:
    """Gives the calling thread an OS name, as Rust's thread::Builder::name does for the core's thread."""
    import ctypes
    if os.name == "nt":
        k = ctypes.WinDLL("kernel32")
        k.GetCurrentThread.restype = ctypes.c_void_p
        k.SetThreadDescription.argtypes = (ctypes.c_void_p, ctypes.c_wchar_p)
        k.SetThreadDescription(k.GetCurrentThread(), name)
    else:
        ctypes.CDLL(None).prctl(15, name.encode(), 0, 0, 0)  # PR_SET_NAME


@pytest.mark.skipif(not NAMED_THREADS, reason="thread names are read on Windows and Linux")
def test_named_thread_cpu_is_a_share_of_the_machine():
    """A busy thread costs one core, 100/ncpu % of the machine; a waiting thread costs nothing."""
    psutil = pytest.importorskip("psutil")
    stop, named = threading.Event(), threading.Barrier(3, timeout=10)

    def busy() -> None:
        name_current_thread("n7rt-busy")
        named.wait()
        while not stop.is_set():
            pass

    def idle() -> None:
        name_current_thread("n7rt-idle")
        named.wait()
        stop.wait()

    workers = [threading.Thread(target=f, daemon=True) for f in (busy, idle)]
    for w in workers:
        w.start()
    named.wait()
    try:
        samplers = {n: n7rt_app.TreeSampler(os.getpid(), thread=n) for n in ("n7rt-busy", "n7rt-idle")}
        t0 = time.monotonic()
        for s in samplers.values():
            s.sample(0.0)
        time.sleep(1.5)
        t = time.monotonic() - t0
        for s in samplers.values():
            s.sample(t)
    finally:
        stop.set()
        for w in workers:
            w.join()
    core = 100 / psutil.cpu_count()
    busy_share = samplers["n7rt-busy"].samples[-1]["thread_cpu_percent"]
    idle_share = samplers["n7rt-idle"].samples[-1]["thread_cpu_percent"]
    assert busy_share is not None and idle_share is not None, "named threads not found"
    assert 0.3 * core <= busy_share <= 1.2 * core
    assert idle_share <= 0.05 * core
    assert samplers["n7rt-idle"].summary()["thread"] == "n7rt-idle"


def test_thread_absent_is_reported_not_invented():
    pytest.importorskip("psutil")
    sampler = n7rt_app.TreeSampler(os.getpid(), thread="no-such-thread")
    sampler.sample(0.0)
    sampler.sample(0.6)
    assert [s["thread_cpu_percent"] for s in sampler.samples] == [None, None]
    assert sampler.summary()["thread_cpu_percent_mean"] is None


def test_memory_reference_settles_on_a_steady_process():
    pytest.importorskip("psutil")
    sampler = n7rt_app.TreeSampler(os.getpid())
    t0 = time.monotonic()
    mark = sampler.settle(lambda: time.monotonic() - t0, window_s=1.0, tolerance_mb=8.0, timeout_s=10.0)
    assert mark["settled"] is True and mark["private_mb"] > 0 and mark["spread_mb"] <= 8.0
    measured = [s for s in sampler.samples if s["processes"]]
    assert measured[-1]["t"] - measured[0]["t"] >= 1.0   # the whole window was observed


def test_memory_reference_gives_up_at_its_timeout():
    pytest.importorskip("psutil")
    sampler = n7rt_app.TreeSampler(os.getpid())
    t0 = time.monotonic()
    mark = sampler.settle(lambda: time.monotonic() - t0, window_s=0.5, tolerance_mb=-1.0, timeout_s=1.2)
    assert mark["settled"] is False and mark["private_mb"] > 0
    assert 1.2 <= time.monotonic() - t0 < 3.0


# ------------------------------------------------------------ app-test command line

@pytest.mark.parametrize("argv, message", [
    (["app-test", "--phys", "{board}"], "without a board"),
    (["app-test", "{board}", "--scenario", "phys-perf"], "use --phys"),
    (["app-test"], "name the board"),
    (["app-test", "--phys", "--scenario", "perf"], "--scenario phys, phys-perf, pack-<id> or all"),
    (["app-test", "--phys", "--scenario", "mission-apps"], "--scenario phys, phys-perf, pack-<id> or all"),
    (["app-test", "{board}", "--scenario", "mission-apps", "--expect-verdict", "maybe"], "invalid choice"),
    (["app-test", "{board}", "--scenario", "mission-apps"], "set N7RT_INSTRUCTOR_DIR, N7RT_E2E_INSTRUCTOR_CODE"),
    (["app-test", "{board}", "--scenario", "student-apps"],
     "set N7RT_INSTRUCTOR_DIR, N7RT_E2E_INSTRUCTOR_CODE, N7RT_E2E_PACK, N7RT_E2E_GROUP_CODE"),
])
def test_app_test_refuses_to_mix_board_and_physical_lab(argv, message):
    """Refused before any registry lookup, probe, SWD access or launch."""
    import stm32lab
    board = sorted(stm32lab.load_boards())[0]
    env = {k: v for k, v in os.environ.items() if k not in n7rt_app.MISSION_ENV["student-apps"]}
    proc = subprocess.run([sys.executable, str(STM32LAB), *(a.format(board=board) for a in argv)],
                          capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=120, env=env)
    assert proc.returncode == 2 and message in proc.stderr, proc.stdout + proc.stderr


def test_explicit_relative_paths_are_resolved_before_child_changes_directory(tmp_path, monkeypatch):
    monkeypatch.chdir(tmp_path)
    exe = Path("instructor") / n7rt_app.EXE_NAME
    tool = Path("instructor") / n7rt_app.TOOL_NAME
    exe.parent.mkdir()
    exe.touch()
    tool.touch()

    assert n7rt_app.app_exe(True, exe) == tmp_path / exe
    assert n7rt_app.tool_exe(tmp_path / "student" / n7rt_app.EXE_NAME, tool) == tmp_path / tool


def test_physical_lab_accepts_executable_outside_student_repository(tmp_path, monkeypatch):
    from types import SimpleNamespace
    import stm32lab

    student = tmp_path / "student"
    exe = tmp_path / "instructor" / n7rt_app.EXE_NAME
    exe.parent.mkdir()
    exe.touch()
    monkeypatch.setattr(stm32lab, "ROOT", student)
    monkeypatch.setattr(stm32lab, "LOG_DIR", student / "data" / "logs")
    calls = []
    monkeypatch.setattr(n7rt_app, "run_phys", lambda app, *a, **kw: calls.append(app) or {"ok": True})
    monkeypatch.setattr(n7rt_app, "evaluate", lambda *a: [("scenario", True, "passed")])

    args = SimpleNamespace(board=None, scenario="phys", release=True, exe=exe, seconds=120)
    assert stm32lab.cmd_app_test_phys(args) == 0
    assert calls == [exe]


# ------------------------------------------------------------ UI guards, vitest, Rust

@pytest.mark.skipif(NODE is None, reason="node absent")
def test_ui_safety_check_passes_on_the_real_ui():
    proc = subprocess.run([NODE, str(APP / "scripts" / "check-ui.mjs")], capture_output=True, text=True)
    assert proc.returncode == 0, proc.stdout + proc.stderr


@pytest.mark.skipif(NODE is None, reason="node absent")
def test_ui_safety_check_rejects_html_sinks(tmp_path):
    (tmp_path / "ui" / "src").mkdir(parents=True)
    (tmp_path / "ui" / "index.html").write_text('<script type="module" src="./src/main.ts"></script>\n', encoding="utf-8")
    (tmp_path / "ui" / "src" / "main.ts").write_text("row.innerHTML = board.text;\n", encoding="utf-8")
    proc = subprocess.run([NODE, str(APP / "scripts" / "check-ui.mjs"), str(tmp_path)], capture_output=True, text=True)
    assert proc.returncode == 1 and "innerHTML" in proc.stderr


GUARD = "*, *::before, *::after { border-radius: 0 !important; }\n"


def ui_tree(tmp_path, css: str, ts: str = "export {};\n", html_extra: str = "", styles: dict | None = None) -> Path:
    (tmp_path / "ui" / "src").mkdir(parents=True)
    (tmp_path / "ui" / "index.html").write_text(
        f'<script type="module" src="./src/main.ts"></script>{html_extra}\n', encoding="utf-8")
    (tmp_path / "ui" / "src" / "main.ts").write_text(ts, encoding="utf-8")
    (tmp_path / "ui" / "styles.css").write_text(css, encoding="utf-8")
    for name, text in (styles or {}).items():  # ui/styles/<name>: the design system's sheets
        (tmp_path / "ui" / "styles").mkdir(exist_ok=True)
        (tmp_path / "ui" / "styles" / name).write_text(text, encoding="utf-8")
    return tmp_path


# A minimal design system that passes the token rules: colours in tokens.css only, dark native controls.
STYLES = {
    "tokens.css": ":root { --text: #e4e6eb; --line: #24262f; }\n",
    "base.css": GUARD + "html { color-scheme: dark; }\n",
    "components.css": (".btn { color: var(--text); border: 1px solid var(--line); }\n"
                       ".select { background-image: url(\"data:image/svg+xml,%3Csvg fill='%23a3a8b4'/%3E\"); }\n"
                       "#fade .x { color: var(--text); }\n"),  # an id selector, not a colour
}


def check_ui(root: Path) -> subprocess.CompletedProcess:
    return subprocess.run([NODE, str(APP / "scripts" / "check-ui.mjs"), str(root)], capture_output=True, text=True,
                          encoding="utf-8", errors="replace")


@pytest.mark.skipif(NODE is None, reason="node absent")
def test_design_guard_accepts_square_styles(tmp_path):
    ts = ("const xs = [1, 2].filter((x) => x > 1);\nctx.lineJoin = 'miter';\nctx.lineCap = 'butt';\n"
          "ctx.fillRect(0, 0, 4, 4);\nctx.setLineDash([2, 2]);\nexport { xs };\n")
    proc = check_ui(ui_tree(tmp_path, GUARD + ".btn { border: 1px solid #333; transition: color 80ms linear; animation: none; }\n", ts))
    assert proc.returncode == 0, proc.stdout + proc.stderr


@pytest.mark.skipif(NODE is None, reason="node absent")
@pytest.mark.parametrize("css, ts, html, why", [
    (GUARD + ".card { border-radius: 4px; }", "export {};", "", "coins arrondis"),
    (GUARD + ".chip { border-top-left-radius: 999px }", "export {};", "", "coins arrondis"),
    (GUARD, "el.style.borderRadius = '6px';", "", "borderRadius"),
    (GUARD, "export {};", '<svg><rect rx="3" width="4" height="4"/></svg>', "coins arrondis"),
    (GUARD + ".panel { box-shadow: 0 2px 8px #000; }", "export {};", "", "ombres"),
    (GUARD + ".x { transition: opacity 300ms ease; }", "export {};", "", "transition"),
    (GUARD + "@keyframes pulse { from { opacity: 0 } }", "export {};", "", "animation"),
    (".btn { border: 1px solid #333; }", "export {};", "", "garde globale"),
    (GUARD + ".glass { backdrop-filter: blur(6px); }", "export {};", "", "flou"),
    (GUARD + ".icon { filter: drop-shadow(0 1px 2px #000); }", "export {};", "", "ombre portée"),
    (GUARD, "ctx.shadowBlur = 4;", "", "ombre Canvas"),
    (GUARD, "ctx.roundRect(0, 0, 4, 4, 2);", "", "roundRect"),
    (GUARD, "ctx.arc(0, 0, 3, 0, 6.28);", "", "cercles"),
    (GUARD, "ctx.lineJoin = 'round';", "", "jointures arrondies"),
    (GUARD, "ctx.lineDashOffset = t;", "", "tirets"),
    (GUARD, "ctx.filter = 'blur(2px)';", "", "filtre Canvas"),
])
def test_design_guard_rejects(tmp_path, css, ts, html, why):
    proc = check_ui(ui_tree(tmp_path, css, ts, html))
    assert proc.returncode == 1 and why in proc.stderr, proc.stdout + proc.stderr


@pytest.mark.skipif(NODE is None, reason="node absent")
def test_design_guard_accepts_classed_controls_and_tokens(tmp_path):
    ts = ('const a = el("button", "btn fiche-save", "Enregistrer");\nconst b = el("select", `select ${cls}`);\n'
          'const c = el("input", cls);\nconst d = el("input");\nconst e = el("textarea", "textarea x");\nexport { a, b, c, d, e };\n')
    proc = check_ui(ui_tree(tmp_path, "", ts, styles=STYLES))
    assert proc.returncode == 0, proc.stdout + proc.stderr


@pytest.mark.skipif(NODE is None, reason="node absent")
@pytest.mark.parametrize("ts, styles, why", [
    ('el("button", "fiche-save", "Enregistrer");', STYLES, "classe du système de design"),
    ('el("select", `fiche-order ${x}`);', STYLES, "classe du système de design"),
    ('el("textarea");', STYLES, "contrôle sans classe"),
    ("export {};", {**STYLES, "components.css": ".x { color: var(--text-muted, var(--text)); }\n"}, "non déclarée"),
    ("export {};", {**STYLES, "components.css": ".x { border-color: #454a5a; }\n"}, "couleur brute"),
    ("export {};", {**STYLES, "base.css": GUARD}, "color-scheme"),
])
def test_design_guard_rejects_unclassed_controls_and_stray_colours(tmp_path, ts, styles, why):
    proc = check_ui(ui_tree(tmp_path, "", ts, styles=styles))
    assert proc.returncode == 1 and why in proc.stderr, proc.stdout + proc.stderr


@pytest.mark.skipif(NPM is None or not (APP / "node_modules").is_dir(),
                    reason="npm or app/node_modules absent (npm install in app/)")
def test_vitest_suite():
    """Canvas, panels, units and DTO fixtures (docs/physical-lab-v1.md §10): `npm test` in app/."""
    proc = subprocess.run([NPM, "test"], cwd=APP, capture_output=True, text=True, encoding="utf-8",
                          errors="replace", env={**os.environ, "NO_COLOR": "1"}, timeout=900)
    assert proc.returncode == 0, proc.stdout[-3000:] + proc.stderr[-3000:]


@pytest.mark.skipif(os.environ.get("N7RT_RUN_CARGO") != "1" or shutil.which("cargo") is None,
                    reason="Rust tests: set N7RT_RUN_CARGO=1 with a working cargo (app/README.md)")
def test_rust_crates():
    """Codec (framing properties), link engine, Physical Lab model, solver and engine. Not --workspace, which would
    build src-tauri (docs/physical-lab-v1.md §10)."""
    proc = subprocess.run(["cargo", "test", "-p", "n7rt-link", "-p", "n7rt-lab", "-p", "n7rt-circuit",
                           "-p", "n7rt-solver"], cwd=APP, capture_output=True, text=True, encoding="utf-8",
                          errors="replace")
    assert proc.returncode == 0, proc.stdout[-3000:] + proc.stderr[-3000:]


# --- Editions: application and tool given explicitly, mission artifacts out of the student tree ---------------------
def test_explicit_exe_and_tool(tmp_path):
    exe = tmp_path / "edition" / n7rt_app.EXE_NAME
    exe.parent.mkdir()
    exe.write_bytes(b"")
    assert n7rt_app.app_exe(True, exe) == exe
    with pytest.raises(FileNotFoundError):
        n7rt_app.app_exe(True, tmp_path / "absent" / n7rt_app.EXE_NAME)
    tool = tmp_path / "bin" / n7rt_app.TOOL_NAME
    assert n7rt_app.tool_exe(exe, tool) == tool
    assert n7rt_app.tool_exe(exe) == exe.parent / n7rt_app.TOOL_NAME


def test_mission_reports_go_to_the_instructor_directory(tmp_path):
    logs = tmp_path / "logs"
    env = {n7rt_app.ENV_INSTRUCTOR_DIR: str(tmp_path / "missions-repo")}
    for scenario in n7rt_app.MISSION_SCENARIOS:
        mission = n7rt_app.mission_of(scenario)
        assert n7rt_app.report_dir(scenario, logs, env) == tmp_path / "missions-repo" / "missions" / mission / "reports" / "app-test"
    assert n7rt_app.report_dir("functional", logs, env) == logs
    assert n7rt_app.report_dir("mission-apps", logs, {}) == logs


def test_ide_scenarios_require_workflow_proof_and_keep_mission_reports_private(tmp_path):
    for scenario in ("ide", "ide-student-apps", "ide-mission-apps"):
        missing = n7rt_app.evaluate({"ok": True, "steps": [{"name": "nominal", "ok": True}]}, scenario)[0]
        assert missing[0] == "IDE edit/build/flash/reconnect" and missing[1] is False
        failed = n7rt_app.evaluate({"ide": [{"ok": False}]}, scenario)[0]
        assert failed[1] is False
        passed = n7rt_app.evaluate({"ide": [{"ok": True}]}, scenario)[0]
        assert passed[1] is True
    env = {n7rt_app.ENV_INSTRUCTOR_DIR: str(tmp_path / "instructor")}
    assert n7rt_app.report_dir("ide-student-apps", tmp_path, env) == tmp_path / "instructor/missions/apps/reports/app-test"


@pytest.mark.parametrize("last_capture_ok", [False, True])
def test_ide_capture_acceptance_requires_every_requested_screenshot(last_capture_ok):
    names = ["01-edit", "02-unsaved", "03-build", "04-error", "05-flash", "06-reconnected"]
    events = []
    for name in names:
        events.append({"step": f"shot:{name}"})
        captured = last_capture_ok or name != "06-reconnected"
        events.append({"event": f"screenshot app_ide_{name}.png" if captured else "screenshot failed"})
    result = {
        "ide": [{"ok": True} for _ in range(6)],
        "ok": True,
        "steps": [{"name": "nominal", "ok": True}],
        "verifications": [{"step": "nominal", "mismatches": [], "oracle_mismatches": [],
                           "matched": 1, "displayed": 1, "oracle_checked": 0}],
        "harness": {"events": events},
    }
    checks = n7rt_app.evaluate(result, "ide")
    screenshots = [check for check in checks if check[0] == "screenshots"]
    assert len(screenshots) == 1
    assert screenshots[0][1] is last_capture_ok
    assert screenshots[0][2].startswith("6/6" if last_capture_ok else "5/6")
    assert all(ok for label, ok, _ in checks if label != "screenshots")


# --------------------------------------------------------------------------------------- release 0.3 progression

PASS_LINE = "Vérification réussie. Étape validée le 14/10/2026 à 10:42."
FAIL_DIAG = ["Échec à corriger.", "Diagnostic (fiche) : non conforme", "Réparation (essai caché) : non vérifiée",
             "Le détail n'est pas communiqué. Reprenez vos mesures et vos essais."]
CP_LINES = [f"CP-{k + 1} — {label} : PASS" for k, label in enumerate((
    "Recevoir et filtrer les trames de l'ECU", "Décoder les valeurs nominales",
    "Respecter les limites et les valeurs invalides", "Contrôler l'intégrité des messages",
    "Superviser fraîcheur et séquence", "Traiter un défaut signalé par la source",
    "Appliquer la logique de sécurité de la mission", "Reprendre après un défaut"))]


def progression_result(stage: str = "full", **over) -> dict:
    result = mission_result("fail")
    del result["mission"]
    names = {"until-incident": n7rt_app.PROGRESSION_SHOTS[:8], "resume": n7rt_app.PROGRESSION_SHOTS[8:]}.get(stage, n7rt_app.PROGRESSION_SHOTS)
    shots = [{"t": 1.0 + i, "step": f"shot:{n}"} for i, n in enumerate(names)]
    captured = [{"t": 1.5 + i, "event": f"screenshot app_progression-apps_{n}.png"} for i, n in enumerate(names)]
    attempts = [{"step": 0, "label": "E0", "lines": [PASS_LINE, "Incident 1 ouvert. Le véhicule revient d'essai avec une anomalie."]},
                {"step": 1, "label": "E1 catégorie fausse, sans réparation", "lines": list(FAIL_DIAG)},
                {"step": 1, "label": "E1", "lines": [PASS_LINE, "Incident 2 ouvert. Le véhicule revient d'essai avec une anomalie."]},
                {"step": 2, "label": "E2", "lines": [PASS_LINE, "Incident 3 ouvert. Le véhicule revient d'essai avec une anomalie."]},
                {"step": 3, "label": "E3", "lines": [PASS_LINE, "Qualification finale ouverte."]},
                {"step": 4, "label": "E4", "lines": [*CP_LINES, "Mission clôturée.", "La fiche finale est exportée dans le dossier Documents."]}]
    result.update({
        "progression": {"stage": stage, "attempts": attempts, "flashes": [{"at": "reference-r2", "crc32": "0x1234ABCD"}],
                        "closed": {"mission": "apps", "group": "E2E", "reproductions": 1}},
        "ide": [{"step": "flash-reconnect-ui", "ok": True, "crc": "0x0000BEEF"}],
        "export": {"folder_name": "apps-E2E-20261014-1042", "complete": True, "sha256_ok": True, "etapes": ["VALIDÉ"] * 6,
                   "temps_actif": "0 h 41 min", "reproductions": 1,
                   "tentatives": [{"verdict": v, "crc32": c} for v, c in (("PASS", "0x0000BEEF"), ("FAIL", "0x0000BEEF"),
                                  ("PASS", "0x0000BEEF"), ("PASS", "0x1234ABCD"), ("PASS", "0x1234ABCD"), ("PASS", "0x1234ABCD"))],
                   "reparations": [{"rang": k, "crc32_activation": "0x0000BEEF", "crc32_validation": "0x1234ABCD",
                                    "circuit_lines": 2} for k in (1, 2, 3)]},
        "harness": {"exit_code": 0, "events": shots + captured},
        "shots": {n: {"dpr": 1.5, "inner": [1360, 860], "rects": {"#progress-section": [10, 20, 300, 400]}} for n in names},
    })
    result["steps"].append({"name": "e1-reussite", "ok": True})
    result.update(over)
    return result


def progression_verdict(result: dict) -> dict[str, bool]:
    return {name: ok for name, ok, _ in n7rt_app.evaluate(result, "progression-apps")}


def test_a_full_progression_passes_every_check():
    v = progression_verdict(progression_result())
    assert all(v.values()), v
    for label in ("verdict lines are public texts only", "mission closed (E5)", "recorded CRC-32 = flashed binary",
                  "attempts recorded = attempts played", "SHA-256 printed = SHA-256 of fiche.json", "screenshots",
                  "no runtime reboot during the mission", "reproductions recorded", "active time recorded"):
        assert label in v, label


@pytest.mark.parametrize("change, failing", [
    # a verdict line with a value or an expected answer in it
    (lambda r: r["progression"]["attempts"][1]["lines"].append("Mesure attendue : 0,37 V"), "verdict lines are public texts only"),
    (lambda r: r["export"]["tentatives"][3].update(crc32="0xDEADBEEF"), "recorded CRC-32 = flashed binary"),
    (lambda r: r["export"]["tentatives"][1].update(verdict="PASS"), "attempts recorded = attempts played"),
    (lambda r: r["export"].update(sha256_ok=False), "SHA-256 printed = SHA-256 of fiche.json"),
    (lambda r: r["export"].update(reproductions=0), "reproductions recorded"),
    (lambda r: r["export"].update(temps_actif="0 h 00 min"), "active time recorded"),
    (lambda r: r["export"]["reparations"][2].update(crc32_validation=None), "repair recorded for the three incidents (CRC at activation and validation)"),
    (lambda r: r["export"]["etapes"].__setitem__(4, "EN COURS"), "every step VALIDÉ in the fiche"),
    # a capture taken while its panel was hidden (an element without rectangle)
    (lambda r: r["shots"]["07-consigner"]["rects"].update({"#progress-fiche .incident-consign": None}), "tutorial captures, every element shown"),
])
def test_a_progression_fails_on_each_inconsistency(change, failing):
    result = progression_result()
    change(result)
    v = progression_verdict(result)
    assert [k for k, ok in v.items() if not ok] == [failing], v


def test_a_progression_without_its_final_fiche_fails():
    v = progression_verdict(progression_result(export={}))
    assert v["final fiche exported"] is False


def test_the_until_incident_stage_ends_in_incident_1():
    stopped = progression_result("until-incident")
    stopped["progression"] = {"stage": "until-incident", "attempts": stopped["progression"]["attempts"][:2],
                              "stopped_at": {"step": 1, "attempts": 1}, "resume_part_sha256": "ab" * 32}
    del stopped["export"]
    v = progression_verdict(stopped)
    assert all(v.values()) and "closed in incident 1 after its first attempt" in v and "mission closed (E5)" not in v, v
    stopped["progression"]["stopped_at"] = {"step": 2, "attempts": 1}
    assert progression_verdict(stopped)["closed in incident 1 after its first attempt"] is False


def test_the_resume_stage_compares_the_attempts_it_played_with_the_end_of_the_fiche():
    result = progression_result("resume")
    result["progression"]["attempts"] = result["progression"]["attempts"][2:]
    assert all(progression_verdict(result).values())


def test_the_pack_scenario_runs_without_board_and_checks_the_initial_progression():
    assert n7rt_app.MISSION_ENV["pack-bps"] == ("N7RT_INSTRUCTOR_DIR", "N7RT_E2E_INSTRUCTOR_CODE", "N7RT_E2E_PACK", "N7RT_E2E_GROUP_CODE")
    assert all(s not in n7rt_app.MISSION_SCENARIOS for s in n7rt_app.PACK_SCENARIOS)
    steps = [{"name": "ouverture-mission", "ok": True},
             {"name": "progression-initiale", "ok": True, "detail": {"steps": ["EN COURS"] + ["À FAIRE"] * 4}}]
    result = {"ok": True, "steps": steps, "harness": {"exit_code": 0, "events": []}}
    assert all(ok for _, ok, _ in n7rt_app.evaluate(result, "pack-sas"))
    result["steps"] = steps[:1]
    assert not all(ok for _, ok, _ in n7rt_app.evaluate(result, "pack-sas"))


def test_read_export_keeps_states_verdicts_and_crc_never_the_answers(tmp_path):
    import hashlib
    folder = tmp_path / "N7RT CAN Lab" / "fiches" / "apps-E2E-20261014-1042"
    (folder / "scellés").mkdir(parents=True)
    doc = {"etapes": [{"etape": "Essai nominal", "etat": "VALIDÉ"}], "temps_actif": "0 h 12 min",
           "nominal": {"supervision": {"timeout_ms": 123}},
           "incidents": [{"rang": 1, "categorie": "SECRET", "reparation": {"crc32_activation": "0x1", "crc32_validation": "0x2",
                                                                           "circuit": ["Fil retiré", "Fil ajouté"]}}],
           "reproductions": [{"incident": 1}], "tentatives": [{"etape": "Essai nominal", "verdict": "PASS", "crc32": "0x1",
                                                               "circuit_sha256": "c" * 64}],
           "qualification": {"competences": None}}
    raw = json.dumps(doc).encode()
    (folder / "fiche.json").write_bytes(raw)
    (folder / "fiche.html").write_text(f"<p>Empreinte SHA-256 de fiche.json : {hashlib.sha256(raw).hexdigest()}</p>", encoding="utf-8")
    (folder / "scellés" / "fiche-finale.sealed").write_bytes(b"x")
    ex = n7rt_app.read_export("apps", "E2E", 0, root=tmp_path)
    assert ex["complete"] and ex["sha256_ok"] and ex["reproductions"] == 1 and ex["etapes"] == ["VALIDÉ"]
    assert ex["reparations"] == [{"rang": 1, "crc32_activation": "0x1", "crc32_validation": "0x2", "circuit_lines": 2}]
    assert "SECRET" not in json.dumps(ex) and "123" not in json.dumps(ex) and "c" * 64 not in json.dumps(ex)
    (folder / "fiche.json").write_bytes(raw + b" ")
    assert n7rt_app.read_export("apps", "E2E", 0, root=tmp_path)["sha256_ok"] is False
    assert n7rt_app.read_export("apps", "OTHER", 0, root=tmp_path) == {}


def test_a_harness_run_never_lets_webview2_throttle_a_covered_window():
    env = n7rt_app.webview_env({"PATH": "x", "N7RT_E2E_STAGE": "resume"})
    args = env["WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS"]
    assert env["PATH"] == "x" and env["N7RT_E2E_STAGE"] == "resume"
    for flag in ("CalculateNativeWinOcclusion", "--disable-background-timer-throttling", "--disable-renderer-backgrounding",
                 "--disable-backgrounding-occluded-windows", "msWebOOUI"):
        assert flag in args
