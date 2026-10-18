// Public C editor model. Source text is never interpreted as markup or script.
export const FILES = ["student_decoder.c", "student_decoder.h"] as const;
export type StudentFile = typeof FILES[number];
export type TokenKind = "plain" | "comment" | "string" | "keyword" | "number" | "preprocessor";
export interface Token { text: string; kind: TokenKind }

const KEYWORDS = new Set(("auto break case char const continue default do double else enum extern float for goto " +
  "if inline int long register restrict return short signed sizeof static struct switch typedef union unsigned " +
  "void volatile while _Bool _Atomic _Static_assert bool true false uint8_t uint16_t uint32_t uint64_t " +
  "int8_t int16_t int32_t int64_t size_t").split(" "));

/** Single pass lexer: comments/strings win over C keywords, including across lines. */
export function tokenize(source: string): Token[] {
  const result: Token[] = [];
  const pattern = /\/\*[\s\S]*?(?:\*\/|(?![\s\S]))|\/\/[^\n]*|"(?:\\[\s\S]|[^"\\])*"?|'(?:\\[\s\S]|[^'\\])*'?|^[\t ]*#[^\n]*|\b(?:0[xX][\da-fA-F]+|\d+(?:\.\d*)?(?:[eE][+-]?\d+)?)[uUlLfF]*\b|\b[A-Za-z_]\w*\b/gm;
  let end = 0;
  for (const m of source.matchAll(pattern)) {
    const text = m[0];
    if (m.index > end) result.push({ text: source.slice(end, m.index), kind: "plain" });
    const kind: TokenKind = text.startsWith("/*") || text.startsWith("//") ? "comment" :
      text.startsWith('"') || text.startsWith("'") ? "string" : text.trimStart().startsWith("#") ? "preprocessor" :
      /^\d/.test(text) ? "number" : KEYWORDS.has(text) ? "keyword" : "plain";
    result.push({ text, kind });
    end = m.index + text.length;
  }
  if (end < source.length) result.push({ text: source.slice(end), kind: "plain" });
  return result;
}

export function findMatches(source: string, query: string, caseSensitive = false): number[] {
  if (!query) return [];
  const text = caseSensitive ? source : source.toLocaleLowerCase();
  const needle = caseSensitive ? query : query.toLocaleLowerCase();
  const matches: number[] = [];
  for (let i = text.indexOf(needle); i >= 0 && matches.length < 4096; i = text.indexOf(needle, i + needle.length)) matches.push(i);
  return matches;
}

/** Clamp editor positions to the current buffer, including stale selections. */
export function positionOffset(source: string, line: number, column = 1): number {
  const lines = source.split("\n");
  const row = Math.max(0, Math.min(lines.length - 1, Math.floor(line || 1) - 1));
  return lines.slice(0, row).reduce((n, s) => n + s.length + 1, 0) +
    Math.max(0, Math.min(lines[row].length, Math.floor(column || 1) - 1));
}

/** GCC emits one-based UTF-8 byte columns; CodeMirror uses UTF-16 offsets. */
export function compilerPositionOffset(source: string, line: number, byteColumn = 1): number {
  const start = positionOffset(source, line);
  const end = source.indexOf("\n", start);
  const row = source.slice(start, end < 0 ? source.length : end);
  const target = Math.max(0, Math.floor(byteColumn || 1) - 1);
  const utf8 = new TextEncoder();
  let bytes = 0, units = 0;
  for (const character of row) {
    const width = utf8.encode(character).length;
    if (bytes + width > target) break;
    bytes += width;
    units += character.length;
  }
  return start + units;
}

export function cursorPosition(source: string, offset: number): { line: number; column: number } {
  const before = source.slice(0, Math.max(0, offset));
  return { line: before.split("\n").length, column: before.length - before.lastIndexOf("\n") };
}

/** An outline of student's function declarations, never a file browser. */
export function outline(source: string): { name: string; line: number }[] {
  const plain = tokenize(source).map((t) => t.kind === "comment" || t.kind === "string" || t.kind === "preprocessor" ? t.text.replace(/[^\n]/g, " ") : t.text).join("");
  const rows: { name: string; line: number }[] = [];
  const re = /^[\t ]*(?:[\w*]+[\t ]+)+([A-Za-z_]\w*)\s*\([^;{}]*\)\s*(?:\{|;)/gm;
  for (const m of plain.matchAll(re)) rows.push({ name: m[1], line: plain.slice(0, m.index).split("\n").length });
  return rows.slice(0, 200);
}
