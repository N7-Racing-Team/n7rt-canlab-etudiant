// Display formatting (contract docs/physical-lab-v1.md §4) and glyph coverage of the embedded fonts (ui/DESIGN.md §3).

import { describe, expect, it } from "vitest";
import type { Reading } from "./types";
import {
  EM_DASH, FORMATTER_GLYPHS, INFINITY_SIGN, KNOWN_UNITS, MICRO, MINUS, NBSP, exactText, fieldText, formatNumber,
  formatValue, fullText, rangeText, readingView, simTimeText, zoomText,
} from "./units";

const S = NBSP;

describe("parameters: six significant digits, trailing zeros removed", () => {
  const cases: [number, string, string][] = [
    [1e4, "Ω", `10${S}kΩ`],
    [4.7e-6, "F", `4.7${S}${MICRO}F`],
    [1e-5, "F", `10${S}${MICRO}F`],
    [0.1, "A", `100${S}mA`],
    [0.01, "Ω", `10${S}mΩ`],
    [5, "V", `5${S}V`],
    [3.3, "V", `3.3${S}V`],
    [0.25, "W", `250${S}mW`],
    [0.0625, "W", `62.5${S}mW`],
    [120, "Ω", `120${S}Ω`],
    [2.5e4, "Ω", `25${S}kΩ`],
    [1e8, "Ω", `100${S}MΩ`],
    [1e9, "Ω", `1${S}GΩ`],
    [1e12, "Ω", `1${S}TΩ`],
    [1e-12, "F", `1${S}pF`],
    [2.2e-9, "F", `2.2${S}nF`],
    [1e-3, "A", `1${S}mA`],
    [1000, "Hz", `1${S}kHz`],
    [1234567, "Ω", `1.23457${S}MΩ`],
    // Rounding carries into the next prefix.
    [999999.6, "Ω", `1${S}MΩ`],
    [-0.5, "V", `${MINUS}500${S}mV`],
    [0, "V", `0${S}V`],
    [12, "bit", `12${S}bit`],
    [30, "", "30"],
    [0.3, "", "0.3"],
  ];
  it.each(cases)("%s %s", (v, unit, text) => {
    expect(formatNumber(v, unit, "param")).toBe(text);
  });

  it("formats values of every type", () => {
    expect(formatValue("inf", "Ω")).toBe(INFINITY_SIGN);
    expect(formatValue(true, "")).toBe("oui");
    expect(formatValue(false, "")).toBe("non");
    expect(formatValue("momentary", "")).toBe("momentary");
    expect(formatValue(undefined, "V")).toBe(EM_DASH);
    expect(formatNumber(Infinity, "Ω", "param")).toBe(INFINITY_SIGN);
  });
});

