"""Tutorial figures: annotate.py (marks, greyed-out regions, crop) and tutorial_figures.py (the plan of each figure,
from the rectangles a progression report carries)."""
from __future__ import annotations

import json
import sys
from pathlib import Path

import pytest
from PIL import Image

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import annotate  # noqa: E402
import tutorial_figures as tf  # noqa: E402


def capture(path: Path, size=(400, 300), color=(10, 120, 200)) -> Path:
    Image.new("RGB", size, color).save(path)
    return path


def test_annotate_greys_out_all_but_the_kept_regions_then_crops(tmp_path):
    capture(tmp_path / "raw.png")
    spec = tmp_path / "spec.json"
    spec.write_text(json.dumps({"figures": [{"source": "raw.png", "output": "out/fig.png", "keep": [[50, 40, 150, 140]],
                                             "marks": [{"n": 1, "box": [60, 50, 140, 130]}], "crop": [20, 20, 200, 160]}]}),
                    encoding="utf-8")
    [out] = annotate.annotate(spec)
    img = Image.open(out).convert("RGB")
    assert img.size == (180, 140)
    assert img.getpixel((5, 130)) == annotate.MASK            # (25, 150) of the capture: outside the kept region
    assert img.getpixel((60, 60)) == (10, 120, 200)           # (80, 80): kept, inside the box
    assert img.getpixel((40, 70)) == annotate.RED             # (60, 90): the box's left edge
    first = out.read_bytes()
    annotate.annotate(spec)
    assert out.read_bytes() == first                          # deterministic


def test_annotate_draws_a_pill_hugging_a_wide_element(tmp_path):
    capture(tmp_path / "raw.png")
    spec = tmp_path / "spec.json"
    spec.write_text(json.dumps({"figures": [{"source": "raw.png", "output": "fig.png",
                                             "marks": [{"n": 1, "pill": [50, 100, 350, 140], "badge": [30, 80]}]}]}),
                    encoding="utf-8")
    [out] = annotate.annotate(spec)
    img = Image.open(out).convert("RGB")
    assert img.getpixel((51, 120)) == annotate.RED            # the left end, at mid-height
    assert img.getpixel((200, 101)) == annotate.RED           # the top edge
    assert img.getpixel((52, 102)) == (10, 120, 200)          # the bounding box's corner: outside the round end
    assert img.getpixel((200, 160)) == (10, 120, 200)         # nothing below: the outline stays on the element
    assert img.getpixel((20, 80)) == annotate.RED             # the badge where the spec puts it (left of its digit)


def test_annotate_draws_the_badges_at_the_radius_of_the_figure(tmp_path):
    capture(tmp_path / "raw.png")
    spec = tmp_path / "spec.json"
    marks = [{"n": 1, "box": [200, 200, 300, 260], "badge": [100, 100]}]
    spec.write_text(json.dumps({"figures": [{"source": "raw.png", "output": "small.png", "badge_r": 10, "marks": marks},
                                            {"source": "raw.png", "output": "large.png", "marks": marks}]}), encoding="utf-8")
    small, large = (Image.open(p).convert("RGB") for p in annotate.annotate(spec))
    assert small.getpixel((88, 100)) == (10, 120, 200) and large.getpixel((88, 100)) == annotate.RED


def test_annotate_refuses_a_crop_outside_the_capture(tmp_path):
    capture(tmp_path / "raw.png")
    spec = tmp_path / "spec.json"
    spec.write_text(json.dumps({"figures": [{"source": "raw.png", "output": "fig.png", "marks": [], "crop": [0, 0, 500, 100]}]}),
                    encoding="utf-8")
    with pytest.raises(ValueError, match="outside"):
        annotate.annotate(spec)


def rects_for(name: str) -> dict:
    """Every element the plan of `name` needs, stacked in a 300 x 500 CSS window."""
    plan = tf.PLAN[name]
    sels = list(dict.fromkeys([*(s for s, *_ in tf._keeps(plan)), *(s for _, _, ss, *_ in plan["marks"] for s in ss)]))
    return {s: [40, 30 + 25 * k, 200, 20] for k, s in enumerate(sels)}


def test_a_figure_maps_css_rectangles_to_the_window_capture():
    shot = {"dpr": 1.5, "inner": [300, 500], "rects": rects_for("02-progression")}
    # capture of the visible frame: 1 px borders and a 30 px title bar around 450 x 750 client pixels
    fig = tf.figure("02-progression", shot, (452, 781), "raw/02-progression.png", "../out/02-progression.png")
    ox, oy = 1, 30
    x, y, w, h = shot["rects"]["#progress-section"]
    m = tf.MARGIN
    assert fig["keep"] == [[ox + (x - m) * 1.5, oy + (y - m) * 1.5, ox + (x + w + m) * 1.5, oy + (y + h + m) * 1.5]]
    assert [mk["n"] for mk in fig["marks"]] == [1, 2, 3, 4, 5]
    assert "pill" in fig["marks"][2] and "box" in fig["marks"][0]
    x1, y1, x2, y2 = fig["crop"]
    assert 0 <= x1 < x2 <= 452 and 0 <= y1 < y2 <= 781


