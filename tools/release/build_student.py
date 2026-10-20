#!/usr/bin/env python3
"""Reproducible student release of N7RT CAN Lab (docs/distribution.md §5), run from the student repository.

    python tools/release/build_student.py --build-date 2026-10-06 [--jobs 1] [--package-only] [--commit SHA]

1. Checks the toolchain (Python, Node, npm, rustc, cargo) and records its versions.
2. `npm ci` then `npm run build` in app/ (the front end embedded in the executable).
3. `cargo build --release --locked -p n7rt-can-lab --features tauri/custom-protocol` WITHOUT the oracle
   (N7RT_ORACLE_DIR forced empty: public edition, docs/distribution.md §1), with --remap-path-prefix for the
   repository (.) and the cargo home (cargo), symbols stripped by the release profile, and on a windows-gnu host the
   GNU toolchain settings of app/scripts/windows-gnu-env.ps1. The executable is refused if it carries the marker of
   an instructor build.
4. One deterministic archive in dist/ (sorted entries, 1980-01-01 timestamps, fixed permissions, manifest.json with
   the SHA-256 of every entry, the source commit when git is available, the tool versions and the build date given
   as an argument, never read from the clock): n7rt-canlab-<version>-windows-x64.zip = the application, the
   public SDK project N7RT_CANLab_Student/, runtime pin, student editor helpers,
   library/, shared/missions/, public docs/captures, LISEZMOI.txt. Python is a prerequisite,
   never bundled; requirements-student-ide.txt lists the helper's Python dependency.
Same sources + same toolchain + same date => same bytes. Audit the archive from the instructor repository before
publishing (tools/audit_student.py). The instructor tools (n7rt-tool and the instructor edition of the application,
both with the oracle) are built from the instructor repository (tools/build_instructor.py), which reuses the
functions of this script.
"""
from __future__ import annotations

import argparse
import datetime as dt
import fnmatch
import hashlib
import json
import os
import platform
import re
import shutil
import subprocess
import sys
import time
import zipfile
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
APP = ROOT / "app"
TEMPLATES = Path(__file__).resolve().parent / "templates"
SDK = ROOT / "firmware" / "n7rt_student" / "N7RT_CANLab_Student"
FIXED_TIME = (1980, 1, 1, 0, 0, 0)
FILE_MODE = 0o100644
TEMP_DIRS = {".vitest", ".vite", ".cache", "coverage", ".nyc_output", ".tmp", ".temp", "__pycache__"}
TEMP_FILES = ("*.tmp", "*.temp", "*.orig", "*.rej", "*.bak", "*.swp", "*~")
COMPILED_RESOURCES = (
    "firmware/n7rt_student/runtime-release.json",
    "firmware/n7rt_student/N7RT_CANLab_Student/n7rt/lib/n7rt_runtime.o",
    "firmware/n7rt_student/N7RT_CANLab_Student/student/student_decoder.c",
    "firmware/n7rt_student/N7RT_CANLab_Student/student/student_decoder.h",
    "shared/missions/apps/enonce.json",
    "shared/missions/wsf/enonce.json",
    "shared/missions/sas/enonce.json",
    "shared/missions/bps/enonce.json",
)
SDK_EXCLUDED = {"Debug", "Release", "build", ".settings"}
SDK_RELEASE_FILES = (
    ".project", ".cproject", ".gitignore", "N7RT_CANLab_Student.launch", "LISEZMOI_ETUDIANT.txt",
    "student/student_decoder.c", "student/student_decoder.h", "n7rt/manifest.json", "n7rt/LISEZMOI.txt",
    "n7rt/include/n7rt.h", "n7rt/lib/n7rt_runtime.o", "n7rt/lib/libn7rt_glue.a", "n7rt/ld/N7RT_H753_student.ld",
)
STUDENT_HELPERS = ("student_ide.py", "n7rt_sdk.py", "stm32lab.py", "flash_decoder.py", "boards.json")
STUDENT_DOCS = (
    "release-0.2-closure-r1.md",
    "missions-four-r1.md", "missions-four-manual-acceptance.md",
    "release-notes-0.2-four-missions.md",
    "inapp-student-ide-r1.md", "inapp-student-ide-tutorial.md", "n7rt-student-sdk-v0.md", "distribution.md",
    "inapp-student-ide-board-c-verification-20261008.json",
    "n7rt-can-lab-app-v0.md", "n7rt-link-v0.md", "n7rt-runtime-v0.md", "can-lab-architecture.md",
    "physical-lab-v1.md", "physical-lab-v1-validation.md",
    # the student tutorial is the PDF (docs/pdf/, tools/docs/build_pdf.py); the documentation index lists the five
    # documents of the release (the four instructor guides are only in the instructor archive)
    "N7RT_CANLab_0.3_Tutoriel_Etudiant.pdf", "README_CANLab_0.3.md", "demarrage-5-minutes.md", "known-issues-0.3.md",
    # installation and diagnostic of the build tools, at the path the Code STM32 messages give
    "stm32-toolchain-windows.md", "stm32-toolchain-windows.html",
)
STUDENT_IMAGE_PATTERNS = [f"{folder}/*.{extension}"
                          for folder in ("can-lab-ui", "physical-lab", "inapp-student-ide")
                          for extension in ("png", "svg", "jpg")]
