"""N7RT runtime checks on a real board over the ST-LINK VCP: smoke test, soak, passive watch.

Operator entry point: tools/stm32/stm32lab.py (smoke-test --runtime, soak, watch --runtime).
Every board output is decoded with the reference codec and compared, in execution order, with
what the self-test student must produce for what the PC sent (spec 9.4 tick rule +
selftest_model.py). A lost, duplicated, reordered or corrupted packet is therefore a failure.
"""
from __future__ import annotations

import hashlib
import os
import statistics
import struct
import sys
import time
from collections import deque
from pathlib import Path
from typing import Any, Callable

sys.path.insert(0, str(Path(__file__).resolve().parent))
import n7rt_link as L  # noqa: E402
import selftest_model as M  # noqa: E402
import protocol_docs as D  # noqa: E402

P2B, B2P = L.PC_TO_BOARD, L.BOARD_TO_PC
HELLO, SESSION_START, SESSION_STOP, RESET_REQ = 0x01, 0x02, 0x03, 0x04
VCAN_FRAME, SIM_TIME, HEARTBEAT_PC = 0x10, 0x12, 0x7F
HELLO_ACK, SESSION_ACK, FAULT_REPORT, NACK = 0x81, 0x82, 0x83, 0x84
RESULT, TICK_RESULT, STUDENT_LOG, HEARTBEAT = 0x90, 0x91, 0x92, 0xFF
WINDOW = 32
TICK_MS = 10
HOST_HEARTBEAT_S = 0.25
STALL_S = 0.75                 # spec 10: T_STALL
RSR_SFT = 1 << 24
CORE_HZ = 64_000_000           # runtime v0: SYSCLK = HSI 64 MHz
LINE_BYTES_PER_S = L.BAUD_V0 / L.UART_BITS_PER_BYTE


class RuntimeLink:
    """PC side of N7RT-Link v0 on a serial port: reference codec, non-blocking reads."""

    def __init__(self, port: str, baud: int = L.BAUD_V0) -> None:
        import serial
        self.ser = serial.Serial(port, baud, timeout=0, write_timeout=2)
        self.ser.reset_input_buffer()
        self.rx = L.Receiver(B2P)
        self.seq = 0
        self.bytes_out = 0
        self.bytes_in = 0
        self.frame_errors: dict[str, int] = {}
        self.rejects: list[L.Event] = []
        # Raw frames as received, paired 1:1 with the receiver events: every packet the board sends
        # is re-encoded with the reference codec and must be byte-identical (canonical encoder).
        self._raw = bytearray()
        self._frames: deque[bytes] = deque()
        self.encoded_checked = 0
        self.encoded_mismatches: list[str] = []
        self.write(b"\x00")  # spec 4.3: flush whatever partial frame the board holds

    def close(self) -> None:
        self.ser.close()

    def write(self, data: bytes) -> None:
        self.ser.write(data)
        self.bytes_out += len(data)

    def wire(self, type_: int, fields: dict[str, Any], seq: int | None = None) -> tuple[int, bytes]:
        if seq is None:
            seq, self.seq = self.seq, (self.seq + 1) & 0xFFFF
        payload = L.encode_payload(L.spec_for(P2B, type_), fields, validate=False)
        return seq, L.encode_wire(L.build_packet(type_, seq, payload))

    def send(self, type_: int, fields: dict[str, Any]) -> int:
        seq, wire = self.wire(type_, fields)
        self.write(wire)
        return seq

    def poll(self) -> list[tuple[float, L.Event]]:
        waiting = self.ser.in_waiting
        if not waiting:
            return []
        data = self.ser.read(waiting)
        now = time.perf_counter()
        self.bytes_in += len(data)
        self._raw += data
        *frames, self._raw = self._raw.split(b"\x00")
        self._frames.extend(bytes(f) for f in frames if f)
        out = []
        for ev in self.rx.feed(data):
            raw = self._frames.popleft() if self._frames else None
            if ev.event == "packet":
                self._check_encoding(ev, raw)
                out.append((now, ev))
            elif ev.event == "frame_error":
                self.frame_errors[ev.code] = self.frame_errors.get(ev.code, 0) + 1
            else:
                self.rejects.append(ev)
        return out

    def _check_encoding(self, ev: L.Event, raw: bytes | None) -> None:
        payload = L.encode_payload(L.spec_for(B2P, ev.type), ev.fields, ev.extension, validate=False)
        expect = L.cobs_encode(L.build_packet(ev.type, ev.seq, payload))
        self.encoded_checked += 1
        if raw != expect and len(self.encoded_mismatches) < 10:
            self.encoded_mismatches.append(f"0x{ev.type:02X} seq {ev.seq}: {raw.hex() if raw else None}")


class ExampleModel:
    """Python model of the template student_decoder.c (integration smoke test, decodes nothing)."""

    INIT_LOG = "student_decoder ready"

    def on_frame(self, frame: dict[str, Any]) -> list[dict[str, Any]]:
        data = bytes.fromhex(frame["data"])
        second = {"slot": 1, "kind": 0, "value": float(data[0])} if data else {"slot": 1, "kind": 1, "value": 0.0}
        return [{"slot": 0, "kind": 0, "value": float(frame["len"])}, second]

    def on_tick(self, t_us: int) -> tuple[list[dict[str, Any]], str | None]:
        return [], None


