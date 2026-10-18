"""Python model of firmware/stm32_h753_bringup/runtime/student/n7rt_selftest_student.c.

The self-test student publishes values the PC can recompute exactly; smoke tests and the soak
compare every RESULT / TICK_RESULT / STUDENT_LOG with this model. Floats are exact (all values
stay below 2**24).
"""
from __future__ import annotations

from typing import Any

LOG_EVERY_TICKS = 5
LOG_LINE_LEN = 120


def frame_entries(frame: dict[str, Any]) -> list[dict[str, Any]]:
    data = bytes.fromhex(frame["data"])
    values = [frame["can_id"] & 0xFFFF, sum(data), frame["len"], frame["dlc"], frame["flags"]]
    entries = [{"slot": i, "kind": 0, "value": float(v)} for i, v in enumerate(values)]
    if data:
        entries += [{"slot": 5, "kind": 0, "value": float(data[0])},
                    {"slot": 6, "kind": 0, "value": float(data[-1])}]
    else:
        entries += [{"slot": 5, "kind": 1, "value": 0.0}, {"slot": 6, "kind": 1, "value": 0.0}]
    entries.append({"slot": 7, "kind": 0, "value": float((frame["t_us"] & 0xFFFFFFFF) % 1_000_000)})
    return entries


def tick_entries(t_us: int) -> list[dict[str, Any]]:
    ms = (t_us & 0xFFFFFFFF) // 1000
    return [{"slot": k, "kind": 0, "value": float((ms + k) % 65536)} for k in range(8)]


def log_text(ticks_seen: int, frames_seen: int) -> str:
    head = f"selftest tick {ticks_seen} frames {frames_seen} "
    return head + "." * (LOG_LINE_LEN - len(head))


class SelftestModel:
    """Expected board output for one session, fed in the order the runtime executes hooks."""

    def __init__(self) -> None:
        self.frames_seen = 0
        self.ticks_seen = 0

    def on_frame(self, frame: dict[str, Any]) -> list[dict[str, Any]]:
        self.frames_seen += 1
        return frame_entries(frame)

    def on_tick(self, t_us: int) -> tuple[list[dict[str, Any]], str | None]:
        self.ticks_seen += 1
        log = log_text(self.ticks_seen, self.frames_seen) if self.ticks_seen % LOG_EVERY_TICKS == 0 else None
        return tick_entries(t_us), log
