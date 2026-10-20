import { describe, expect, it } from "vitest";
import { compilerPositionOffset, cursorPosition, failureText, findMatches, outline, positionOffset, tokenize } from "./model";

describe("C editor", () => {
  it("preserves exact source text, including hostile markup, unicode and multiline comments", () => {
    const source = '#include "student_decoder.h"\n/* int\nreturn */\nvoid f(void) { n7rt_log("<img onerror=x> Ω"); }\n';
    expect(tokenize(source).map((t) => t.text).join("")).toBe(source);
    expect(tokenize(source).find((t) => t.text.includes("onerror"))?.kind).toBe("string");
    expect(tokenize(source).find((t) => t.text.includes("return"))?.kind).toBe("comment");
  });
  it("handles unfinished comments/quotes while editing", () => {
    expect(tokenize('/* return\nint').at(-1)?.kind).toBe("comment");
    expect(tokenize('"hello').at(-1)?.kind).toBe("string");
  });
  it("finds literal text, empty queries and both case modes", () => {
    expect(findMatches("int INT [.* [.*", "[.*")).toEqual([8, 12]);
    expect(findMatches("int INT", "INT")).toEqual([0, 4]);
    expect(findMatches("int INT", "INT", true)).toEqual([4]);
    expect(findMatches("int", "")).toEqual([]);
    expect(findMatches("x".repeat(10000), "x")).toHaveLength(4096);
  });
  it("maps and clamps compiler positions without leaving the source", () => {
    expect(positionOffset("a\nbc\n", 2, 2)).toBe(3);
    expect(positionOffset("a\nbc", 999, 999)).toBe(4);
    expect(positionOffset("a\nbc", -2, -2)).toBe(0);
    expect(cursorPosition("a\nbc", 3)).toEqual({ line: 2, column: 2 });
  });
  it("outline ignores fake declarations in comments and strings", () => {
    expect(outline('/* void evil(void) {} */\nvoid student_init(void)\n{\n}\nvoid student_on_tick(uint64_t now);')).toEqual([
      { name: "student_init", line: 2 }, { name: "student_on_tick", line: 5 },
    ]);
  });
  it("maps GCC byte columns after accents, emoji and tabs to the exact character", () => {
    for (const source of ["/*é*/ int x = ;", "/*😀*/ int x = ;", "\t/*é😀*/ int x = ;"]) {
      const expected = source.indexOf(";");
      const byteColumn = new TextEncoder().encode(source.slice(0, expected)).length + 1;
      expect(compilerPositionOffset(source, 1, byteColumn)).toBe(expected);
      expect(compilerPositionOffset("first\n" + source, 2, byteColumn)).toBe(6 + expected);
      expect(compilerPositionOffset(source, 999, 999)).toBe(source.length);
    }
    expect(compilerPositionOffset("é😀x", 1, 2)).toBe(0);
    expect(compilerPositionOffset("é😀x", 1, 4)).toBe(1);
    expect(compilerPositionOffset("é😀x", -1, -1)).toBe(0);
  });
});

describe("failed build or flash banner", () => {
  const generic = "Compilation échouée. Corrigez les problèmes indiqués.";
  it("names the missing tool instead of pointing at the student's code", () => {
    const missing = "Compilateur arm-none-eabi-gcc introuvable : installez Arm GNU Toolchain (arm-none-eabi).";
    expect(failureText({ message: missing, diagnostics: [{ file: null }] }, generic)).toBe(missing);
    expect(failureText({ message: "ST-LINK : commande refusée.\n    Error: no probe", diagnostics: [] }, generic))
      .toBe("ST-LINK : commande refusée.");
  });
  it("keeps the generic text when the compiler located problems, or without a message", () => {
    expect(failureText({ message: "Compilation refusée.", diagnostics: [{ file: "student_decoder.c" }] }, generic)).toBe(generic);
    expect(failureText({ diagnostics: [] }, generic)).toBe(generic);
    expect(failureText(null, generic)).toBe(generic);
  });
});
