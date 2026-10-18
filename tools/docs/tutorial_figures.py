"""Builds the figures of the student tutorial (docs/tutoriel-etudiant-0.3.md) from the captures of the progression
scenario on a board (tools/stm32/n7rt_app.py, progression-apps): each capture comes with the rectangles of the
elements the scenario named (report "shots"); this script turns them into numbered marks matching the « À faire » and
« À observer » lists of the tutorial, greys out everything but the panels the text is about and crops to them, then
runs annotate.py. Nothing outside the kept panels can show in a figure: never the bus, the slots, the bench canvas, a
filled fiche or the status bar.

usage: python tools/docs/tutorial_figures.py <work dir> <out dir> <report.json>...

The reports are the JSON reports of the scenario runs (until-incident then resume, or one full run), each next to its
captures <report stem>_<name>.png. <work dir> receives spec.json and the raw captures; <out dir> the annotated
figures <name>.png (docs/img/tutoriel-0.3).
"""
from __future__ import annotations

import json
import math
import os
import re
import shutil
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import annotate  # noqa: E402

STYLES = Path(__file__).resolve().parents[2] / "app/ui/styles"
# Margin around a kept panel and around the crop, in CSS pixels.
MARGIN = 12
# Margin of a mark around its element, in CSS pixels, when no other mark is near (closer marks get less: their
# outlines never touch).
MARK_MARGIN = 4
# Radius of the numbered badges of the tutorial's figures, in pixels of the capture: two badges fit beside two
# stacked lines of the progression's verdict (28 px apart at 125 %).
BADGE_R = 13
# Offset of a badge from the corner of its mark, in pixels of the capture.
BADGE_SHIFT = 6
# The window's status bar (board, CRC, session), at the bottom of the client area: never in a figure.
STATUSBAR_CSS = float(re.search(r"--h-statusbar:\s*([\d.]+)px", (STYLES / "tokens.css").read_text(encoding="utf-8")).group(1))


def _meter_label_css() -> float:
    """Width of the label column of a multimeter row and of its gap (lab.css `.meter-row`): « Fonction », « COM »,
    « VΩ » stand left of their select."""
    lab = (STYLES / "lab.css").read_text(encoding="utf-8")
    row = re.search(r"\.meter-row\s*\{[^}]*grid-template-columns:\s*([\d.]+)px[^}]*gap:\s*var\((--[\w-]+)\)", lab)
    gap = re.search(re.escape(row.group(2)) + r":\s*([\d.]+)px", (STYLES / "tokens.css").read_text(encoding="utf-8"))
    return float(row.group(1)) + float(gap.group(1))


METER_LABEL_CSS = _meter_label_css()


def _lines(*ks: int) -> list[str]:
    return [f"#progress-feedback .feedback-line@{k}" for k in ks]


# For each figure: the panels kept (and cropped to), then the marks (number, shape, elements joined into one
# rectangle[, place of the badge]), in the order of the tutorial's numbered lists. A kept panel is a selector, or
# (selector, margin[, extra CSS px on its left]). A « circle » is drawn as a pill hugging its element (where to act);
# a box frames what to read or fill. A badge takes the first free place among the places of BADGE_PLACES, away from
# the other marks and badges, unless the plan names one.
# The progression is in the cockpit's side panel, the fiche in the cockpit's bottom dock (tab « Fiche »: #progress-fiche
# is its scrolling area, kept without its tab bar), the multimeter in the Physical Lab (its rows keep their labels).
# The confirmation dialog of figure 09 is kept without a margin: the bus table lies right behind it; its button keeps
# its badge above « Annuler ». In figures 10 and 11 the end of the verdict is under the action bar (the panel scrolls):
# figure 10 marks the eight CP lines only, figure 11 the steps (all « VALIDÉ », the fiche exported) and the button.
PLAN: dict[str, dict] = {
    "01-ouvrir": {"keep": ["#student-locked"], "marks": [
        (1, "box", ["#student-locked-title"]), (2, "circle", ["#student-import"]), (3, "box", ["#student-code", "#student-open"])]},
    "02-progression": {"keep": ["#progress-section"], "marks": [
        (1, "box", ["#progress-steps"]), (2, "box", ["#progress-order"]), (3, "circle", ["#progress-fiche-toggle"]),
        (4, "circle", ["#progress-verify"]), (5, "box", ["#progress-blocked"])]},
    "03-fiche-nominale": {"keep": [("#progress-fiche", 0)], "marks": [
        *((k + 1, "box", [f"#progress-fiche .fiche-nav-item@{k}"]) for k in range(7)), (8, "circle", ["#progress-fiche .fiche-save"])]},
    "04-verdict-e0": {"keep": ["#progress-section"], "marks": [(2, "box", _lines(0)), (3, "box", _lines(1, 2))]},
    "05-multimetre": {"keep": [".meter-display", *((s, MARGIN, METER_LABEL_CSS) for s in (".meter-function", ".meter-com", ".meter-vohm"))],
                      "marks": [(1, "box", [".meter-function"]), (2, "box", [".meter-com"]), (3, "box", [".meter-vohm"]), (4, "box", [".meter-display"])]},
    "06-fiche-incident": {"keep": [("#progress-fiche", 0)], "marks": [
        (1, "box", ["#progress-fiche .incident-target-kind"]), (2, "box", ["#progress-fiche .incident-category"]),
        (3, "box", ["#progress-fiche .incident-state"]), (4, "box", ["#progress-fiche .incident-value-row"]),
        (5, "box", ["#progress-fiche .fiche-justification"])]},
    "07-consigner": {"keep": ["#progress-fiche .incident-consign", "#progress-fiche .incident-consigned"], "marks": [
        (1, "circle", ["#progress-fiche .incident-consign"]), (2, "box", ["#progress-fiche .incident-consigned"])]},
    "08-verdict-incident": {"keep": ["#progress-section"], "marks": [(1, "box", _lines(0)), (2, "box", _lines(1)), (3, "box", _lines(2))]},
    "09-reproduire": {"keep": ["#progress-reproduce", ("#reproduce-dialog", 0)], "marks": [
        (1, "circle", ["#progress-reproduce"]), (2, "circle", ["#reproduce-confirm"], "upper-left")]},
    "10-qualification": {"keep": ["#progress-section"], "marks": [(2, "box", _lines(*range(8)))]},
    "11-cloture": {"keep": ["#progress-section"], "marks": [(1, "box", ["#progress-steps"]), (2, "circle", ["#progress-final"])]},
}

