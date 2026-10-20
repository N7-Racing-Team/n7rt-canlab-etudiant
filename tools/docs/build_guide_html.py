#!/usr/bin/env python3
"""Standalone HTML copies of the student guides, readable offline by a double-click (no Markdown reader).

    python tools/docs/build_guide_html.py           write the HTML files next to their Markdown source
    python tools/docs/build_guide_html.py --check   exit 1 if an HTML file is missing or out of date

The Markdown subset is the one the guides use: headings, paragraphs, flat lists, tables, fenced code blocks,
block quotes, inline code, bold, links and <https://…> autolinks. Heading ids follow GitHub's anchors, so a
link `#section` works both on GitHub and in the HTML copy. A relative link to another converted guide points to
its HTML copy; a link to the PDF tutorial keeps its `#page=N`. Same Markdown, same bytes: no date, no
dependency, one file per guide with its style and script inline.
"""
from __future__ import annotations

import argparse
import html
import re
import sys
from pathlib import Path, PurePosixPath

ROOT = Path(__file__).resolve().parents[2]
# Markdown source -> HTML copy, relative to the repository (and archive) root.
GUIDES = {
    "DEMARRER_ICI.md": "DEMARRER_ICI.html",
    "docs/stm32-toolchain-windows.md": "docs/stm32-toolchain-windows.html",
}

STYLE = """
:root { --bg: #ffffff; --fg: #1d1f24; --muted: #5b616e; --line: #d9dce3; --accent: #5b2bb5; --code-bg: #f3f2f7;
  --note-bg: #f1ecfb; --warn-bg: #fff4e5; --warn-line: #c77700; }
@media (prefers-color-scheme: dark) {
  :root { --bg: #16171b; --fg: #e6e7ea; --muted: #a2a7b3; --line: #33363e; --accent: #b79bf2; --code-bg: #22242a;
    --note-bg: #241d33; --warn-bg: #2d2416; --warn-line: #e0a040; }
}
* { box-sizing: border-box; }
html { scroll-behavior: smooth; }
body { margin: 0; background: var(--bg); color: var(--fg); font: 16px/1.6 "Segoe UI", system-ui, sans-serif; }
.layout { display: flex; gap: 32px; max-width: 1200px; margin: 0 auto; padding: 24px 16px 64px; }
nav.toc { flex: 0 0 260px; position: sticky; top: 16px; align-self: flex-start; max-height: calc(100vh - 32px);
  overflow: auto; font-size: 14px; border-right: 1px solid var(--line); padding-right: 16px; }
nav.toc p { margin: 0 0 8px; font-weight: 600; text-transform: uppercase; letter-spacing: .04em; color: var(--muted); }
nav.toc ol { list-style: none; margin: 0; padding: 0; }
nav.toc li { margin: 0 0 6px; }
nav.toc a { color: var(--fg); text-decoration: none; }
nav.toc a:hover { color: var(--accent); text-decoration: underline; }
main { flex: 1 1 auto; min-width: 0; }
h1 { font-size: 30px; line-height: 1.25; margin: 0 0 16px; }
h2 { font-size: 24px; margin: 48px 0 12px; padding-top: 12px; border-top: 2px solid var(--line); }
h3 { font-size: 19px; margin: 32px 0 8px; }
h4 { font-size: 16px; margin: 24px 0 8px; }
a { color: var(--accent); }
code { font-family: Consolas, "Cascadia Mono", monospace; font-size: .92em; background: var(--code-bg);
  padding: 1px 5px; border-radius: 3px; overflow-wrap: anywhere; }
pre { position: relative; background: var(--code-bg); border: 1px solid var(--line); border-radius: 6px;
  padding: 12px 14px; overflow-x: auto; }
pre code { background: none; padding: 0; font-size: 14px; overflow-wrap: normal; }
pre button { position: absolute; top: 6px; right: 6px; font: 12px "Segoe UI", sans-serif; padding: 2px 8px;
  border: 1px solid var(--line); border-radius: 4px; background: var(--bg); color: var(--fg); cursor: pointer; }
.table { overflow-x: auto; margin: 12px 0; }
table { border-collapse: collapse; width: 100%; font-size: 15px; }
th, td { border: 1px solid var(--line); padding: 6px 10px; text-align: left; vertical-align: top; }
th { background: var(--code-bg); }
blockquote { margin: 16px 0; padding: 8px 16px; background: var(--note-bg); border-left: 4px solid var(--accent);
  border-radius: 0 6px 6px 0; }
blockquote.warn { background: var(--warn-bg); border-left-color: var(--warn-line); }
blockquote p { margin: 6px 0; }
li { margin: 4px 0; }
.top { font-size: 14px; }
@media (max-width: 860px) { .layout { display: block; } nav.toc { position: static; max-height: none;
  border-right: 0; border-bottom: 1px solid var(--line); padding: 0 0 12px; margin-bottom: 16px; } }
@media print { nav.toc, pre button, .top { display: none; } .layout { display: block; } }
""".strip()

