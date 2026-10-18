"""Memory-soak analysis of application + WebView2 private bytes (MiB, 2**20 bytes).

The absolute budget and permitted drift are kept separate: a stable footprint can exceed its budget.
One-minute medians and ten-minute endpoint medians suppress GC/cache oscillations without hiding peaks.
No UI access or private mission material is needed by this analysis.
"""
from __future__ import annotations

import math
import re
from statistics import median


def _finite(value) -> bool:
    return isinstance(value, (int, float)) and not isinstance(value, bool) and math.isfinite(value)


def _sha256(value) -> bool:
    return isinstance(value, str) and re.fullmatch(r"[0-9a-fA-F]{64}", value) is not None


def _visible_window(value) -> bool:
    return bool(isinstance(value, dict) and value.get("found") is True and value.get("visible_ok") is True
                and value.get("is_iconic") is False and value.get("is_visible") is True
                and value.get("restore_requested") is False
                and type(value.get("window_pid")) is int and value["window_pid"] > 0
                and type(value.get("hwnd")) is int and value["hwnd"] > 0
                and isinstance(value.get("rect"), list) and len(value["rect"]) == 4
                and all(_finite(x) for x in value["rect"]) and min(value["rect"][2:]) > 0
                and isinstance(value.get("client"), list) and len(value["client"]) == 2
                and all(_finite(x) and x > 0 for x in value["client"]))


