import { beforeEach, describe, expect, it, vi } from "vitest";
import { ide, type IdeView } from "./api";
import { StudentIde } from "./view";
import { FICHE_NOT_SAVED, FICHE_UNSAVED, watchFiche } from "../views/unsaved";

const native = vi.hoisted(() => ({ onCloseRequested: vi.fn(), destroy: vi.fn() }));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => native }));
vi.mock("./api", () => ({ ide: { state: vi.fn() } }));

type Choice = "cancel" | "save" | "discard";
type Guard = {
  operation: boolean;
  closing: boolean;
  closeChoice(message: string, modified: boolean, title?: string): Promise<Choice>;
  requestNativeClose(destroy: () => Promise<void>): Promise<void>;
};
const state = (busy = false) => ({ busy } as IdeView);
const setup = (modified = false) => {
  const editor = new StudentIde(() => {});
  const guard = editor as unknown as Guard;
  const dirty = vi.spyOn(editor, "modified", "get").mockReturnValue(modified);
  const choice = vi.spyOn(guard, "closeChoice").mockResolvedValue("cancel");
  return { editor, guard, dirty, choice };
};

beforeEach(() => {
  vi.clearAllMocks();
  watchFiche(null);
  vi.mocked(ide.state).mockReset().mockResolvedValue(state());
  native.onCloseRequested.mockResolvedValue(() => {});
  native.destroy.mockResolvedValue(undefined);
});