class Expected:
    """Board output predicted from what the PC sent, in the order the runtime must produce it."""

    def __init__(self, model_factory: Callable[[], Any] = M.SelftestModel) -> None:
        self.model_factory = model_factory
        self.queue: deque[tuple[str, int, Any]] = deque()
        self.model = model_factory()
        self.tick_us = 0
        self.next_tick = 0
        self.logs_missing = 0
        self.over_budget = 0

    def session(self, tick_ms: int) -> None:
        self.model = self.model_factory()
        self.tick_us = tick_ms * 1000
        self.next_tick = self.tick_us

    def _advance(self, t_us: int) -> None:
        while self.tick_us and self.next_tick <= t_us:
            entries, log = self.model.on_tick(self.next_tick)
            if log is not None:
                self.queue.append(("log", self.next_tick, log))
            self.queue.append(("tick", self.next_tick, entries))
            self.next_tick += self.tick_us

    def frame(self, f: dict[str, Any]) -> None:
        self._advance(f["t_us"])
        self.queue.append(("result", f["frame_seq"], self.model.on_frame(f)))

    def sim_time(self, t_us: int) -> None:
        self._advance(t_us)

    def match(self, ev: L.Event) -> str | None:
        while self.queue and self.queue[0][0] == "log" and ev.type != STUDENT_LOG:
            self.queue.popleft()  # rate-limited by the board: must show up in HEARTBEAT.log_dropped
            self.logs_missing += 1
        if not self.queue:
            return f"unexpected 0x{ev.type:02X} {ev.fields}"
        kind, key, want = self.queue.popleft()
        f = ev.fields
        if kind == "log":
            ok = ev.type == STUDENT_LOG and f["t_us"] == key and f["text"] == want
        elif kind == "tick":
            ok = ev.type == TICK_RESULT and f["t_us"] == key and f["entries"] == want
        else:
            ok = ev.type == RESULT and f["frame_seq"] == key and f["entries"] == want
        if ev.type in (RESULT, TICK_RESULT):
            self.over_budget += bool(f["flags"] & 0x02)
            ok = ok and f["status"] == 0 and f["n"] == len(want) and (f["flags"] & ~0x02) == 0
        return None if ok else f"expected {kind} {key}, got 0x{ev.type:02X} {f}"


def make_frame(frame_seq: int, t_us: int, can_id: int, flags: int, dlc: int, data: bytes) -> dict[str, Any]:
    return {"frame_seq": frame_seq, "t_us": t_us, "can_id": can_id, "flags": flags, "dlc": dlc,
            "len": L.can_data_length(dlc, flags), "bus": 0, "data": data.hex()}


def uid_bytes(uid_words: str) -> str:
    """boards.json 'w0-w1-w2' -> HELLO_ACK.uid (three little-endian words)."""
    return b"".join(struct.pack("<I", int(w, 16)) for w in uid_words.split("-")).hex()


def challenge(nonce: bytes, image: bytes) -> str:
    return hashlib.sha256(nonce + image).hexdigest()


class _Session:
    """Shared plumbing: heartbeats aside, stray packets recorded, expectation checking."""

    def __init__(self, link: RuntimeLink) -> None:
        self.link = link
        self.beats: list[tuple[float, dict[str, Any]]] = []
        self.stray: list[L.Event] = []
        self.boots: list[L.Event] = []
        self.faults: list[L.Event] = []
        self.backlog: deque[tuple[float, L.Event]] = deque()

    def _route(self, t: float, ev: L.Event) -> bool:
        if ev.type == HEARTBEAT:
            self.beats.append((t, ev.fields))
            return True
        if ev.type == FAULT_REPORT:
            self.faults.append(ev)
        return False

    def poll(self) -> list[tuple[float, L.Event]]:
        """Packets not consumed yet (left by wait) first, then whatever the link delivers now."""
        items = list(self.backlog) + self.link.poll()
        self.backlog.clear()
        return items

    def wait(self, pred: Callable[[L.Event], bool], timeout: float) -> L.Event | None:
        deadline = time.perf_counter() + timeout
        while True:
            items = self.poll()
            for i, (t, ev) in enumerate(items):
                if self._route(t, ev):
                    continue
                if pred(ev):
                    self.backlog.extendleft(reversed(items[i + 1:]))  # keep what came after it
                    return ev
                self.stray.append(ev)
            if time.perf_counter() >= deadline:
                return None
            time.sleep(0.001)

    def exchange(self, exp: Expected, items: list[tuple[int, dict[str, Any]]], timeout: float = 2.0) -> list[str]:
        """Send items (VCAN_FRAME / SIM_TIME), then consume exactly the predicted board output."""
        for type_, fields in items:
            if type_ == VCAN_FRAME:
                exp.frame(fields)
            else:
                exp.sim_time(fields["t_us"])
            self.link.send(type_, fields)
        problems: list[str] = []
        deadline = time.perf_counter() + timeout
        while exp.queue and time.perf_counter() < deadline:
            for t, ev in self.poll():
                if self._route(t, ev):
                    continue
                if ev.type in (RESULT, TICK_RESULT, STUDENT_LOG):
                    problem = exp.match(ev)
                    if problem:
                        problems.append(problem)
                else:
                    self.stray.append(ev)
            time.sleep(0.001)
        if exp.queue:
            problems.append(f"{len(exp.queue)} expected packet(s) never arrived, first {exp.queue[0][:2]}")
        return problems

    def next_beat(self, timeout: float = 1.0) -> dict[str, Any] | None:
        count = len(self.beats)
        self.wait(lambda ev: False, 0.0)
        deadline = time.perf_counter() + timeout
        while len(self.beats) <= count and time.perf_counter() < deadline:
            self.wait(lambda ev: False, 0.02)
        return self.beats[-1][1] if len(self.beats) > count else None


