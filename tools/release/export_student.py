#!/usr/bin/env python3
"""Migrate the combined workspace to a new unversioned student tree (N7-Racing-Team/n7rt-canlab).

    python tools/release/export_student.py <target> [--instructor DIR ...] [--dry-run] [--list]

Allowlist copy (docs/distribution.md §1 and §5): only the paths listed below leave the workspace, minus build
outputs and machine-local files. A target in or containing a Git repository is refused, even with --force.
The target is emptied first, except its build caches
(app/target, node_modules, app/src-tauri/gen: gitignored, never copied), then the templates
of tools/release/templates/ are written at its root (README.md, .gitignore, .gitattributes, pyproject.toml).

Refused (exit 1, the target is not touched): any file under a `restricted` directory, any *.n7rtpack or
*.n7rtrun, any file named pack-*.json, bench-*.json, golden*, solution*, master.key or auth.json, and any file whose SHA-256 equals a file of the instructor
tree (the instructor-side paths of the workspace, plus every --instructor DIR given).
After publication, the versioned student and instructor repositories are the canonical sources.
This migration script must not overwrite them from the historical combined workspace.
"""
from __future__ import annotations

import argparse
import fnmatch
import hashlib
import json
import re
import os
import shutil
import stat
import sys
from pathlib import Path

SOURCE = Path(__file__).resolve().parents[2]
TEMPLATES = Path(__file__).resolve().parent / "templates"

# Single files (paths relative to the workspace root).
FILES = [
    "app/Cargo.toml", "app/Cargo.lock", "app/README.md", "app/package.json", "app/package-lock.json",
    "app/tsconfig.json", "app/vite.config.ts", "app/CODEMIRROR-LICENSES.txt",
    "firmware/n7rt_student/.gitignore", "firmware/n7rt_student/runtime-release.json",
    "library/README.md", "library/links.json",
    "tools/library/check_library.py",
    "tools/stm32/stm32lab.py", "tools/stm32/stm32lab.cmd", "tools/stm32/n7rt_sdk.py", "tools/stm32/n7rt_app.py",
    "tools/stm32/flash_decoder.py", "tools/stm32/boards.json",
    "tools/stm32/student_ide.py", "tools/stm32/n7rt_memory.py", "tools/stm32/requirements-student-ide.txt",
    "tools/n7rt_link/n7rt_link.py", "tools/n7rt_link/protocol_docs.py", "tools/n7rt_link/runtime_check.py",
    "tools/n7rt_link/selftest_model.py",
    "tools/n7rt_link/tests/test_n7rt_link.py", "tools/n7rt_link/tests/test_runtime_check.py",
    # Public documentation (each one reviewed: no mission value, no instructor procedure).
    "docs/can-lab-architecture.md", "docs/distribution.md", "docs/n7rt-can-lab-app-v0.md", "docs/n7rt-link-v0.md",
    "docs/n7rt-runtime-v0.md", "docs/n7rt-student-sdk-v0.md", "docs/physical-lab-v1.md",
    "docs/physical-lab-v1-validation.md",
    "docs/inapp-student-ide-r1.md", "docs/inapp-student-ide-tutorial.md",
    "docs/inapp-student-ide-board-c-verification-20261008.json", "docs/release-0.2-closure-r1.md",
    "docs/missions-four-r1.md", "docs/missions-four-manual-acceptance.md",
    "docs/release-notes-0.2-four-missions.md",
    "docs/N7RT_CANLab_0.3_Tutoriel_Etudiant.pdf", "docs/README_CANLab_0.3.md", "docs/demarrage-5-minutes.md",
    "docs/known-issues-0.3.md",
]

