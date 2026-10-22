"""N7RT student SDK (S1.3), used by stm32lab (sdk, build/flash/smoke-test --student) and the editor.

One SDK per board family, side by side in the template project: n7rt/ for NUCLEO-H753ZI (Cortex-M7),
n7rt_g0b1/ for NUCLEO-G0B1RE (Cortex-M0+). The student source is the same for both.

- generate(): package a runtime release (pinned in firmware/n7rt_student/runtime-release*.json)
  into the template project: precompiled runtime object, ABI-tagged glue, public header,
  generated linker script, manifest. The runtime bytes are taken from the validated build, never
  recompiled; the generator refuses any image whose SHA-256 differs from the pin.
- check_sdk(): the SDK files agree with each other and with the pin (no secret, no runtime source).
- cubeide_build(): real STM32CubeIDE headless clean build of the template project (H753 only).
- student_image_report(): placement and integrity of a student image.
"""
from __future__ import annotations

import hashlib
import json
import os
import re
import shutil
import struct
import subprocess
import tempfile
import zipfile
import zlib
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
PROJECT_NAME = "N7RT_CANLab_Student"
SOURCE_SDK_DIR = ROOT / "firmware" / "n7rt_student"
# Source checkouts retain firmware/; the student archive ships its CubeIDE
# project at the archive root. Both layouts contain the same public SDK.
SDK_DIR = (SOURCE_SDK_DIR if (SOURCE_SDK_DIR / PROJECT_NAME).is_dir()
           else ROOT if (ROOT / PROJECT_NAME).is_dir() else SOURCE_SDK_DIR)
PROJECT = SDK_DIR / PROJECT_NAME
CONFIG = "Debug"  # never "N7RT": it would be the same folder as n7rt/ on case-insensitive file systems
DIST = SDK_DIR / "dist"
RUNTIME_HEADER = ROOT / "firmware" / "stm32_h753_bringup" / "runtime" / "include" / "n7rt.h"
SDK_VERSION = "0.1.0"
EXE = ".exe" if os.name == "nt" else ""

RUNTIME_BASE = 0x08000000
STUDENT_HEADER = 0x08040000
API_TABLE = 0x08000400
STUDENT_MAGIC = 0x5453374E
HEAP_SIZE = 8 * 1024
STUDENT_FILES = ("student/student_decoder.c", "student/student_decoder.h")
FORBIDDEN = ("dbc", "mapping", "seed", "secret", "answer", "solution", "expected_value", "scale", "offset")


@dataclass(frozen=True)
class Family:
    """A supported board: its MCU identity, student memory map and SDK folder. Same ABI v1 for all:
    runtime at 0x08000000, API table at 0x08000400, student header at 0x08040000."""
    key: str
    board: str
    mcu: str
    chip: str                      # part number of the linker script header
    runtime_ram_label: str
    dev_id: int                    # DBGMCU_IDCODE.DEV_ID
    uid_addr: int                  # 96-bit unique ID
    cpu_flags: tuple[str, ...]
    student_flash: tuple[int, int]
    student_ram: tuple[int, int]
    ram_label: str
    runtime_sp: int                # initial SP of the runtime vector table
    sdk_dir: str
    linker_script: str
    release_name: str
    sw_reset_bit: int              # software reset flag in the reset_flags of HELLO_ACK

    @property
    def n7rt(self) -> Path:
        return PROJECT / self.sdk_dir

    @property
    def generated(self) -> tuple[str, ...]:
        return ("include/n7rt.h", "lib/n7rt_runtime.o", "lib/libn7rt_glue.a", f"ld/{self.linker_script}",
                "LISEZMOI.txt")

    @property
    def release_file(self) -> Path:
        return next((p for p in (SDK_DIR / self.release_name, SOURCE_SDK_DIR / self.release_name,
                                 ROOT / self.release_name) if p.is_file()), SDK_DIR / self.release_name)


H753 = Family("h753", "NUCLEO-H753ZI", "STM32H753", "STM32H753ZI", "AXI SRAM", 0x450, 0x1FF1E800,
              ("-mcpu=cortex-m7", "-mthumb", "-mfpu=fpv5-d16", "-mfloat-abi=hard"),
              (0x08040000, 0x08100000), (0x20000000, 0x20020000), "DTCM 128 KB", 0x24004000,
              "n7rt", "N7RT_H753_student.ld", "runtime-release.json", 1 << 24)  # RCC_RSR.SFTRSTF
