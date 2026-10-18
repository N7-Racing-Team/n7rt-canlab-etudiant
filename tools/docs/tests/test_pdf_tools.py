"""Tests of the PDF documentation tools: the figure composer refuses every annotation that could mislead a reader,
the cross-check ties the badges of the text to the marks of the figures, the PDF check finds a missing outline."""
from __future__ import annotations

import hashlib
import json
import shutil
import subprocess
import sys
from pathlib import Path

import pytest
from PIL import Image

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))
import check_figrefs  # noqa: E402
import check_pdf  # noqa: E402
import pdf_figures  # noqa: E402

EXE = "a" * 64


def capture(dir_: Path, cid: str, targets: dict, mission: str = "demo", exe: str = EXE, size=(400, 300), scale=2.5) -> None:
    img = Image.new("RGB", (int(size[0] * scale), int(size[1] * scale)), (30, 34, 42))
    p = dir_ / f"{cid}.png"
    img.save(p)
    meta = {"id": cid, "mission": mission, "run": "test", "exe_sha256": exe, "scale": scale, "viewport": list(size), "png": p.name,
            "png_sha256": hashlib.sha256(p.read_bytes()).hexdigest(), "targets": targets}
    (dir_ / f"{cid}.json").write_text(json.dumps(meta), encoding="utf-8")


def opts(**kw):
    o = type("O", (), {"root": None, "exe_sha256": EXE, "min_figures": 0})()
    for k, v in kw.items():
        setattr(o, k, v)
    return o


def spec(dir_: Path, figures: list, mission: str | None = "demo") -> Path:
    p = dir_ / "figures.json"
    p.write_text(json.dumps({"document": "t", "mission": mission, "figures": figures}), encoding="utf-8")
    return p


@pytest.fixture()
def caps(tmp_path):
    d = tmp_path / "caps"
    d.mkdir()
    capture(d, "c1", {"#ok": {"rect": [100, 100, 60, 24], "visible": True}, "#hidden": {"rect": [10, 10, 20, 20], "visible": False},
                      "#sq": {"rect": [250, 150, 20, 20], "visible": True}, "#big": {"rect": [20, 20, 360, 260], "visible": True},
                      "wire:a~b": {"rect": [40, 200, 100, 40], "visible": True, "points": [[40, 200], [140, 200], [140, 240]]},
                      "cm:10-20": {"rect": [20, 40, 300, 110], "visible": True}})
    return d


def build(tmp_path, caps, figures, mission="demo", **kw):
    return pdf_figures.build(spec(tmp_path, figures, mission), caps, tmp_path / "out", tmp_path / "manifest.json", opts(**kw))


def test_marks_are_drawn_and_listed(tmp_path, caps):
    m = build(tmp_path, caps, [{"id": "f", "capture": "c1", "marks": [
        {"n": 1, "target": "#ok"}, {"n": 2, "target": "#sq", "arrow": True}, {"n": 3, "target": "wire:a~b", "shape": "trace"},
        {"n": 4, "target": "cm:12-13", "shape": "box"}]}])
    assert m["f"]["marks"] == [1, 2, 3, 4]
    assert (tmp_path / "out" / "f.png").exists()


def red_in(img: Image.Image, css_box, scale=2.5) -> int:
    x1, y1, x2, y2 = (int(v * scale) for v in css_box)
    px = img.convert("RGB").crop((x1, y1, x2, y2)).tobytes()
    return sum(1 for i in range(0, len(px), 3) if px[i] > 150 and px[i + 1] < 90 and px[i + 2] < 90)


def test_badge_position_asked_by_a_pill_or_box_mark_is_honoured(tmp_path, caps):
    # pill around #ok (100,100 60x24): its outline is 5 px wider; a badge has a radius of 10.5 px
    ne, west = (163, 77, 183, 97), (72, 102, 93, 122)
    build(tmp_path, caps, [{"id": "f", "capture": "c1", "crop": "none", "marks": [{"n": 1, "target": "#ok", "shape": "pill"}]}])
    img = Image.open(tmp_path / "out" / "f.png")
    assert red_in(img, west) > 1000 and red_in(img, ne) < 300
    build(tmp_path, caps, [{"id": "f", "capture": "c1", "crop": "none",
                            "marks": [{"n": 1, "target": "#ok", "shape": "pill", "badge": "ne"}]}])
    img = Image.open(tmp_path / "out" / "f.png")
    assert red_in(img, ne) > 1000 and red_in(img, west) < 300


@pytest.mark.parametrize("figure, message", [
    ({"id": "f", "capture": "absent", "marks": [{"n": 1, "target": "#ok"}]}, "absente"),
    ({"id": "f", "capture": "c1", "marks": [{"n": 1, "target": "#nope"}]}, "absente de la capture"),
    ({"id": "f", "capture": "c1", "marks": [{"n": 1, "target": "#hidden"}]}, "non visible"),
    ({"id": "f", "capture": "c1", "marks": [{"n": 1, "target": "#ok"}, {"n": 3, "target": "#sq"}]}, "attendu 1..2"),
    ({"id": "f", "capture": "c1", "marks": []}, "sans marque"),
    ({"id": "f", "capture": "c1", "marks": [{"n": 1, "target": "#big", "shape": "circle"}]}, "trop grand"),
    ({"id": "f", "capture": "c1", "marks": [{"n": 1, "target": "#sq"}], "crop": [0, 0, 120, 120]}, "sort du cadre"),
])
def test_misleading_figures_are_refused(tmp_path, caps, figure, message):
    with pytest.raises(pdf_figures.FigureError, match=message):
        build(tmp_path, caps, [figure])