# Where a badge goes first: beside a pill or a mark one line tall (its number reads with the line), at the corner of a
# taller box.
BADGE_PLACES = {"tall": ("upper-left", "left", "lower-left", "top", "bottom"), "line": ("left", "upper-left", "lower-left", "top", "bottom")}


def _keeps(plan: dict) -> list[tuple[str, float, float]]:
    """(selector, margin, extra left) of each kept panel."""
    out = []
    for k in plan["keep"]:
        k = (k,) if isinstance(k, str) else tuple(k)
        out.append((k[0], k[1] if len(k) > 1 else MARGIN, k[2] if len(k) > 2 else 0.0))
    return out


def _union(rects: list[list[float]]) -> list[float]:
    return [min(r[0] for r in rects), min(r[1] for r in rects), max(r[0] + r[2] for r in rects), max(r[1] + r[3] for r in rects)]


def _gap(a: list[float], b: list[float]) -> float:
    """Distance between two [x1, y1, x2, y2] rectangles along the axis that separates them (0: they overlap)."""
    dx = max(a[0] - b[2], b[0] - a[2], 0.0)
    dy = max(a[1] - b[3], b[1] - a[3], 0.0)
    return max(dx, dy)


def _clearance(p: tuple[float, float], r: list[float]) -> float:
    """Distance from point `p` to rectangle `r` [x1, y1, x2, y2] (0 inside)."""
    dx = max(r[0] - p[0], 0.0, p[0] - r[2])
    dy = max(r[1] - p[1], 0.0, p[1] - r[3])
    return math.hypot(dx, dy)


def _badge_at(place: str, box: list[float]) -> tuple[float, float]:
    x1, y1, x2, y2 = box
    return {
        "upper-left": (x1 - BADGE_SHIFT, y1 - BADGE_SHIFT),
        "left": (x1 - BADGE_R - 3, (y1 + y2) / 2),
        "lower-left": (x1 - BADGE_SHIFT, y2 + BADGE_SHIFT),
        "top": ((x1 + x2) / 2, y1 - BADGE_R - 3),
        "bottom": ((x1 + x2) / 2, y2 + BADGE_R + 3),
    }[place]


def _place_badges(marks: list[dict], forced: list[str | None]) -> None:
    """The badge of each mark, at the first place whose disc touches no other mark and no badge already placed."""
    rects = [m.get("pill") or m["box"] for m in marks]
    placed: list[tuple[float, float]] = []
    for i, m in enumerate(marks):
        line = "pill" in m or rects[i][3] - rects[i][1] <= 3 * BADGE_R
        places = (forced[i],) if forced[i] else BADGE_PLACES["line" if line else "tall"]
        def free(p: tuple[float, float]) -> bool:
            if p[0] < BADGE_R or p[1] < BADGE_R:
                return False
            if any(_clearance(p, r) < BADGE_R + 2 for j, r in enumerate(rects) if j != i):
                return False
            return all(math.hypot(p[0] - q[0], p[1] - q[1]) >= 2 * BADGE_R + 2 for q in placed)
        spots = [_badge_at(place, rects[i]) for place in places]
        spot = next((s for s in spots if free(s)), spots[0])
        m["badge"] = [spot[0], spot[1]]
        placed.append(spot)


