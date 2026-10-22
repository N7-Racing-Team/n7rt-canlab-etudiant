"""Student editor toolchain and upload guards. Hardware is always replaced by mocks."""
from __future__ import annotations

import json
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import student_ide as ide  # noqa: E402

TOOLS = ide.lab.find_tool("gcc")
needs_tools = pytest.mark.skipif(TOOLS is None, reason="STM32 GCC missing")
SERIAL = "000000000000000000000001"
UID = "00000001-00000002-00000003"


@pytest.fixture
def project(tmp_path):
    for name in ide.SOURCE_NAMES:
        shutil.copyfile(ide.sdk.PROJECT / "student" / name, tmp_path / name)
    return tmp_path


@pytest.fixture(scope="module")
def compiled(tmp_path_factory):
    if TOOLS is None:
        pytest.skip("STM32 GCC missing")
    path = tmp_path_factory.mktemp("editor-compiled")
    for name in ide.SOURCE_NAMES:
        shutil.copyfile(ide.sdk.PROJECT / "student" / name, path / name)
    result = ide.build(path)
    assert result["ok"], result
    return path, result


def copy_compiled(compiled, tmp_path):
    source, result = compiled
    shutil.copytree(source, tmp_path, dirs_exist_ok=True)
    return tmp_path, result["artifact"]["sha256"]


def no_hardware(monkeypatch):
    def forbidden(*args, **kwargs):
        pytest.fail("Unexpected hardware access")
    monkeypatch.setattr(ide.lab, "list_probes", forbidden)
    monkeypatch.setattr(ide.lab, "read_target", forbidden)
    monkeypatch.setattr(ide, "_programmer", forbidden)


@needs_tools
def test_gcc_build_template_keeps_runtime_out_of_upload(compiled):
    path, result = compiled
    artifact = result["artifact"]
    assert artifact["flash_bytes"] == 260
    assert artifact["student_crc32"] == 0x5CBC80B3
    assert artifact["ram_bytes"] == 8192
    assert (path / "build/student.bin").stat().st_size == 260
    assert (path / "build/student.bin").read_bytes()[:4] == b"N7ST"
    targets = artifact["targets"]
    assert set(targets) == {"h753", "g0b1"}
    assert targets["h753"]["student_crc32"] == 0x5CBC80B3 and targets["h753"]["flash_bytes"] == 260
    g0 = targets["g0b1"]
    assert (path / "build/student-g0b1.bin").stat().st_size == g0["flash_bytes"]
    assert (path / "build/student-g0b1.bin").read_bytes()[:4] == b"N7ST"
    assert g0["student_crc32"] != 0x5CBC80B3 and g0["board"] == "NUCLEO-G0B1RE" and g0["dev_id"] == 0x467
    for image in targets.values():
        assert image["load_address"] == "0x08040000"
        assert all(check["ok"] for check in image["checks"])
    assert artifact["sha256"] == ide._build_id(targets)


@pytest.mark.parametrize("source", [
    '#include "../../instructor/master.key"\n',
    '#include "C:/private/secret.h"\n',
    '#define F "secret.h"\n#include F\n',
    '#include_next <stdint.h>\n',
    '#embed "secret.bin"\n',
    '__asm__(".incbin \\\"secret.bin\\\"");\n',
    '__a\\\nsm__(".incbin \\\"secret.bin\\\"");\n',
    '#define JOIN(a,b) a ## b\n',
    '#pragma GCC dependency "secret.h"\n',
    '%:include "../../instructor/master.key"\n',
    '#if __has_include("secret.h")\n#endif\n',
])
def test_forbidden_source_does_not_start_compiler_or_hardware(project, source, monkeypatch):
    no_hardware(monkeypatch)
    monkeypatch.setattr(ide, "_execute", lambda *a, **k: pytest.fail("compiler started"))
    (project / "student_decoder.c").write_text(source, encoding="utf-8")
    with pytest.raises(ide.IdeError):
        ide.build(project)


def test_source_size_and_nul_are_bounded(project):
    file = project / "student_decoder.c"
    file.write_bytes(b"x" * (ide.SOURCE_LIMIT + 1))
    with pytest.raises(ide.IdeError, match="256"):
        ide._sources(project)
    file.write_bytes(b"\0")
    with pytest.raises(ide.IdeError, match="nul"):
        ide._sources(project)


def test_comments_and_strings_are_not_preprocessor_operations():
    ide._validate_source("student_decoder.c", '#include "student_decoder.h" // asm\n'
                         '/* #include "secret.h" */\nconst char *s = "asm %: ?? ##";\n')


@needs_tools
def test_compiler_diagnostic_has_file_and_line_and_no_stale_artifact(project):
    (project / "build").mkdir()
    for name in ide.ARTIFACT_NAMES:
        (project / "build" / name).write_text("obsolete", encoding="utf-8")
    (project / "student_decoder.c").write_text('#include "student_decoder.h"\nvoid student_init(void) { syntax error; }\n', encoding="utf-8")
    result = ide.build(project)
    assert not result["ok"]
    assert any(d["file"] == "student_decoder.c" and d["line"] == 2 and d["severity"] == "error"
               for d in result["diagnostics"])
    assert not any((project / "build" / name).exists() for name in ide.ARTIFACT_NAMES)