describe("native window close guard", () => {
  it("registers once and prevents the native close before awaiting backend state", async () => {
    const { editor } = setup();
    await editor.protectNativeClose();
    await editor.protectNativeClose();
    expect(native.onCloseRequested).toHaveBeenCalledTimes(1);
    const handler = native.onCloseRequested.mock.calls[0][0] as (event: { preventDefault(): void }) => Promise<void>;
    const event = { preventDefault: vi.fn() };
    const pending = handler(event);
    expect(event.preventDefault).toHaveBeenCalledOnce();
    expect(native.destroy).not.toHaveBeenCalled();
    await pending;
    expect(native.destroy).toHaveBeenCalledOnce();
  });

  it("keeps the window open when an authoritative backend operation is active", async () => {
    const { guard, choice } = setup();
    vi.mocked(ide.state).mockResolvedValue(state(true));
    await guard.requestNativeClose(native.destroy);
    expect(choice).toHaveBeenCalledWith(expect.any(String), false);
    expect(native.destroy).not.toHaveBeenCalled();
  });

  it("keeps the window open for a local operation before the backend observes it", async () => {
    const { guard, choice } = setup();
    guard.operation = true;
    await guard.requestNativeClose(native.destroy);
    expect(choice).toHaveBeenCalledWith(expect.any(String), false);
    expect(native.destroy).not.toHaveBeenCalled();
  });

  it("cancels a dirty close without saving or destroying", async () => {
    const { editor, guard, choice } = setup(true);
    const save = vi.spyOn(editor, "run").mockResolvedValue();
    await guard.requestNativeClose(native.destroy);
    expect(choice).toHaveBeenCalledWith(expect.any(String), true);
    expect(save).not.toHaveBeenCalled();
    expect(native.destroy).not.toHaveBeenCalled();
  });

  it("saves before closing only when the editor reports all buffers saved", async () => {
    const { editor, guard, dirty, choice } = setup(true);
    choice.mockResolvedValue("save");
    const save = vi.spyOn(editor, "run").mockImplementation(async () => { dirty.mockReturnValue(false); });
    await guard.requestNativeClose(native.destroy);
    expect(save).toHaveBeenCalledWith("save");
    expect(native.destroy).toHaveBeenCalledOnce();
    expect(guard.closing).toBe(true);
  });

  it("refuses close after a failed save leaves dirty buffers", async () => {
    const { editor, guard, choice } = setup(true);
    choice.mockResolvedValueOnce("save").mockResolvedValueOnce("cancel");
    vi.spyOn(editor, "run").mockResolvedValue();
    await guard.requestNativeClose(native.destroy);
    expect(choice).toHaveBeenCalledTimes(2);
    expect(native.destroy).not.toHaveBeenCalled();
  });

  it("allows an explicit discard after rechecking that no work started", async () => {
    const { guard, choice } = setup(true);
    choice.mockResolvedValue("discard");
    await guard.requestNativeClose(native.destroy);
    expect(ide.state).toHaveBeenCalledTimes(2);
    expect(native.destroy).toHaveBeenCalledOnce();
  });

  it("blocks work that started while the dirty dialog was open", async () => {
    const { guard, choice } = setup(true);
    choice.mockResolvedValueOnce("discard").mockResolvedValueOnce("cancel");
    vi.mocked(ide.state).mockResolvedValueOnce(state()).mockResolvedValueOnce(state(true));
    await guard.requestNativeClose(native.destroy);
    expect(choice).toHaveBeenCalledTimes(2);
    expect(native.destroy).not.toHaveBeenCalled();
  });

  it("coalesces repeated close requests while a confirmation is pending", async () => {
    const { guard, choice } = setup(true);
    let answer!: (choice: Choice) => void;
    choice.mockImplementation(() => new Promise((resolve) => { answer = resolve; }));
    const first = guard.requestNativeClose(native.destroy);
    await vi.waitFor(() => expect(choice).toHaveBeenCalledOnce());
    await guard.requestNativeClose(native.destroy);
    expect(choice).toHaveBeenCalledOnce();
    answer("cancel");
    await first;
    expect(native.destroy).not.toHaveBeenCalled();
  });

  it("fails closed when backend state cannot be verified", async () => {
    const { editor, guard, choice } = setup();
    vi.mocked(ide.state).mockRejectedValue(new Error("private backend failure"));
    await guard.requestNativeClose(native.destroy);
    expect(choice).toHaveBeenCalledWith(expect.any(String), false);
    expect(editor.failure).not.toContain("private backend failure");
    expect(native.destroy).not.toHaveBeenCalled();
  });

  it("asks before losing the fiche's unsaved changes and closes once they are saved", async () => {
    const { guard, choice } = setup();
    const fiche = { unsaved: true, saveUnsaved: vi.fn(async () => true) };
    watchFiche(fiche);
    choice.mockResolvedValue("save");
    await guard.requestNativeClose(native.destroy);
    expect(choice).toHaveBeenCalledWith(FICHE_UNSAVED, true, "Fiche non enregistrée");
    expect(fiche.saveUnsaved).toHaveBeenCalledOnce();
    expect(native.destroy).toHaveBeenCalledOnce();
  });

  it("keeps the window open when the fiche cannot be saved or the student goes back", async () => {
    const { guard, choice } = setup();
    watchFiche({ unsaved: true, saveUnsaved: vi.fn(async () => false) });
    choice.mockResolvedValueOnce("save").mockResolvedValueOnce("cancel");
    await guard.requestNativeClose(native.destroy);
    expect(choice).toHaveBeenLastCalledWith(FICHE_NOT_SAVED, false, "Fermeture suspendue");
    expect(native.destroy).not.toHaveBeenCalled();
    choice.mockReset().mockResolvedValue("cancel");
    await guard.requestNativeClose(native.destroy);
    expect(native.destroy).not.toHaveBeenCalled();
  });

  it("closes without saving the fiche only on an explicit discard", async () => {
    const { guard, choice } = setup();
    const fiche = { unsaved: true, saveUnsaved: vi.fn(async () => true) };
    watchFiche(fiche);
    choice.mockResolvedValue("discard");
    await guard.requestNativeClose(native.destroy);
    expect(fiche.saveUnsaved).not.toHaveBeenCalled();
    expect(native.destroy).toHaveBeenCalledOnce();
  });

  it("restores the unload protection after a failed native destroy", async () => {
    const { guard, choice } = setup();
    native.destroy.mockRejectedValueOnce(new Error("private native failure"));
    await guard.requestNativeClose(native.destroy);
    expect(guard.closing).toBe(false);
    expect(choice).toHaveBeenCalledWith(expect.any(String), false);
  });
});
