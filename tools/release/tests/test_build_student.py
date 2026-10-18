"""Release builds retry memory failures without hiding ordinary compilation errors."""
from __future__ import annotations

import sys
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import Mock

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import build_student as B  # noqa: E402


def fake_commands(monkeypatch, outcomes):
    pending = iter(outcomes)

    def run(cmd, *, cwd, env, stdout, stderr):
        rc, output = next(pending)
        stdout.write(output)
        return SimpleNamespace(returncode=rc)

    command = Mock(side_effect=run)
    sleep = Mock()
    monkeypatch.setattr(B.subprocess, "run", command)
    monkeypatch.setattr(B.time, "sleep", sleep)
    return command, sleep


@pytest.mark.parametrize("message", [
    "failed to mmap rmeta metadata",
    "out of memory",
    "Allocation failed",
    "not enough memory",
])
def test_memory_failure_retries_and_preserves_diagnostics(tmp_path, monkeypatch, message):
    command, sleep = fake_commands(monkeypatch, [(1, message), (0, "Finished release")])

    B.step("cargo-app", ["cargo", "build"], tmp_path, {}, tmp_path, retries=4)

    assert command.call_count == 2
    sleep.assert_called_once_with(5)
    assert (tmp_path / "cargo-app.attempt-1.log").read_text() == message
    assert (tmp_path / "cargo-app.log").read_text() == "Finished release"


@pytest.mark.parametrize("message", [
    "error: Unexpected\nwindres failed to compile resource.rc",
    "can't find crate for `serde`",
    "error[E0308]: mismatched types",
])
def test_regular_compile_failure_stops_without_retry(tmp_path, monkeypatch, capsys, message):
    command, sleep = fake_commands(monkeypatch, [(1, message)])

    with pytest.raises(SystemExit, match="1"):
        B.step("cargo-app", ["cargo", "build"], tmp_path, {}, tmp_path, retries=4)

    assert command.call_count == 1
    sleep.assert_not_called()
    assert (tmp_path / "cargo-app.log").read_text() == message
    assert not list(tmp_path.glob("*.attempt-*.log"))
    assert message in capsys.readouterr().err


def test_exhausted_memory_retries_stop_and_keep_every_failure(tmp_path, monkeypatch):
    command, sleep = fake_commands(monkeypatch, [(1, "out of memory 1"), (1, "out of memory 2")])

    with pytest.raises(SystemExit, match="1"):
        B.step("cargo-app", ["cargo", "build"], tmp_path, {}, tmp_path, retries=2)

    assert command.call_count == 2
    sleep.assert_called_once_with(5)
    assert (tmp_path / "cargo-app.attempt-1.log").read_text() == "out of memory 1"
    assert (tmp_path / "cargo-app.log").read_text() == "out of memory 2"


def test_cli_defaults_to_one_cargo_job(monkeypatch):
    monkeypatch.setattr(sys, "argv", ["build_student.py", "--build-date", "2026-10-08"])

    class StopAfterArgs(Exception):
        pass

    build_env = Mock(side_effect=StopAfterArgs)
    monkeypatch.setattr(B, "build_env", build_env)

    with pytest.raises(StopAfterArgs):
        B.main()

    assert build_env.call_args.args[0] == 1


@pytest.mark.parametrize("jobs", ["0", "-1", "two"])
def test_cli_refuses_invalid_job_count(monkeypatch, capsys, jobs):
    monkeypatch.setattr(sys, "argv", ["build_student.py", "--jobs", jobs])

    with pytest.raises(SystemExit, match="2"):
        B.main()

    assert "--jobs must be a positive integer" in capsys.readouterr().err


def test_instructor_environment_defaults_to_one_job(monkeypatch):
    monkeypatch.setenv("CARGO_BUILD_JOBS", "8")
    monkeypatch.setattr(B, "which", lambda name, env: name)
    monkeypatch.setattr(B, "run_capture", lambda *args, **kw: "host: x86_64-unknown-linux-gnu")

    assert B.build_env(None, 0)["CARGO_BUILD_JOBS"] == "1"


@pytest.mark.parametrize("jobs", [0, -1])
def test_build_environment_refuses_invalid_job_count(jobs):
    with pytest.raises(SystemExit, match="1"):
        B.build_env(jobs, 0)