@needs_tools
@pytest.mark.parametrize("line", [
    "/*é*/ int x = ;",
    "/*🛠️*/ int x = ;",
    "\tint x = ;",
    "/* é 🛠️ */\tint x = ;",
])
def test_real_gcc_reports_one_based_utf8_byte_columns(project, line):
    (project / "student_decoder.c").write_text('#include "student_decoder.h"\n' + line + "\n", encoding="utf-8")
    result = ide.build(project)
    assert not result["ok"]
    expected = len(line[:line.index(";")].encode("utf-8")) + 1
    assert any(d["file"] == "student_decoder.c" and d["line"] == 2
               and d["column"] == expected and d["severity"] == "error"
               for d in result["diagnostics"]), result
    assert not (project / "build/student.bin").exists()


def test_fatal_error_is_an_error_diagnostic():
    result = ide._diagnostics("student_decoder.h:7:4: fatal error: example.h: No such file or directory")
    assert result == [{"file": "student_decoder.h", "line": 7, "column": 4,
                       "severity": "error", "message": "example.h: No such file or directory"}]


@pytest.mark.parametrize("source", [
    '\n__attribute__((section(".n7rt_runtime"), used)) const unsigned intruder = 1;\n',
    '\nunsigned char too_big[130u * 1024u];\n',
])
@needs_tools
def test_modified_runtime_and_ram_overflow_fail_without_artifact(project, source):
    file = project / "student_decoder.c"
    text = file.read_text(encoding="utf-8")
    if "too_big" in source:
        text = text.replace('n7rt_log("student_decoder ready");', 'too_big[0] = 1; n7rt_publish(0, too_big[0]);')
        text = text.replace('#include "student_decoder.h"', '#include "student_decoder.h"' + source)
    else:
        text += source
    file.write_text(text, encoding="utf-8")
    result = ide.build(project)
    assert not result["ok"]
    assert not (project / "build/student.bin").exists()


def test_compiler_environment_drops_private_credentials_and_path_injections(monkeypatch):
    monkeypatch.setenv("N7RT_E2E_INSTRUCTOR_CODE", "not-a-real-code")
    monkeypatch.setenv("CPATH", "untrusted")
    monkeypatch.setenv("LIBRARY_PATH", "untrusted")
    env = ide._environment()
    assert not any(key in env for key in ("N7RT_E2E_INSTRUCTOR_CODE", "CPATH", "LIBRARY_PATH"))


@needs_tools
def test_stale_source_refuses_flash_before_hardware(compiled, tmp_path, monkeypatch):
    path, digest = copy_compiled(compiled, tmp_path)
    no_hardware(monkeypatch)
    file = path / "student_decoder.c"
    file.write_text(file.read_text(encoding="utf-8") + "\n/* changed */\n", encoding="utf-8")
    with pytest.raises(ide.IdeError, match="source a changé"):
        ide.flash(path, SERIAL, UID, digest)


@needs_tools
def test_wrong_artifact_digest_refuses_flash_before_hardware(compiled, tmp_path, monkeypatch):
    path, digest = copy_compiled(compiled, tmp_path)
    no_hardware(monkeypatch)
    with pytest.raises(ide.IdeError, match="image diffère"):
        ide.flash(path, SERIAL, UID, "0" * 64)


@pytest.mark.parametrize("family", ["h753", "g0b1"])
@pytest.mark.parametrize("failure", ["uid", "runtime", "write", "verification", "after", "none"])
@needs_tools
def test_flash_guards_and_exact_student_only_write(compiled, tmp_path, monkeypatch, failure, family):
    path, digest = copy_compiled(compiled, tmp_path)
    fam = ide.sdk.family(family)
    size = compiled[1]["artifact"]["targets"][family]["flash_bytes"]
    calls = []
    monkeypatch.setattr(ide.lab, "list_probes", lambda: [{"sn": SERIAL}])
    monkeypatch.setattr(ide.lab, "read_target", lambda sn: {
        "dev_id": fam.dev_id, "uid": "FFFFFFFF-FFFFFFFF-FFFFFFFF" if failure == "uid" else UID})
    monkeypatch.setattr(ide, "_runtime_hash", lambda sn, tmp, label, f:
                        "0" * 64 if failure == "runtime" or (failure == "after" and label == "after")
                        else ide.sdk.release(f)["image_sha256"])
    def programmer(*args):
        calls.append(args)
        if "-w" in args:
            assert "-e" not in args and "-ob" not in args and "-rst" not in args
            binary = Path(args[args.index("-w") + 1])
            assert binary.name == "student.bin" and binary.stat().st_size == size
            assert binary.read_bytes() == (path / "build" / ide._elf_name(fam)).with_suffix(".bin").read_bytes()
            assert args[args.index("-w") + 2] == "0x08040000"
            if failure == "write":
                raise ide.IdeError("write failed")
            return "not verified" if failure == "verification" else "Download verified successfully"
        return "reset done"
    monkeypatch.setattr(ide, "_programmer", programmer)
    if failure != "none":
        with pytest.raises(ide.IdeError):
            ide.flash(path, SERIAL, UID, digest)
    else:
        result = ide.flash(path, SERIAL, UID, digest)
        assert result["ok"] and result["runtime_unchanged"] and result["board"] == fam.board
        assert result["artifact"]["student_crc32"] == compiled[1]["artifact"]["targets"][family]["student_crc32"]
    writes = [c for c in calls if "-w" in c]
    resets = [c for c in calls if "-rst" in c]
    assert len(writes) == (0 if failure in ("uid", "runtime") else 1)
    assert bool(resets) == (failure == "none")


