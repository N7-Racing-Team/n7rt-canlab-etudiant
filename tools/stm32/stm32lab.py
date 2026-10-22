#!/usr/bin/env python3
"""Command-line bench tool for the two NUCLEO-H753ZI boards of the CAN lab.

Every hardware action selects the probe by its ST-LINK serial number and checks
the MCU unique ID read over SWD against boards.json before touching the board.
A command never falls back to "the first probe found".

  python tools/stm32/stm32lab.py detect
  python tools/stm32/stm32lab.py deps
  python tools/stm32/stm32lab.py build [A|B|all]
  python tools/stm32/stm32lab.py flash A|B
  python tools/stm32/stm32lab.py smoke-test [A|B|all]
  python tools/stm32/stm32lab.py watch [A|B|all] [--seconds N]
  python tools/stm32/stm32lab.py build|flash ... --can-bus    (CAN bus test image)
  python tools/stm32/stm32lab.py can-link [--seconds N]

N7RT runtime image (any registered board, see docs/n7rt-runtime-v0.md):
  python tools/stm32/stm32lab.py build --runtime
  python tools/stm32/stm32lab.py flash|smoke-test|watch <board> --runtime
  python tools/stm32/stm32lab.py soak <board> [--minutes 10] [--rate 600]

N7RT CAN Lab application (app/, see app/README.md):
  python tools/stm32/stm32lab.py app-test <board> [--scenario functional|perf|showcase|[ide-]mission-<id>|
                                    [ide-]student-<id>|all] [--release] [--expect-verdict pass|fail|any]
      <id> = apps, wsf, sas or bps (mission scenarios: nominal verdict). mission-<id> needs N7RT_INSTRUCTOR_DIR and
      N7RT_E2E_INSTRUCTOR_CODE; student-<id> also N7RT_E2E_PACK (group pack file) and N7RT_E2E_GROUP_CODE (its run
      file is judged with n7rt-tool, built beside the app); ide-<scenario> also N7RT_E2E_DECODER_C
  python tools/stm32/stm32lab.py app-test --phys [--scenario phys|phys-perf|all] [--release]   (no board)
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import time
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
FW_DIR = ROOT / "firmware" / "stm32_h753_bringup"
THIRD_PARTY = FW_DIR / "third_party"
G0_FW_DIR = ROOT / "firmware" / "stm32_g0b1_runtime"  # N7RT runtime for NUCLEO-G0B1RE (instructor sources)
BOARDS_FILE = Path(__file__).resolve().with_name("boards.json")
# Machine-local boards (not part of the shared A/B bench, ignored by git and by the dashboard),
# same schema as boards.json. "images": ["runtime"] marks a board without a bring-up image.
LOCAL_BOARDS_FILE = BOARDS_FILE.with_name("boards.local.json")
N7RT_TOOLS = Path(__file__).resolve().parents[1] / "n7rt_link"
LOG_DIR = ROOT / "data" / "logs"

EXE = ".exe" if os.name == "nt" else ""
ST_ROOT = Path(os.environ.get("STM32LAB_ST_ROOT", "C:/ST"))

# Official ST sources, pinned to the exact commits shipped in STM32CubeH7 v1.13.0.
DEPS = (
    ("cmsis_core", "https://github.com/STMicroelectronics/cmsis_core.git",
     "v5.6.0_cm7", "04fe7bb2025811f10848ce14be810dde981934cf"),
    ("cmsis_device_h7", "https://github.com/STMicroelectronics/cmsis_device_h7.git",
     "v1.10.7", "8f922cdc7cc6de2344e75ddd657889f4ff761790"),
    ("stm32h7xx_hal_driver", "https://github.com/STMicroelectronics/stm32h7xx_hal_driver.git",
     "v1.11.6", "a1996eed9172b59887bafaaa0ea1816ea14d48b5"),
)
# G0B1 runtime: device header and startup only (no HAL); it shares cmsis_core above.
G0_DEPS = (
    ("cmsis_device_g0", "https://github.com/STMicroelectronics/cmsis-device-g0.git",
     "v1.4.5", "f576c24e123edf3332988ecd49512c0f35f85186"),
)

# tool -> (executable, STM32Cube bundle, STM32CubeCLT sub-directory, STM32CubeIDE plugin prefix)
TOOLS = {
    "gcc": ("arm-none-eabi-gcc", "gnu-tools-for-stm32", "GNU-tools-for-STM32/bin",
            "com.st.stm32cube.ide.mcu.externaltools.gnu-tools-for-stm32."),
    "cmake": ("cmake", "cmake", "CMake/bin", "com.st.stm32cube.ide.mcu.externaltools.cmake."),
    "ninja": ("ninja", "ninja", "Ninja/bin", "com.st.stm32cube.ide.mcu.externaltools.ninja."),
    "programmer": ("STM32_Programmer_CLI", "programmer", "STM32CubeProgrammer/bin",
                   "com.st.stm32cube.ide.mcu.externaltools.cubeprogrammer."),
}
# tool -> default directories of the standalone Windows installers, under %ProgramFiles% and %ProgramFiles(x86)%:
# (install directory, "*" = one per version, newest first; its bin directory). Arm GNU Toolchain (and its former
# name GNU Arm Embedded Toolchain), STM32CubeProgrammer.
INSTALLER_DIRS = {
    "gcc": (("Arm GNU Toolchain arm-none-eabi/*", "bin"), ("GNU Arm Embedded Toolchain/*", "bin")),
    "programmer": (("STMicroelectronics/STM32Cube/STM32CubeProgrammer", "bin"),),
}

EXPECTED_DEV_ID = 0x450          # STM32H742/743/750/753 line (bring-up bench boards)
UID_ADDR = "0x1FF1E800"          # 96-bit unique ID, RM0433
G0_DEV_ID, G0_UID_ADDR = 0x467, "0x1FFF7590"  # STM32G0B1/C1 (RM0444): the other supported family
VCP_BAUD = 115200
HEARTBEAT_MS = 1000
ANSI = re.compile(r"\x1b\[[0-9;]*[A-Za-z]")


class LabError(Exception):
    pass


# --------------------------------------------------------------------------- toolchain

def _version_key(name: str) -> tuple[int, ...]:
    return tuple(int(n) for n in re.findall(r"\d+", name))


def _newest_first(paths):
    return sorted(paths, key=lambda p: _version_key(p.name), reverse=True)


def _tool_candidates(key: str):
    exe, bundle, clt_sub, ide_prefix = TOOLS[key]
    exe += EXE
    override = os.environ.get(f"STM32LAB_{key.upper()}")
    if override:
        yield "env STM32LAB_" + key.upper(), Path(override)
    bundles = os.environ.get("CUBE_BUNDLE_PATH") or str(
        Path(os.environ.get("LOCALAPPDATA", "~")) / "stm32cube" / "bundles")
    for d in _newest_first(Path(bundles).expanduser().glob(f"{bundle}/*")):
        yield f"STM32Cube bundle {bundle} {d.name}", d / "bin" / exe
    for d in _newest_first(ST_ROOT.glob("STM32CubeCLT*")):
        yield d.name, d / clt_sub / exe
    for ide in _newest_first(ST_ROOT.glob("STM32CubeIDE*")):
        for plugin in _newest_first((ide / "STM32CubeIDE" / "plugins").glob(ide_prefix + "*")):
            yield f"{ide.name} plugin", plugin / "tools" / "bin" / exe
    found = shutil.which(exe)
    if found:
        yield "PATH", Path(found)
    found = shutil.which(exe, path=_registry_path()) if os.name == "nt" else None
    if found:
        yield "PATH (registry)", Path(found)
    roots = {os.environ.get(v) for v in ("ProgramFiles", "ProgramFiles(x86)", "ProgramW6432")} - {None, ""}
    for root in sorted(roots):
        for pattern, bin_dir in INSTALLER_DIRS.get(key, ()):
            for d in _newest_first(Path(root).glob(pattern)):
                yield f"{Path(root).name}/{pattern.split('/')[0]}", d / bin_dir / exe


def _registry_path() -> str:
    """The user and system PATH as currently saved: a PATH edited after this process (or the application that
    started it) was launched is seen without a restart."""
    import winreg
    parts = []
    for hive, key in ((winreg.HKEY_LOCAL_MACHINE, r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment"),
                      (winreg.HKEY_CURRENT_USER, r"Environment")):
        try:
            with winreg.OpenKey(hive, key) as k:
                parts.append(os.path.expandvars(str(winreg.QueryValueEx(k, "Path")[0])))
        except OSError:
            pass
    return os.pathsep.join(parts)


_tool_cache: dict[str, tuple[str, Path] | None] = {}


def find_tool(key: str) -> tuple[str, Path] | None:
    if key not in _tool_cache:
        _tool_cache[key] = next(((s, p) for s, p in _tool_candidates(key) if p.is_file()), None)
    return _tool_cache[key]


def require_tool(key: str) -> Path:
    found = find_tool(key)
    if not found:
        raise LabError(f"{TOOLS[key][0]} not found (see docs/stm32-toolchain-windows.md)")
    return found[1]


def tool_version(key: str, path: Path) -> str:
    try:
        out = subprocess.run([str(path), "--version"], capture_output=True, text=True,
                             encoding="utf-8", errors="replace", timeout=30).stdout
    except (OSError, subprocess.TimeoutExpired) as exc:
        return f"? ({exc})"
    out = ANSI.sub("", out)
    if key == "programmer":
        m = re.search(r"version:\s*(\S+)", out)
        return m.group(1) if m else "?"
    return next((line.strip() for line in out.splitlines() if line.strip()), "?")


# --------------------------------------------------------------------------- programmer

def programmer(*args: str, timeout: int = 120) -> tuple[int, str]:
    cli = require_tool("programmer")
    proc = subprocess.run([str(cli), *args], capture_output=True, text=True,
                          encoding="utf-8", errors="replace", timeout=timeout)
    return proc.returncode, ANSI.sub("", proc.stdout + proc.stderr)


def _tail(text: str, n: int = 12) -> str:
    lines = [line for line in text.splitlines() if line.strip()]
    return "\n".join("    " + line for line in lines[-n:])


def list_probes() -> list[dict]:
    _, out = programmer("-l", "st")
    probes = []
    for block in re.split(r"ST-Link Probe \d+ :", out)[1:]:
        sn = re.search(r"ST-LINK SN\s*:\s*(\S+)", block)
        fw = re.search(r"ST-LINK FW\s*:\s*(\S+)", block)
        board = re.search(r"Board Name\s*:\s*(\S+)", block)
        if sn:
            probes.append({"sn": sn.group(1), "fw": fw.group(1) if fw else "?",
                           "board": board.group(1) if board else "?"})
    return probes


def read_target(sn: str) -> dict:
    """Connect without reset/halt and read the device ID and 96-bit UID (address of the H7, else the G0B1)."""
    addr = UID_ADDR
    code, out = programmer("-c", "port=SWD", f"sn={sn}", "mode=HOTPLUG", "-r32", addr, "12")
    dev = re.search(r"Device ID\s*:\s*(0x[0-9A-Fa-f]+)", out)
    if dev and int(dev.group(1), 16) == G0_DEV_ID:
        addr = G0_UID_ADDR
        code, out = programmer("-c", "port=SWD", f"sn={sn}", "mode=HOTPLUG", "-r32", addr, "12")
        dev = re.search(r"Device ID\s*:\s*(0x[0-9A-Fa-f]+)", out)
    rev = re.search(r"Revision ID\s*:\s*(.+)", out)
    uid = re.search(addr + r"\s*:\s*([0-9A-Fa-f]{8})\s+([0-9A-Fa-f]{8})\s+([0-9A-Fa-f]{8})",
                    out, re.IGNORECASE)
    if code != 0 or not dev or not uid:
        raise LabError(f"SWD read failed on ST-LINK {sn} (exit {code}):\n{_tail(out)}")
    return {"dev_id": int(dev.group(1), 16), "rev": rev.group(1).strip() if rev else "?",
            "uid": "-".join(g.upper() for g in uid.groups())}


def com_ports_by_sn() -> dict[str, str]:
    try:
        from serial.tools import list_ports
    except ImportError as exc:
        raise LabError("pyserial missing: python -m pip install pyserial") from exc
    return {p.serial_number: p.device for p in list_ports.comports() if p.serial_number}


# --------------------------------------------------------------------------- registry

def load_boards() -> dict[str, dict]:
    boards = json.loads(BOARDS_FILE.read_text(encoding="utf-8"))["boards"]
    if LOCAL_BOARDS_FILE.is_file():
        local = json.loads(LOCAL_BOARDS_FILE.read_text(encoding="utf-8"))["boards"]
        clash = sorted(set(local) & set(boards))
        if clash:
            raise LabError(f"{LOCAL_BOARDS_FILE.name} redefines board(s) {clash} of {BOARDS_FILE.name}")
        boards = {**boards, **local}
    return boards


def has_bringup(key: str) -> bool:
    return "bringup" in board_entry(key).get("images", ["bringup", "runtime"])


def require_bringup(key: str) -> None:
    if not has_bringup(key):
        raise LabError(f"board {key} is registered for the N7RT runtime image only: add --runtime")


def board_family(key: str):
    """Board family of a registered board: "family" in the registry, NUCLEO-H753ZI when absent."""
    try:
        return sdk.family(board_entry(key).get("family", "h753"))
    except sdk.SdkError as exc:
        raise LabError(f"board {key}: {exc}") from exc


def board_entry(key: str) -> dict:
    boards = load_boards()
    if key not in boards:
        raise LabError(f"unknown board '{key}' (known: {', '.join(boards)})")
    return boards[key]


def guard(key: str) -> dict:
    """Refuse to go further unless the registered probe and MCU are the ones connected."""
    entry = board_entry(key)
    sn = entry["stlink_sn"]
    connected = [p["sn"] for p in list_probes()]
    if sn not in connected:
        raise LabError(f"board {key}: ST-LINK {sn} not connected (connected: {connected or 'none'})")
    target = read_target(sn)
    expected = board_family(key).dev_id
    if target["dev_id"] != expected:
        raise LabError(f"board {key}: device ID 0x{target['dev_id']:03X}, expected 0x{expected:03X}")
    if target["uid"] != entry["mcu_uid"].upper():
        raise LabError(f"board {key}: MCU UID {target['uid']} behind ST-LINK {sn} "
                       f"does not match boards.json ({entry['mcu_uid']})")
    return target


# --------------------------------------------------------------------------- image

def preset(key: str, can_bus: bool = False) -> str:
    """CMake preset: board-a / board-b (loopback only), board-a-can / board-b-can (FDCAN1 on the bus)."""
    return f"board-{key.lower()}" + ("-can" if can_bus else "")


def image_paths(key: str, can_bus: bool = False) -> tuple[Path, Path]:
    base = FW_DIR / "build" / preset(key, can_bus) / f"h753_bringup_board_{key.lower()}"
    return base.with_suffix(".elf"), base.with_suffix(".bin")


def check_image(key: str, can_bus: bool = False) -> Path:
    elf, binary = image_paths(key, can_bus)
    flag = " --can-bus" if can_bus else ""
    if not elf.is_file() or not binary.is_file():
        raise LabError(f"no image for board {key}: run 'build {key}{flag}' first ({elf})")
    data = binary.read_bytes()
    tags = set(re.findall(rb"STM32LAB_BOARD=([A-Z])\x00", data))
    if tags != {key.encode()}:
        found = sorted(t.decode() for t in tags) or "none"
        raise LabError(f"{binary.name} carries board tag {found}, expected {key}: refusing to flash")
    variant = set(re.findall(rb"STM32LAB_CAN=([a-z]+)\x00", data))
    expected = b"bus" if can_bus else b"loopback"
    if variant != {expected}:
        found = sorted(v.decode() for v in variant) or "none"
        raise LabError(f"{binary.name} carries CAN variant {found}, expected {expected.decode()}: refusing to flash")
    return elf


# --------------------------------------------------------------------------- N7RT runtime image

RUNTIME_PRESET = "runtime"
FLASH_BASE = 0x08000000


@dataclass(frozen=True)
class RuntimeBuild:
    """Where one family's runtime is built, and the layout its image must have."""
    family: str
    src: Path
    tag: bytes
    fixed_symbols: dict
    student_regions: tuple
    runtime_regions: tuple

    @property
    def build(self) -> Path:
        return self.src / "build" / RUNTIME_PRESET

    @property
    def elf(self) -> Path:
        return self.build / "n7rt_runtime.elf"

    @property
    def bin(self) -> Path:
        return self.build / "n7rt_runtime.bin"

    @property
    def map(self) -> Path:
        return self.build / "n7rt_runtime.map"

    @property
    def student_lib(self) -> Path:
        return self.build / "libn7rt_student.a"


