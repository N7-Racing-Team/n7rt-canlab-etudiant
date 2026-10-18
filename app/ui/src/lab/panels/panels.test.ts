// Pure helpers of the panels and of the e2e scenario (no DOM).

import { describe, expect, it } from "vitest";
import { parseShown } from "../e2e-phys";
import { formatNumber, fullText } from "../units";
import { mergeJournal } from "./console";
import { keepNotes, parsePosition } from "./inspector";

describe("journal", () => {
  const row = (frame: number, text: string, kind = "édition") => ({ frame, kind, cls: "", text });

  it("puts each refusal after the commands of its frame, in arrival order", () => {
    const core = [row(0, "a"), row(5, "b"), row(5, "c"), row(9, "d")];
    const local = [row(5, "refus 1", "refus"), row(7, "refus 2", "refus")];
    expect(mergeJournal(core, local).map((r) => r.text)).toEqual(["a", "b", "c", "refus 1", "refus 2", "d"]);
    expect(mergeJournal([], local).map((r) => r.text)).toEqual(["refus 1", "refus 2"]);
    expect(mergeJournal(core, []).map((r) => r.text)).toEqual(["a", "b", "c", "d"]);
  });

  it("keeps the newest entries only", () => {
    const core = Array.from({ length: 400 }, (_, i) => row(i, `c${i}`));
    const merged = mergeJournal(core, [row(399, "dernier refus", "refus")], 300);
    expect(merged.length).toBe(300);
    expect(merged[0].text).toBe("c101");
    expect(merged[299].text).toBe("dernier refus");
  });
});

describe("potentiometer position typed in the inspector", () => {
  it("accepts a point or a comma, from 0 to 1", () => {
    expect(parsePosition("0.7")).toBe(0.7);
    expect(parsePosition(" 0,3 ")).toBe(0.3);
    expect(parsePosition(".5")).toBe(0.5);
    expect(parsePosition("1")).toBe(1);
    expect(parsePosition("0")).toBe(0);
  });

  it("refuses anything else", () => {
    for (const t of ["", "-0.1", "1.0001", "abc", "0.5.1", "1e-1", "0x1", "NaN", "Infinity"]) expect(parsePosition(t)).toBe(null);
  });
});

describe("command labels of the inspector", () => {
  it("keep a parenthesised note on one line: the label wraps before it", () => {
    expect(keepNotes("Position du curseur (0 = A)")).toBe("Position du curseur (0 = A)");
    expect(keepNotes("Appui (bouton) puis (relâché ici)")).toBe("Appui (bouton) puis (relâché ici)");
    expect(keepNotes("Contact fermé")).toBe("Contact fermé");
  });
});

describe("reading the displayed values back (e2e)", () => {
  it("parses the full value of a tooltip to the same double", () => {
    for (const [v, unit] of [[1.499997000006, "V"], [-4.99999000002e-4, "A"], [2.49999000001e-3, "W"], [1228, ""], [3.5e-14, "A"]] as const) {
      expect(parseShown(fullText(v, unit))).toBe(v);
    }
  });

  it("parses displayed texts and rejects words", () => {
    expect(parseShown(formatNumber(1.4999970, "V", "reading"))).toBe(1.5);
    expect(parseShown(formatNumber(-4.99999e-4, "A", "reading"))).toBeCloseTo(-4.99999e-4, 12);
    expect(parseShown("flottant")).toBe(null);
    expect(parseShown("—")).toBe(null);
  });
});

describe("operating states of the APPS bench parts", () => {
  it("shows the reset state of the black box and the travel of the pedal sensor", async () => {
    const { opStateLines } = await import("./text");
    expect(opStateLines({ kind: "ecu", powered: true }, "ecu_apps")).toEqual(["Alimenté"]);
    expect(opStateLines({ kind: "ecu", powered: false }, "ecu_apps")).toEqual(["En reset"]);
    expect(opStateLines({ kind: "ecu", powered: true }, "ecu_stm32")).toEqual(["Carte alimentée"]);
    expect(opStateLines({ kind: "pot", position: 0.457 }, "mlx90377")).toEqual(["Course de la pédale : 45.7 %"]);
    expect(opStateLines({ kind: "pot", position: 0.3 })).toEqual(["Curseur à 0.300 (0 = A, 1 = B)"]);
  });
});
