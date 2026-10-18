"""Public N7RT-Link layouts and throughput budget; no fixed input/result fixtures.

    python tools/n7rt_link/protocol_docs.py layouts
    python tools/n7rt_link/protocol_docs.py budget
    python tools/n7rt_link/protocol_docs.py check-doc
    python tools/n7rt_link/protocol_docs.py sync-doc
"""
from __future__ import annotations
import argparse
import re
import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent))
import n7rt_link as L

ROOT = Path(__file__).resolve().parents[2]
SPEC_PATH = ROOT / "docs" / "n7rt-link-v0.md"
P2B, B2P = L.PC_TO_BOARD, L.BOARD_TO_PC
# V1 load hypotheses from docs/can-lab-architecture.md section 6 (mission cap, worst cases).
V1_LOAD = {
    "bus_frames_per_s": 600,
    "can_data_bytes": 8,
    "publications_per_result": 8,
    "tick_period_ms": 10,
    "log_lines_per_s": 20,
    "heartbeat_hz": 4,
}
MAX_UTILISATION = 0.70   # each direction must keep at least 30 % of the line free

def _spec(direction: str, type_: int) -> L.MessageSpec:
    spec = L.spec_for(direction, type_)
    assert spec is not None, (direction, type_)
    return spec


# --- Throughput budget (spec section 12) -------------------------------------------------------
def budget() -> dict[str, Any]:
    lo = V1_LOAD
    cap = L.BAUD_V0 / L.UART_BITS_PER_BYTE
    ticks = 1000 / lo["tick_period_ms"]
    pubs = lo["publications_per_result"] * L.ENTRY.size
    size = lambda d, t, extra=0: L.wire_bound(_spec(d, t).fixed_size + extra)  # noqa: E731
    rows = {
        P2B: [
            ("VCAN_FRAME (CAN classique, 8 o)", lo["bus_frames_per_s"],
             size(P2B, 0x10, lo["can_data_bytes"]), True),
            ("SIM_TIME (tick 10 ms)", ticks, size(P2B, 0x12), False),
            ("HEARTBEAT", lo["heartbeat_hz"], size(P2B, 0x7F), False),
        ],
        B2P: [
            ("RESULT (8 publications)", lo["bus_frames_per_s"], size(B2P, 0x90, pubs), True),
            ("TICK_RESULT (8 publications)", ticks, size(B2P, 0x91, pubs), False),
            ("STUDENT_LOG (120 o)", lo["log_lines_per_s"], size(B2P, 0x92, L.MAX_LOG_TEXT), False),
            ("HEARTBEAT", lo["heartbeat_hz"], size(B2P, 0xFF), False),
        ],
    }
    out: dict[str, Any] = {"capacity_bytes_per_s": cap, "directions": {}}
    for d, rs in rows.items():
        load = sum(rate * wire for _, rate, wire, _ in rs)
        fixed = sum(rate * wire for _, rate, wire, per_frame in rs if not per_frame)
        per_frame = sum(wire for _, _, wire, pf in rs if pf)
        out["directions"][d] = {
            "rows": [(label, rate, wire, rate * wire) for label, rate, wire, _ in rs],
            "bytes_per_s": load,
            "utilisation": load / cap,
            "max_frames_per_s": int((cap - fixed) // per_frame),
        }
    out["max_frames_per_s"] = min(v["max_frames_per_s"] for v in out["directions"].values())
    return out


# --- Generated spec blocks -----------------------------------------------------------------
_TAIL_ROWS = {
    "can_data": ("data", "u8[len]", "octets de données de la trame, dans l'ordre du bus"),
    "entries": ("entries", "n × {slot u8, kind u8, value f32}",
                "slot de la mission ; kind 0 = valeur publiée, 1 = rejet (value = +0.0)"),
    "text": ("text", "u8[text_len]", "UTF-8 valide, sans octet nul"),
}
_TAIL_MAX = {"can_data": L.MAX_CAN_DATA, "entries": L.MAX_ENTRIES * L.ENTRY.size,
             "text": L.MAX_LOG_TEXT}
_ARROW = {P2B: "PC → carte", B2P: "carte → PC"}


def layouts_md() -> str:
    parts = []
    for m in MESSAGES_ORDERED():
        lines = [f"#### `0x{m.type:02X}` {m.name} — {_ARROW[m.direction]}", ""]
        if m.doc:
            lines += [m.doc, ""]
        lines += ["| Offset | Champ | Type | Description |", "|---:|---|---|---|"]
        off = 0
        for f in m.fields:
            lines.append(f"| {off} | `{f.name}` | {f.type_name} | {f.doc} |")
            off += f.size
        if m.tail:
            name, typ, doc = _TAIL_ROWS[m.tail]
            lines.append(f"| {off} | `{name}` | {typ} | {doc} |")
            size = f"{m.fixed_size} à {m.fixed_size + _TAIL_MAX[m.tail]} octets"
        else:
            size = f"{m.fixed_size} octets"
        lines += ["", f"Charge utile définie : {size}.", ""]
        parts.append("\n".join(lines))
    return "\n".join(parts).rstrip() + "\n"


def MESSAGES_ORDERED() -> list[L.MessageSpec]:  # noqa: N802
    return sorted(L.MESSAGES, key=lambda m: (m.direction != P2B, m.type))


def budget_md() -> str:
    b = budget()
    cap = b["capacity_bytes_per_s"]
    lines = [f"Capacité de la ligne : {L.BAUD_V0} Bd × 8N1 = {cap:.0f} octets/s par sens.", ""]
    for d in (P2B, B2P):
        v = b["directions"][d]
        lines += [f"**{_ARROW[d]}**", "",
                  "| Message | Débit (/s) | Octets sur le fil (max) | Octets/s |",
                  "|---|---:|---:|---:|"]
        for label, rate, wire, total in v["rows"]:
            lines.append(f"| {label} | {rate:g} | {wire} | {total:g} |")
        util = v["utilisation"]
        lines += [f"| **Total** | | | **{v['bytes_per_s']:g}** |", "",
                  f"Occupation : **{util:.1%}** · marge : **{1 - util:.1%}** · trames/s maximales "
                  f"dans ce sens avec la même charge annexe : **{v['max_frames_per_s']}**.", ""]
    lines.append(f"Débit de trames maximal tenu dans les deux sens : **{b['max_frames_per_s']} "
                 f"trames/s**, soit {b['max_frames_per_s'] / V1_LOAD['bus_frames_per_s']:.2f} × "
                 f"le plafond V1 de {V1_LOAD['bus_frames_per_s']} trames/s.")
    return "\n".join(lines) + "\n"


_BLOCK = re.compile(r"(<!-- BEGIN GENERATED: (\w+) -->\n)(.*?)(<!-- END GENERATED: \2 -->)", re.S)
GENERATORS = {"layouts": layouts_md, "budget": budget_md}


def sync_doc(text: str) -> str:
    return _BLOCK.sub(lambda m: m.group(1) + GENERATORS[m.group(2)]() + m.group(4), text)


def doc_blocks(text: str) -> dict[str, str]:
    return {m.group(2): m.group(3) for m in _BLOCK.finditer(text)}


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("command", choices=("layouts", "budget", "check-doc", "sync-doc"))
    args = ap.parse_args(argv)
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8")
    if args.command == "layouts":
        print(layouts_md(), end="")
    elif args.command == "budget":
        print(budget_md(), end="")
    else:
        text = SPEC_PATH.read_text(encoding="utf-8")
        blocks = doc_blocks(text)
        if set(blocks) != set(GENERATORS):
            print("FAIL: missing or unknown generated documentation block", file=sys.stderr)
            return 1
        updated = sync_doc(text)
        if args.command == "sync-doc":
            SPEC_PATH.write_text(updated, encoding="utf-8", newline="\n")
            print("synced", SPEC_PATH.relative_to(ROOT))
        elif updated != text:
            print("FAIL: generated documentation is stale", file=sys.stderr)
            return 1
        else:
            print("documentation layouts and budget: all passed")
    return 0

if __name__ == "__main__":
    sys.exit(main())
