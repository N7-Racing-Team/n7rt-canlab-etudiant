// N7RT CAN Lab 0.3 — shared template of the PDF documents: the student tutorial (public) and the four instructor
// guides (private repository, which imports this file). Build: tools/docs/build_pdf.py.
//
// Inputs (typst --input): `manifest` = root-relative path of the figures manifest written by tools/docs/pdf_figures.py
// (figure id -> image path, marks); `data` (instructor guides) = root-relative path of the mission's answer data.
// A figure, a badge or a data path that does not exist stops the build: nothing is placed or typed by hand.

// N7RT art direction (the team's documents, e.g. « Checklist roulage N7RT »): navy, white and pale grey, black text,
// Computer Modern. Red stays for what must be seen on the captures (marks, badges) and for the confidential marking.
#let navy = rgb("#182B49")
#let paper = rgb("#F5F7FA")
#let red = rgb("#d42728")
#let ink = rgb("#000000")
#let muted = rgb("#5B6678")
#let rule = rgb("#CBD2DD")
#let tint = paper
#let serif = ("New Computer Modern",)
#let mono = ("Consolas", "DejaVu Sans Mono")

#let _manifest = if "manifest" in sys.inputs { json(sys.inputs.manifest) } else { (:) }
#let _data = if "data" in sys.inputs { json(sys.inputs.data) } else { (:) }

// Inline badge: the same red disc and white number as the marks drawn on the captures. The box is only as high as a
// capital letter and the disc is placed over it, so that a line carrying a badge keeps the spacing of the others.
#let b(n) = box(width: 1.24em, height: 0.7em, place(center + horizon, circle(radius: 0.6em, fill: red,
  stroke: 0.6pt + white, inset: 0pt, align(center + horizon,
    text(fill: white, weight: "bold", size: 0.72em, font: ("Segoe UI", "Arial"), str(n))))))

// ------------------------------------------------------------------------------------------- callouts
// the rounded pale-grey box with a navy border of the N7RT documents; « Attention » adds a thick bar on the left
#let callout(accent, label, body, bar: false) = block(width: 100%, inset: (x: 9pt, y: 7pt),
  stroke: if bar { (left: 3pt + accent, rest: 0.7pt + accent) } else { 0.7pt + accent },
  fill: paper, radius: 4pt, breakable: true, above: 0.9em, below: 0.9em)[#text(weight: "bold", fill: accent, label) #h(0.4em) #body]
#let note(body) = callout(navy, "À savoir", body)
#let warn(body) = callout(navy, "Attention", body, bar: true)
#let tip(body) = callout(navy, "Astuce", body)
#let secret(body) = callout(red, "Encadrant", body, bar: true)
#let hint(level, body) = callout(navy, "Indice " + str(level), body)

// ------------------------------------------------------------------------------------------- figures
#let LEGEND = (
  ("etapes", "Étapes"), ("regarder", "Où regarder"), ("cliquer", "Où cliquer"), ("entrer", "Quoi entrer"),
  ("attendu", "Valeur attendue"), ("pourquoi", "Pourquoi"), ("observer", "Ce qu’on doit observer"),
  ("erreur", "Erreur classique"), ("echec", "Si cela échoue"),
)
// A capture: the image and its caption on one page, kept with the first row of its legend (in this fixed order); a
// legend row is never split by a page break.
#let fig(id, caption: none, width: 100%, ..fields) = {
  let f = _manifest.at(id, default: none)
  if f == none { panic("figure absente du manifeste : " + id) }
  let named = fields.named()
  for k in named.keys() {
    if LEGEND.find(((key, t)) => key == k) == none { panic("rubrique inconnue « " + k + " » (figure " + id + ")") }
  }
  let rows = LEGEND.filter(((key, t)) => key in named).map(((key, t)) => (t, named.at(key)))
  block(breakable: false, sticky: rows.len() > 0, width: 100%, above: 1.1em, below: 0.5em)[
    #figure(image(f.path, width: width), caption: caption, kind: image, supplement: [Figure])
    #label("fig-" + id)
    // for the control table of the build (typst query "<figmeta>"): where each figure landed
    #context [#metadata((id: id, page: here().page(), capture: f.at("capture", default: none), marks: f.at("marks", default: ()))) <figmeta>]
  ]
  if rows.len() > 0 {
    set table.cell(breakable: false)
    // N7RT table: navy rules above and below, thin grey rules between the rows, no vertical line
    table(columns: (3.3cm, 1fr), inset: (x: 6pt, y: 4.2pt),
      stroke: (x, y) => (top: if y == 0 { 0.8pt + navy } else { 0.3pt + rule },
        bottom: if y == rows.len() - 1 { 0.8pt + navy } else { none }),
      fill: (x, y) => if x == 0 { paper },
      ..rows.map(((t, v)) => (text(weight: "bold", size: 0.9em, fill: navy, hyphenate: false, t), v)).flatten())
  }
}
#let figref(id) = ref(label("fig-" + id))