SCRIPT = """
document.querySelectorAll("pre").forEach(function (pre) {
  var b = document.createElement("button");
  b.type = "button"; b.textContent = "Copier";
  b.addEventListener("click", function () {
    var text = pre.querySelector("code").innerText;
    function done() { b.textContent = "Copié"; setTimeout(function () { b.textContent = "Copier"; }, 1500); }
    if (navigator.clipboard && window.isSecureContext) {
      navigator.clipboard.writeText(text).then(done, fallback);
    } else { fallback(); }
    function fallback() {
      var t = document.createElement("textarea"); t.value = text; document.body.appendChild(t); t.select();
      try { document.execCommand("copy"); done(); } catch (e) { b.textContent = "Sélectionnez et copiez"; }
      document.body.removeChild(t);
    }
  });
  pre.appendChild(b);
});
""".strip()


def slug(text: str) -> str:
    """GitHub's heading anchor: lower case, punctuation dropped (letters, digits, '-' and '_' kept), spaces -> '-'."""
    text = re.sub(r"<[^>]+>", "", text).strip().lower()
    return "".join("-" if c == " " else c for c in text if c.isalnum() or c in " -_")


def heading_text(raw: str) -> str:
    """Plain text of a Markdown heading (what GitHub slugs): code ticks, emphasis and link targets removed."""
    text = re.sub(r"\[([^\]]*)\]\([^)]*\)", r"\1", raw)
    return text.replace("`", "").replace("**", "")


def link_target(target: str, source: str) -> str:
    """A link to another converted guide points to its HTML copy; every other target is kept."""
    if re.match(r"^[a-z]+:|^#", target):
        return target
    path, _, fragment = target.partition("#")
    resolved = str(PurePosixPath(source).parent.joinpath(path))
    parts: list[str] = []
    for part in resolved.split("/"):
        if part == "..":
            parts.pop() if parts else parts.append(part)
        elif part not in ("", "."):
            parts.append(part)
    resolved = "/".join(parts)
    if resolved in GUIDES:
        path = path[: -len(".md")] + ".html"
    return path + ("#" + fragment if fragment else "")


def inline(text: str, source: str) -> str:
    """Inline Markdown to HTML: code spans first (verbatim), then links, autolinks and bold, everything escaped."""
    out: list[str] = []
    pos = 0
    for m in re.finditer(r"`([^`]+)`", text):
        out.append(_inline_plain(text[pos:m.start()], source))
        out.append(f"<code>{html.escape(m.group(1), quote=False)}</code>")
        pos = m.end()
    out.append(_inline_plain(text[pos:], source))
    return "".join(out)


def _inline_plain(text: str, source: str) -> str:
    pieces: list[str] = []
    pos = 0
    pattern = re.compile(r"\[([^\]]+)\]\(([^)\s]+)\)|<(https?://[^>\s]+)>")
    for m in pattern.finditer(text):
        pieces.append(_emphasis(html.escape(text[pos:m.start()], quote=False)))
        if m.group(3):
            url = m.group(3)
            pieces.append(f'<a href="{html.escape(url)}">{html.escape(url, quote=False)}</a>')
        else:
            label = _inline_plain(m.group(1), source)
            href = link_target(m.group(2), source)
            pieces.append(f'<a href="{html.escape(href)}">{label}</a>')
        pos = m.end()
    pieces.append(_emphasis(html.escape(text[pos:], quote=False)))
    return "".join(pieces)


def _emphasis(escaped: str) -> str:
    return re.sub(r"\*\*(.+?)\*\*", r"<strong>\1</strong>", escaped)


def _cells(line: str) -> list[str]:
    line = line.strip()
    if line.startswith("|"):
        line = line[1:]
    if line.endswith("|"):
        line = line[:-1]
    cells, cur, in_code = [], "", False
    for c in line:
        if c == "`":
            in_code = not in_code
        if c == "|" and not in_code:
            cells.append(cur.strip())
            cur = ""
        else:
            cur += c
    cells.append(cur.strip())
    return cells


