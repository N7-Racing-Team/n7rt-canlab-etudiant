"""The runnable archive contains the public editor resources, never private trees or a Python runtime."""
from __future__ import annotations

import json
import shutil
import subprocess
import sys
import zipfile
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import build_student as B  # noqa: E402
import export_student as E  # noqa: E402

SOURCE_ROOT = B.ROOT
SOURCE_SDK = B.SDK


def test_every_image_a_shipped_document_shows_is_shipped_too():
    """A document of the archive never points to a missing figure (the tutorial's figures are generated into
    docs/img/tutoriel-0.3 by tools/docs/tutorial_figures.py, from a run on a board)."""
    import re
    shipped = set(B.tree(SOURCE_ROOT / "docs/img", "docs/img", B.STUDENT_IMAGE_PATTERNS))
    for name in B.STUDENT_DOCS:
        if not name.endswith(".md"):
            continue
        for link in re.findall(r"!\[[^\]]*\]\(([^)\s]+)\)", (SOURCE_ROOT / "docs" / name).read_text(encoding="utf-8")):
            if "://" not in link:
                assert f"docs/{link}" in shipped, f"{name}: {link} is not in the archive"


@pytest.fixture
def resources(tmp_path, monkeypatch):
    root = tmp_path / "source"
    sdk = root / "firmware/n7rt_student/N7RT_CANLab_Student"
    templates = root / "tools/release/templates"
    required = [*("docs/" + name for name in B.STUDENT_DOCS),
                *("tools/stm32/" + name for name in B.STUDENT_HELPERS),
                "tools/stm32/requirements-student-ide.txt", "firmware/n7rt_student/runtime-release.json",
                "app/CODEMIRROR-LICENSES.txt",
                "tools/release/templates/LISEZMOI.txt", "DEMARRER_ICI.md", "DEMARRER_ICI.html"]
    for relative in required:
        target = root / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(SOURCE_ROOT / relative, target)
    for relative in B.SDK_RELEASE_FILES:
        target = sdk / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(SOURCE_SDK / relative, target)
    fixtures = {
        "docs/img/inapp-student-ide/01-project.png": b"public image fixture",
        "docs/img/inapp-student-ide/.vitest/retained.png": b"temporary test image",
        "docs/img/unreviewed/secret.png": b"image outside reviewed folders",
        "docs/img/private-note.txt": b"not an image",
        "library/README.md": b"publisher links only",
        "library/links.json": b"[]",
        "library/apps/manifest.json": b"{}",
        "library/.vite/stale/manifest.json": b"{}",
        "library/apps/local-only.pdf": b"excluded PDF",
        "library-private/secret.pdf": b"private library",
        "restricted/missions/apps/pack-v0.json": b"private pack",
        "instructor/master.key": b"private key",
        "cohorts/test/apps/groups.json": b"private group codes",
        "tools/stm32/python.exe": b"unreviewed interpreter",
        "shared/missions/apps/enonce.json": b"{}",
        "shared/missions/apps/solution.c": b"private solution",
        "firmware/n7rt_student/N7RT_CANLab_Student/Debug/old.elf": b"old build",
        "firmware/n7rt_student/N7RT_CANLab_Student/student/extra.c": b"unreviewed source",
    }
    for relative, data in fixtures.items():
        target = root / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)
    exe = root / "app/target/release/n7rt-can-lab.exe"
    exe.parent.mkdir(parents=True)
    exe.write_bytes(b"public executable fixture")
    monkeypatch.setattr(B, "ROOT", root)
    monkeypatch.setattr(B, "SDK", sdk)
    monkeypatch.setattr(B, "TEMPLATES", templates)
    return root, exe


def archive(path, files):
    entries = {name: source.read_bytes() for name, source in files.items()}
    entries["manifest.json"] = B.manifest("student", "0.2.0", "2026-10-08", None, {}, entries)
    return B.write_zip(path, entries)


def test_archive_allowlist_includes_editor_and_omits_private_or_extra_files(resources):
    root, exe = resources
    files = B.student_sources(exe)
    assert "runtime-release.json" in files
    assert "requirements-student-ide.txt" in files
    assert "docs/inapp-student-ide-r1.md" in files
    assert "docs/inapp-student-ide-tutorial.md" in files
    assert "docs/img/inapp-student-ide/01-project.png" in files
    assert "CODEMIRROR-LICENSES.txt" in files
    assert {"tools/stm32/" + name for name in B.STUDENT_HELPERS} <= files.keys()
    assert "N7RT_CANLab_Student/n7rt/lib/n7rt_runtime.o" in files
    assert not any(name.startswith(("restricted/", "instructor/", "cohorts/", "library-private/"))
                   for name in files)
    # the student tutorial is the only PDF of the archive: the library's documents (links only) never enter it
    assert "docs/N7RT_CANLab_0.3_Tutoriel_Etudiant.pdf" in files
    assert not any(name.endswith(".pdf") and name != "docs/N7RT_CANLab_0.3_Tutoriel_Etudiant.pdf" for name in files)
    assert not any(name.endswith((".n7rtpack", ".n7rtrun", "extra.c", "python.exe", "old.elf")) for name in files)
    assert "docs/tutoriel-etudiant-0.3.md" not in files and not any(n.startswith("docs/img/tutoriel-0.3/") for n in files)
    assert "docs/img/private-note.txt" not in files
    assert "docs/img/unreviewed/secret.png" not in files
    assert not any("/.vitest/" in name or "/.vite/" in name for name in files)
    assert not any(name.endswith(("python.dll", "python312.dll")) for name in files)


