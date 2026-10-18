// ViewHost state machine (ui/src/shell.ts) and per-view layouts, without DOM.

import { describe, expect, it } from "vitest";
import {
  COCKPIT_LAYOUT_KEY, FRAMES_ROOM, PHYS_LAYOUT_KEY, ViewHost, cockpitDefaults, ficheShare, physDefaults, readLayout, shownTab, tallHeight,
  tallLimits, writeLayout, type HostedView,
} from "../../shell";

/** A view with its own state: frames counted, data kept across hide/show. */
class FakeView implements HostedView {
  visible = false;
  frames = 0;
  readonly data: string[] = [];
  failOn: "show" | "hide" | "frame" | null = null;

  constructor(private readonly name: string, private readonly log: string[]) {}

  show(): void {
    this.log.push(`show:${this.name}`);
    if (this.failOn === "show") throw new Error("show");
    this.visible = true;
  }

  hide(): void {
    this.log.push(`hide:${this.name}`);
    this.visible = false;
    if (this.failOn === "hide") throw new Error("hide");
  }

  frame(): void {
    if (this.failOn === "frame") throw new Error("frame");
    this.frames++;
  }
}

function setup() {
  const log: string[] = [];
  const errors: string[] = [];
  const cockpit = new FakeView("cockpit", log);
  const phys = new FakeView("phys", log);
  const host = new ViewHost({ cockpit, phys });
  host.onError = (where) => errors.push(where);
  const changes: string[] = [];
  host.onChange = (n) => changes.push(n);
  return { log, errors, cockpit, phys, host, changes };
}

describe("ViewHost", () => {
  it("starts with no view and draws nothing", () => {
    const { host, cockpit, phys } = setup();
    expect(host.active).toBe(null);
    host.frame(0);
    expect(cockpit.frames + phys.frames).toBe(0);
  });

  it("first shows one view and hides all the others", () => {
    const { host, log, changes, cockpit, phys } = setup();
    expect(host.show("cockpit")).toBe(true);
    expect(log).toEqual(["hide:phys", "show:cockpit"]);
    expect(changes).toEqual(["cockpit"]);
    expect([cockpit.visible, phys.visible]).toEqual([true, false]);
  });

  it("hides the active view before showing the next one, and frames only the active view", () => {
    const { host, log, cockpit, phys } = setup();
    host.show("cockpit");
    host.frame(1);
    host.show("phys");
    host.frame(2);
    host.frame(3);
    expect(log).toEqual(["hide:phys", "show:cockpit", "hide:cockpit", "show:phys"]);
    expect([cockpit.frames, phys.frames]).toEqual([1, 2]);
    expect([cockpit.visible, phys.visible]).toEqual([false, true]);
  });

  it("ignores the active view, unknown views and inherited property names", () => {
    const { host, log } = setup();
    host.show("cockpit");
    log.length = 0;
    expect(host.show("cockpit")).toBe(false);
    expect(host.show("oscilloscope")).toBe(false);
    expect(host.show("toString")).toBe(false);
    expect(log).toEqual([]);
    expect(host.active).toBe("cockpit");
  });

  it("keeps each view's state across cockpit -> phys -> cockpit", () => {
    const { host, cockpit, phys } = setup();
    host.show("cockpit");
    cockpit.data.push("onglet liaison", "trame 12 sélectionnée");
    host.show("phys");
    phys.data.push("R1 sélectionnée");
    host.show("cockpit");
    expect(host.active).toBe("cockpit");
    expect(cockpit.data).toEqual(["onglet liaison", "trame 12 sélectionnée"]);
    expect(phys.data).toEqual(["R1 sélectionnée"]);
    expect([cockpit.visible, phys.visible]).toEqual([true, false]);
  });

  it("ignores a switch requested while switching", () => {
    const log: string[] = [];
    const cockpit = new FakeView("cockpit", log);
    const phys = new FakeView("phys", log);
    const host = new ViewHost({ cockpit, phys });
    let nested: boolean | null = null;
    phys.show = () => {
      log.push("show:phys");
      nested = host.show("cockpit");
    };
    host.show("cockpit");
    host.show("phys");
    expect(nested).toBe(false);
    expect(host.active).toBe("phys");
    expect(log).toEqual(["hide:phys", "show:cockpit", "hide:cockpit", "show:phys"]);
  });

  it("reports a failing view without breaking the host", () => {
    const { host, errors, cockpit, phys } = setup();
    host.show("cockpit");
    phys.failOn = "show";
    expect(host.show("phys")).toBe(true);
    expect(host.active).toBe("phys");
    phys.failOn = "frame";
    for (let i = 0; i < 10; i++) host.frame(i);
    cockpit.failOn = "hide";
    phys.failOn = "hide";
    expect(host.show("cockpit")).toBe(true);
    host.frame(11);
    expect(cockpit.frames).toBe(1);
    // Frame errors are reported three times at most per activation.
    expect(errors).toEqual(["show:phys", "frame:phys", "frame:phys", "frame:phys", "hide:phys"]);
  });
});

