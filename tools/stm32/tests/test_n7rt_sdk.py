"""Student SDK (S1.3): consistency, and links made with the toolchain STM32CubeIDE really uses.

Compile and link flags are those of the CubeIDE managed build of the template project
(firmware/n7rt_student/N7RT_CANLab_Student, configuration Debug). The last test drives the real
CubeIDE headless build; it is heavy and runs only with N7RT_RUN_CUBEIDE=1.
"""
from __future__ import annotations

import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import n7rt_sdk as sdk  # noqa: E402
import stm32lab as lab  # noqa: E402

CUBEIDE = sdk.find_cubeide(lab.ST_ROOT)
CUBE_BIN = sdk.cubeide_gcc_dir(CUBEIDE) if CUBEIDE else None
BUNDLE = lab.find_tool("gcc")
needs_tools = pytest.mark.skipif(CUBE_BIN is None or BUNDLE is None or not (sdk.N7RT / "manifest.json").is_file(),
                                 reason="STM32CubeIDE toolchain, GCC bundle or generated SDK missing")
TEMPLATE = (sdk.PROJECT / "student" / "student_decoder.c").read_text(encoding="utf-8")
CFLAGS = ["-mcpu=cortex-m7", "-std=gnu11", "-g3", "-O0", "-ffunction-sections", "-fdata-sections", "-Wall",
          "-Wextra", "--specs=nano.specs", "-mfpu=fpv5-d16", "-mfloat-abi=hard", "-mthumb"]


def cube(tool: str) -> str:
    return str(CUBE_BIN / f"arm-none-eabi-{tool}{sdk.EXE}")


def build(tmp: Path, source: str, include: Path | None = None, blob: Path | None = None,
          script: Path | None = None) -> subprocess.CompletedProcess:
    """Compile + link like the CubeIDE managed build (same flags as its generated makefile)."""
    (tmp / "student").mkdir(exist_ok=True)
    (tmp / "student" / "student_decoder.h").write_text('#include "n7rt.h"\n', encoding="utf-8")
    (tmp / "student" / "student_decoder.c").write_text(source, encoding="utf-8")
    inc = include or sdk.N7RT / "include"
    proc = subprocess.run([cube("gcc"), "student/student_decoder.c", "-c", "-I", str(inc), *CFLAGS,
                           "-o", "student/student_decoder.o"], cwd=tmp, capture_output=True, text=True)
    if proc.returncode != 0:
        return proc
    return subprocess.run(
        [cube("gcc"), "-o", "N7RT_CANLab_Student.elf", "student/student_decoder.o",
         str(blob or sdk.N7RT / "lib" / "n7rt_runtime.o"), "-ln7rt_glue", "-mcpu=cortex-m7",
         f"-T{script or sdk.N7RT / 'ld' / 'N7RT_H753_student.ld'}", "--specs=nosys.specs",
         "-Wl,-Map=N7RT_CANLab_Student.map", "-Wl,--gc-sections", "-nostartfiles", "-static",
         f"-L{sdk.N7RT / 'lib'}", "--specs=nano.specs", "-mfpu=fpv5-d16", "-mfloat-abi=hard", "-mthumb",
         "-Wl,--start-group", "-lc", "-lm", "-Wl,--end-group"],
        cwd=tmp, capture_output=True, text=True)


@needs_tools
def test_sdk_files_agree_with_each_other_and_with_the_release():
    checks = sdk.check_sdk(BUNDLE[1].parent)
    assert all(ok for _, ok, _ in checks), [c for c in checks if not c[1]]


needs_gcc = pytest.mark.skipif(BUNDLE is None or not (sdk.G0B1.n7rt / "manifest.json").is_file(),
                               reason="GCC or generated G0B1 SDK missing")


@needs_gcc
def test_every_board_sdk_agrees_with_its_own_release():
    for fam in sdk.FAMILIES:
        checks = sdk.check_sdk(BUNDLE[1].parent, fam)
        assert all(ok for _, ok, _ in checks), (fam.key, [c for c in checks if not c[1]])
    assert sdk.release(sdk.G0B1)["image_sha256"] != sdk.release(sdk.H753)["image_sha256"]


