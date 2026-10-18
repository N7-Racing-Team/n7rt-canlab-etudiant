"""Cross-checks the figures of a Typst document against their spec: every badge drawn on a capture is explained in
the text of that figure and the text cites no badge the capture does not carry.

usage: python tools/docs/check_figrefs.py <doc.typ> <figures.json> [--legend all|steps] [--min N] [--part other.typ ...]

In the source, a figure is `#fig("id", caption: [...], regarder: [...], ...)`; its badges are cited as `#b(n)`
anywhere inside the call. With --legend all (instructor guides) the eight legend fields are required on every figure:
regarder, cliquer, entrer, attendu, pourquoi, observer, erreur, echec. Exit 1 on any mismatch.
"""
from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

LEGEND = ("regarder", "cliquer", "entrer", "attendu", "pourquoi", "observer", "erreur", "echec")


def calls(text: str, name: str = "#fig("):
    """Yields (id, argument text, line) of every call, matching brackets and skipping strings and comments."""
    i = 0
    while True:
        i = text.find(name, i)
        if i < 0:
            return
        line = text.count("\n", 0, i) + 1
        j = i + len(name)
        depth, k = 1, j
        in_str = False
        while k < len(text) and depth:
            c = text[k]
            if in_str:
                if c == "\\":
                    k += 1
                elif c == '"':
                    in_str = False
            elif c == '"':
                in_str = True
            elif c in "([{":
                depth += 1
            elif c in ")]}":
                depth -= 1
            elif c == "/" and text.startswith("//", k):
                k = text.find("\n", k)
                if k < 0:
                    break
                continue
            k += 1
        args = text[j:k - 1]
        m = re.match(r'\s*"([^"]+)"', args)
        yield (m.group(1) if m else None), args, line
        i = k


def main(argv=None) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("typ", type=Path)
    ap.add_argument("figures", type=Path)
    ap.add_argument("--legend", choices=("all", "steps"), default="steps")
    ap.add_argument("--min", type=int, default=0)
    ap.add_argument("--part", type=Path, action="append", default=[])
    a = ap.parse_args(argv)
    spec = json.loads(a.figures.read_text(encoding="utf-8"))
    marks = {f["id"]: sorted(int(m["n"]) for m in f.get("marks", [])) for f in spec["figures"]}
    text = "\n".join(p.read_text(encoding="utf-8") for p in [a.typ, *a.part])
    problems, used = [], set()
    for fid, args, line in calls(text):
        if fid is None:
            problems.append(f"ligne {line} : #fig sans identifiant littéral")
            continue
        if fid in used:
            problems.append(f"ligne {line} : figure {fid} utilisée deux fois")
        used.add(fid)
        if fid not in marks:
            problems.append(f"ligne {line} : figure {fid} absente de la spec")
            continue
        cited = sorted({int(n) for n in re.findall(r"#b\((\d+)\)", args)})
        if cited != marks[fid]:
            problems.append(f"ligne {line} : figure {fid} : pastilles citées {cited}, dessinées {marks[fid]}")
        if a.legend == "all":
            missing = [k for k in LEGEND if not re.search(rf"\b{k}\s*:\s*\[", args)]
            if missing:
                problems.append(f"ligne {line} : figure {fid} : rubriques absentes {missing}")
    for fid in marks:
        if fid not in used:
            problems.append(f"figure {fid} de la spec jamais placée dans le texte")
    if a.min and len(used) < a.min:
        problems.append(f"{len(used)} figures placées, minimum {a.min}")
    for p in problems:
        print("  PROBLÈME", p)
    print(f"{'FIGREFS FAIL' if problems else 'FIGREFS OK'} {len(used)} figures, {sum(len(v) for k, v in marks.items() if k in used)} pastilles")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
