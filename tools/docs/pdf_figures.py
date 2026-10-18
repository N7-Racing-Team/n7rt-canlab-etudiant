"""Figures of the 0.3 PDF documents: real captures of the application, annotated from their targets.

usage: python tools/docs/pdf_figures.py <figures.json> <captures dir> <output dir> <manifest.json> [--root DIR]
                                       [--exe-sha256 HEX] [--mission ID] [--min-figures N]

A capture is a PNG of the application window taken over WebView2 DevTools (tools/docs capture campaign) and its JSON:
    {"id", "mission", "run", "exe_sha256", "scale", "viewport": [w, h], "png", "png_sha256",
     "targets": {name: {"rect": [x, y, w, h], "visible": true, "points": [[x, y], ...]?}}, "step", "taken_at"}
with every rectangle in CSS pixels of the page (image pixels = CSS pixels x scale).

The figure spec names, for each figure, its capture and its marks by target name; nothing is placed by hand in
pixels:
    {"document": "...", "mission": "apps" | null, "figures": [{"id", "capture",
      "marks": [{"n": 1, "target": "#progress-verify", "shape": "auto|circle|pill|box|trace", "arrow": true, "badge": "nw"}],
      "hide": [target | [x, y, w, h]], "dim": [...], "keep": [...], "crop": "auto" | [x, y, w, h] | {"around": [...]}}]}

- hide: solid fill (content that must not be readable: public figures); keep: everything else is hidden;
- dim: darkened, still readable for orientation;
- crop "auto": the marks, their badges and arrows plus context, at least MIN_CROP of the window in each direction.

The build fails (exit 1) when a capture or a target is missing, a target is not visible, a capture was taken by
another executable or for another mission, a mark, badge or arrow falls outside the crop, a circle is much larger
than its target, the numbers of a figure are not 1..n, or the document has fewer figures than --min-figures.
Deterministic: the same spec and captures give the same files.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import math
import sys
from dataclasses import dataclass
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

RED = (214, 39, 40)
WHITE = (255, 255, 255)
HIDE = (58, 62, 70)
# sizes in CSS pixels of the page, multiplied by the capture scale
LINE = 2.6
MARGIN = 5.0
BADGE_R = 10.5
ARROW_LEN = 58.0
ARROW_HEAD = 13.0
MIN_CROP = (0.42, 0.40)      # fraction of the window kept around the marks, width and height
ASPECT = (1.25, 2.3)         # width / height of an automatic crop
CONTEXT = 46.0               # extra context around the marks before the minimum applies
MAX_CIRCLE = 150.0           # largest target side (CSS px) a circle may surround


class FigureError(Exception):
    pass


def font(size: int) -> ImageFont.ImageFont:
    for name in ("segoeuib.ttf", "arialbd.ttf", "DejaVuSans-Bold.ttf"):
        try:
            return ImageFont.truetype(name, size)
        except OSError:
            continue
    return ImageFont.load_default()


@dataclass
class Box:
    x1: float
    y1: float
    x2: float
    y2: float

    @property
    def w(self) -> float:
        return self.x2 - self.x1

    @property
    def h(self) -> float:
        return self.y2 - self.y1

    @property
    def c(self) -> tuple[float, float]:
        return ((self.x1 + self.x2) / 2, (self.y1 + self.y2) / 2)

    def grow(self, d: float) -> "Box":
        return Box(self.x1 - d, self.y1 - d, self.x2 + d, self.y2 + d)

    def union(self, o: "Box") -> "Box":
        return Box(min(self.x1, o.x1), min(self.y1, o.y1), max(self.x2, o.x2), max(self.y2, o.y2))

    def inside(self, o: "Box", pad: float = 0) -> bool:
        return self.x1 >= o.x1 + pad and self.y1 >= o.y1 + pad and self.x2 <= o.x2 - pad and self.y2 <= o.y2 - pad

    def overlaps(self, o: "Box") -> bool:
        return not (self.x2 <= o.x1 or o.x2 <= self.x1 or self.y2 <= o.y1 or o.y2 <= self.y1)

    def t(self) -> tuple[float, float, float, float]:
        return (self.x1, self.y1, self.x2, self.y2)


def rect_box(r, k: float) -> Box:
    x, y, w, h = r
    return Box(x * k, y * k, (x + w) * k, (y + h) * k)


def load_capture(dirs, cid: str) -> tuple[dict, Image.Image]:
    """The capture `cid` from the first folder that holds it (one folder, or a list of folders)."""
    folders = [Path(d) for d in (dirs if isinstance(dirs, (list, tuple)) else [dirs])]
    meta_p = next((d / f"{cid}.json" for d in folders if (d / f"{cid}.json").exists()), None)
    if meta_p is None:
        raise FigureError(f"capture {cid} absente ({', '.join(str(d) for d in folders)})")
    dir_ = meta_p.parent
    meta = json.loads(meta_p.read_text(encoding="utf-8"))
    png = dir_ / meta.get("png", f"{cid}.png")
    if not png.exists():
        raise FigureError(f"image de la capture {cid} absente ({png})")
    data = png.read_bytes()
    if meta.get("png_sha256") and hashlib.sha256(data).hexdigest() != meta["png_sha256"]:
        raise FigureError(f"capture {cid} : image modifiée depuis la prise (SHA-256)")
    img = Image.open(png).convert("RGB")
    return meta, img


def target_of(meta: dict, name, k: float, fig_id: str):
    """(box in image pixels, points or None) of a named target, or of an explicit rectangle [x, y, w, h]."""
    if isinstance(name, list):
        return rect_box(name, k), None
    t = meta.get("targets", {}).get(name)
    if t is None and name.startswith("cm:"):
        # lines A..B inside a captured range a..b: every editor line has the same height
        A, B = (int(x) for x in name[3:].split("-"))
        for key, cand in meta.get("targets", {}).items():
            if key.startswith("cm:") and cand.get("visible"):
                a, b = (int(x) for x in key[3:].split("-"))
                if a <= A <= B <= b:
                    x, y, w, h = cand["rect"]
                    lh = h / (b - a + 1)
                    t = {"rect": [x, y + (A - a) * lh, w, (B - A + 1) * lh], "visible": True}
                    break
    if t is None:
        raise FigureError(f"{fig_id} : cible {name!r} absente de la capture {meta['id']}")
    if not t.get("visible", False):
        raise FigureError(f"{fig_id} : cible {name!r} non visible sur la capture {meta['id']}")
    box = rect_box(t["rect"], k)
    if box.w <= 0 or box.h <= 0:
        raise FigureError(f"{fig_id} : cible {name!r} de taille nulle")
    pts = [(x * k, y * k) for x, y in t["points"]] if t.get("points") else None
    return box, pts


def outline_of(box: Box, shape: str, k: float) -> tuple[str, Box]:
    m = MARGIN * k
    if shape == "auto":
        ratio = box.w / box.h
        shape = "box" if (box.w > 260 * k and box.h > 90 * k) else ("circle" if 0.6 <= ratio <= 1.7 else "pill")
    if shape == "circle":
        r = math.hypot(box.w, box.h) / 2 + m
        cx, cy = box.c
        return "circle", Box(cx - r, cy - r, cx + r, cy + r)
    if shape in ("pill", "box", "trace"):
        return shape, box.grow(m)
    raise FigureError(f"forme inconnue : {shape}")


def draw_outline(d: ImageDraw.ImageDraw, shape: str, o: Box, k: float, pts=None) -> None:
    w = max(2, round(LINE * k))
    if shape == "circle":
        d.ellipse(o.t(), outline=RED, width=w)
    elif shape == "pill":
        d.rounded_rectangle(o.t(), radius=min(o.w, o.h) / 2, outline=RED, width=w)
    elif shape == "box":
        d.rounded_rectangle(o.t(), radius=round(4 * k), outline=RED, width=w)
    elif shape == "trace":
        if not pts:
            raise FigureError("tracé sans points")
        d.line(pts, fill=RED, width=round(w * 1.6), joint="curve")


def draw_arrow(d: ImageDraw.ImageDraw, a, b, k: float) -> None:
    w = max(2, round(LINE * k))
    d.line((a, b), fill=RED, width=w)
    ang = math.atan2(b[1] - a[1], b[0] - a[0])
    h = ARROW_HEAD * k
    left = (b[0] - h * math.cos(ang - math.pi / 7), b[1] - h * math.sin(ang - math.pi / 7))
    right = (b[0] - h * math.cos(ang + math.pi / 7), b[1] - h * math.sin(ang + math.pi / 7))
    d.polygon((b, left, right), fill=RED)


def draw_badge(d: ImageDraw.ImageDraw, at, n: int, k: float) -> None:
    r = BADGE_R * k
    x, y = at
    d.ellipse((x - r, y - r, x + r, y + r), fill=RED, outline=WHITE, width=max(2, round(1.3 * k)))
    f = font(round(r * 1.25))
    tb = d.textbbox((0, 0), str(n), font=f)
    d.text((x - (tb[2] - tb[0]) / 2 - tb[0], y - (tb[3] - tb[1]) / 2 - tb[1]), str(n), fill=WHITE, font=f)


def _seg_dist(p, a, b) -> float:
    ax, ay = a
    bx, by = b
    dx, dy = bx - ax, by - ay
    t = 0.0 if dx == dy == 0 else max(0.0, min(1.0, ((p[0] - ax) * dx + (p[1] - ay) * dy) / (dx * dx + dy * dy)))
    return math.hypot(p[0] - (ax + t * dx), p[1] - (ay + t * dy))


def _segments_close(a1, a2, b1, b2, d: float) -> bool:
    """True when segment a1-a2 comes within d of segment b1-b2 (sampled: exact enough for a figure)."""
    for i in range(11):
        t = i / 10
        p = (a1[0] + (a2[0] - a1[0]) * t, a1[1] + (a2[1] - a1[1]) * t)
        if _seg_dist(p, b1, b2) < d:
            return True
    return False


def arrow_for(o: Box, occupied: list[Box], frame: Box, k: float, polylines=()):
    """An arrow from free space towards the outline: sixteen directions, three lengths; it may not cross another
    mark's box nor come close to a traced wire."""
    cx, cy = o.c
    rx, ry = o.w / 2, o.h / 2
    dirs = [(-1, -1), (1, -1), (-1, 1), (1, 1), (-1, 0), (1, 0), (0, -1), (0, 1)]
    dirs += [(math.cos(a), math.sin(a)) for a in (math.radians(x) for x in (202.5, 337.5, 157.5, 22.5, 247.5, 292.5, 112.5, 67.5))]
    for L in (ARROW_LEN * k, 0.75 * ARROW_LEN * k, 1.4 * ARROW_LEN * k):
        for dx, dy in dirs:
            n = math.hypot(dx, dy)
            ux, uy = dx / n, dy / n
            tip = (cx + ux * rx * 0.98, cy + uy * ry * 0.98)
            tail = (tip[0] + ux * L, tip[1] + uy * L)
            seg = Box(min(tip[0], tail[0]), min(tip[1], tail[1]), max(tip[0], tail[0]), max(tip[1], tail[1])).grow(4 * k)
            if not Box(tail[0] - 1, tail[1] - 1, tail[0] + 1, tail[1] + 1).inside(frame, 2 * BADGE_R * k):
                continue
            if any(seg.overlaps(b) for b in occupied):
                continue
            if any(_segments_close(tail, tip, p[i], p[i + 1], 8 * k) for p in polylines for i in range(len(p) - 1)):
                continue
            return tail, tip
    return None


