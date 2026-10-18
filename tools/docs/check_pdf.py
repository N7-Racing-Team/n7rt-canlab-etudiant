"""Checks a generated PDF of the 0.3 documentation and reports its counts.

usage: python tools/docs/check_pdf.py <file.pdf> [--manifest figures.json] [--source doc.typ] [--render DIR] [--json OUT]

Counts: pages, sections (bookmarks of level 1 and 2), clickable links of the table of contents, figures (images
placed on the pages), annotated captures (figures of the manifest with marks), code blocks (in the source). Fails when:
the document has no outline or its table of contents has no link, a page has no number in its footer, a text span or
an image goes past the page edge (cut content), a font is not embedded, or a figure of the manifest is not placed.
"""
from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

import fitz  # PyMuPDF

EDGE = 4.0  # points: tolerance of the page edge


def check(pdf: Path, manifest: Path | None, source: Path | None, render: Path | None, parts=()) -> dict:
    doc = fitz.open(pdf)
    problems: list[str] = []
    toc = doc.get_toc()
    sections = [t for t in toc if t[0] <= 2]
    if not toc:
        problems.append("pas de signets (sommaire PDF)")
    toc_links = 0
    for pno in range(min(4, doc.page_count)):
        toc_links += sum(1 for l in doc[pno].get_links() if l.get("kind") == fitz.LINK_GOTO)
    if toc_links == 0:
        problems.append("sommaire sans lien cliquable")
    images = 0
    unnumbered = []
    for pno, page in enumerate(doc):
        W, H = page.rect.width, page.rect.height
        foot = page.get_text("text", clip=fitz.Rect(0, H - 45, W, H))
        if not re.search(rf"Page\s+{pno + 1}\s*/\s*{doc.page_count}", foot):
            unnumbered.append(pno + 1)
        d = page.get_text("dict")
        for block in d["blocks"]:
            if block.get("type") == 0:
                for line in block["lines"]:
                    for span in line["spans"]:
                        x0, y0, x1, y1 = span["bbox"]
                        if span["text"].strip() and (x0 < -EDGE or y0 < -EDGE or x1 > W + EDGE or y1 > H + EDGE):
                            problems.append(f"page {pno + 1} : texte hors page « {span['text'][:40]} »")
        for info in page.get_image_info():
            images += 1
            x0, y0, x1, y1 = info["bbox"]
            if x0 < -EDGE or y0 < -EDGE or x1 > W + EDGE or y1 > H + EDGE:
                problems.append(f"page {pno + 1} : image hors page")
        if render:
            render.mkdir(parents=True, exist_ok=True)
            page.get_pixmap(dpi=110).save(render / f"page-{pno + 1:03d}.png")
    if unnumbered:
        problems.append(f"pages sans numéro « Page n / N » : {unnumbered[:10]}")
    fonts = {}
    for pno in range(doc.page_count):
        for f in doc.get_page_fonts(pno):
            fonts[f[3]] = f[1]
    not_embedded = [name for name, ext in fonts.items() if ext in ("", "n/a")]
    if not_embedded:
        problems.append(f"polices non incorporées : {not_embedded}")
    report = {"pdf": pdf.name, "pages": doc.page_count, "sections": len(sections), "toc_entries": len(toc),
              "toc_links": toc_links, "images": images}
    if manifest:
        m = json.loads(manifest.read_text(encoding="utf-8"))
        report["figures"] = len(m)
        report["annotated_captures"] = sum(1 for v in m.values() if v.get("marks"))
        report["marks"] = sum(len(v.get("marks", [])) for v in m.values())
        if images < len(m):
            problems.append(f"{len(m)} figures au manifeste, {images} images placées")
    if source:
        text = "\n".join(p.read_text(encoding="utf-8") for p in [source, *[q for q in parts if q]])
        report["code_blocks"] = len(re.findall(r"#(?:code|src)\(", text)) + len(re.findall(r"^```", text, re.M)) // 2
    report["problems"] = problems
    return report


def main(argv=None) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("pdf", type=Path)
    ap.add_argument("--manifest", type=Path)
    ap.add_argument("--source", type=Path)
    ap.add_argument("--render", type=Path)
    ap.add_argument("--json", type=Path)
    a = ap.parse_args(argv)
    r = check(a.pdf, a.manifest, a.source, a.render)
    if a.json:
        a.json.write_text(json.dumps(r, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    print(json.dumps({k: v for k, v in r.items() if k != "problems"}, ensure_ascii=False))
    for p in r["problems"]:
        print("  PROBLÈME", p)
    return 1 if r["problems"] else 0


if __name__ == "__main__":
    sys.exit(main())
