// Display formatting of the Physical Lab (contract docs/physical-lab-v1.md §4). Display only: text typed by the
// user is sent as is to the core, which parses and validates it (one implementation, in Rust).
//
// - Engineering prefix, point decimal, U+00A0 between the number and the unit, micro written U+00B5.
// - 6 significant digits at most: trailing zeros removed for parameters, kept for readings (stable width).
// - Readings that are not Valid are words, never numbers.
// - Every character these functions can emit is in FORMATTER_GLYPHS, which the tests check against the embedded
//   fonts (IBM Plex latin subset + N7RT Symbols, ui/DESIGN.md §3).

import type { Reading, Value, ValueType } from "./types";

export const NBSP = " ";
export const MICRO = "µ";
/** Typographic minus of displayed numbers (fields being edited use the ASCII hyphen, accepted by the core). */
export const MINUS = "−";
export const INFINITY_SIGN = "∞";
export const EM_DASH = "—";

/** Display prefixes, femto to tera. */
const PREFIX: Record<number, string> = { [-15]: "f", [-12]: "p", [-9]: "n", [-6]: MICRO, [-3]: "m", 0: "", 3: "k", 6: "M", 9: "G", 12: "T" };
const DISPLAY_EXP = { min: -15, max: 12 };
/** Prefixes accepted by the core's grammar (§4): p to G only. */
const INPUT_EXP = { min: -12, max: 9 };

/** Units shown with an engineering prefix; any other unit ("bit", "") is shown as a plain number. */
const PREFIXED_UNITS = new Set(["V", "A", "W", "Ω", "Ω", "F", "Hz", "s"]);
/** Units of the V1 catalog and of the published measures, for the glyph set. */
export const KNOWN_UNITS = ["V", "A", "W", "Ω", "Ω", "F", "Hz", "s", "bit", "%"];

export const WORDS = {
  floating: "flottant",
  noReference: "sans référence",
  undefined: "indéfini",
  open: "circuit ouvert",
  saturated: "saturé",
  unavailable: EM_DASH,
  yes: "oui",
  no: "non",
} as const;

/** French reasons of Reading Unavailable (stable codes of crates/n7rt-lab/src/phys/mod.rs). */
const REASONS_FR: Record<string, string> = {
  solver_singular: "système singulier, aucun résultat pour cet îlot",
  mode_unresolved: "modes non résolus (cycle de commutation)",
  dominant_non_modelise_v1: "état dominant non modélisé en V1",
  txd_undefined: "TXD entre ses seuils",
  ecu_unpowered: "ECU non alimentée",
  ic_model: "non calculé pour ce circuit intégré",
  unknown_pin: "broche inconnue",
  not_sampled_yet: "pas encore échantillonné",
  hidden_mission: "masqué en mode mission",
  // Emitted by the core besides the documented list (probe of a pin without logic level, internal fallback).
  not_logic_pin: "broche sans niveau logique",
  invalid_reading: "lecture incohérente du moteur",
};

export type Digits = "param" | "reading";

/** Six significant digits, correctly rounded (toExponential), as a digit string and a decimal exponent. */
function significant(abs: number): { digits: string; exp: number } {
  const [m, e] = abs.toExponential(5).split("e");
  return { digits: m.replace(".", ""), exp: Number(e) };
}

/** Shortest digits that read back to the same double (exact representation). */
function shortest(abs: number): { digits: string; exp: number } {
  const [m, e] = abs.toExponential().split("e");
  return { digits: m.replace(".", ""), exp: Number(e) };
}

function trimZeros(digits: string): string {
  const t = digits.replace(/0+$/, "");
  return t === "" ? "0" : t;
}

/** Places the decimal point after intLen digits (padding with zeros on either side). */
function placePoint(digits: string, intLen: number): string {
  if (intLen <= 0) return `0.${"0".repeat(-intLen)}${digits}`;
  if (intLen >= digits.length) return digits + "0".repeat(intLen - digits.length);
  return `${digits.slice(0, intLen)}.${digits.slice(intLen)}`;
}

function withUnit(num: string, prefix: string, unit: string): string {
  const u = prefix + unit;
  return u ? `${num}${NBSP}${u}` : num;
}

/** Number with an engineering prefix: "10 kΩ" (param), "1.50000 V" (reading). */
function prefixed(v: number, unit: string, digits: Digits): string {
  const sign = v < 0 ? MINUS : "";
  const abs = Math.abs(v);
  if (abs === 0) return withUnit(digits === "param" ? "0" : "0.00000", "", unit);
  const s = significant(abs);
  const exp3 = Math.floor(s.exp / 3) * 3;
  if (exp3 < DISPLAY_EXP.min) {
    // Below the femto range: fixed resolution of the smallest prefix.
    const fixed = (abs / 1e-15).toFixed(5);
    const text = digits === "param" ? fixed.replace(/\.?0+$/, "") : fixed;
    return withUnit(Number(fixed) === 0 ? text : sign + text, PREFIX[DISPLAY_EXP.min], unit);
  }
  const e = Math.min(exp3, DISPLAY_EXP.max);
  const d = digits === "param" ? trimZeros(s.digits) : s.digits;
  return withUnit(sign + placePoint(d, s.exp - e + 1), PREFIX[e], unit);
}

