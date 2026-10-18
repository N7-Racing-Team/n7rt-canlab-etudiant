// Preview lint (dev only, see boot.ts): the acceptance checks of the UI redesign, measured on the drawn page.
// boot.ts exposes it as window.__previewLint(); shoot.mjs --lint runs it after each shot and writes the report.
//
//   L1 overlap     no intersection larger than 1 px² between two boxes that do not contain each other: buttons,
//                  fields, step boxes, badges, verdict lines, fiche fields, fiche nav items, record heads. What scrolls
//                  under a sticky bar (the progression's action bar, the fiche's head) is covered by design, not
//                  overlapping; a strip that scrolls sideways only (the fiche's index) belongs to the bar around it.
//   L2 truncation  no text wider than its box, nor cut by an ancestor that hides its overflow; no ellipsis except
//                  under [data-lint-ellipsis] (hash or UID values with a title); no input value or placeholder wider
//                  than its field. Scrolling containers do not count: L7 watches the horizontal ones.
//   L3 select fit  the label of the selected option fits the content box of its select.
//   L4 contrast    text at least 4.5:1 (3:1 for large text) against its composited background, disabled controls
//                  included; field borders and step nodes at least 3:1 against the surface around them.
//   L5 primary     at most one enabled .btn--primary per area (progression, fiche); the shown verify or final button
//                  lies inside the viewport and inside the visible part of its side panel.
//   L6 disabled    every visible disabled button has a title, an aria-describedby text, or a visible reason in its
//                  area (#progress-blocked, .fiche-note, .meter-note).
//   L7 h-scroll    no horizontal overflow in the side panel, the progression, the fiche and the multimeter; the window
//                  itself never scrolled (a scrollIntoView, like the e2e captures', would shift the whole application).
//
// Colours are composited along the ancestors (background-color and opacity; background images are ignored), from
// the white canvas up. Pseudo-element text (::before, ::after) is read through getComputedStyle.

export type Rule = "L1" | "L2" | "L3" | "L4" | "L5" | "L6" | "L7";

export interface LintFailure {
  rule: Rule;
  /** A short selector of the element (tag, id, classes; the nearest ancestor id when it has none). */
  target: string;
  /** Its text, shortened. */
  text: string;
  detail: string;
}

export interface LintReport {
  pass: boolean;
  viewport: { width: number; height: number; dpr: number };
  /** Number of items each rule looked at. */
  checked: Record<Rule, number>;
  /** Number of failures per rule (the list below is capped per rule). */
  counts: Record<Rule, number>;
  failures: LintFailure[];
}

const RULES: Rule[] = ["L1", "L2", "L3", "L4", "L5", "L6", "L7"];
const MAX_PER_RULE = 120;

const L1_SELECTOR =
  "button, input:not([type=hidden]), select, textarea, .step, .badge, .feedback-line, .ff, .fiche-nav-item, .fiche-record-head";
/** Areas where at most one primary button may be enabled: [name, root, excluded sub-root]. */
const PRIMARY_AREAS: [string, string, string | null][] = [
  ["progression", "#progress-section", "#progress-fiche"],
  ["fiche", "#progress-fiche", null],
];
/** Visible reasons for a disabled button: [reason, area that it explains]. */
const REASONS: [string, string][] = [
  ["#progress-blocked", "#progress-section"],
  [".fiche-note", "#progress-fiche"],
  [".meter-note", "#phys-meter"],
];
const NO_HSCROLL = [".side-left", "#progress-section", "#progress-fiche", "#phys-meter"];
/** Fields drawn without a border on purpose (row-high editors of a table, whose grid is the boundary). */
const BORDERLESS_FIELDS = ".lab-params .input, .lab-params .select";
/** Graphic marks (no text) that must stand out from their surface, as pseudo-elements: [selector, pseudo]. */
const MARKS: [string, "::before" | "::after"][] = [[".step", "::before"]];
const TEXT_INPUTS = new Set(["", "text", "search", "email", "url", "tel", "password", "number"]);
const SKIP_TEXT_PARENTS = new Set(["SCRIPT", "STYLE", "NOSCRIPT", "TEMPLATE", "TITLE", "OPTION", "OPTGROUP", "SELECT", "TEXTAREA"]);

// ----------------------------------------------------------------------------------------------- geometry

interface Box { l: number; t: number; r: number; b: number }

