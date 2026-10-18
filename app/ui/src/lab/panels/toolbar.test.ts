import { describe, expect, it } from "vitest";
import indexHtml from "../../../index.html?raw";

// Contract §8.4: « Simulation : RUN / STOP / RESET » in words, zoom (−, %, +, ajuster), at every workspace width.
// The layout itself is measured in a browser; here no style rule may take one of these controls away.

// Vitest turns CSS imports (?raw included) into empty strings: the file is read from disk.
const NODE_FS = "node:fs";
const fs = (await import(/* @vite-ignore */ NODE_FS)) as { readFileSync(p: URL, enc: "utf8"): string };
const labCss = fs.readFileSync(new URL("../../../styles/lab.css", import.meta.url), "utf8");
const css = labCss.replace(/\/\*[\s\S]*?\*\//g, "");

/** Innermost rules (inside @container and @media too): selectors and declarations. */
function rules(src: string): { selectors: string[]; body: string }[] {
  return [...src.matchAll(/([^{}]+)\{([^{}]*)\}/g)].map((m) => ({
    selectors: m[1].split(",").map((s) => s.trim().replace(/^@[^{]*$/, "")),
    body: m[2],
  }));
}

/** The element whose opening tag carries this id. */
function tag(id: string): string {
  const m = indexHtml.match(new RegExp(`<[a-z]+ id="${id}"[^>]*>`));
  if (!m) throw new Error(id);
  return m[0];
}

describe("workspace toolbar", () => {
  it("never hides a control of the bar: no display: none on the bar, its labels or its buttons", () => {
    const hiding = rules(css).filter((r) => /display\s*:\s*none/.test(r.body)).flatMap((r) => r.selectors);
    const bar = ["lab-bar", "tool-label", "tool-group", "lab-zoom", "lab-time", "#phys-fit", "#phys-zoom", "#phys-run",
      "#phys-stop", "#phys-reset", "#phys-time", "#phys-undo", "#phys-redo", "#phys-tool"];
    for (const s of hiding) for (const b of bar) expect(s.includes(b), `${s} hides ${b}`).toBe(false);
  });

  it("wraps whole groups onto a second row instead of clipping them on a narrow workspace", () => {
    const bar = rules(css).find((r) => r.selectors.includes(".lab-bar"));
    expect(bar?.body).toMatch(/flex-wrap\s*:\s*wrap/);
    expect(bar?.body).not.toMatch(/(^|[\s;])height\s*:/);
  });

  it("says « Simulation : » in words before RUN, STOP and RESET, and keeps a labelled fit button", () => {
    const group = indexHtml.match(/<span class="tool-label">Simulation&nbsp;:<\/span>([\s\S]*?)<\/div>/);
    expect(group).not.toBeNull();
    const words = [...(group?.[1] ?? "").matchAll(/<button id="phys-(run|stop|reset)"[^>]*>([^<]*)<\/button>/g)].map((m) => m[2]);
    expect(words).toEqual(["RUN", "STOP", "RESET"]);
    const fit = tag("phys-fit");
    expect(fit).toContain('aria-label="Ajuster"');
    expect(fit).toContain("(F)");
    const icon = indexHtml.match(/id="phys-fit"[^>]*><svg class="icon"><use href="#([a-z-]+)"\/><\/svg><\/button>/);
    expect(icon).not.toBeNull();
    expect(indexHtml).toContain(`<symbol id="${icon?.[1]}"`);
  });

  it("shows « ralenti » in the message line, outside the bar's groups: it never changes the rows of the bar", () => {
    const bar = indexHtml.indexOf('<div class="lab-bar"');
    const row = indexHtml.indexOf('<div class="message" id="phys-message-row">');
    const slow = indexHtml.indexOf('<span id="phys-slow"');
    expect(bar).toBeGreaterThan(-1);
    expect(row).toBeGreaterThan(bar);
    expect(slow).toBeGreaterThan(row);
    expect(indexHtml.indexOf("</div>", row)).toBeGreaterThan(slow);
  });
});
