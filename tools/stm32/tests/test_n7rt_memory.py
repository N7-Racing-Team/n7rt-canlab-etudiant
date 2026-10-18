"""Memory verdicts distinguish incomplete evidence, sustained growth, and a stable over-budget footprint."""
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import n7rt_memory  # noqa: E402


def run(base=250, slope=0, *, end=7200, gap=False, runtime_ok=True):
    times = range(0, end + 1, 5)
    samples = [{"t": t, "private_mb": base + slope * t / 3600,
                "window_visibility": {"found": True, "visible_ok": True, "is_iconic": False,
                                      "is_visible": True, "restore_requested": False, "window_pid": 1,
                                      "hwnd": 42, "rect": [0, 0, 1360, 860], "client": [1360, 830]},
                "per_process_private_mb": {"app:1": 10, "webview:2": base - 10 + slope * t / 3600}}
               for t in times if not gap or not 2400 <= t <= 2500]
    return {"harness": {"samples": samples, "events": [{"t": 0, "step": "run-soak"}],
                         "process_roles": {"app:1": "app", "webview:2": "renderer"}, "exit_code": 0,
                         "exe_sha256": "1" * 64},
            "release_validation": {"candidate_exe_sha256": "1" * 64,
                                   "hash_verified_before_launch": True, "hash_verified_after_run": True},
            "soak": {"elapsed_seconds": end, "journal_incomplete": False},
            "steps": [{"name": "chargement", "ok": True,
                       "checks": [{"name": "gabarit", "ok": True}],
                       "detail": {"components": 150, "junctions": 50, "wires": 300}},
                      {"name": "edition-churn", "ok": True,
                       "checks": [{"name": name, "ok": True} for name in (
                           "churn complet", "topologie et états originaux restaurés", "aucun redémarrage moteur pendant churn",
                           "channels temporaires libérés")],
                       "detail": {"cycles": 100, "created": 100, "label_updates": 200,
                                  "pot_interactions": 100, "deleted": 100, "deleted_state_checks": 100,
                                  "original_components": 200, "original_wires": 300,
                                  "topology_restored": True, "engine_restarts": 0,
                                  "retained_pot_channels_before": 1, "retained_pot_channels_after": 1,
                                  "retained_pot_channels_peak": 2}},
                      {"name": "run-soak", "ok": runtime_ok,
                       "checks": [{"name": "journal sans perte", "ok": True}]}]}


def test_journal_integrity_must_be_explicit_and_lossless():
    for mutation in range(5):
        result = run()
        if mutation == 0:
            result["steps"][-1]["checks"] = []
        elif mutation == 1:
            result["steps"][-1]["checks"][0]["ok"] = False
        elif mutation == 2:
            result["soak"].pop("journal_incomplete")
        elif mutation == 3:
            result["soak"]["journal_incomplete"] = True
        else:
            result["soak"]["journal_incomplete"] = 0
        verdict = n7rt_memory.analyze(result)
        assert not verdict["complete"] and not verdict["journal_integrity_ok"]


def test_native_window_visibility_must_remain_attested_for_every_RUN_sample():
    for key, value in (("is_iconic", True), ("is_visible", False), ("visible_ok", False),
                       ("restore_requested", True), ("rect", [0, 0, 0, 34]), ("window_pid", False)):
        result = run()
        result["harness"]["samples"][500]["window_visibility"][key] = value
        verdict = n7rt_memory.analyze(result)
        assert not verdict["complete"] and verdict["invalid_window_sample_count"] == 1