SMOKE_VARIANTS = (  # (can_id, flags, dlc, data): classic, extended, RTR, empty, FD 64 + BRS, FD + ESI, DLC 15
    (0x123, 0, 8, bytes(range(1, 9))),
    (0x1ABCDEF, L.CAN_EXT, 2, b"\xa5\x5a"),
    (0x321, L.CAN_RTR, 4, b""),
    (0x7FF, 0, 0, b""),
    (0x456, L.CAN_FD | L.CAN_BRS, 15, bytes(range(64))),
    (0x010, L.CAN_FD | L.CAN_ESI, 9, bytes(range(100, 112))),
    (0x200, 0, 15, b"\xff" * 8),
)


def smoke(port: str, reset_board: Callable[[], None], image: bytes, uid_words: str,
          emit: Callable[[str], None] = print) -> dict[str, Any]:
    """Bidirectional smoke test of a freshly flashed runtime. Returns a report with 'ok'."""
    checks: list[dict[str, Any]] = []

    def check(name: str, ok: bool, detail: str = "") -> None:
        checks.append({"check": name, "ok": bool(ok), "detail": detail})
        emit(f"   {'PASS' if ok else 'FAIL'}  {name:<28} {detail}")

    link = RuntimeLink(port)
    s = _Session(link)
    report: dict[str, Any] = {"port": port, "checks": checks}
    try:
        reset_board()
        boot = s.wait(lambda e: e.type == HELLO_ACK and e.fields["reason"] == 1, 3.0)
        check("boot_hello_ack", boot is not None, "HELLO_ACK(reason=1) after an ST-LINK reset")
        if boot is None:
            return {**report, "ok": False}
        f = boot.fields
        report["boot"] = f
        s.beats.clear()  # heartbeats of the previous boot are not this run's
        check("identity", f["proto"] == 0 and (f["abi_major"], f["abi_minor"]) == (1, 0)
              and f["dev_id"] == 0x450 and f["uid"] == uid_bytes(uid_words),
              f"runtime {f['rt_major']}.{f['rt_minor']}.{f['rt_patch']} build {f['build_id']}, "
              f"UID {f['uid']}, DEV 0x{f['dev_id']:03X} REV 0x{f['rev_id']:04X}, "
              f"RCC_RSR 0x{f['reset_flags']:08X}")
        check("student_ok", f["student_status"] == 1 and f["fault_pending"] == 0 and f["rx_window"] == WINDOW,
              f"student_status {f['student_status']}, ABI {f['student_abi_major']}.{f['student_abi_minor']}, "
              f"CRC 0x{f['student_crc32']:08X}, W {f['rx_window']}")
        check("boot_challenge", f["challenge"] == challenge(bytes(16), image),
              f"SHA-256(16x00 || {len(image)} B runtime image) matches the flashed .bin")

        nonce = os.urandom(16)
        t0 = time.perf_counter()
        link.send(HELLO, {"proto_min": 0, "proto_max": 0, "nonce": nonce.hex()})
        ack = s.wait(lambda e: e.type == HELLO_ACK and e.fields["reason"] == 0, 1.0)
        check("hello_challenge", ack is not None and ack.fields["challenge"] == challenge(nonce, image),
              f"random nonce, answered in {(time.perf_counter() - t0) * 1000:.1f} ms")

        seq = link.send(VCAN_FRAME, make_frame(1, 0, 0x123, 0, 8, bytes(8)))
        nack = s.wait(lambda e: e.type == NACK, 1.0)
        check("e_state_outside_session",
              nack is not None and nack.fields == {"ref_seq": seq, "ref_type": VCAN_FRAME, "code": 0x10},
              f"{nack.fields if nack else 'no NACK'}")

        link.send(SESSION_START, {"session_id": 0x5A, "mode": 1, "tick_period_ms": TICK_MS})
        ack = s.wait(lambda e: e.type == SESSION_ACK, 1.0)
        check("session_start", ack is not None and ack.fields == {"session_id": 0x5A, "op": 1, "status": 0},
              f"{ack.fields if ack else 'no SESSION_ACK'}, tick {TICK_MS} ms")

        exp = Expected()
        exp.session(TICK_MS)
        fs, t = 0, 0
        items = []
        for _ in range(3):
            for can_id, flags, dlc, data in SMOKE_VARIANTS:
                fs, t = fs + 1, t + 7000
                items.append((VCAN_FRAME, make_frame(fs, t, can_id, flags, dlc, data)))
        problems = s.exchange(exp, items)
        check("frames_and_ticks", not problems,
              f"{fs} frames (classic, ext, RTR, empty, FD64+BRS, FD+ESI, DLC15) -> RESULT, "
              f"{t // 10000} ticks -> TICK_RESULT, logs; exact content and order"
              + (f"; {problems[:3]}" if problems else ""))

        base = s.next_beat()

        def crc_error() -> bytes:   # its seq is consumed and lost: one rx_seq_gaps at the board
            _, wire = link.wire(VCAN_FRAME, make_frame(fs + 1, t, 0x111, 0, 8, bytes(8)))
            packet = bytearray(L.cobs_decode(wire[:-1]))
            packet[-1] ^= 0x01
            return L.encode_wire(bytes(packet))

        def truncated() -> bytes:   # seq consumed and lost as well
            _, wire = link.wire(VCAN_FRAME, make_frame(fs + 1, t, 0x222, 0, 8, bytes(8)))
            return wire[: len(wire) // 2] + b"\x00"

        builders = [(crc_error, 1), (lambda: bytes.fromhex("a55aff1337") + b"\x00", 0),  # noise
                    (lambda: b"\x55" * 300 + b"\x00", 0), (truncated, 1)]            # overflow
        sent_corrupt, seq_lost, recovered = b"", 0, []
        for build, lost in builders:  # seq numbers are taken at send time, in wire order
            chunk = build()
            link.write(chunk)
            sent_corrupt += chunk
            seq_lost += lost
            fs, t = fs + 1, t + 1000
            recovered += s.exchange(exp, [(VCAN_FRAME, make_frame(fs, t, 0x300 + fs, 0, 8, bytes([fs] * 8)))])
        expected_errors = L.Receiver(P2B).feed(sent_corrupt)
        check("recovery_after_corruption", not recovered,
              f"CRC error, noise, 300-byte run, truncated packet: next frame answered each time"
              + (f"; {recovered[:2]}" if recovered else ""))

        seq, link.seq = link.seq, (link.seq + 1) & 0xFFFF   # unknown type: built raw, valid CRC
        link.write(L.encode_wire(L.build_packet(0x42, seq, b"\x01\x02")))
        nack = s.wait(lambda e: e.type == NACK, 1.0)
        check("e_unknown_type", nack is not None and nack.fields == {"ref_seq": seq, "ref_type": 0x42, "code": 0x01},
              f"{nack.fields if nack else 'no NACK'}")
        seq = link.send(VCAN_FRAME, make_frame(fs + 1, t - 1, 0x123, 0, 8, bytes(8)))
        nack = s.wait(lambda e: e.type == NACK, 1.0)
        check("e_time_backwards", nack is not None and nack.fields == {"ref_seq": seq, "ref_type": 0x10, "code": 0x13},
              f"{nack.fields if nack else 'no NACK'}")
        bad = L.encode_payload(L.spec_for(P2B, VCAN_FRAME),
                               make_frame(fs + 1, t, 0x123, 0x20, 8, bytes(8)), validate=False)
        seq, link.seq = link.seq, (link.seq + 1) & 0xFFFF
        link.write(L.encode_wire(L.build_packet(VCAN_FRAME, seq, bad)))
        nack = s.wait(lambda e: e.type == NACK, 1.0)
        check("e_payload", nack is not None and nack.fields == {"ref_seq": seq, "ref_type": 0x10, "code": 0x04},
              f"{nack.fields if nack else 'no NACK'}")

        t += 55000
        problems = s.exchange(exp, [(SIM_TIME, {"t_us": t})])
        check("sim_time_ticks", not problems, f"SIM_TIME +55 ms -> ticks and logs in order" + (f"; {problems[:2]}" if problems else ""))

        end = s.next_beat()
        if base and end:
            want_crc = sum(e.code == L.E_CRC for e in expected_errors)
            want_framing = sum(e.code != L.E_CRC for e in expected_errors)
            delta = {k: end[k] - base[k] for k in ("rx_crc_errors", "rx_framing_errors", "rx_overruns", "rx_seq_gaps")}
            check("board_error_counters",
                  delta == {"rx_crc_errors": want_crc, "rx_framing_errors": want_framing, "rx_overruns": 0,
                            "rx_seq_gaps": seq_lost},
                  f"board deltas {delta}, expected CRC {want_crc} / framing {want_framing} / gaps {seq_lost}")
        else:
            check("board_error_counters", False, "no HEARTBEAT")

        deadline = time.perf_counter() + 2.5
        while len(s.beats) < 8 and time.perf_counter() < deadline:
            s.wait(lambda ev: False, 0.05)
        uptimes = [b["uptime_ms"] for _, b in s.beats]
        gaps = [b - a for a, b in zip(uptimes, uptimes[1:])]
        check("heartbeat_cadence", len(gaps) >= 4 and all(225 <= g <= 275 for g in gaps),
              f"{len(s.beats)} HEARTBEATs, board-clock gaps {min(gaps, default=0)}..{max(gaps, default=0)} ms "
              f"(spec 250 +/- 25 ms)")

        # PC-side disconnection in the middle of a packet, then reconnection: the board keeps its
        # session (it never acts on host silence) and resynchronises on the new port's delimiter.
        before = s.next_beat()
        _, wire = link.wire(VCAN_FRAME, make_frame(fs + 1, t, 0x123, 0, 8, bytes(8)))
        link.write(wire[: len(wire) // 2])
        link.close()
        time.sleep(0.5)
        link = s.link = RuntimeLink(port)
        link.send(HELLO, {"proto_min": 0, "proto_max": 0, "nonce": os.urandom(16).hex()})
        hello = s.wait(lambda e: e.type == HELLO_ACK, 1.0)
        after = s.next_beat()
        fs, t = fs + 1, t + 1000
        problems = s.exchange(exp, [(VCAN_FRAME, make_frame(fs, t, 0x123, 0, 8, bytes(range(8))))])
        check("reconnect_mid_packet",
              hello is not None and hello.fields["reason"] == 0 and before is not None and after is not None
              and after["uptime_ms"] > before["uptime_ms"] and after["state"] == 1 and not problems,
              f"port closed after half a packet, reopened 0.5 s later: HELLO_ACK(reason "
              f"{hello.fields['reason'] if hello else '?'}), session state {after['state'] if after else '?'}, "
              f"uptime {before['uptime_ms'] if before else '?'} -> {after['uptime_ms'] if after else '?'} ms, "
              f"next frame answered" + (f"; {problems[:2]}" if problems else ""))

        link.send(SESSION_STOP, {"session_id": 0x5A})
        ack = s.wait(lambda e: e.type == SESSION_ACK, 1.0)
        check("session_stop", ack is not None and ack.fields == {"session_id": 0x5A, "op": 2, "status": 0},
              f"{ack.fields if ack else 'no SESSION_ACK'}")

        t0 = time.perf_counter()
        link.send(RESET_REQ, {})
        boot = s.wait(lambda e: e.type == HELLO_ACK and e.fields["reason"] == 1, 1.5)
        check("reset_request", boot is not None and bool(boot.fields["reset_flags"] & RSR_SFT),
              f"HELLO_ACK(boot) after {(time.perf_counter() - t0) * 1000:.0f} ms, "
              f"RCC_RSR 0x{boot.fields['reset_flags']:08X}" if boot else "no reboot within 1.5 s")

        check("clean_link", not link.frame_errors and not link.rejects and not s.stray and not s.faults,
              f"PC frame errors {link.frame_errors}, rejects {len(link.rejects)}, "
              f"stray {[hex(e.type) for e in s.stray][:5]}, faults {len(s.faults)}")
    finally:
        link.close()
    report["bytes"] = {"pc_to_board": link.bytes_out, "board_to_pc": link.bytes_in}
    report["ok"] = all(c["ok"] for c in checks)
    return report


def soak_frame(i: int, t_us: int) -> dict[str, Any]:
    data = bytes(((i * 31 + j * 17) ^ (i >> 3)) & 0xFF for j in range(8))
    return make_frame(i, t_us, (0x100 + i * 37) & 0x7FF, 0, 8, data)


def _percentile(values: list[float], q: float) -> float:
    if not values:
        return 0.0
    ordered = sorted(values)
    return ordered[min(len(ordered) - 1, int(q * len(ordered)))]


def soak(port: str, duration_s: float, rate: int, emit: Callable[[str], None] = print,
         progress_s: float = 30.0) -> dict[str, Any]:
    """Sustained V1 load (frames + SIM_TIME every tick + host heartbeats), every output verified."""
    link = RuntimeLink(port)
    s = _Session(link)
    exp = Expected()
    problems: list[str] = []
    rtts: list[float] = []
    inflight: dict[int, float] = {}
    seq_to_frame: dict[int, int] = {}
    counts = {"frames": 0, "sim_time": 0, "results": 0, "ticks": 0, "logs": 0, "nacks": 0,
              "host_heartbeats": 0, "credit_waits": 0}
    report: dict[str, Any] = {"port": port, "rate": rate, "requested_s": duration_s}

    def problem(text: str) -> None:
        if len(problems) < 50:
            problems.append(text)
        report["problem_count"] = report.get("problem_count", 0) + 1

    def handle(t: float, ev: L.Event) -> None:
        if s._route(t, ev):
            return
        if ev.type == HELLO_ACK and ev.fields["reason"] == 1:
            s.boots.append(ev)
            problem(f"board rebooted: RCC_RSR 0x{ev.fields['reset_flags']:08X}")
        elif ev.type == FAULT_REPORT:
            problem(f"FAULT_REPORT {ev.fields}")
        elif ev.type == NACK:
            counts["nacks"] += 1
            inflight.pop(seq_to_frame.pop(ev.fields["ref_seq"], -1), None)
            problem(f"NACK {ev.fields}")
        elif ev.type in (RESULT, TICK_RESULT, STUDENT_LOG):
            if ev.type == RESULT:
                counts["results"] += 1
                sent = inflight.pop(ev.fields["frame_seq"], None)
                if sent is not None:
                    rtts.append((t - sent) * 1000)
            elif ev.type == TICK_RESULT:
                counts["ticks"] += 1
            else:
                counts["logs"] += 1
            mismatch = exp.match(ev)
            if mismatch:
                problem(mismatch)
        else:
            s.stray.append(ev)

    try:
        link.send(HELLO, {"proto_min": 0, "proto_max": 0, "nonce": os.urandom(16).hex()})
        hello = s.wait(lambda e: e.type == HELLO_ACK and e.fields["reason"] == 0, 1.0)
        if hello is None:
            raise RuntimeError("no HELLO_ACK: is the runtime image running on this port?")
        report["hello"] = {k: hello.fields[k] for k in ("rt_major", "rt_minor", "rt_patch", "build_id",
                                                         "uid", "reset_flags", "student_status")}
        link.send(SESSION_START, {"session_id": 0x50AC, "mode": 1, "tick_period_ms": TICK_MS})
        if s.wait(lambda e: e.type == SESSION_ACK, 1.0) is None:
            raise RuntimeError("no SESSION_ACK")
        exp.session(TICK_MS)
        base = s.next_beat()
        if base is None:
            raise RuntimeError("no HEARTBEAT")
        bytes_out0, bytes_in0 = link.bytes_out, link.bytes_in
        start = time.perf_counter()
        next_hb, next_progress = start, start + progress_s
        i, k = 1, 1
        emit(f"   soak {duration_s:.0f} s at {rate} frames/s + SIM_TIME every {TICK_MS} ms "
             f"(self-test student: 8 publications per frame and per tick, 20 log lines/s)")
        while True:
            now = time.perf_counter()
            for t, ev in s.poll():
                handle(t, ev)
            elapsed = now - start
            if elapsed >= duration_s:
                break
            batch = bytearray()
            if now >= next_hb:
                _, w = link.wire(HEARTBEAT_PC, {"host_uptime_ms": int(elapsed * 1000) & 0xFFFFFFFF})
                batch += w
                counts["host_heartbeats"] += 1
                next_hb += HOST_HEARTBEAT_S
            due = int(elapsed * 1e6)
            while True:
                t_frame = i * 1_000_000 // rate
                t_sim = k * TICK_MS * 1000
                if min(t_frame, t_sim) > due:
                    break
                if t_sim <= t_frame:
                    _, w = link.wire(SIM_TIME, {"t_us": t_sim})
                    exp.sim_time(t_sim)
                    counts["sim_time"] += 1
                    k += 1
                else:
                    if len(inflight) >= WINDOW:
                        counts["credit_waits"] += 1
                        break
                    f = soak_frame(i, t_frame)
                    seq, w = link.wire(VCAN_FRAME, f)
                    exp.frame(f)
                    inflight[i] = now
                    seq_to_frame[seq] = i
                    if len(seq_to_frame) > 4096:
                        seq_to_frame.pop(next(iter(seq_to_frame)))
                    counts["frames"] += 1
                    i += 1
                batch += w
            if batch:
                link.write(bytes(batch))
            else:
                time.sleep(0.0005)
            if now >= next_progress:
                next_progress += progress_s
                emit(f"   t={elapsed:5.0f} s  frames {counts['frames']}  results {counts['results']}  "
                     f"ticks {counts['ticks']}  problems {report.get('problem_count', 0)}  "
                     f"RTT p99 {_percentile(rtts[-20000:], 0.99):.2f} ms")
        sent_s = time.perf_counter() - start
        bytes_out = link.bytes_out - bytes_out0
        bytes_in_mark = link.bytes_in
        drain_until = time.perf_counter() + 3.0
        while (inflight or exp.queue) and time.perf_counter() < drain_until:
            for t, ev in s.poll():
                handle(t, ev)
            time.sleep(0.001)
        bytes_in = bytes_in_mark - bytes_in0
        end_beat = s.next_beat()
        link.send(SESSION_STOP, {"session_id": 0x50AC})
        stop = s.wait(lambda e: e.type == SESSION_ACK, 1.0)
        for t, ev in s.poll():
            handle(t, ev)
    finally:
        link.close()

    lost = len(inflight)
    if exp.queue:
        problem(f"{len(exp.queue)} predicted packets never arrived")
    host_gaps = [b - a for (a, _), (b, _) in zip(s.beats, s.beats[1:])]
    uptimes = [bf["uptime_ms"] for _, bf in s.beats]
    resets = len(s.boots) + sum(b < a for a, b in zip(uptimes, uptimes[1:]))
    delta = {key: end_beat[key] - base[key] for key in
             ("rx_frames", "rx_crc_errors", "rx_framing_errors", "rx_overruns", "rx_seq_gaps", "log_dropped")} \
        if end_beat else {}
    budget = D.budget()
    report.update({
        "duration_s": round(sent_s, 1),
        "counts": counts,
        "lost_frames": lost,
        "board_deltas": delta,
        "board_queue_high_water": end_beat["queue_high_water"] if end_beat else None,
        "board_max_hook_us": round(end_beat["max_cycles"] / (CORE_HZ / 1e6), 1) if end_beat else None,
        "logs_missing": exp.logs_missing,
        "over_budget_hooks": exp.over_budget,
        "pc_frame_errors": link.frame_errors,
        "pc_rejects": len(link.rejects),
        "stray": [hex(e.type) for e in s.stray][:10],
        "faults": len(s.faults),
        "resets": resets,
        "heartbeats": len(s.beats),
        "heartbeat_max_gap_s": round(max(host_gaps, default=0.0), 3),
        "rtt_ms": {"p50": round(_percentile(rtts, 0.50), 2), "p95": round(_percentile(rtts, 0.95), 2),
                   "p99": round(_percentile(rtts, 0.99), 2), "max": round(max(rtts, default=0.0), 2),
                   "mean": round(statistics.fmean(rtts), 2) if rtts else 0.0},
        "throughput": {
            "observed_pc_to_board_Bps": round(bytes_out / sent_s),
            "observed_board_to_pc_Bps": round(bytes_in / sent_s),
            "observed_pc_to_board_util": round(bytes_out / sent_s / LINE_BYTES_PER_S, 4),
            "observed_board_to_pc_util": round(bytes_in / sent_s / LINE_BYTES_PER_S, 4),
            "computed_worst_case_pc_to_board_Bps": budget["directions"][P2B]["bytes_per_s"],
            "computed_worst_case_board_to_pc_Bps": budget["directions"][B2P]["bytes_per_s"],
            "line_capacity_Bps": LINE_BYTES_PER_S,
            "frames_per_s": round(counts["frames"] / sent_s, 1),
        },
        "session_stop_ack": bool(stop and stop.fields["status"] == 0),
        "problems": problems,
    })
    criteria = {
        "duration": sent_s >= duration_s,
        "rate": counts["frames"] >= 0.99 * rate * duration_s,
        "no_lost_frame": lost == 0 and counts["results"] == counts["frames"],
        "outputs_exact": not problems,
        "no_nack": counts["nacks"] == 0,
        "pc_link_clean": not link.frame_errors and not link.rejects and not s.stray,
        "board_counters_clean": bool(delta) and delta["rx_crc_errors"] == 0 and delta["rx_framing_errors"] == 0
        and delta["rx_overruns"] == 0 and delta["rx_seq_gaps"] == 0,
        "board_received_all": bool(delta) and delta["rx_frames"] == counts["frames"],
        "logs_accounted": bool(delta) and delta["log_dropped"] == exp.logs_missing,
        "no_reset_no_fault": resets == 0 and not s.faults,
        "no_stall": bool(host_gaps) and max(host_gaps) <= STALL_S,
        "session_alive": report["session_stop_ack"],
    }
    report["criteria"] = criteria
    report["ok"] = all(criteria.values())
    return report


def watch(resolve_port: Callable[[], str | None], seconds: float | None,
          on_event: Callable[[dict[str, Any]], None], stop=None) -> int:
    """Passive monitor (no byte sent): boots, faults, reboots inferred from the heartbeat uptime,
    silences longer than T_STALL, USB loss. Returns the number of incidents."""
    import serial

    incidents = 0
    ser = None
    rx = L.Receiver(B2P)
    last_rx: float | None = None
    silent = False
    uptime: int | None = None
    start = time.monotonic()
    next_scan = 0.0
    present = True
    try:
        while not (stop and stop.is_set()) and (seconds is None or time.monotonic() - start < seconds):
            now = time.monotonic()
            if now >= next_scan:
                next_scan = now + 1.0
                port = resolve_port()
                if (port is not None) != present:
                    present = port is not None
                    incidents += port is None
                    on_event({"event": "usb_back" if port else "usb_lost", "com": port})
                if port is None and ser:
                    ser.close()
                    ser = None
                if port and ser is None:
                    try:
                        ser = serial.Serial(port, L.BAUD_V0, timeout=0)
                        rx = L.Receiver(B2P)
                        last_rx = time.monotonic()
                        on_event({"event": "port_open", "com": port})
                    except serial.SerialException as exc:
                        on_event({"event": "port_error", "com": port, "error": str(exc).splitlines()[0][:160]})
                        incidents += 1
            if ser is not None:
                try:
                    data = ser.read(ser.in_waiting or 1)
                except serial.SerialException:
                    ser.close()
                    ser = None
                    continue
                if data:
                    if silent:
                        on_event({"event": "resumed", "gap_s": round(time.monotonic() - last_rx, 2)})
                        silent = False
                    last_rx = time.monotonic()
                for ev in rx.feed(data):
                    if ev.event != "packet":
                        incidents += 1
                        on_event({"event": "link_error", "kind": ev.event, "code": ev.code})
                    elif ev.type == HELLO_ACK and ev.fields["reason"] == 1:
                        incidents += 1
                        on_event({"event": "boot", "reset_flags": f"0x{ev.fields['reset_flags']:08X}",
                                  "previous_uptime_ms": uptime, "student_status": ev.fields["student_status"],
                                  "fault_pending": ev.fields["fault_pending"]})
                        uptime = 0
                    elif ev.type == FAULT_REPORT:
                        incidents += 1
                        on_event({"event": "fault", **ev.fields})
                    elif ev.type == HEARTBEAT:
                        if uptime is not None and ev.fields["uptime_ms"] < uptime:
                            incidents += 1
                            on_event({"event": "reboot_inferred", "uptime_ms": ev.fields["uptime_ms"],
                                      "previous_uptime_ms": uptime})
                        uptime = ev.fields["uptime_ms"]
                if last_rx is not None and not silent and time.monotonic() - last_rx > STALL_S:
                    silent = True
                    incidents += 1
                    on_event({"event": "silence", "after_s": round(time.monotonic() - last_rx, 2)})
            time.sleep(0.02)
    except KeyboardInterrupt:
        pass
    finally:
        if ser:
            ser.close()
    on_event({"event": "summary", "incidents": incidents, "uptime_ms": uptime})
    return incidents


# --- S1.3: student image built by STM32CubeIDE against the precompiled runtime -------------------

def student_smoke(port: str, reset_board: Callable[[], None], runtime_image: bytes, student_crc32: int,
                  uid_words: str, emit: Callable[[str], None] = print) -> dict[str, Any]:
    """Student image (CubeIDE build): handshake and generic starter behavior, without fixed fixtures."""
    checks: list[dict[str, Any]] = []

    def check(name: str, ok: bool, detail: str = "") -> None:
        checks.append({"check": name, "ok": bool(ok), "detail": detail})
        emit(f"   {'PASS' if ok else 'FAIL'}  {name:<30} {detail}")

    link = RuntimeLink(port)
    s = _Session(link)
    report: dict[str, Any] = {"port": port, "checks": checks}
    try:
        reset_board()
        boot = s.wait(lambda e: e.type == HELLO_ACK and e.fields["reason"] == 1, 3.0)
        check("boot_hello_ack", boot is not None, "HELLO_ACK(reason=1) after an ST-LINK reset")
        if boot is None:
            return {**report, "ok": False}
        f = boot.fields
        report["boot"] = f
        s.beats.clear()
        check("runtime_unchanged", f["challenge"] == challenge(bytes(16), runtime_image)
              and f["uid"] == uid_bytes(uid_words),
              f"runtime {f['rt_major']}.{f['rt_minor']}.{f['rt_patch']} build {f['build_id']}: "
              f"SHA-256(16x00 || runtime) = precompiled image, UID {f['uid']}")
        check("student_image_loaded", f["student_status"] == 1
              and (f["student_abi_major"], f["student_abi_minor"]) == (1, 0)
              and f["student_crc32"] == student_crc32,
              f"student_status {f['student_status']}, ABI {f['student_abi_major']}.{f['student_abi_minor']}, "
              f"CRC 0x{f['student_crc32']:08X} (CubeIDE image 0x{student_crc32:08X})")
        nonce = os.urandom(16)
        link.send(HELLO, {"proto_min": 0, "proto_max": 0, "nonce": nonce.hex()})
        ack = s.wait(lambda e: e.type == HELLO_ACK and e.fields["reason"] == 0, 1.0)
        check("hello_challenge", ack is not None and ack.fields["challenge"] == challenge(nonce, runtime_image),
              "random nonce")

        link.send(SESSION_START, {"session_id": 0x51, "mode": 1, "tick_period_ms": TICK_MS})
        log = s.wait(lambda e: e.type == STUDENT_LOG, 1.0)
        ack = s.wait(lambda e: e.type == SESSION_ACK, 1.0)
        logged = log.fields["text"] if log else None
        check("session_and_student_init", logged == ExampleModel.INIT_LOG
              and ack is not None and ack.fields["status"] == 0,
              f"student_init() logged {logged!r}, SESSION_ACK status {ack.fields['status'] if ack else '?'}")
        exp = Expected(ExampleModel)
        exp.session(TICK_MS)
        fs, t, items = 0, 0, []
        for _ in range(3):
            for can_id, flags, dlc, data in SMOKE_VARIANTS:
                fs, t = fs + 1, t + 7000
                items.append((VCAN_FRAME, make_frame(fs, t, can_id, flags, dlc, data)))
        problems = s.exchange(exp, items)
        check("student_decoder_results", not problems,
              f"{fs} VCAN_FRAME -> student_on_frame() -> RESULT exact, {t // 10000} TICK_RESULT"
              + (f"; {problems[:2]}" if problems else ""))

        check("board_encoder_byte_exact", link.encoded_checked > 0 and not link.encoded_mismatches,
              f"{link.encoded_checked} board packets re-encoded identically by the reference codec"
              + (f"; {link.encoded_mismatches[:2]}" if link.encoded_mismatches else ""))
        check("clean_link", not link.frame_errors and not link.rejects and not s.stray and not s.faults,
              f"PC frame errors {link.frame_errors}, rejects {len(link.rejects)}, "
              f"stray {[hex(e.type) for e in s.stray][:5]}, faults {len(s.faults)}")
    finally:
        link.close()
    report["ok"] = all(c["ok"] for c in checks)
    return report