G0B1 = Family("g0b1", "NUCLEO-G0B1RE", "STM32G0B1", "STM32G0B1RE", "first 48 KB of SRAM", 0x467, 0x1FFF7590,
              ("-mcpu=cortex-m0plus", "-mthumb", "-mfloat-abi=soft"),
              (0x08040000, 0x08080000), (0x2000C000, 0x20024000), "SRAM 96 KB", 0x20002000,
              "n7rt_g0b1", "N7RT_G0B1_student.ld", "runtime-release-g0b1.json", 1 << 28)  # RCC_CSR.SFTRSTF
FAMILIES = (H753, G0B1)

# H753 names kept for the bench tools written before the G0B1.
N7RT = H753.n7rt
RELEASE_FILE = H753.release_file
STUDENT_FLASH = H753.student_flash
STUDENT_RAM = H753.student_ram
RUNTIME_SP = H753.runtime_sp
GENERATED = H753.generated


class SdkError(Exception):
    pass


def family(key: str) -> Family:
    found = next((f for f in FAMILIES if f.key == key), None)
    if found is None:
        raise SdkError(f"unknown board family '{key}' (known: {', '.join(f.key for f in FAMILIES)})")
    return found


def family_for_dev_id(dev_id: int) -> Family | None:
    return next((f for f in FAMILIES if f.dev_id == dev_id), None)


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def release(fam: Family = H753) -> dict:
    return json.loads(fam.release_file.read_text(encoding="utf-8"))


def version_tag(rel: dict) -> str:
    return "v" + rel["runtime_version"].replace(".", "_")


def _run(cmd: list, cwd: Path | None = None) -> str:
    proc = subprocess.run([str(c) for c in cmd], cwd=cwd, capture_output=True, text=True)
    if proc.returncode != 0:
        raise SdkError(f"{Path(str(cmd[0])).name} failed: {(proc.stderr or proc.stdout).strip()[:400]}")
    return proc.stdout


def _tool(bin_dir: Path, name: str) -> Path:
    return bin_dir / f"arm-none-eabi-{name}{EXE}"


# --- generation ---------------------------------------------------------------------------------

def _sdk_header(rel: dict) -> str:
    major, minor = rel["abi"]
    text = RUNTIME_HEADER.read_text(encoding="utf-8")
    tail = "#ifdef __cplusplus\n}\n#endif\n\n#endif /* N7RT_H */\n"
    if not text.endswith(tail):
        raise SdkError(f"{RUNTIME_HEADER} changed shape: update the SDK header generator")
    block = (
        f"/* ---- Student SDK {SDK_VERSION}, runtime {rel['runtime_version']}, ABI {major}.{minor} ---- */\n"
        f"#define N7RT_ABI_MAJOR {major}\n"
        f"#define N7RT_ABI_MINOR {minor}\n"
        f"/* Link-time compatibility check: only the runtime glue of ABI {major} (n7rt/lib) defines this\n"
        " * symbol, so building with a header of another SDK fails at link time. */\n"
        f"extern const char n7rt_glue_abi_{major}[];\n"
        "__attribute__((section(\".n7rt_abi_check\"), used))\n"
        f"static const char *const n7rt_abi_check = n7rt_glue_abi_{major};\n\n")
    head = ("/* N7RT student SDK: generated by `stm32lab sdk` from the runtime public header. Do not edit. */\n")
    return head + text[: -len(tail)] + block + tail


