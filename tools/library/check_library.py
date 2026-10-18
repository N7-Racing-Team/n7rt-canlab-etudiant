# -*- coding: utf-8 -*-
"""Checks the document library.

Student side (library/): each folder's manifest.json follows shared/schemas/library-manifest.schema.json, is sorted,
gives every document its publisher URL, and lists every PDF present with its SHA-256 (the repositories carry no PDF:
links-only distribution, V-M03; local copies may exist in the maintainer's workspace); no file name or text reveals a
role, a trap or a relevance.
library/links.json (documents cited by link, opened by the application at the publisher's address) follows
shared/schemas/library-links.schema.json, is sorted by title, names only documents of library/README.md « Documents
cités par lien » that are not copied in library/, and carries no banned word.
Instructor side (restricted/library/): every local file of private-manifest.json exists with its SHA-256, no PDF is
unlisted, and every student PDF is in the private manifest.

Usage, from anywhere: python tools/library/check_library.py   (exit 0 = no failure)
Requires: jsonschema (pip install jsonschema).
"""
import hashlib, json, os, re, sys, unicodedata
from pathlib import Path

import jsonschema

ROOT = Path(__file__).resolve().parents[2]
os.chdir(ROOT)
fails, notes = [], []


def sha(p):
    h = hashlib.sha256()
    with open(p, "rb") as f:
        for b in iter(lambda: f.read(1 << 20), b""):
            h.update(b)
    return h.hexdigest()


def fold(s):
    return unicodedata.normalize("NFKD", s).encode("ascii", "ignore").decode().lower()


schema = json.load(open("shared/schemas/library-manifest.schema.json", encoding="utf-8"))
jsonschema.Draft202012Validator.check_schema(schema)
val = jsonschema.Draft202012Validator(schema, format_checker=jsonschema.FormatChecker())

# 1. Public manifests: schema, every PDF listed exactly once, SHA-256, sorted.
public = {}
for folder in sorted(os.listdir("library")):
    d = os.path.join("library", folder)
    if not os.path.isdir(d):
        continue
    man = json.load(open(os.path.join(d, "manifest.json"), encoding="utf-8"))
    for e in val.iter_errors(man):
        fails.append(f"schema {folder}: {e.message[:160]}")
    names = [m["local_file"] for m in man]
    if names != sorted(names):
        fails.append(f"{folder}: entries not sorted by local_file")
    # links-only distribution (V-M03): a listed document may have no local copy, every local file must be listed
    pdfs = sorted(f for f in os.listdir(d) if f != "manifest.json")
    unlisted = sorted(set(pdfs) - set(names))
    if unlisted:
        fails.append(f"{folder}: files not in the manifest {unlisted}")
    for m in man:
        if not m.get("primary_url"):
            fails.append(f"{folder}/{m['local_file']}: no primary_url (the application opens the publisher's address)")
    for m in man:
        p = os.path.join(d, m["local_file"])
        if os.path.exists(p):
            with open(p, "rb") as f:
                if f.read(5) != b"%PDF-":
                    fails.append(f"{p}: no %PDF header")
            if sha(p) != m["sha256"]:
                fails.append(f"{p}: SHA-256 mismatch")
            public[m["sha256"]] = p
    notes.append(f"library/{folder}: {len(man)} entries, {len(pdfs)} local copies")

# 2. No role, relevance or trap wording anywhere student-side (text files and file names).
BANNED = [r"distract", r"leurre", r"piege", r"\btrap", r"\brole", r"required", r"\brequis", r"obligatoire",
          r"\butile", r"useful", r"pertinen", r"reference-only", r"decoy", r"indispensable", r"bon document",
          r"\bcorrect", r"\bmauvais", r"a lire en priorite", r"answer", r"reponse", r"golden", r"mapping", r"seed",
          r"\bdbc\b", r"data id", r"\bsection"]

# 1b. Documents cited by link: schema, order, README list, not a local copy, no banned word in any field.
links_schema = json.load(open("shared/schemas/library-links.schema.json", encoding="utf-8"))
jsonschema.Draft202012Validator.check_schema(links_schema)
links_val = jsonschema.Draft202012Validator(links_schema, format_checker=jsonschema.FormatChecker())
links = json.load(open("library/links.json", encoding="utf-8"))
for e in links_val.iter_errors(links):
    fails.append(f"schema links.json: {e.message[:160]}")
