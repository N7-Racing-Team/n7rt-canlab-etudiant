// Values shown on the schematic (contract §4 "Affichage"), formatted by ../units like the panels: the canvas and the
// inspector always write the same value the same way. Readings that are not Valid are words, never numbers.

import type { Reading, Value } from "../types";
import { formatValue, readingView } from "../units";

/** A parameter value as stored in the document (number, "inf", bool or choice): « 10 kΩ », « ∞ », « oui ». */
export const formatParam = (value: Value | undefined, unit: string): string => formatValue(value, unit);

/** A live reading: six significant digits when Valid (trailing zeros kept), words for every other state. */
export const formatReading = (r: Reading<number> | undefined, unit: string): string => readingView(r, unit).text;
