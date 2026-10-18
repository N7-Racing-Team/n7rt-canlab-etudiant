"""Builds one PDF of the 0.3 documentation and checks it: the figures (tools/docs/pdf_figures.py, when a captures
folder is given; otherwise the committed figures and their manifest), the badges cited by the text
(tools/docs/check_figrefs.py), the Typst compilation, then the PDF itself (tools/docs/check_pdf.py): outline,
clickable table of contents, page numbers, nothing past the page edge, every figure placed. Exit 1 on any failure.

usage: python tools/docs/build_pdf.py <doc.json> [--captures "<folder>;<folder>"]

<doc.json> (paths relative to its folder unless absolute):
    {"source": "tutoriel-etudiant.typ", "figures": "figures.json", "images": "../../img/tutoriel-0.3-pdf",
     "manifest": "figures-manifest.json", "root": "../..", "output": "../../N7RT_CANLab_0.3_Tutoriel_Etudiant.pdf",
     "legend": "steps" | "all", "min_figures": 30, "mission": null, "data": null, "exe_sha256": "…",
     "captures": null, "report": "build-report.json", "render": null}

`captures` (a campaign folder, private) recomposes the figures; without it the committed images are used as they are.
`root` is the Typst root: every path the document reads (template, images, data, sources) must be inside it.
"""
from __future__ import annotations

import json
import os
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import check_figrefs  # noqa: E402
import check_pdf  # noqa: E402
import pdf_figures  # noqa: E402


def resolve(base: Path, p: str | None) -> Path | None:
    if p is None:
        return None
    q = Path(p)
    return q if q.is_absolute() else (base / q).resolve()


def rel_to_root(root: Path, p: Path) -> str:
    return "/" + p.resolve().relative_to(root.resolve()).as_posix()


def main(argv=None) -> int:
    args = argv if argv is not None else sys.argv[1:]
    override = None
    if len(args) == 3 and args[1] == "--captures":
        # the raw captures (private campaign folders, several separated by ";"): never written into the config
        override = [str(Path(p).resolve()) for p in args[2].split(";") if p]
        args = args[:1]
    if len(args) != 1:
        sys.exit(__doc__)
    cfg_p = Path(args[0]).resolve()
    cfg = json.loads(cfg_p.read_text(encoding="utf-8"))
    if override:
        cfg["captures"] = override
    base = cfg_p.parent
    root = resolve(base, cfg["root"])
    source, manifest = resolve(base, cfg["source"]), resolve(base, cfg["manifest"])
    if isinstance(cfg["figures"], list):
        # one spec per part of the document, merged (ids must stay unique: the composer refuses duplicates)
        specs = [json.loads(resolve(base, f).read_text(encoding="utf-8")) for f in cfg["figures"] if resolve(base, f).exists()]
        merged = {"document": source.stem, "mission": specs[0].get("mission") if specs else cfg.get("mission"),
                  "figures": [fig for s in specs for fig in s.get("figures", [])]}
        figures = manifest.with_name(manifest.stem + "-spec.json")
        figures.parent.mkdir(parents=True, exist_ok=True)
        figures.write_bytes((json.dumps(merged, ensure_ascii=False, indent=1) + "\n").encode("utf-8"))
    else:
        figures = resolve(base, cfg["figures"])
    images, output = resolve(base, cfg["images"]), resolve(base, cfg["output"])
    report: dict = {"document": source.name, "output": output.name}
    ok = True
    # 1. figures
    raw = cfg.get("captures")
    captures = [resolve(base, c) for c in raw] if isinstance(raw, list) else resolve(base, raw)
    if captures:
        opts = type("O", (), {"root": str(root), "exe_sha256": cfg.get("exe_sha256"), "min_figures": cfg.get("min_figures", 0)})()
        try:
            pdf_figures.build(figures, captures, images, manifest, opts)
            print(f"FIGURES OK")
        except pdf_figures.FigureError as e:
            print(f"FIGURES FAIL\n{e}")
            return 1
    elif not manifest.exists():
        print(f"FIGURES FAIL : manifeste {manifest} absent (donner « captures » pour composer les figures)")
        return 1
    m = json.loads(manifest.read_text(encoding="utf-8"))
    if cfg.get("exe_sha256"):
        stale = [k for k, v in m.items() if v.get("exe_sha256") != cfg["exe_sha256"]]
        if stale:
            print(f"FIGURES FAIL : figures d'un autre exécutable : {stale[:8]}")
            return 1
    if cfg.get("mission"):
        other = [k for k, v in m.items() if v.get("mission") not in (cfg["mission"],)]
        if other:
            print(f"FIGURES FAIL : figures d'une autre mission : {other[:8]}")
            return 1
    missing = [k for k, v in m.items() if not (root / v["path"].lstrip("/")).exists()]
    if missing:
        print(f"FIGURES FAIL : images absentes : {missing[:8]}")
        return 1
    if len(m) < cfg.get("min_figures", 0):
        print(f"FIGURES FAIL : {len(m)} figures, minimum {cfg['min_figures']}")
        return 1
    # 2. badges cited by the text
    fr_args = [str(source), str(figures), "--legend", cfg.get("legend", "steps"), "--min", str(cfg.get("min_figures", 0))]
    for extra in cfg.get("parts", []):
        fr_args += ["--part", str(resolve(base, extra))]
    if check_figrefs.main(fr_args) != 0:
        ok = False
    # 3. Typst
    cmd = ["typst", "compile", "--root", str(root), "--input", f"manifest={rel_to_root(root, manifest)}"]
    data = resolve(base, cfg.get("data"))
    if data:
        cmd += ["--input", f"data={rel_to_root(root, data)}"]
    # on a machine short of memory, N7RT_TYPST_JOBS=1 (or 2) caps Typst's parallel jobs: a 230-page instructor guide
    # with 150 captures needs about 2.5 GB of commit with the default (one job per CPU)
    if os.environ.get("N7RT_TYPST_JOBS", "").isdigit():
        cmd += ["--jobs", os.environ["N7RT_TYPST_JOBS"]]
    cmd += [str(source), str(output)]
    p = subprocess.run(cmd, capture_output=True, text=True, encoding="utf-8", errors="replace")
    if p.returncode != 0:
        print("TYPST FAIL\n" + (p.stderr or p.stdout)[-4000:])
        return 1
    warnings = [l for l in (p.stderr or "").splitlines() if l.startswith("warning")]
    report["typst_warnings"] = warnings
    print(f"TYPST OK {output.name}" + (f" ({len(warnings)} avertissements)" if warnings else ""))
    # 4. the PDF
    r = check_pdf.check(output, manifest, source, resolve(base, cfg.get("render")), parts=[resolve(base, x) for x in cfg.get("parts", [])])
    report.update(r)
    if r["problems"]:
        ok = False
        for pr in r["problems"]:
            print("  PROBLÈME", pr)
    rep = resolve(base, cfg.get("report", "build-report.json"))
    rep.write_bytes((json.dumps(report, ensure_ascii=False, indent=1) + "\n").encode("utf-8"))
    print(("BUILD OK " if ok else "BUILD FAIL ") + json.dumps({k: r.get(k) for k in ("pages", "sections", "figures", "annotated_captures", "code_blocks", "toc_links")}, ensure_ascii=False))
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