# Text that only an instructor build (with the oracle) carries: n7rt_vehicle::EDITION.
ORACLE_MARK = b"N7RT-ORACLE-BUILD"
OOM = re.compile(r"0xc0000409|memory allocation|STATUS_STACK_BUFFER_OVERRUN|paging file|os error 1455|"
                 r"only metadata stub found|failed to mmap|out of memory|allocation failed|not enough memory", re.I)


def fail(msg: str) -> None:
    print(f"ERROR: {msg}", file=sys.stderr)
    sys.exit(1)


def run_capture(cmd: list[str], **kw) -> str:
    r = subprocess.run(cmd, capture_output=True, text=True, **kw)
    if r.returncode != 0:
        fail(f"{' '.join(cmd)} failed: {(r.stderr or r.stdout).strip()[:400]}")
    return r.stdout.strip()


def which(name: str, env: dict) -> str:
    path = shutil.which(name, path=env.get("PATH"))
    if not path:
        fail(f"{name} not found on PATH")
    return path


def positive_jobs(value: str) -> int:
    try:
        jobs = int(value)
    except ValueError:
        raise argparse.ArgumentTypeError("--jobs must be a positive integer") from None
    if jobs < 1:
        raise argparse.ArgumentTypeError("--jobs must be a positive integer")
    return jobs


def toolchain(env: dict) -> dict:
    if sys.version_info < (3, 11):
        fail("Python 3.11 or newer required")
    rustc = run_capture([which("rustc", env), "-vV"], env=env)
    return {
        "python": platform.python_version(),
        "zlib": zlib.ZLIB_RUNTIME_VERSION,
        "rustc": rustc.splitlines()[0],
        "host": re.search(r"^host: (\S+)", rustc, re.M).group(1),
        "cargo": run_capture([which("cargo", env), "--version"], env=env),
        "node": run_capture([which("node", env), "--version"], env=env),
        "npm": run_capture([which("npm", env), "--version"], env=env),
    }


def build_env(jobs: int | None, epoch: int, oracle_dir: Path | None = None,
              remaps: tuple[tuple[Path, str], ...] = ()) -> dict:
    """Build environment: toolchain settings, path remapping (this repository -> ".", the cargo home -> "cargo", plus
    `remaps`), fixed date, and the oracle switch: compiled in only with `oracle_dir` (instructor builds)."""
    jobs = 1 if jobs is None else jobs
    if jobs < 1:
        fail("--jobs must be a positive integer")
    env = os.environ.copy()
    cargo_home = Path(env.get("CARGO_HOME") or Path.home() / ".cargo")
    if (cargo_home / "bin").is_dir():
        env["PATH"] = os.pathsep.join([str(cargo_home / "bin"), env.get("PATH", "")])
    host = re.search(r"^host: (\S+)", run_capture([which("rustc", env), "-vV"], env=env), re.M).group(1)
    flags: list[str] = []
    # GNU host without MSVC: same settings as app/scripts/windows-gnu-env.ps1.
    if os.name == "nt" and host.endswith("windows-gnu"):
        tc = Path(env.get("LOCALAPPDATA", "")) / "n7rt-toolchain"
        for p in (tc / "bin" / "windres.exe", tc / "bin" / "dlltool.exe", tc / "lld" / "ld.exe"):
            if not p.is_file():
                print(f"warning: absent: {p} (see app/README.md, Windows sans MSVC)")
        env["PATH"] = os.pathsep.join([str(tc / "bin"), env["PATH"]])
        flags += ["-C", f"link-arg=-B{tc.as_posix()}/lld/"]
    elif env.get("RUSTFLAGS"):
        flags += env["RUSTFLAGS"].split()
    flags += [f"--remap-path-prefix={ROOT}=.", f"--remap-path-prefix={cargo_home}=cargo"]
    flags += [f"--remap-path-prefix={src}={dst}" for src, dst in remaps]
    env.pop("RUSTFLAGS", None)
    env["CARGO_ENCODED_RUSTFLAGS"] = "\x1f".join(flags)
    env["CARGO_INCREMENTAL"] = "0"
    env["SOURCE_DATE_EPOCH"] = str(epoch)
    env["CARGO_BUILD_JOBS"] = str(jobs)
    # empty = the public stand-ins (an empty value is never replaced by a cargo [env] default)
    env["N7RT_ORACLE_DIR"] = str(oracle_dir) if oracle_dir else ""
    return env