@needs_tools
def test_unsupported_board_or_build_without_its_image_refuses_flash(compiled, tmp_path, monkeypatch):
    path, digest = copy_compiled(compiled, tmp_path)
    monkeypatch.setattr(ide.lab, "list_probes", lambda: [{"sn": SERIAL}])
    monkeypatch.setattr(ide, "_programmer", lambda *a: pytest.fail("programmer started"))
    monkeypatch.setattr(ide.lab, "read_target", lambda sn: {"dev_id": 0x413, "uid": UID})
    with pytest.raises(ide.IdeError, match="non prise en charge"):
        ide.flash(path, SERIAL, UID, digest)
    monkeypatch.setattr(ide.lab, "read_target", lambda sn: {"dev_id": ide.sdk.G0B1.dev_id, "uid": UID})
    (path / "build/student-g0b1.elf").unlink()
    with pytest.raises(ide.IdeError, match="NUCLEO-G0B1RE"):
        ide.flash(path, SERIAL, UID, digest)


def test_main_error_is_machine_readable_json(project, capsys):
    (project / "student_decoder.c").unlink()
    assert ide.main(["build", "--workspace", str(project)]) == 1
    response = json.loads(capsys.readouterr().out)
    assert response["ok"] is False and response["operation"] == "build"


def test_no_sdk_or_linker_override_is_a_cli_argument():
    with pytest.raises(SystemExit) as done:
        ide.main(["build", "--workspace", ".", "--linker", "evil.ld"])
    assert done.value.code == 2


@needs_tools
@pytest.mark.parametrize("pin_location", ["runtime-release.json", "firmware/n7rt_student/runtime-release.json"])
def test_archive_root_sdk_builds_outside_source_tree(tmp_path, pin_location):
    package = tmp_path / "extracted"
    scripts = package / "tools/stm32"
    scripts.mkdir(parents=True)
    for name in ("student_ide.py", "n7rt_sdk.py", "stm32lab.py"):
        shutil.copyfile(Path(ide.__file__).parent / name, scripts / name)
    shutil.copytree(ide.sdk.PROJECT, package / ide.sdk.PROJECT_NAME,
                    ignore=shutil.ignore_patterns(ide.sdk.CONFIG, ".settings", "build"))
    for fam in ide.sdk.FAMILIES:  # every board's pin, at the location under test
        pin = package / pin_location.replace("runtime-release.json", fam.release_name)
        pin.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(fam.release_file, pin)
    workspace = tmp_path / "profile"
    workspace.mkdir()
    for name in ide.SOURCE_NAMES:
        shutil.copyfile(package / ide.sdk.PROJECT_NAME / "student" / name, workspace / name)
    done = subprocess.run([sys.executable, "-B", "-X", "utf8", str(scripts / "student_ide.py"),
                           "build", "--workspace", str(workspace)], cwd=package, capture_output=True,
                          text=True, encoding="utf-8", timeout=120)
    result = json.loads(done.stdout)
    assert done.returncode == 0 and result["ok"], result
    assert result["artifact"]["student_crc32"] == 0x5CBC80B3
    assert (workspace / "build/student.bin").stat().st_size == 260
    assert (workspace / "build/student-g0b1.bin").is_file()


@pytest.mark.parametrize("key, operation", [("gcc", "build"), ("programmer", "flash")])
def test_missing_tool_is_explained_in_french_with_the_shipped_guide(project, compiled, tmp_path, monkeypatch, capsys,
                                                                    key, operation):
    real = ide.lab.find_tool
    monkeypatch.setattr(ide.lab, "find_tool", lambda k: None if k == key else real(k))
    if operation == "flash":
        # the real flash path, with a valid compilation: the programmer is checked before any probe is listed
        workspace, sha = copy_compiled(compiled, tmp_path / "flash")
        args = ["--workspace", str(workspace), "--serial", "0" * 24, "--uid", "00000000-00000000-00000000",
                "--expected-sha256", sha]
    else:
        args = ["--workspace", str(project)]
    assert ide.main([operation, *args]) == 1
    error = json.loads(capsys.readouterr().out)["error"]
    assert error == ide.MISSING_TOOL[key] and "introuvable" in error
    assert (Path(__file__).resolve().parents[3] / "docs/stm32-toolchain-windows.md").is_file()
