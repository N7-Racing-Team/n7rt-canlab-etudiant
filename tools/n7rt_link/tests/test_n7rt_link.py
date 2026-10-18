"""Public N7RT-Link properties: generated inputs, roundtrips, bounds and resynchronisation."""
from __future__ import annotations

import hashlib
import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import n7rt_link as L
import protocol_docs as D


def _pseudo_random(n: int, salt: int) -> bytes:
    out = bytearray()
    while len(out) < n:
        out.extend(hashlib.sha256(f"{salt}:{len(out)}".encode()).digest())
    return bytes(out[:n])


def _fields(spec: L.MessageSpec, salt: int) -> dict:
    """Generate legal fields from the public schema; never store their encoded answers."""
    fields = {f.name: _pseudo_random(f.size, salt + i).hex() if f.is_bytes else
              int.from_bytes(_pseudo_random(f.size, salt + i), "little")
              for i, f in enumerate(spec.fields)}
    if spec.direction == L.PC_TO_BOARD:
        if spec.name == "HELLO":
            fields.update(proto_min=L.PROTO_VERSION, proto_max=L.PROTO_VERSION)
        elif spec.name in ("SESSION_START", "SESSION_STOP"):
            fields["session_id"] |= 1
            if spec.name == "SESSION_START":
                fields.update(mode=1 + salt % 2, tick_period_ms=salt % 1001)
        elif spec.name == "VCAN_FRAME":
            flags = (L.CAN_EXT if salt & 1 else 0) | (L.CAN_FD if salt & 2 else 0)
            if flags & L.CAN_FD:
                flags |= L.CAN_BRS if salt & 4 else 0
                flags |= L.CAN_ESI if salt & 8 else 0
            elif salt & 4:
                flags |= L.CAN_RTR
            dlc = salt % 16
            size = L.can_data_length(dlc, flags)
            fields.update(frame_seq=fields["frame_seq"] | 1, flags=flags, dlc=dlc, len=size,
                          can_id=fields["can_id"] & (L.EXT_ID_MAX if flags & L.CAN_EXT else L.STD_ID_MAX),
                          bus=0, data=_pseudo_random(size, salt).hex())
    elif spec.name == "HELLO_ACK":
        fields.update(reason=salt % 2, proto=L.PROTO_VERSION, student_status=salt % 6,
                      fault_pending=salt % 2, rx_window=1 + salt % 255)
    elif spec.name == "SESSION_ACK":
        fields.update(session_id=fields["session_id"] | 1, op=1 + salt % 2, status=salt % 3)
    elif spec.name == "FAULT_REPORT":
        hook = salt % 4
        fields.update(cause=1 + salt % 2, hook=hook,
                      frame_seq=(fields["frame_seq"] | 1) if hook == 2 else 0)
    elif spec.name in ("RESULT", "TICK_RESULT"):
        delivered = salt % 2 == 0
        count = salt % (L.MAX_ENTRIES + 1) if delivered else 0
        entries = [{"slot": (salt + i) % 256, "kind": i % 2,
                    "value": float(salt - i) if i % 2 == 0 else 0.0} for i in range(count)]
        fields.update(status=0 if delivered else 1, flags=salt % 4,
                      cycles=fields["cycles"] if delivered else 0, n=count, entries=entries)
        if spec.name == "RESULT":
            fields["frame_seq"] |= 1
    elif spec.name == "STUDENT_LOG":
        text = "".join(chr(ord("a") + b % 26) for b in _pseudo_random(salt % 120, salt))
        fields.update(text=text, text_len=len(text.encode("utf-8")))
    elif spec.name == "HEARTBEAT":
        fields.update(state=salt % 3, student_disabled=salt % 2)
    return fields


@pytest.mark.parametrize("spec", L.MESSAGES, ids=lambda spec: f"{spec.direction}-{spec.name}")
def test_each_message_roundtrips_generated_fields_and_extensions(spec):
    for salt in range(32):
        fields = _fields(spec, salt)
        extension = _pseudo_random(salt % 9, salt + 100)
        payload = L.encode_payload(spec, fields, extension)
        assert L.decode_payload(spec, payload) == (fields, extension)
        seq = (salt * 3001) % 65536
        wire = L.encode_message(spec.direction, spec.type, seq, fields, extension)
        events = L.Receiver(spec.direction).feed(wire)
        assert len(events) == 1 and events[0].event == "packet"
        event = events[0]
        assert (event.type, event.seq, event.fields, event.extension) == (spec.type, seq, fields, extension)