// ------------------------------------------------------------------------------------------- code
// Lines `a`..`b` of a real source file (root-relative path), with the file's own line numbers; `mark` = (first,
// last) lines tinted in red: the box drawn on the capture of the integrated IDE.
#let _numbered(src, first, lang, mark, size) = {
  show raw.line: it => {
    let n = it.number + first - 1
    let hot = mark != none and n >= mark.at(0) and n <= mark.at(1)
    let num = box(width: 2.7em, align(right, text(fill: if hot { red } else { muted }, str(n))))
    let line = num + h(0.7em) + it.body
    if hot { box(fill: rgb("#fde6e6"), outset: (y: 1.3pt), width: 100%, line) } else { line }
  }
  set text(font: mono, size: size)
  // a short listing stays on one page; a long one (a whole file) flows over several
  block(fill: paper, inset: 6pt, radius: 3pt, width: 100%, stroke: 0.5pt + rule,
    breakable: src.split("\n").len() > 30, raw(src, lang: lang, block: true))
}
#let src(path, a, b, mark: none, lang: "c", size: 7.4pt) = {
  let lines = read(path).replace("\r\n", "\n").split("\n")
  if a < 1 or b > lines.len() or a > b { panic("lignes " + str(a) + "-" + str(b) + " hors de " + path) }
  _numbered(lines.slice(a - 1, b).join("\n"), a, lang, mark, size)
}
#let code(body, first: 1, lang: "c", mark: none, size: 7.4pt) = _numbered(body, first, lang, mark, size)
// Pedagogical pseudocode beside the real code.
#let pseudo(body) = block(fill: rgb("#EDF1F7"), inset: 7pt, radius: 3pt, width: 100%, stroke: 0.5pt + rgb("#B9C4D6"),
  breakable: true)[#set text(font: mono, size: 7.6pt); #body]
#let parallel(left, right) = grid(columns: (1.5fr, 1fr), column-gutter: 8pt, left, right)
// Break points after « _ » and « / » in long identifiers (PROTECTION_NON_CONFORME, MESSAGE/E2E) so that they wrap
// inside a table cell instead of running over the next one.
#let brk(s) = if type(s) == str { s.replace("_", "_\u{200B}").replace("/", "/\u{200B}").replace(".", ".\u{200B}") } else { s }

// ------------------------------------------------------------------------------------------- data
// A value of the instructor guide's answer data (dotted path, indices for lists): a missing path stops the build.
#let at(path) = {
  let v = _data
  for k in path.split(".") {
    if type(v) == dictionary and k in v { v = v.at(k) }
    else if type(v) == array and k.match(regex("^\d+$")) != none and int(k) < v.len() { v = v.at(int(k)) }
    else { panic("valeur absente des données : " + path) }
  }
  v
}
#let fmt(v, unit: none, digits: none) = {
  let s = if type(v) == float and digits != none { str(calc.round(v, digits: digits)) } else if type(v) == str { v } else { repr(v) }
  s = s.replace(".", ",")
  if unit != none { s + " " + unit } else { s }
}
#let val(path, unit: none, digits: none) = fmt(at(path), unit: unit, digits: digits)
#let hex(path) = { let v = at(path); if type(v) == int { "0x" + upper(str(v, base: 16)) } else { str(v) } }

