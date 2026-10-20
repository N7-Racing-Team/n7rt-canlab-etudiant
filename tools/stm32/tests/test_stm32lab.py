"""stm32lab parsing and guards, without hardware (STM32_Programmer_CLI output is replayed)."""
from __future__ import annotations

import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import stm32lab as lab  # noqa: E402

LIST_ST = """
-------- Connected ST-LINK Probes List --------
ST-Link Probe 0 :
   ST-LINK SN  : 002D00263234510737333934
   ST-LINK FW  : V3J17M10
   Access Port Number  : 3
   Board Name  : NUCLEO-H753ZI
ST-Link Probe 1 :
   ST-LINK SN  : 003B002D4142500720353451
   ST-LINK FW  : V3J17M10
   Access Port Number  : 3
   Board Name  : NUCLEO-H753ZI
"""
READ_UID = """
ST-LINK SN  : 002D00263234510737333934
Device ID   : 0x450
Revision ID : Rev V
Device name : STM32H7xx
0x1FF1E800 : 0028003C 32335101 34343734
"""


def test_list_probes(monkeypatch) -> None:
    monkeypatch.setattr(lab, "programmer", lambda *a, **k: (0, LIST_ST))
    assert lab.list_probes() == [
        {"sn": "002D00263234510737333934", "fw": "V3J17M10", "board": "NUCLEO-H753ZI"},
        {"sn": "003B002D4142500720353451", "fw": "V3J17M10", "board": "NUCLEO-H753ZI"},
    ]


def test_read_target(monkeypatch) -> None:
    monkeypatch.setattr(lab, "programmer", lambda *a, **k: (0, READ_UID))
    assert lab.read_target("x") == {"dev_id": 0x450, "rev": "Rev V", "uid": "0028003C-32335101-34343734"}
    monkeypatch.setattr(lab, "programmer", lambda *a, **k: (1, "Error: No debug probe detected."))
    with pytest.raises(lab.LabError):
        lab.read_target("x")


def test_check_image_refuses_other_board_tag(tmp_path, monkeypatch) -> None:
    elf, binary = tmp_path / "fw.elf", tmp_path / "fw.bin"
    elf.write_bytes(b"elf")
    monkeypatch.setattr(lab, "image_paths", lambda key, can_bus=False: (elf, binary))
    binary.write_bytes(b"....STM32LAB_BOARD=B\x00..STM32LAB_CAN=loopback\x00..")
    with pytest.raises(lab.LabError, match="expected A"):
        lab.check_image("A")
    binary.write_bytes(b"....STM32LAB_BOARD=A\x00..STM32LAB_CAN=loopback\x00..")
    assert lab.check_image("A") == elf


def test_check_image_refuses_other_can_variant(tmp_path, monkeypatch) -> None:
    elf, binary = tmp_path / "fw.elf", tmp_path / "fw.bin"
    elf.write_bytes(b"elf")
    monkeypatch.setattr(lab, "image_paths", lambda key, can_bus=False: (elf, binary))
    binary.write_bytes(b"....STM32LAB_BOARD=A\x00..STM32LAB_CAN=loopback\x00..")
    with pytest.raises(lab.LabError, match="expected bus"):
        lab.check_image("A", can_bus=True)
    binary.write_bytes(b"....STM32LAB_BOARD=A\x00..STM32LAB_CAN=bus\x00..")
    assert lab.check_image("A", can_bus=True) == elf
    with pytest.raises(lab.LabError, match="expected loopback"):
        lab.check_image("A")


def _can(**kw) -> dict:
    base = {"tx_id": "0x101", "tx_counter": 0, "tx_queued": 0, "tx_ok": 0, "tx_fifo_full": 0, "rx": 0,
            "rx_lost": 0, "rx_bad": 0, "rx_overflow": 0, "rx_restarts": 0, "rx_other": 0, "tec": 0, "rec": 0,
            "bo": 0, "bus_off_events": 0, **{n: 0 for n in lab.CAN_ERR_FIELDS}}
    return {**base, **kw}


def test_evaluate_can_direction_pass() -> None:
    base = {"A": _can(tx_counter=5, tx_queued=5, tx_ok=5), "B": _can(tx_id="0x102", rx=5)}
    final = {"A": _can(tx_counter=105, tx_queued=105, tx_ok=105), "B": _can(tx_id="0x102", rx=105)}
    frames = [{"id": "0x101", "from": "A", "counter": c} for c in range(5, 105)]
    checks = lab.evaluate_can_direction("A", "B", base, final, frames, 10.0)
    assert [name for name, ok, _ in checks if not ok] == []


