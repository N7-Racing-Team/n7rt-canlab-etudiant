"""N7RT-Link v0 reference codec (Python standard library only).

Normative contract: docs/n7rt-link-v0.md. Public checks exercise codec properties;
exact conformance fixtures are maintained separately in the instructor repository.

This module is the executable reference for the future C runtime and Rust client. It encodes
packets byte for byte as the specification requires, and its stream receiver reports the events
(packet, frame error, reject) that every conforming implementation must report for the same bytes.
Session state (credits, time order, E_STATE...) is not modelled here: see spec section 8.
"""
from __future__ import annotations

import struct
from dataclasses import dataclass, field
from typing import Any

# --- Wire constants (spec sections 2-4) -------------------------------------------------------
PROTO_VERSION = 0
HEADER = struct.Struct("<BBHH")          # proto, type, seq, len
HEADER_SIZE = HEADER.size                # 6
CRC_SIZE = 4
MIN_PACKET = HEADER_SIZE + CRC_SIZE      # 10
MAX_PAYLOAD = 255
MAX_PACKET = HEADER_SIZE + MAX_PAYLOAD + CRC_SIZE  # 265
MAX_ENCODED = MAX_PACKET + 2             # 267: canonical COBS of 265 bytes, delimiter excluded
DELIMITER = 0x00
BAUD_V0 = 921_600
UART_BITS_PER_BYTE = 10                  # 8N1: start + 8 data + stop

PC_TO_BOARD = "pc_to_board"
BOARD_TO_PC = "board_to_pc"
DIRECTIONS = (PC_TO_BOARD, BOARD_TO_PC)

# Framing errors: the frame is dropped and counted, never answered (spec section 5).
E_OVERFLOW = "E_OVERFLOW"
E_COBS = "E_COBS"
E_SHORT = "E_SHORT"
E_CRC = "E_CRC"
FRAMING_ERRORS = (E_OVERFLOW, E_COBS, E_SHORT, E_CRC)

# Reject codes: the header passed the CRC, so the receiver can answer NACK(code) (spec section 10).
NACK_CODES: dict[str, int] = {
    "E_UNKNOWN_TYPE": 0x01,
    "E_VERSION": 0x02,
    "E_LENGTH": 0x03,
    "E_PAYLOAD": 0x04,
    "E_STATE": 0x10,
    "E_CREDIT": 0x11,
    "E_TIME": 0x13,
}

# --- CAN constants, identical to pc/dashboard/can_model.py (spec section 7) ------------------
CAN_EXT = 0x01
CAN_RTR = 0x02
CAN_FD = 0x04
CAN_BRS = 0x08
CAN_ESI = 0x10
CAN_FLAGS_MASK = 0x1F
STD_ID_MAX = 0x7FF
EXT_ID_MAX = 0x1FFFFFFF
CLASSIC_MAX_LEN = 8
FD_LENGTHS = (0, 1, 2, 3, 4, 5, 6, 7, 8, 12, 16, 20, 24, 32, 48, 64)  # index = DLC
MAX_CAN_DATA = 64
MAX_ENTRIES = 8
MAX_LOG_TEXT = 120
ENTRY = struct.Struct("<BBf")            # slot, kind, value (IEEE-754 binary32)
RESULT_FLAG_PUBLISH_OVERFLOW = 0x01
RESULT_FLAG_OVER_BUDGET = 0x02


# --- CRC-32/ISO-HDLC ---------------------------------------------------------------------------
def _crc_table() -> tuple[int, ...]:
    table = []
    for i in range(256):
        c = i
        for _ in range(8):
            c = (c >> 1) ^ 0xEDB88320 if c & 1 else c >> 1
        table.append(c)
    return tuple(table)


_CRC_TABLE = _crc_table()


def crc32(data: bytes) -> int:
    """CRC-32/ISO-HDLC: reflected 0x04C11DB7, init and xorout 0xFFFFFFFF."""
    c = 0xFFFFFFFF
    for b in data:
        c = _CRC_TABLE[(c ^ b) & 0xFF] ^ (c >> 8)
    return c ^ 0xFFFFFFFF


# --- COBS (canonical encoder, liberal decoder) -------------------------------------------------
class CobsError(ValueError):
    pass