const box = (r: DOMRect | DOMRectReadOnly): Box => ({ l: r.left, t: r.top, r: r.right, b: r.bottom });
const area = (b: Box): number => Math.max(0, b.r - b.l) * Math.max(0, b.b - b.t);
const meet = (a: Box, b: Box): Box => ({ l: Math.max(a.l, b.l), t: Math.max(a.t, b.t), r: Math.min(a.r, b.r), b: Math.min(a.b, b.b) });
const viewport = (): Box => ({ l: 0, t: 0, r: innerWidth, b: innerHeight });
const px = (v: string): number => parseFloat(v) || 0;

const styles = new Map<Element, CSSStyleDeclaration>();
function css(el: Element): CSSStyleDeclaration {
  let s = styles.get(el);
  if (!s) styles.set(el, (s = getComputedStyle(el)));
  return s;
}

/** The padding box of an element (its scrollbars excluded), in viewport coordinates. */
function paddingBox(el: Element): Box {
  const r = el.getBoundingClientRect();
  const l = r.left + el.clientLeft;
  const t = r.top + el.clientTop;
  return { l, t, r: l + el.clientWidth, b: t + el.clientHeight };
}

const CLIPS = new Set(["hidden", "clip", "auto", "scroll"]);
const HIDES = new Set(["hidden", "clip"]);

/** `b` cut by the ancestors that clip it. `hidesOnly`: only overflow hidden or clip counts (truncation), up to the
 * first container that scrolls in that axis (what lies beyond can be scrolled to); otherwise scrolling containers
 * and the viewport count too (what is on screen). `self`: the element's own overflow counts. */
function clipped(el: Element, b: Box, hidesOnly: boolean, self: boolean): Box {
  let out = { ...b };
  let a: Element | null = self ? el : el.parentElement;
  const kinds = hidesOnly ? HIDES : CLIPS;
  let freeX = false;
  let freeY = false;
  while (a && a !== document.documentElement) {
    const s = css(a);
    const x = !freeX && kinds.has(s.overflowX);
    const y = !freeY && kinds.has(s.overflowY);
    if (hidesOnly) {
      freeX ||= s.overflowX === "auto" || s.overflowX === "scroll";
      freeY ||= s.overflowY === "auto" || s.overflowY === "scroll";
    }
    if (x || y) {
      const p = paddingBox(a);
      if (x) out = { ...out, l: Math.max(out.l, p.l), r: Math.min(out.r, p.r) };
      if (y) out = { ...out, t: Math.max(out.t, p.t), b: Math.min(out.b, p.b) };
    }
    // a fixed element, or an open dialog (top layer), escapes the clipping of its ancestors
    if (s.position === "fixed" || (a instanceof HTMLDialogElement && a.open)) break;
    a = a.parentElement;
  }
  return hidesOnly ? out : meet(out, viewport());
}

/** Displayed at all: a box, visible, not faded out, not a visually hidden text (.sr). */
function rendered(el: Element): boolean {
  if (el.getClientRects().length === 0) return false;
  // the content of a closed <details> keeps its boxes but is not drawn (content-visibility: hidden)
  if (typeof el.checkVisibility === "function" && !el.checkVisibility()) return false;
  const s = css(el);
  if (s.visibility !== "visible") return false;
  if (el.closest(".sr, [data-lint-skip]")) return false;
  let o = 1;
  for (let a: Element | null = el; a; a = a.parentElement) o *= Number(css(a).opacity);
  return o > 0.05;
}

const onScreen = (el: Element): boolean => area(clipped(el, box(el.getBoundingClientRect()), false, false)) > 1;

// ------------------------------------------------------------------------------------------------- colour

type RGB = [number, number, number];
type RGBA = [number, number, number, number];

function parseColor(text: string): RGBA | null {
  const t = text.trim();
  if (t === "transparent") return [0, 0, 0, 0];
  let m = /^rgba?\(([^)]*)\)$/i.exec(t);
  if (m) {
    const n = m[1].split(/[\s,/]+/).filter(Boolean).map((v) => (v.endsWith("%") ? px(v) / 100 : Number(v)));
    if (n.length < 3 || n.some((v) => !Number.isFinite(v))) return null;
    return [n[0], n[1], n[2], n.length > 3 ? n[3] : 1];
  }
  m = /^color\(srgb\s+([^)]*)\)$/i.exec(t);
  if (m) {
    const n = m[1].split(/[\s/]+/).filter(Boolean).map(Number);
    if (n.length < 3 || n.some((v) => !Number.isFinite(v))) return null;
    return [n[0] * 255, n[1] * 255, n[2] * 255, n.length > 3 ? n[3] : 1];
  }
  return null;
}