describe("readings: six significant digits kept, words for invalid states", () => {
  const cases: [number, string, string][] = [
    // Contract §9.1 values.
    [1.499997000006, "V", `1.50000${S}V`],
    [4.99999000002, "V", `4.99999${S}V`],
    [4.99999000002e-4, "A", `499.999${S}${MICRO}A`],
    [-4.99999000002e-4, "A", `${MINUS}499.999${S}${MICRO}A`],
    [2.49999000001e-3, "W", `2.49999${S}mW`],
    [3.499993, "V", `3.49999${S}V`],
    [3.1528849, "V", `3.15288${S}V`],
    [1.4411882, "V", `1.44119${S}V`],
    [1.2396668, "V", `1.23967${S}V`],
    [3.5e-5, "V", `35.0000${S}${MICRO}V`],
    [5, "V", `5.00000${S}V`],
    [0, "V", `0.00000${S}V`],
    [1228, "", "1228"],
    [4095, "", "4095"],
    // Below the femto range: fixed resolution, never a minus sign on a displayed zero.
    [1e-16, "A", `0.10000${S}fA`],
    [5e-22, "A", `0.00000${S}fA`],
    [-5e-22, "A", `0.00000${S}fA`],
    [1.5e16, "Ω", `15000.0${S}TΩ`],
  ];
  it.each(cases)("%s %s", (v, unit, text) => {
    expect(formatNumber(v, unit, "reading")).toBe(text);
    const belowResolution = unit === "A" && Math.abs(v) < 1e-12;
    const belowVolt = unit === "V" && v !== 0 && Math.abs(v) < 1e-9;
    expect(readingView({ k: "valid", v }, unit).text).toBe(belowResolution ? "< 1 pA" : belowVolt ? "< 1 nV" : text);
  });

  it("shows currents below the 1 pA calculation resolution as « < 1 pA », with the raw value in the title", () => {
    for (const v of [5.26713e-13, -1.99399e-13, 0, 9.99e-13]) {
      const view = readingView({ k: "valid", v }, "A");
      expect(view.text).toBe("< 1 pA");
      expect(view.title).toMatch(/^sous la résolution du calcul/);
    }
    expect(readingView({ k: "valid", v: 1e-12 }, "A").text).toBe(`1.00000${S}pA`);
    expect(readingView({ k: "valid", v: 4.99999e-4 }, "A").text).toBe(`499.999${S}µA`);
    expect(readingView({ k: "valid", v: 1e-13 }, "V").text).not.toBe("< 1 pA");
  });

  it("shows non-zero voltages below 1 nV as « < 1 nV », with the raw value in the title; 0 V keeps its digits", () => {
    for (const v of [-4.4409e-16, 2.2e-16, 9.99e-10, -9.99e-10]) {
      const view = readingView({ k: "valid", v }, "V");
      expect(view.text).toBe("< 1 nV");
      expect(view.title).toMatch(/^sous la résolution du calcul \(.*V\)$/);
      expect(view.state).toBe("value");
    }
    expect(readingView({ k: "valid", v: -4.4409e-16 }, "V").title).toBe(`sous la résolution du calcul (${MINUS}4.4409e${MINUS}16${S}V)`);
    expect(readingView({ k: "valid", v: 0 }, "V").text).toBe(`0.00000${S}V`);
    expect(readingView({ k: "valid", v: -0 }, "V").text).toBe(`0.00000${S}V`);
    expect(readingView({ k: "valid", v: 1e-9 }, "V").text).toBe(`1.00000${S}nV`);
    expect(readingView({ k: "valid", v: 5e-16 }, "A").text).toBe("< 1 pA");
    expect(readingView({ k: "saturated", v: 1e-12 }, "V").text).not.toContain("< 1 nV");
  });

  it("shows invalid readings in words, never as numbers", () => {
    const words: [Reading<number>, string][] = [
      [{ k: "floating" }, "flottant"],
      [{ k: "no_reference" }, "sans référence"],
      [{ k: "undefined" }, "indéfini"],
      [{ k: "open" }, "circuit ouvert"],
      [{ k: "saturated", v: 4095 }, "saturé (4095)"],
      [{ k: "unavailable", reason: "not_sampled_yet" }, EM_DASH],
    ];
    for (const [r, text] of words) {
      const view = readingView(r, "");
      expect(view.text).toBe(text);
      expect(view.state).toBe("word");
      expect(/\d/.test(view.text) && r.k !== "saturated").toBe(false);
    }
  });

  it("gives the reason of an unavailable reading as tooltip", () => {
    const known = readingView({ k: "unavailable", reason: "ecu_unpowered" }, "V");
    expect(known.title).toBe("indisponible : ECU non alimentée (ecu_unpowered)");
    const unknown = readingView({ k: "unavailable", reason: "new_reason" }, "V");
    expect(unknown.title).toBe("indisponible (new_reason)");
    // Every code the core emits (crates/n7rt-lab/src/phys and n7rt-solver) has a French reason.
    for (const reason of ["solver_singular", "mode_unresolved", "dominant_non_modelise_v1", "txd_undefined", "ecu_unpowered",
      "ic_model", "unknown_pin", "not_sampled_yet", "hidden_mission", "not_logic_pin", "invalid_reading"]) {
      expect(readingView({ k: "unavailable", reason }, "V").title).toMatch(new RegExp(`^indisponible : .+ \\(${reason}\\)$`));
    }
    expect(readingView(undefined, "V")).toEqual({ text: EM_DASH, title: "pas encore de valeur", state: "none" });
  });

  it("keeps the full value in the tooltip", () => {
    expect(readingView({ k: "valid", v: 1.499997000006 }, "V").title).toBe(`1.499997000006${S}V`);
    expect(fullText(4.99999000002e-4, "A")).toBe(`499.999000002${S}${MICRO}A`);
    expect(fullText(-3.5e-14, "A")).toBe(`${MINUS}3.5e${MINUS}14${S}A`);
    expect(fullText(1228, "")).toBe("1228");
  });
});

