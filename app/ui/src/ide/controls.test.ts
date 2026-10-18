import { describe, expect, it } from "vitest";
import { flashRefusal } from "./controls";
import type { IdeView } from "./api";
import type { Status } from "../types";

const view = { available: true, busy: false, revision: 2, built_revision: 2, build: { ok: true } } as IdeView;
const board = { state: "READY", session: null, port: { serial: "C" }, board: { runtime_verified: true, compatible: true } } as Status;
describe("IDE operation gates", () => {
  it("allows only a current successful build on an identified verified board", () => {
    expect(flashRefusal(view, false, board, false)).toBeNull();
    expect(flashRefusal(view, true, board, false)).toMatch(/Compilez/);
    expect(flashRefusal({ ...view, built_revision: 1 }, false, board, false)).toMatch(/Compilez/);
    expect(flashRefusal({ ...view, build: { ...view.build!, ok: false } }, false, board, false)).toMatch(/Compilez/);
  });
  it("blocks concurrent operations, active sessions, checking and untrusted cards", () => {
    expect(flashRefusal({ ...view, busy: true }, false, board, false)).toMatch(/cours/);
    expect(flashRefusal(view, false, { ...board, session: {} as Status["session"] }, false)).toMatch(/session/);
    expect(flashRefusal(view, false, board, true)).toMatch(/session/);
    expect(flashRefusal(view, false, { ...board, board: { ...board.board!, runtime_verified: false } }, false)).toMatch(/runtime/);
    expect(flashRefusal(view, false, { ...board, port: { ...board.port!, serial: null } }, false)).toMatch(/identifiée/);
    expect(flashRefusal(view, false, { ...board, state: "LOST" }, false)).toMatch(/prête/);
  });
});
