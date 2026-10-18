// Bench multimeter (lab/panels/meter.ts) in a small fake DOM: the controls carry their design-system classes, the
// probes their swatches, « Mesure clé » shows during an incident only and records through the progression, and a
// running verification locks the probes with the core's message.

import { afterAll, beforeAll, describe, expect, it } from "vitest";
import type { IncidentView, ProgressionView } from "../../types";
import type { LabController, LabState } from "../state";
import { CONSIGN_REFUSED, MeterPanel } from "./meter";

type Listener = () => void;

class FakeNode {
  children: FakeNode[] = [];
  parent: FakeNode | null = null;
  className = "";
  hidden = false;
  disabled = false;
  title = "";
  type = "";
  value = "";
  dataset: Record<string, string> = {};
  private text = "";
  private listeners: Record<string, Listener[]> = {};
  constructor(readonly tagName: string) {}
  get firstChild(): FakeNode | null {
    return this.children[0] ?? null;
  }
  appendChild(c: FakeNode): FakeNode {
    c.parent = this;
    this.children.push(c);
    return c;
  }
  removeChild(c: FakeNode): FakeNode {
    this.children.splice(this.children.indexOf(c), 1);
    c.parent = null;
    return c;
  }
  get textContent(): string {
    return this.children.length ? this.children.map((c) => c.textContent).join("") : this.text;
  }
  set textContent(v: string) {
    this.children = [];
    this.text = v;
  }
  addEventListener(type: string, fn: Listener): void {
    (this.listeners[type] ??= []).push(fn);
  }
  fire(type: string): void {
    for (const fn of this.listeners[type] ?? []) fn();
  }
  byClass(cls: string): FakeNode[] {
    return this.children.flatMap((c) => [...(c.className.split(" ").includes(cls) ? [c] : []), ...c.byClass(cls)]);
  }
}

let saved: unknown;
beforeAll(() => {
  saved = (globalThis as { document?: unknown }).document;
  (globalThis as { document?: unknown }).document = { createElement: (tag: string) => new FakeNode(tag) };
});
afterAll(() => {
  (globalThis as { document?: unknown }).document = saved;
});

async function settle(): Promise<void> {
  for (let i = 0; i < 5; i++) await Promise.resolve();
}

const RUNNING = "Essai en cours — 28 s. Ne débranchez pas la carte et ne fermez pas l'application.";

const incident = (over: Partial<IncidentView> = {}): IncidentView => ({
  rank: 1, editable: true, grandeur: "tension", units: ["V"], hint: "h", consigned: null, categories: [], states: [], rules: [], parts: [],
  pins: [], ...over,
});

const progression = (over: Partial<ProgressionView> = {}): ProgressionView => ({
  readable: true, steps: [], current: 1, order: "", button: "Vérifier la réparation", ready: false, running: false, blocked: null,
  feedback: [], nominal_editable: false, fiche_state: "", documents: [], incident: incident(), ...over,
});

function mounted(): { root: FakeNode; panel: MeterPanel } {
  const root = new FakeNode("div");
  const panel = new MeterPanel(root as unknown as HTMLElement, {} as LabState, {} as LabController);
  panel.mount();
  return { root, panel };
}

describe("bench multimeter", () => {
  it("draws the readout and three design-system selects, with the probes' swatches", () => {
    const { root } = mounted();
    expect(root.byClass("meter-display")[0].className).toBe("meter-display readout");
    expect(["meter-function", "meter-com", "meter-vohm"].map((c) => root.byClass(c)[0].className)).toEqual([
      "select meter-function", "select meter-com", "select meter-vohm",
    ]);
    expect(root.byClass("meter-label").map((l) => [l.textContent, l.dataset.probe])).toEqual([["Fonction", undefined], ["COM", "com"], ["VΩ", "vohm"]]);
    expect([root.byClass("meter-mission")[0].hidden, root.byClass("meter-note")[0].hidden]).toEqual([true, true]);
  });

  it("shows « Mesure clé » during an incident only, and records the measure through the progression", async () => {
    const { root, panel } = mounted();
    const calls: string[] = [];
    const consign = async (): Promise<void> => void calls.push("consign");
    const block = root.byClass("meter-mission")[0];
    panel.setMission(progression({ current: 0, incident: null }), consign);
    expect(block.hidden).toBe(true);
    panel.setMission(progression(), consign);
    expect(block.hidden).toBe(false);
    expect(root.byClass("mission-head")[0].textContent).toBe("Mesure clé");
    expect(root.byClass("meter-quantity")[0].textContent).toBe("Grandeur imposée : tension.");
    const consigned = root.byClass("meter-consigned")[0];
    expect([consigned.textContent, consigned.className]).toEqual(["Mesure non consignée.", "meter-consigned warn"]);
    const button = root.byClass("meter-consign")[0];
    expect([button.textContent, button.className, button.disabled, button.title]).toEqual(["Consigner la mesure", "btn meter-consign ico ico--meter", false, ""]);
    button.fire("click");
    await settle();
    expect(calls).toEqual(["consign"]);
    // the core's refusal, under the button
    const refusal = "Aucune trame récente sur le bus : lancez le véhicule, puis consignez la mesure.";
    panel.setMission(progression(), () => Promise.reject(refusal));
    button.fire("click");
    await settle();
    expect(root.byClass("meter-error")[0].textContent).toBe(refusal);
    // recorded; no state declared yet
    panel.setMission(progression({ incident: incident({ consigned: "le 15/10/2026 à 14:32", grandeur: null }) }), consign);
    expect([consigned.textContent, consigned.className]).toEqual(["Mesure consignée le 15/10/2026 à 14:32.", "meter-consigned ok"]);
    expect(root.byClass("meter-quantity")[0].textContent).toBe("Choisissez l'état observé : il impose la grandeur à relever.");
  });

  it("locks the probes during a verification and says why; a validated incident cannot be recorded", () => {
    const { root, panel } = mounted();
    const consign = async (): Promise<void> => {};
    const selects = (): FakeNode[] => ["meter-function", "meter-com", "meter-vohm"].map((c) => root.byClass(c)[0]);
    const note = root.byClass("meter-note")[0];
    const button = root.byClass("meter-consign")[0];
    panel.setMission(progression({ running: true, blocked: RUNNING, incident: incident({ editable: false }) }), consign);
    expect(selects().map((s) => [s.disabled, s.title])).toEqual(Array(3).fill([true, RUNNING]));
    expect([note.hidden, note.textContent, button.disabled, button.title]).toEqual([false, RUNNING, true, RUNNING]);
    panel.setMission(progression({ incident: incident({ editable: false }) }), consign);
    expect(selects().map((s) => [s.disabled, s.title])).toEqual(Array(3).fill([false, ""]));
    expect([note.hidden, button.disabled, button.title]).toEqual([true, true, CONSIGN_REFUSED]);
    expect(CONSIGN_REFUSED).toBe("La mesure ne peut être consignée que pendant l'incident en cours, hors vérification.");
    panel.setMission(null, consign);
    expect([root.byClass("meter-mission")[0].hidden, note.hidden, selects()[0].disabled]).toEqual([true, true, false]);
  });
});