/** Number without prefix: integers as integers, otherwise six significant digits. */
function plain(v: number, unit: string, digits: Digits): string {
  const sign = v < 0 ? MINUS : "";
  const abs = Math.abs(v);
  if (Number.isInteger(abs) && abs < 1e15) return withUnit(sign + String(abs), "", unit);
  const s = significant(abs);
  const d = digits === "param" ? trimZeros(s.digits) : s.digits;
  const num = s.exp >= -6 && s.exp < 15 ? placePoint(d, s.exp + 1) : `${placePoint(d, 1)}e${s.exp < 0 ? MINUS : ""}${Math.abs(s.exp)}`;
  return withUnit(sign + num, "", unit);
}

/** A finite number for display. +∞ is "∞" (number_or_inf parameters); NaN never reaches the UI but shows "—". */
export function formatNumber(v: number, unit: string, digits: Digits): string {
  if (Number.isNaN(v)) return EM_DASH;
  if (!Number.isFinite(v)) return v > 0 ? INFINITY_SIGN : MINUS + INFINITY_SIGN;
  return PREFIXED_UNITS.has(unit) ? prefixed(v, unit, digits) : plain(v, unit, digits);
}

/** A parameter or state value as text: "10 kΩ", "4.7 µF", "∞", "oui", choice code as is. */
export function formatValue(v: Value | undefined, unit: string): string {
  if (v === undefined) return EM_DASH;
  if (typeof v === "boolean") return v ? WORDS.yes : WORDS.no;
  if (v === "inf") return INFINITY_SIGN;
  if (typeof v === "string") return v;
  return formatNumber(v, unit, "param");
}

/** Compact prefixed text of a number in a field: "10k", "4.7µ", "250m". Digits: display or exact. */
function compact(v: number, exact: boolean): string {
  if (v === 0 || Number.isNaN(v)) return Number.isNaN(v) ? "" : "0";
  const sign = v < 0 ? (exact ? "-" : MINUS) : "";
  if (!Number.isFinite(v)) return sign + INFINITY_SIGN;
  const abs = Math.abs(v);
  const s = exact ? shortest(abs) : significant(abs);
  const d = trimZeros(s.digits);
  const range = exact ? INPUT_EXP : DISPLAY_EXP;
  const exp3 = Math.floor(s.exp / 3) * 3;
  if (exp3 < range.min || exp3 > range.max + 3) {
    // Outside the prefixes: decimal exponent, which the core's grammar accepts too.
    return `${sign}${placePoint(d, 1)}e${s.exp < 0 ? "-" : ""}${Math.abs(s.exp)}`;
  }
  const e = Math.min(exp3, range.max);
  return sign + placePoint(d, s.exp - e + 1) + PREFIX[e];
}

/** Text of a parameter field at rest: six significant digits, zeros removed ("10k", "4.7µ", "∞", "12"). */
export function fieldText(v: Value | undefined, vtype: ValueType): string {
  if (v === undefined) return "";
  if (typeof v === "boolean" || typeof v === "string") return v === "inf" ? INFINITY_SIGN : String(v);
  if (vtype === "int" && Number.isInteger(v)) return (v < 0 ? MINUS : "") + String(Math.abs(v));
  return compact(v, false);
}

/**
 * Exact text of a parameter in a field being edited: it reads back to the same double through the core's grammar
 * (shortest round-trip digits, decimal point moved, never multiplied by the prefix).
 */
export function exactText(v: Value | undefined, vtype: ValueType): string {
  if (v === undefined) return "";
  if (typeof v === "boolean" || typeof v === "string") return v === "inf" ? INFINITY_SIGN : String(v);
  if (vtype === "int" && Number.isInteger(v)) return String(v);
  return compact(v, true);
}

export interface ReadingView {
  text: string;
  /** Tooltip: full value, or the reason of an unavailable reading. */
  title: string;
  /** "value": a number; "word": an invalid state in words; "none": nothing to show. */
  state: "value" | "word" | "none";
}

export function reasonText(reason: string): string {
  const fr = REASONS_FR[reason];
  return fr ? `indisponible : ${fr} (${reason})` : `indisponible (${reason})`;
}

/** Full value of a reading for its tooltip, exact digits: "1.499997000006 V". */
export function fullText(v: number, unit: string): string {
  if (!Number.isFinite(v)) return formatNumber(v, unit, "reading");
  if (!PREFIXED_UNITS.has(unit)) return withUnit((v < 0 ? MINUS : "") + String(Math.abs(v)).replaceAll("-", MINUS), "", unit);
  const text = compact(v, true).replaceAll("-", MINUS);
  const m = /^(.*?)([fpnµmkMGT]?)$/.exec(text);
  return m ? withUnit(m[1], m[2], unit) : text;
}