def cobs_encode(data: bytes) -> bytes:
    """Canonical COBS: no empty block is emitted after a final 254-byte block."""
    out = bytearray()
    block = bytearray()
    last = len(data) - 1
    for i, b in enumerate(data):
        if b == 0:
            out.append(len(block) + 1)
            out += block
            block.clear()
            continue
        block.append(b)
        if len(block) == 254:
            out.append(0xFF)
            out += block
            block.clear()
            if i == last:
                return bytes(out)
    out.append(len(block) + 1)
    out += block
    return bytes(out)


def cobs_decode(data: bytes) -> bytes:
    if not data:
        raise CobsError("empty frame")
    out = bytearray()
    i = 0
    n = len(data)
    while i < n:
        code = data[i]
        if code == 0:
            raise CobsError(f"zero byte at offset {i}")
        end = i + code
        if end > n:
            raise CobsError(f"block at offset {i} needs {code - 1} bytes, {n - i - 1} left")
        chunk = data[i + 1:end]
        if 0 in chunk:
            raise CobsError(f"zero byte inside block at offset {i}")
        out += chunk
        i = end
        if code != 0xFF and i < n:
            out.append(0)
    return bytes(out)


def wire_bound(payload_len: int) -> int:
    """Largest possible wire size (COBS + delimiter) of a packet carrying payload_len bytes."""
    n = HEADER_SIZE + payload_len + CRC_SIZE
    return n + -(-n // 254) + 1


# --- Message catalogue (spec section 6) ----------------------------------------------------------
_TYPE_NAMES = {"B": "u8", "H": "u16", "I": "u32", "Q": "u64", "f": "f32"}


@dataclass(frozen=True)
class Field:
    name: str
    fmt: str   # one little-endian struct item: B, H, I, Q, f or Ns
    doc: str

    @property
    def size(self) -> int:
        return struct.calcsize("<" + self.fmt)

    @property
    def is_bytes(self) -> bool:
        return self.fmt.endswith("s")

    @property
    def type_name(self) -> str:
        return f"u8[{self.fmt[:-1]}]" if self.is_bytes else _TYPE_NAMES[self.fmt]


@dataclass(frozen=True)
class MessageSpec:
    type: int
    name: str
    direction: str
    fields: tuple[Field, ...]
    tail: str | None = None   # "can_data" | "entries" | "text"
    doc: str = ""

    @property
    def layout(self) -> struct.Struct:
        return struct.Struct("<" + "".join(f.fmt for f in self.fields))

    @property
    def fixed_size(self) -> int:
        return self.layout.size


F = Field
MESSAGES: tuple[MessageSpec, ...] = (
    MessageSpec(0x01, "HELLO", PC_TO_BOARD, (
        F("proto_min", "B", "plus petite version de protocole acceptée par le PC"),
        F("proto_max", "B", "plus grande version acceptée (≥ proto_min)"),
        F("nonce", "16s", "défi aléatoire, renvoyé haché dans HELLO_ACK.challenge"),
    ), doc="Ouvre ou rouvre le dialogue ; autorisé dans tout état."),
    MessageSpec(0x02, "SESSION_START", PC_TO_BOARD, (
        F("session_id", "I", "identifiant de session ≠ 0"),
        F("mode", "B", "1 = exploration, 2 = qualification"),
        F("tick_period_ms", "H",
          "période de student_on_tick en temps bus, 0 = pas de tick, ≤ 1000"),
    ), doc="Réinitialise la RAM étudiante et appelle student_init()."),
    MessageSpec(0x03, "SESSION_STOP", PC_TO_BOARD, (
        F("session_id", "I", "session à fermer (≠ 0)"),
    )),
    MessageSpec(0x04, "RESET_REQ", PC_TO_BOARD, (),
                doc="La carte se réinitialise en ≤ 50 ms ; aucune réponse avant HELLO_ACK(boot)."),
    MessageSpec(0x05, "NACK", PC_TO_BOARD, (
        F("ref_seq", "H", "seq de l'en-tête du paquet refusé"),
        F("ref_type", "B", "type du paquet refusé"),
        F("code", "B", "code d'erreur (§11)"),
    )),
    MessageSpec(0x10, "VCAN_FRAME", PC_TO_BOARD, (
        F("frame_seq", "I", "numéro de trame dans la session, 1, 2, 3… (≠ 0)"),
        F("t_us", "Q", "instant de réception, µs de temps bus depuis le début de session"),
        F("can_id", "I", "identifiant 11 bits, ou 29 bits si CAN_EXT"),
        F("flags", "B", "CAN_EXT 0x01 · CAN_RTR 0x02 · CAN_FD 0x04 · CAN_BRS 0x08 · CAN_ESI 0x10"),
        F("dlc", "B", "code DLC tel que transmis sur le bus (0..15)"),
        F("len", "B", "nombre d'octets de data (fonction de dlc et flags, §8.2)"),
        F("bus", "B", "0 en v0"),
    ), tail="can_data", doc="VirtualCanFrame : une trame reçue par le nœud étudiant."),
    MessageSpec(0x12, "SIM_TIME", PC_TO_BOARD, (
        F("t_us", "Q", "temps bus courant en µs (non décroissant dans la session)"),
    ), doc="Fait avancer le temps bus sans trame (déclenche les ticks)."),
    MessageSpec(0x7F, "HEARTBEAT", PC_TO_BOARD, (
        F("host_uptime_ms", "I", "horloge monotone du PC, diagnostic seulement"),
    )),
    MessageSpec(0x81, "HELLO_ACK", BOARD_TO_PC, (
        F("reason", "B", "0 = réponse à HELLO, 1 = émis spontanément au boot"),
        F("proto", "B", "version de protocole parlée par la carte (0)"),
        F("rt_major", "B", "version du runtime (semver)"),
        F("rt_minor", "B", ""),
        F("rt_patch", "B", ""),
        F("abi_major", "H", "ABI runtime ↔ code étudiant offerte par le runtime"),
        F("abi_minor", "H", ""),
        F("build_id", "8s", "identifiant de build du runtime (hash de commit tronqué)"),
        F("challenge", "32s", "SHA-256(nonce ‖ image runtime) ; nonce = 16 × 0x00 si reason = 1"),
        F("uid", "12s", "UID du MCU (0x1FF1E800, 3 mots little-endian)"),
        F("dev_id", "H", "DBGMCU_IDCODE.DEV_ID (0x450)"),
        F("rev_id", "H", "DBGMCU_IDCODE.REV_ID"),
        F("reset_flags", "I", "RCC_RSR capturé au boot"),
        F("student_status", "B",
          "0 absent · 1 ok · 2 magic · 3 ABI · 4 bornes · 5 désactivé après faute"),
        F("student_abi_major", "H", "ABI déclarée par l'en-tête étudiant (0 si absent)"),
        F("student_abi_minor", "H", ""),
        F("student_crc32", "I", "CRC-32 de la zone étudiante (0 si absente)"),
        F("rx_window", "B", "fenêtre de crédits W en VCAN_FRAME (≥ 1, 32 recommandé)"),
        F("fault_pending", "B", "1 si un FAULT_REPORT suit immédiatement"),
    ), doc="Identité, compatibilité et défi d'intégrité du runtime."),
    MessageSpec(0x82, "SESSION_ACK", BOARD_TO_PC, (
        F("session_id", "I", "session concernée (≠ 0)"),
        F("op", "B", "1 = réponse à SESSION_START, 2 = réponse à SESSION_STOP"),
        F("status", "B", "0 ok · 1 pas de code étudiant valide · 2 session inconnue"),
    )),
    MessageSpec(0x83, "FAULT_REPORT", BOARD_TO_PC, (
        F("cause", "B", "1 = faute processeur, 2 = watchdog"),
        F("hook", "B", "0 aucun · 1 student_init · 2 student_on_frame · 3 student_on_tick"),
        F("frame_seq", "I", "trame en cours si hook = 2, sinon 0"),
        F("cfsr", "I", "SCB->CFSR"),
        F("hfsr", "I", "SCB->HFSR"),
        F("mmfar", "I", "SCB->MMFAR"),
        F("bfar", "I", "SCB->BFAR"),
        F("pc", "I", "PC empilé"),
        F("lr", "I", "LR empilé"),
        F("xpsr", "I", "xPSR empilé"),
    ), doc="Dernière faute enregistrée (envoyée après HELLO_ACK si fault_pending = 1)."),
    MessageSpec(0x84, "NACK", BOARD_TO_PC, (
        F("ref_seq", "H", "seq de l'en-tête du paquet refusé"),
        F("ref_type", "B", "type du paquet refusé"),
        F("code", "B", "code d'erreur (§11)"),
    )),
    MessageSpec(0x90, "RESULT", BOARD_TO_PC, (
        F("frame_seq", "I", "VCAN_FRAME traitée (≠ 0)"),
        F("status", "B", "0 = livrée au code étudiant · 1 = non livrée (code étudiant désactivé)"),
        F("flags", "B", "0x01 publications perdues (> 8) · 0x02 budget de temps dépassé"),
        F("cycles", "I", "durée du hook en cycles DWT (0 si status = 1)"),
        F("n", "B", "nombre d'entrées (≤ 8 ; 0 si status = 1)"),
    ), tail="entries", doc="Réponse à exactement une VCAN_FRAME acceptée."),
    MessageSpec(0x91, "TICK_RESULT", BOARD_TO_PC, (
        F("t_us", "Q", "frontière de tick exécutée (multiple de tick_period_ms × 1000)"),
        F("status", "B", "comme RESULT.status"),
        F("flags", "B", "comme RESULT.flags"),
        F("cycles", "I", "durée de student_on_tick"),
        F("n", "B", "nombre d'entrées (≤ 8)"),
    ), tail="entries"),
    MessageSpec(0x92, "STUDENT_LOG", BOARD_TO_PC, (
        F("t_us", "Q", "temps bus à l'appel de n7rt_log"),
        F("text_len", "B", "longueur du texte en octets (≤ 120)"),
    ), tail="text", doc="Texte de n7rt_log, UTF-8 valide sans octet nul, 20 lignes/s max."),
    MessageSpec(0xFF, "HEARTBEAT", BOARD_TO_PC, (
        F("uptime_ms", "I", "temps depuis le boot"),
        F("acked_frame_seq", "I", "plus grand frame_seq déjà répondu (RESULT ou NACK), 0 si aucun"),
        F("hook_running_ms", "H", "durée du hook en cours, 0 si aucun (saturé à 65535)"),
        F("state", "B", "0 repos · 1 session exploration · 2 session qualification"),
        F("student_disabled", "B", "1 si le code étudiant est désactivé après une faute"),
        F("rx_frames", "I", "VCAN_FRAME reçues depuis le boot"),
        F("rx_seq_gaps", "H", "trous de seq constatés côté carte"),
        F("rx_framing_errors", "H", "E_OVERFLOW + E_COBS + E_SHORT"),
        F("rx_crc_errors", "H", "E_CRC"),
        F("rx_overruns", "H", "débordements UART/DMA"),
        F("log_dropped", "H", "lignes STUDENT_LOG perdues (limite de débit)"),
        F("queue_high_water", "B", "occupation maximale de la file de VCAN_FRAME"),
        F("max_cycles", "I", "plus longue exécution de hook observée"),
    ), doc="Émis par interruption timer toutes les 250 ms, même pendant un hook."),
)
del F

RESERVED_TYPES: dict[int, str] = {0x11: "VCAN_STATUS (incrément I4)", 0x93: "VCAN_TX (V2)"}
_BY_KEY = {(m.direction, m.type): m for m in MESSAGES}


def spec_for(direction: str, type_: int) -> MessageSpec | None:
    """Message the receiver on the far side of `direction` accepts for this type, if any."""
    return _BY_KEY.get((direction, type_))


# --- Payload encode / decode ---------------------------------------------------------------------
class PayloadError(ValueError):
    pass


def _check(condition: bool, reason: str) -> None:
    if not condition:
        raise PayloadError(reason)


def can_data_length(dlc: int, flags: int) -> int:
    if flags & CAN_RTR:
        return 0
    return FD_LENGTHS[dlc] if flags & CAN_FD else min(dlc, CLASSIC_MAX_LEN)


def _validate_can(f: dict[str, Any]) -> None:
    flags = f["flags"]
    _check(f["frame_seq"] != 0, "frame_seq is 0")
    _check(flags & ~CAN_FLAGS_MASK == 0, "reserved flag bits set")
    _check(f["bus"] == 0, "bus must be 0 in v0")
    ext, fd, rtr = flags & CAN_EXT, flags & CAN_FD, flags & CAN_RTR
    _check(f["can_id"] <= (EXT_ID_MAX if ext else STD_ID_MAX), "can_id out of range")
    _check(f["dlc"] <= 15, "dlc > 15")
    _check(not (fd and rtr), "RTR does not exist in CAN FD")
    _check(bool(fd) or not flags & (CAN_BRS | CAN_ESI), "BRS/ESI require CAN_FD")
    _check(f["len"] == can_data_length(f["dlc"], flags), "len inconsistent with dlc/flags")


def _validate_entries(f: dict[str, Any]) -> None:
    _check(f["status"] in (0, 1), "unknown status")
    _check(f["flags"] & ~(RESULT_FLAG_PUBLISH_OVERFLOW | RESULT_FLAG_OVER_BUDGET) == 0,
           "reserved result flag bits set")
    if f["status"] == 1:
        _check(f["n"] == 0 and f["cycles"] == 0, "not-delivered result carries data")
    for e in f["entries"]:
        _check(e["kind"] in (0, 1), "unknown entry kind")
        if e["kind"] == 1:
            _check(struct.pack("<f", e["value"]) == bytes(4), "reject entry value must be +0.0")


def _validate(spec: MessageSpec, f: dict[str, Any]) -> None:
    t, d = spec.type, spec.direction
    if d == PC_TO_BOARD:
        if t == 0x01:
            _check(f["proto_min"] <= f["proto_max"], "proto_min > proto_max")
        elif t == 0x02:
            _check(f["session_id"] != 0, "session_id is 0")
            _check(f["mode"] in (1, 2), "unknown mode")
            _check(f["tick_period_ms"] <= 1000, "tick_period_ms > 1000")
        elif t == 0x03:
            _check(f["session_id"] != 0, "session_id is 0")
        elif t == 0x10:
            _validate_can(f)
        return
    if t == 0x81:
        _check(f["reason"] in (0, 1), "unknown reason")
        _check(f["student_status"] <= 5, "unknown student_status")
        _check(f["fault_pending"] in (0, 1), "fault_pending not boolean")
        _check(f["rx_window"] >= 1, "rx_window is 0")
    elif t == 0x82:
        _check(f["session_id"] != 0, "session_id is 0")
        _check(f["op"] in (1, 2), "unknown op")
        _check(f["status"] <= 2, "unknown status")
    elif t == 0x83:
        _check(f["cause"] in (1, 2), "unknown cause")
        _check(f["hook"] <= 3, "unknown hook")
        _check((f["hook"] == 2) == (f["frame_seq"] != 0), "frame_seq only with hook on_frame")
    elif t == 0x90:
        _check(f["frame_seq"] != 0, "frame_seq is 0")
        _validate_entries(f)
    elif t == 0x91:
        _validate_entries(f)
    elif t == 0xFF:
        _check(f["state"] <= 2, "unknown state")
        _check(f["student_disabled"] in (0, 1), "student_disabled not boolean")


def decode_payload(spec: MessageSpec, payload: bytes) -> tuple[dict[str, Any], bytes]:
    """Return (fields, extension). Raises PayloadError for every E_PAYLOAD case."""
    _check(len(payload) >= spec.fixed_size, "payload shorter than the fixed part")
    values = spec.layout.unpack_from(payload, 0)
    f: dict[str, Any] = {
        fld.name: (v.hex() if fld.is_bytes else v) for fld, v in zip(spec.fields, values)
    }
    pos = spec.fixed_size
    if spec.tail == "can_data":
        n = f["len"]
        _check(n <= MAX_CAN_DATA, "len > 64")
        _check(len(payload) >= pos + n, "payload shorter than len")
        f["data"] = payload[pos:pos + n].hex()
        pos += n
    elif spec.tail == "entries":
        n = f["n"]
        _check(n <= MAX_ENTRIES, "more than 8 entries")
        _check(len(payload) >= pos + n * ENTRY.size, "payload shorter than n entries")
        f["entries"] = [
            {"slot": s, "kind": k, "value": v}
            for s, k, v in (ENTRY.unpack_from(payload, pos + i * ENTRY.size) for i in range(n))
        ]
        pos += n * ENTRY.size
    elif spec.tail == "text":
        n = f["text_len"]
        _check(n <= MAX_LOG_TEXT, "text_len > 120")
        _check(len(payload) >= pos + n, "payload shorter than text_len")
        raw = payload[pos:pos + n]
        _check(0 not in raw, "NUL byte in text")
        try:
            f["text"] = raw.decode("utf-8")
        except UnicodeDecodeError:
            raise PayloadError("text is not valid UTF-8") from None
        pos += n
    _validate(spec, f)
    return f, payload[pos:]


def encode_payload(spec: MessageSpec, fields: dict[str, Any], extension: bytes = b"",
                   *, validate: bool = True) -> bytes:
    values = [bytes.fromhex(fields[f.name]) if f.is_bytes else fields[f.name] for f in spec.fields]
    out = bytearray(spec.layout.pack(*values))
    if spec.tail == "can_data":
        out += bytes.fromhex(fields["data"])
    elif spec.tail == "entries":
        for e in fields["entries"]:
            out += ENTRY.pack(e["slot"], e["kind"], e["value"])
    elif spec.tail == "text":
        out += fields["text"].encode("utf-8")
    out += extension
    payload = bytes(out)
    if validate:
        decoded, ext = decode_payload(spec, payload)
        _check(decoded == fields and ext == extension, "fields do not round-trip")
    return payload


def build_packet(type_: int, seq: int, payload: bytes, *, proto: int = PROTO_VERSION,
                 length: int | None = None) -> bytes:
    body = HEADER.pack(proto, type_, seq, len(payload) if length is None else length) + payload
    return body + struct.pack("<I", crc32(body))


def encode_wire(packet: bytes) -> bytes:
    return cobs_encode(packet) + bytes([DELIMITER])


def encode_message(direction: str, type_: int, seq: int, fields: dict[str, Any],
                   extension: bytes = b"") -> bytes:
    spec = spec_for(direction, type_)
    if spec is None:
        raise ValueError(f"no {direction} message of type 0x{type_:02X}")
    return encode_wire(build_packet(type_, seq, encode_payload(spec, fields, extension)))


# --- Receiver (spec section 5) -----------------------------------------------------------------
@dataclass(frozen=True)
class Event:
    event: str                       # "packet" | "frame_error" | "reject"
    code: str | None = None          # error code for frame_error / reject
    type: int | None = None
    seq: int | None = None
    fields: dict[str, Any] | None = None
    extension: bytes = b""
    reason: str = field(default="", compare=False)


def parse_frame(encoded: bytes, direction: str) -> Event:
    """Apply the normative check order to one delimiter-free frame."""
    try:
        packet = cobs_decode(encoded)
    except CobsError as exc:
        return Event("frame_error", E_COBS, reason=str(exc))
    if len(packet) < MIN_PACKET:
        return Event("frame_error", E_SHORT, reason=f"{len(packet)} bytes")
    body = packet[:-CRC_SIZE]
    if crc32(body) != struct.unpack_from("<I", packet, len(body))[0]:
        return Event("frame_error", E_CRC)
    proto, type_, seq, length = HEADER.unpack_from(packet)

    def reject(code: str, reason: str) -> Event:
        return Event("reject", code, type_, seq, reason=reason)

    if proto != PROTO_VERSION:
        return reject("E_VERSION", f"proto {proto}")
    if length > MAX_PAYLOAD or length != len(packet) - MIN_PACKET:
        return reject("E_LENGTH", f"header len {length}, actual {len(packet) - MIN_PACKET}")
    spec = spec_for(direction, type_)
    if spec is None:
        return reject("E_UNKNOWN_TYPE", f"type 0x{type_:02X} not accepted {direction}")
    try:
        fields, extension = decode_payload(spec, body[HEADER_SIZE:])
    except PayloadError as exc:
        return reject("E_PAYLOAD", str(exc))
    return Event("packet", None, type_, seq, fields, extension)


class Receiver:
    """Stream receiver: bytes in, events out. One instance per direction and per link."""

    def __init__(self, direction: str) -> None:
        if direction not in DIRECTIONS:
            raise ValueError(direction)
        self.direction = direction
        self._buf = bytearray()
        self._overflow = False

    def feed(self, data: bytes) -> list[Event]:
        events: list[Event] = []
        for b in data:
            if b == DELIMITER:
                if self._overflow:
                    events.append(Event("frame_error", E_OVERFLOW))
                elif self._buf:
                    events.append(parse_frame(bytes(self._buf), self.direction))
                self._buf.clear()
                self._overflow = False
            elif not self._overflow:
                if len(self._buf) == MAX_ENCODED:
                    self._overflow = True
                    self._buf.clear()
                else:
                    self._buf.append(b)
        return events