def step(name: str, cmd: list[str], cwd: Path, env: dict, logs: Path, retries: int = 1) -> None:
    logs.mkdir(parents=True, exist_ok=True)
    log = logs / f"{name}.log"
    for attempt in range(1, retries + 1):
        print(f"== {name}: {' '.join(cmd)}" + (f" (attempt {attempt})" if attempt > 1 else ""), flush=True)
        with log.open("w", encoding="utf-8", errors="replace") as f:
            rc = subprocess.run(cmd, cwd=cwd, env=env, stdout=f, stderr=subprocess.STDOUT).returncode
        if rc == 0:
            return
        text = log.read_text(encoding="utf-8", errors="replace")
        if attempt < retries and OOM.search(text):
            attempt_log = logs / f"{name}.attempt-{attempt}.log"
            shutil.copyfile(log, attempt_log)
            print(f"   out of memory, retrying in 5 seconds (log {attempt_log})", flush=True)
            time.sleep(5)
            continue
        tail = "\n".join(text.splitlines()[-25:])
        fail(f"{name} failed (exit {rc}), log {log}\n{tail}")


def versions() -> str:
    tauri = json.loads((APP / "src-tauri" / "tauri.conf.json").read_text(encoding="utf-8"))["version"]
    npm = json.loads((APP / "package.json").read_text(encoding="utf-8"))["version"]
    cargo = re.search(r'(?ms)^\[workspace\.package\].*?^version\s*=\s*"([^"]+)"',
                      (APP / "Cargo.toml").read_text(encoding="utf-8"))
    found = {"tauri.conf.json": tauri, "package.json": npm, "Cargo.toml": cargo.group(1) if cargo else None}
    if len(set(found.values())) != 1:
        fail(f"inconsistent versions: {found}")
    return tauri


def source_commit(explicit: str | None) -> dict | None:
    if explicit:
        # An asserted commit is not evidence of a clean checkout.
        return {"commit": explicit, "dirty": None}
    git = shutil.which("git")
    if not git or not (ROOT / ".git").exists():
        return None
    head = subprocess.run([git, "-C", str(ROOT), "rev-parse", "HEAD"], capture_output=True, text=True)
    if head.returncode != 0:
        return None
    dirty = subprocess.run([git, "-C", str(ROOT), "status", "--porcelain"], capture_output=True, text=True)
    if dirty.returncode != 0:
        fail("cannot determine whether the candidate source tree is clean")
    source = {"commit": head.stdout.strip(), "dirty": bool(dirty.stdout.strip())}
    if source["dirty"]:
        paths = subprocess.run([git, "-C", str(ROOT), "ls-files", "-z", "--cached", "--others",
                                "--exclude-standard"], capture_output=True)
        if paths.returncode != 0:
            fail("cannot fingerprint the candidate source tree")
        digest = hashlib.sha256()
        for name in sorted(set(paths.stdout.split(b"\0")) - {b""}):
            path = ROOT / os.fsdecode(name)
            digest.update(name + b"\0")
            digest.update(hashlib.sha256(path.read_bytes()).digest() if path.is_file() else b"DELETED")
        source["working_tree_sha256"] = digest.hexdigest()
    return source


def application_inputs(oracle_dir: Path | None = None) -> str:
    """Fingerprint compilation inputs, excluding generated output and documentation."""
    digest = hashlib.sha256()
    roots = [(APP, "app")]
    if oracle_dir:
        roots.append((oracle_dir, "oracle"))
    excluded = TEMP_DIRS | {"target", "node_modules", "dist", ".git", "gen", "__pycache__"}
    files = {}
    for name in COMPILED_RESOURCES:
        path = ROOT / name
        if not path.is_file():
            fail(f"compiled resource missing: {name}")
        files[name] = path
    for root, prefix in roots:
        for directory, dirs, names in os.walk(root):
            dirs[:] = [d for d in dirs if d.lower() not in excluded]
            for name in names:
                if name.endswith(".md") or any(fnmatch.fnmatch(name.lower(), p) for p in TEMP_FILES):
                    continue
                path = Path(directory) / name
                files[f"{prefix}/{path.relative_to(root).as_posix()}"] = path
    for name in sorted(files):
        digest.update(name.encode("utf-8") + b"\0" + hashlib.sha256(files[name].read_bytes()).digest())
    return digest.hexdigest()