describe("field texts", () => {
  /** The core's exact conversion (contract §4): decimal string with the prefix as an exponent, read once. */
  const EXP: Record<string, number> = { p: -12, n: -9, [MICRO]: -6, m: -3, k: 3, M: 6, G: 9 };
  const read = (text: string): number => {
    const m = /^(-?[0-9.]+(?:e-?\d+)?)([pnµmkMG]?)$/.exec(text);
    if (!m) throw new Error(`texte illisible : ${text}`);
    return Number(m[2] ? `${m[1]}e${EXP[m[2]]}` : m[1]);
  };

  it("writes the exact representation while editing", () => {
    const cases: [number, string][] = [
      [4.7e-6, `4.7${MICRO}`], [1e4, "10k"], [0.1, "100m"], [1e12, "1000G"], [1e-12, "1p"], [2.2e-9, "2.2n"],
      [1.0000000001e4, "10.000000001k"], [1e-13, "1e-13"], [123456789012, "123.456789012G"], [5, "5"], [0, "0"],
      [-0.5, "-500m"], [0.0625, "62.5m"], [1e16, "1e16"],
    ];
    for (const [v, text] of cases) expect(exactText(v, "number")).toBe(text);
    expect(exactText("inf", "number_or_inf")).toBe(INFINITY_SIGN);
    expect(exactText(12, "int")).toBe("12");
  });

  it("reads every E12 value of every prefix back to the same double", () => {
    const e12 = [1.0, 1.2, 1.5, 1.8, 2.2, 2.7, 3.3, 3.9, 4.7, 5.6, 6.8, 8.2];
    let checked = 0;
    for (let decade = -14; decade <= 13; decade++) {
      for (const m of e12) {
        const v = Number(`${m}e${decade}`);
        expect(read(exactText(v, "number"))).toBe(v);
        checked++;
      }
    }
    // Doubles with long expansions (sums, divisions) round-trip as well.
    let x = 0.1;
    for (let i = 0; i < 500; i++) {
      x = (x * 7.31 + 0.013) % 997.3;
      for (const scale of [1e-12, 1e-6, 1, 1e3, 1e9]) {
        const v = x * scale;
        expect(read(exactText(v, "number"))).toBe(v);
        checked++;
      }
    }
    expect(checked).toBe(28 * 12 + 2500);
  });

  it("shows six significant digits at rest", () => {
    expect(fieldText(1234567.89, "number")).toBe("1.23457M");
    expect(fieldText(1e12, "number")).toBe("1T");
    expect(fieldText(0.0625, "number")).toBe("62.5m");
    expect(fieldText("inf", "number_or_inf")).toBe(INFINITY_SIGN);
    expect(fieldText(1000, "int")).toBe("1000");
    expect(fieldText("12", "choice")).toBe("12");
  });
});