# docs/can-lab-architecture.md 4.2, runtime/N7RT_H753_RUNTIME.ld and stm32_g0b1_runtime/N7RT_G0B1_RUNTIME.ld
RUNTIME_BUILDS = {
    "h753": RuntimeBuild("h753", FW_DIR, b"STM32LAB_IMAGE=n7rt_runtime\x00",
                         {"g_pfnVectors": 0x08000000, "n7rt_api_table": 0x08000400,
                          "n7rt_student_header": 0x08040000, "_estack": 0x24004000},
                         ((0x08040000, 0x08100000), (0x20000000, 0x20020000)),
                         ((0x08000000, 0x08040000), (0x24000000, 0x24080000))),
    "g0b1": RuntimeBuild("g0b1", G0_FW_DIR, b"STM32LAB_IMAGE=n7rt_runtime_g0b1\x00",
                         {"g_pfnVectors": 0x08000000, "n7rt_api_table": 0x08000400,
                          "n7rt_student_header": 0x08040000, "_estack": 0x20002000},
                         ((0x08040000, 0x08080000), (0x2000C000, 0x20024000)),
                         ((0x08000000, 0x08040000), (0x20000000, 0x2000C000))),
}
H753_RUNTIME = RUNTIME_BUILDS["h753"]
# H753 names kept for the tools written before the G0B1.
RUNTIME_BUILD, RUNTIME_ELF, RUNTIME_BIN = H753_RUNTIME.build, H753_RUNTIME.elf, H753_RUNTIME.bin
RUNTIME_MAP, STUDENT_LIB, RUNTIME_TAG = H753_RUNTIME.map, H753_RUNTIME.student_lib, H753_RUNTIME.tag
FIXED_SYMBOLS = H753_RUNTIME.fixed_symbols
STUDENT_REGIONS, RUNTIME_REGIONS = H753_RUNTIME.student_regions, H753_RUNTIME.runtime_regions


def _nm(*args: str) -> str:
    nm = require_tool("gcc").with_name("arm-none-eabi-nm" + EXE)
    proc = subprocess.run([str(nm), *args], capture_output=True, text=True)
    if proc.returncode != 0:
        raise LabError(f"nm {' '.join(args)} failed: {proc.stderr.strip()[:200]}")
    return proc.stdout


def _nm_symbols(text: str) -> dict[str, tuple[int, str]]:
    out = {}
    for line in text.splitlines():
        parts = line.split()
        if len(parts) == 3 and re.fullmatch(r"[0-9a-fA-F]+", parts[0]):
            out[parts[2]] = (int(parts[0], 16), parts[1])
    return out


def _undefined(text: str) -> set[str]:
    return {line.split()[-1] for line in text.splitlines() if line.strip().startswith("U ")}


def _in(addr: int, regions) -> bool:
    return any(lo <= addr < hi for lo, hi in regions)


def map_student_sections(text: str) -> list[tuple[str, int, int]]:
    """(section, address, size) of every non-empty input section of libn7rt_student.a in the link map."""
    body = text.split("Linker script and memory map", 1)[-1]
    out, pending = [], None
    for line in body.splitlines():
        m = re.match(r"^ (\.\S+)\s+0x([0-9a-f]+)\s+0x([0-9a-f]+)\s+(\S.*)$", line)
        if m:
            name, addr, size, origin = m.group(1), int(m.group(2), 16), int(m.group(3), 16), m.group(4)
        else:
            m = re.match(r"^\s+0x([0-9a-f]+)\s+0x([0-9a-f]+)\s+(\S.*)$", line) if pending else None
            if m:
                name, addr, size, origin = pending, int(m.group(1), 16), int(m.group(2), 16), m.group(3)
            else:
                named = re.match(r"^ (\.\S+)$", line)
                pending = named.group(1) if named else None
                continue
        pending = None
        if "libn7rt_student.a(" in origin and size > 0 and not name.startswith(NOT_LOADED):
            out.append((name, addr, size))
    return out


NOT_LOADED = (".debug", ".comment", ".ARM.attributes", ".note")  # never in target memory