def test_capture_of_another_executable_or_mission_is_refused(tmp_path, caps):
    with pytest.raises(pdf_figures.FigureError, match="autre exécutable"):
        build(tmp_path, caps, [{"id": "f", "capture": "c1", "marks": [{"n": 1, "target": "#ok"}]}], exe_sha256="b" * 64)
    with pytest.raises(pdf_figures.FigureError, match="de la mission demo, document apps"):
        build(tmp_path, caps, [{"id": "f", "capture": "c1", "marks": [{"n": 1, "target": "#ok"}]}], mission="apps")


def test_modified_capture_is_refused(tmp_path, caps):
    Image.new("RGB", (10, 10)).save(caps / "c1.png")
    with pytest.raises(pdf_figures.FigureError, match="modifiée"):
        build(tmp_path, caps, [{"id": "f", "capture": "c1", "marks": [{"n": 1, "target": "#ok"}]}])


def test_minimum_figure_count(tmp_path, caps):
    with pytest.raises(pdf_figures.FigureError, match="minimum 30"):
        build(tmp_path, caps, [{"id": "f", "capture": "c1", "marks": [{"n": 1, "target": "#ok"}]}], min_figures=30)


def typ(tmp_path, text: str) -> Path:
    p = tmp_path / "doc.typ"
    p.write_text(text, encoding="utf-8")
    return p


def test_badges_of_the_text_match_the_figure(tmp_path):
    figs = spec(tmp_path, [{"id": "f", "capture": "c1", "marks": [{"n": 1, "target": "#ok"}, {"n": 2, "target": "#sq"}]}])
    good = typ(tmp_path, '#fig("f", caption: [x], etapes: [#b(1) puis #b(2)])')
    assert check_figrefs.main([str(good), str(figs)]) == 0
    bad = typ(tmp_path, '#fig("f", caption: [x], etapes: [#b(1) seulement])')
    assert check_figrefs.main([str(bad), str(figs)]) == 1
    extra = typ(tmp_path, '#fig("f", caption: [x], etapes: [#b(1) #b(2) #b(3)])')
    assert check_figrefs.main([str(extra), str(figs)]) == 1


def test_instructor_figures_need_the_eight_rubrics(tmp_path):
    figs = spec(tmp_path, [{"id": "f", "capture": "c1", "marks": [{"n": 1, "target": "#ok"}]}])
    seven = typ(tmp_path, '#fig("f", regarder: [#b(1)], cliquer: [a], entrer: [a], attendu: [a], pourquoi: [a], observer: [a], erreur: [a])')
    assert check_figrefs.main([str(seven), str(figs), "--legend", "all"]) == 1
    eight = typ(tmp_path, '#fig("f", regarder: [#b(1)], cliquer: [a], entrer: [a], attendu: [a], pourquoi: [a], observer: [a], erreur: [a], echec: [a])')
    assert check_figrefs.main([str(eight), str(figs), "--legend", "all"]) == 0


def test_unplaced_figure_is_reported(tmp_path):
    figs = spec(tmp_path, [{"id": "f", "capture": "c1", "marks": [{"n": 1, "target": "#ok"}]},
                           {"id": "g", "capture": "c1", "marks": [{"n": 1, "target": "#ok"}]}])
    t = typ(tmp_path, '#fig("f", etapes: [#b(1)])')
    assert check_figrefs.main([str(t), str(figs)]) == 1


@pytest.mark.skipif(shutil.which("typst") is None, reason="typst absent")
def test_pdf_check_finds_a_document_without_outline(tmp_path):
    src = tmp_path / "plain.typ"
    src.write_text('#set page(footer: context [Page #counter(page).display() / #counter(page).final().first()])\n= Titre\nTexte.\n', encoding="utf-8")
    out = tmp_path / "plain.pdf"
    subprocess.run(["typst", "compile", str(src), str(out)], check=True)
    r = check_pdf.check(out, None, None, None)
    assert any("sommaire sans lien" in p for p in r["problems"])
    src.write_text('#set page(footer: context [Page #counter(page).display() / #counter(page).final().first()])\n#outline()\n= Titre\nTexte.\n', encoding="utf-8")
    subprocess.run(["typst", "compile", str(src), str(out)], check=True)
    assert check_pdf.check(out, None, None, None)["problems"] == []


import check_labels  # noqa: E402


def test_labels_of_a_document_are_found_in_the_application(tmp_path):
    ok = typ(tmp_path, "Cliquez « Enregistrer la fiche » puis « Lancer la vérification ».\n")
    assert check_labels.main([str(ok)]) == 0
    composite = typ(tmp_path, "La ligne « Fiche nominale : conforme / non conforme » donne le verdict.\n")
    assert check_labels.main([str(composite)]) == 0
    invented = typ(tmp_path, "Cliquez « Valider la mission maintenant ».\n")
    assert check_labels.main([str(invented)]) == 1
    allow = tmp_path / "allow.txt"
    allow.write_text("Valider la mission maintenant\n", encoding="utf-8")
    assert check_labels.main([str(invented), "--allow", str(allow)]) == 0
