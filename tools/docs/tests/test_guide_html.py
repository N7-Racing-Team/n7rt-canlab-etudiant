"""Student guides: the HTML copies match their Markdown, every link and anchor resolves, the archive ships every
document the guide links to, and each « PDF §x, p. n » reference points to the page where that section starts."""
from __future__ import annotations

import re
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "tools/docs"))
sys.path.insert(0, str(ROOT / "tools/release"))
import build_guide_html as G  # noqa: E402
import build_student as B  # noqa: E402

LINK = re.compile(r"\[([^\]]+)\]\(([^)\s]+)\)")
PDF = "docs/N7RT_CANLab_0.3_Tutoriel_Etudiant.pdf"


def markdown(source: str) -> str:
    return (ROOT / source).read_text(encoding="utf-8")


def without_code(text: str) -> str:
    text = re.sub(r"```.*?```", "", text, flags=re.S)
    return re.sub(r"`[^`]*`", "", text)


def test_html_copies_are_up_to_date():
    assert G.main(["--check"]) == 0, "run: python tools/docs/build_guide_html.py"


@pytest.mark.parametrize("source", sorted(G.GUIDES))
def test_every_link_and_anchor_resolves(source):
    for label, target in LINK.findall(without_code(markdown(source))):
        if re.match(r"^[a-z]+:", target):
            continue
        path, _, fragment = target.partition("#")
        file = (ROOT / source).parent / path if path else ROOT / source
        assert file.resolve().is_file(), f"{source}: [{label}] -> {target}: no such file"
        if fragment and file.suffix == ".md":
            rel = file.resolve().relative_to(ROOT).as_posix()
            anchors = {a for _, a, _ in G.convert(file.read_text(encoding="utf-8"), rel)[2]}
            assert fragment in anchors, f"{source}: [{label}] -> #{fragment}: no such heading"


def test_the_archive_ships_every_document_the_guides_link_to():
    shipped = {"docs/" + name for name in B.STUDENT_DOCS} | {"DEMARRER_ICI.md", "DEMARRER_ICI.html"}
    for source, html in G.GUIDES.items():
        assert source in shipped and html in shipped, f"{source} or {html} is not in the student archive"
        for _, target in LINK.findall(without_code(markdown(source))):
            path = target.partition("#")[0]
            if path and not re.match(r"^[a-z]+:", path):
                rel = ((ROOT / source).parent / path).resolve().relative_to(ROOT).as_posix()
                assert rel in shipped, f"{source} links to {rel}, which the archive does not ship"


@pytest.mark.parametrize("source", sorted(G.GUIDES))
def test_no_raw_angle_bracket_that_github_would_hide(source):
    text = re.sub(r"<https?://[^>\s]+>", "", without_code(markdown(source)))
    assert not re.search(r"<[A-Za-zé/]", text), "a <placeholder> outside a code span disappears on GitHub"


def test_html_copies_are_self_contained():
    for target in G.GUIDES.values():
        page = (ROOT / target).read_text(encoding="utf-8")
        assert not re.search(r"<(link|img|iframe)\b|\bsrc=|@import|url\(", page), target
        assert '<meta charset="utf-8">' in page and "<title>" in page


def test_pdf_references_point_to_the_page_where_the_section_starts():
    pypdf = pytest.importorskip("pypdf")
    reader = pypdf.PdfReader(str(ROOT / PDF))
    start: dict[str, int] = {}

    def walk(items):
        for item in items:
            if isinstance(item, list):
                walk(item)
            else:
                number = item.title.split(" ", 1)[0]
                start.setdefault(number, reader.get_destination_page_number(item) + 1)
    walk(reader.outline)
    checked = 0
    for source in G.GUIDES:
        for label, target in LINK.findall(markdown(source)):
            if PDF.rsplit("/", 1)[1] not in target:
                continue
            m = re.match(r"(?:PDF )?§([\d.]+)(?: (?:et|à) §[\d.]+)?, p\. (\d+)", label)
            page = int(target.rsplit("#page=", 1)[1])
            assert m, f"{source}: [{label}] is not of the form « PDF §x, p. n »"
            assert int(m.group(2)) == page, f"{source}: [{label}] links to page {page}"
            assert start[m.group(1)] == page, f"{source}: §{m.group(1)} starts on page {start[m.group(1)]}"
            checked += 1
    assert checked >= 20


def test_heading_anchors_follow_github():
    assert G.slug("1. Télécharger et extraire l'archive") == "1-télécharger-et-extraire-larchive"
    assert G.slug("6. RUN du Physical Lab et Start du bus : deux choses différentes") == \
        "6-run-du-physical-lab-et-start-du-bus--deux-choses-différentes"
    assert G.link_target("docs/stm32-toolchain-windows.md#2-arm", "DEMARRER_ICI.md") == \
        "docs/stm32-toolchain-windows.html#2-arm"
    assert G.link_target("../DEMARRER_ICI.md", "docs/stm32-toolchain-windows.md") == "../DEMARRER_ICI.html"
    assert G.link_target("docs/demarrage-5-minutes.md", "DEMARRER_ICI.md") == "docs/demarrage-5-minutes.md"
