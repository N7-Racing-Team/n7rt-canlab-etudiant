"""Annotates the screenshots of a tutorial: numbered badges, circles, boxes and arrows drawn from a JSON spec.

usage: python tools/docs/annotate.py <spec.json>

The spec lists, for each figure, the raw capture (relative to the spec), the annotated output (relative to the spec)
and its marks, in pixels of the raw capture:

    {"figures": [{"source": "raw/02-progression.png", "output": "02-progression.png", "marks": [
        {"n": 1, "circle": [cx, cy, r]},
        {"n": 2, "box": [x1, y1, x2, y2]},
        {"n": 3, "arrow": [[x1, y1], [x2, y2]]},
        {"n": 4, "pill": [x1, y1, x2, y2]}]}]}

A pill is the circle of an element wider than tall: a rounded outline whose ends are half-circles, hugging the
element (a circle around a wide button would cover its neighbours).

Every mark carries its number in a badge (at `badge` [x, y] when given, else next to the mark; radius `badge_r` of the
figure when given); the text of the tutorial refers to these numbers. A figure may give `keep` [[x1, y1, x2, y2], ...]: everything outside these regions is
greyed out first (only the panels the text is about stay readable), and `crop` [x1, y1, x2, y2]: the marks are drawn,
then only that region is kept (pixels of the raw capture). Deterministic: the same spec and captures give the same
files.
"""
from __future__ import annotations

import json
import math
import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

RED = (214, 39, 40)
WHITE = (255, 255, 255)
MASK = (226, 228, 232)
WIDTH = 4
BADGE_R = 15


def font(size: int) -> ImageFont.ImageFont:
    for name in ("arialbd.ttf", "DejaVuSans-Bold.ttf", "Arial Bold.ttf"):
        try:
            return ImageFont.truetype(name, size)
        except OSError:
            continue
    return ImageFont.load_default()


def badge(d: ImageDraw.ImageDraw, at: tuple[float, float], n: int, r: float = BADGE_R) -> None:
    x, y = at
    d.ellipse((x - r, y - r, x + r, y + r), fill=RED, outline=WHITE, width=2)
    text = str(n)
    f = font(round(18 * r / BADGE_R))
    box = d.textbbox((0, 0), text, font=f)
    d.text((x - (box[2] - box[0]) / 2 - box[0], y - (box[3] - box[1]) / 2 - box[1]), text, fill=WHITE, font=f)


def arrow(d: ImageDraw.ImageDraw, a: tuple[float, float], b: tuple[float, float]) -> None:
    d.line((a, b), fill=RED, width=WIDTH)
    angle = math.atan2(b[1] - a[1], b[0] - a[0])
    head = 18
    left = (b[0] - head * math.cos(angle - math.pi / 7), b[1] - head * math.sin(angle - math.pi / 7))
    right = (b[0] - head * math.cos(angle + math.pi / 7), b[1] - head * math.sin(angle + math.pi / 7))
    d.polygon((b, left, right), fill=RED)


def annotate(spec_path: Path) -> list[Path]:
    spec = json.loads(spec_path.read_text(encoding="utf-8"))
    root = spec_path.parent
    written = []
    for fig in spec["figures"]:
        img = Image.open(root / fig["source"]).convert("RGB")
        if "keep" in fig:
            shown = Image.new("RGB", img.size, MASK)
            for x1, y1, x2, y2 in fig["keep"]:
                box = tuple(int(round(v)) for v in (max(0, x1), max(0, y1), min(img.width, x2), min(img.height, y2)))
                if box[0] < box[2] and box[1] < box[3]:
                    shown.paste(img.crop(box), box[:2])
            img = shown
        d = ImageDraw.Draw(img)
        for m in fig["marks"]:
            if "circle" in m:
                cx, cy, r = m["circle"]
                d.ellipse((cx - r, cy - r, cx + r, cy + r), outline=RED, width=WIDTH)
                spot = (cx + r * 0.75, cy - r * 0.75)
            elif "pill" in m:
                x1, y1, x2, y2 = m["pill"]
                r = min(x2 - x1, y2 - y1) / 2
                d.rounded_rectangle((x1, y1, x2, y2), radius=r, outline=RED, width=WIDTH)
                # on the upper-left arc, as a circle's badge sits on its upper-right
                spot = (x1 + r * (1 - math.sqrt(0.5)), y1 + r * (1 - math.sqrt(0.5)))
            elif "box" in m:
                x1, y1, x2, y2 = m["box"]
                d.rounded_rectangle((x1, y1, x2, y2), radius=6, outline=RED, width=WIDTH)
                spot = (x1, y1)
            elif "arrow" in m:
                (x1, y1), (x2, y2) = m["arrow"]
                arrow(d, (x1, y1), (x2, y2))
                spot = (x1, y1)
            else:
                raise ValueError(f"{fig['output']}: mark without circle, pill, box or arrow: {m}")
            badge(d, tuple(m.get("badge", spot)), int(m["n"]), fig.get("badge_r", BADGE_R))
        if "crop" in fig:
            x1, y1, x2, y2 = (int(round(v)) for v in fig["crop"])
            if not (0 <= x1 < x2 <= img.width and 0 <= y1 < y2 <= img.height):
                raise ValueError(f"{fig['output']}: crop {fig['crop']} outside the {img.width}x{img.height} capture")
            img = img.crop((x1, y1, x2, y2))
        out = root / fig["output"]
        out.parent.mkdir(parents=True, exist_ok=True)
        img.save(out, optimize=True)
        written.append(out)
    return written


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    for p in annotate(Path(sys.argv[1])):
        print(p)