def build_receipt(exe: Path, inputs: str, date: str, tools: dict, *, oracle_dir: Path | None = None) -> None:
    if application_inputs(oracle_dir) != inputs:
        fail("compilation inputs changed during the build; rebuild the candidate")
    receipt = {"format": "n7rt-build-receipt", "version": 1, "application_inputs_sha256": inputs,
               "executable_sha256": hashlib.sha256(exe.read_bytes()).hexdigest(),
               "build_date": date, "toolchain": tools, "oracle": oracle_dir is not None}
    exe.with_suffix(".build.json").write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def verify_build_receipt(exe: Path, date: str, tools: dict, *, oracle_dir: Path | None = None) -> None:
    try:
        receipt = json.loads(exe.with_suffix(".build.json").read_text(encoding="utf-8"))
    except (OSError, ValueError):
        fail("missing or invalid build receipt; compile the candidate before packaging")
    expected = {"format": "n7rt-build-receipt", "version": 1,
                "application_inputs_sha256": application_inputs(oracle_dir),
                "executable_sha256": hashlib.sha256(exe.read_bytes()).hexdigest(),
                "build_date": date, "toolchain": tools, "oracle": oracle_dir is not None}
    if receipt != expected:
        fail("build receipt differs from current inputs, executable, edition, date or toolchain; rebuild")


def tree(base: Path, prefix: str, patterns: list[str] | None = None, skip: set[str] = frozenset()) -> dict:
    out = {}
    for dirpath, dirnames, filenames in os.walk(base):
        dirnames[:] = [d for d in dirnames if d not in skip and d.lower() not in TEMP_DIRS]
        for name in filenames:
            if any(fnmatch.fnmatch(name.lower(), pat) for pat in TEMP_FILES):
                continue
            p = Path(dirpath) / name
            inner = p.relative_to(base).as_posix()
            if patterns is None or any(fnmatch.fnmatch(inner, pat) for pat in patterns):
                out[f"{prefix}/{inner}"] = p
    return out


def write_zip(path: Path, entries: dict[str, bytes]) -> str:
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_suffix(".tmp")
    with zipfile.ZipFile(tmp, "w") as zf:
        for name in sorted(entries):
            zi = zipfile.ZipInfo(name, date_time=FIXED_TIME)
            zi.compress_type = zipfile.ZIP_DEFLATED
            zi.create_system = 3
            zi.external_attr = FILE_MODE << 16
            zf.writestr(zi, entries[name], compresslevel=9)
    os.replace(tmp, path)
    return hashlib.sha256(path.read_bytes()).hexdigest()


def manifest(kind: str, version: str, date: str, commit, tools: dict, files: dict[str, bytes]) -> bytes:
    doc = {
        "format": "n7rt-release-manifest", "version": 1, "product": "n7rt-canlab", "kind": kind,
        "app_version": version, "build_date": date, "source": commit, "toolchain": tools,
        "files": [{"path": n, "size": len(files[n]), "sha256": hashlib.sha256(files[n]).hexdigest()}
                  for n in sorted(files)],
    }
    return (json.dumps(doc, indent=2, sort_keys=True, ensure_ascii=True) + "\n").encode("utf-8")