@needs_gcc
def test_g0b1_template_links_with_soft_float_and_lands_in_its_regions(tmp_path):
    gcc = BUNDLE[1].parent / f"arm-none-eabi-gcc{sdk.EXE}"
    n7rt, flags = sdk.G0B1.n7rt, [*sdk.G0B1.cpu_flags, "-std=gnu11", "-O0", "--specs=nano.specs"]
    (tmp_path / "student_decoder.h").write_text('#include "n7rt.h"\n', encoding="utf-8")
    # float and division on a Cortex-M0+: libgcc helpers must be linked into the student region
    (tmp_path / "student_decoder.c").write_text(
        TEMPLATE + "\nfloat n7rt_test_scale(unsigned a, unsigned b) { return (float)(a / b) * 0.5f; }\n",
        encoding="utf-8")
    proc = subprocess.run([gcc, *flags, "-I", n7rt / "include", "-I", tmp_path, "-ffunction-sections", "-c",
                           "student_decoder.c", "-o", "s.o"], cwd=tmp_path, capture_output=True, text=True)
    assert proc.returncode == 0, proc.stderr
    proc = subprocess.run([gcc, *flags, "s.o", n7rt / "lib" / "n7rt_runtime.o", "-L", n7rt / "lib",
                           "-T", n7rt / "ld" / sdk.G0B1.linker_script, "--specs=nosys.specs", "-nostartfiles", "-static",
                           "-Wl,--undefined=n7rt_test_scale", "-o", "s.elf",
                           "-Wl,--start-group", "-ln7rt_glue", "-lc", "-lm", "-lgcc", "-Wl,--end-group"],
                          cwd=tmp_path, capture_output=True, text=True)
    assert proc.returncode == 0, proc.stderr
    report = sdk.student_image_report(BUNDLE[1].parent, tmp_path / "s.elf", sdk.G0B1)
    assert report["ok"], [c for c in report["checks"] if not c[1]]
    assert sdk.sha256(report["runtime_image"]) == sdk.release(sdk.G0B1)["image_sha256"]


@needs_tools
def test_template_links_and_lands_where_the_runtime_expects_it(tmp_path):
    proc = build(tmp_path, TEMPLATE)
    assert proc.returncode == 0, proc.stderr
    report = sdk.student_image_report(BUNDLE[1].parent, tmp_path / "N7RT_CANLab_Student.elf")
    assert report["ok"], [c for c in report["checks"] if not c[1]]
    assert sdk.sha256(report["runtime_image"]) == sdk.release()["image_sha256"]


@needs_tools
def test_ram_report_counts_live_data_bss_and_heap_when_end_symbols_are_absent(tmp_path, monkeypatch):
    source = """#include "student_decoder.h"
static volatile uint8_t retained[125];
static volatile uint32_t settings[3] = {1u, 2u, 3u};
void student_init(void) { retained[0] = (uint8_t)settings[0]; }
void student_on_frame(const n7rt_can_frame_t *frame) { retained[124] = frame->len; }
void student_on_tick(uint64_t now_us) { (void)now_us; n7rt_publish(0u, (float)(retained[0] + retained[124])); }
"""
    proc = build(tmp_path, source)
    assert proc.returncode == 0, proc.stderr
    elf = tmp_path / "N7RT_CANLab_Student.elf"
    symbols = sdk._symbols(Path(cube("nm")), elf)
    assert "_end" not in symbols and "end" not in symbols
    sections = subprocess.run([cube("objdump"), "-h", str(elf)], capture_output=True, text=True, check=True).stdout
    heap = re.search(r"^\s*\d+\s+\._heap\s+([0-9a-fA-F]+)\s+([0-9a-fA-F]+)", sections, re.M)
    assert heap is not None
    heap_size, heap_address = (int(v, 16) for v in heap.groups())
    assert sdk.HEAP_SIZE <= heap_size < sdk.HEAP_SIZE + 8
    report = sdk.student_image_report(BUNDLE[1].parent, elf)
    assert report["ok"], [c for c in report["checks"] if not c[1]]
    assert report["student_ram_used"] == heap_address + heap_size - sdk.STUDENT_RAM[0]
    assert report["student_ram_used"] > sdk.HEAP_SIZE
    assert heap_address + heap_size - sdk.HEAP_SIZE > symbols["__n7rt_student_bss_end"][0], "alignment padding also consumes DTCM"
    # Boundary symbols belong to the pinned SDK contract. Missing metadata
    # must fail closed rather than reintroducing the 8 KiB under-estimate.
    symbols.pop("__n7rt_student_bss_end")
    monkeypatch.setattr(sdk, "_symbols", lambda *_args: symbols)
    assert not sdk.student_image_report(BUNDLE[1].parent, elf)["ok"]


# Reachable growth: the hooks use the arrays, as real student code would. (Unreferenced code is
# legitimately dropped by --gc-sections and does not grow the image.)
FLASH_GROWTH = """#include "student_decoder.h"
const unsigned char growth[800u * 1024u] = {1};
void student_init(void) {}
void student_on_frame(const n7rt_can_frame_t *frame) { n7rt_publish(0u, (float)growth[frame->len]); }
void student_on_tick(uint64_t now_us) { (void)now_us; }
"""
RAM_GROWTH = """#include "student_decoder.h"
unsigned char table[130u * 1024u];
void student_init(void) {}
void student_on_frame(const n7rt_can_frame_t *frame) { table[frame->len] = 1u; n7rt_publish(0u, (float)table[0]); }
void student_on_tick(uint64_t now_us) { (void)now_us; }
"""