const over = (top: RGBA, under: RGB): RGB => [0, 1, 2].map((i) => top[i] * top[3] + under[i] * (1 - top[3])) as RGB;
const mix = (a: RGB, b: RGB, k: number): RGB => [0, 1, 2].map((i) => a[i] * k + b[i] * (1 - k)) as RGB;

function luminance(c: RGB): number {
  const [r, g, b] = c.map((v) => {
    const s = v / 255;
    return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

function contrast(a: RGB, b: RGB): number {
  const [x, y] = [luminance(a), luminance(b)].sort((p, q) => q - p);
  return (x + 0.05) / (y + 0.05);
}

const hex = (c: RGB): string => `#${c.map((v) => Math.round(v).toString(16).padStart(2, "0")).join("")}`;

/** The colour of one pixel of `el`: the backgrounds of its ancestors and its own, then `layers` painted by `el`
 * (text, border, pseudo-element), every group faded by its opacity. `ownBg` replaces the element's background. */
function paint(el: Element, layers: RGBA[], ownBg?: RGBA): RGB {
  const chain: Element[] = [];
  for (let a: Element | null = el; a; a = a.parentElement) chain.unshift(a);
  const go = (i: number, backdrop: RGB): RGB => {
    const s = css(chain[i]);
    const last = i === chain.length - 1;
    const bg = last && ownBg ? ownBg : parseColor(s.backgroundColor) ?? [0, 0, 0, 0];
    let x = over(bg, backdrop);
    if (last) for (const l of layers) x = over(l, x);
    else x = go(i + 1, x);
    const o = Number(s.opacity);
    return o < 1 ? mix(x, backdrop, o) : x;
  };
  return go(0, [255, 255, 255]);
}

function large(s: CSSStyleDeclaration): boolean {
  const size = px(s.fontSize);
  return size >= 24 || (size >= 18.66 && Number(s.fontWeight) >= 700);
}

// ----------------------------------------------------------------------------------------------- the lint

let canvas: CanvasRenderingContext2D | null = null;
function textWidth(text: string, s: CSSStyleDeclaration): number {
  canvas ??= document.createElement("canvas").getContext("2d");
  if (!canvas) return 0;
  canvas.font = `${s.fontStyle} ${s.fontWeight} ${s.fontSize} ${s.fontFamily}`;
  const spacing = s.letterSpacing === "normal" ? 0 : px(s.letterSpacing);
  return canvas.measureText(text).width + spacing * text.length;
}

function describe(el: Element): string {
  const id = el.id ? `#${el.id}` : "";
  const classes = typeof el.className === "string" ? el.className.trim().split(/\s+/).filter(Boolean).slice(0, 3) : [];
  const own = `${el.tagName.toLowerCase()}${id}${classes.map((c) => `.${c}`).join("")}`;
  const anchor = id ? null : el.parentElement?.closest("[id]");
  return anchor ? `#${anchor.id} ${own}` : own;
}

const short = (t: string): string => {
  const s = t.replace(/\s+/g, " ").trim();
  return s.length > 60 ? `${s.slice(0, 57)}…` : s;
};

function controlText(el: Element): string {
  if (el instanceof HTMLSelectElement) return el.selectedOptions[0]?.text ?? "";
  if (el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement) return el.value || el.placeholder;
  return el.textContent ?? "";
}

function contentWidth(el: Element): number {
  const s = css(el);
  return el.clientWidth - px(s.paddingLeft) - px(s.paddingRight);
}

export function lint(): LintReport {
  styles.clear();
  const failures: LintFailure[] = [];
  const counts = Object.fromEntries(RULES.map((r) => [r, 0])) as Record<Rule, number>;
  const checked = Object.fromEntries(RULES.map((r) => [r, 0])) as Record<Rule, number>;
  const fail = (rule: Rule, el: Element, detail: string, text = controlText(el)): void => {
    counts[rule]++;
    if (counts[rule] <= MAX_PER_RULE) failures.push({ rule, target: describe(el), text: short(text), detail });
  };
  const body = document.body;

  // ---- text: L2 (truncation) and L4 (contrast), grouped by the element that holds the text
  const holders = new Map<Element, Text[]>();
  const walker = document.createTreeWalker(body, NodeFilter.SHOW_TEXT);
  for (let n = walker.nextNode(); n; n = walker.nextNode()) {
    const t = n as Text;
    const p = t.parentElement;
    if (!p || !t.data.trim() || SKIP_TEXT_PARENTS.has(p.tagName) || p.closest("svg")) continue;
    const list = holders.get(p);
    if (list) list.push(t);
    else holders.set(p, [t]);
  }
  const range = document.createRange();
  for (const [el, texts] of holders) {
    if (!rendered(el)) continue;
    const s = css(el);
    const text = texts.map((t) => t.data).join("");
    // L4
    checked.L4++;
    const fg = parseColor(s.color);
    if (fg && fg[3] > 0) {
      const ratio = contrast(paint(el, [fg]), paint(el, []));
      const need = large(s) ? 3 : 4.5;
      if (ratio < need) fail("L4", el, `texte ${ratio.toFixed(2)}:1 < ${need}:1 (${hex(paint(el, [fg]))} sur ${hex(paint(el, []))})`, text);
    }
    // L2
    if (el.closest("[data-lint-ellipsis]")) continue;
    checked.L2++;
    if (s.textOverflow === "ellipsis" && s.overflowX !== "visible" && el.scrollWidth > el.clientWidth + 1) {
      fail("L2", el, `ellipse : ${el.scrollWidth} px de texte pour ${el.clientWidth} px`, text);
      continue;
    }
    const own = box(el.getBoundingClientRect());
    const cut = clipped(el, own, true, true);
    let worst = 0;
    let why = "";
    for (const t of texts) {
      range.selectNodeContents(t);
      for (const r of Array.from(range.getClientRects())) {
        if (r.width === 0) continue;
        // 1 px more of vertical slack: glyph boxes may exceed a tight line height
        const beyond = Math.max(r.right - cut.r, cut.l - r.left, r.bottom - cut.b - 1, cut.t - r.top - 1, r.right - own.r, own.l - r.left);
        if (beyond > worst) {
          worst = beyond;
          why = r.right - own.r > 1 || own.l - r.left > 1 ? "dépasse sa boîte" : "coupé par un ancêtre";
        }
      }
    }
    if (worst > 1) fail("L2", el, `${why} de ${worst.toFixed(1)} px`, text);
  }

  // ---- pseudo-element text (L4) and graphic marks (L4, 3:1)
  for (const el of Array.from(body.querySelectorAll("*"))) {
    if (el.closest("svg")) continue;
    for (const pseudo of ["::before", "::after"] as const) {
      const ps = getComputedStyle(el, pseudo);
      const content = ps.content;
      if (!content || content === "none" || content === "normal" || ps.display === "none" || /^(url|image-set)\(/.test(content)) continue;
      // the text before the alternative (« "!" / "" »), quotes removed; counter() and attr() count as text
      const literal = content.replace(/\s\/\s.*$/, "").replace(/"([^"]*)"/g, "$1").trim();
      if (!literal || !rendered(el)) continue;
      const fg = parseColor(ps.color);
      if (!fg || fg[3] === 0) continue;
      checked.L4++;
      const layers: RGBA[] = [parseColor(ps.backgroundColor) ?? [0, 0, 0, 0]];
      const ratio = contrast(paint(el, [...layers, fg]), paint(el, layers));
      const need = large(ps) ? 3 : 4.5;
      if (ratio < need) fail("L4", el, `texte ${pseudo} ${ratio.toFixed(2)}:1 < ${need}:1`, literal);
    }
  }
  for (const [selector, pseudo] of MARKS) {
    for (const el of Array.from(body.querySelectorAll(selector))) {
      if (!rendered(el)) continue;
      const ps = getComputedStyle(el, pseudo);
      if (ps.content === "none" || ps.display === "none" || px(ps.width) <= 0) continue;
      checked.L4++;
      const fill = parseColor(ps.backgroundColor) ?? [0, 0, 0, 0];
      const edge = parseColor(ps.borderTopColor) ?? [0, 0, 0, 0];
      const surface = paint(el, []);
      const ratio = Math.max(
        fill[3] > 0 ? contrast(paint(el, [fill]), surface) : 0,
        px(ps.borderTopWidth) > 0 && edge[3] > 0 ? contrast(paint(el, [edge]), surface) : 0,
      );
      if (ratio < 3) fail("L4", el, `repère ${pseudo} ${ratio.toFixed(2)}:1 < 3:1`, el.textContent ?? "");
    }
  }

  // ---- form controls: values (L2, L4), selects (L3), field boundaries (L4)
  for (const el of Array.from(body.querySelectorAll("input, select, textarea"))) {
    if (!rendered(el)) continue;
    const s = css(el);
    const isInput = el instanceof HTMLInputElement;
    if (isInput && !TEXT_INPUTS.has(el.type)) continue;
    const disabled = (el as HTMLInputElement).disabled;
    // value or placeholder contrast
    const placeholder = (isInput || el instanceof HTMLTextAreaElement) && !(el as HTMLInputElement).value;
    const shown = controlText(el);
    if (shown.trim()) {
      const color = parseColor(placeholder ? getComputedStyle(el, "::placeholder").color : s.color);
      if (color) {
        checked.L4++;
        const ratio = contrast(paint(el, [color]), paint(el, []));
        const need = large(s) ? 3 : 4.5;
        if (ratio < need) {
          fail("L4", el, `${placeholder ? "indication" : "valeur"}${disabled ? " (désactivé)" : ""} ${ratio.toFixed(2)}:1 < ${need}:1 (${hex(paint(el, [color]))} sur ${hex(paint(el, []))})`);
        }
      }
    }
    // L3: the selected label in its select
    if (el instanceof HTMLSelectElement) {
      checked.L3++;
      const native = s.appearance !== "none";
      const room = contentWidth(el) - (native ? 20 : 0);
      const need = textWidth(shown, s);
      if (need > room + 1.5) fail("L3", el, `libellé de ${need.toFixed(0)} px pour ${room.toFixed(0)} px`);
    } else if (isInput && shown) {
      // L2: an input value or placeholder wider than the field
      checked.L2++;
      const need = textWidth(shown, placeholder ? getComputedStyle(el, "::placeholder") : s);
      const room = contentWidth(el);
      if (need > room + 1.5) fail("L2", el, `${placeholder ? "indication" : "valeur"} de ${need.toFixed(0)} px pour ${room.toFixed(0)} px`);
    }
    // field boundary against its surroundings (WCAG 1.4.11; a disabled field is exempt)
    if (disabled || el.matches(BORDERLESS_FIELDS) || !el.parentElement) continue;
    checked.L4++;
    const outside = paint(el.parentElement, []);
    const edge = parseColor(s.borderTopColor) ?? [0, 0, 0, 0];
    const ratio = Math.max(
      px(s.borderTopWidth) > 0 ? contrast(paint(el, [], edge), outside) : 0,
      contrast(paint(el, []), outside),
    );
    if (ratio < 3) fail("L4", el, `bordure du champ ${ratio.toFixed(2)}:1 < 3:1 (${s.borderTopColor})`);
  }

  // ---- L1: overlapping boxes
  interface Item { el: Element; scroller: Element | null; sticky: Element | null; local: Box; seen: Box }
  const items: Item[] = [];
  for (const el of Array.from(body.querySelectorAll(L1_SELECTOR))) {
    if (!rendered(el)) continue;
    const own = box(el.getBoundingClientRect());
    const seen = clipped(el, own, false, false);
    if (area(seen) <= 1) continue;
    let local = own;
    let scroller: Element | null = null;
    let sticky: Element | null = css(el).position === "sticky" ? el : null;
    for (let a = el.parentElement; a && a !== document.documentElement; a = a.parentElement) {
      const s = css(a);
      // a one-line strip that scrolls sideways only (the fiche's index on a short window) is not a panel: its items
      // stay in the panel around it (and in its sticky head), clipped by the strip
      if ((s.overflowX === "auto" || s.overflowX === "scroll") && HIDES.has(s.overflowY)) {
        const p = paddingBox(a);
        local = { l: Math.max(local.l, p.l), r: Math.min(local.r, p.r), t: Math.max(local.t, p.t), b: Math.min(local.b, p.b) };
        continue;
      }
      if (s.overflowX === "auto" || s.overflowX === "scroll" || s.overflowY === "auto" || s.overflowY === "scroll") {
        scroller = a;
        break;
      }
      if (!sticky && s.position === "sticky") sticky = a;
      const x = HIDES.has(s.overflowX);
      const y = HIDES.has(s.overflowY);
      if (x || y) {
        const p = paddingBox(a);
        if (x) local = { ...local, l: Math.max(local.l, p.l), r: Math.min(local.r, p.r) };
        if (y) local = { ...local, t: Math.max(local.t, p.t), b: Math.min(local.b, p.b) };
      }
    }
    items.push({ el, scroller, sticky, local, seen });
  }
  checked.L1 = items.length;
  for (let i = 0; i < items.length; i++) {
    for (let j = i + 1; j < items.length; j++) {
      const a = items[i];
      const b = items[j];
      if (a.el.contains(b.el) || b.el.contains(a.el)) continue;
      // a sticky bar covers what scrolls under it in the same panel
      if (a.sticky !== b.sticky && a.scroller === b.scroller) continue;
      const shared = meet(a.scroller === b.scroller ? a.local : a.seen, a.scroller === b.scroller ? b.local : b.seen);
      const overlap = area(shared);
      if (overlap > 1) fail("L1", a.el, `chevauche ${describe(b.el)} « ${short(controlText(b.el))} » sur ${overlap.toFixed(0)} px²`);
    }
  }

  // ---- L5: one primary per area, the verification button in view
  for (const [name, root, without] of PRIMARY_AREAS) {
    const area_ = document.querySelector(root);
    if (!area_ || !rendered(area_)) continue;
    checked.L5++;
    const primaries = Array.from(area_.querySelectorAll<HTMLButtonElement>(".btn--primary")).filter(
      (b) => !b.disabled && rendered(b) && !(without && b.closest(without)),
    );
    if (primaries.length > 1) fail("L5", area_, `${primaries.length} boutons principaux actifs dans la zone ${name} : ${primaries.map(describe).join(", ")}`, "");
  }
  const action = ["#progress-verify", "#progress-final"].map((q) => document.querySelector(q)).find((b) => b && rendered(b));
  if (action) {
    checked.L5++;
    const r = box(action.getBoundingClientRect());
    const side = action.closest(".side");
    const room = side ? meet(paddingBox(side), viewport()) : viewport();
    if (r.l < room.l - 1 || r.t < room.t - 1 || r.r > room.r + 1 || r.b > room.b + 1) {
      fail("L5", action, `hors de la partie visible du panneau (bouton ${r.t.toFixed(0)}–${r.b.toFixed(0)} px, visible ${room.t.toFixed(0)}–${room.b.toFixed(0)} px)`);
    }
  }

  // ---- L6: a visible reason for every visible disabled button
  for (const el of Array.from(body.querySelectorAll<HTMLButtonElement>("button:disabled"))) {
    if (!rendered(el) || !onScreen(el)) continue;
    checked.L6++;
    if (el.title.trim()) continue;
    const described = (el.getAttribute("aria-describedby") ?? "").split(/\s+/).filter(Boolean)
      .map((id) => document.getElementById(id)).some((d) => d && rendered(d) && d.textContent?.trim());
    if (described) continue;
    const explained = REASONS.some(([reason, root]) => {
      const zone = el.closest(root);
      return !!zone && Array.from(zone.querySelectorAll(reason)).some((r) => rendered(r) && r.textContent?.trim());
    });
    if (!explained) fail("L6", el, "désactivé sans raison visible (ni title, ni message)");
  }

  // ---- L7: no horizontal scrolling
  for (const q of NO_HSCROLL) {
    for (const el of Array.from(document.querySelectorAll(q))) {
      if (!rendered(el)) continue;
      checked.L7++;
      if (el.scrollWidth > el.clientWidth + 1) fail("L7", el, `défilement horizontal : ${el.scrollWidth} px pour ${el.clientWidth} px`, "");
    }
  }
  const page = document.scrollingElement;
  if (page) {
    checked.L7++;
    if (page.scrollTop !== 0 || page.scrollLeft !== 0) {
      fail("L7", page, `la fenêtre a défilé (${page.scrollLeft}, ${page.scrollTop}) : un contenu dépasse de l'application`, "");
    }
  }

  styles.clear();
  return {
    pass: failures.length === 0,
    viewport: { width: innerWidth, height: innerHeight, dpr: devicePixelRatio },
    checked,
    counts,
    failures,
  };
}