describe("per-view layouts", () => {
  class MemoryStore {
    readonly map = new Map<string, string>();
    getItem(k: string) {
      return this.map.get(k) ?? null;
    }
    setItem(k: string, v: string) {
      this.map.set(k, v);
    }
  }

  it("keeps the cockpit and the Physical Lab layouts apart", () => {
    const store = new MemoryStore();
    expect(COCKPIT_LAYOUT_KEY).toBe("n7rt-can-lab.layout.v1");
    expect(PHYS_LAYOUT_KEY).toBe("n7rt-can-lab.layout.v1.phys");
    writeLayout(store, COCKPIT_LAYOUT_KEY, { ...cockpitDefaults(1600), left: 400, tab: "link" });
    writeLayout(store, PHYS_LAYOUT_KEY, { ...physDefaults(), hidden: ["bottom"], tab: "phys-nets" });
    expect(readLayout(store, COCKPIT_LAYOUT_KEY, cockpitDefaults(1600))).toEqual({ ...cockpitDefaults(1600), left: 400, tab: "link" });
    expect(readLayout(store, PHYS_LAYOUT_KEY, physDefaults())).toEqual({ ...physDefaults(), hidden: ["bottom"], tab: "phys-nets" });
  });

  it("falls back to the defaults on missing, corrupted or ill-typed storage", () => {
    const store = new MemoryStore();
    expect(readLayout(store, PHYS_LAYOUT_KEY, physDefaults())).toEqual(physDefaults());
    expect(readLayout(null, PHYS_LAYOUT_KEY, physDefaults())).toEqual(physDefaults());
    store.setItem(PHYS_LAYOUT_KEY, "{pas du json");
    expect(readLayout(store, PHYS_LAYOUT_KEY, physDefaults())).toEqual(physDefaults());
    store.setItem(PHYS_LAYOUT_KEY, JSON.stringify({ left: "large", hidden: "right", tab: 3, bottom: 150 }));
    expect(readLayout(store, PHYS_LAYOUT_KEY, physDefaults())).toEqual({ ...physDefaults(), bottom: 150 });
    // First run of the cockpit on a narrow window: results hidden, as before.
    expect(cockpitDefaults(1200).hidden).toEqual(["right"]);
    expect(cockpitDefaults(1400).hidden).toEqual([]);
  });

  it("remembers a tall tab's own dock height, and ignores a malformed one", () => {
    const store = new MemoryStore();
    writeLayout(store, COCKPIT_LAYOUT_KEY, { ...cockpitDefaults(1600), tab: "fiche", heights: { fiche: 520 } });
    expect(readLayout(store, COCKPIT_LAYOUT_KEY, cockpitDefaults(1600))).toEqual({ ...cockpitDefaults(1600), tab: "fiche", heights: { fiche: 520 } });
    for (const heights of [{ fiche: "haut" }, [500], 12, { fiche: null }]) {
      store.setItem(COCKPIT_LAYOUT_KEY, JSON.stringify({ bottom: 200, heights }));
      const read = readLayout(store, COCKPIT_LAYOUT_KEY, cockpitDefaults(1600));
      expect(read).toEqual({ ...cockpitDefaults(1600), bottom: 200 });
      expect("heights" in read).toBe(false);
    }
  });

  it("sizes a tall tab from the workspace, within its limits", () => {
    const share = (ws: number) => ws * 0.6;
    expect(tallLimits(833)).toEqual([240, 653]);
    expect(tallHeight(undefined, share, 833)).toBe(500); // 60 % of the workspace
    expect(tallHeight(600, share, 833)).toBe(600); // the remembered height
    expect(tallHeight(800, share, 833)).toBe(653); // at least 180 px left above
    expect(tallHeight(100, share, 833)).toBe(240);
    expect(tallHeight(undefined, share, 0)).toBe(240); // hidden view: no height to share
  });

  it("gives the fiche more of a short workspace, about four rows of frames left above it", () => {
    expect(ficheShare(706)).toBe(706 - FRAMES_ROOM); // 1366 x 768: 494 px instead of 424
    expect(tallHeight(undefined, ficheShare, 706)).toBe(494);
    expect(ficheShare(833)).toBe(500); // 1702 x 1114 at 125 %: unchanged
    expect(ficheShare(1056)).toBe(634); // 60 % of a tall workspace
    expect(tallHeight(300, ficheShare, 706)).toBe(300); // a height the student chose is kept
  });

  it("never selects a hidden tab, even a remembered one", () => {
    const tabs = [{ name: "fiche", hidden: true }, { name: "logs", hidden: false }, { name: "link", hidden: false }];
    expect(shownTab(tabs, "fiche", "logs")).toBe("logs");
    expect(shownTab(tabs, "link", "logs")).toBe("link");
    expect(shownTab(tabs, "inconnu", "logs")).toBe("logs");
    expect(shownTab([{ ...tabs[0], hidden: false }, tabs[1]], "fiche", "logs")).toBe("fiche");
  });
});