def test_dirty_candidate_manifest_fingerprints_uncommitted_bytes(tmp_path, monkeypatch):
    (tmp_path / ".git").mkdir()
    source = tmp_path / "source.py"
    source.write_bytes(b"first\n")
    monkeypatch.setattr(B, "ROOT", tmp_path)
    monkeypatch.setattr(B.shutil, "which", lambda name: "git")

    def git(cmd, **kwargs):
        if "rev-parse" in cmd:
            return SimpleNamespace(returncode=0, stdout="base-commit\n")
        if "status" in cmd:
            return SimpleNamespace(returncode=0, stdout=" M source.py\n")
        return SimpleNamespace(returncode=0, stdout=b"source.py\0")

    monkeypatch.setattr(B.subprocess, "run", git)
    first = B.source_commit(None)
    assert first["commit"] == "base-commit" and first["dirty"] is True
    assert first == B.source_commit(None)
    source.write_bytes(b"second\n")
    assert first["working_tree_sha256"] != B.source_commit(None)["working_tree_sha256"]
    source.unlink()
    assert B.source_commit(None)["working_tree_sha256"] != first["working_tree_sha256"]


def test_git_status_failure_cannot_claim_clean_source(tmp_path, monkeypatch):
    (tmp_path / ".git").mkdir()
    monkeypatch.setattr(B, "ROOT", tmp_path)
    monkeypatch.setattr(B.shutil, "which", lambda name: "git")
    outcomes = iter([SimpleNamespace(returncode=0, stdout="base\n"),
                     SimpleNamespace(returncode=1, stdout="")])
    monkeypatch.setattr(B.subprocess, "run", lambda *args, **kwargs: next(outcomes))
    with pytest.raises(SystemExit, match="1"):
        B.source_commit(None)


def test_build_receipt_rejects_stale_inputs_and_binary(tmp_path, monkeypatch):
    app = tmp_path / "app"
    app.mkdir()
    source = app / "main.rs"
    source.write_bytes(b"compiled input")
    exe = app / "target/release/app.exe"
    exe.parent.mkdir(parents=True)
    exe.write_bytes(b"compiled binary")
    monkeypatch.setattr(B, "APP", app)
    monkeypatch.setattr(B, "ROOT", tmp_path)
    for name in B.COMPILED_RESOURCES:
        path = tmp_path / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(b"external compiled input")
    inputs = B.application_inputs()
    B.build_receipt(exe, inputs, "2026-10-08", {"rustc": "test"})
    B.verify_build_receipt(exe, "2026-10-08", {"rustc": "test"})
    external = tmp_path / B.COMPILED_RESOURCES[0]
    external.write_bytes(b"changed external input")
    with pytest.raises(SystemExit, match="1"):
        B.verify_build_receipt(exe, "2026-10-08", {"rustc": "test"})
    external.write_bytes(b"external compiled input")
    (app / "README.md").write_text("documentation-only change")
    B.verify_build_receipt(exe, "2026-10-08", {"rustc": "test"})
    source.write_bytes(b"changed input")
    with pytest.raises(SystemExit, match="1"):
        B.verify_build_receipt(exe, "2026-10-08", {"rustc": "test"})
    source.write_bytes(b"compiled input")
    exe.write_bytes(b"different binary")
    with pytest.raises(SystemExit, match="1"):
        B.verify_build_receipt(exe, "2026-10-08", {"rustc": "test"})


def test_build_receipt_missing_or_changed_during_build_blocks_packaging(tmp_path, monkeypatch):
    monkeypatch.setattr(B, "APP", tmp_path)
    monkeypatch.setattr(B, "COMPILED_RESOURCES", ())
    exe = tmp_path / "app.exe"
    exe.write_bytes(b"binary")
    with pytest.raises(SystemExit, match="1"):
        B.verify_build_receipt(exe, "2026-10-08", {})
    with pytest.raises(SystemExit, match="1"):
        B.build_receipt(exe, "stale-inputs", "2026-10-08", {})


def test_student_build_attests_its_actual_target_not_an_ambient_target(monkeypatch):
    monkeypatch.setattr(sys, "argv", ["build_student.py", "--build-date", "2026-10-08"])
    env = {"CARGO_TARGET_DIR": "other-target"}
    monkeypatch.setattr(B, "build_env", lambda *args: env)

    class StopAfterEnvironment(Exception):
        pass

    monkeypatch.setattr(B, "toolchain", Mock(side_effect=StopAfterEnvironment))
    with pytest.raises(StopAfterEnvironment):
        B.main()
    assert env["CARGO_TARGET_DIR"] == str(B.APP / "target")
