#!/usr/bin/env python3
"""JSON bridge for the student editor. Fixed GCC build, student-region-only flash.

Only student_decoder.c/.h are inputs. No project makefile, user flags, runtime
source or mission data is read. This protects the upload path; C executing on
the MCU is not a security sandbox.

One build compiles the source once per supported board (NUCLEO-H753ZI, NUCLEO-G0B1RE), each against
its own SDK; the upload sends the image of the board actually connected (read over SWD).
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import stat
import subprocess
import sys
import tempfile
import zlib
from pathlib import Path

if sys.version_info < (3, 11):  # the application shows this report instead of a Python traceback
    print(json.dumps({"ok": False, "operation": sys.argv[1] if len(sys.argv) > 1 else "", "diagnostics": [],
                      "checks": [], "error": f"Python {sys.version_info[0]}.{sys.version_info[1]} trop ancien : "
                      "installez Python 3.11 ou plus récent, puis fermez et relancez CAN Lab (voir DEMARRER_ICI.md)."},
                     ensure_ascii=False))
    sys.exit(1)

import n7rt_sdk as sdk
import stm32lab as lab

SOURCE_NAMES = ("student_decoder.c", "student_decoder.h")
SOURCE_LIMIT = 256 * 1024
LOG_LIMIT = 1024 * 1024
ARTIFACT_NAMES = ("student.elf", "student.bin", "student-g0b1.elf", "student-g0b1.bin", "build.json")
STANDARD_HEADERS = {
    "assert.h", "ctype.h", "errno.h", "float.h", "inttypes.h", "limits.h",
    "math.h", "stdbool.h", "stddef.h", "stdint.h", "stdio.h", "stdlib.h",
    "string.h", "stdarg.h", "time.h",
}
COMMON_CFLAGS = ["-std=gnu11", "-g3", "-O0", "-ffunction-sections", "-fdata-sections",
                 "-Wall", "-Wextra", "-fmax-errors=20", "-fdiagnostics-column-unit=byte",
                 "-fdiagnostics-column-origin=1", "--specs=nano.specs"]
CFLAGS = [*sdk.H753.cpu_flags, *COMMON_CFLAGS]


def cflags(fam: sdk.Family) -> list[str]:
    return [*fam.cpu_flags, *COMMON_CFLAGS]


def _elf_name(fam: sdk.Family) -> str:
    """H753 keeps the historical names (student.elf / student.bin)."""
    return "student.elf" if fam is sdk.H753 else f"student-{fam.key}.elf"


class IdeError(Exception):
    pass


# A missing tool names what to install and the guide shipped with the application (path relative to its folder).
MISSING_TOOL = {
    "gcc": "Compilateur arm-none-eabi-gcc introuvable : installez Arm GNU Toolchain (arm-none-eabi), "
           "puis relancez la compilation. Installation et diagnostic : docs/stm32-toolchain-windows.md.",
    "programmer": "STM32_Programmer_CLI introuvable : installez STM32CubeProgrammer, puis relancez le chargement. "
                  "Installation et diagnostic : docs/stm32-toolchain-windows.md.",
}


def _require(key: str) -> Path:
    found = lab.find_tool(key)
    if not found:
        raise IdeError(MISSING_TOOL[key])
    return found[1]


def _digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def _linked(path: Path) -> bool:
    if not path.exists() and not path.is_symlink():
        return False
    info = path.lstat()
    return path.is_symlink() or bool(getattr(info, "st_file_attributes", 0)
                                    & getattr(stat, "FILE_ATTRIBUTE_REPARSE_POINT", 0))


def _workspace(path: Path) -> Path:
    if _linked(path):
        raise IdeError("Le dossier du projet ne peut pas être un lien.")
    path = path.resolve(strict=True)
    if not path.is_dir() or path.is_relative_to(sdk.PROJECT.resolve()):
        raise IdeError("Choisissez un dossier de travail séparé du SDK.")
    return path


def _source_text(text: str) -> str:
    # C phase 2 precedes comments/directives: joining prevents escaped-line bypasses.
    text = re.sub(r"\\\r?\n", "", text)
    tokens = re.compile(r'/\*.*?\*/|//[^\n]*|"(?:\\.|[^"\\])*"|\'(?:\\.|[^\'\\])*\'', re.S)
    return tokens.sub(lambda m: re.sub(r"[^\n]", " ", m[0])
                      if m[0].startswith(("/*", "//")) else m[0], text)


def _validate_source(name: str, text: str) -> None:
    clean = _source_text(text)
    tokens = re.sub(r'"(?:\\.|[^"\\])*"|\'(?:\\.|[^\'\\])*\'', " ", clean)
    if "??" in tokens or "%:" in tokens:
        raise IdeError(f"{name}: les trigraphes ne sont pas pris en charge.")
    if re.search(r"\b(?:asm|__asm|__asm__|_Pragma|__has_include|__has_include_next)\b|##", tokens):
        raise IdeError(f"{name}: assembleur et directives dynamiques non autorisés dans cet éditeur.")
    for line in clean.splitlines():
        directive = re.match(r"\s*#\s*(\w+)\b(.*)", line)
        if not directive:
            continue
        kind, value = directive.groups()
        value = value.strip()
        if kind in ("include_next", "embed", "import"):
            raise IdeError(f"{name}: directive #{kind} non autorisée.")
        if kind == "pragma" and value != "once":
            raise IdeError(f"{name}: seul #pragma once est accepté.")
        if kind == "include":
            target = re.fullmatch(r'"([^"\n]+)"|<([^>\n]+)>', value)
            header = next((v for v in target.groups() if v), "") if target else ""
            if header not in STANDARD_HEADERS | {"student_decoder.h", "n7rt.h"}:
                raise IdeError(f"{name}: en-tête non autorisé ; utilisez le SDK ou un en-tête C standard.")


def _sources(workspace: Path) -> dict[str, bytes]:
    files = {}
    for name in SOURCE_NAMES:
        path = workspace / name
        if _linked(path) or not path.is_file() or path.stat().st_size > SOURCE_LIMIT:
            raise IdeError(f"{name}: fichier ordinaire UTF-8 requis, limité à 256 Kio.")
        data = path.read_bytes()
        if len(data) > SOURCE_LIMIT or b"\0" in data:
            raise IdeError(f"{name}: source trop grande ou contenant un octet nul.")
        try:
            text = data.decode("utf-8-sig")
        except UnicodeDecodeError as exc:
            raise IdeError(f"{name}: encodage UTF-8 requis.") from exc
        _validate_source(name, text)
        files[name] = data
    return files


def _source_hashes(files: dict[str, bytes]) -> dict[str, str]:
    return {name: _digest(data) for name, data in files.items()}


def _environment() -> dict[str, str]:
    # GCC must not inherit include/library injections or instructor credentials.
    keep = {"path", "systemroot", "windir", "temp", "tmp", "localappdata",
            "appdata", "userprofile", "home", "lang"}
    return {k: v for k, v in os.environ.items() if k.lower() in keep}


def _execute(args: list, cwd: Path | None = None, timeout: int = 60) -> subprocess.CompletedProcess:
    return subprocess.run([str(a) for a in args], cwd=cwd, env=_environment(),
                          capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=timeout)


def _sdk_check(bin_dir: Path, fam: sdk.Family = sdk.H753) -> list[dict]:
    manifest = json.loads((fam.n7rt / "manifest.json").read_text(encoding="utf-8"))
    expected = {f"{fam.sdk_dir}/{p}" for p in fam.generated}
    if set(manifest.get("files", {})) != expected:
        raise IdeError(f"Manifest du SDK {fam.board} invalide.")
    checks = [{"check": f"{fam.key}: {label}", "ok": ok, "detail": detail}
              for label, ok, detail in sdk.check_sdk(bin_dir, fam)]
    if not all(c["ok"] for c in checks):
        raise IdeError(f"Le SDK {fam.board} a été modifié ou ne correspond pas au runtime validé.")
    return checks


def _layout_check(bin_dir: Path, elf: Path, fam: sdk.Family = sdk.H753) -> list[str]:
    """Validate every allocated section before objcopy can allocate a sparse image."""
    out = sdk._run([sdk._tool(bin_dir, "objdump"), "-h", elf])
    lines = out.splitlines()
    load_names = []
    for i, line in enumerate(lines):
        section = re.match(r"\s*\d+\s+(\S+)\s+([\da-f]+)\s+([\da-f]+)\s+([\da-f]+)\s+", line, re.I)
        if not section:
            continue
        name, size, vma, lma = section.groups()
        size, vma, lma = (int(v, 16) for v in (size, vma, lma))
        flags = lines[i + 1] if i + 1 < len(lines) else ""
        if not size or "ALLOC" not in flags:
            continue
        if name == ".n7rt_runtime":
            rel = sdk.release(fam)
            if vma != sdk.RUNTIME_BASE or lma != sdk.RUNTIME_BASE or size != rel["image_size"]:
                raise IdeError("Section runtime déplacée ou altérée.")
            continue
        student_flash, student_ram = fam.student_flash, fam.student_ram
        in_flash = student_flash[0] <= vma and vma + size <= student_flash[1]
        in_ram = student_ram[0] <= vma and vma + size <= student_ram[1]
        if not (in_flash or in_ram):
            raise IdeError(f"Section {name} en dehors des régions étudiantes ({fam.board}).")
        if "LOAD" in flags:
            if not (student_flash[0] <= lma and lma + size <= student_flash[1]):
                raise IdeError(f"Section {name} chargerait des octets hors de la flash étudiante.")
            load_names.append(name)
    if ".n7rt_student_header" not in load_names:
        raise IdeError("En-tête étudiant absent.")
    return load_names


def _image(bin_dir: Path, elf: Path, output: Path, fam: sdk.Family = sdk.H753) -> dict:
    sections = _layout_check(bin_dir, elf, fam)
    report = sdk.student_image_report(bin_dir, elf, fam)
    if not report["ok"] or not (64 <= report["student_flash_used"] <= fam.student_flash[1] - fam.student_flash[0]):
        raise IdeError(f"Image étudiante invalide pour {fam.board} ; aucun téléversement possible.")
    args = [sdk._tool(bin_dir, "objcopy"), "-O", "binary", "--gap-fill=0"]
    for section in sections:
        args.append(f"--only-section={section}")
    sdk._run([*args, elf, output])
    binary = output.read_bytes()
    if len(binary) != report["student_flash_used"]:
        raise IdeError("La taille de l'image extraite ne correspond pas à l'en-tête étudiant.")
    if zlib.crc32(binary) != report["student_crc32"]:
        raise IdeError("L'image extraite diffère de l'ELF validé.")
    return {"sha256": _digest(binary), "student_crc32": report["student_crc32"],
            "flash_bytes": len(binary), "ram_bytes": report["student_ram_used"],
            "load_address": f"0x{fam.student_flash[0]:08X}", "board": fam.board, "dev_id": fam.dev_id,
            "runtime_sha256": sdk.release(fam)["image_sha256"],
            "checks": [{"check": name, "ok": ok, "detail": detail} for name, ok, detail in report["checks"]]}


def _build_id(targets: dict) -> str:
    """Identity of a whole build: every board image. The editor passes it back to flash."""
    return _digest("".join(f"{key}:{targets[key]['sha256']}\n" for key in sorted(targets)).encode())


def _diagnostics(log: str) -> list[dict]:
    # GCC columns count UTF-8 bytes from 1. The editor converts them to UTF-16
    # positions in the exact source line before navigating or drawing a marker.
    found = []
    for line in log.splitlines():
        match = re.match(r"(?:.*[/\\])?(student_decoder\.[ch]):(\d+)(?::(\d+))?:\s*(fatal error|error|warning|note):\s*(.*)", line)
        if match:
            name, row, col, severity, message = match.groups()
            found.append({"file": name, "line": int(row), "column": int(col or 1),
                          "severity": "error" if severity == "fatal error" else severity, "message": message})
    return found


def build(path: Path) -> dict:
    workspace = _workspace(path)
    output = workspace / "build"
    if _linked(output):
        raise IdeError("Le dossier de compilation ne peut pas être un lien.")
    output.mkdir(exist_ok=True)
    for name in ARTIFACT_NAMES:
        artifact = output / name
        if _linked(artifact):
            raise IdeError("Un fichier de compilation ne peut pas être un lien.")
        artifact.unlink(missing_ok=True)
    files = _sources(workspace)
    bin_dir = _require("gcc").parent
    checks = [c for fam in sdk.FAMILIES for c in _sdk_check(bin_dir, fam)]
    gcc = sdk._tool(bin_dir, "gcc")
    with tempfile.TemporaryDirectory(prefix=".ide-build-", dir=workspace) as name:
        tmp = Path(name)
        for source, data in files.items():
            (tmp / source).write_bytes(data)
        log, targets = "", {}
        for fam in sdk.FAMILIES:  # the same source for every board; the first failure stops the build
            shutil.copytree(fam.n7rt, tmp / fam.sdk_dir)
            inc, libs, obj = tmp / fam.sdk_dir / "include", tmp / fam.sdk_dir / "lib", f"student-{fam.key}.o"
            done = _execute([gcc, *cflags(fam), "-I", inc, "-I", tmp, "-c", "student_decoder.c", "-o", obj], tmp)
            # Diagnostics come from the first board only: the others compile the very same source.
            step = done.stdout + done.stderr if not targets or done.returncode else ""
            log = (log + step)[:LOG_LIMIT]
            if done.returncode == 0:
                done = _execute([gcc, *cflags(fam), obj, libs / "n7rt_runtime.o", "-L", libs,
                                 "-T", tmp / fam.sdk_dir / "ld" / fam.linker_script, "--specs=nosys.specs",
                                 "-nostartfiles", "-static", "-Wl,--gc-sections", "-o", _elf_name(fam),
                                 "-Wl,--start-group", "-ln7rt_glue", "-lc", "-lm", "-Wl,--end-group"], tmp)
                log = (log + done.stdout + done.stderr)[:LOG_LIMIT]
            if done.returncode != 0:
                log = log.replace(str(tmp), "<projet>")
                return {"ok": False, "operation": "build", "error": f"Compilation refusée ({fam.board}).",
                        "diagnostics": _diagnostics(log), "log": log, "checks": checks}
            elf = tmp / _elf_name(fam)
            targets[fam.key] = _image(bin_dir, elf, elf.with_suffix(".bin"), fam)
        log = log.replace(str(tmp), "<projet>")
        if _source_hashes(_sources(workspace)) != _source_hashes(files):
            raise IdeError("Le source a changé pendant la compilation ; recompilez.")
        # Top level: the build identity (sha256) and the H753 figures, as before; one entry per board in targets.
        first = targets[sdk.H753.key]
        artifact = {"sha256": _build_id(targets), "student_crc32": first["student_crc32"],
                    "flash_bytes": first["flash_bytes"], "ram_bytes": first["ram_bytes"],
                    "targets": targets, "source_sha256": _source_hashes(files)}
        (tmp / "build.json").write_text(json.dumps(artifact), encoding="utf-8")
        for name in ARTIFACT_NAMES:
            if _linked(output / name):
                raise IdeError("Un fichier de compilation ne peut pas être un lien.")
            os.replace(tmp / name, output / name)
    return {"ok": True, "operation": "build", "artifact": artifact, "checks": checks,
            "diagnostics": _diagnostics(log), "log": log}


def _programmer(*args: str) -> str:
    try:
        done = _execute([_require("programmer"), *args], timeout=45)
    except (OSError, subprocess.TimeoutExpired) as exc:
        raise IdeError("ST-LINK inaccessible ou délai dépassé ; reconnectez la carte.") from exc
    out = done.stdout + done.stderr
    if done.returncode:
        raise IdeError("ST-LINK : commande refusée. " + lab._tail(out)[-1200:])
    return out


def _runtime_hash(serial: str, tmp: Path, label: str, fam: sdk.Family = sdk.H753) -> str:
    dump = tmp / f"runtime-{label}.bin"
    _programmer("-c", "port=SWD", f"sn={serial}", "mode=HOTPLUG", "-u",
                f"0x{sdk.RUNTIME_BASE:08X}", f"0x{sdk.release(fam)['image_size']:X}", str(dump))
    if not dump.is_file():
        raise IdeError("Lecture du runtime impossible ; téléversement refusé.")
    return _digest(dump.read_bytes())


def flash(path: Path, serial: str, uid: str, expected_sha256: str) -> dict:
    if not re.fullmatch(r"[0-9a-fA-F]{64}", expected_sha256):
        raise IdeError("Empreinte de compilation requise.")
    workspace = _workspace(path)
    output = workspace / "build"
    if _linked(output) or any(_linked(output / name) for name in ARTIFACT_NAMES):
        raise IdeError("Les fichiers de compilation ne peuvent pas être des liens.")
    receipt_path, elf = output / "build.json", output / "student.elf"
    if not receipt_path.is_file() or receipt_path.stat().st_size > 64 * 1024 or not elf.is_file() or elf.stat().st_size > 8 * 1024 * 1024:
        raise IdeError("Compilez d'abord le projet.")
    receipt = json.loads(receipt_path.read_text(encoding="utf-8"))
    if receipt.get("source_sha256") != _source_hashes(_sources(workspace)):
        raise IdeError("Le source a changé depuis la compilation ; recompilez.")
    if receipt.get("sha256") != expected_sha256.lower():
        raise IdeError("L'image diffère de la dernière compilation ; téléversement refusé.")
    targets = receipt.get("targets")
    if not isinstance(targets, dict) or _build_id(targets) != receipt["sha256"]:
        raise IdeError("Compilation d'une version précédente : recompilez le projet.")
    fam = _connected_family(serial)
    target = targets.get(fam.key)
    elf = output / _elf_name(fam)
    if not isinstance(target, dict) or _linked(elf) or not elf.is_file() or elf.stat().st_size > 8 * 1024 * 1024:
        raise IdeError(f"Aucune image compilée pour la {fam.board} : recompilez le projet.")

    def unchanged() -> None:
        if receipt.get("source_sha256") != _source_hashes(_sources(workspace)):
            raise IdeError("Le source a changé avant le téléversement ; recompilez.")

    return flash_image(elf, serial, uid, target["sha256"], unchanged, fam)


def _connected_family(serial: str) -> sdk.Family:
    """Family of the board behind this ST-LINK, from its DEV_ID read over SWD."""
    if not re.fullmatch(r"[0-9a-fA-F]{8,64}", serial):
        raise IdeError("Identité ST-LINK/MCU invalide ; aucun téléversement.")
    _require("programmer")
    if serial.upper() not in {p["sn"].upper() for p in lab.list_probes()}:
        raise IdeError("Le ST-LINK sélectionné n'est plus connecté.")
    fam = sdk.family_for_dev_id(lab.read_target(serial)["dev_id"])
    if fam is None:
        raise IdeError("Carte non prise en charge : CAN Lab fonctionne avec une NUCLEO-H753ZI ou une NUCLEO-G0B1RE.")
    return fam


def flash_image(elf: Path, serial: str, uid: str, expected_sha256: str | None = None,
                source_guard=None, fam: sdk.Family = sdk.H753) -> dict:
    """Shared safe uploader for the editor and the existing student CubeIDE commands (H753 image)."""
    if not re.fullmatch(r"[0-9a-fA-F]{8,64}", serial) or not re.fullmatch(r"[0-9a-fA-F]{8}(?:-[0-9a-fA-F]{8}){2}", uid):
        raise IdeError("Identité ST-LINK/MCU invalide ; aucun téléversement.")
    if _linked(elf) or not elf.is_file() or elf.stat().st_size > 8 * 1024 * 1024:
        raise IdeError("Image de compilation absente ou invalide.")
    bin_dir = _require("gcc").parent
    _require("programmer")  # before the probe list, which would otherwise fail on the bench tool's English message
    _sdk_check(bin_dir, fam)
    with tempfile.TemporaryDirectory(prefix="n7rt-student-flash-") as name:
        tmp = Path(name)
        shutil.copyfile(elf, tmp / "student.elf")
        artifact = _image(bin_dir, tmp / "student.elf", tmp / "student.bin", fam)
        if expected_sha256 is not None and artifact["sha256"] != expected_sha256.lower():
            raise IdeError("L'image diffère de la dernière compilation ; téléversement refusé.")
        if serial.upper() not in {p["sn"].upper() for p in lab.list_probes()}:
            raise IdeError("Le ST-LINK sélectionné n'est plus connecté.")
        target = lab.read_target(serial)
        if target["uid"].upper() != uid.upper():
            raise IdeError("La carte connectée ne correspond pas à la carte sélectionnée.")
        if target["dev_id"] != fam.dev_id:
            raise IdeError(f"Cette image est pour une {fam.board} : la carte connectée est différente.")
        before = _runtime_hash(serial, tmp, "before", fam)
        if before != sdk.release(fam)["image_sha256"]:
            raise IdeError("Le runtime de la carte diffère du SDK ; seul un encadrant peut le réinstaller.")
        if source_guard is not None:
            source_guard()
        # A flat binary starts at sector 2. Never pass ELF, erase-all, option bytes,
        # or an address outside student flash to the programmer's write command.
        out = _programmer("-c", "port=SWD", f"sn={serial}", "mode=UR", "-w",
                          str(tmp / "student.bin"), f"0x{fam.student_flash[0]:08X}", "-v")
        if "Download verified successfully" not in out:
            raise IdeError("Le téléversement n'a pas été vérifié ; reconnectez la carte.")
        after = _runtime_hash(serial, tmp, "after", fam)
        if after != before:
            raise IdeError("Contrôle du runtime après téléversement échoué ; contactez l'encadrant.")
        _programmer("-c", "port=SWD", f"sn={serial}", "mode=HOTPLUG", "-rst")
    return {"ok": True, "operation": "flash", "serial": serial, "uid": target["uid"], "board": fam.board,
            "runtime_unchanged": True, "artifact": artifact}


def inventory() -> dict:
    from serial.tools import list_ports
    tools = {name: {"available": bool(lab.find_tool(name)),
                    "path": str(lab.find_tool(name)[1]) if lab.find_tool(name) else None}
             for name in ("gcc", "programmer")}
    ports = [{"device": p.device, "description": p.description, "serial_number": p.serial_number,
              "vid": p.vid, "pid": p.pid} for p in list_ports.comports()]
    probes = lab.list_probes() if tools["programmer"]["available"] else []
    return {"ok": all(t["available"] for t in tools.values()), "operation": "inventory",
            "tools": tools, "ports": ports, "probes": probes}


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="operation", required=True)
    sub.add_parser("inventory")
    for operation in ("build", "flash"):
        command = sub.add_parser(operation)
        command.add_argument("--workspace", type=Path, required=True)
        if operation == "flash":
            command.add_argument("--serial", required=True)
            command.add_argument("--uid", required=True)
            command.add_argument("--expected-sha256", required=True)
    args = parser.parse_args(argv)
    try:
        if args.operation == "inventory":
            result = inventory()
        elif args.operation == "build":
            result = build(args.workspace)
        else:
            result = flash(args.workspace, args.serial, args.uid, args.expected_sha256)
    except Exception as exc:
        result = {"ok": False, "operation": args.operation, "error": str(exc)[:4000],
                  "diagnostics": [], "checks": []}
    print(json.dumps(result, ensure_ascii=False))
    return 0 if result["ok"] else 1


if __name__ == "__main__":
    sys.exit(main())