COMPASS = {"n": (0, -1), "ne": (1, -1), "e": (1, 0), "se": (1, 1), "s": (0, 1), "sw": (-1, 1), "w": (-1, 0), "nw": (-1, -1)}


def compass_spot(o: Box, shape: str, where: str, k: float):
    """The badge position asked by a mark ("badge": "nw" …): just outside the outline in that direction."""
    if where not in COMPASS:
        raise FigureError(f"position de pastille inconnue : {where} (attendu {', '.join(COMPASS)})")
    dx, dy = COMPASS[where]
    r = BADGE_R * k
    cx, cy = o.c
    if shape == "circle":
        n = math.hypot(dx, dy)
        rr = o.w / 2 + r * 0.95
        return (cx + rr * dx / n, cy + rr * dy / n)
    return (cx + dx * (o.w / 2 + r * 0.75), cy + dy * (o.h / 2 + r * 0.75))


def badge_for(o: Box, shape: str, occupied: list[Box], frame: Box, k: float, prefer=None):
    r = BADGE_R * k
    cx, cy = o.c
    if shape == "circle":
        # just outside the circle, so that the badge never covers the target or its label
        rr = o.w / 2 + r * 0.95
        spots = [(cx + rr * math.cos(a), cy + rr * math.sin(a)) for a in (-math.pi / 4, -3 * math.pi / 4, math.pi / 4, 3 * math.pi / 4, -math.pi / 2, math.pi)]
    else:
        d = r * 0.75
        spots = [(o.x1 - d, o.y1 - d), (o.x2 + d, o.y1 - d), (o.x1 - d, o.y2 + d), (o.x2 + d, o.y2 + d),
                 (o.x1 - r * 1.2, cy), (o.x2 + r * 1.2, cy)]
    if prefer is not None:
        spots.insert(0, prefer)
    for s in spots:
        bb = Box(s[0] - r, s[1] - r, s[0] + r, s[1] + r)
        if bb.inside(frame, 1) and not any(bb.overlaps(b) for b in occupied):
            return s, bb
    s = spots[0]
    return s, Box(s[0] - r, s[1] - r, s[0] + r, s[1] + r)