def runtime_layout_report(rb: RuntimeBuild = H753_RUNTIME) -> dict:
    """Runtime <-> student separation, checked on the built artefacts (no board needed)."""
    checks: list[tuple[str, bool, str]] = []
    RUNTIME_BUILD, STUDENT_LIB = rb.build, rb.student_lib
    STUDENT_REGIONS, RUNTIME_REGIONS = rb.student_regions, rb.runtime_regions
    syms = _nm_symbols(_nm("-n", str(rb.elf)))
    for name, addr in rb.fixed_symbols.items():
        got = syms.get(name, (None, ""))[0]
        checks.append((f"addr {name}", got == addr, f"0x{got:08X}" if got is not None else "missing"))
    image_end = syms.get("__n7rt_image_end", (0, ""))[0]
    checks.append(("runtime image <= 256 KB", FLASH_BASE < image_end <= 0x08040000,
                   f"ends at 0x{image_end:08X} ({image_end - FLASH_BASE} B)"))
    student_defined = {n for n, (_, kind) in _nm_symbols(_nm("--defined-only", str(STUDENT_LIB))).items()
                       if kind in "TtRrDdBb"}
    misplaced = sorted(n for n in student_defined if n in syms and not _in(syms[n][0], STUDENT_REGIONS))
    checks.append(("student symbols in student regions", bool(student_defined) and not misplaced,
                   f"{len(student_defined)} symbols" + (f", misplaced {misplaced}" if misplaced else "")))
    foreign = sorted(u for u in _undefined(_nm("-u", str(STUDENT_LIB))) - student_defined
                     if not u.startswith("__n7rt_student_"))
    checks.append(("student image self-contained", not foreign,
                   "only ABI linker symbols referenced" if not foreign else f"references {foreign}"))
    objects = sorted((RUNTIME_BUILD / "CMakeFiles" / "n7rt_runtime.dir").rglob("*.obj")) + \
        sorted((RUNTIME_BUILD / "CMakeFiles" / "n7rt_runtime.dir").rglob("*.o"))
    # Globals only: a helper the student image carries as a local copy (libgcc on the M0+) is not a link to it.
    student_global = {n for n, (_, kind) in _nm_symbols(_nm("--defined-only", str(STUDENT_LIB))).items()
                      if kind in "TRDB"}
    leaks = sorted(set().union(*(_undefined(_nm("-u", str(o))) for o in objects)) & student_global) \
        if objects else []
    checks.append(("runtime ignores student symbols", bool(objects) and not leaks,
                   f"{len(objects)} runtime objects" + (f", reference {leaks}" if leaks else "")))
    sections = map_student_sections(rb.map.read_text(encoding="utf-8", errors="replace"))
    outside = [f"{n}@0x{a:08X}" for n, a, _ in sections if not _in(a, STUDENT_REGIONS)]
    checks.append(("student input sections (map)", bool(sections) and not outside,
                   f"{len(sections)} sections" + (f", outside {outside}" if outside else "")))
    stray = sorted(n for n, (a, kind) in syms.items() if kind in "TtRrDdBb" and n not in student_defined
                   and not n.startswith("__n7rt_student") and not _in(a, RUNTIME_REGIONS))
    checks.append(("runtime symbols in runtime regions", not stray, f"outside {stray[:5]}" if stray else ""))
    return {"ok": all(ok for _, ok, _ in checks), "checks": checks, "image_end": image_end}


def check_runtime_image(verbose: bool = False, rb: RuntimeBuild = H753_RUNTIME) -> dict:
    if not rb.elf.is_file() or not rb.bin.is_file():
        raise LabError(f"no runtime image: run 'build --runtime --family {rb.family}' first ({rb.elf})")
    if rb.tag not in rb.bin.read_bytes():
        raise LabError(f"{rb.bin.name} has no {rb.tag[:-1].decode()} tag: refusing to flash")
    report = runtime_layout_report(rb)
    if verbose or not report["ok"]:
        for name, ok, detail in report["checks"]:
            print(f"   {'PASS' if ok else 'FAIL'}  {name:<36} {detail}")
    if not report["ok"]:
        raise LabError("runtime layout check failed (see above)")
    image = rb.bin.read_bytes()[: report["image_end"] - FLASH_BASE]
    report["image"] = image
    report["image_sha256"] = hashlib.sha256(image).hexdigest()
    return report


def runtime_tools():
    """tools/n7rt_link (reference codec, hardware checks), imported on demand."""
    if str(N7RT_TOOLS) not in sys.path:
        sys.path.insert(0, str(N7RT_TOOLS))
    import runtime_check
    return runtime_check


def runtime_port(key: str) -> tuple[str, str]:
    sn = board_entry(key)["stlink_sn"]
    port = com_ports_by_sn().get(sn)
    if port is None:
        raise LabError(f"board {key}: no virtual COM port for ST-LINK {sn}")
    return sn, port


# --------------------------------------------------------------------------- N7RT student SDK (S1.3)

import n7rt_sdk as sdk  # noqa: E402  (tools/stm32/n7rt_sdk.py)

STUDENT_WORKSPACE = sdk.SDK_DIR / "build" / "cubeide_workspace"


def _print_checks(checks) -> None:
    for name, ok, detail in checks:
        print(f"   {'PASS' if ok else 'FAIL'}  {name:<48} {detail}")


def require_cubeide() -> Path:
    cubeide = sdk.find_cubeide(ST_ROOT)
    if cubeide is None:
        raise LabError("STM32CubeIDE not found: set STM32LAB_CUBEIDE to its stm32cubeidec executable")
    return cubeide


def student_elf() -> Path:
    return sdk.PROJECT / sdk.CONFIG / f"{sdk.PROJECT_NAME}.elf"


def check_student_image(verbose: bool = False) -> dict:
    """SDK consistency + placement/integrity of the image built by CubeIDE (H753); refuse on any failure."""
    elf = student_elf()
    if not elf.is_file():
        raise LabError(f"no student image: run 'build --student' first ({elf})")
    bin_dir = require_tool("gcc").parent
    checks = sdk.check_sdk(bin_dir)
    report = sdk.student_image_report(bin_dir, elf)
    checks += report["checks"]
    if verbose or not all(ok for _, ok, _ in checks):
        _print_checks(checks)
    if not all(ok for _, ok, _ in checks):
        raise LabError("student image or SDK check failed (see above)")
    return report


def cmd_sdk(args) -> int:
    bin_dir = require_tool("gcc").parent
    families = sdk.FAMILIES if args.family == "all" else (sdk.family(args.family),)
    ok = True
    for fam in families:
        rb = RUNTIME_BUILDS[fam.key]
        if not args.check:
            rt = check_runtime_image(rb=rb)
            build_id = re.search(r'N7RT_BUILD_ID_HEX "([0-9a-f]+)"',
                                 (rb.build / "n7rt_build_id.h").read_text(encoding="utf-8"))
            print(f"== sdk {fam.board}: runtime build {build_id.group(1) if build_id else '?'} -> "
                  f"{fam.n7rt.relative_to(ROOT)}")
            try:
                man = sdk.generate(bin_dir, rt["image"], build_id.group(1) if build_id else "", rb.student_lib, fam)
            except sdk.SdkError as exc:
                raise LabError(str(exc)) from exc
            print(f"   SDK {man['sdk_version']}: runtime {man['runtime']['runtime_version']} "
                  f"({man['runtime']['image_size']} B, SHA-256 {man['runtime']['image_sha256'][:16]}), "
                  f"ABI {man['abi']['major']}.{man['abi']['minor']}, reset {man['runtime']['reset_vector']}")
        print(f"== sdk check {fam.board} ({fam.sdk_dir}/)")
        checks = sdk.check_sdk(bin_dir, fam)
        _print_checks(checks)
        ok = ok and all(passed for _, passed, _ in checks)
    if ok and not args.check:
        print(f"   distributable: {sdk.make_dist().relative_to(ROOT)}")
    print(f"== sdk: {'OK' if ok else 'FAILED'}")
    return 0 if ok else 1


def _student_build() -> int:
    cubeide = require_cubeide()
    print(f"== build student project with STM32CubeIDE ({cubeide.parent.parent.name}), headless clean build")
    ok, elf, log = sdk.cubeide_build(cubeide, sdk.PROJECT, STUDENT_WORKSPACE)
    LOG_DIR.mkdir(parents=True, exist_ok=True)
    log_path = LOG_DIR / f"cubeide_student_build_{datetime.now():%Y%m%d-%H%M%S}.log"
    log_path.write_text(log, encoding="utf-8")
    for line in log.splitlines():
        if re.search(r"Memory region|_FLASH:|_RAM:|error|warning:|Build Finished|overflowed", line):
            print(f"   {line.strip()}")
    print(f"   CubeIDE log: {log_path.relative_to(ROOT)}")
    if not ok:
        raise LabError("CubeIDE build failed")
    report = check_student_image(verbose=True)
    print(f"   image {elf.relative_to(ROOT)}: student flash {report['student_flash_used']} B, "
          f"student RAM {report['student_ram_used']} B (heap included), SHA-256 {report['image_sha256'][:16]}")
    print(f"   compilers in the image: {', '.join(f'{v} ({n})' for n, v in report['compilers'])}")
    return 0


def _student_smoke(key: str) -> bool:
    rc = runtime_tools()
    image = check_student_image()
    target = guard(key)
    sn, port = runtime_port(key)
    print(f"== smoke-test board {key} (student image built by CubeIDE): ST-LINK {sn}, {port}, "
          f"MCU UID {target['uid']}")

    def reset_board() -> None:
        code, out = programmer("-c", "port=SWD", f"sn={sn}", "mode=HOTPLUG", "-rst")
        if code != 0:
            raise LabError(f"reset via ST-LINK {sn} failed (exit {code}):\n{_tail(out)}")

    report = rc.student_smoke(port, reset_board, image["runtime_image"], image["student_crc32"],
                              board_entry(key)["mcu_uid"])
    report.update({"board": key, "stlink_sn": sn, "image_sha256": image["image_sha256"],
                   "compilers": image["compilers"]})
    LOG_DIR.mkdir(parents=True, exist_ok=True)
    log = LOG_DIR / f"student_smoke_{key.lower()}_{datetime.now():%Y%m%d-%H%M%S}.json"
    log.write_text(json.dumps(report, indent=2, default=str), encoding="utf-8")
    passed = sum(c["ok"] for c in report["checks"])
    print(f"   report: {log.relative_to(ROOT)}")
    print(f"   board {key}: {'PASS' if report['ok'] else 'FAIL'} ({passed}/{len(report['checks'])})")
    return report["ok"]


# --------------------------------------------------------------------------- commands

def detect_report() -> dict:
    """Toolchain, probes and registry match. 'status' per probe: ok | uid_mismatch | unregistered | swd_error."""
    tools = {}
    for key in TOOLS:
        found = find_tool(key)
        tools[key] = {"exe": TOOLS[key][0], "source": found[0] if found else None,
                      "path": str(found[1]) if found else None,
                      "version": tool_version(key, found[1]) if found else None}
    report = {"tools": tools, "probes": [], "missing_boards": [], "ok": False}
    if not find_tool("programmer"):
        return report

    boards = load_boards()
    by_sn = {b["stlink_sn"]: k for k, b in boards.items()}
    ports = com_ports_by_sn()
    ok = True
    for p in list_probes():
        key = by_sn.get(p["sn"])
        entry = {"sn": p["sn"], "fw": p["fw"], "board_name": p["board"], "com": ports.get(p["sn"]),
                 "board": key, "node": boards[key]["node"] if key else None, "mcu": None, "error": None}
        try:
            t = read_target(p["sn"])
            entry["mcu"] = {"dev_id": f"0x{t['dev_id']:03X}", "rev": t["rev"], "uid": t["uid"]}
        except LabError as exc:
            entry["error"] = str(exc).splitlines()[0]
        if entry["error"]:
            entry["status"] = "swd_error"
        elif key is None:
            entry["status"] = "unregistered"
        elif entry["mcu"]["uid"] != boards[key]["mcu_uid"].upper():
            entry["status"] = "uid_mismatch"
        else:
            entry["status"] = "ok"
        ok = ok and entry["status"] in ("ok", "unregistered")
        report["probes"].append(entry)
    connected = {p["sn"] for p in report["probes"]}
    # Only the bring-up bench boards are expected; a runtime-only (local) board is optional.
    report["missing_boards"] = [k for k, b in boards.items()
                                if b["stlink_sn"] not in connected and has_bringup(k)]
    report["ok"] = ok and not report["missing_boards"]
    return report


