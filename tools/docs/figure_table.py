"""Control table of the figures of a built PDF: one row per figure with the page where it landed, its source
capture, its marks (target, shape, arrow, number) and the result of the automatic checks; the visual review can mark
a figure FAIL with --fail id[,id...] (reason in --note).

usage: python tools/docs/figure_table.py <doc.json> <out.md> [--fail id,id] [--note "id: reason"]...

The page of each figure comes from the Typst metadata the template emits next to it (`<figmeta>`), evaluated on the
same sources and inputs as the build; marks and shapes from the figure spec(s) and the manifest of the build.
"""
from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from build_pdf import rel_to_root, resolve  # noqa: E402


def main(argv=None) -> int:
    a = argv if argv is not None else sys.argv[1:]
    if len(a) < 2:
        sys.exit(__doc__)
    cfg_p, out = Path(a[0]).resolve(), Path(a[1])
    fails, notes = set(), {}
    i = 2
    while i < len(a):
        if a[i] == "--fail":
            fails |= set(a[i + 1].split(","))
        elif a[i] == "--note":
            k, _, v = a[i + 1].partition(":")
            notes[k.strip()] = v.strip()
        i += 2
    cfg = json.loads(cfg_p.read_text(encoding="utf-8"))
    base = cfg_p.parent
    root = resolve(base, cfg["root"])
    source, manifest = resolve(base, cfg["source"]), resolve(base, cfg["manifest"])
    specs = cfg["figures"] if isinstance(cfg["figures"], list) else [cfg["figures"]]
    marks_of = {}
    for s in specs:
        p = resolve(base, s)
        if p.exists():
            for f in json.loads(p.read_text(encoding="utf-8")).get("figures", []):
                marks_of[f["id"]] = f
    man = json.loads(manifest.read_text(encoding="utf-8"))
    cmd = ["typst", "eval", "query(<figmeta>).map(it => it.value)", "--in", str(source), "--root", str(root),
           "--input", f"manifest={rel_to_root(root, manifest)}", "--format", "json"]
    data = resolve(base, cfg.get("data"))
    if data:
        cmd += ["--input", f"data={rel_to_root(root, data)}"]
    p = subprocess.run(cmd, capture_output=True, text=True, encoding="utf-8", errors="replace")
    if p.returncode != 0:
        print(p.stderr[-2000:])
        return 1
    placed = json.loads(p.stdout)
    pdf = resolve(base, cfg["output"]).name
    rows = ["| PDF | page | figure | capture source | cible(s) | cercle / forme | flèche | numéros | texte associé | contrôle |",
            "|---|---|---|---|---|---|---|---|---|---|"]
    bad = 0
    for f in placed:
        fid = f["id"]
        spec = marks_of.get(fid, {})
        ms = spec.get("marks", [])
        targets = "; ".join(str(m.get("target")) for m in ms)
        shapes = "; ".join(m.get("shape", "auto") for m in ms)
        arrows = "; ".join("oui" if m.get("arrow") else "non" for m in ms)
        nums = ", ".join(str(m["n"]) for m in ms)
        ok = fid in man and fid not in fails and bool(ms)
        bad += not ok
        verdict = "PASS" if ok else "FAIL" + (f" ({notes[fid]})" if fid in notes else "")
        rows.append(f"| {pdf} | {f['page']} | {fid} | {f.get('capture') or man.get(fid, {}).get('capture')} | {targets} | {shapes} | {arrows} | "
                    f"{nums} | pastilles {nums} citées (contrôle figures/texte) | {verdict} |")
    out.write_bytes(("\n".join(rows) + "\n").encode("utf-8"))
    print(f"{len(placed)} figures, {bad} FAIL -> {out}")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