def compose(fig: dict, meta: dict, img: Image.Image, spec: dict, opts) -> tuple[Image.Image, dict]:
    fid = fig["id"]
    if opts.exe_sha256 and meta.get("exe_sha256") != opts.exe_sha256:
        raise FigureError(f"{fid} : capture {meta['id']} prise avec un autre exécutable ({str(meta.get('exe_sha256'))[:12]})")
    want = spec.get("mission")
    if want and meta.get("mission") not in (want,):
        raise FigureError(f"{fid} : capture {meta['id']} de la mission {meta.get('mission')}, document {want}")
    k = float(meta["scale"])
    W, H = img.size
    full = Box(0, 0, W, H)
    # hide / keep / dim
    for name in fig.get("hide", []):
        b, _ = target_of(meta, name, k, fid)
        ImageDraw.Draw(img).rectangle(b.t(), fill=HIDE)
    if fig.get("keep"):
        shown = Image.new("RGB", img.size, HIDE)
        for name in fig["keep"]:
            b, _ = target_of(meta, name, k, fid)
            bb = tuple(int(round(v)) for v in (max(0, b.x1), max(0, b.y1), min(W, b.x2), min(H, b.y2)))
            shown.paste(img.crop(bb), bb[:2])
        img = shown
    if fig.get("dim"):
        dark = Image.new("RGB", img.size, (0, 0, 0))
        mask = Image.new("L", img.size, 0)
        md = ImageDraw.Draw(mask)
        for name in fig["dim"]:
            b, _ = target_of(meta, name, k, fid)
            md.rectangle(b.t(), fill=150)
        img = Image.composite(dark, img, mask)
    d = ImageDraw.Draw(img)
    marks = fig.get("marks", [])
    if not marks:
        raise FigureError(f"{fid} : figure sans marque (une capture ne doit jamais laisser deviner où regarder)")
    numbers = [int(m["n"]) for m in marks]
    if sorted(numbers) != list(range(1, len(numbers) + 1)):
        raise FigureError(f"{fid} : numéros {numbers}, attendu 1..{len(numbers)}")
    placed = []
    occupied: list[Box] = []
    for m in marks:
        tbox, pts = target_of(meta, m["target"], k, fid)
        shape, o = outline_of(tbox, m.get("shape", "auto"), k)
        if shape == "circle":
            # a circle only around a compact target: a large or elongated one gets a pill or a box (a giant circle
            # covers its neighbours and no longer says where to look)
            ratio = tbox.w / tbox.h
            if max(tbox.w, tbox.h) > MAX_CIRCLE * k or not (0.55 <= ratio <= 1.8):
                raise FigureError(f"{fid} : cercle {m['n']} trop grand ou mal adapté à sa cible "
                                  f"({tbox.w / k:.0f}×{tbox.h / k:.0f} px) : utiliser pill ou box")
        if shape in ("box", "pill"):
            # a panel along the window's edge: its outline drawn just inside the image
            inset = LINE * k / 2 + 1
            o = Box(max(o.x1, inset), max(o.y1, inset), min(o.x2, W - inset), min(o.y2, H - inset))
        placed.append((m, shape, o, tbox, pts))
        if shape != "trace":
            occupied.append(o)
    polylines = [pts for (_, shape, _, _, pts) in placed if shape == "trace" and pts]
    frame = full
    if isinstance(fig.get("crop"), list):
        frame = rect_box(fig["crop"], k)
    drawn = []
    for m, shape, o, tbox, pts in placed:
        draw_outline(d, shape, o, k, pts)
        extent = o
        tail = None
        if m.get("arrow"):
            got = arrow_for(o, [b for b in occupied if b is not o], frame, k,
                            polylines=[p for p in polylines if p is not pts])
            if got is None:
                raise FigureError(f"{fid} : pas de place pour la flèche de la marque {m['n']}")
            tail, tip = got
            draw_arrow(d, tail, tip, k)
            extent = extent.union(Box(min(tail[0], tip[0]), min(tail[1], tip[1]), max(tail[0], tip[0]), max(tail[1], tip[1])))
            occupied.append(Box(min(tail[0], tip[0]), min(tail[1], tip[1]), max(tail[0], tip[0]), max(tail[1], tip[1])))
        spot = None
        if m.get("badge"):
            # the position the writer asked for, just outside its own outline: it may touch that outline (the
            # automatic spots may not), never the target itself, another mark, an arrow or another badge
            want = compass_spot(o, shape, m["badge"], k)
            r = BADGE_R * k
            wb = Box(want[0] - r, want[1] - r, want[0] + r, want[1] + r)
            if (wb.inside(frame, 1) and not wb.overlaps(tbox.grow(-2 * k))
                    and not any(wb.overlaps(b) for b in occupied if b is not o)):
                spot, bb = want, wb
            else:
                spot, bb = badge_for(o, shape, occupied, frame, k, prefer=want)
        if spot is None:
            spot, bb = badge_for(o, shape, occupied, frame, k, prefer=tail)
        draw_badge(d, spot, int(m["n"]), k)
        occupied.append(bb)
        drawn.append((m, extent.union(bb)))
    # crop
    crop = fig.get("crop", "auto")
    if crop == "auto" or isinstance(crop, dict):
        around = None
        for _, e in drawn:
            around = e if around is None else around.union(e)
        if isinstance(crop, dict):
            for name in crop.get("around", []):
                b, _ = target_of(meta, name, k, fid)
                around = around.union(b)
        around = around.grow(CONTEXT * k)
        minw, minh = W * MIN_CROP[0], H * MIN_CROP[1]
        if isinstance(crop, dict) and "min" in crop:
            minw, minh = W * crop["min"][0], H * crop["min"][1]
        cx, cy = around.c
        w, h = max(around.w, minw), max(around.h, minh)
        # readable proportions on an A4 page: between ASPECT[0]:1 and ASPECT[1]:1
        if w / h > ASPECT[1]:
            h = min(H, w / ASPECT[1])
        elif w / h < ASPECT[0]:
            w = min(W, h * ASPECT[0])
        x1 = min(max(0, cx - w / 2), max(0, W - w))
        y1 = min(max(0, cy - h / 2), max(0, H - h))
        frame = Box(x1, y1, min(W, x1 + w), min(H, y1 + h))
    elif isinstance(crop, list):
        frame = rect_box(crop, k)
    elif crop == "none":
        frame = full
    for m, e in drawn:
        if not e.inside(frame.grow(0.5)):
            raise FigureError(f"{fid} : la marque {m['n']} (cercle, flèche ou pastille) sort du cadre")
    out = img.crop(tuple(int(round(v)) for v in frame.t()))
    info = {"marks": numbers, "capture": meta["id"], "capture_sha256": meta.get("png_sha256"), "mission": meta.get("mission"),
            "run": meta.get("run"), "exe_sha256": meta.get("exe_sha256"), "size": list(out.size),
            "targets": [m["target"] if isinstance(m["target"], str) else "rect" for m in marks]}
    return out, info