def cmd_detect(args) -> int:
    report = detect_report()
    if args.json:
        print(json.dumps(report, indent=2))
        return 0 if report["ok"] else 1

    print("Toolchain")
    for tool in report["tools"].values():
        if tool["path"]:
            print(f"  {tool['exe']:<22} {tool['version']}")
            print(f"  {'':<22} [{tool['source']}] {tool['path']}")
        else:
            print(f"  {tool['exe']:<22} NOT FOUND")
    if not report["tools"]["programmer"]["path"]:
        return 1

    boards = load_boards()
    print(f"\nST-LINK probes ({len(report['probes'])})")
    for p in report["probes"]:
        mcu = p["mcu"]
        mcu_text = f"dev {mcu['dev_id']} {mcu['rev']} UID {mcu['uid']}" if mcu else f"SWD read failed: {p['error']}"
        verdict = {
            "unregistered": "not in boards.json",
            "uid_mismatch": f"Board {p['board']}: UID MISMATCH vs boards.json",
        }.get(p["status"], f"Board {p['board']} ({p['node']})" if p["board"] else "not in boards.json")
        print(f"  ST-LINK {p['sn']}  FW {p['fw']}  {p['board_name']}  {p['com'] or 'no VCP'}")
        print(f"      MCU {mcu_text}")
        print(f"      -> {verdict}")
    for key in report["missing_boards"]:
        print(f"  Board {key} ({boards[key]['stlink_sn']}) NOT CONNECTED")
    return 0 if report["ok"] else 1


GPIOB_IDR = 0x58020410   # PB0 = LD1 (green), RM0433
LED_ON_MS = 120          # firmware: FLASH_ON_MS
EDGE_MARGIN_MS = 3       # tick and pin are read a few ms apart: ignore samples this close to an edge


def _symbol_address(elf: Path, name: str) -> int:
    nm = require_tool("gcc").with_name("arm-none-eabi-nm" + EXE)
    out = subprocess.run([str(nm), str(elf)], capture_output=True, text=True).stdout
    match = re.search(rf"^([0-9a-fA-F]+) \w {re.escape(name)}$", out, re.MULTILINE)
    if not match:
        raise LabError(f"symbol {name} not found in {elf.name}")
    return int(match.group(1), 16)


def cmd_led_check(args) -> int:
    """Measure the LD1 pattern at the pin: PB0 level paired with the firmware millisecond tick over SWD."""
    key = args.board
    entry = board_entry(key)
    expected = int(entry["led_hz"])
    elf = check_image(key, args.can_bus)
    guard(key)
    tick_addr = _symbol_address(elf, "uwTick")
    sn = entry["stlink_sn"]
    print(f"== led-check board {key}: sampling PB0 + uwTick over SWD for {args.seconds:.0f} s (no halt)")
    samples: list[tuple[int, int, float]] = []
    deadline = time.monotonic() + args.seconds
    while time.monotonic() < deadline:
        code, out = programmer("-c", "port=SWD", f"sn={sn}", "mode=HOTPLUG",
                               "-r32", f"0x{tick_addr:08X}", "1", "-r32", f"0x{GPIOB_IDR:08X}", "1")
        tick = re.search(rf"0x{tick_addr:08X}\s*:\s*([0-9A-Fa-f]{{8}})", out, re.IGNORECASE)
        idr = re.search(rf"0x{GPIOB_IDR:08X}\s*:\s*([0-9A-Fa-f]{{8}})", out, re.IGNORECASE)
        if code != 0 or not tick or not idr:
            raise LabError(f"SWD sample failed (exit {code}):\n{_tail(out)}")
        samples.append((int(tick.group(1), 16), int(idr.group(1), 16) & 1, time.monotonic()))

    ticks = [s[0] for s in samples]
    span_host = (samples[-1][2] - samples[0][2]) * 1000
    span_tick = ticks[-1] - ticks[0]
    if len(samples) < 20 or any(b <= a for a, b in zip(ticks, ticks[1:])) or abs(span_tick - span_host) > 0.1 * span_host:
        raise LabError(f"uwTick does not advance like a 1 ms tick ({len(samples)} samples, {span_tick} ms vs "
                       f"{span_host:.0f} ms): is the running image the one built by 'build {key}'?")
    verdicts = {}
    for hz in (1, 2):
        period = 1000 // hz
        usable = [(t, lvl) for t, lvl, _ in samples
                  if min(t % period, abs(t % period - LED_ON_MS), period - t % period) > EDGE_MARGIN_MS]
        verdicts[hz] = sum(lvl != int(t % period < LED_ON_MS) for t, lvl in usable)
    on = sum(lvl for _, lvl, _ in samples)
    late_on = sum(lvl for t, lvl, _ in samples if 500 <= t % 1000 < 500 + LED_ON_MS)
    matched = [hz for hz, bad in verdicts.items() if bad == 0]
    ok = matched == [expected] and on >= 2
    print(f"   {len(samples)} samples over {span_tick / 1000:.1f} s of target time, PB0 high in {on} "
          f"({late_on} in the 500-620 ms window)")
    for hz, bad in verdicts.items():
        print(f"   model {hz} Hz: {bad} contradicting sample(s)")
    print(f"   board {key}: {'PASS' if ok else 'FAIL'} - measured {matched or 'no model'}, expected {expected} Hz")
    return 0 if ok else 1


def _git(*args: str, cwd: Path | None = None) -> str:
    proc = subprocess.run(["git", *args], cwd=cwd, capture_output=True, text=True)
    if proc.returncode != 0:
        raise LabError(f"git {' '.join(args)} failed:\n{_tail(proc.stderr)}")
    return proc.stdout.strip()


def cmd_deps(_args) -> int:
    pinned = [(THIRD_PARTY, dep) for dep in DEPS]
    if G0_FW_DIR.is_dir():
        pinned += [(G0_FW_DIR / "third_party", dep) for dep in G0_DEPS]
    for folder, (name, url, tag, sha) in pinned:
        folder.mkdir(parents=True, exist_ok=True)
        dest = folder / name
        if not (dest / ".git").exists():
            print(f"  {name}: cloning {url} @ {tag}")
            _git("-c", "advice.detachedHead=false", "clone", "--quiet", "--depth", "1",
                 "--branch", tag, url, str(dest))
        head = _git("rev-parse", "HEAD", cwd=dest)
        if head != sha:
            raise LabError(f"{name}: HEAD {head} != pinned {sha} ({tag}); delete {dest} and rerun")
        print(f"  {name:<22} {tag:<12} {sha[:12]}  OK")
    return 0


def cmd_build(args) -> int:
    if args.student:
        return _student_build()
    missing = [name for name, *_ in DEPS if not (THIRD_PARTY / name / ".git").exists()]
    if missing:
        raise LabError(f"missing ST sources {missing}: run 'deps' first")
    cmake, gcc, ninja = require_tool("cmake"), require_tool("gcc"), require_tool("ninja")
    env = os.environ.copy()
    env["PATH"] = os.pathsep.join([str(gcc.parent), str(ninja.parent), str(cmake.parent), env["PATH"]])
    size = gcc.with_name("arm-none-eabi-size" + EXE)
    if args.runtime:
        rb = RUNTIME_BUILDS[args.family]
        if rb.family != "h753" and not (rb.src / "third_party" / G0_DEPS[0][0] / ".git").exists():
            raise LabError(f"missing ST sources for {rb.family}: run 'deps' first")
        print(f"== build N7RT runtime {sdk.family(rb.family).board} (preset {RUNTIME_PRESET})", flush=True)
        for cmd in ([cmake, "--preset", RUNTIME_PRESET], [cmake, "--build", "--preset", RUNTIME_PRESET]):
            if subprocess.run([str(c) for c in cmd], cwd=rb.src, env=env).returncode != 0:
                raise LabError("runtime build failed")
        subprocess.run([str(size), str(rb.elf)], env=env)
        report = check_runtime_image(verbose=True, rb=rb)
        build_id = re.search(r'N7RT_BUILD_ID_HEX "([0-9a-f]+)"',
                             (rb.build / "n7rt_build_id.h").read_text(encoding="utf-8"))
        print(f"   image {rb.elf.relative_to(ROOT)}  build_id {build_id.group(1) if build_id else '?'}")
        print(f"   runtime image {len(report['image'])} B, SHA-256 {report['image_sha256']}")
        return 0
    for key in _keys(args.board):
        require_bringup(key)
        name = preset(key, args.can_bus)
        print(f"== build board {key} (preset {name})", flush=True)
        for cmd in ([cmake, "--preset", name], [cmake, "--build", "--preset", name]):
            if subprocess.run([str(c) for c in cmd], cwd=FW_DIR, env=env).returncode != 0:
                raise LabError(f"build failed for board {key}")
        elf, _ = image_paths(key, args.can_bus)
        subprocess.run([str(size), str(elf)], env=env)
        check_image(key, args.can_bus)
        print(f"   image {elf.relative_to(ROOT)}  (board tag {key}, CAN "
              f"{'bus' if args.can_bus else 'loopback'} verified)")
    return 0


def cmd_flash(args) -> int:
    key = args.board
    if args.student:
        elf = student_elf()
        print(f"   student image SHA-256 {check_student_image()['image_sha256']} (CubeIDE build)")
        import student_ide
        entry = board_entry(key)
        try:
            result = student_ide.flash_image(elf, entry["stlink_sn"], entry["mcu_uid"])
        except (student_ide.IdeError, sdk.SdkError) as exc:
            raise LabError(str(exc)) from exc
        print(f"   student region written + verified + reset; runtime unchanged; "
              f"student CRC-32 0x{result['artifact']['student_crc32']:08X}")
        return 0
    elif args.runtime:
        rb = RUNTIME_BUILDS[board_family(key).key]
        elf = rb.elf
        print(f"   runtime image {rb.family} SHA-256 {check_runtime_image(rb=rb)['image_sha256']}")
    else:
        require_bringup(key)
        elf = check_image(key, args.can_bus)
    target = guard(key)
    sn = board_entry(key)["stlink_sn"]
    print(f"== flash board {key}: ST-LINK {sn}, MCU UID {target['uid']} verified")
    if args.can_bus:
        print("   CAN bus test image: FDCAN1 drives PD1 (CN9-27) and reads PD0 (CN9-25)")
    print(f"   image {elf.relative_to(ROOT)}")
    t0 = time.monotonic()
    code, out = programmer("-c", "port=SWD", f"sn={sn}", "mode=UR", "-w", str(elf), "-v", "-rst")
    verified = "Download verified successfully" in out
    if code != 0 or not verified:
        raise LabError(f"flash board {key} FAILED (exit {code}):\n{_tail(out)}")
    print(f"   written + verified + reset in {time.monotonic() - t0:.1f} s")
    return 0


