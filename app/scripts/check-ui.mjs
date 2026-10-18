// UI checks, run by `npm run check` and by the project test suite (tools/stm32/tests/test_n7rt_app.py).
//
// Safety: no HTML parsing sinks for dynamic data (innerHTML, outerHTML, insertAdjacentHTML,
// document.write, DOMParser, createContextualFragment), no eval / new Function / string timers,
// no inline <script>, style or event handler in index.html (the CSP forbids them anyway).
//
// Design system (ui/DESIGN.md): square geometry — no non-zero border-radius anywhere (CSS, SVG
// rx/ry, TypeScript), the global radius guard present; no shadows; no keyframe animation;
// transitions of 150 ms at most. Canvas2D (Physical Lab schematic): no canvas shadow, no rounded shape
// (roundRect, arc, arcTo, ellipse: junctions are squares), no round line joins or caps, no animated dash, no
// filter; in CSS no blur or drop-shadow filter.
//
// Design-system classes and tokens (ui/DESIGN.md §2, §5), against white native controls and stray colours:
// - in ui/src, a control created with el("button" | "input" | "select" | "textarea", <literal class>) carries its
//   design-system class (.btn, .input, .select, .textarea, or another component class listed in CONTROL_CLASSES);
//   a class computed at run time is not checked; a button, select or textarea created without any class is refused;
// - in ui/styles/*.css, every var(--x) is declared in one of these files, no raw #hex colour outside tokens.css
//   (data URIs aside), and base.css keeps `color-scheme: dark` (the WebView draws native controls dark).
import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import { fileURLToPath } from "node:url";

// Optional argument: another application root (used by the tests to prove the check fails).
const root = process.argv[2] ?? join(fileURLToPath(import.meta.url), "..", "..");
const ui = join(root, "ui");