def test_evaluate_can_direction_no_ack_gives_hint() -> None:
    base = {"A": _can(), "B": _can(tx_id="0x102")}
    final = {"A": _can(tx_counter=4, tx_queued=4, tx_fifo_full=96, err_ack=300, tec=128), "B": _can(tx_id="0x102")}
    failed = [name for name, ok, _ in lab.evaluate_can_direction("A", "B", base, final, [], 10.0) if not ok]
    assert "sender_frames_acknowledged" in failed and "error_active_A" in failed and "no_protocol_error_A" in failed
    assert any(h.startswith("err_ack: no ACK") for h in lab.can_hints("A", "B", base, final))


def test_evaluate_can_direction_detects_gap() -> None:
    base = {"A": _can(), "B": _can(tx_id="0x102")}
    final = {"A": _can(tx_counter=100, tx_queued=100, tx_ok=100), "B": _can(tx_id="0x102", rx=99, rx_lost=1)}
    frames = [{"id": "0x101", "from": "A", "counter": c} for c in range(100) if c != 50]
    failed = [name for name, ok, _ in lab.evaluate_can_direction("A", "B", base, final, frames, 10.0) if not ok]
    assert {"receiver_count_equals_sent", "telemetry_frames_consecutive", "no_loss_no_bad_frame"} <= set(failed)


def test_detect_report_statuses(monkeypatch) -> None:
    uids = {"002D00263234510737333934": "0028003C-32335101-34343734",
            "003B002D4142500720353451": "DEADBEEF-00000000-00000000"}
    monkeypatch.setattr(lab, "find_tool", lambda key: ("test", Path("tool.exe")))
    monkeypatch.setattr(lab, "tool_version", lambda key, path: "1.0")
    monkeypatch.setattr(lab, "com_ports_by_sn", lambda: {"002D00263234510737333934": "COM3"})
    monkeypatch.setattr(lab, "list_probes", lambda: [{"sn": sn, "fw": "V3J17M10", "board": "NUCLEO-H753ZI"}
                                                     for sn in uids])
    monkeypatch.setattr(lab, "read_target", lambda sn: {"dev_id": 0x450, "rev": "Rev V", "uid": uids[sn]})
    report = lab.detect_report()
    assert [(p["board"], p["status"], p["com"]) for p in report["probes"]] == [
        ("A", "ok", "COM3"), ("B", "uid_mismatch", None)]
    assert report["ok"] is False and report["missing_boards"] == []


def test_reset_cause() -> None:
    # 0x01460000: captured after `STM32_Programmer_CLI -rst` (smoke-test) on 2026-09-25
    assert lab.reset_cause("0x01460000") == "CPU+PIN+SFT: software (SYSRESETREQ: debugger, programmer or firmware)"
    assert lab.reset_cause("0x00460000").endswith("NRST pin (RESET button B2, or ST-LINK hardware reset)")
    assert lab.reset_cause("0x00E60000").endswith("power-on")
    assert lab.reset_cause(None).startswith("unknown")


def test_parse_event_recovers_boot_glued_to_a_line_cut_by_reset() -> None:
    # captured on board A, 2026-09-28 17:10:31: the smoke-test reset cut a heartbeat line
    glued = ('{"ts":"2056","node":"stm32_a","type":"heartbeat","state":"running","meta":{"board":"A","seq":2,'
             '"uptime_ms":2056�{"ts":"0","node":"stm32_a","type":"state","state":"boot","meta":{"board":"A",'
             '"reset_flags":"0x01460000"}}')
    assert lab.parse_event(glued)["state"] == "boot"
    assert lab.parse_event('{"ts":"1","type":"heartbeat"}')["type"] == "heartbeat"
    assert lab.parse_event("garbage") is None and lab.parse_event('{"ts":"1", cut') is None