def _capture(port: str, sn: str, seconds: float) -> list[str]:
    import serial
    lines: list[str] = []
    with serial.Serial(port, VCP_BAUD, timeout=0.1) as ser:
        ser.reset_input_buffer()
        code, out = programmer("-c", "port=SWD", f"sn={sn}", "mode=HOTPLUG", "-rst")
        if code != 0:
            raise LabError(f"reset via ST-LINK {sn} failed (exit {code}):\n{_tail(out)}")
        buf = b""
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            buf += ser.read(512)
            *done, buf = buf.split(b"\n")
            lines += [raw.decode("utf-8", "replace").strip() for raw in done]
    return [line for line in lines if line]


def parse_event(line: str) -> dict | None:
    """One VCP line -> event dict, or None. A reset can cut a line in the middle: the tail of the cut
    line and the next line (often the boot event) then arrive glued; the last JSON object is kept."""
    for start in (0, line.rfind('{"ts":')):
        if start < 0:
            continue
        try:
            ev = json.loads(line[start:])
        except json.JSONDecodeError:
            continue
        if isinstance(ev, dict):
            return ev
    return None


def _smoke_one(key: str, seconds: float) -> bool:
    entry = board_entry(key)
    sn, node, uid = entry["stlink_sn"], entry["node"], entry["mcu_uid"].upper()
    checks: list[tuple[str, bool, str]] = []

    def check(name: str, ok: bool, detail: str = "") -> None:
        checks.append((name, bool(ok), detail))

    print(f"== smoke-test board {key}")
    target = guard(key)
    check("probe+swd_uid", True, f"ST-LINK {sn} -> MCU {target['uid']}")
    port = com_ports_by_sn().get(sn)
    check("vcp_port", port is not None, port or "no COM port for this ST-LINK")
    lines = _capture(port, sn, seconds) if port else []

    LOG_DIR.mkdir(parents=True, exist_ok=True)
    log = LOG_DIR / f"smoke_board_{key.lower()}_{datetime.now():%Y%m%d-%H%M%S}.jsonl"
    log.write_text("\n".join(lines) + "\n", encoding="utf-8")

    # Lines before the boot event may be the tail of a line cut by the reset.
    parsed = [parse_event(line) for line in lines]
    boot_idx = next((i for i, ev in enumerate(parsed)
                     if ev and ev.get("type") == "state" and ev.get("state") == "boot"), None)
    after = parsed[boot_idx:] if boot_idx is not None else []
    boot = after[0] if after else {}
    meta = boot.get("meta", {}) if boot else {}

    check("boot_event", boot_idx is not None, f"{len(lines)} lines captured in {seconds:.1f} s")
    check("identity", boot.get("node") == node and meta.get("board") == key,
          f"node={boot.get('node')} board={meta.get('board')} (expected {node}/{key}), "
          f"fw {meta.get('fw_version')}, CAN image {meta.get('can_mode')}")
    check("uid_firmware", str(meta.get("uid", "")).upper() == uid, f"firmware reports {meta.get('uid')}")
    check("dev_id", meta.get("dev_id") == f"0x{EXPECTED_DEV_ID:03X}", f"firmware reports {meta.get('dev_id')}")
    invalid = [lines[boot_idx + i] for i, ev in enumerate(after) if ev is None] if after else []
    check("json_lines", not invalid and bool(after), f"{len(invalid)} invalid line(s) after boot")
    foreign = {ev.get("node") for ev in after if ev and ev.get("node") != node}
    check("no_foreign_node", not foreign, f"foreign nodes: {sorted(map(str, foreign))}" if foreign else "")

    selftest = next((ev for ev in after if ev and ev.get("state") == "selftest"), {})
    fd = selftest.get("meta", {}).get("fdcan1_internal_loopback")
    check("fdcan1_internal_loopback", fd == "pass",
          f"{fd} (controller-internal only: no pin, no transceiver, NOT a CAN bus test)")
    if meta.get("can_mode") == "bus":  # bus test image: FDCAN1 must have started in normal mode
        link = next((ev for ev in after if ev and ev.get("state") == "can_link"), {}).get("meta", {})
        check("can_link_started", link.get("status") == "ok",
              f"{link.get('status')} ({link.get('frame')}, {link.get('bitrate')} bit/s, TX {link.get('tx_id')}, "
              f"RX {link.get('rx_id')}; start only, the bus is tested by 'can-link')")

    beats = [ev for ev in after if ev and ev.get("type") == "heartbeat"]
    seqs = [ev.get("meta", {}).get("seq") for ev in beats]
    stamps = [int(ev["ts"]) for ev in beats if str(ev.get("ts", "")).isdigit()]
    gaps = [b - a for a, b in zip(stamps, stamps[1:])]
    consecutive = bool(seqs) and all(isinstance(s, int) for s in seqs) \
        and seqs == list(range(seqs[0], seqs[0] + len(seqs)))
    on_time = all(abs(g - HEARTBEAT_MS) <= 50 for g in gaps)
    check("heartbeats", len(beats) >= 2 and consecutive and on_time,
          f"{len(beats)} beats, seq {seqs}, gaps {gaps} ms, state {[ev.get('state') for ev in beats]}")
    check("running", bool(beats) and all(ev.get("state") == "running" for ev in beats), "")

    ok = all(passed for _, passed, _ in checks)
    for name, passed, detail in checks:
        print(f"   {'PASS' if passed else 'FAIL'}  {name:<26} {detail}")
    print(f"   capture: {log.relative_to(ROOT)}")
    print(f"   board {key}: {'PASS' if ok else 'FAIL'} ({sum(p for _, p, _ in checks)}/{len(checks)})")
    return ok


def _runtime_smoke(key: str) -> bool:
    rc = runtime_tools()
    fam = board_family(key)
    image = check_runtime_image(rb=RUNTIME_BUILDS[fam.key])
    target = guard(key)
    sn, port = runtime_port(key)
    print(f"== smoke-test board {key} (N7RT runtime): ST-LINK {sn}, {port}, MCU UID {target['uid']}")

    def reset_board() -> None:
        code, out = programmer("-c", "port=SWD", f"sn={sn}", "mode=HOTPLUG", "-rst")
        if code != 0:
            raise LabError(f"reset via ST-LINK {sn} failed (exit {code}):\n{_tail(out)}")

    report = rc.smoke(port, reset_board, image["image"], board_entry(key)["mcu_uid"],
                      dev_id=fam.dev_id, sw_reset_bit=fam.sw_reset_bit)
    report.update({"board": key, "stlink_sn": sn, "image_sha256": image["image_sha256"]})
    LOG_DIR.mkdir(parents=True, exist_ok=True)
    log = LOG_DIR / f"runtime_smoke_{key.lower()}_{datetime.now():%Y%m%d-%H%M%S}.json"
    log.write_text(json.dumps(report, indent=2, default=str), encoding="utf-8")
    passed = sum(c["ok"] for c in report["checks"])
    print(f"   report: {log.relative_to(ROOT)}")
    print(f"   board {key}: {'PASS' if report['ok'] else 'FAIL'} ({passed}/{len(report['checks'])})")
    return report["ok"]


def cmd_smoke(args) -> int:
    if args.student:
        return 0 if _student_smoke(args.board) else 1
    if args.runtime:
        return 0 if _runtime_smoke(args.board) else 1
    results = {key: _smoke_one(key, args.seconds) for key in _keys(args.board)}
    if len(results) > 1:
        print("== summary " + "  ".join(f"{k}:{'PASS' if v else 'FAIL'}" for k, v in results.items()))
    return 0 if all(results.values()) else 1


# RCC_RSR bits (RM0433). On the H7 every internal reset also pulls NRST low, so PINRSTF is set
# with SFTRSTF/BORRSTF/...; PINRSTF with none of them means NRST was driven from outside the MCU.
RSR_FLAGS = ((17, "CPU"), (19, "D1"), (20, "D2"), (21, "BOR"), (22, "PIN"), (23, "POR"),
             (24, "SFT"), (26, "IWDG1"), (28, "WWDG1"), (30, "LPWR"))
RESET_CAUSES = (("POR", "power-on"), ("BOR", "brown-out"), ("IWDG1", "watchdog"), ("WWDG1", "watchdog"),
                ("LPWR", "low-power"), ("SFT", "software (SYSRESETREQ: debugger, programmer or firmware)"),
                ("PIN", "NRST pin (RESET button B2, or ST-LINK hardware reset)"))
SILENCE_S = 2.5          # heartbeat every 1 s: 2.5 s without a line = halted, held in reset or USB gone


# RCC_CSR bits of the G0B1 (RM0444): PWR covers both power-on and brown-out.
CSR_FLAGS = ((25, "OBL"), (26, "PIN"), (27, "POR"), (28, "SFT"), (29, "IWDG1"), (30, "WWDG1"), (31, "LPWR"))


def reset_cause(flags, family: str = "h753") -> str:
    """Boot-event reset_flags ('0x01460000') -> 'SFT+PIN+CPU: software (...)'."""
    try:
        value = int(str(flags), 16)
    except ValueError:
        return f"unknown ({flags})"
    names = [name for bit, name in (CSR_FLAGS if family == "g0b1" else RSR_FLAGS) if value >> bit & 1]
    cause = next((text for name, text in RESET_CAUSES if name in names), "unknown")
    return f"{'+'.join(names) or 'none'}: {cause}"