# Directory trees: (root, file patterns relative to the root; None = every file).
TREES = [
    ("app/crates", None), ("app/scripts", None), ("app/src-tauri", None), ("app/tools/zig-shims", None),
    ("app/ui", None),
    ("firmware/n7rt_student/N7RT_CANLab_Student", None),
    # links-only distribution (V-M03, docs/distribution.md §1): manifests, never the document PDFs
    ("library", ["*/manifest.json"]),
    ("shared/schemas", None), ("shared/protocols", None), ("shared/constants", None), ("shared/examples", None),
    ("shared/missions", ["*/enonce.json"]),
    ("tools/stm32/tests", ["*.py"]),
    ("tools/docs", ["*.py"]),
    ("tools/docs/tests", ["*.py"]),
    ("tools/release", None),
    ("docs/img", [f"{folder}/*.{extension}"
                  for folder in ("can-lab-ui", "physical-lab", "inapp-student-ide", "tutoriel-0.3-pdf")
                  for extension in ("png", "svg", "jpg")] + ["tutoriel-0.3-pdf/manifest.json"]),
    # sources of the student tutorial PDF (Typst, figure specs, build configuration)
    ("docs/pdf", ["*.typ", "tutoriel-etudiant*.json", "figures-etudiant*.json"]),
]

# Never copied, wherever they are inside an allowed tree.
EXCLUDED_DIR_NAMES = {"target", "node_modules", "dist", "build", "Debug", "Release", "__pycache__", ".pytest_cache",
                      ".mypy_cache", ".ruff_cache", ".git", ".vs", ".idea", ".arduino-build", ".vitest", ".vite",
                      ".cache", "coverage", ".nyc_output", ".tmp", ".temp"}
# app/ui/preview: the design-review harness (fake core, invented fixtures, screenshot script), dev only.
EXCLUDED_DIRS = {"app/src-tauri/gen", "app/ui/preview", "firmware/n7rt_student/N7RT_CANLab_Student/.settings"}
EXCLUDED_FILES = ["*.pyc", "*.pyo", "*.log", "hs_err*", "*.orig", "*.rej", "*.tmp", "*.temp", "*.bak", "*~", "*.swp",
                  ".DS_Store", "Thumbs.db", "desktop.ini", "*.local.json", "*.local.env"]

# Root files written from the templates: (template, target path).
ROOT_TEMPLATES = [("README.md", "README.md"), ("gitignore", ".gitignore"), ("gitattributes", ".gitattributes"),
                  ("pyproject.toml", "pyproject.toml")]

# Instructor-side paths of the workspace: their file hashes must never reach the student tree.
INSTRUCTOR_ROOTS = ["restricted", "firmware/stm32_h753_bringup", "firmware/stm32_node_a", "firmware/stm32_node_b",
                    "firmware/arduino_helper_a", "firmware/arduino_helper_b", "tools/n7rt_link/c_runtime.py",
                    "tools/n7rt_link/tests/test_c_runtime.py", "docs/apps-mission-slice.md",
                    "docs/hardware-wiring.md"]
INSTRUCTOR_SKIP_DIRS = {"build", "third_party", "Board_A", "Board_B", ".git", "__pycache__", ".arduino-build"}
TRIVIAL = 8  # files whose stripped content is shorter than this (empty, .gitkeep) carry no secret


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def excluded(rel: str) -> bool:
    parts = rel.split("/")
    if any(p in EXCLUDED_DIR_NAMES for p in parts[:-1]):
        return True
    if any("/".join(parts[:i]) in EXCLUDED_DIRS for i in range(1, len(parts))):
        return True
    return any(fnmatch.fnmatch(parts[-1], pat) for pat in EXCLUDED_FILES)


def select(source: Path) -> tuple[list[str], list[str]]:
    """Allowlisted files (sorted relative POSIX paths) and missing allowlist entries."""
    chosen: set[str] = set()
    missing: list[str] = []
    for rel in FILES:
        if (source / rel).is_file():
            chosen.add(rel)
        else:
            missing.append(rel)
    for root, patterns in TREES:
        base = source / root
        if not base.is_dir():
            missing.append(root + "/")
            continue
        for dirpath, dirnames, filenames in os.walk(base):
            dirnames[:] = sorted(d for d in dirnames if d not in EXCLUDED_DIR_NAMES)
            for name in filenames:
                path = Path(dirpath) / name
                rel = path.relative_to(source).as_posix()
                inner = path.relative_to(base).as_posix()
                if excluded(rel):
                    continue
                if patterns is None or any(fnmatch.fnmatch(inner, p) for p in patterns):
                    chosen.add(rel)
    return sorted(chosen), missing