def figure(name: str, shot: dict, size: tuple[int, int], source: str, output: str) -> dict:
    """The annotate.py figure of one capture: CSS rectangles of the client area mapped to the pixels of the window
    capture (frame borders around, title bar above)."""
    plan = PLAN[name]
    rects = shot.get("rects") or {}
    dpr = float(shot["dpr"])
    inner_w, inner_h = (float(v) for v in shot["inner"])
    width, height = size
    ox = round((width - inner_w * dpr) / 2)
    oy = round(height - inner_h * dpr - ox)

    def px(box: list[float], margin: float = 0, left: float = 0) -> list[float]:
        x1, y1, x2, y2 = box
        return [ox + (x1 - margin - left) * dpr, oy + (y1 - margin) * dpr, ox + (x2 + margin) * dpr, oy + (y2 + margin) * dpr]

    def found(sels: list[str]) -> list[list[float]]:
        missing = [s for s in sels if not rects.get(s)]
        if missing:
            raise ValueError(f"{name}: no rectangle for {missing} in the report")
        return [rects[s] for s in sels]

    bottom = oy + (inner_h - STATUSBAR_CSS) * dpr  # the top of the status bar
    keep = []
    for sel, margin, left in _keeps(plan):
        x1, y1, x2, y2 = px(_union(found([sel])), margin, left)
        keep.append([x1, y1, x2, min(y2, bottom)])
    elements = [_union(found(sels)) for _, _, sels, *_ in plan["marks"]]
    marks, forced = [], []
    for i, (n, shape, _sels, *place) in enumerate(plan["marks"]):
        # a mark keeps clear of its neighbours: half the gap to the nearest other marked element, minus 1 CSS px
        near = min((_gap(elements[i], e) for j, e in enumerate(elements) if j != i), default=math.inf)
        margin = min(MARK_MARGIN, max(1.0, near / 2 - 1))
        marks.append({"n": n, "pill" if shape == "circle" else "box": px(elements[i], margin)})
        forced.append(place[0] if place else None)
    _place_badges(marks, forced)
    crop = _union([[k[0], k[1], k[2] - k[0], k[3] - k[1]] for k in keep])
    for m in marks:  # every mark and its badge stay inside the figure
        x1, y1, x2, y2 = m.get("pill") or m["box"]
        bx, by = m["badge"]
        reach = BADGE_R + 4
        crop = [min(crop[0], x1, bx - reach), min(crop[1], y1, by - reach), max(crop[2], x2, bx + reach), max(crop[3], y2, by + reach)]
    crop = [max(0, round(crop[0])), max(0, round(crop[1])), min(width, round(crop[2])), min(height, round(crop[3]), round(bottom))]
    return {"source": source, "output": output, "keep": keep, "marks": marks, "crop": crop, "badge_r": BADGE_R}


def build(work: Path, out: Path, reports: list[Path]) -> list[Path]:
    from PIL import Image

    work.mkdir(parents=True, exist_ok=True)
    (work / "raw").mkdir(exist_ok=True)
    figures = {}
    for report in reports:
        shots = json.loads(report.read_text(encoding="utf-8")).get("shots") or {}
        for name, shot in shots.items():
            capture = report.parent / f"{report.stem}_{name}.png"
            if name not in PLAN or not capture.is_file():
                continue
            raw = work / "raw" / f"{name}.png"
            shutil.copyfile(capture, raw)
            with Image.open(raw) as img:
                size = img.size
            figures[name] = figure(name, shot, size, f"raw/{name}.png", _rel(out.resolve() / f"{name}.png", work.resolve()))
    missing = sorted(set(PLAN) - set(figures))
    if missing:
        raise SystemExit(f"captures missing from the reports: {', '.join(missing)}")
    spec = work / "spec.json"
    spec.write_text(json.dumps({"figures": [figures[n] for n in sorted(figures)]}, indent=1), encoding="utf-8")
    return annotate.annotate(spec)


def _rel(target: Path, base: Path) -> str:
    return os.path.relpath(target, base).replace("\\", "/")


if __name__ == "__main__":
    if len(sys.argv) < 4:
        sys.exit(__doc__)
    for p in build(Path(sys.argv[1]), Path(sys.argv[2]), [Path(a) for a in sys.argv[3:]]):
        print(p)