class RebootDetector:
    """Turns one board's VCP lines into incidents: boot event, reboot inferred from the heartbeat
    uptime going backwards (boot line lost), silence. Host time is passed in: testable offline."""

    def __init__(self, silence_s: float = SILENCE_S) -> None:
        self.silence_s = silence_s
        self.last_rx: float | None = None
        self.silent = False
        self.uptime_ms: int | None = None       # last heartbeat uptime
        self.first: tuple[float, int] | None = None  # (host time, uptime) of the first heartbeat
        self.incidents = 0

    def feed(self, line: str, now: float) -> list[dict]:
        events = []
        if self.silent:
            events.append({"event": "resumed", "gap_s": round(now - self.last_rx, 1)})
            self.silent = False
        self.last_rx = now
        ev = parse_event(line)
        if ev is None:
            return events
        meta = ev.get("meta") or {}
        if ev.get("type") == "state" and ev.get("state") == "boot":
            events.append({"event": "boot", "reset_flags": meta.get("reset_flags"),
                           "cause": reset_cause(meta.get("reset_flags")),
                           "build_system": meta.get("build_system"), "fw_version": meta.get("fw_version"),
                           "previous_uptime_ms": self.uptime_ms})
            self.uptime_ms = 0
        elif ev.get("type") == "heartbeat" and isinstance(meta.get("uptime_ms"), int):
            uptime = meta["uptime_ms"]
            if self.uptime_ms is not None and uptime < self.uptime_ms:
                events.append({"event": "reboot_inferred", "uptime_ms": uptime,
                               "previous_uptime_ms": self.uptime_ms})
            if self.first is None:
                self.first = (now, uptime)
            self.uptime_ms = uptime
        self.incidents += sum(e["event"] != "resumed" for e in events)
        return events

    def port_opened(self, now: float) -> None:
        """Silence is also counted from the port opening: a board halted before the watch started is caught."""
        if self.last_rx is None:
            self.last_rx = now

    def idle(self, now: float) -> list[dict]:
        if self.last_rx is None or self.silent or now - self.last_rx <= self.silence_s:
            return []
        self.silent = True
        self.incidents += 1
        return [{"event": "silence", "after_s": round(now - self.last_rx, 1)}]

    def continuity(self, now: float) -> dict:
        """Uptime gained vs host time elapsed since the first heartbeat: equal when nothing reset."""
        if self.first is None or self.uptime_ms is None:
            return {"uptime_ms": self.uptime_ms}
        host_ms = (self.last_rx - self.first[0]) * 1000
        return {"uptime_ms": self.uptime_ms, "uptime_gain_ms": self.uptime_ms - self.first[1],
                "host_elapsed_ms": round(host_ms)}


def watch_boards(keys: list[str], seconds: float | None, stop=None, emit=print) -> dict[str, int]:
    """Read the VCP of each board, report reboots, silences and USB losses. Returns incidents per board.
    Every received line and incident goes to data/logs/watch_<time>.jsonl with the host time."""
    import serial

    boards = {k: board_entry(k) for k in keys}
    detectors = {k: RebootDetector() for k in keys}
    ports: dict[str, serial.Serial | None] = dict.fromkeys(keys)
    usb_present = dict.fromkeys(keys, True)
    open_error: dict[str, str | None] = dict.fromkeys(keys)
    bufs = dict.fromkeys(keys, b"")
    LOG_DIR.mkdir(parents=True, exist_ok=True)
    log_path = LOG_DIR / f"watch_{'_'.join(keys).lower()}_{datetime.now():%Y%m%d-%H%M%S}.jsonl"
    start = time.monotonic()
    next_scan = 0.0

    with log_path.open("w", encoding="utf-8") as log:
        def record(key: str, event: dict) -> None:
            stamp = datetime.now().isoformat(timespec="milliseconds")
            log.write(json.dumps({"t": stamp, "board": key, **event}) + "\n")
            log.flush()
            if event["event"] != "line":
                details = " ".join(f"{k}={v}" for k, v in event.items() if k != "event")
                emit(f"{stamp[11:23]}  board {key}  {event['event'].upper():<16} {details}")

        emit(f"== watch {', '.join(keys)}: log {log_path.relative_to(ROOT)} (Ctrl+C to stop)")
        try:
            while not (stop and stop.is_set()) and (seconds is None or time.monotonic() - start < seconds):
                now = time.monotonic()
                if now >= next_scan:  # COM numbers are resolved from the ST-LINK serial every second
                    next_scan = now + 1.0
                    by_sn = com_ports_by_sn()
                    for key in keys:
                        com = by_sn.get(boards[key]["stlink_sn"])
                        if (com is not None) != usb_present[key]:
                            usb_present[key] = com is not None
                            detectors[key].incidents += com is None
                            record(key, {"event": "usb_back" if com else "usb_lost", "com": com})
                        if com is None and ports[key]:
                            ports[key].close()
                            ports[key] = None
                        if com and ports[key] is None:
                            try:
                                ports[key] = serial.Serial(com, VCP_BAUD, timeout=0)
                                open_error[key] = None
                                detectors[key].port_opened(time.monotonic())
                                record(key, {"event": "port_open", "com": com})
                            except serial.SerialException as exc:
                                if open_error[key] is None:
                                    detectors[key].incidents += 1
                                    record(key, {"event": "port_error", "com": com,
                                                 "error": str(exc).splitlines()[0][:160]})
                                open_error[key] = str(exc)
                for key in keys:
                    ser = ports[key]
                    if ser is None:
                        continue
                    try:
                        bufs[key] += ser.read(ser.in_waiting or 1)
                    except serial.SerialException:
                        ser.close()
                        ports[key] = None
                        continue
                    *lines, bufs[key] = bufs[key].split(b"\n")
                    for raw in lines:
                        text = raw.decode("utf-8", "replace").strip()
                        if text:
                            record(key, {"event": "line", "raw": text[:400]})
                            for event in detectors[key].feed(text, time.monotonic()):
                                record(key, event)
                    for event in detectors[key].idle(time.monotonic()):
                        record(key, event)
                time.sleep(0.02)
        except KeyboardInterrupt:
            pass
        finally:
            for ser in ports.values():
                if ser:
                    ser.close()
        for key in keys:
            summary = {"event": "summary", "incidents": detectors[key].incidents,
                       **detectors[key].continuity(time.monotonic())}
            record(key, summary)
    return {key: detectors[key].incidents for key in keys}


def cmd_watch(args) -> int:
    if args.runtime:
        return _runtime_watch(args.board, args.seconds)
    incidents = watch_boards(_keys(args.board), args.seconds)
    print("== watch " + "  ".join(f"{k}:{'OK' if n == 0 else f'{n} INCIDENT(S)'}" for k, n in incidents.items()))
    return 0 if not any(incidents.values()) else 1


def _runtime_watch(key: str, seconds: float | None) -> int:
    rc = runtime_tools()
    sn = board_entry(key)["stlink_sn"]
    LOG_DIR.mkdir(parents=True, exist_ok=True)
    log_path = LOG_DIR / f"runtime_watch_{key.lower()}_{datetime.now():%Y%m%d-%H%M%S}.jsonl"
    print(f"== watch board {key} (N7RT runtime, passive): log {log_path.relative_to(ROOT)} (Ctrl+C to stop)")
    with log_path.open("w", encoding="utf-8") as log:
        def on_event(event: dict) -> None:
            stamp = datetime.now().isoformat(timespec="milliseconds")
            if event["event"] == "boot":
                event["cause"] = reset_cause(event["reset_flags"], board_family(key).key)
            log.write(json.dumps({"t": stamp, "board": key, **event}) + "\n")
            log.flush()
            details = " ".join(f"{k}={v}" for k, v in event.items() if k != "event")
            print(f"{stamp[11:23]}  board {key}  {event['event'].upper():<16} {details}")
        incidents = rc.watch(lambda: com_ports_by_sn().get(sn), seconds, on_event)
    print(f"== watch {key}: {'OK' if incidents == 0 else f'{incidents} INCIDENT(S)'}")
    return 0 if incidents == 0 else 1


def cmd_soak(args) -> int:
    rc = runtime_tools()
    key = args.board
    target = guard(key)
    sn, port = runtime_port(key)
    print(f"== soak board {key} (N7RT runtime): ST-LINK {sn}, {port}, MCU UID {target['uid']}")
    report = rc.soak(port, args.minutes * 60, args.rate)
    report.update({"board": key, "stlink_sn": sn})
    LOG_DIR.mkdir(parents=True, exist_ok=True)
    log = LOG_DIR / f"runtime_soak_{key.lower()}_{datetime.now():%Y%m%d-%H%M%S}.json"
    log.write_text(json.dumps(report, indent=2, default=str), encoding="utf-8")
    c, th, rtt = report["counts"], report["throughput"], report["rtt_ms"]
    print(f"   {report['duration_s']} s, {c['frames']} frames ({th['frames_per_s']}/s), {c['results']} RESULT, "
          f"{c['ticks']} TICK_RESULT, {c['logs']} STUDENT_LOG, {c['sim_time']} SIM_TIME, NACK {c['nacks']}")
    print(f"   lost {report['lost_frames']}, board deltas {report['board_deltas']}, resets {report['resets']}, "
          f"faults {report['faults']}, PC frame errors {report['pc_frame_errors']}, rejects {report['pc_rejects']}")
    print(f"   observed: PC->board {th['observed_pc_to_board_Bps']} B/s ({th['observed_pc_to_board_util']:.1%}), "
          f"board->PC {th['observed_board_to_pc_Bps']} B/s ({th['observed_board_to_pc_util']:.1%}) "
          f"of {th['line_capacity_Bps']:.0f} B/s per direction")
    print(f"   computed worst case: PC->board {th['computed_worst_case_pc_to_board_Bps']} B/s, "
          f"board->PC {th['computed_worst_case_board_to_pc_Bps']} B/s")
    print(f"   RTT frame->RESULT ms: p50 {rtt['p50']}  p95 {rtt['p95']}  p99 {rtt['p99']}  max {rtt['max']}; "
          f"max hook {report['board_max_hook_us']} us; heartbeat max gap {report['heartbeat_max_gap_s']} s")
    for name, ok in report["criteria"].items():
        print(f"   {'PASS' if ok else 'FAIL'}  {name}")
    for text in report["problems"][:10]:
        print(f"   problem: {text}")
    print(f"   report: {log.relative_to(ROOT)}")
    print(f"== soak board {key}: {'PASS' if report['ok'] else 'FAIL'}")
    return 0 if report["ok"] else 1


# --------------------------------------------------------------------------- CAN link (bus test image)

CAN_PERIOD_MS = 100      # firmware CAN_LINK_PERIOD_MS
CAN_ERR_FIELDS = ("err_stuff", "err_form", "err_ack", "err_bit1", "err_bit0", "err_crc")
CAN_HINTS = (
    ("err_ack", "sender", "no ACK: the other node does not see the frames (CAN_H/CAN_L open or swapped, "
                          "other MCP2551 unpowered or in standby with Rs high, other board not in bus image)"),
    ("err_bit0", "sender", "sender cannot put a dominant bit on the bus: TXD wire (PD1 -> MCP2551 pin 1), "
                           "MCP2551 VDD/GND, or Rs high (standby)"),
    ("err_bit1", "sender", "recessive bit read back dominant: another node or a short holds the bus dominant"),
    ("err_stuff", "any", "stuff errors: bit timing mismatch, CAN_H/CAN_L swapped, termination or noise"),
    ("err_form", "any", "form errors: bit timing mismatch or noise"),
    ("err_crc", "any", "CRC errors: noise, termination (expect 60 ohm) or bit timing"),
)


def _delta(final: dict, base: dict, name: str) -> int:
    return int(final.get(name, 0)) - int(base.get(name, 0))


