import { afterEach, describe, expect, it, vi } from "vitest";
import { FICHE_NOT_SAVED, FICHE_UNSAVED, ficheUnsaved, settleFiche, watchFiche, type CloseChoice } from "./unsaved";

const work = (unsaved: boolean, saves = true) => ({ unsaved, saveUnsaved: vi.fn(async () => saves) });
const asking = (...answers: CloseChoice[]) =>
  vi.fn(async (_message: string, _offer: boolean): Promise<CloseChoice> => answers.shift() ?? "cancel");

afterEach(() => watchFiche(null));

describe("a close never loses the fiche's unsaved changes silently", () => {
  it("lets the close go on without a question when nothing is unsaved", async () => {
    const ask = asking();
    expect(await settleFiche(ask)).toBe(true);
    watchFiche(work(false));
    expect(await settleFiche(ask)).toBe(true);
    expect(ask).not.toHaveBeenCalled();
  });

  it("keeps the fiche open when the student goes back", async () => {
    const w = work(true);
    watchFiche(w);
    const ask = asking("cancel");
    expect(await settleFiche(ask)).toBe(false);
    expect(ask).toHaveBeenCalledWith(FICHE_UNSAVED, true);
    expect(w.saveUnsaved).not.toHaveBeenCalled();
  });

  it("closes without saving only on an explicit discard", async () => {
    const w = work(true);
    watchFiche(w);
    expect(await settleFiche(asking("discard"))).toBe(true);
    expect(w.saveUnsaved).not.toHaveBeenCalled();
  });

  it("tells the reload guard whether the watched fiche holds unsaved changes", () => {
    expect(ficheUnsaved()).toBe(false);
    watchFiche(work(true));
    expect(ficheUnsaved()).toBe(true);
    watchFiche(work(false));
    expect(ficheUnsaved()).toBe(false);
  });

  it("saves before closing, and stays open when the save fails", async () => {
    const ok = work(true);
    watchFiche(ok);
    expect(await settleFiche(asking("save"))).toBe(true);
    expect(ok.saveUnsaved).toHaveBeenCalledOnce();
    watchFiche(work(true, false));
    const ask = asking("save", "cancel");
    expect(await settleFiche(ask)).toBe(false);
    expect(ask).toHaveBeenLastCalledWith(FICHE_NOT_SAVED, false);
  });
});