def test_a_circle_of_the_plan_is_a_pill_hugging_its_element_with_the_badge_outside():
    shot = {"dpr": 1.5, "inner": [300, 500], "rects": rects_for("02-progression")}
    fig = tf.figure("02-progression", shot, (452, 781), "a.png", "b.png")
    x, y, w, h = shot["rects"]["#progress-verify"]
    x1, y1, x2, y2 = fig["marks"][3]["pill"]
    margin = (1 + x * 1.5 - x1) / 1.5
    assert 1 <= margin <= tf.MARK_MARGIN
    assert [x1, y1, x2, y2] == [1 + (x - margin) * 1.5, 30 + (y - margin) * 1.5, 1 + (x + w + margin) * 1.5, 30 + (y + h + margin) * 1.5]
    bx, by = fig["marks"][3]["badge"]
    assert bx + tf.BADGE_R <= x1 or by + tf.BADGE_R <= y1


@pytest.mark.parametrize("name", list(tf.PLAN))
def test_badges_and_outlines_never_sit_on_another_mark(name):
    # one badge per mark, beside it: never on another mark's outline or another badge; outlines never touch
    fig = tf.figure(name, {"dpr": 1.5, "inner": [300, 500], "rects": rects_for(name)}, (452, 781), "a.png", "b.png")
    rects = [m.get("pill") or m["box"] for m in fig["marks"]]
    for i, m in enumerate(fig["marks"]):
        for j, r in enumerate(rects):
            if j != i:
                assert tf._clearance(tuple(m["badge"]), r) >= tf.BADGE_R + 2, (name, m["n"], j)
                assert tf._gap(rects[i], r) > 0, (name, m["n"], j)
        for other in fig["marks"][:i]:
            assert ((m["badge"][0] - other["badge"][0]) ** 2 + (m["badge"][1] - other["badge"][1]) ** 2) ** 0.5 >= 2 * tf.BADGE_R + 2
    assert fig["badge_r"] == tf.BADGE_R


def test_the_multimeter_rows_keep_their_labels():
    shot = {"dpr": 1.5, "inner": [300, 500], "rects": rects_for("05-multimetre")}
    fig = tf.figure("05-multimetre", shot, (452, 781), "a.png", "b.png")
    x = shot["rects"][".meter-com"][0]
    assert tf.METER_LABEL_CSS > 0
    assert fig["keep"][2][0] == 1 + (x - tf.MARGIN - tf.METER_LABEL_CSS) * 1.5
    assert all("box" in m for m in fig["marks"])  # the display is read, not acted on


def test_the_status_bar_is_never_in_a_figure():
    # the progression section runs past the bottom of the window, over the status bar
    rects = rects_for("11-cloture")
    rects["#progress-section"] = [40, 30, 200, 600]
    fig = tf.figure("11-cloture", {"dpr": 1.5, "inner": [300, 500], "rects": rects}, (452, 781), "a.png", "b.png")
    top_of_status_bar = 30 + (500 - tf.STATUSBAR_CSS) * 1.5
    assert all(k[3] <= top_of_status_bar for k in fig["keep"])
    assert fig["crop"][3] <= top_of_status_bar


def test_the_confirmation_dialog_is_kept_without_the_bus_behind_it():
    shot = {"dpr": 1, "inner": [300, 500], "rects": rects_for("09-reproduire")}
    fig = tf.figure("09-reproduire", shot, (302, 531), "a.png", "b.png")
    x, y, w, h = shot["rects"]["#reproduce-dialog"]
    assert fig["keep"][1] == [1 + x, 30 + y, 1 + x + w, 30 + y + h]


def test_a_figure_without_a_rectangle_it_needs_is_refused():
    rects = rects_for("09-reproduire")
    del rects["#reproduce-confirm"]
    with pytest.raises(ValueError, match="reproduce-confirm"):
        tf.figure("09-reproduire", {"dpr": 1, "inner": [300, 500], "rects": rects}, (302, 531), "a.png", "b.png")


def test_the_plan_follows_the_numbered_lists_of_the_tutorial():
    text = (Path(__file__).resolve().parents[3] / "docs" / "tutoriel-etudiant-0.3.md").read_text(encoding="utf-8")
    for name in tf.PLAN:
        assert f"img/tutoriel-0.3/{name}.png" in text, name
    assert [n for n, _, _ in tf.PLAN["03-fiche-nominale"]["marks"]] == list(range(1, 9))
    assert [n for n, _, _ in tf.PLAN["06-fiche-incident"]["marks"]] == list(range(1, 6))


def test_build_writes_the_eleven_figures_from_the_reports(tmp_path):
    reports = []
    for part, names in (("a", list(tf.PLAN)[:8]), ("b", list(tf.PLAN)[8:])):
        report = tmp_path / "runs" / f"app_progression-apps_{part}.json"
        report.parent.mkdir(exist_ok=True)
        shots = {n: {"dpr": 1, "inner": [300, 500], "rects": rects_for(n)} for n in names}
        report.write_text(json.dumps({"shots": shots}), encoding="utf-8")
        for n in names:
            capture(report.parent / f"{report.stem}_{n}.png", size=(302, 531))
        reports.append(report)
    written = tf.build(tmp_path / "work", tmp_path / "figures", reports)
    assert sorted(p.name for p in written) == sorted(f"{n}.png" for n in tf.PLAN)
    assert all(p.resolve().parent == (tmp_path / "figures").resolve() for p in written)
    with pytest.raises(SystemExit, match="10-qualification"):
        tf.build(tmp_path / "work2", tmp_path / "figures2", reports[:1])