def evaluate_can_direction(sender: str, receiver: str, base: dict, final: dict, frames: list[dict],
                           seconds: float) -> list[tuple[str, bool, str]]:
    """Checks for one direction. base/final: 'can' status meta per board before/after the TX window;
    frames: can_rx events the receiver reported during the window. Pure: testable without hardware."""
    s0, s1, r0, r1 = base[sender], final[sender], base[receiver], final[receiver]
    sent, queued = _delta(s1, s0, "tx_ok"), _delta(s1, s0, "tx_queued")
    received = _delta(r1, r0, "rx")
    expected_min = int(seconds * 1000 / CAN_PERIOD_MS * 0.8)
    counters = [int(f["counter"]) for f in frames if f.get("from") == sender and f.get("id") == s1.get("tx_id")]
    consecutive = bool(counters) and counters == list(range(counters[0], counters[0] + len(counters)))
    checks = [
        ("sender_frames_acknowledged", sent >= expected_min,
         f"{sender} tx_ok +{sent} (expected >= {expected_min} in {seconds:.0f} s at {CAN_PERIOD_MS} ms)"),
        ("all_queued_frames_acknowledged", sent == queued and _delta(s1, s0, "tx_fifo_full") == 0,
         f"queued +{queued}, acknowledged +{sent}, tx_fifo_full +{_delta(s1, s0, 'tx_fifo_full')}"),
        ("receiver_count_equals_sent", received == sent, f"{receiver} rx +{received} vs {sender} tx_ok +{sent}"),
        ("telemetry_frames_consecutive", consecutive and len(counters) == received,
         f"{len(counters)} can_rx events, counters {counters[:1]}..{counters[-1:]}"),
        ("last_counter_matches_sender", bool(counters) and counters[-1] == int(s1.get("tx_counter", 0)) - 1,
         f"{receiver} last {counters[-1] if counters else None}, {sender} next {s1.get('tx_counter')}"),
        ("no_loss_no_bad_frame", all(_delta(r1, r0, n) == 0 for n in ("rx_lost", "rx_bad", "rx_overflow",
                                                                       "rx_restarts", "rx_other")),
         ", ".join(f"{n} +{_delta(r1, r0, n)}" for n in ("rx_lost", "rx_bad", "rx_overflow", "rx_restarts", "rx_other"))),
        ("reverse_direction_silent", _delta(s1, s0, "rx") == 0, f"{sender} rx +{_delta(s1, s0, 'rx')} (other TX off)"),
    ]
    for board, (b0, b1) in ((sender, (s0, s1)), (receiver, (r0, r1))):
        errs = {n: _delta(b1, b0, n) for n in CAN_ERR_FIELDS if _delta(b1, b0, n)}
        checks.append((f"no_protocol_error_{board}", not errs and _delta(b1, b0, "bus_off_events") == 0,
                       f"{errs or 'none'}, bus_off +{_delta(b1, b0, 'bus_off_events')}, "
                       f"last_error {b1.get('last_error')}"))
        checks.append((f"error_active_{board}", int(b1.get("tec", 0)) == 0 and int(b1.get("rec", 0)) == 0
                       and not int(b1.get("bo", 0)), f"TEC {b1.get('tec')} REC {b1.get('rec')} "
                       f"EP {b1.get('ep')} BO {b1.get('bo')}"))
    return checks


def can_hints(sender: str, receiver: str, base: dict, final: dict) -> list[str]:
    hints = []
    for field, who, text in CAN_HINTS:
        boards = (sender,) if who == "sender" else (sender, receiver)
        if any(_delta(final[b], base[b], field) for b in boards):
            hints.append(f"{field}: {text}")
    if not any(_delta(final[b], base[b], n) for b in (sender, receiver) for n in CAN_ERR_FIELDS) \
            and _delta(final[sender], base[sender], "tx_ok") == 0:
        hints.append("no frame acknowledged and no error seen: is TX really on the bus image, "
                     "is PD1 wired to MCP2551 TXD (pin 1)?")
    return hints


class VcpLink:
    """Both VCPs opened at once: send one-line commands, collect JSON events with host time."""

    def __init__(self, keys: list[str], log) -> None:
        import serial

        ports = com_ports_by_sn()
        self.ser, self.buf, self.events, self.log = {}, {}, [], log
        for key in keys:
            port = ports.get(board_entry(key)["stlink_sn"])
            if not port:
                raise LabError(f"board {key}: no VCP for its ST-LINK")
            try:
                self.ser[key] = serial.Serial(port, VCP_BAUD, timeout=0)
            except serial.SerialException as exc:
                raise LabError(f"board {key}: cannot open {port} ({exc}); release it in the dashboard") from exc
            self.buf[key] = b""

    def close(self) -> None:
        for ser in self.ser.values():
            ser.close()

    def pump(self, seconds: float = 0.0) -> None:
        deadline = time.monotonic() + seconds
        while True:
            for key, ser in self.ser.items():
                self.buf[key] += ser.read(ser.in_waiting or 1)
                *lines, self.buf[key] = self.buf[key].split(b"\n")
                for raw in lines:
                    text = raw.decode("utf-8", "replace").strip()
                    if not text:
                        continue
                    self.log.write(json.dumps({"t": datetime.now().isoformat(timespec="milliseconds"),
                                               "board": key, "raw": text}) + "\n")
                    ev = parse_event(text)
                    if ev is not None:
                        self.events.append((key, time.monotonic(), ev))
            if time.monotonic() >= deadline:
                return
            time.sleep(0.01)

    def command(self, key: str, cmd: str, want_state: str, timeout: float = 2.0, tries: int = 3) -> dict:
        """Send cmd, return the meta of the answering 'can' event (state want_state)."""
        for _ in range(tries):
            mark = len(self.events)
            self.ser[key].write((cmd + "\n").encode())
            deadline = time.monotonic() + timeout
            while time.monotonic() < deadline:
                self.pump(0.05)
                for k, _, ev in self.events[mark:]:
                    if k != key:
                        continue
                    if ev.get("type") == "can" and ev.get("state") == want_state:
                        return ev.get("meta", {})
                    if ev.get("state") == "command_rejected":
                        reason = ev.get("meta", {}).get("reason")
                        raise LabError(f"board {key} rejected '{cmd}' ({reason}): "
                                       + ("flash the bus image: 'flash X --can-bus'" if reason == "loopback_build"
                                          else "see the can_link boot event"))
        raise LabError(f"board {key}: no answer to '{cmd}' (running the bus image? run 'smoke-test {key}')")


def cmd_can_link(args) -> int:
    LOG_DIR.mkdir(parents=True, exist_ok=True)
    log_path = LOG_DIR / f"can_link_{datetime.now():%Y%m%d-%H%M%S}.jsonl"
    results: dict[str, bool] = {}
    print(f"== can-link: A -> B then B -> A, classic 11-bit, 500 kbit/s, {args.seconds:.0f} s per direction")
    print(f"   log {log_path.relative_to(ROOT)}")
    with log_path.open("w", encoding="utf-8") as log:
        link = VcpLink(["A", "B"], log)
        try:
            for key in ("A", "B"):
                link.command(key, "CAN TX 0", "tx_off")
            link.pump(0.5)
            for sender, receiver in (("A", "B"), ("B", "A")):
                base = {k: link.command(k, "CAN STATUS", "status") for k in ("A", "B")}
                mark = len(link.events)
                link.command(sender, "CAN TX 1", "tx_on")
                link.pump(args.seconds)
                link.command(sender, "CAN TX 0", "tx_off")
                link.pump(0.5)
                final = {k: link.command(k, "CAN STATUS", "status") for k in ("A", "B")}
                frames = [ev.get("meta", {}) for k, _, ev in link.events[mark:]
                          if k == receiver and ev.get("type") == "can_rx"]
                checks = evaluate_can_direction(sender, receiver, base, final, frames, args.seconds)
                ok = all(passed for _, passed, _ in checks)
                results[f"{sender}->{receiver}"] = ok
                print(f"\n== {sender} -> {receiver}: ID {final[sender].get('tx_id')}")
                for name, passed, detail in checks:
                    print(f"   {'PASS' if passed else 'FAIL'}  {name:<32} {detail}")
                for hint in ([] if ok else can_hints(sender, receiver, base, final)):
                    print(f"   HINT  {hint}")
                log.write(json.dumps({"t": datetime.now().isoformat(timespec="milliseconds"),
                                      "verdict": f"{sender}->{receiver}", "ok": ok,
                                      "checks": checks, "base": base, "final": final}) + "\n")
        finally:
            for key in ("A", "B"):
                try:
                    link.ser[key].write(b"CAN TX 0\n")
                except Exception:  # noqa: BLE001 - best effort, the port may be gone
                    pass
            link.close()
    print("\n== can-link " + "  ".join(f"{d}:{'PASS' if ok else 'FAIL'}" for d, ok in results.items()))
    return 0 if results and all(results.values()) else 1


# --------------------------------------------------------------------------- N7RT CAN Lab application (S1.4)

def _app_verdict(report: Path, result: dict, checks: list[tuple[str, bool, str]], **context) -> bool:
    """Writes the run's report with its checks, prints one PASS/FAIL line per check, returns the verdict."""
    result.update({**context, "checks": [{"check": c, "ok": ok, "detail": d} for c, ok, d in checks]})
    report.write_text(json.dumps(result, indent=2, ensure_ascii=False, default=str), encoding="utf-8")
    for c, ok, d in checks:
        print(f"   [{'PASS' if ok else 'FAIL'}] {c}: {d}")
    verdict = all(ok for _, ok, _ in checks)
    shown = report.relative_to(ROOT) if report.is_relative_to(ROOT) else report
    print(f"   {'PASS' if verdict else 'FAIL'} — {shown}")
    return verdict


def cmd_app_test_phys(args) -> int:
    """Physical Lab scenarios (docs/physical-lab-v1.md §8.6, §13): no registry lookup, no ST-LINK, no SWD,
    no serial port."""
    import n7rt_app
    if args.board is not None:
        raise LabError("app-test --phys runs without a board: drop the board argument")
    if args.scenario not in (*n7rt_app.PHYS_SCENARIOS, *n7rt_app.PACK_SCENARIOS, "all"):
        raise LabError(f"app-test --phys: --scenario phys, phys-perf, pack-<id> or all (not {args.scenario})")
    missing = n7rt_app.mission_env_missing(args.scenario)
    if missing:
        raise LabError(f"app-test --scenario {args.scenario}: set {', '.join(missing)} in the environment")
    try:
        exe = n7rt_app.app_exe(args.release, args.exe)
    except FileNotFoundError as exc:
        raise LabError(str(exc)) from exc
    scenarios = list(n7rt_app.PHYS_SCENARIOS) if args.scenario == "all" else [args.scenario]
    LOG_DIR.mkdir(parents=True, exist_ok=True)
    failed = 0
    shown = exe.relative_to(ROOT) if exe.is_relative_to(ROOT) else exe
    for scenario in scenarios:
        print(f"== app-test {scenario} (Physical Lab, no board): {shown}")
        out_dir = n7rt_app.report_dir(scenario, LOG_DIR)
        out_dir.mkdir(parents=True, exist_ok=True)
        report = out_dir / f"app_{scenario}_{datetime.now():%Y%m%d-%H%M%S}.json"
        result = n7rt_app.run_phys(exe, scenario, report,
                                 timeout_s=n7rt_app.PHYS_TIMEOUT_S + (args.seconds if args.seconds >= 600 else 0),
                                 seconds=args.seconds)
        failed += not _app_verdict(report, result, n7rt_app.evaluate(result, scenario), exe=str(exe))
    return 0 if failed == 0 else 1