def test_missing_required_helper_blocks_packaging(resources, capsys):
    root, exe = resources
    (root / "tools/stm32/student_ide.py").unlink()
    with pytest.raises(SystemExit, match="1"):
        B.student_sources(exe)
    assert "student_ide.py" in capsys.readouterr().err


def test_archive_is_deterministic_and_every_entry_is_manifested(resources, tmp_path):
    _, exe = resources
    files = B.student_sources(exe)
    first, second = tmp_path / "one.zip", tmp_path / "two.zip"
    assert archive(first, files) == archive(second, files)
    assert first.read_bytes() == second.read_bytes()
    with zipfile.ZipFile(first) as zipped:
        assert zipped.testzip() is None
        manifest = json.loads(zipped.read("manifest.json"))
        assert {row["path"] for row in manifest["files"]} == set(zipped.namelist()) - {"manifest.json"}
        for row in manifest["files"]:
            data = zipped.read(row["path"])
            assert row["sha256"] == B.hashlib.sha256(data).hexdigest()
            assert row["size"] == len(data)
        assert all(info.date_time == B.FIXED_TIME for info in zipped.infolist())


def test_source_export_lists_editor_helpers_and_two_public_docs():
    selected, missing = E.select(SOURCE_ROOT)
    for name in ("tools/stm32/student_ide.py", "tools/stm32/requirements-student-ide.txt",
                 "docs/inapp-student-ide-r1.md", "docs/inapp-student-ide-tutorial.md", "app/CODEMIRROR-LICENSES.txt"):
        assert name in selected and name not in missing
        assert E.forbidden_name(name) is None


def test_source_export_preserves_required_release_documents_and_memory_evaluator():
    selected, missing = E.select(SOURCE_ROOT)
    for relative in ("tools/stm32/n7rt_memory.py", "docs/inapp-student-ide-board-c-verification-20261008.json",
                     "docs/release-0.2-closure-r1.md"):
        assert relative in selected and relative not in missing
        assert E.forbidden_name(relative) is None
    assert {"docs/" + name for name in B.STUDENT_DOCS} <= set(selected)


@pytest.mark.parametrize("relative", ["app/ui/.vite/vitest/fixture/results.json", "app/ui/.vitest/results.json",
                                     "app/ui/.cache/test.json", "app/ui/coverage/index.html",
                                     "app/ui/preview/fixtures.ts", "app/ui/preview/shoot.mjs",
                                     "tools/release/script.py.tmp", "docs/notes.md.bak"])
def test_source_export_excludes_test_caches_and_temporary_files(tmp_path, relative):
    target = tmp_path / relative
    target.parent.mkdir(parents=True)
    target.write_text("temporary fixture")
    selected, _ = E.select(tmp_path)
    assert E.excluded(relative)
    assert relative not in selected


def test_windows_checkout_uses_required_script_endings_and_preserves_pinned_json(tmp_path):
    if not shutil.which("git"):
        pytest.skip("git missing")
    source = tmp_path / "source"
    source.mkdir()
    shutil.copyfile(SOURCE_ROOT / ".gitattributes", source / ".gitattributes")
    fixtures = {"helper.py": b"print('fixture')\n", "build.sh": b"#!/bin/sh\nexit 0\n",
                "helper.ps1": b"Write-Output 'fixture'\n", "helper.cmd": b"@echo off\nexit /b 0\n",
                "manifest.json": b'{"fixture":"byte-pinned"}\r\n'}
    for name, data in fixtures.items():
        (source / name).write_bytes(data)
    def git(*args):
        return subprocess.run(["git", "-C", str(source), *args], capture_output=True, check=True)
    git("init", "--quiet")
    git("config", "core.autocrlf", "true")
    git("add", ".gitattributes", *fixtures)
    extracted = tmp_path / "checkout"
    extracted.mkdir()
    git("checkout-index", "--all", "--prefix=" + str(extracted).replace("\\", "/") + "/")
    for name in ("helper.py", "build.sh", "helper.ps1"):
        assert (extracted / name).read_bytes() == fixtures[name]
    assert (extracted / "helper.cmd").read_bytes() == fixtures["helper.cmd"].replace(b"\n", b"\r\n")
    assert (extracted / "manifest.json").read_bytes() == fixtures["manifest.json"]


def test_packaged_archive_compiles_student_code_after_extraction(resources, tmp_path):
    # Real GCC; source packaging and extraction only. No executable launch or hardware.
    sys.path.insert(0, str(SOURCE_ROOT / "tools/stm32"))
    import stm32lab
    if not stm32lab.find_tool("gcc"):
        pytest.skip("STM32 GCC missing")
    _, exe = resources
    zip_path = tmp_path / "release.zip"
    archive(zip_path, B.student_sources(exe))
    extracted = tmp_path / "outside-repository"
    with zipfile.ZipFile(zip_path) as zipped:
        zipped.extractall(extracted)
    workspace = tmp_path / "student-profile"
    workspace.mkdir()
    for name in ("student_decoder.c", "student_decoder.h"):
        shutil.copyfile(extracted / "N7RT_CANLab_Student/student" / name, workspace / name)
    done = subprocess.run([sys.executable, "-B", "-X", "utf8",
                           str(extracted / "tools/stm32/student_ide.py"), "build", "--workspace", str(workspace)],
                          cwd=extracted, capture_output=True, text=True, encoding="utf-8", timeout=120)
    report = json.loads(done.stdout)
    assert done.returncode == 0 and report["ok"], report
    assert report["artifact"]["student_crc32"] == 0x5CBC80B3
    assert report["artifact"]["flash_bytes"] == 260