describe("other texts", () => {
  it("gives parameter ranges", () => {
    const base = { editable: true };
    expect(rangeText({ ...base, vtype: "number", min: 0.01, max: 1e8, unit: "Ω" })).toBe(`de 10${S}mΩ à 100${S}MΩ`);
    expect(rangeText({ ...base, vtype: "number_or_inf", min: 1e3, max: 1e12, unit: "Ω" })).toBe(`de 1${S}kΩ à 1${S}TΩ, ou ${INFINITY_SIGN}`);
    expect(rangeText({ ...base, vtype: "int", min: 8, max: 16, unit: "bit" })).toBe(`de 8${S}bit à 16${S}bit`);
    expect(rangeText({ ...base, vtype: "choice", min: 0, max: 0, unit: "" })).toBe("");
    expect(rangeText({ vtype: "number", min: 120, max: 120, unit: "Ω", editable: false })).toBe("valeur fixe");
  });

  it("formats the simulated time to the millisecond", () => {
    expect(simTimeText(0)).toBe(`0.000${S}s`);
    expect(simTimeText(999_999)).toBe(`0.000${S}s`);
    expect(simTimeText(1_000_000)).toBe(`0.001${S}s`);
    expect(simTimeText(12_345_678_901)).toBe(`12.345${S}s`);
  });

  it("formats the zoom as a percentage, ratio or percent", () => {
    expect(zoomText(1)).toBe(`100${S}%`);
    expect(zoomText(100)).toBe(`100${S}%`);
    expect(zoomText(1.25)).toBe(`125${S}%`);
    expect(zoomText(0.262144)).toBe(`26${S}%`);
    expect(zoomText(381.4697265625)).toBe(`381${S}%`);
  });

  it("uses U+00A0 between number and unit, U+00B5 for micro, U+2212 for minus", () => {
    const outputs = [formatNumber(-4.7e-6, "F", "param"), formatNumber(-4.7e-6, "F", "reading")];
    for (const t of outputs) {
      expect(t.includes("μ")).toBe(false);
      expect(t.includes(" ") || t.includes(" ") || t.includes(" ")).toBe(false);
      expect(t.startsWith(MINUS)).toBe(true);
      expect(t.includes(`${S}${MICRO}F`)).toBe(true);
    }
  });
});

// ------------------------------------------------------------------------------------------------- glyphs

/** Every output of the formatter over a broad sweep of inputs. */
function sweep(): string[] {
  const out: string[] = [];
  const values = [0, 1, -1, 0.5, 1e-18, 3e-16, 1e-15, 2.2e-12, 4.7e-9, 3.3e-6, 1e-3, 0.0625, 1.499997000006, 12, 120,
    999.9996, 1228, 4095, 1e4, 1e6, 1e9, 1e12, 1.5e16, 1e21, Infinity];
  for (const unit of [...KNOWN_UNITS, ""]) {
    for (const v of values) {
      for (const s of [v, -v]) {
        out.push(formatNumber(s, unit, "param"), formatNumber(s, unit, "reading"), fullText(s, unit));
        out.push(fieldText(s, "number"), exactText(s, "number"), fieldText(s, "int"));
        out.push(readingView({ k: "valid", v: s }, unit).text, readingView({ k: "saturated", v: s }, unit).text);
      }
    }
    out.push(rangeText({ vtype: "number_or_inf", min: 1e3, max: 1e12, unit, editable: true }));
  }
  const words: Reading<number>[] = [{ k: "floating" }, { k: "no_reference" }, { k: "undefined" }, { k: "open" }];
  const reasons = ["solver_singular", "mode_unresolved", "dominant_non_modelise_v1", "txd_undefined", "ecu_unpowered",
    "ic_model", "unknown_pin", "not_sampled_yet", "hidden_mission", "not_logic_pin", "invalid_reading", "other_code_1"];
  for (const r of [...words, ...reasons.map((reason): Reading<number> => ({ k: "unavailable", reason }))]) {
    const v = readingView(r, "V");
    out.push(v.text, v.title);
  }
  out.push(readingView(undefined, "V").title, simTimeText(123_456_789_012), zoomText(1.25), formatValue(true, ""),
    formatValue(false, ""), rangeText({ vtype: "number", min: 1, max: 1, unit: "V", editable: false }));
  return out;
}

const range = (a: number, b: number) => Array.from({ length: b - a + 1 }, (_, i) => a + i);

