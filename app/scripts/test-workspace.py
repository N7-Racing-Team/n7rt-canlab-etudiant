#!/usr/bin/env python3
"""Run the real Cargo test binaries, including Tauri unit tests on Windows.

Tauri's default resource links only application binaries. A unit-test executable
that imports TaskDialogIndirect also needs the Common Controls v6 manifest.
This runner embeds Tauri's own manifest in those generated test artifacts only;
it changes no production binary, registry value, global setting or test logic.
"""
from __future__ import annotations

import argparse
import ctypes
import datetime as dt
import importlib.util
import json
import os
import re
import subprocess
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
APP = ROOT / "app"


def student_build():
    path = ROOT / "tools" / "release" / "build_student.py"
    spec = importlib.util.spec_from_file_location("build_student", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def oracle_directory(explicit: Path | None) -> Path:
    choices = [explicit] if explicit else [
        ROOT / "restricted" / "oracle" / "n7rt-vehicle",
        ROOT.parent / "n7rt-canlab-missions" / "oracle" / "n7rt-vehicle",
    ]
    for path in choices:
        if path is not None and (path / "judge.rs").is_file():
            return path.resolve()
    raise RuntimeError("Oracle unavailable: use --oracle-dir with the instructor repository's oracle directory.")


def tauri_manifest(env: dict) -> bytes:
    lock = tomllib.loads((APP / "Cargo.lock").read_text(encoding="utf-8"))
    version = next(p["version"] for p in lock["package"] if p["name"] == "tauri-build")
    cargo_home = Path(env.get("CARGO_HOME") or Path.home() / ".cargo")
    files = sorted((cargo_home / "registry" / "src").glob(
        f"*/tauri-build-{version}/src/windows-app-manifest.xml"))
    if not files:
        raise RuntimeError("Tauri's standard test manifest is unavailable in the Cargo source cache.")
    return files[0].read_bytes()


def embed_test_manifest(executable: Path, manifest: bytes) -> bool:
    if os.name != "nt":
        return False
    image = executable.read_bytes()
    if b"TaskDialogIndirect\0" not in image or b"Microsoft.Windows.Common-Controls" in image:
        return False
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    begin = kernel.BeginUpdateResourceW
    begin.argtypes = [ctypes.c_wchar_p, ctypes.c_int]
    begin.restype = ctypes.c_void_p
    update = kernel.UpdateResourceW
    update.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p,
                       ctypes.c_ushort, ctypes.c_void_p, ctypes.c_ulong]
    update.restype = ctypes.c_int
    end = kernel.EndUpdateResourceW
    end.argtypes = [ctypes.c_void_p, ctypes.c_int]
    end.restype = ctypes.c_int
    handle = begin(str(executable), False)
    if not handle:
        raise ctypes.WinError(ctypes.get_last_error())
    data = ctypes.create_string_buffer(manifest)
    # RT_MANIFEST = 24, CREATEPROCESS_MANIFEST_RESOURCE_ID = 1, English (US).
    if not update(handle, 24, 1, 1033, data, len(manifest)):
        error = ctypes.get_last_error()
        end(handle, True)
        raise ctypes.WinError(error)
    if not end(handle, False):
        raise ctypes.WinError(ctypes.get_last_error())
    return True


def compile_tests(cargo: str, env: dict, packages: list[str]) -> list[Path]:
    command = [cargo, "test", "--locked", "--workspace", "--no-run", "--message-format=json"]
    if packages:
        command.remove("--workspace")
        for package in packages:
            command += ["--package", package]
    artifacts = set()
    with subprocess.Popen(command, cwd=APP, env=env, stdout=subprocess.PIPE,
                          text=True, encoding="utf-8", errors="replace") as process:
        for line in process.stdout:
            try:
                event = json.loads(line)
            except json.JSONDecodeError:
                print(line, end="", flush=True)
                continue
            if event.get("reason") == "compiler-artifact" and event.get("profile", {}).get("test") and event.get("executable"):
                artifacts.add(Path(event["executable"]).resolve())
            elif event.get("reason") == "compiler-message":
                rendered = event.get("message", {}).get("rendered")
                if rendered:
                    print(rendered, end="", file=sys.stderr, flush=True)
        if process.wait() != 0:
            raise RuntimeError("Cargo did not finish compiling the test workspace.")
    return sorted(artifacts)


def main() -> int:
    build = student_build()
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--oracle", action="store_true", help="private oracle tests (instructor sources required)")
    mode.add_argument("--public", action="store_true", help="public student tests (default)")
    parser.add_argument("--oracle-dir", type=Path)
    parser.add_argument("--jobs", type=build.positive_jobs, default=1)
    parser.add_argument("--build-date", default="2026-10-08", help="fixed environment date YYYY-MM-DD")
    parser.add_argument("--package", action="append", default=[], help="only this package (repeatable)")
    parser.add_argument("--report", type=Path)
    parser.add_argument("--timeout", type=int, default=600,
                        help="seconds allowed to each test binary (default 600; the slowest oracle walks need more)")
    parser.add_argument("test_args", nargs=argparse.REMAINDER, help="arguments after -- go to every test binary")
    args = parser.parse_args()
    try:
        date = dt.date.fromisoformat(args.build_date)
        epoch = int(dt.datetime(date.year, date.month, date.day, tzinfo=dt.timezone.utc).timestamp())
        oracle = oracle_directory(args.oracle_dir) if args.oracle else None
        env = build.build_env(args.jobs, epoch, oracle_dir=oracle)
        artifacts = compile_tests(build.which("cargo", env), env, args.package)
        if not artifacts:
            raise RuntimeError("Cargo produced no test executables.")
        manifest = tauri_manifest(env) if os.name == "nt" else b""
        test_args = args.test_args[1:] if args.test_args[:1] == ["--"] else args.test_args
        if not any(value.startswith("--test-threads") for value in test_args):
            test_args += ["--test-threads=1"]
        result = {"mode": "oracle" if args.oracle else "public", "jobs": args.jobs,
                  "suites": [], "passed": 0, "failed": 0, "ignored": 0}
        failed = False
        for executable in artifacts:
            patched = embed_test_manifest(executable, manifest)
            test_env = env.copy()
            # Cargo normally adds the deps/profile paths while running an executable.
            test_env["PATH"] = os.pathsep.join([str(executable.parent), str(executable.parent.parent), env.get("PATH", "")])
            print(f"== {executable.name}" + (" (standard Tauri manifest applied)" if patched else ""), flush=True)
            done = subprocess.run([str(executable), *test_args], cwd=APP, env=test_env,
                                  capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=args.timeout)
            text = done.stdout + done.stderr
            print(text, end="", flush=True)
            counts = re.search(r"test result: .*? (\d+) passed; (\d+) failed; (\d+) ignored", text)
            suite = {"binary": executable.name, "exit_code": done.returncode,
                     "manifest_applied": patched}
            if counts:
                for key, number in zip(("passed", "failed", "ignored"), counts.groups()):
                    suite[key] = int(number)
                    result[key] += int(number)
            result["suites"].append(suite)
            failed |= done.returncode != 0
        result["ok"] = not failed
        report = args.report or APP / "target" / f"test-workspace-{result['mode']}.json"
        report.parent.mkdir(parents=True, exist_ok=True)
        report.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
        print(f"{len(artifacts)} suites: {result['passed']} passed, {result['failed']} failed, {result['ignored']} ignored. Report: {report}")
        return 1 if failed else 0
    except (OSError, RuntimeError, ValueError, subprocess.TimeoutExpired) as error:
        print(f"ERROR: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