/** Currents below this magnitude are shown as « < 1 pA » (numerical resolution, contract §5.3). */
export const CURRENT_RESOLUTION_A = 1e-12;
/** Non-zero voltages below this magnitude are shown as « < 1 nV » (residue of a difference, e.g. CANH − CANL). */
export const VOLTAGE_RESOLUTION_V = 1e-9;

/** A reading of the core: a formatted number when Valid, words otherwise. */
export function readingView(r: Reading<number> | null | undefined, unit: string): ReadingView {
  if (!r) return { text: EM_DASH, title: "pas encore de valeur", state: "none" };
  switch (r.k) {
    case "valid":
      // Contract §5.3: a capacitor current is only resolved to ε·(2C/h)·|V|; residues below 1 pA are numerical
      // noise, never shown as a measured current.
      if (unit === "A" && Math.abs(r.v) < CURRENT_RESOLUTION_A) {
        return { text: "< 1 pA", title: `sous la résolution du calcul (${fullText(r.v, unit)})`, state: "value" };
      }
      // A node at the reference reads exactly 0 and keeps its digits.
      if (unit === "V" && r.v !== 0 && Math.abs(r.v) < VOLTAGE_RESOLUTION_V) {
        return { text: "< 1 nV", title: `sous la résolution du calcul (${fullText(r.v, unit)})`, state: "value" };
      }
      return { text: formatNumber(r.v, unit, "reading"), title: fullText(r.v, unit), state: "value" };
    case "saturated":
      return {
        text: `${WORDS.saturated} (${formatNumber(r.v, unit, "reading")})`,
        title: `hors de la plage de conversion, valeur bornée : ${fullText(r.v, unit)}`,
        state: "word",
      };
    case "floating":
      return { text: WORDS.floating, title: "potentiel indéterminé : aucun chemin vers une référence", state: "word" };
    case "no_reference":
      return { text: WORDS.noReference, title: "îlot sans masse : seules les différences de tension sont définies", state: "word" };
    case "undefined":
      return { text: WORDS.undefined, title: "entrée logique entre ses seuils", state: "word" };
    case "open":
      return { text: WORDS.open, title: "aucun chemin résistif", state: "word" };
    case "unavailable":
      return { text: WORDS.unavailable, title: reasonText(r.reason), state: "word" };
  }
}

/** Range of a parameter for its tooltip: "de 10 mΩ à 100 MΩ", "de 1 kΩ à 1 TΩ, ou ∞". */
export function rangeText(def: { vtype: ValueType; min: number; max: number; unit: string; editable: boolean }): string {
  if (!def.editable) return "valeur fixe";
  if (def.vtype === "bool" || def.vtype === "choice") return "";
  const digits = "param";
  const text = `de ${formatNumber(def.min, def.unit, digits)} à ${formatNumber(def.max, def.unit, digits)}`;
  return def.vtype === "number_or_inf" ? `${text}, ou ${INFINITY_SIGN}` : text;
}

/** Simulated time, millisecond resolution: "12.345 s". */
export function simTimeText(ns: number): string {
  const ms = Math.max(0, Math.floor(ns / 1e6));
  return `${Math.floor(ms / 1000)}.${String(ms % 1000).padStart(3, "0")}${NBSP}s`;
}

/**
 * Zoom for the toolbar: "100 %". LabState.view.zoomPercent is documented in percent (default 100) while
 * Editor.zoomTo takes a ratio (1 = 100 %); the zoom ladder (1.25^k, k in [-6, 6]) keeps ratios below 4 and
 * percents above 26, so both readings are accepted.
 */
export function zoomText(z: number): string {
  const percent = z < 10 ? z * 100 : z;
  return `${Math.round(percent)}${NBSP}%`;
}

/** Every character the functions above can emit, for the glyph coverage test. */
export const FORMATTER_GLYPHS: ReadonlySet<string> = (() => {
  const parts = [
    "0123456789.e-+", NBSP, MINUS, INFINITY_SIGN, EM_DASH, " ,():",
    Object.values(PREFIX).join(""),
    KNOWN_UNITS.join(""),
    Object.values(WORDS).join(""),
    Object.values(REASONS_FR).join(""),
    "indisponible hors de la plage de conversion, valeur bornée potentiel indéterminé aucun chemin vers une référence",
    "îlot sans masse seules les différences de tension sont définies entrée logique entre ses seuils résistif",
    "pas encore de valeur fixe de à ou",
    "< 1 pA < 1 nV sous la résolution du calcul",
    "abcdefghijklmnopqrstuvwxyz_",
  ];
  return new Set(Array.from(parts.join("")));
})();
