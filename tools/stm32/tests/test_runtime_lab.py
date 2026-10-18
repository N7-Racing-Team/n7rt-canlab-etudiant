"""stm32lab runtime support without hardware: local board registry, map parser, layout check."""
from __future__ import annotations

import json
import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import stm32lab as lab  # noqa: E402


@pytest.fixture()
def registry(tmp_path, monkeypatch):
    shared = {"boards": {"A": {"stlink_sn": "SN_A", "mcu_uid": "1-2-3"},
                         "B": {"stlink_sn": "SN_B", "mcu_uid": "4-5-6"}}}
    (tmp_path / "boards.json").write_text(json.dumps(shared), encoding="utf-8")
    monkeypatch.setattr(lab, "BOARDS_FILE", tmp_path / "boards.json")
    monkeypatch.setattr(lab, "LOCAL_BOARDS_FILE", tmp_path / "boards.local.json")
    return tmp_path


def test_local_registry_is_merged_and_all_stays_bringup(registry):
    assert sorted(lab.load_boards()) == ["A", "B"]
    local = {"boards": {"C": {"stlink_sn": "SN_C", "mcu_uid": "7-8-9", "images": ["runtime"]}}}
    (registry / "boards.local.json").write_text(json.dumps(local), encoding="utf-8")
    assert sorted(lab.load_boards()) == ["A", "B", "C"]
    assert lab._keys("all") == ["A", "B"]
    assert lab._keys("C") == ["C"]
    with pytest.raises(lab.LabError, match="runtime image only"):
        lab.require_bringup("C")
    lab.require_bringup("A")


def test_local_registry_cannot_redefine_a_shared_board(registry):
    (registry / "boards.local.json").write_text(json.dumps({"boards": {"A": {}}}), encoding="utf-8")
    with pytest.raises(lab.LabError, match="redefines"):
        lab.load_boards()


MAP = """\
Discarded input sections
 .text          0x00000000        0x0 libn7rt_student.a(n7rt_student_glue.c.obj)

Linker script and memory map
.n7rt_student_text
                0x08040040      0x240
 .text.n7rt_publish
                0x08040040       0x14 libn7rt_student.a(n7rt_student_glue.c.obj)
 .text.put_u32  0x08040070       0x4c libn7rt_student.a(n7rt_selftest_student.c.obj)
 .text.main     0x08001fc0      0x2c0 CMakeFiles/n7rt_runtime.dir/runtime/src/n7rt_runtime_main.c.obj
 .debug_info    0x0001976a      0x4bc libn7rt_student.a(n7rt_selftest_student.c.obj)
 .bss.frames_seen
                0x20000000        0x4 libn7rt_student.a(n7rt_selftest_student.c.obj)
"""


def test_map_parser_keeps_loaded_student_sections_only():
    assert lab.map_student_sections(MAP) == [
        (".text.n7rt_publish", 0x08040040, 0x14),
        (".text.put_u32", 0x08040070, 0x4C),
        (".bss.frames_seen", 0x20000000, 0x4),
    ]


@pytest.mark.skipif(not lab.RUNTIME_ELF.is_file() or lab.find_tool("gcc") is None,
                    reason="runtime image not built (stm32lab build --runtime)")
def test_built_runtime_passes_the_layout_check():
    report = lab.runtime_layout_report()
    assert report["ok"], [c for c in report["checks"] if not c[1]]