@needs_tools
def test_student_code_growing_past_its_flash_region_fails_the_link(tmp_path):
    proc = build(tmp_path, FLASH_GROWTH)
    assert proc.returncode != 0 and "region `ST_FLASH' overflowed" in proc.stderr, proc.stderr[-400:]


@needs_tools
def test_student_data_growing_past_dtcm_fails_the_link(tmp_path):
    proc = build(tmp_path, RAM_GROWTH)
    assert proc.returncode != 0 and "region `ST_RAM' overflowed" in proc.stderr, proc.stderr[-400:]


@needs_tools
def test_student_cannot_put_anything_in_the_runtime_region(tmp_path):
    source = TEMPLATE + '\n__attribute__((section(".n7rt_runtime"), used)) const unsigned intruder = 1;\n'
    proc = build(tmp_path, source)
    assert proc.returncode != 0, "a student section landed in the runtime flash"
    assert "RT_FLASH' overflowed" in proc.stderr or "runtime image must sit unchanged" in proc.stderr


@needs_tools
def test_header_of_another_abi_fails_the_link(tmp_path):
    other = tmp_path / "other_include"
    other.mkdir()
    header = (sdk.N7RT / "include" / "n7rt.h").read_text(encoding="utf-8")
    (other / "n7rt.h").write_text(header.replace("n7rt_glue_abi_1", "n7rt_glue_abi_2"), encoding="utf-8")
    proc = build(tmp_path, TEMPLATE, include=other)
    assert proc.returncode != 0 and "n7rt_glue_abi_2" in proc.stderr, proc.stderr[-400:]


def _fake_runtime(tmp: Path, payload: bytes, tag: str) -> Path:
    (tmp / "fake.bin").write_bytes(payload)
    (tmp / "fake.S").write_text(
        '    .section .n7rt_runtime, "a"\n    .incbin "fake.bin"\n'
        f"    .global {tag}\n    .set {tag}, 0x08000000\n"
        "    .global n7rt_runtime_abi_1\n    .set n7rt_runtime_abi_1, 1\n"
        "    .global n7rt_runtime_reset\n    .set n7rt_runtime_reset, 0x08000461\n", encoding="utf-8")
    subprocess.run([cube("gcc"), "-c", "-mcpu=cortex-m7", "-mthumb", "-x", "assembler-with-cpp", "fake.S",
                    "-o", "fake.o"], cwd=tmp, check=True, capture_output=True)
    return tmp / "fake.o"


@needs_tools
def test_runtime_of_another_version_or_size_fails_the_link(tmp_path):
    image = sdk.runtime_bytes_from_object(BUNDLE[1].parent, sdk.N7RT / "lib" / "n7rt_runtime.o")
    other_version = _fake_runtime(tmp_path, image, "n7rt_runtime_v0_2_0")
    proc = build(tmp_path, TEMPLATE, blob=other_version)
    assert proc.returncode != 0 and "is not runtime 0.1.0" in proc.stderr, proc.stderr[-400:]
    longer = _fake_runtime(tmp_path, image + b"\x00" * 4, "n7rt_runtime_v0_1_0")
    proc = build(tmp_path, TEMPLATE, blob=longer)
    assert proc.returncode != 0 and "RT_FLASH' overflowed" in proc.stderr, proc.stderr[-400:]


@needs_tools
def test_linker_script_of_another_release_fails_the_link(tmp_path):
    script = (sdk.N7RT / "ld" / "N7RT_H753_student.ld").read_text(encoding="utf-8")
    edited = tmp_path / "other.ld"
    edited.write_text(script.replace("n7rt_runtime_v0_1_0", "n7rt_runtime_v0_2_0"), encoding="utf-8")
    proc = build(tmp_path, TEMPLATE, script=edited)
    assert proc.returncode != 0 and "is not runtime" in proc.stderr, proc.stderr[-400:]


@pytest.mark.skipif(os.environ.get("N7RT_RUN_CUBEIDE") != "1" or CUBEIDE is None,
                    reason="real CubeIDE headless build: set N7RT_RUN_CUBEIDE=1")
def test_cubeide_build_fails_when_the_student_outgrows_its_flash(tmp_path):
    project = tmp_path / sdk.PROJECT_NAME
    shutil.copytree(sdk.PROJECT, project, ignore=shutil.ignore_patterns(sdk.CONFIG, ".settings"))
    (project / "student" / "student_decoder.c").write_text(FLASH_GROWTH, encoding="utf-8")
    ok, elf, log = sdk.cubeide_build(CUBEIDE, project, tmp_path / "workspace")
    assert not ok and not elf.is_file()
    assert "region `ST_FLASH' overflowed" in log, log[-1500:]