def build(spec_p: Path, captures: Path, out_dir: Path, manifest_p: Path, opts) -> dict:
    spec = json.loads(spec_p.read_text(encoding="utf-8"))
    out_dir.mkdir(parents=True, exist_ok=True)
    root = Path(opts.root).resolve() if opts.root else None
    manifest, errors = {}, []
    seen = set()
    for fig in spec["figures"]:
        try:
            if fig["id"] in seen:
                raise FigureError(f"{fig['id']} : identifiant en double")
            seen.add(fig["id"])
            meta, img = load_capture(captures, fig["capture"])
            out, info = compose(fig, meta, img, spec, opts)
            p = out_dir / f"{fig['id']}.png"
            # 256 colours, no dithering: an interface capture keeps its sharp text and red marks at half the size
            out.quantize(colors=256, method=Image.Quantize.MEDIANCUT, dither=Image.Dither.NONE).save(p, optimize=True)
            rel = "/" + p.resolve().relative_to(root).as_posix() if root else p.as_posix()
            manifest[fig["id"]] = {"path": rel, **info}
        except FigureError as e:
            errors.append(str(e))
    if opts.min_figures and len(manifest) < opts.min_figures:
        errors.append(f"{spec.get('document')} : {len(manifest)} figures, minimum {opts.min_figures}")
    # bytes, not write_text: on Windows write_text would turn every line end into CRLF (the repositories are * -text)
    manifest_p.write_bytes((json.dumps(manifest, indent=1, ensure_ascii=False) + "\n").encode("utf-8"))
    if errors:
        raise FigureError("\n".join(errors))
    return manifest


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("spec", type=Path)
    ap.add_argument("captures", type=Path)
    ap.add_argument("out", type=Path)
    ap.add_argument("manifest", type=Path)
    ap.add_argument("--root")
    ap.add_argument("--exe-sha256")
    ap.add_argument("--min-figures", type=int, default=0)
    opts = ap.parse_args(argv)
    try:
        m = build(opts.spec, opts.captures, opts.out, opts.manifest, opts)
    except FigureError as e:
        print(f"FIGURES FAIL\n{e}", file=sys.stderr)
        return 1
    print(f"FIGURES OK {len(m)} figures, {sum(len(v['marks']) for v in m.values())} marques")
    return 0


if __name__ == "__main__":
    sys.exit(main())