def student_sources(exe: Path) -> dict[str, Path]:
    """Only reviewed public resources enter the runnable archive; no Python runtime or private tree."""
    sources = {exe.name: exe}
    loader = exe.parent / "WebView2Loader.dll"
    if loader.is_file():
        sources[loader.name] = loader
    sources.update({f"{SDK.name}/{name}": SDK / name for name in SDK_RELEASE_FILES})
    sources["runtime-release.json"] = ROOT / "firmware/n7rt_student/runtime-release.json"
    sources.update({f"tools/stm32/{name}": ROOT / "tools/stm32" / name for name in STUDENT_HELPERS})
    sources["requirements-student-ide.txt"] = ROOT / "tools/stm32/requirements-student-ide.txt"
    sources["CODEMIRROR-LICENSES.txt"] = ROOT / "app/CODEMIRROR-LICENSES.txt"
    sources.update({f"docs/{name}": ROOT / "docs" / name for name in STUDENT_DOCS})
    sources.update(tree(ROOT / "docs/img", "docs/img", STUDENT_IMAGE_PATTERNS))
    sources.update(tree(ROOT / "library", "library", ["README.md", "links.json", "*/manifest.json"]))
    sources.update(tree(ROOT / "shared/missions", "shared/missions", ["*/enonce.json"]))
    sources["LISEZMOI.txt"] = TEMPLATES / "LISEZMOI.txt"
    # the student guide, and its offline HTML copy (tools/docs/build_guide_html.py) opened by a double-click
    sources["DEMARRER_ICI.md"] = ROOT / "DEMARRER_ICI.md"
    sources["DEMARRER_ICI.html"] = ROOT / "DEMARRER_ICI.html"
    missing = [name for name, path in sources.items() if not path.is_file()]
    if missing:
        fail("missing student archive resources: " + ", ".join(sorted(missing)))
    return sources


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--build-date", help="release date YYYY-MM-DD (default: $SOURCE_DATE_EPOCH)")
    ap.add_argument("--jobs", type=positive_jobs, default=1, help="cargo jobs (default: 1, limits memory use)")
    ap.add_argument("--package-only", action="store_true", help="skip npm and cargo, package the existing build")
    ap.add_argument("--commit", help="source commit to record (default: git rev-parse HEAD when available)")
    ap.add_argument("--out", type=Path, default=ROOT / "dist")
    args = ap.parse_args()

    if args.build_date:
        try:
            day = dt.date.fromisoformat(args.build_date)
        except ValueError:
            fail("--build-date must be YYYY-MM-DD")
        epoch = int(dt.datetime(day.year, day.month, day.day, tzinfo=dt.timezone.utc).timestamp())
    elif os.environ.get("SOURCE_DATE_EPOCH", "").isdigit():
        epoch = int(os.environ["SOURCE_DATE_EPOCH"])
        day = dt.datetime.fromtimestamp(epoch, dt.timezone.utc).date()
    else:
        fail("give --build-date YYYY-MM-DD (or SOURCE_DATE_EPOCH): the clock is never used")
    date = day.isoformat()

    env = build_env(args.jobs, epoch)
    # The output we attest and package must be the output this build actually produces.
    env["CARGO_TARGET_DIR"] = str(APP / "target")
    tools = toolchain(env)
    version = versions()
    print(f"N7RT CAN Lab {version}, build date {date}, {tools['rustc']} ({tools['host']}), node {tools['node']}")
    if "windows" not in tools["host"]:
        print("warning: the release archives are named windows-x64; this host is " + tools["host"])

    logs = args.out / "logs"
    inputs = application_inputs()
    if not args.package_only:
        npm = which("npm", env)
        cargo = which("cargo", env)
        step("npm-ci", [npm, "ci", "--no-audit", "--no-fund"], APP, env, logs)
        step("npm-build", [npm, "run", "build"], APP, env, logs)
        step("cargo-app", [cargo, "build", "--release", "--locked", "-p", "n7rt-can-lab",
                           "--features", "tauri/custom-protocol"], APP, env, logs, retries=4)

    rel = APP / "target" / "release"
    exe_suffix = ".exe" if os.name == "nt" else ""
    exe = rel / f"n7rt-can-lab{exe_suffix}"
    if not exe.is_file():
        fail(f"missing build output {exe}")
    if ORACLE_MARK in exe.read_bytes():
        fail(f"{exe} is an instructor build (it contains the oracle): rebuild without N7RT_ORACLE_DIR")
    if not args.package_only:
        build_receipt(exe, inputs, date, tools)
    verify_build_receipt(exe, date, tools)
    if not (TEMPLATES / "LISEZMOI.txt").is_file():
        fail(f"missing {TEMPLATES / 'LISEZMOI.txt'}")
    commit = source_commit(args.commit)

    sources = student_sources(exe)
    student = {name: p.read_bytes() for name, p in sources.items()}
    student["manifest.json"] = manifest("student", version, date, commit, tools, student)

    name = f"n7rt-canlab-{version}-windows-x64.zip"
    digest = write_zip(args.out / name, student)
    (args.out / f"{name}.sha256").write_text(f"{digest}  {name}\n", encoding="ascii")
    size = (args.out / name).stat().st_size
    print(f"executable  {exe.name}  sha256 {hashlib.sha256(student[exe.name]).hexdigest()}  (public edition)")
    print(f"archive     {args.out / name}  {len(student)} entries, {size / 1e6:.1f} MB, sha256 {digest}")
    print("Next: audit the archive from the instructor repository before publishing "
          "(python tools/audit_student.py <student repo> <archive>).")
    return 0


if __name__ == "__main__":
    sys.exit(main())
