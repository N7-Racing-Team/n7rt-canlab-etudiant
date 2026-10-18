# -*- coding: utf-8 -*-
"""Builds a COPY of the student CubeIDE project with a given student_decoder.c and flashes it on a board.
The public template (firmware/n7rt_student/N7RT_CANLab_Student/student/) is never modified.
Usage (from the project root): python tools/stm32/flash_decoder.py <student_decoder.c> [--header <student_decoder.h>] [--board C] [--build-only]"""
import argparse, shutil, sys, time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools" / "stm32"))
import stm32lab  # noqa: E402
import n7rt_sdk as sdk  # noqa: E402
import student_ide  # noqa: E402

ap = argparse.ArgumentParser()
ap.add_argument("decoder")
ap.add_argument("--header")
ap.add_argument("--board", default="C")
ap.add_argument("--build-only", action="store_true")
a = ap.parse_args()

work = sdk.SDK_DIR / "build" / "decoder-copy"
dst = work / sdk.PROJECT_NAME
shutil.rmtree(work, ignore_errors=True)
shutil.copytree(sdk.PROJECT, dst, ignore=shutil.ignore_patterns(sdk.CONFIG, "build", ".settings"))
shutil.copyfile(a.decoder, dst / "student" / "student_decoder.c")
if a.header:
    shutil.copyfile(a.header, dst / "student" / "student_decoder.h")

cubeide = stm32lab.require_cubeide()
print(f"== build {Path(a.decoder).name} in a copy of the student project")
ok, elf, log = sdk.cubeide_build(cubeide, dst, work / "workspace")
(work / "cubeide.log").write_text(log, encoding="utf-8")
for line in log.splitlines():
    if any(k in line for k in ("error", "warning:", "Build Finished", "overflowed")):
        print("   " + line.strip())
if not ok:
    sys.exit("BUILD FAILED (see " + str(work / "cubeide.log") + ")")
bin_dir = stm32lab.require_tool("gcc").parent
report = sdk.student_image_report(bin_dir, elf)
bad = [c for c in report["checks"] if not c[1]]
print(f"   image checks: {len(report['checks']) - len(bad)}/{len(report['checks'])} OK; student CRC-32 "
      f"0x{report['student_crc32']:08X}; flash {report['student_flash_used']} B")
if bad:
    sys.exit(f"IMAGE CHECK FAILED: {bad}")
if a.build_only:
    sys.exit(0)

key = a.board
entry = stm32lab.board_entry(key)
sn = entry["stlink_sn"]
print(f"== flash board {key}: ST-LINK {sn}, MCU UID {entry['mcu_uid']}")
t0 = time.monotonic()
try:
    student_ide.flash_image(elf, sn, entry["mcu_uid"])
except (student_ide.IdeError, sdk.SdkError, stm32lab.LabError) as exc:
    sys.exit(f"FLASH FAILED: {exc}")
print(f"   student region written + verified + reset in {time.monotonic() - t0:.1f} s; "
      f"runtime unchanged; student CRC-32 0x{report['student_crc32']:08X}")