def cmd_app_test(args) -> int:
    """Run the application's own end-to-end scenario on a real board, or the Physical Lab without any board
    with --phys (tools/stm32/n7rt_app.py)."""
    import n7rt_app
    if args.phys:
        return cmd_app_test_phys(args)
    if args.board is None:
        raise LabError("app-test: name the board, or use --phys for the Physical Lab (no board)")
    if args.scenario in n7rt_app.PHYS_SCENARIOS:
        raise LabError(f"app-test --scenario {args.scenario} is a Physical Lab scenario: use --phys, without a board")
    missing = n7rt_app.mission_env_missing(args.scenario)
    if missing:
        raise LabError(f"app-test --scenario {args.scenario}: set {', '.join(missing)} in the environment "
                       "(instructor directory, instructor code; group pack and group code for student-<mission>; "
                       "decoder source for ide-<scenario>)")
    key = args.board
    target = guard(key)
    sn, port = runtime_port(key)
    exe = n7rt_app.app_exe(args.release, args.exe)
    scenarios = ["functional", "perf"] if args.scenario == "all" else [args.scenario]

    def halt() -> None:
        # Like a debugger breakpoint: the core stays halted while the SWD session lasts (the CLI
        # resumes it when it disconnects), here the ~6 s of a 2 MB flash read.
        dump = LOG_DIR / f"app_test_halt_{os.getpid()}.bin"
        try:
            code, out = programmer("-c", f"port=SWD sn={sn} mode=HOTPLUG", "-halt", "-u", "0x08000000", "0x200000", str(dump))
        finally:
            dump.unlink(missing_ok=True)
        if code != 0 or "Core halted" not in out:
            raise LabError(f"SWD halt failed: {_tail(out)}")

    def resume() -> None:
        code, out = programmer("-c", f"port=SWD sn={sn} mode=HOTPLUG", "-run")
        if code != 0:
            raise LabError(f"SWD run failed: {_tail(out)}")

    LOG_DIR.mkdir(parents=True, exist_ok=True)
    failed = 0
    shown = exe.relative_to(ROOT) if exe.is_relative_to(ROOT) else exe
    for scenario in scenarios:
        print(f"== app-test {scenario} board {key}: ST-LINK {sn}, {port}, MCU UID {target['uid']}, {shown}")
        stamp = f"{datetime.now():%Y%m%d-%H%M%S}"
        out_dir = n7rt_app.report_dir(scenario, LOG_DIR)
        out_dir.mkdir(parents=True, exist_ok=True)
        report = out_dir / f"app_{scenario}_{key.lower()}_{stamp}.json"
        timeout = (args.seconds + 120) if scenario == "perf" else 300
        if n7rt_app.is_progression(scenario):
            timeout = n7rt_app.PROGRESSION_TIMEOUT_S
            os.environ[n7rt_app.ENV_STAGE] = args.stage
        result = n7rt_app.run(exe, scenario, sn, port, report, seconds=args.seconds, rate=args.rate,
                              coordinated=(scenario in ("functional", "ide") and not args.no_coordinated),
                              halt=halt, resume=resume, timeout_s=timeout, expect_verdict=args.expect_verdict,
                              tool=args.tool)
        checks = n7rt_app.evaluate(result, scenario, args.seconds, args.rate, expect_verdict=args.expect_verdict)
        failed += not _app_verdict(report, result, checks, board=key, stlink_sn=sn, port=port, exe=str(exe))
    return 0 if failed == 0 else 1


def _keys(value: str) -> list[str]:
    """'all' = the bring-up bench boards; runtime-only boards are always named explicitly."""
    return sorted(k for k in load_boards() if has_bringup(k)) if value == "all" else [value]


def main() -> int:
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(errors="replace")
    boards = sorted(load_boards())
    parser = argparse.ArgumentParser(prog="stm32lab", description=__doc__.split("\n\n")[0])
    sub = parser.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("detect", help="toolchain, ST-LINK probes, COM ports, registry match")
    p.add_argument("--json", action="store_true", help="machine-readable report (used by the dashboard)")
    p.set_defaults(func=cmd_detect)
    sub.add_parser("deps", help="fetch pinned official ST sources into third_party/").set_defaults(func=cmd_deps)
    can_help = "CAN bus test image (FDCAN1 normal mode on PD0/PD1, external transceiver required)"
    rt_help = "N7RT runtime image (board-agnostic, N7RT-Link v0 at 921600 Bd)"
    p = sub.add_parser("build", help="build the bring-up firmware, or the runtime image with --runtime")
    p.add_argument("board", nargs="?", default="all", choices=boards + ["all"])
    p.add_argument("--can-bus", action="store_true", help=can_help)
    st_help = "student project (firmware/n7rt_student) built by STM32CubeIDE against the precompiled runtime"
    p.add_argument("--runtime", action="store_true", help=rt_help)
    p.add_argument("--student", action="store_true", help=st_help + " (headless clean build + checks)")
    p.add_argument("--family", default="h753", choices=[f.key for f in sdk.FAMILIES],
                   help="with --runtime: board family of the runtime image (h753 = NUCLEO-H753ZI, g0b1 = NUCLEO-G0B1RE)")
    p.set_defaults(func=cmd_build)
    p = sub.add_parser("flash", help="flash one board, selected by ST-LINK serial + MCU UID")
    p.add_argument("board", choices=boards)
    p.add_argument("--can-bus", action="store_true", help=can_help)
    p.add_argument("--runtime", action="store_true", help=rt_help)
    p.add_argument("--student", action="store_true", help=st_help)
    p.set_defaults(func=cmd_flash)
    p = sub.add_parser("smoke-test", help="reset a board and check its VCP output")
    p.add_argument("board", nargs="?", default="all", choices=boards + ["all"])
    p.add_argument("--seconds", type=float, default=3.5, help="capture duration after reset")
    p.add_argument("--runtime", action="store_true", help=rt_help + ": bidirectional N7RT-Link test")
    p.add_argument("--student", action="store_true", help=st_help + ": generic student starter and verified runtime")
    p.set_defaults(func=cmd_smoke)
    p = sub.add_parser("sdk", help="package the validated runtime into the student CubeIDE project, then check it")
    p.add_argument("--check", action="store_true", help="only check the SDK already in the project")
    p.add_argument("--family", default="all", choices=[f.key for f in sdk.FAMILIES] + ["all"],
                   help="SDK of one board family (default: every family)")
    p.set_defaults(func=cmd_sdk)
    p = sub.add_parser("soak", help="N7RT runtime: sustained V1 load, every board output verified")
    p.add_argument("board", choices=boards)
    p.add_argument("--minutes", type=float, default=10.0, help="duration (spec: 10 minutes)")
    p.add_argument("--rate", type=int, default=600, help="VCAN_FRAME per second (V1 cap: 600)")
    p.set_defaults(func=cmd_soak)
    p = sub.add_parser("led-check", help="measure the LD1 blink rate at the pin over SWD (no halt)")
    p.add_argument("board", choices=boards)
    p.add_argument("--seconds", type=float, default=15.0, help="sampling duration")
    p.add_argument("--can-bus", action="store_true", help="the running image is the CAN bus test image")
    p.set_defaults(func=cmd_led_check)
    p = sub.add_parser("can-link", help="physical CAN test A->B then B->A (both boards on the bus image)")
    p.add_argument("--seconds", type=float, default=10.0, help="TX window per direction")
    p.set_defaults(func=cmd_can_link)
    p = sub.add_parser("app-test", help="N7RT CAN Lab app: automated end-to-end scenario on a real board, "
                                        "or the Physical Lab without any board (--phys)")
    p.add_argument("board", nargs="?", choices=boards, help="registered board (board scenarios; none with --phys)")
    p.add_argument("--phys", action="store_true",
                   help="Physical Lab scenarios (docs/physical-lab-v1.md 8.6, 13): no board, ST-LINK, SWD or serial port")
    import n7rt_app
    p.add_argument("--scenario", choices=["functional", "perf", "showcase", "ide", *n7rt_app.MISSION_SCENARIOS,
                                          *n7rt_app.PACK_SCENARIOS, "phys", "phys-perf", "all"],
                   default="all",
                   help="board: all = functional + perf; showcase = screenshots of the main screens; mission-<id> "
                        "(apps, wsf, sas, bps) = that mission (instructor), its bus and its nominal check; "
                        "student-<id> = student flow, run file judged with n7rt-tool; progression-<id> = release "
                        "0.3 pack from E0 to E5 (N7RT_E2E_ANSWERS, --stage); ide-<scenario> = integrated "
                        "IDE build and flash first. "
                        "--phys: phys (manual test replayed), phys-perf (budgets), all = both")
    p.add_argument("--expect-verdict", choices=["pass", "fail", "any"], default="any",
                   help="mission scenarios: verdict the nominal check must give (pass with the reference decoder, "
                        "fail with the SDK template); any = reported only")
    p.add_argument("--seconds", type=int, default=120,
                   help="perf: load duration; phys-perf: >=600 adds a sustained RUN soak of this duration")
    p.add_argument("--rate", type=int, default=600, help="perf: frames per second (V1 cap: 600)")
    p.add_argument("--release", action="store_true", help="test the release build (app/target/release)")
    p.add_argument("--exe", type=Path, help="application to test instead of this repository's build (e.g. the "
                                            "instructor edition from the instructor tools archive, for mission-apps)")
    p.add_argument("--tool", type=Path, help="student-apps: n7rt-tool that judges the run file (the instructor "
                                             "tools archive's, with the oracle); default: next to the application")
    p.add_argument("--no-coordinated", action="store_true", help="skip the SWD halt and busy-port steps")
    p.add_argument("--stage", choices=list(n7rt_app.PROGRESSION_STAGES), default="full",
                   help="progression-<id>: full (E0 to E5), until-incident (the application is closed in incident 1) "
                        "or resume (reopened there, then E5)")
    p.set_defaults(func=cmd_app_test)
    p = sub.add_parser("watch", help="read the VCPs and report any reboot, silence or USB loss")
    p.add_argument("board", nargs="?", default="all", choices=boards + ["all"])
    p.add_argument("--seconds", type=float, default=None, help="stop after N s (default: Ctrl+C)")
    p.add_argument("--runtime", action="store_true", help=rt_help + ": passive N7RT-Link monitor")
    p.set_defaults(func=cmd_watch)
    args = parser.parse_args()
    try:
        return args.func(args)
    except LabError as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