/**
 * Coverage of the embedded fonts as listed in ui/DESIGN.md §3: the IBM Plex latin subset (Basic Latin, Latin-1
 * Supplement, the general punctuation of the subset, minus sign) and N7RT Symbols (° ± µ Ω Ω ← → ∆ − ∞ ≈ ≠ ≤ ≥).
 */
const PLEX_LATIN = new Set([...range(0x20, 0x7e), ...range(0xa0, 0xff), 0x2013, 0x2014, 0x2018, 0x2019, 0x201c,
  0x201d, 0x2022, 0x2026, 0x2212]);
const N7RT_SYMBOLS = [0xb0, 0xb1, 0xb5, 0x3a9, 0x2126, 0x2190, 0x2192, 0x2206, 0x2212, 0x221e, 0x2248, 0x2260, 0x2264,
  0x2265];
const DOCUMENTED = new Set([...PLEX_LATIN, ...N7RT_SYMBOLS]);
const hex = (c: string) => `U+${c.codePointAt(0)!.toString(16).toUpperCase().padStart(4, "0")}`;

describe("glyph coverage", () => {
  it("exports every character the formatter emits", () => {
    const missing = new Set<string>();
    for (const text of sweep()) for (const c of text) if (!FORMATTER_GLYPHS.has(c)) missing.add(`${c} ${hex(c)}`);
    expect([...missing]).toEqual([]);
  });

  it("finds every exported character in the documented font coverage", () => {
    const missing = [...FORMATTER_GLYPHS].filter((c) => !DOCUMENTED.has(c.codePointAt(0)!)).map(hex);
    expect(missing).toEqual([]);
    // The usual traps: Greek mu and the thin spaces are not in the fonts.
    for (const c of ["μ", " ", " ", "α"]) expect(DOCUMENTED.has(c.codePointAt(0)!)).toBe(false);
  });

  it("finds every exported character in the embedded font files", async () => {
    const files = import.meta.glob("../../fonts/*.woff2", { query: "?inline", import: "default", eager: true }) as Record<string, string>;
    const cmap = async (name: string) => {
      const entry = Object.entries(files).find(([path]) => path.endsWith(`/${name}`));
      if (!entry) throw new Error(`police absente : ${name}`);
      return woff2CodePoints(await dataUrlBytes(entry[1]));
    };
    for (const [plex, symbols] of [
      ["ibm-plex-mono-latin-400-normal.woff2", "n7rt-symbols-mono-400.woff2"],
      ["ibm-plex-mono-latin-500-normal.woff2", "n7rt-symbols-mono-500.woff2"],
      ["ibm-plex-sans-latin-400-normal.woff2", "n7rt-symbols-sans-400.woff2"],
      ["ibm-plex-sans-latin-500-normal.woff2", "n7rt-symbols-sans-500.woff2"],
      ["ibm-plex-sans-latin-600-normal.woff2", "n7rt-symbols-sans-500.woff2"],
    ]) {
      const a = await cmap(plex);
      const b = await cmap(symbols);
      const missing = [...FORMATTER_GLYPHS].filter((c) => !a.has(c.codePointAt(0)!) && !b.has(c.codePointAt(0)!)).map(hex);
      expect({ font: plex, missing }).toEqual({ font: plex, missing: [] });
      // The documented N7RT Symbols list matches the file.
      expect(N7RT_SYMBOLS.filter((cp) => !b.has(cp)).map((cp) => cp.toString(16))).toEqual([]);
      for (const cp of PLEX_LATIN) expect(a.has(cp) || cp === 0x7f).toBe(true);
    }
  });
});

// --------------------------------------------------------------------------- WOFF2 cmap reader (test only)

async function dataUrlBytes(url: string): Promise<Uint8Array> {
  const comma = url.indexOf(",");
  const bin = atob(url.slice(comma + 1));
  const bytes = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
  return bytes;
}