def test_stable_with_gc_oscillation():
    result = run()
    for s in result["harness"]["samples"]:
        extra = 8 if (s["t"] // 5) % 2 else 0
        s["private_mb"] += extra
        s["per_process_private_mb"]["webview:2"] += extra
    verdict = n7rt_memory.analyze(result)
    assert verdict["complete"] and verdict["all_memory_criteria_ok"]
    assert abs(verdict["tree_trend"]["slope_mib_per_hour"]) < 1


def test_sustained_leak_fails_drift_with_process_attribution():
    verdict = n7rt_memory.analyze(run(base=180, slope=30))
    assert verdict["complete"] and verdict["absolute_budget_ok"]
    assert not verdict["drift_ok"] and verdict["classification"].startswith("fuite restante")
    assert verdict["per_role"]["renderer"]["slope_mib_per_hour"] > 29


def test_over_budget_can_be_stable_without_passing_all_criteria():
    verdict = n7rt_memory.analyze(run(base=400))
    assert verdict["classification"] == "stable" and not verdict["absolute_budget_ok"]
    assert not verdict["all_memory_criteria_ok"]


def test_peak_is_not_hidden_by_median_drift():
    result = run()
    result["harness"]["samples"][300]["private_mb"] = 350
    result["harness"]["samples"][300]["per_process_private_mb"]["webview:2"] = 340
    verdict = n7rt_memory.analyze(result)
    assert verdict["drift_ok"] and not verdict["absolute_budget_ok"]
    assert verdict["peak_private_mib"] == 350


def test_short_missing_samples_and_failed_run_are_incomplete():
    for result in (run(end=7100), run(gap=True), run(runtime_ok=False)):
        verdict = n7rt_memory.analyze(result)
        assert verdict["classification"] == "incomplete"
        assert not verdict["complete"] and not verdict["all_memory_criteria_ok"]


def test_missing_or_nonfinite_process_memory_cannot_certify_stability():
    corruptions = (lambda s: s.update(per_process_private_mb={}),
                   lambda s: s["per_process_private_mb"].pop("app:1"),
                   lambda s: s["per_process_private_mb"].pop("webview:2") if s["t"] >= 3600 else None,
                   lambda s: s.update(per_process_private_mb={"app:1": float("nan"), "webview:2": float("nan")}),
                   lambda s: s["per_process_private_mb"].update({"unknown:3": 5}),
                   lambda s: s["per_process_private_mb"].update({"app:1": -1}))
    for corrupt in corruptions:
        result = run()
        for sample in result["harness"]["samples"]:
            corrupt(sample)
        verdict = n7rt_memory.analyze(result)
        assert verdict["classification"] == "incomplete"
        assert verdict["invalid_process_sample_count"] > 0


def test_aggregate_process_memory_must_match_the_tree_total():
    result = run()
    result["harness"]["samples"][10]["private_mb"] += 10
    verdict = n7rt_memory.analyze(result)
    assert not verdict["complete"] and verdict["invalid_process_sample_count"] == 1


def test_filtered_single_corrupt_sample_cannot_disappear_from_evidence():
    for field, value in (("private_mb", float("nan")), ("per_process_private_mb", None), ("t", float("nan"))):
        result = run()
        result["harness"]["samples"][10][field] = value
        verdict = n7rt_memory.analyze(result)
        assert not verdict["complete"] and verdict["invalid_raw_sample_count"] == 1


def test_missing_invalid_or_unverified_candidate_provenance_is_incomplete():
    for value in (None, "", "1" * 63, "z" * 64, 123):
        result = run()
        result["harness"]["exe_sha256"] = value
        verdict = n7rt_memory.analyze(result)
        assert not verdict["complete"] and not verdict["artifact_provenance_ok"]
    for key, value in (("candidate_exe_sha256", None), ("candidate_exe_sha256", "2" * 64),
                       ("hash_verified_before_launch", False), ("hash_verified_after_run", False),
                       ("hash_verified_before_launch", 1), ("hash_verified_after_run", None)):
        result = run()
        result["release_validation"][key] = value
        verdict = n7rt_memory.analyze(result)
        assert verdict["classification"] == "incomplete" and not verdict["all_memory_criteria_ok"]
        assert not verdict["artifact_provenance_ok"]
    result = run()
    result.pop("release_validation")
    assert not n7rt_memory.analyze(result)["complete"]


def test_missing_failed_or_undersized_representative_circuit_is_incomplete():
    result = run()
    result["steps"].pop(0)
    assert not n7rt_memory.analyze(result)["representative_load_ok"]
    for field, value in (("ok", False), ("checks", []), ("checks", [{"name": "gabarit", "ok": False}])):
        result = run()
        result["steps"][0][field] = value
        verdict = n7rt_memory.analyze(result)
        assert not verdict["complete"] and not verdict["representative_load_ok"]
    for field, value in (("components", 149), ("junctions", 49), ("wires", 299),
                         ("components", "150"), ("wires", float("nan")), ("junctions", True)):
        result = run()
        result["steps"][0]["detail"][field] = value
        verdict = n7rt_memory.analyze(result)
        assert verdict["classification"] == "incomplete" and not verdict["all_memory_criteria_ok"]
        assert not verdict["representative_load_ok"]


def test_boolean_and_noninteger_exit_status_cannot_certify_a_completed_run():
    for exit_code in (False, True, 0.0, "0", None, 2):
        result = run()
        result["harness"]["exit_code"] = exit_code
        verdict = n7rt_memory.analyze(result)
        assert not verdict["complete"] and verdict["classification"] == "incomplete"
    # Exit 1 is admissible when the independent RUN/load checks pass but a short perf budget fails.
    result = run()
    result["harness"]["exit_code"] = 1
    assert n7rt_memory.analyze(result)["complete"]


def test_churn_must_exercise_edits_and_restore_the_representative_document():
    result = run()
    result["steps"].pop(1)
    assert not n7rt_memory.analyze(result)["edit_churn_ok"]
    for field, value in (("cycles", 99), ("label_updates", 199), ("deleted_state_checks", 0),
                         ("engine_restarts", 1), ("engine_restarts", False),
                         ("retained_pot_channels_after", 2), ("retained_pot_channels_peak", 1),
                         ("retained_pot_channels_before", False),
                         ("topology_restored", False), ("topology_restored", 1)):
        result = run()
        result["steps"][1]["detail"][field] = value
        verdict = n7rt_memory.analyze(result)
        assert verdict["classification"] == "incomplete" and not verdict["edit_churn_ok"]
    result = run()
    result["steps"][1]["checks"][1]["ok"] = False
    assert not n7rt_memory.analyze(result)["complete"]
