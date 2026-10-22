import { describe, expect, it } from "vitest";
import { imageFor, type Artifact } from "./api";

const h753 = { sha256: "a", student_crc32: 1, flash_bytes: 260, ram_bytes: 8192 };
const g0b1 = { sha256: "b", student_crc32: 2, flash_bytes: 456, ram_bytes: 8192 };

describe("image of the connected board", () => {
  it("picks the image compiled for the board family", () => {
    const build: Artifact = { ...h753, sha256: "build", targets: { h753, g0b1 } };
    expect(imageFor(build, "g0b1")).toBe(g0b1);
    expect(imageFor(build, "h753")).toBe(h753);
    expect(imageFor(build, undefined)?.student_crc32).toBe(1);
  });

  it("never shows the H753 figures for a G0B1 when the build has no G0B1 image", () => {
    const old: Artifact = { ...h753 };
    expect(imageFor(old, "g0b1")).toBeUndefined();
    expect(imageFor(old, "h753")).toBe(old);
    expect(imageFor(null, "h753")).toBeUndefined();
  });
});
