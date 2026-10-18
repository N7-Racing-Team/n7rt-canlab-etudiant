import { describe, expect, it } from "vitest";
import { formatParam, formatReading } from "./format";

const NB = " ";

describe("schematic value format (contract §4)", () => {
  it("writes parameters with an engineering prefix, a decimal point, U+00A0 and the micro sign U+00B5", () => {
    expect(formatParam(1e4, "Ω")).toBe(`10${NB}kΩ`);
    expect(formatParam(4.7e-6, "F")).toBe(`4.7${NB}µF`);
    expect(formatParam(0.1, "A")).toBe(`100${NB}mA`);
    expect(formatParam(120, "Ω")).toBe(`120${NB}Ω`);
    expect(formatParam(2.2e-9, "F")).toBe(`2.2${NB}nF`);
    expect(formatParam(1234567, "Ω")).toBe(`1.23457${NB}MΩ`);
  });

  it("writes parameters of every type", () => {
    expect(formatParam("inf", "Ω")).toBe("∞");
    expect(formatParam(true, "")).toBe("oui");
    expect(formatParam("latching", "")).toBe("latching");
    expect(formatParam(undefined, "V")).toBe("—");
  });

  it("keeps six significant digits for valid readings, with the minus sign U+2212", () => {
    expect(formatReading({ k: "valid", v: 1.49999 }, "V")).toBe(`1.49999${NB}V`);
    expect(formatReading({ k: "valid", v: 2.5 }, "V")).toBe(`2.50000${NB}V`);
    expect(formatReading({ k: "valid", v: -1.5 }, "V")).toBe(`−1.50000${NB}V`);
    expect(formatReading({ k: "valid", v: 499.999e-6 }, "A")).toBe(`499.999${NB}µA`);
  });

  it("writes invalid readings in words, never as numbers", () => {
    expect(formatReading({ k: "floating" }, "V")).toBe("flottant");
    expect(formatReading({ k: "no_reference" }, "V")).toBe("sans référence");
    expect(formatReading({ k: "undefined" }, "V")).toBe("indéfini");
    expect(formatReading({ k: "open" }, "V")).toBe("circuit ouvert");
    expect(formatReading({ k: "unavailable", reason: "ecu_unpowered" }, "V")).toBe("—");
    expect(formatReading({ k: "saturated", v: 3.4 }, "V")).toBe(`saturé (3.40000${NB}V)`);
    expect(formatReading(undefined, "V")).toBe("—");
  });
});