def _trend(rows: list[tuple[float, float]], start: float, warmup_s: float) -> dict:
    bins: dict[int, list[tuple[float, float]]] = {}
    for t, mb in rows:
        if t >= start + warmup_s:
            bins.setdefault(int((t - start - warmup_s) // 60), []).append((t, mb))
    points = [(median(t for t, _ in group), median(mb for _, mb in group)) for group in bins.values()]
    points.sort()
    if len(points) < 3:
        return {"minute_bins": len(points), "slope_mib_per_hour": None, "endpoint_mib_per_hour": None}
    mt = sum(t for t, _ in points) / len(points)
    mm = sum(mb for _, mb in points) / len(points)
    denom = sum((t - mt) ** 2 for t, _ in points)
    slope = sum((t - mt) * (mb - mm) for t, mb in points) / denom * 3600
    first = points[:10]
    last = points[-10:]
    begin = median(mb for _, mb in first)
    end = median(mb for _, mb in last)
    dt = median(t for t, _ in last) - median(t for t, _ in first)
    return {"minute_bins": len(points), "slope_mib_per_hour": round(slope, 3),
            "endpoint_first_mib": round(begin, 2), "endpoint_last_mib": round(end, 2),
            "endpoint_growth_mib": round(end - begin, 2),
            "endpoint_mib_per_hour": round((end - begin) / dt * 3600, 3) if dt > 0 else None}


def analyze(result: dict, *, requested_seconds: int = 7200, warmup_seconds: int = 600,
            budget_mib: float = 300, drift_mib_per_hour: float = 20) -> dict:
    """Judge a completed, continuously sampled RUN soak, not idle time or restarted short runs."""
    harness = result.get("harness") or {}
    soak = result.get("soak") or {}
    steps = result.get("steps") or []
    events = harness.get("events") or []
    start = next((e.get("t") for e in events if e.get("step") == "run-soak"), None)
    step = next((s for s in steps if s.get("name") == "run-soak"), {})
    raw = harness.get("samples") or []
    roles = harness.get("process_roles") or {}
    provenance = result.get("release_validation") or {}
    exe_hash = harness.get("exe_sha256")
    candidate_hash = provenance.get("candidate_exe_sha256")
    artifact_provenance_ok = bool(_sha256(exe_hash) and _sha256(candidate_hash)
                                  and exe_hash.lower() == candidate_hash.lower()
                                  and provenance.get("hash_verified_before_launch") is True
                                  and provenance.get("hash_verified_after_run") is True)
    load = next((s for s in steps if s.get("name") == "chargement"), {})
    circuit = load.get("detail") or {}
    representative_load_ok = bool(load.get("ok") is True
                                  and any(c.get("name") == "gabarit" and c.get("ok") is True
                                          for c in load.get("checks", []))
                                  and all(_finite(circuit.get(key)) and circuit[key] >= minimum
                                          for key, minimum in (("components", 150), ("junctions", 50), ("wires", 300))))
    churn = next((s for s in steps if s.get("name") == "edition-churn"), {})
    churn_detail = churn.get("detail") or {}
    churn_checks = {c.get("name") for c in churn.get("checks", []) if c.get("ok") is True}
    edit_churn_ok = bool(churn.get("ok") is True
                         and {"churn complet", "topologie et états originaux restaurés",
                              "aucun redémarrage moteur pendant churn", "channels temporaires libérés"} <= churn_checks
                         and all(type(churn_detail.get(key)) is int and churn_detail[key] >= minimum
                                 for key, minimum in (("cycles", 100), ("created", 100), ("label_updates", 200),
                                                      ("pot_interactions", 100), ("deleted", 100),
                                                      ("deleted_state_checks", 100), ("original_components", 200),
                                                      ("original_wires", 300)))
                         and churn_detail.get("topology_restored") is True
                         and type(churn_detail.get("engine_restarts")) is int and churn_detail["engine_restarts"] == 0
                         and type(churn_detail.get("retained_pot_channels_before")) is int
                         and churn_detail["retained_pot_channels_before"] >= 0
                         and type(churn_detail.get("retained_pot_channels_after")) is int
                         and churn_detail["retained_pot_channels_after"] == churn_detail["retained_pot_channels_before"]
                         and type(churn_detail.get("retained_pot_channels_peak")) is int
                         and churn_detail["retained_pot_channels_peak"] == churn_detail["retained_pot_channels_before"] + 1)
    window = [s for s in raw if isinstance(s, dict) and _finite(s.get("t"))
              and _finite(start) and start <= s["t"] <= start + requested_seconds + 3]
    rows = [s for s in window if _finite(s.get("private_mb"))
            and s["private_mb"] > 0 and isinstance(s.get("per_process_private_mb"), dict)]
    invalid_raw_samples = len(window) - len(rows) + sum(
        not isinstance(s, dict) or not _finite(s.get("t")) for s in raw)
    rows.sort(key=lambda s: s["t"])
    invalid_process_samples = 0
    for sample in rows:
        per = sample["per_process_private_mb"]
        valid_values = bool(per and all(_finite(mb) and mb > 0 and proc in roles for proc, mb in per.items()))
        active_roles = {roles[proc] for proc, mb in per.items() if proc in roles and _finite(mb) and mb > 0}
        sum_ok = valid_values and abs(sum(per.values()) - sample["private_mb"]) <= len(per) * 0.1 + 0.1
        if not (sum_ok and "app" in active_roles and "renderer" in active_roles):
            invalid_process_samples += 1
    gaps = [b["t"] - a["t"] for a, b in zip(rows, rows[1:])]
    observed = rows[-1]["t"] - rows[0]["t"] if rows else 0
    elapsed = soak.get("elapsed_seconds")
    exit_code = harness.get("exit_code")
    role_names = set(roles.values())
    run_checks = {c.get("name") for c in step.get("checks", []) if c.get("ok") is True}
    journal_integrity_ok = "journal sans perte" in run_checks and soak.get("journal_incomplete") is False
    visibility_rows = [sample for sample in window if sample["t"] <= start + requested_seconds]
    invalid_window_samples = sum(not _visible_window(sample.get("window_visibility")) for sample in visibility_rows)
    complete = bool(_finite(start) and _finite(elapsed) and elapsed >= requested_seconds
                    and observed >= requested_seconds - 3 and gaps and max(gaps) <= 10
                    and step.get("ok") is True and type(exit_code) is int and exit_code in (0, 1)
                    and invalid_raw_samples == 0 and invalid_process_samples == 0
                    and artifact_provenance_ok and representative_load_ok and edit_churn_ok and journal_integrity_ok
                    and invalid_window_samples == 0
                    and "app" in role_names and "renderer" in role_names)
    tree_rows = [(s["t"], s["private_mb"]) for s in rows]
    trend = _trend(tree_rows, start, warmup_seconds) if rows else {}
    slope = trend.get("slope_mib_per_hour")
    endpoint = trend.get("endpoint_mib_per_hour")
    drift_ok = bool(complete and _finite(slope) and _finite(endpoint)
                    and slope <= drift_mib_per_hour and endpoint <= drift_mib_per_hour)
    peak = max((s["private_mb"] for s in rows), default=None)
    budget_ok = bool(complete and peak is not None and peak <= budget_mib)
    by_role: dict[str, dict] = {}
    for role in sorted(role_names):
        points = [(s["t"], sum(mb for proc, mb in s["per_process_private_mb"].items()
                                 if roles.get(proc) == role and _finite(mb))) for s in rows]
        if points:
            by_role[role] = {"peak_mib": max(mb for _, mb in points),
                             "first_mib": points[0][1], "last_mib": points[-1][1],
                             **_trend(points, start, warmup_seconds)}
    return {"format": "n7rt-memory-soak-analysis", "version": 1, "units": "MiB = 1048576 bytes",
            "scenario": "phys-perf sustained RUN, 150 components / 300 wires, actuator gestures, visible production journal",
            "requested_seconds": requested_seconds, "observed_sample_seconds": round(observed, 2),
            "sample_count": len(rows), "max_sample_gap_seconds": max(gaps, default=None),
            "invalid_process_sample_count": invalid_process_samples,
            "invalid_raw_sample_count": invalid_raw_samples,
            "invalid_window_sample_count": invalid_window_samples,
            "warmup_seconds_excluded_from_drift": warmup_seconds, "complete": complete,
            "artifact_provenance_ok": artifact_provenance_ok,
            "representative_load_ok": representative_load_ok,
            "edit_churn_ok": edit_churn_ok,
            "journal_integrity_ok": journal_integrity_ok,
            "loaded_circuit": {key: circuit.get(key) for key in ("components", "junctions", "wires")},
            "classification": "stable" if drift_ok else "fuite restante / dérive excessive" if complete else "incomplete",
            "classification_scope": f"Memory growth within {drift_mib_per_hour:.3f} MiB/h over this sustained run; absolute budget judged separately",
            "budget_mib": budget_mib, "drift_limit_mib_per_hour": drift_mib_per_hour,
            "peak_private_mib": peak, "absolute_budget_ok": budget_ok, "drift_ok": drift_ok,
            "tree_trend": trend, "per_role": by_role,
            "exe_sha256": harness.get("exe_sha256"), "application_exit_code": harness.get("exit_code"),
            "runtime_checks_ok": step.get("ok") is True,
            "all_memory_criteria_ok": complete and drift_ok and budget_ok}