@pytest.mark.parametrize("pattern", ("zeros", "nonzero", "random"))
def test_cobs_roundtrip_and_overhead(pattern):
    lengths = set(range(0, 600, 7)) | {1, 253, 254, 255, 256, 509, 510}
    for size in sorted(lengths):
        raw = bytes(size) if pattern == "zeros" else bytes((i % 255) + 1 for i in range(size)) \
            if pattern == "nonzero" else _pseudo_random(size, size)
        encoded = L.cobs_encode(raw)
        assert 0 not in encoded
        assert L.cobs_decode(encoded) == raw
        assert len(encoded) <= size + max(1, -(-size // 254))


def test_crc_detects_each_single_bit_change_in_generated_input():
    for size in (1, 7, 64, L.MAX_PACKET):
        raw = _pseudo_random(size, size)
        checksum = L.crc32(raw)
        for index in range(size):
            for bit in range(8):
                changed = bytearray(raw)
                changed[index] ^= 1 << bit
                assert L.crc32(changed) != checksum


@pytest.mark.parametrize("direction", L.DIRECTIONS)
def test_generated_stream_is_invariant_under_arbitrary_fragmentation(direction):
    specs = [spec for spec in L.MESSAGES if spec.direction == direction]
    stream = b"".join(L.encode_message(direction, spec.type, i % 65536, _fields(spec, i))
                      for i, spec in enumerate(specs * 5))
    whole = L.Receiver(direction).feed(stream)
    assert len(whole) == len(specs) * 5
    for chunk_size in (1, 2, 7, 31, len(stream)):
        receiver = L.Receiver(direction)
        chunked = [event for start in range(0, len(stream), chunk_size)
                   for event in receiver.feed(stream[start:start + chunk_size])]
        assert chunked == whole


@pytest.mark.parametrize("damage", ("crc", "version", "length", "type", "short", "overflow"))
def test_receiver_rejects_generated_damage_then_recovers(damage):
    spec = next(spec for spec in L.MESSAGES if spec.direction == L.PC_TO_BOARD and spec.name == "HEARTBEAT")
    payload = L.encode_payload(spec, _fields(spec, 71))
    packet = L.build_packet(spec.type, 65535, payload)
    if damage == "crc":
        changed = bytearray(packet)
        changed[L.HEADER_SIZE] ^= 1
        damaged = L.encode_wire(changed)
    elif damage == "version":
        damaged = L.encode_wire(L.build_packet(spec.type, 65535, payload, proto=L.PROTO_VERSION + 1))
    elif damage == "length":
        damaged = L.encode_wire(L.build_packet(spec.type, 65535, payload, length=len(payload) + 1))
    elif damage == "type":
        unknown = next(value for value in range(256) if L.spec_for(spec.direction, value) is None)
        damaged = L.encode_wire(L.build_packet(unknown, 65535, payload))
    elif damage == "short":
        damaged = L.encode_wire(packet[:L.MIN_PACKET - 1])
    else:
        damaged = bytes([1]) * (L.MAX_ENCODED + 1) + bytes([L.DELIMITER])
    good = L.encode_message(spec.direction, spec.type, 0, _fields(spec, 73))
    events = L.Receiver(spec.direction).feed(damaged + good)
    assert len(events) == 2 and events[0].event != "packet"
    assert events[1].event == "packet" and events[1].seq == 0
    assert events[1].fields == _fields(spec, 73)


def test_wire_bound_covers_generated_payloads():
    for size in (0, 1, 28, 59, 244, 245, L.MAX_PAYLOAD):
        wire = L.encode_wire(L.build_packet(0x7F, 1, _pseudo_random(size, size)))
        assert len(wire) <= L.wire_bound(size)
    assert L.wire_bound(L.MAX_PAYLOAD) - 1 == L.MAX_ENCODED


def test_message_catalogue_is_consistent():
    for spec in L.MESSAGES:
        assert (spec.type < 0x80) == (spec.direction == L.PC_TO_BOARD)
        assert spec.type not in L.RESERVED_TYPES
        tail = {"can_data": L.MAX_CAN_DATA, "entries": L.MAX_ENTRIES * L.ENTRY.size,
                "text": L.MAX_LOG_TEXT}.get(spec.tail, 0)
        assert spec.fixed_size + tail <= L.MAX_PAYLOAD
    assert len({(spec.direction, spec.type) for spec in L.MESSAGES}) == len(L.MESSAGES)


def test_can_flags_match_dashboard_model():
    sys.path.insert(0, str(D.ROOT))
    can_model = pytest.importorskip("pc.dashboard.can_model")
    assert L.FD_LENGTHS == can_model.FD_LENGTHS
    assert (L.STD_ID_MAX, L.EXT_ID_MAX) == (can_model.STD_ID_MAX, can_model.EXT_ID_MAX)


def test_line_budget_covers_load_with_margin():
    budget = D.budget()
    for direction, data in budget["directions"].items():
        assert data["utilisation"] <= D.MAX_UTILISATION, (direction, data["utilisation"])
    assert budget["max_frames_per_s"] >= D.V1_LOAD["bus_frames_per_s"]


def test_spec_generated_blocks_are_in_sync():
    blocks = D.doc_blocks(D.SPEC_PATH.read_text(encoding="utf-8"))
    assert set(blocks) == set(D.GENERATORS)
    for name, generate in D.GENERATORS.items():
        assert blocks[name] == generate(), f"run: python tools/n7rt_link/protocol_docs.py sync-doc ({name})"