def convert(markdown: str, source: str) -> tuple[str, str, list[tuple[int, str, str]]]:
    """(title, body HTML, headings as (level, id, text)) of a Markdown document."""
    lines = markdown.replace("\r\n", "\n").split("\n")
    body: list[str] = []
    headings: list[tuple[int, str, str]] = []
    seen: dict[str, int] = {}
    title = ""
    i = 0
    para: list[str] = []

    def flush() -> None:
        if para:
            body.append("<p>" + inline(" ".join(s.strip() for s in para), source) + "</p>")
            para.clear()

    while i < len(lines):
        line = lines[i]
        if line.startswith("```"):
            flush()
            code: list[str] = []
            i += 1
            while i < len(lines) and not lines[i].startswith("```"):
                code.append(lines[i])
                i += 1
            body.append("<pre><code>" + html.escape("\n".join(code), quote=False) + "</code></pre>")
            i += 1
            continue
        m = re.match(r"^(#{1,4}) (.*)$", line)
        if m:
            flush()
            level, raw = len(m.group(1)), m.group(2).strip()
            base = slug(heading_text(raw))
            n = seen.get(base, 0)
            seen[base] = n + 1
            anchor = base if n == 0 else f"{base}-{n}"
            if level == 1 and not title:
                title = heading_text(raw)
            headings.append((level, anchor, heading_text(raw)))
            body.append(f'<h{level} id="{html.escape(anchor)}">{inline(raw, source)}</h{level}>')
            i += 1
            continue
        if line.startswith("|") and i + 1 < len(lines) and re.match(r"^\|?\s*:?-{3,}", lines[i + 1]):
            flush()
            head = _cells(line)
            rows = []
            i += 2
            while i < len(lines) and lines[i].startswith("|"):
                rows.append(_cells(lines[i]))
                i += 1
            out = ['<div class="table"><table><thead><tr>']
            out += [f"<th>{inline(c, source)}</th>" for c in head]
            out.append("</tr></thead><tbody>")
            for row in rows:
                out.append("<tr>" + "".join(f"<td>{inline(c, source)}</td>" for c in row) + "</tr>")
            out.append("</tbody></table></div>")
            body.append("".join(out))
            continue
        if line.startswith(">"):
            flush()
            quote: list[str] = []
            while i < len(lines) and lines[i].startswith(">"):
                quote.append(lines[i][1:].strip())
                i += 1
            text = " ".join(quote)
            kind = ' class="warn"' if text.startswith("**Attention") else ""
            paragraphs = [p for p in re.split(r"\s*<br>\s*", text) if p]
            body.append(f"<blockquote{kind}>" + "".join(f"<p>{inline(p, source)}</p>" for p in paragraphs)
                        + "</blockquote>")
            continue
        m = re.match(r"^(\d+)\. (.*)$", line) or re.match(r"^([-*]) (.*)$", line)
        if m:
            flush()
            ordered = m.group(1)[0].isdigit()
            items: list[str] = []
            start = m.group(1) if ordered else ""
            while i < len(lines):
                m = re.match(r"^\d+\. (.*)$", lines[i]) if ordered else re.match(r"^[-*] (.*)$", lines[i])
                if m:
                    items.append(m.group(1))
                elif lines[i].startswith("   ") and lines[i].strip() and items:
                    items[-1] += " " + lines[i].strip()
                else:
                    break
                i += 1
            tag = "ol" if ordered else "ul"
            attr = f' start="{start}"' if ordered and start != "1" else ""
            body.append(f"<{tag}{attr}>" + "".join(f"<li>{inline(it, source)}</li>" for it in items) + f"</{tag}>")
            continue
        if not line.strip():
            flush()
            i += 1
            continue
        para.append(line)
        i += 1
    flush()
    return title, "\n".join(body), headings


def render(source: str) -> str:
    title, body, headings = convert((ROOT / source).read_text(encoding="utf-8"), source)
    toc = "".join(f'<li><a href="#{html.escape(a)}">{html.escape(t, quote=False)}</a></li>'
                  for level, a, t in headings if level == 2 and t != "Sommaire")
    return (
        "<!DOCTYPE html>\n"
        '<html lang="fr">\n<head>\n<meta charset="utf-8">\n'
        '<meta name="viewport" content="width=device-width, initial-scale=1">\n'
        f"<title>{html.escape(title, quote=False)}</title>\n"
        f"<style>\n{STYLE}\n</style>\n</head>\n<body>\n"
        '<div class="layout">\n'
        f'<nav class="toc" aria-label="Sommaire"><p>Sommaire</p><ol>{toc}</ol></nav>\n'
        f"<main>\n{body}\n"
        f'<p class="top"><a href="#">Haut de page</a> · copie HTML de <code>{html.escape(source)}</code></p>\n'
        "</main>\n</div>\n"
        f"<script>\n{SCRIPT}\n</script>\n</body>\n</html>\n"
    )


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--check", action="store_true", help="exit 1 if an HTML copy is missing or out of date")
    args = ap.parse_args(argv)
    stale = []
    for source, target in GUIDES.items():
        data = render(source).encode("utf-8")
        path = ROOT / target
        if args.check:
            if not path.is_file() or path.read_bytes() != data:
                stale.append(target)
        else:
            path.write_bytes(data)
            print(f"{target}  {len(data)} bytes")
    if stale:
        print("out of date: " + ", ".join(stale) + " (python tools/docs/build_guide_html.py)", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