const SINKS = [
  /\.innerHTML\b/, /\.outerHTML\b/, /insertAdjacentHTML/, /document\.write/, /\bDOMParser\b/,
  /createContextualFragment/, /\beval\s*\(/, /new\s+Function\s*\(/, /setTimeout\s*\(\s*["'`]/, /setInterval\s*\(\s*["'`]/,
  /borderRadius|border-radius|boxShadow|box-shadow/,
];

// Canvas2D constructs forbidden by DESIGN.md §7, with the reason printed on failure.
const CANVAS = [
  [/\.shadow(Blur|Color|Offset[XY])\b/, "ombre Canvas interdite"],
  [/\broundRect\s*\(/, "coins arrondis interdits (roundRect)"],
  [/\.(arc|arcTo|ellipse)\s*\(/, "cercles et arcs interdits sur le schéma"],
  [/line(Join|Cap)\s*=\s*['"`]round/, "jointures arrondies interdites"],
  [/\blineDashOffset\b/, "animation de tirets interdite"],
  [/\.filter\s*=(?!=)/, "filtre Canvas interdit"],
];

function files(dir, exts) {
  let names;
  try {
    names = readdirSync(dir);
  } catch {
    return [];
  }
  return names.flatMap((name) => {
    const p = join(dir, name);
    return statSync(p).isDirectory() ? files(p, exts) : exts.some((e) => p.endsWith(e)) ? [p] : [];
  });
}

// Design-system classes accepted per control: the generic component first, then component classes that style the
// control entirely (list rows drawn as buttons, the bench library, the range slider).
const CONTROL_CLASSES = {
  button: ["btn", "tab", "lab-item", "lib-doc", "ide-outline-item", "ide-problem"],
  input: ["input", "range"],
  select: ["select"],
  textarea: ["textarea"],
};
// el("tag") or el("tag", <class>): the class as a "…", '…' or `…` literal, or any other expression.
const EL_CALL = /\bel\(\s*["'](button|input|select|textarea)["']\s*(?:\)|,\s*(?:"([^"]*)"|'([^']*)'|`([^`]*)`|([^,)]+)))/g;

const problems = [];
const where = (f, text, index) => `${relative(root, f)}:${text.slice(0, index).split("\n").length}`;
/** Comments blanked out, line breaks kept (so that offsets give the right line). */
const blankJsComments = (text) =>
  text.replace(/\/\*[\s\S]*?\*\//g, (c) => c.replace(/[^\n]/g, " ")).replace(/^\s*\/\/.*$/gm, (c) => c.replace(/./g, " "));

for (const f of files(join(ui, "src"), [".ts", ".js"])) {
  const text = readFileSync(f, "utf8");
  text.split(/\r?\n/).forEach((line, i) => {
    if (line.trim().startsWith("//") || line.trim().startsWith("*")) return;
    for (const re of SINKS) if (re.test(line)) problems.push(`${relative(root, f)}:${i + 1}: ${line.trim()}`);
    for (const [re, why] of CANVAS) if (re.test(line)) problems.push(`${relative(root, f)}:${i + 1}: ${line.trim()} (${why})`);
  });
  if (f.endsWith(".test.ts")) continue;
  const code = blankJsComments(text);
  for (const m of code.matchAll(EL_CALL)) {
    const [call, tag, dq, sq, tpl, other] = m;
    if (other !== undefined) continue; // computed at run time: not checked
    const literal = dq ?? sq ?? tpl;
    const accepted = CONTROL_CLASSES[tag];
    if (literal === undefined) {
      // a checkbox sits in a label.check, which styles it: an input may come without a class
      if (tag !== "input") problems.push(`${where(f, code, m.index)}: ${call.trim()} (contrôle sans classe : ajoutez .${accepted[0]})`);
      continue;
    }
    const tokens = literal.replace(/\$\{[^}]*\}/g, " ").split(/\s+/).filter(Boolean);
    if (!tokens.some((t) => accepted.includes(t))) {
      problems.push(`${where(f, code, m.index)}: ${call.trim()}…) (classe du système de design absente : .${accepted.join(", .")})`);
    }
  }
}

const ms = (v) => [...v.matchAll(/(\d*\.?\d+)(ms|s)\b/g)].map(([, n, u]) => Number(n) * (u === "s" ? 1000 : 1));
const cssFiles = files(ui, [".css"]);
let guard = false;
for (const f of cssFiles) {
  const css = readFileSync(f, "utf8").replace(/\/\*[\s\S]*?\*\//g, (c) => c.replace(/[^\n]/g, " "));
  for (const m of css.matchAll(/(?:^|[;{\s])(border(?:-[a-z]+)*-radius)\s*:\s*([^;}]+)/gi)) {
    const v = m[2].trim();
    if (/^0(px|%)?(\s*!important)?$/i.test(v)) {
      if (/!important/.test(v)) guard = true;
    } else problems.push(`${where(f, css, m.index)}: ${m[1]}: ${v} (coins arrondis interdits)`);
  }
  for (const m of css.matchAll(/(?:^|[;{\s])((?:box|text)-shadow)\s*:\s*([^;}]+)/gi)) {
    if (!/^none(\s*!important)?$/i.test(m[2].trim())) problems.push(`${where(f, css, m.index)}: ${m[1]}: ${m[2].trim()} (ombres interdites)`);
  }
  for (const m of css.matchAll(/(?:^|[;{\s])((?:-webkit-)?(?:backdrop-)?filter)\s*:\s*([^;}]*(?:blur|drop-shadow)[^;}]*)/gi)) {
    problems.push(`${where(f, css, m.index)}: ${m[1]}: ${m[2].trim()} (flou et ombre portée interdits)`);
  }
  for (const m of css.matchAll(/@keyframes|(?:^|[;{\s])animation(?:-name)?\s*:\s*([^;}]+)/gi)) {
    if (!m[1] || !/^none\s*(?:!important)?$/i.test(m[1].trim())) problems.push(`${where(f, css, m.index)}: animation interdite`);
  }
  for (const m of css.matchAll(/(?:^|[;{\s])transition(?:-duration)?\s*:\s*([^;}]+)/gi)) {
    if (ms(m[1]).some((d) => d > 150)) problems.push(`${where(f, css, m.index)}: transition > 150 ms`);
  }
  for (const m of css.matchAll(/--t-[a-z]+\s*:\s*([^;}]+)/gi)) {
    if (ms(m[1]).some((d) => d > 150)) problems.push(`${where(f, css, m.index)}: jeton de durée > 150 ms`);
  }
}
if (cssFiles.length && !guard) problems.push("ui/styles: règle de garde globale `border-radius: 0 !important` absente");

// Tokens: the stylesheets of ui/styles only (the design system), comments and url(…) contents blanked out.
const stylesDir = join(ui, "styles");
if (existsSync(stylesDir)) {
  const sheets = readdirSync(stylesDir).filter((n) => n.endsWith(".css")).map((n) => {
    const f = join(stylesDir, n);
    const css = readFileSync(f, "utf8")
      .replace(/\/\*[\s\S]*?\*\//g, (c) => c.replace(/[^\n]/g, " "))
      .replace(/url\(\s*(["']?)[\s\S]*?\1\s*\)/g, (c) => c.replace(/[^\n]/g, " "));
    return { f, name: n, css };
  });
  const declared = new Set(sheets.flatMap(({ css }) => [...css.matchAll(/(--[\w-]+)\s*:/g)].map((m) => m[1])));
  for (const { f, name, css } of sheets) {
    for (const m of css.matchAll(/var\(\s*(--[\w-]+)/g)) {
      if (!declared.has(m[1])) problems.push(`${where(f, css, m.index)}: var(${m[1]}) (variable CSS non déclarée dans ui/styles)`);
    }
    if (name === "tokens.css") continue;
    for (const m of css.matchAll(/#(?:[0-9a-f]{8}|[0-9a-f]{6}|[0-9a-f]{3,4})(?![\w-])/gi)) {
      // a value, not an id selector: after the last { ; or } comes a property name and its colon
      const before = css.slice(0, m.index);
      const start = Math.max(before.lastIndexOf("{"), before.lastIndexOf(";"), before.lastIndexOf("}"));
      if (start >= 0 && before[start] !== "}" && before.slice(start).includes(":")) {
        problems.push(`${where(f, css, m.index)}: ${m[0]} (couleur brute hors de tokens.css : utilisez un jeton)`);
      }
    }
  }
  const base = sheets.find((s) => s.name === "base.css");
  if (!base || !/(?:^|[;{\s])color-scheme\s*:\s*dark\b/.test(base.css)) {
    problems.push("ui/styles/base.css: `color-scheme: dark` absent (les contrôles natifs seraient dessinés en clair)");
  }
}

const html = readFileSync(join(ui, "index.html"), "utf8");
if (/<script(?![^>]*\bsrc=)[^>]*>/i.test(html)) problems.push("ui/index.html: inline <script>");
if (/<style\b/i.test(html) || /\sstyle\s*=/i.test(html)) problems.push("ui/index.html: inline style");
if (/\son[a-z]+\s*=/i.test(html)) problems.push("ui/index.html: inline event handler");
for (const m of html.matchAll(/\s(rx|ry)\s*=\s*"([^"]*)"/gi)) {
  if (Number(m[2]) !== 0) problems.push(`ui/index.html: SVG ${m[1]}="${m[2]}" (coins arrondis interdits)`);
}

if (problems.length) {
  console.error("check-ui: forbidden constructs\n" + problems.join("\n"));
  process.exit(1);
}
console.log(`check-ui: OK (${files(join(ui, "src"), [".ts"]).length} TypeScript files, ${cssFiles.length} CSS files, index.html; square geometry guard present; control classes, tokens and dark color-scheme checked)`);