def _beat(uptime: int) -> str:
    return ('{"ts":"%d","node":"stm32_b","type":"heartbeat","state":"running",'
            '"meta":{"board":"B","seq":%d,"uptime_ms":%d}}' % (uptime, uptime // 1000, uptime))


def test_reboot_detector() -> None:
    d = lab.RebootDetector(silence_s=2.5)
    assert d.feed(_beat(5000), 0.0) == [] and d.feed(_beat(6000), 1.0) == []
    assert d.idle(3.0) == []
    boot = ('{"ts":"0","node":"stm32_b","type":"state","state":"boot",'
            '"meta":{"board":"B","build_system":"cmake","reset_flags":"0x00460000"}}')
    [ev] = d.feed(boot, 2.0)
    assert ev["event"] == "boot" and ev["previous_uptime_ms"] == 6000 and "NRST pin" in ev["cause"]
    assert d.feed(_beat(1000), 3.0) == []                 # first beat after a seen boot: no double count
    assert [e["event"] for e in d.feed(_beat(400), 4.0)] == ["reboot_inferred"]  # boot line lost
    assert d.idle(6.6) == [{"event": "silence", "after_s": 2.6}] and d.idle(7.0) == []
    assert d.feed("garbage", 8.0) == [{"event": "resumed", "gap_s": 4.0}]
    assert d.incidents == 3


def test_reboot_detector_board_silent_from_start() -> None:
    d = lab.RebootDetector(silence_s=2.5)
    assert d.idle(100.0) == []                            # port not open yet: nothing to judge
    d.port_opened(100.0)
    assert d.idle(102.0) == [] and d.idle(103.0) == [{"event": "silence", "after_s": 3.0}]
    assert d.incidents == 1


def test_reboot_detector_continuity() -> None:
    d = lab.RebootDetector()
    for i in range(4):
        d.feed(_beat(100_000 + 1000 * i), 10.0 + i)
    assert d.continuity(20.0) == {"uptime_ms": 103_000, "uptime_gain_ms": 3000, "host_elapsed_ms": 3000}


@pytest.mark.parametrize("key, folder", [
    ("gcc", "Arm GNU Toolchain arm-none-eabi/14.3 rel1/bin"),
    ("gcc", "GNU Arm Embedded Toolchain/10 2021.10/bin"),
    ("programmer", "STMicroelectronics/STM32Cube/STM32CubeProgrammer/bin"),
])
def test_standalone_installers_are_found_without_path(tmp_path, monkeypatch, key, folder) -> None:
    exe = tmp_path / "Program Files (x86)" / folder / (lab.TOOLS[key][0] + lab.EXE)
    exe.parent.mkdir(parents=True)
    exe.write_bytes(b"")
    for name in ("ProgramFiles", "ProgramW6432", "LOCALAPPDATA", f"STM32LAB_{key.upper()}"):
        monkeypatch.delenv(name, raising=False)
    monkeypatch.setenv("ProgramFiles(x86)", str(tmp_path / "Program Files (x86)"))
    monkeypatch.setenv("CUBE_BUNDLE_PATH", str(tmp_path / "bundles"))
    monkeypatch.setattr(lab, "ST_ROOT", tmp_path / "ST")
    monkeypatch.setattr(lab.shutil, "which", lambda *a, **k: None)
    monkeypatch.setattr(lab, "_tool_cache", {})
    assert lab.find_tool(key)[1] == exe


def test_newest_installed_toolchain_wins(tmp_path, monkeypatch) -> None:
    for version in ("12.2 rel1", "14.3 rel1", "13.3 rel1"):
        exe = tmp_path / "Arm GNU Toolchain arm-none-eabi" / version / "bin" / ("arm-none-eabi-gcc" + lab.EXE)
        exe.parent.mkdir(parents=True)
        exe.write_bytes(b"")
    monkeypatch.setenv("ProgramFiles", str(tmp_path))
    monkeypatch.delenv("ProgramFiles(x86)", raising=False)
    monkeypatch.delenv("ProgramW6432", raising=False)
    monkeypatch.delenv("STM32LAB_GCC", raising=False)
    monkeypatch.setenv("CUBE_BUNDLE_PATH", str(tmp_path / "bundles"))
    monkeypatch.setattr(lab, "ST_ROOT", tmp_path / "ST")
    monkeypatch.setattr(lab.shutil, "which", lambda *a, **k: None)
    monkeypatch.setattr(lab, "_tool_cache", {})
    assert lab.find_tool("gcc")[1].parts[-3] == "14.3 rel1"


@pytest.mark.skipif(sys.platform != "win32", reason="Windows registry")
def test_registry_path_is_read_fresh() -> None:
    # the saved user/system PATH, not this process's copy: a PATH edited after launch is seen
    assert "Windows" in lab._registry_path()
