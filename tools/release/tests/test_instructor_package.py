"""The instructor ZIP uses the public SDK/editor allowlist and works after independent extraction."""
from __future__ import annotations

import importlib.util
import json
import shutil
import subprocess
import sys
import zipfile
from pathlib import Path

import pytest

from test_student_package import B, SOURCE_ROOT, resources  # noqa: F401; shared fixture

# This suite belongs to the combined instructor source tree. Public exports intentionally
# omit restricted/: never discover or load a neighbouring instructor repository implicitly.
instructor_builder = SOURCE_ROOT / "restricted/tools/build_instructor.py"
if not instructor_builder.is_file():
    pytest.skip("Instructor archive tests require the combined private source tree.", allow_module_level=True)

spec = importlib.util.spec_from_file_location("instructor_package", instructor_builder)
I = importlib.util.module_from_spec(spec)
spec.loader.exec_module(I)


@pytest.fixture
def executables(resources):
    root, exe = resources
    exe.write_bytes(b"instructor app fixture " + B.ORACLE_MARK)
    tool = exe.parent / "n7rt-tool.exe"
    tool.write_bytes(b"instructor judge fixture " + B.ORACLE_MARK)
    return root, exe, tool


def test_instructor_resources_match_public_archive_allowlist(executables):
    _, exe, tool = executables
    public = B.student_sources(exe)
    files = I.instructor_files(B, exe, tool)
    assert files.keys() == public.keys() | {tool.name}
    assert all(files[name] == path.read_bytes() for name, path in public.items())
    assert "N7RT_CANLab_Student/n7rt/lib/n7rt_runtime.o" in files
    assert "runtime-release.json" in files and "tools/stm32/student_ide.py" in files
    assert "requirements-student-ide.txt" in files and "CODEMIRROR-LICENSES.txt" in files
    assert "docs/inapp-student-ide-tutorial.md" in files
    assert "docs/img/inapp-student-ide/01-project.png" in files
    assert not any(name.startswith(("restricted/", "oracle/", "missions/", "cohorts/", "instructor/", "library-private/"))
                   or name.endswith((".pdf", ".n7rtpack", ".n7rtrun", "extra.c", "python.exe", "old.elf"))
                   for name in files)


@pytest.mark.parametrize("filename", ["n7rt-can-lab.exe", "n7rt-tool.exe"])
def test_public_executable_cannot_enter_instructor_archive(executables, filename, capsys):
    _, exe, tool = executables
    (exe.parent / filename).write_bytes(b"public binary fixture")
    with pytest.raises(SystemExit, match="1"):
        I.instructor_files(B, exe, tool)
    assert "without the oracle" in capsys.readouterr().err


@pytest.mark.parametrize("relative", ["tools/stm32/student_ide.py", "firmware/n7rt_student/runtime-release.json"])
def test_missing_editor_resource_blocks_instructor_archive(executables, relative, capsys):
    root, exe, tool = executables
    (root / relative).unlink()
    with pytest.raises(SystemExit, match="1"):
        I.instructor_files(B, exe, tool)
    assert Path(relative).name in capsys.readouterr().err


def test_extracted_instructor_archive_compiles_without_student_checkout(executables, tmp_path):
    # Real GCC; no executable launch, serial connection, or flash.
    sys.path.insert(0, str(SOURCE_ROOT / "tools/stm32"))
    import stm32lab
    if not stm32lab.find_tool("gcc"):
        pytest.skip("STM32 GCC missing")
    _, exe, tool = executables
    files = I.instructor_files(B, exe, tool)
    files["manifest.json"] = B.manifest("instructor-tools", "0.2.0", "2026-10-08", None, {}, files)
    zipped = tmp_path / "instructor.zip"
    B.write_zip(zipped, files)
    extracted = tmp_path / "independent-instructor/tools/bin"
    with zipfile.ZipFile(zipped) as release:
        release.extractall(extracted)
        manifest = json.loads(release.read("manifest.json"))
        assert {row["path"] for row in manifest["files"]} == set(release.namelist()) - {"manifest.json"}
    workspace = tmp_path / "student-profile"
    workspace.mkdir()
    for name in ("student_decoder.c", "student_decoder.h"):
        shutil.copyfile(extracted / "N7RT_CANLab_Student/student" / name, workspace / name)
    unrelated_cwd = tmp_path / "unrelated-cwd"
    unrelated_cwd.mkdir()
    result = subprocess.run([sys.executable, "-B", "-X", "utf8",
                             str(extracted / "tools/stm32/student_ide.py"), "build", "--workspace", str(workspace)],
                            cwd=unrelated_cwd, capture_output=True, text=True, encoding="utf-8", timeout=120)
    report = json.loads(result.stdout)
    assert result.returncode == 0 and report["ok"], report
    assert report["artifact"]["student_crc32"] == 0x5CBC80B3
    assert report["artifact"]["flash_bytes"] == 260
    assert (workspace / "build/student.bin").is_file()