const WOFF2_TAGS = ["cmap", "head", "hhea", "hmtx", "maxp", "name", "OS/2", "post", "cvt ", "fpgm", "glyf", "loca", "prep",
  "CFF ", "VORG", "EBDT", "EBLC", "gasp", "hdmx", "kern", "LTSH", "PCLT", "VDMX", "vhea", "vmtx", "BASE", "GDEF", "GPOS",
  "GSUB", "EBSC", "JSTF", "MATH", "CBDT", "CBLC", "COLR", "CPAL", "SVG ", "sbix", "acnt", "avar", "bdat", "bloc", "bsln",
  "cvar", "fdsc", "feat", "fmtx", "fvar", "gvar", "hsty", "just", "lcar", "mort", "morx", "opbd", "prop", "trak", "Zapf",
  "Silf", "Glat", "Gloc", "Feat", "Sill"];

/** Code points mapped to a glyph by the cmap of a WOFF2 font (W3C WOFF2 §5, OpenType cmap formats 4 and 12). */
async function woff2CodePoints(file: Uint8Array): Promise<Set<number>> {
  const dv = new DataView(file.buffer, file.byteOffset, file.byteLength);
  if (dv.getUint32(0) !== 0x774f4632) throw new Error("pas un fichier WOFF2");
  const numTables = dv.getUint16(12);
  const compressedLength = dv.getUint32(20);
  let p = 48;
  const base128 = () => {
    let v = 0;
    for (let i = 0; i < 5; i++) {
      const b = dv.getUint8(p++);
      v = v * 128 + (b & 0x7f);
      if (!(b & 0x80)) return v;
    }
    throw new Error("UIntBase128 invalide");
  };
  const tables: { tag: string; length: number }[] = [];
  for (let i = 0; i < numTables; i++) {
    const flags = dv.getUint8(p++);
    let tag = WOFF2_TAGS[flags & 0x3f];
    if ((flags & 0x3f) === 0x3f) {
      tag = String.fromCharCode(dv.getUint8(p), dv.getUint8(p + 1), dv.getUint8(p + 2), dv.getUint8(p + 3));
      p += 4;
    }
    const version = flags >> 6;
    let length = base128();
    const transformed = tag === "glyf" || tag === "loca" ? version === 0 : version !== 0;
    if (transformed) length = base128();
    tables.push({ tag, length });
  }
  const stream = new Blob([file.slice(p, p + compressedLength)]).stream()
    .pipeThrough(new DecompressionStream("brotli" as CompressionFormat));
  const data = new Uint8Array(await new Response(stream).arrayBuffer());
  let offset = 0;
  let cmap: DataView | null = null;
  for (const t of tables) {
    if (t.tag === "cmap") cmap = new DataView(data.buffer, data.byteOffset + offset, t.length);
    offset += t.length;
  }
  if (!cmap) throw new Error("table cmap absente");
  const c = cmap;
  const points = new Set<number>();
  for (let i = 0; i < c.getUint16(2); i++) {
    const sub = c.getUint32(8 + i * 8);
    const format = c.getUint16(sub);
    if (format === 4) {
      const segX2 = c.getUint16(sub + 6);
      const ends = sub + 14, starts = ends + segX2 + 2, deltas = starts + segX2, ranges = deltas + segX2;
      for (let s = 0; s < segX2 / 2; s++) {
        const end = c.getUint16(ends + 2 * s), start = c.getUint16(starts + 2 * s);
        const delta = c.getInt16(deltas + 2 * s), ro = c.getUint16(ranges + 2 * s);
        for (let cp = start; cp <= end && cp !== 0xffff; cp++) {
          let gid = ro === 0 ? (cp + delta) & 0xffff : c.getUint16(ranges + 2 * s + ro + 2 * (cp - start));
          if (ro !== 0 && gid !== 0) gid = (gid + delta) & 0xffff;
          if (gid !== 0) points.add(cp);
        }
      }
    } else if (format === 12) {
      const groups = c.getUint32(sub + 12);
      for (let g = 0; g < groups; g++) {
        const start = c.getUint32(sub + 16 + g * 12), end = c.getUint32(sub + 20 + g * 12), first = c.getUint32(sub + 24 + g * 12);
        for (let cp = start; cp <= end; cp++) if (first + cp - start !== 0) points.add(cp);
      }
    }
  }
  return points;
}
