"""Hardware-check logic without a board: output prediction, ordering, log accounting, UID format."""
from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import n7rt_link as L  # noqa: E402
import runtime_check as R  # noqa: E402
import selftest_model as M  # noqa: E402


def board_event(type_: int, fields: dict) -> L.Event:
    wire = L.encode_message(L.BOARD_TO_PC, type_, 0, fields)
    (ev,) = L.Receiver(L.BOARD_TO_PC).feed(wire)
    return ev


def result(frame: dict, flags: int = 0) -> L.Event:
    return board_event(R.RESULT, {"frame_seq": frame["frame_seq"], "status": 0, "flags": flags,
                                  "cycles": 1, "n": 8, "entries": M.frame_entries(frame)})


def tick(t_us: int) -> L.Event:
    return board_event(R.TICK_RESULT, {"t_us": t_us, "status": 0, "flags": 0, "cycles": 1, "n": 8,
                                       "entries": M.tick_entries(t_us)})


def test_expected_follows_the_tick_rule_and_order():
    exp = R.Expected()
    exp.session(10)
    f1 = R.make_frame(1, 5000, 0x123, 0, 8, bytes(8))
    f2 = R.make_frame(2, 25000, 0x124, 0, 2, b"\x01\x02")
    exp.frame(f1)
    exp.frame(f2)
    assert exp.match(result(f1)) is None
    assert exp.match(result(f2)) is not None, "ticks at 10 and 20 ms must come before frame 2"


def test_expected_accepts_rate_limited_logs_and_counts_them():
    exp = R.Expected()
    exp.session(10)
    exp.sim_time(50000)                      # 5 ticks, the 5th one logs
    for t in (10000, 20000, 30000, 40000, 50000):
        assert exp.match(tick(t)) is None    # the log line never came
    assert exp.logs_missing == 1 and not exp.queue


def test_expected_flags_over_budget_but_rejects_other_flags():
    exp = R.Expected()
    exp.session(0)
    f = R.make_frame(1, 0, 0x10, 0, 0, b"")
    exp.frame(f)
    assert exp.match(result(f, flags=0x02)) is None and exp.over_budget == 1
    exp.frame(R.make_frame(2, 0, 0x10, 0, 0, b""))
    assert exp.match(result(R.make_frame(2, 0, 0x10, 0, 0, b""), flags=0x01)) is not None


def test_uid_bytes_matches_hello_ack_layout():
    # boards.json word order, each word little-endian (values seen on board C, 2026-10-04)
    assert R.uid_bytes("002D0046-3233510E-32363331") == "46002d000e51333231333632"