def forbidden_name(rel: str) -> str | None:
    """Why a path may never be in the student tree, or None."""
    parts = rel.split("/")
    low = [p.lower() for p in parts]
    name = low[-1]
    if "restricted" in low[:-1]:
        return "under a restricted directory"
    if name.endswith((".n7rtpack", ".n7rtrun")):
        return "group pack or run file"
    if name in ("master.key", "auth.json"):
        return "instructor secret file"
    if fnmatch.fnmatch(name, "pack-*.json") or fnmatch.fnmatch(name, "bench-*.json"):
        return "mission pack or bench file name"
    if any(p.startswith(("golden", "solution")) for p in low):
        return "golden vectors or solution name"
    return None


def fixed_transport_fixture(value) -> bool:
    """Recognise fixed input/encoded-answer pairs, independent of file name or metadata."""
    if isinstance(value, dict):
        hex_value = lambda key: isinstance(value.get(key), str) and bool(
            re.fullmatch(r"(?:[0-9a-fA-F]{2})*", value[key]))
        if hex_value("wire_hex") and isinstance(value.get("fields", value.get("expect")), (dict, list)):
            return True
        if hex_value("decoded_hex") and hex_value("encoded_hex"):
            return True
        if hex_value("input_hex") and (hex_value("crc32") or hex_value("encoded_hex")):
            return True
        return any(fixed_transport_fixture(child) for child in value.values())
    if isinstance(value, list):
        return any(fixed_transport_fixture(child) for child in value)
    return False


def forbidden_content(path: Path) -> str | None:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (UnicodeError, json.JSONDecodeError):
        return None
    return "fixed transport input/answer fixture" if fixed_transport_fixture(value) else None


def instructor_hashes(source: Path, extra: list[Path]) -> dict[str, str]:
    """SHA-256 -> instructor path, for every non-trivial file of the instructor tree."""
    roots = [source / r for r in INSTRUCTOR_ROOTS] + extra
    out: dict[str, str] = {}

    def add(path: Path) -> None:
        data = path.read_bytes()
        if len(data.strip()) >= TRIVIAL:
            out.setdefault(hashlib.sha256(data).hexdigest(), str(path))

    for root in roots:
        if root.is_file():
            add(root)
            continue
        for dirpath, dirnames, filenames in os.walk(root):
            dirnames[:] = [d for d in dirnames if d not in INSTRUCTOR_SKIP_DIRS]
            for name in filenames:
                add(Path(dirpath) / name)
    return out


def combined_workspace(source: Path) -> str | None:
    """None when `source` is the maintainer's combined workspace (app/ and restricted/ side by side), else why not.
    The exported repositories hold a copy of this script: run there, it must refuse instead of wiping its target."""
    if (source / "app" / "Cargo.toml").is_file() and (source / "restricted" / "tools" / "export_instructor.py").is_file():
        return None
    return (f"{source} is not the combined workspace (app/ and restricted/ side by side): the repositories are exports, "
            "this script only runs from the maintainer's workspace")


def git_repository(path: Path) -> bool:
    marker = path / ".git"
    return (marker.exists() or marker.is_symlink()
            or ((path / "HEAD").is_file() and (path / "config").is_file() and (path / "objects").is_dir()))


def safe_target(target: Path, source: Path, force: bool) -> str | None:
    t, s = target.resolve(), source.resolve()
    if t == s or s in t.parents or t in s.parents:
        return "the target must be outside the workspace (and must not contain it)"
    if any(git_repository(p) for p in (t, *t.parents)):
        return "the target must be outside every Git repository; --force cannot overwrite canonical sources"
    if t.exists() and not t.is_dir():
        return "the target must be a directory"
    if t.is_dir() and any(git_repository(Path(p)) for p, _, _ in os.walk(t)):
        return "the target contains a Git repository; --force cannot delete canonical sources"
    if not t.exists() or not any(t.iterdir()) or force:
        return None
    if (t / "tools" / "release" / "export_student.py").is_file():
        return None
    return "the target is neither empty nor a previous unversioned export (use --force for a non-Git target)"