def _linker_script(rel: dict, reset: int, fam: Family = H753) -> str:
    major, minor = rel["abi"]
    size = rel["image_size"]
    tag = version_tag(rel)
    flash_kb = (fam.student_flash[1] - fam.student_flash[0]) // 1024
    ram_kb = (fam.student_ram[1] - fam.student_ram[0]) // 1024
    return f"""\
/*
 * N7RT student image, {fam.chip}. Generated by `stm32lab sdk` (SDK {SDK_VERSION}, runtime {rel['runtime_version']},
 * ABI {major}.{minor}). Do not edit: the runtime reads the student header at 0x08040000 and the student
 * calls the runtime through its API table at 0x{API_TABLE:08X} (docs/can-lab-architecture.md 4.2).
 *
 *   RT_FLASH 0x08000000  precompiled runtime, exactly {size} bytes (SHA-256 {rel['image_sha256'][:16]}...)
 *   ST_FLASH 0x08040000  student header (64 B), student code and constants, {flash_kb} KB
 *   ST_RAM   0x{fam.student_ram[0]:08X}  student .data / .bss / heap ({HEAP_SIZE // 1024} KB), {fam.ram_label}
 * The runtime RAM ({fam.runtime_ram_label}) is not declared: nothing of the student image can be placed there.
 */
ENTRY(n7rt_runtime_reset)            /* 0x{reset:08X}: the runtime Reset_Handler */
EXTERN(n7rt_student_header)

__n7rt_heap_size = {HEAP_SIZE};

MEMORY
{{
  RT_FLASH (rx) : ORIGIN = 0x08000000, LENGTH = {size}
  ST_FLASH (rx) : ORIGIN = 0x08040000, LENGTH = {flash_kb}K
  ST_RAM   (rw) : ORIGIN = 0x{fam.student_ram[0]:08X}, LENGTH = {ram_kb}K
}}

SECTIONS
{{
  .n7rt_runtime : {{ KEEP(*(.n7rt_runtime)) }} >RT_FLASH

  .n7rt_student_header : {{ KEEP(*(.n7rt_student_header)) }} >ST_FLASH

  .text :
  {{
    . = ALIGN(4);
    *(.text .text.*)
    *(.glue_7) *(.glue_7t) *(.vfp11_veneer) *(.v4_bx) *(.eh_frame)
    . = ALIGN(4);
  }} >ST_FLASH

  .rodata :
  {{
    . = ALIGN(4);
    *(.rodata .rodata.*)
    KEEP(*(.n7rt_abi_check))
    . = ALIGN(4);
  }} >ST_FLASH

  .ARM.extab : {{ *(.ARM.extab* .gnu.linkonce.armextab.*) }} >ST_FLASH
  .ARM.exidx : {{ *(.ARM.exidx*) }} >ST_FLASH

  .data :
  {{
    . = ALIGN(4);
    __n7rt_student_data_start = .;
    *(.data .data.*)
    . = ALIGN(4);
    __n7rt_student_data_end = .;
  }} >ST_RAM AT>ST_FLASH
  __n7rt_student_data_load = LOADADDR(.data);
  __n7rt_student_image_end = LOADADDR(.data) + SIZEOF(.data);

  .bss (NOLOAD) :
  {{
    . = ALIGN(4);
    __n7rt_student_bss_start = .;
    *(.bss .bss.*)
    *(COMMON)
    . = ALIGN(4);
    __n7rt_student_bss_end = .;
  }} >ST_RAM

  ._heap (NOLOAD) :
  {{
    . = ALIGN(8);
    PROVIDE(end = .);
    PROVIDE(_end = .);
    . = . + __n7rt_heap_size;
  }} >ST_RAM
}}

ASSERT(DEFINED(n7rt_runtime_{tag}), "N7RT: n7rt/lib/n7rt_runtime.o is not runtime {rel['runtime_version']}: use the linker script shipped with it")
ASSERT(DEFINED(n7rt_runtime_abi_{major}) && DEFINED(n7rt_glue_abi_{major}), "N7RT: runtime, glue and linker script do not share ABI {major}")
ASSERT(ADDR(.n7rt_runtime) == 0x08000000 && SIZEOF(.n7rt_runtime) == {size}, "N7RT: the runtime image must sit unchanged at 0x08000000 ({size} bytes)")
ASSERT(ADDR(.n7rt_student_header) == 0x08040000 && SIZEOF(.n7rt_student_header) == 64, "N7RT: student header missing or misplaced (n7rt/lib/libn7rt_glue.a)")
ASSERT(__n7rt_student_image_end <= 0x{fam.student_flash[1]:08X}, "N7RT: student image overflows its flash region")
"""


