"""Checks that the interface labels quoted in a document exist in the final application's sources.

usage: python tools/docs/check_labels.py <doc.typ> [more.typ ...] [--allow allow.txt]

Every text between French quotes « … » in the Typst sources (outside comments and code) is looked for, verbatim, in
the UI (app/ui/index.html, app/ui/src/**/*.ts) and in the core's French messages (app/crates/**/*.rs): button and field
labels, step names, verdict lines, messages. A label found nowhere is reported (a quotation that is not a label —
a document title, a hypothetical example — goes in the allow list, one per line). Labels built at runtime from parts
(« Slot 0 — … », « E0 · Essai nominal ») are matched on their longest literal fragment. Exit 1 on an unknown label.
"""
from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
QUOTE = re.compile(r"«\s*([^«»]{2,160}?)\s*»")


def corpus() -> str:
    parts = [(ROOT / "app/ui/index.html").read_text(encoding="utf-8")]
    for p in sorted((ROOT / "app/ui/src").rglob("*.ts")):
        if not p.name.endswith(".test.ts"):
            parts.append(p.read_text(encoding="utf-8"))
    for p in sorted((ROOT / "app/crates").rglob("*.rs")) + sorted((ROOT / "app/src-tauri/src").rglob("*.rs")):
        parts.append(p.read_text(encoding="utf-8"))
    # messages of the integrated IDE's build helper (shown in its console)
    for p in sorted((ROOT / "tools/stm32").glob("*.py")):
        parts.append(p.read_text(encoding="utf-8"))
    text = "\n".join(parts)
    # the sources escape non-ASCII letters (Rust « \u{e9} », TypeScript « \u00e9 ») and apostrophes, and use
    # typographic spaces in places: one normal form
    text = re.sub(r"\\u\{([0-9a-fA-F]{1,6})\}", lambda m: chr(int(m.group(1), 16)), text)
    text = re.sub(r"\\u([0-9a-fA-F]{4})", lambda m: chr(int(m.group(1), 16)), text)
    return normal(text.replace("\\'", "'").replace('\\"', '"'))


def normal(s: str) -> str:
    return re.sub(r"\s+", " ", s.replace(" ", " ").replace(" ", " ").replace("’", "'")).strip()


def fragments(label: str) -> list[str]:
    """The literal pieces of a label that may contain runtime values (numbers, ids, dates, placeholders) or list
    alternatives (« conforme / non conforme »): every piece must exist in the sources."""
    pieces = re.split(r"<[^>]+>|\d+(?:[.,]\d+)?|0x[0-9A-Fa-f]+|…|\.\.\.| : | / | — | · ", label)
    return sorted({normal(p) for p in pieces if len(normal(p)) >= 4}, key=len, reverse=True)


def known(label: str, ui: str) -> bool:
    if label in ui:
        return True
    parts = fragments(label)
    return bool(parts) and all(p in ui for p in parts)


def main(argv=None) -> int:
    a = argv if argv is not None else sys.argv[1:]
    allow: set[str] = set()
    if "--allow" in a:
        i = a.index("--allow")
        allow = {normal(x) for x in Path(a[i + 1]).read_text(encoding="utf-8").splitlines() if x.strip()}
        a = a[:i] + a[i + 2:]
    ui = corpus()
    unknown, found = [], 0
    for src in a:
        text = re.sub(r"//[^\n]*", "", Path(src).read_text(encoding="utf-8"))
        text = re.sub(r"```.*?```", "", text, flags=re.S)
        for ln, line in enumerate(text.splitlines(), 1):
            for m in QUOTE.finditer(line):
                label = normal(re.sub(r"#\w+\([^)]*\)", "", m.group(1)))
                if not label or label in allow:
                    continue
                if known(label, ui):
                    found += 1
                else:
                    unknown.append(f"{Path(src).name}:{ln}: « {label} »")
    for u in unknown:
        print("  INCONNU", u)
    print(f"{found} libellés trouvés dans l'interface, {len(unknown)} introuvables")
    return 1 if unknown else 0


if __name__ == "__main__":
    sys.exit(main())