def _writable(func, path, _exc) -> None:
    os.chmod(path, stat.S_IWRITE)  # read-only files (node_modules, git objects copied by tools) on Windows
    func(path)


# Kept in the target across migrations: build caches (gitignored, never copied from the workspace), so that
# a re-export does not cost a full rebuild. Everything else is deleted.
KEPT = ["dist", "app/target", "app/node_modules", "app/ui/node_modules", "app/src-tauri/gen"]


def _remove(entry: Path) -> None:
    if entry.is_dir() and not entry.is_symlink():
        if sys.version_info >= (3, 12):
            shutil.rmtree(entry, onexc=_writable)
        else:
            shutil.rmtree(entry, onerror=_writable)
    else:
        os.chmod(entry, stat.S_IWRITE)
        entry.unlink()


def clear(target: Path, rel: str = "") -> None:
    for entry in target.iterdir():
        r = rel + entry.name
        if r in KEPT:
            continue
        if entry.is_dir() and not entry.is_symlink() and any(k.startswith(r + "/") for k in KEPT):
            clear(entry, r + "/")
        else:
            _remove(entry)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("target", type=Path)
    ap.add_argument("--source", type=Path, default=SOURCE, help="workspace root (default: this script's repo)")
    ap.add_argument("--instructor", type=Path, action="append", default=[],
                    help="extra instructor tree whose file hashes are refused (e.g. the instructor export)")
    ap.add_argument("--dry-run", action="store_true", help="check only, write nothing")
    ap.add_argument("--list", action="store_true", help="print every exported path")
    ap.add_argument("--force", action="store_true", help="accept a non-empty unversioned target; never a Git tree")
    args = ap.parse_args()
    source = args.source.resolve()

    files, missing = select(source)
    for m in missing:
        print(f"note: allowlist entry absent from the workspace: {m}")

    problems = [f"{rel}: {why}" for rel in files if (why := forbidden_name(rel))]
    problems.extend(f"{rel}: {why}" for rel in files
                    if (why := forbidden_content(source / rel)))
    for d in args.instructor:
        if not d.is_dir():
            print(f"error: --instructor {d}: not a directory", file=sys.stderr)
            return 2
    private = instructor_hashes(source, [d.resolve() for d in args.instructor])
    for rel in files:
        other = private.get(sha256(source / rel))
        if other:
            problems.append(f"{rel}: same SHA-256 as instructor file {other}")
    for tpl, _ in ROOT_TEMPLATES:
        if not (TEMPLATES / tpl).is_file():
            problems.append(f"template missing: {TEMPLATES / tpl}")
    if problems:
        print("EXPORT REFUSED (nothing written):", file=sys.stderr)
        for p in problems:
            print(f"  {p}", file=sys.stderr)
        return 1

    total = sum((source / rel).stat().st_size for rel in files)
    print(f"student export: {len(files)} files, {total / 1e6:.1f} MB, "
          f"{len(private)} instructor hashes checked")
    if args.list:
        for rel in files:
            print(f"  {rel}")
    if args.dry_run:
        print("dry run: target not written")
        return 0

    target = args.target
    why = combined_workspace(source) or safe_target(target, source, args.force)
    if why:
        print(f"error: {target}: {why}", file=sys.stderr)
        return 2
    target.mkdir(parents=True, exist_ok=True)
    clear(target)
    for rel in files:
        dst = target / rel
        dst.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source / rel, dst)
    for tpl, rel in ROOT_TEMPLATES:
        shutil.copyfile(TEMPLATES / tpl, target / rel)
    print(f"written to {target.resolve()}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