README = """\
N7RT CAN Lab, SDK etudiant {sdk} (runtime {rt}, ABI {abi})
==========================================================

NE RIEN MODIFIER dans ce dossier {sdk_dir}/ : il est verifie a chaque build et sur la carte ({board}).

  include/n7rt.h              API etudiante : trame recue, publication, rejet, temps, log
  lib/n7rt_runtime.o          runtime N7RT precompile (place a 0x08000000, jamais recompile)
  lib/libn7rt_glue.a          liaison runtime <-> votre code (en-tete a 0x08040000)
  ld/{ld:<26}  carte memoire : votre code a partir de 0x08040000, vos donnees en {ram}
  manifest.json               versions et empreintes SHA-256 de ces fichiers

Votre code : student/student_decoder.c et student/student_decoder.h, rien d'autre.
"""


def generate(bin_dir: Path, runtime_image: bytes, build_id: str, student_archive: Path, fam: Family = H753) -> dict:
    """bin_dir: toolchain used for the runtime (GCC bundle); runtime_image: runtime region of the
    S1.2 build; student_archive: the S1.2 libn7rt_student.a (its glue object is reused as is)."""
    rel = release(fam)
    N7RT = fam.n7rt
    if len(runtime_image) != rel["image_size"] or sha256(runtime_image) != rel["image_sha256"]:
        raise SdkError(f"runtime image {len(runtime_image)} B / {sha256(runtime_image)[:16]} is not the validated "
                       f"release {rel['image_size']} B / {rel['image_sha256'][:16]} ({fam.release_name})")
    if build_id != rel["build_id"]:
        raise SdkError(f"runtime build id {build_id} differs from the release {rel['build_id']}")
    sp, reset = struct.unpack_from("<II", runtime_image, 0)
    if sp != fam.runtime_sp or not (reset & 1) or not RUNTIME_BASE <= reset < RUNTIME_BASE + len(runtime_image):
        raise SdkError(f"unexpected vector table: SP 0x{sp:08X}, reset 0x{reset:08X}")
    major, _ = rel["abi"]
    tag = version_tag(rel)
    for sub in ("include", "lib", "ld"):
        (N7RT / sub).mkdir(parents=True, exist_ok=True)
    gcc, objcopy, ar = _tool(bin_dir, "gcc"), _tool(bin_dir, "objcopy"), _tool(bin_dir, "ar")
    with tempfile.TemporaryDirectory() as tmp_name:
        tmp = Path(tmp_name)
        (tmp / "n7rt_runtime.bin").write_bytes(runtime_image)
        (tmp / "n7rt_runtime.S").write_text(
            "/* Precompiled N7RT runtime image, linked at 0x08000000 by the student linker script. */\n"
            "    .section .n7rt_runtime, \"a\"\n"
            "    .global __n7rt_runtime_start\n__n7rt_runtime_start:\n"
            "    .incbin \"n7rt_runtime.bin\"\n"
            "    .global __n7rt_runtime_end\n__n7rt_runtime_end:\n"
            f"    .global n7rt_runtime_{tag}\n    .set n7rt_runtime_{tag}, 0x{RUNTIME_BASE:08X}\n"
            f"    .global n7rt_runtime_abi_{major}\n    .set n7rt_runtime_abi_{major}, {major}\n"
            f"    .global n7rt_runtime_reset\n    .set n7rt_runtime_reset, 0x{reset:08X}\n",
            encoding="utf-8", newline="\n")
        _run([gcc, "-c", *fam.cpu_flags,
              "-x", "assembler-with-cpp", "n7rt_runtime.S", "-o", "n7rt_runtime.o"], cwd=tmp)
        shutil.copyfile(tmp / "n7rt_runtime.o", N7RT / "lib" / "n7rt_runtime.o")
        _run([ar, "x", student_archive, "n7rt_student_glue.c.obj"], cwd=tmp)
        _run([objcopy, "--strip-debug", "--add-symbol", f"n7rt_glue_abi_{major}=1,global",
              "n7rt_student_glue.c.obj", "n7rt_glue.o"], cwd=tmp)
        glue_lib = N7RT / "lib" / "libn7rt_glue.a"
        glue_lib.unlink(missing_ok=True)
        _run([ar, "rcsD", glue_lib, tmp / "n7rt_glue.o"])
    (N7RT / "include" / "n7rt.h").write_text(_sdk_header(rel), encoding="utf-8", newline="\n")
    (N7RT / "ld" / fam.linker_script).write_text(_linker_script(rel, reset, fam), encoding="utf-8", newline="\n")
    (N7RT / "LISEZMOI.txt").write_text(README.format(sdk=SDK_VERSION, rt=rel["runtime_version"],
                                                     abi=f"{major}.{rel['abi'][1]}", sdk_dir=fam.sdk_dir,
                                                     board=fam.board, ld=fam.linker_script,
                                                     ram="DTCM" if fam is H753 else "SRAM"),
                                       encoding="utf-8", newline="\n")
    manifest = {
        "_doc": f"N7RT student SDK manifest (generated by stm32lab sdk). Every {fam.sdk_dir}/ file is listed with its SHA-256.",
        "sdk_version": SDK_VERSION,
        "runtime": {k: rel[k] for k in ("runtime_version", "build_id", "image_size", "image_sha256", "toolchain",
                                        "validated_on", "evidence")}
        | {"load_address": f"0x{RUNTIME_BASE:08X}", "initial_sp": f"0x{sp:08X}", "reset_vector": f"0x{reset:08X}"},
        "abi": {"major": rel["abi"][0], "minor": rel["abi"][1], "student_header": f"0x{STUDENT_HEADER:08X}",
                "api_table": f"0x{API_TABLE:08X}",
                "symbols": {"runtime_version": f"n7rt_runtime_{tag}", "runtime_abi": f"n7rt_runtime_abi_{major}",
                            "glue_abi": f"n7rt_glue_abi_{major}", "entry": "n7rt_runtime_reset"}},
        "layout": {"runtime_flash": [f"0x{RUNTIME_BASE:08X}", rel["image_size"]],
                   "student_flash": [f"0x{fam.student_flash[0]:08X}", fam.student_flash[1] - fam.student_flash[0]],
                   "student_ram": [f"0x{fam.student_ram[0]:08X}", fam.student_ram[1] - fam.student_ram[0]],
                   "student_heap": HEAP_SIZE},
        "student_files": list(STUDENT_FILES),
        "files": {f"{fam.sdk_dir}/{rel_path}": sha256((N7RT / rel_path).read_bytes()) for rel_path in fam.generated},
    }
    (N7RT / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8", newline="\n")
    return manifest


def make_dist() -> Path:
    """Zip of the template project as students receive it (no build output)."""
    rel = release()
    DIST.mkdir(parents=True, exist_ok=True)
    out = DIST / f"{PROJECT_NAME}_sdk{SDK_VERSION}_rt{rel['runtime_version']}.zip"
    files = sorted(p for p in PROJECT.rglob("*") if p.is_file()
                   and p.relative_to(PROJECT).parts[0] not in (CONFIG, ".settings"))  # IDE output and state
    with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as z:
        for p in files:
            info = zipfile.ZipInfo(f"{PROJECT_NAME}/{p.relative_to(PROJECT).as_posix()}", date_time=(2026, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            z.writestr(info, p.read_bytes())
    return out


# --- checks --------------------------------------------------------------------------------------

def _symbols(nm: Path, path: Path, *opts: str) -> dict[str, tuple[int, str]]:
    out = {}
    for line in _run([nm, *opts, path]).splitlines():
        parts = line.split()
        if len(parts) == 3 and re.fullmatch(r"[0-9a-fA-F]+", parts[0]):
            out[parts[2]] = (int(parts[0], 16), parts[1])
    return out


def _undefined(nm: Path, path: Path) -> set[str]:
    return {line.split()[-1] for line in _run([nm, "-u", path]).splitlines() if line.strip().startswith("U ")}


def runtime_bytes_from_object(bin_dir: Path, obj: Path) -> bytes:
    with tempfile.TemporaryDirectory() as tmp_name:
        out = Path(tmp_name) / "rt.bin"
        _run([_tool(bin_dir, "objcopy"), "-O", "binary", "-j", ".n7rt_runtime", obj, out])
        return out.read_bytes()


def check_sdk(bin_dir: Path, fam: Family = H753) -> list[tuple[str, bool, str]]:
    """Consistency of one SDK shipped in the template project. No board, no CubeIDE."""
    rel = release(fam)
    N7RT = fam.n7rt
    checks: list[tuple[str, bool, str]] = []
    nm = _tool(bin_dir, "nm")
    man_path = N7RT / "manifest.json"
    if not man_path.is_file():
        return [("manifest present", False, f"{man_path} missing: run 'stm32lab sdk'")]
    man = json.loads(man_path.read_text(encoding="utf-8"))
    rt = man["runtime"]
    checks.append(("manifest = runtime release",
                   all(rt[k] == rel[k] for k in ("runtime_version", "build_id", "image_size", "image_sha256"))
                   and [man["abi"]["major"], man["abi"]["minor"]] == rel["abi"],
                   f"runtime {rt['runtime_version']} build {rt['build_id']} ABI {man['abi']['major']}.{man['abi']['minor']}"))
    names = [p.name for p in PROJECT.iterdir()]
    extra = sorted(str(p.relative_to(PROJECT)) for p in N7RT.rglob("*") if p.is_file()
                   and f"{fam.sdk_dir}/{p.relative_to(N7RT).as_posix()}" not in man["files"] and p.name != "manifest.json")
    checks.append((f"SDK folder is exactly {fam.sdk_dir}/ and holds only SDK files", fam.sdk_dir in names and not extra,
                   f"folders {sorted(names)}" + (f", extra {extra[:4]}" if extra else "")))
    bad = [f for f, digest in man["files"].items() if not (PROJECT / f).is_file()
           or sha256((PROJECT / f).read_bytes()) != digest]
    checks.append(("files match manifest SHA-256", not bad, f"{len(man['files'])} files" + (f", differ {bad}" if bad else "")))
    obj = N7RT / "lib" / "n7rt_runtime.o"
    image = runtime_bytes_from_object(bin_dir, obj)
    checks.append(("runtime bytes = validated S1.2 image", sha256(image) == rel["image_sha256"],
                   f"{len(image)} B, SHA-256 {sha256(image)[:16]}"))
    syms = _symbols(nm, obj)
    tag, major = version_tag(rel), rel["abi"][0]
    reset = int(rt["reset_vector"], 16)
    checks.append(("runtime object tags", syms.get(f"n7rt_runtime_{tag}", (None,))[0] == RUNTIME_BASE
                   and syms.get(f"n7rt_runtime_abi_{major}", (None,))[0] == major
                   and syms.get("n7rt_runtime_reset", (None,))[0] == reset
                   and struct.unpack_from("<I", image, 4)[0] == reset,
                   f"n7rt_runtime_{tag}, n7rt_runtime_abi_{major}, reset 0x{reset:08X}"))
    other = sorted(n for n, (_, kind) in syms.items() if kind not in "aA" and not n.startswith("__n7rt_runtime_"))
    checks.append(("runtime object exposes no internal symbol", not other, f"{other}" if other else "only tags"))
    glue = N7RT / "lib" / "libn7rt_glue.a"
    gsyms = _symbols(nm, glue, "--defined-only")
    want = {"n7rt_student_header", "n7rt_publish", "n7rt_reject", "n7rt_now_us", "n7rt_log", f"n7rt_glue_abi_{major}"}
    undefined = _undefined(nm, glue)
    allowed = {"student_init", "student_on_frame", "student_on_tick"}
    stray = sorted(u for u in undefined - allowed if not u.startswith("__n7rt_student_"))
    checks.append(("glue: header + services, ABI tag, no runtime reference", want <= set(gsyms) and not stray,
                   f"defines {sorted(want & set(gsyms))}" + (f", references {stray}" if stray else "")))
    debug = _run([_tool(bin_dir, "objdump"), "-h", glue])
    checks.append(("glue carries no debug information", ".debug_" not in debug, ""))
    header = (N7RT / "include" / "n7rt.h").read_text(encoding="utf-8")
    checks.append(("header ABI check", f"n7rt_glue_abi_{major}" in header and f"N7RT_ABI_MAJOR {major}" in header, ""))
    ld = (N7RT / "ld" / fam.linker_script).read_text(encoding="utf-8")
    checks.append(("linker script = release",
                   f"LENGTH = {rel['image_size']}" in ld and f"n7rt_runtime_{tag}" in ld
                   and f"n7rt_glue_abi_{major}" in ld and "ORIGIN = 0x08040000" in ld, ""))
    sources = sorted(str(p.relative_to(PROJECT)) for p in PROJECT.rglob("*")
                     if p.suffix.lower() in (".c", ".s", ".cpp", ".h") and p.is_file()
                     and p.relative_to(PROJECT).parts[0] not in ("student", CONFIG)
                     and p not in {f.n7rt / "include" / "n7rt.h" for f in FAMILIES})
    checks.append(("no runtime source in the project", not sources, f"{sources}" if sources else
                   "student/ + n7rt/include/n7rt.h only"))
    texts = [p for p in PROJECT.rglob("*") if p.is_file() and p.suffix.lower() in (".c", ".h", ".ld", ".txt", ".json",
                                                                                     ".launch", ".project", ".cproject")]
    blob_text = " ".join(s.decode() for s in re.findall(rb"[\x20-\x7e]{4,}", image))
    words = {w: re.compile(rf"(?<![a-z0-9]){w}(?![a-z0-9])", re.IGNORECASE) for w in FORBIDDEN}  # whole words
    hits = sorted({w for p in texts for w, rx in words.items()
                   if p.relative_to(PROJECT).parts[0] != CONFIG and rx.search(p.read_text(encoding="utf-8"))}
                  | {w for w, rx in words.items() if rx.search(blob_text)})
    checks.append(("no mission content (words scan)", not hits, f"found {hits}" if hits else
                   f"{len(texts)} text files + runtime strings"))
    return checks


# --- CubeIDE ---------------------------------------------------------------------------------------

def find_cubeide(st_root: Path) -> Path | None:
    """STM32CubeIDE command-line launcher: STM32LAB_CUBEIDE, else the usual install locations."""
    env = os.environ.get("STM32LAB_CUBEIDE")
    if env:
        return Path(env)
    candidates = sorted(st_root.glob("STM32CubeIDE*/STM32CubeIDE/stm32cubeidec.exe"), reverse=True)
    candidates += sorted(Path("/opt/st").glob("stm32cubeide_*/stm32cubeide"), reverse=True)
    candidates += [Path("/Applications/STM32CubeIDE.app/Contents/Eclipse/stm32cubeide")]
    return next((c for c in candidates if c.is_file()), None)


def cubeide_gcc_dir(cubeide: Path) -> Path | None:
    """GNU toolchain bundled with that CubeIDE (the one its managed build uses)."""
    found = sorted(cubeide.parent.glob(
        f"plugins/com.st.stm32cube.ide.mcu.externaltools.gnu-tools-for-stm32.*/tools/bin/arm-none-eabi-gcc{EXE}"),
        reverse=True)
    return found[0].parent if found else None


def cubeide_build(cubeide: Path, project: Path, workspace: Path) -> tuple[bool, Path, str]:
    """Clean build in a fresh workspace (the project is imported in place). Returns (ok, elf, log)."""
    shutil.rmtree(workspace, ignore_errors=True)
    build_dir = project / CONFIG
    if build_dir.resolve() != (project / "n7rt").resolve():  # never the SDK folder, whatever the case rules
        shutil.rmtree(build_dir, ignore_errors=True)
    # A headless build needs far less than the GUI: a small heap keeps it working on loaded PCs.
    vmargs = os.environ.get("STM32LAB_CUBEIDE_VMARGS", "-Xms64m -Xmx512m").split()
    cmd = [cubeide, "--launcher.suppressErrors", "-nosplash", "-application",
           "org.eclipse.cdt.managedbuilder.core.headlessbuild", "-data", workspace,
           "-import", project, "-cleanBuild", f"{project.name}/{CONFIG}", "-vmargs", *vmargs]
    proc = subprocess.run([str(c) for c in cmd], capture_output=True, text=True, timeout=1200)
    log = proc.stdout + proc.stderr
    elf = project / CONFIG / f"{project.name}.elf"
    errors = re.search(r"Build Finished\. (\d+) errors", log)
    ok = proc.returncode == 0 and elf.is_file() and (errors is None or errors.group(1) == "0")
    return ok, elf, log


def student_image_report(bin_dir: Path, elf: Path, fam: Family = H753) -> dict:
    """Placement and integrity of a student image (editor or CubeIDE output). Needs no board."""
    rel = release(fam)
    STUDENT_FLASH, STUDENT_RAM = fam.student_flash, fam.student_ram
    checks: list[tuple[str, bool, str]] = []
    with tempfile.TemporaryDirectory() as tmp_name:
        flat = Path(tmp_name) / "image.bin"
        _run([_tool(bin_dir, "objcopy"), "-O", "binary", elf, flat])
        image = flat.read_bytes()
    runtime = image[: rel["image_size"]]
    checks.append(("runtime at 0x08000000 = validated S1.2 image", sha256(runtime) == rel["image_sha256"],
                   f"{len(runtime)} B, SHA-256 {sha256(runtime)[:16]} (precompiled, not rebuilt)"))
    gap = image[rel["image_size"]: STUDENT_HEADER - RUNTIME_BASE]
    checks.append(("nothing between runtime end and 0x08040000", not any(gap), f"{len(gap)} B gap"))
    hdr = image[STUDENT_HEADER - RUNTIME_BASE: STUDENT_HEADER - RUNTIME_BASE + 64]
    fields = struct.unpack("<IHHIIIIIIIII", hdr[:44]) if len(hdr) == 64 else None
    ok_header = fields is not None and fields[0] == STUDENT_MAGIC and (fields[1], fields[2]) == tuple(rel["abi"])
    image_end = fields[11] if fields else 0
    checks.append(("student header at 0x08040000", ok_header,
                   f"magic {fields[0]:08X}, ABI {fields[1]}.{fields[2]}" if fields else "missing"))
    hooks_ok = fields is not None and all(STUDENT_FLASH[0] < f < STUDENT_FLASH[1] and f & 1 for f in fields[3:6])
    ram_ok = fields is not None and all(STUDENT_RAM[0] <= f <= STUDENT_RAM[1] for f in fields[7:11])
    checks.append((f"hooks in student flash, data/bss in {'DTCM' if fam is H753 else 'student SRAM'}", hooks_ok and ram_ok, ""))
    nm = _tool(bin_dir, "nm")
    syms = _symbols(nm, elf)
    outside = sorted(n for n, (a, kind) in syms.items() if kind in "TtRrDdBb"
                     and not n.startswith("__n7rt_runtime_")
                     and not (STUDENT_FLASH[0] <= a < STUDENT_FLASH[1] or STUDENT_RAM[0] <= a < STUDENT_RAM[1]))
    checks.append(("every student symbol in student regions", not outside, f"outside {outside[:6]}" if outside else
                   f"{len(syms)} symbols"))
    readelf = _run([_tool(bin_dir, "readelf"), "-h", elf])
    entry = re.search(r"Entry point address:\s+0x([0-9a-f]+)", readelf)
    reset = struct.unpack_from("<I", runtime, 4)[0]
    checks.append(("entry = runtime Reset_Handler", entry is not None and int(entry.group(1), 16) == reset,
                   f"0x{int(entry.group(1), 16):08X}" if entry else "?"))
    comment = _run([_tool(bin_dir, "readelf"), "-p", ".comment", elf])
    compilers = sorted(set(re.findall(r"GCC: \(([^)]*)\) ([0-9.]+)", comment)))
    flash_used = image_end - STUDENT_FLASH[0] if image_end else 0
    # PROVIDE(_end/end) emits a symbol only if it is referenced. Most decoders
    # do not call malloc, so those symbols can be absent even with live .bss.
    # The pinned SDK script reserves its heap after .data/.bss, aligned to 8.
    ram_markers = [syms.get(name) for name in ("__n7rt_student_data_end", "__n7rt_student_bss_end")]
    markers_ok = all(marker is not None and STUDENT_RAM[0] <= marker[0] <= STUDENT_RAM[1]
                     for marker in ram_markers)
    static_end = max([STUDENT_RAM[0], *(marker[0] for marker in ram_markers if marker is not None)])
    heap_start = (static_end + 7) & ~7
    ram_end = heap_start + HEAP_SIZE
    checks.append(("student RAM includes data/bss and the aligned SDK heap",
                   markers_ok and ram_end <= STUDENT_RAM[1],
                   f"{ram_end - STUDENT_RAM[0]} B including {HEAP_SIZE} B heap" if markers_ok
                   else "missing or invalid SDK RAM boundary symbols"))
    crc = zlib.crc32(image[STUDENT_HEADER - RUNTIME_BASE: image_end - RUNTIME_BASE]) if image_end else 0
    return {"ok": all(ok for _, ok, _ in checks), "checks": checks, "runtime_image": runtime,
            "student_crc32": crc, "student_flash_used": flash_used, "student_ram_used": ram_end - STUDENT_RAM[0],
            "image_sha256": sha256(image), "compilers": compilers}