// ------------------------------------------------------------------------------------------- tables
// as in the N7RT documents: a navy rule above and under the header (repeated on each page) and at the end, thin grey
// rules between the rows, no vertical line
#let tableau(columns: auto, header: (), ..cells) = table(columns: columns, inset: (x: 5pt, y: 4pt),
  stroke: (x, y) => (top: if y > 1 { 0.3pt + rule } else { none }),
  table.header(table.hline(stroke: 0.9pt + navy), ..header.map(h => text(weight: "bold", size: 0.92em, h)),
    table.hline(stroke: 0.6pt + navy)),
  ..cells, table.hline(stroke: 0.9pt + navy))

// ------------------------------------------------------------------------------------------- document
// N7RT identity: the cover takes the banner of the team's documents (navy, rounded, white capitals, « Projet » and
// « Pôle » line, tricolour stripe of the badge) and their document card; the official badge (n7rt-logo.png, from the
// team Drive) is placed on the cover only, so that the images counted by tools/docs/check_pdf.py stay the figures.
#let doc(title: "", subtitle: "", audience: "", confidential: false, version: "0.3.0", date: "", provenance: none, body) = {
  set document(title: title, author: "N7 Racing Team — Pôle DATA — CAN Lab")
  set text(font: serif, size: 10pt, lang: "fr", fill: ink, hyphenate: true)
  set par(justify: true, leading: 0.62em, spacing: 0.9em)
  set heading(numbering: "1.1")
  // headings in navy; a chapter is numbered « 1. » and underlined by a navy rule over the whole width
  show heading.where(level: 1): it => {
    pagebreak(weak: true)
    v(0.2em)
    block(width: 100%, sticky: true, below: 0.9em, {
      text(17pt, weight: "bold", fill: navy, if it.numbering != none {
        [#context counter(heading).display("1.")#h(0.8em)#it.body]
      } else { it.body })
      v(-0.5em)
      line(length: 100%, stroke: 0.7pt + navy)
    })
  }
  show heading.where(level: 2): it => block(sticky: true, above: 1.15em, below: 0.65em,
    text(12.5pt, weight: "bold", fill: navy, it))
  show heading.where(level: 3): it => block(sticky: true, above: 0.95em, below: 0.55em,
    text(10.8pt, weight: "bold", fill: navy, it))
  show raw.where(block: false): it => box(fill: rgb("#E9EDF3"), inset: (x: 2.5pt), outset: (y: 2pt), radius: 2pt,
    text(font: mono, size: 0.92em, it))
  show link: it => text(fill: rgb("#2A5599"), it)
  // an interface label quoted « … » is never hyphenated (« Fla-sher » would no longer read as the button's label)
  show regex("«[^«»]{1,60}»"): it => text(hyphenate: false, it)
  set figure(gap: 0.5em)
  show figure.caption: it => text(size: 0.9em, fill: muted, it)
  // tables: left-aligned text (no justification gaps in narrow cells), slightly smaller
  show table: set par(justify: false)
  show table: set text(size: 9pt)
  set list(indent: 0.6em)
  set enum(indent: 0.6em)

  let mark = if confidential { "CONFIDENTIEL — ENCADRANT — NE PAS DIFFUSER AUX ÉTUDIANTS" } else { audience }
  set page(paper: "a4", margin: (x: 17mm, top: 21mm, bottom: 19mm),
    header: context {
      if counter(page).get().first() > 1 {
        set text(7.6pt, fill: muted, hyphenate: false)
        grid(columns: (1fr, auto),
          [#text(fill: navy, weight: "bold", tracking: 0.14em, "N7 RACING TEAM")#h(0.6em)·#h(0.6em)#title],
          if confidential { text(fill: red, weight: "bold", mark) } else { mark })
        v(-0.35em); line(length: 100%, stroke: 0.5pt + navy)
      }
    },
    footer: context {
      set text(7.6pt, fill: muted)
      line(length: 100%, stroke: 0.3pt + rule)
      v(-0.3em)
      grid(columns: (1fr, auto, 1fr), [N7RT CAN Lab #version · Pôle DATA · FS 2027 · #date],
        [Page #counter(page).display() / #counter(page).final().first()],
        align(right)[#if confidential { text(fill: red, "Confidentiel encadrant") } else { "Document étudiant" }])
    })

  // cover: the N7RT banner
  v(2mm)
  block(width: 100%, fill: navy, radius: 9pt, inset: (x: 14pt, top: 12pt, bottom: 10pt), {
    set text(fill: white)
    set par(justify: false)
    grid(columns: (22mm, 1fr), column-gutter: 10pt, align: horizon,
      image("n7rt-logo.png", width: 22mm),
      align(center, {
        text(8.5pt, tracking: 0.32em, "N7 RACING TEAM")
        v(0.1em)
        text(22pt, weight: "bold", upper(title))
        v(0.05em)
        text(10pt, tracking: 0.12em, "N7RT CAN LAB " + version)
      }))
    v(0.55em)
    grid(columns: (1fr, 1fr),
      text(9.5pt)[#text(fill: rgb("#C3CCDB"))[Projet :] #text(weight: "bold")[Formula Student 2027]],
      align(right, text(9.5pt)[#text(fill: rgb("#C3CCDB"))[Pôle :] #text(weight: "bold")[DATA — CAN Lab]]))
    v(0.4em)
    // the tricolour stripe of the team badge
    align(center, stack(dir: ltr, ..(rgb("#2D5BD3"), white, rgb("#E2262D")).map(c => box(width: 9mm, height: 2pt, fill: c))))
  })
  v(12mm)
  text(13pt, fill: navy, subtitle)
  v(9mm)
  if confidential {
    block(fill: rgb("#fde8e8"), inset: 10pt, radius: 6pt, width: 100%, stroke: 1pt + red)[
      #text(fill: red, weight: "bold", size: 11pt)[CONFIDENTIEL — corrigé encadrant] \
      Ce document contient les réponses exactes du pack de référence V0, toutes les variantes d'incident et le décodeur de
      référence. Il ne doit jamais être remis, projeté ni photographié devant des étudiants, ni copié dans l'archive étudiante.]
  } else {
    block(fill: paper, inset: 10pt, radius: 6pt, width: 100%, stroke: 0.7pt + navy)[#text(weight: "bold", fill: navy)[Public visé :] #audience]
  }
  v(1fr)
  // the document card of the N7RT documents
  block(width: 100%, fill: paper, radius: 6pt, stroke: 0.7pt + navy, inset: 10pt, {
    set text(8.6pt)
    set par(justify: false)
    grid(columns: (auto, 1fr), column-gutter: 12pt, row-gutter: 0.6em,
      text(fill: navy, weight: "bold", "Équipe"), [N7 Racing Team · ENSEEIHT, Toulouse],
      text(fill: navy, weight: "bold", "Version"), [N7RT CAN Lab #version · #date],
      text(fill: navy, weight: "bold", "Statut"),
      if confidential { text(fill: red, weight: "bold", "Confidentiel encadrant") } else { [Document étudiant] })
    if provenance != none {
      v(0.3em)
      line(length: 100%, stroke: 0.3pt + rule)
      v(0.1em)
      text(7.8pt, fill: muted, provenance)
    }
  })
  pagebreak()
  outline(title: "Sommaire", depth: 2, indent: auto)
  pagebreak()
  body
}