if isinstance(links, list) and all(isinstance(e, dict) for e in links):
    titles = [e.get("title", "") for e in links]
    if titles != sorted(titles, key=lambda t: (fold(t), t)):
        fails.append("links.json: entries not sorted by title")
    readme = open("library/README.md", encoding="utf-8").read()
    cited = set(re.findall(r"<(https?://[^>]+)>", readme.split("## Documents cités par lien", 1)[-1]))
    local = {(m["title"], m["publisher"], m["revision"]) for d in sorted(os.listdir("library"))
             if os.path.isdir(os.path.join("library", d))
             for m in json.load(open(os.path.join("library", d, "manifest.json"), encoding="utf-8"))}
    urls = [e.get("primary_url") for e in links]
    for u in sorted({u for u in urls if urls.count(u) > 1}):
        fails.append(f"links.json: {u} listed twice")
    for e in links:
        what = e.get("title", "?")
        if e.get("primary_url") not in cited:
            fails.append(f"links.json: '{what}' is not cited by link in library/README.md")
        if (e.get("title"), e.get("publisher"), e.get("revision")) in local:
            fails.append(f"links.json: '{what}' has a local copy in library/")
        for k, v in e.items():
            for b in BANNED:
                if isinstance(v, str) and re.search(b, fold(v)):
                    fails.append(f"links.json: banned '{b}' in {k} of '{what}'")
    per = {m: sum(m in e.get("missions", []) for e in links) for m in ("APPS", "WSF", "SAS", "BPS")}
    notes.append(f"library/links.json: {len(links)} documents cited by link {per}")
for dp, _, fs in os.walk("library"):
    for f in fs:
        p = os.path.join(dp, f)
        for b in BANNED:
            if re.search(b, fold(f)):
                fails.append(f"banned '{b}' in file name {p}")
        if f.endswith((".md", ".json", ".txt", ".csv", ".html")):
            for i, line in enumerate(open(p, encoding="utf-8"), 1):
                for b in BANNED:
                    if re.search(b, fold(line)):
                        notes.append(f"WORD '{b}' {p}:{i}: {line.strip()[:150]}")

# 3. Private manifest: every local file exists with its SHA-256; student entries match the public copy.
#    Skipped in the student repository, which has no restricted/ folder (docs/distribution.md §1).
if not os.path.exists("restricted/library/private-manifest.json"):
    notes.append("private manifest: absent (student repository), instructor side not checked")
else:
    pm = json.load(open("restricted/library/private-manifest.json", encoding="utf-8"))
    avail = {}
    for d in pm["documents"]:
        avail[d["availability"]] = avail.get(d["availability"], 0) + 1
        lf = d.get("local_file")
        if not lf:
            if d["availability"] != "no-local-file":
                fails.append(f"{d['id']}: no local_file but availability {d['availability']}")
            continue
        if not os.path.exists(lf):
            fails.append(f"{d['id']}: missing {lf}")
            continue
        if sha(lf) != d["sha256"]:
            fails.append(f"{d['id']}: SHA-256 mismatch {lf}")
        if d["availability"] == "library" and not lf.startswith("library/"):
            fails.append(f"{d['id']}: availability library but file {lf}")
        if d["availability"] != "library" and lf.startswith("library/"):
            fails.append(f"{d['id']}: {d['availability']} file inside library/")
    listed = {d["local_file"] for d in pm["documents"] if d.get("local_file")}
    for dp, _, fs in os.walk("restricted/library"):
        for f in fs:
            p = Path(dp, f).as_posix()
            if f.endswith(".pdf") and p not in listed:
                fails.append(f"unlisted private file {p}")
    for s, p in public.items():
        if Path(p).as_posix() not in listed:
            fails.append(f"public file {p} absent from private manifest")
    notes.append(f"private manifest: {len(pm['documents'])} documents {avail}")

for n in notes:
    print(n)
print("FAILS:", len(fails))
for f in fails:
    print("  FAIL", f)
sys.exit(1 if fails else 0)
