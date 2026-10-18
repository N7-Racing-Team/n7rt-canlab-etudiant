// Student progression (views/progress.ts): shape parsing of the fiche form, step boxes from the core's exact
// states, the verification button gated by the core, text only (an innerHTML write throws in this fake DOM), and no
// score, counter or percentage anywhere in the module.

import progressSrc from "./progress.ts?raw";
import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import type { IncidentView, NominalPart, ProgressionView } from "../types";
import {
  CLOSED_FICHE, CLOSED_LINE, DECISIONS, DETAIL_HIDDEN, EXPORT_PENDING, EXPORTED, FicheForm, MISSING_LABEL, MISSING_LEAD, NOT_EXPORTED, NOT_SAVED,
  ProgressPanel, RUNNING_NOTE, SHAPE_REFUSED, emptyPart, feedbackModel, formatIdentifier, missingFields, parseIdentifier, parseInteger,
  parseInvalidCode, parseNumber, parseSlots, progressStatus, stepClass, type ProgressElements,
} from "./progress";
import { emptyIncident as emptyIncidentPart, VALIDATED_INCIDENT } from "./incident";
import { DIRTY, FIX_FIELDS, FORMAT, SAVED_FIRST } from "./form";

type Listener = (e: { target: FakeNode }) => void;

class FakeNode {
  children: FakeNode[] = [];
  parent: FakeNode | null = null;
  className = "";
  hidden = false;
  disabled = false;
  /** A dialog's state (showModal / close). */
  open = false;
  type = "";
  placeholder = "";
  dataset: Record<string, string> = {};
  private text = "";
  private listeners: Record<string, Listener[]> = {};
  protected own = "";
  constructor(readonly tagName: string) {}
  get value(): string {
    return this.own;
  }
  set value(v: string) {
    this.own = v;
  }
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
  set innerHTML(_v: string) {
    throw new Error("innerHTML is forbidden for dynamic data");
  }
  addEventListener(type: string, fn: Listener): void {
    (this.listeners[type] ??= []).push(fn);
  }
  showModal(): void {
    this.open = true;
  }
  close(): void {
    this.open = false;
  }
  fire(type: string): void {
    for (const fn of this.listeners[type] ?? []) fn({ target: this });
  }
  all(tag: string): FakeNode[] {
    return this.children.flatMap((c) => [...(c.tagName === tag ? [c] : []), ...c.all(tag)]);
  }
  byClass(cls: string): FakeNode[] {
    return this.children.flatMap((c) => [...(c.className.split(" ").includes(cls) ? [c] : []), ...c.byClass(cls)]);
  }
}

class FakeSelect extends FakeNode {
  get options(): FakeNode[] {
    return this.children;
  }
  get value(): string {
    const o = this.children.find((c) => c.value === this.own);
    return o ? o.value : this.children[0]?.value ?? "";
  }
  set value(v: string) {
    this.own = v;
  }
}

let saved: unknown;
beforeAll(() => {
  saved = (globalThis as { document?: unknown }).document;
  (globalThis as { document?: unknown }).document = {
    createElement: (tag: string) => (tag === "select" ? new FakeSelect(tag) : new FakeNode(tag)),
  };
});
afterAll(() => {
  (globalThis as { document?: unknown }).document = saved;
});

const STATES = ["EN COURS", "À FAIRE", "À FAIRE", "À FAIRE", "À FAIRE"];
const NAMES = ["Essai nominal", "Incident 1", "Incident 2", "Incident 3", "Qualification finale"];

function view(over: Partial<ProgressionView> = {}): ProgressionView {
  return {
    readable: true,
    steps: NAMES.map((name, i) => ({ name, state: STATES[i], since: null, current: i === 0 })),
    current: 0,
    order: "Essai nominal. Identifiez les trames de votre ECU sur le bus, décodez-les, programmez la carte et documentez votre décodage dans la fiche. Lancez la vérification quand votre code et votre fiche sont prêts.",
    button: "Lancer la vérification",
    ready: false,
    running: false,
    blocked: "Complétez la fiche de cette étape avant la vérification. Champs à renseigner : Trames : au moins une trame.",
    feedback: [],
    nominal_editable: true,
    fiche_state: "En cours de rédaction",
    documents: [{ id: "doc-1", title: "Norme A" }],
    incident: null,
    ...over,
  };
}

function elements(): ProgressElements & Record<string, FakeNode> {
  const n = (tag: string) => new FakeNode(tag) as unknown as HTMLElement;
  return {
    section: n("div"), steps: n("div"), order: n("p"), verify: n("button") as HTMLButtonElement, blocked: n("p"),
    feedback: n("div"), ficheToggle: n("button") as HTMLButtonElement, fiche: n("div"), final: n("button") as HTMLButtonElement,
    reproduce: n("button") as HTMLButtonElement, reproduceDialog: n("dialog") as HTMLDialogElement,
    reproduceConfirm: n("button") as HTMLButtonElement, reproduceCancel: n("button") as HTMLButtonElement,
  } as unknown as ProgressElements & Record<string, FakeNode>;
}

/** Lets the panel's async handlers run to their end. */
async function settle(): Promise<void> {
  for (let i = 0; i < 5; i++) await Promise.resolve();
}

const incidentActions = {
  loadIncident: async () => null,
  saveIncident: async () => {},
  consign: async () => {},
  reproduce: async () => {},
  openFinal: async () => {},
};
const quiet = { verify: async () => {}, loadFiche: async () => emptyPart(), saveFiche: async () => {}, ...incidentActions };

describe("form shapes", () => {
  it("reads identifiers on 11 bits only", () => {
    expect(parseIdentifier("0x7A5")).toBe(0x7a5);
    expect(parseIdentifier(" 0x7ff ")).toBe(0x7ff);
    expect(parseIdentifier("")).toBeNull();
    expect(parseIdentifier("0x800")).toBeUndefined();
    expect(parseIdentifier("272")).toBeUndefined();
    expect(parseIdentifier("0xZZ")).toBeUndefined();
    expect(formatIdentifier(0x7a5)).toBe("0x7A5");
    expect(formatIdentifier(0xa)).toBe("0x00A");
    expect(formatIdentifier(null)).toBe("");
  });

  it("reads integers, numbers, slots and invalid codes", () => {
    expect(parseInteger("63", 0, 63)).toBe(63);
    expect(parseInteger("64", 0, 63)).toBeUndefined();
    expect(parseInteger("1.5", 0, 63)).toBeUndefined();
    expect(parseNumber("0,1")).toBeCloseTo(0.1);
    expect(parseNumber("-5")).toBe(-5);
    expect(parseNumber("1e-3")).toBe(0.001);
    expect(parseNumber("abc")).toBeUndefined();
    expect(parseNumber("")).toBeNull();
    expect(parseSlots("0, 1")).toEqual([0, 1]);
    expect(parseSlots("")).toEqual([]);
    expect(parseSlots("0, x")).toBeUndefined();
    expect(parseInvalidCode("aucun")).toBe("AUCUN");
    expect(parseInvalidCode("0xFFF")).toBe(4095);
    expect(parseInvalidCode("4095")).toBe(4095);
    expect(parseInvalidCode("")).toBeNull();
    expect(parseInvalidCode("n/a")).toBeUndefined();
  });

  it("offers the public decision values in order", () => {
    expect(DECISIONS.map((d) => d.label.slice(0, 4))).toEqual(["DD-1", "DD-2", "DD-3", "DD-4"]);
    expect(DECISIONS[2].values).toEqual(["REJETER", "BORNER", "PUBLIER"]);
  });
});

describe("progress panel", () => {
  it("draws the five steps with the core's exact states, then the fiche box", () => {
    const e = elements();
    const panel = new ProgressPanel(e, quiet);
    panel.mount();
    panel.setView(view());
    const boxes = (e.steps as unknown as FakeNode).children;
    expect(boxes.map((b) => b.textContent)).toEqual([...NAMES.map((n, i) => n + STATES[i]), "FicheEn cours de rédaction"]);
    expect(boxes[0].className).toBe("step step-open step-current");
    expect(boxes[1].className).toBe("step step-todo");
    expect(boxes[5].className).toBe("step step-fiche");
    expect(stepClass({ name: "x", state: "ÉCHEC À CORRIGER", since: null, current: false })).toBe("step step-failed");
    expect(stepClass({ name: "x", state: "VALIDÉ", since: "le 13/10/2026 à 08:30", current: false })).toBe("step step-passed");
  });

  it("enables the button only when the core says ready, and shows the blocking message", async () => {
    const e = elements();
    let asked = 0;
    const panel = new ProgressPanel(e, { ...quiet, verify: async () => void asked++ });
    panel.mount();
    panel.setView(view());
    expect(e.verify.disabled).toBe(true);
    expect(e.blocked.textContent).toContain("Complétez la fiche de cette étape");
    (e.verify as unknown as FakeNode).fire("click");
    await settle();
    expect(asked).toBe(0);
    panel.setView(view({ ready: true, blocked: null }));
    expect(e.verify.disabled).toBe(false);
    expect(e.blocked.hidden).toBe(true);
    (e.verify as unknown as FakeNode).fire("click");
    await settle();
    expect(asked).toBe(1);
  });

  it("shows a refusal of the core under the button until the view changes", async () => {
    const e = elements();
    const panel = new ProgressPanel(e, { ...quiet, verify: async () => Promise.reject("Connectez la carte et vérifiez que votre programme est chargé.") });
    panel.mount();
    panel.setView(view({ ready: true, blocked: null }));
    (e.verify as unknown as FakeNode).fire("click");
    await settle();
    expect(e.blocked.hidden).toBe(false);
    expect(e.blocked.textContent).toBe("Connectez la carte et vérifiez que votre programme est chargé.");
    panel.setView(view({ ready: true, blocked: null, feedback: ["x"] }));
    expect(e.blocked.hidden).toBe(true);
  });

  it("saves the open fiche before verifying, and verifies nothing when a field is wrongly shaped", async () => {
    const e = elements();
    const calls: string[] = [];
    const panel = new ProgressPanel(e, {
      ...incidentActions,
      verify: async () => void calls.push("verify"),
      loadFiche: async () => emptyPart(),
      saveFiche: async (p) => void calls.push(`save ${p.justification}`),
    });
    panel.mount();
    panel.setView(view({ ready: true, blocked: null }), "apps|G01");
    (e.ficheToggle as unknown as FakeNode).fire("click");
    await settle();
    const fiche = e.fiche as unknown as FakeNode;
    fiche.byClass("fiche-justification")[0].value = "brouillon";
    (e.verify as unknown as FakeNode).fire("click");
    await settle();
    expect(calls).toEqual(["save brouillon", "verify"]);
    // a wrong shape: nothing saved, nothing verified, the message under the button
    fiche.byClass("fiche-add")[1].fire("click"); // « Ajouter une trame »
    fiche.byClass("fiche-id")[0].value = "0x900";
    calls.length = 0;
    (e.verify as unknown as FakeNode).fire("click");
    await settle();
    expect(calls).toEqual([]);
    expect(e.blocked.textContent).toContain("pas au bon format");
  });

  it("closes the fiche of another pack and keeps it when only the view changes", async () => {
    const e = elements();
    const panel = new ProgressPanel(e, quiet);
    panel.mount();
    panel.setView(view(), "apps|G01");
    (e.ficheToggle as unknown as FakeNode).fire("click");
    await settle();
    expect(e.fiche.hidden).toBe(false);
    panel.setView(view({ feedback: ["y"] }), "apps|G01");
    expect(e.fiche.hidden).toBe(false);
    panel.setView(view(), "apps|G02");
    expect(e.fiche.hidden).toBe(true);
    expect((e.fiche as unknown as FakeNode).children).toEqual([]);
    expect(e.ficheToggle.textContent).toBe("Ouvrir la fiche");
  });

  it("shows the verdict lines as given and hides itself without progression", () => {
    const e = elements();
    const panel = new ProgressPanel(e, quiet);
    panel.mount();
    const lines = ["Échec à corriger.", "Fiche nominale : conforme", "Essai nominal : non réussi", "Le détail n'est pas communiqué. Reprenez vos mesures et vos essais."];
    panel.setView(view({ feedback: lines }));
    expect((e.feedback as unknown as FakeNode).children.map((c) => c.textContent)).toEqual(lines);
    panel.setView(null);
    expect(e.section.hidden).toBe(true);
  });
});

describe("fiche form", () => {
  function filled(): NominalPart {
    const p = emptyPart();
    p.documents_utilises.push({ document: "doc-1", pages: "p. 3" });
    p.trames.push({
      identifiant: 0x7a5, format: "STANDARD_11_BITS", dlc: 8, periode_ms: 20,
      protection: { presente: true, crc: { ordre: "INTEL", lsb_bit: 0, longueur: 8 }, compteur: { ordre: "INTEL", lsb_bit: 8, longueur: 4 } },
      compteur_de_vie: { ordre: null, lsb_bit: null, longueur: null },
    });
    p.signaux.push({
      nom: "course", identifiant: 0x7a5, slots: [0, 1], ordre: "MOTOROLA", lsb_bit: 28, longueur: 12, signe: "NON_SIGNÉ", facteur: 0.1,
      offset: -5, code_invalide: "AUCUN",
    });
    p.statuts.push({ identifiant: 0x7a5, slots: [1], ordre: "INTEL", lsb_bit: 12, longueur: 2, code_ok: 0 });
    p.supervision = { premier_timeout_ms: 240, timeout_ms: 120, peremption_ms: 80, pertes_consecutives_tolerees: 2 };
    p.decisions = ["PUBLIER", "UTILISER_SES_DONNÉES", "BORNER", "PUBLIER_LA_DERNIÈRE"];
    p.justification = "libre";
    return p;
  }

  it("round-trips a filled part through the inputs", () => {
    const root = new FakeNode("div") as unknown as HTMLElement;
    const form = new FicheForm(root, async () => {});
    form.render(filled(), [{ id: "doc-1", title: "Norme A" }], true);
    expect(form.read()).toBe(true);
    expect(form.current()).toEqual(filled());
  });

  it("refuses a wrong shape instead of saving it", async () => {
    const root = new FakeNode("div");
    let saves = 0;
    const form = new FicheForm(root as unknown as HTMLElement, async () => {
      saves++;
    });
    form.render(filled(), [], true);
    const id = root.byClass("fiche-id")[0];
    id.value = "0x900";
    expect(form.read()).toBe(false);
    root.byClass("fiche-save")[0].fire("click");
    await Promise.resolve();
    expect(saves).toBe(0);
    expect(root.byClass("fiche-status")[0].textContent).toContain("pas au bon format");
  });

  it("keeps a validated part read-only", () => {
    const root = new FakeNode("div");
    const form = new FicheForm(root as unknown as HTMLElement, async () => {});
    form.render(filled(), [], false, true);
    expect(root.byClass("fiche-id").every((i) => i.disabled)).toBe(true);
    expect(root.byClass("fiche-add")).toEqual([]);
    expect(root.byClass("fiche-remove")).toEqual([]);
    expect(root.textContent).toContain("Partie validée");
  });
});

describe("module rules", () => {
  it("never writes HTML and never shows a score, a counter or a percentage", () => {
    const sinks = new RegExp(["inner" + "HTML", "outer" + "HTML", "insertAdjacent" + "HTML", "document" + "\\.write"].join("|"));
    expect(progressSrc).not.toMatch(sinks);
    // in the strings the module can display (the header comment states the rule itself)
    expect(progressSrc).not.toMatch(/(["'`])[^"'`\n]*\b(score|points|pourcentage|essais? restants?)\b[^"'`\n]*\1/i);
    expect(progressSrc).not.toMatch(/%\s*\$\{/);
  });
});

describe("incident part", () => {
  const incident = (over: Partial<IncidentView> = {}): IncidentView => ({
    rank: 1,
    editable: true,
    grandeur: null,
    units: [],
    hint: "Relevez cette grandeur dans des conditions où l'anomalie se manifeste, sans bouger les curseurs avant d'appuyer sur Consigner.",
    consigned: null,
    categories: [{ value: "MESSAGE/E2E", definition: "déf. message" }, { value: "CAN_BUS/PROTOCOL", definition: "déf. capteur" }],
    states: [{ value: "PROTECTION_NON_CONFORME", definition: "déf. protection" }, { value: "VALEUR_HORS_PLAGE", definition: "déf. défaut" }],
    rules: [
      { state: "PROTECTION_NON_CONFORME", component: "proportion (composant)", other: "proportion", units: ["%", "fraction"] },
      { state: "VALEUR_HORS_PLAGE", component: "tension (composant)", other: "tension", units: ["V", "mV"] },
    ],
    parts: ["Q3", "R7"],
    pins: ["Q3.in", "R7.p2"],
    ...over,
  });

  async function openIncident(actions: Partial<typeof quiet> = {}): Promise<{ e: ReturnType<typeof elements>; fiche: FakeNode; calls: string[] }> {
    const e = elements();
    const calls: string[] = [];
    const panel = new ProgressPanel(e, {
      ...quiet,
      verify: async () => void calls.push("verify"),
      saveIncident: async (p) => void calls.push(`save ${JSON.stringify([p.cible, p.categorie, p.etat_observe, p.mesure_cle.valeur, p.mesure_cle.unite])}`),
      consign: async () => void calls.push("consign"),
      ...actions,
    });
    panel.mount();
    panel.setView(view({ ready: true, blocked: null, current: 1, incident: incident() }), "apps|G01");
    (e.ficheToggle as unknown as FakeNode).fire("click");
    await settle();
    return { e, fiche: e.fiche as unknown as FakeNode, calls };
  }

  it("designates a conductor by two pins, imposes the quantity of the declared state, saves before verifying", async () => {
    const { e, fiche, calls } = await openIncident();
    expect(fiche.byClass("fiche-title")[0].textContent).toBe("Fiche — incident 1");
    const kind = fiche.byClass("incident-target-kind")[0];
    kind.value = "CONDUCTEUR";
    kind.fire("change");
    fiche.byClass("incident-pin-a")[0].value = "R7.p2";
    fiche.byClass("incident-pin-b")[0].value = "Q3.in";
    fiche.byClass("incident-category")[0].value = "CAN_BUS/PROTOCOL";
    const state = fiche.byClass("incident-state")[0];
    state.value = "VALEUR_HORS_PLAGE";
    state.fire("change");
    expect(fiche.byClass("incident-quantity")[0].textContent).toBe("Grandeur imposée : tension.");
    expect(fiche.byClass("incident-definition")[1].textContent).toBe("déf. défaut");
    expect((fiche.byClass("incident-unit")[0] as unknown as FakeSelect).options.map((o) => o.value)).toEqual(["", "V", "mV"]);
    fiche.byClass("incident-value")[0].value = "120";
    fiche.byClass("incident-unit")[0].value = "mV";
    (e.verify as unknown as FakeNode).fire("click");
    await settle();
    expect(calls).toEqual([
      `save ${JSON.stringify([{ type: "CONDUCTEUR", conducteur: { a: "R7.p2", b: "Q3.in" }, composant: null, identifiant: null }, "CAN_BUS/PROTOCOL", "VALEUR_HORS_PLAGE", 120, "mV"])}`,
      "verify",
    ]);
  });

  it("records the measure after saving the declared fields, and refuses a wrong identifier", async () => {
    const { fiche, calls } = await openIncident();
    const kind = fiche.byClass("incident-target-kind")[0];
    kind.value = "IDENTIFIANT";
    kind.fire("change");
    fiche.byClass("incident-identifier")[0].value = "0x7a5";
    fiche.byClass("incident-consign")[0].fire("click");
    await settle();
    expect(calls).toEqual([`save ${JSON.stringify([{ type: "IDENTIFIANT", conducteur: { a: null, b: null }, composant: null, identifiant: "0x7A5" }, null, null, null, null])}`, "consign"]);
    calls.length = 0;
    fiche.byClass("incident-identifier")[0].value = "0x900";
    fiche.byClass("incident-consign")[0].fire("click");
    await settle();
    expect(calls).toEqual([]);
    expect(fiche.byClass("fiche-status")[0].textContent).toContain("pas au bon format");
  });

  it("follows the core: a recorded measure is shown, a validated or running part is read-only", async () => {
    const e = elements();
    const panel = new ProgressPanel(e, quiet);
    panel.mount();
    panel.setView(view({ current: 1, incident: incident() }), "apps|G01");
    (e.ficheToggle as unknown as FakeNode).fire("click");
    await settle();
    const fiche = e.fiche as unknown as FakeNode;
    expect(fiche.byClass("incident-consigned")[0].textContent).toBe("Mesure non consignée.");
    panel.setView(view({ current: 1, incident: incident({ consigned: "le 13/10/2026 à 10:42", editable: false }) }), "apps|G01");
    expect(fiche.byClass("incident-consigned")[0].textContent).toBe("Mesure consignée le 13/10/2026 à 10:42.");
    expect(fiche.byClass("incident-consign")[0].disabled).toBe(true);
    // the next incident: the form of incident 1 closes
    panel.setView(view({ current: 2, incident: incident({ rank: 2 }) }), "apps|G01");
    expect(e.fiche.hidden).toBe(true);
  });
});

describe("closure", () => {
  it("offers the final fiche once the mission is closed, and shows a refusal of the core", async () => {
    const e = elements();
    let opened = 0;
    const panel = new ProgressPanel(e, { ...quiet, openFinal: async () => void opened++ });
    panel.mount();
    panel.setView(view());
    expect(e.final.hidden).toBe(true);
    panel.setView(view({ current: null, fiche_state: "Exportée le 13/10/2026 à 12:00" }));
    expect(e.final.hidden).toBe(false);
    expect(e.verify.hidden).toBe(true);
    (e.final as unknown as FakeNode).fire("click");
    await settle();
    expect(opened).toBe(1);
    const failing = new ProgressPanel(elements(), { ...quiet, openFinal: async () => Promise.reject("Dossier Documents introuvable.") });
    const e2 = elements();
    const p2 = new ProgressPanel(e2, { ...quiet, openFinal: async () => Promise.reject("Dossier Documents introuvable.") });
    void failing;
    p2.mount();
    p2.setView(view({ current: null }));
    (e2.final as unknown as FakeNode).fire("click");
    await settle();
    expect(e2.blocked.textContent).toBe("Dossier Documents introuvable.");
  });
});

describe("incident part, kept edits", () => {
  const incidentView = (over: Partial<IncidentView> = {}): IncidentView => ({
    rank: 1, editable: true, grandeur: null, units: [], hint: "h", consigned: null,
    categories: [{ value: "MESSAGE/E2E", definition: "d" }], states: [{ value: "PROTECTION_NON_CONFORME", definition: "d" }],
    rules: [{ state: "PROTECTION_NON_CONFORME", component: "p", other: "p", units: ["%", "fraction"] }],
    parts: ["Q3", "R7"], pins: ["Q3.in", "R7.p2"], ...over,
  });

  it("keeps a saved pin that the bench list does not offer, and every edit across a redraw with a wrong shape", async () => {
    const e = elements();
    const saved = { ...emptyIncidentPart(1), cible: { type: "CONDUCTEUR" as const, conducteur: { a: "X9.k", b: "R7.p2" }, composant: null, identifiant: null } };
    const panel = new ProgressPanel(e, { ...quiet, loadIncident: async () => saved });
    panel.mount();
    panel.setView(view({ current: 1, incident: incidentView({ pins: [] }) }), "apps|G01");
    (e.ficheToggle as unknown as FakeNode).fire("click");
    await settle();
    const fiche = e.fiche as unknown as FakeNode;
    expect(fiche.byClass("incident-pin-a")[0].value).toBe("X9.k");
    expect((fiche.byClass("incident-pin-a")[0] as unknown as FakeSelect).options.map((o) => o.textContent)).toContain("X9.k (absent du banc)");
    // a wrong value, a category and a justification, then a redraw from the core
    fiche.byClass("incident-value")[0].value = "4,1 V";
    fiche.byClass("incident-category")[0].value = "MESSAGE/E2E";
    fiche.byClass("fiche-justification")[0].value = "mes notes";
    panel.setView(view({ current: 1, incident: incidentView() }), "apps|G01");
    expect(fiche.byClass("incident-value")[0].value).toBe("4,1 V");
    expect(fiche.byClass("incident-category")[0].value).toBe("MESSAGE/E2E");
    expect(fiche.byClass("fiche-justification")[0].value).toBe("mes notes");
    expect(fiche.byClass("incident-pin-a")[0].value).toBe("X9.k");
  });

  it("saves a hidden form before verifying, and freezes the justification during a run", async () => {
    const e = elements();
    const calls: string[] = [];
    const panel = new ProgressPanel(e, {
      ...quiet,
      verify: async () => void calls.push("verify"),
      saveIncident: async (p) => void calls.push(`save ${p.justification}`),
    });
    panel.mount();
    panel.setView(view({ ready: true, blocked: null, current: 1, incident: incidentView() }), "apps|G01");
    (e.ficheToggle as unknown as FakeNode).fire("click");
    await settle();
    const fiche = e.fiche as unknown as FakeNode;
    fiche.byClass("fiche-justification")[0].value = "brouillon";
    (e.ficheToggle as unknown as FakeNode).fire("click"); // « Fermer la fiche »
    expect(e.fiche.hidden).toBe(true);
    (e.verify as unknown as FakeNode).fire("click");
    await settle();
    expect(calls).toEqual(["save brouillon", "verify"]);
    panel.setView(view({ running: true, current: 1, incident: incidentView({ editable: false }) }), "apps|G01");
    expect(fiche.byClass("fiche-justification")[0].disabled).toBe(true);
  });
});

describe("« Reproduire l'anomalie » (R-11)", () => {
  const incident: IncidentView = {
    rank: 2, editable: true, grandeur: null, units: [], hint: "h", consigned: null, categories: [], states: [], rules: [],
    parts: [], pins: [],
  };

  it("is offered in an incident step only, confirmed by M-18, and refused during a run", async () => {
    const e = elements();
    const calls: string[] = [];
    const panel = new ProgressPanel(e, { ...quiet, reproduce: async () => void calls.push("reproduce") });
    panel.mount();
    panel.setView(view(), "apps|G01");
    expect(e.reproduce.hidden).toBe(true);
    panel.setView(view({ current: 2, incident }), "apps|G01");
    expect(e.reproduce.hidden).toBe(false);
    expect(e.reproduce.disabled).toBe(false);
    // cancelled: nothing happens
    (e.reproduce as unknown as FakeNode).fire("click");
    expect(e.reproduceDialog.open).toBe(true);
    (e.reproduceCancel as unknown as FakeNode).fire("click");
    expect(e.reproduceDialog.open).toBe(false);
    expect(calls).toEqual([]);
    // confirmed
    (e.reproduce as unknown as FakeNode).fire("click");
    (e.reproduceConfirm as unknown as FakeNode).fire("click");
    await settle();
    expect(e.reproduceDialog.open).toBe(false);
    expect(calls).toEqual(["reproduce"]);
    panel.setView(view({ current: 2, running: true, incident: { ...incident, editable: false } }), "apps|G01");
    expect(e.reproduce.disabled).toBe(true);
  });

  it("shows the core's refusal", async () => {
    const e = elements();
    const panel = new ProgressPanel(e, { ...quiet, reproduce: async () => Promise.reject("Le banc de la mission n'est plus ouvert.") });
    panel.mount();
    panel.setView(view({ current: 1, incident: { ...incident, rank: 1 } }), "apps|G01");
    (e.reproduce as unknown as FakeNode).fire("click");
    (e.reproduceConfirm as unknown as FakeNode).fire("click");
    await settle();
    expect((e.blocked as unknown as FakeNode).textContent + (e.feedback as unknown as FakeNode).textContent).toContain("n'est plus ouvert");
  });
});

// ------------------------------------------------------------------------------------- redesign: panel structure

/** A property the fake DOM does not declare (ARIA reflection, title): read as the page would. */
const prop = (n: unknown, name: string): unknown => (n as Record<string, unknown>)[name];

const CPS = [
  "Recevoir et filtrer les trames de l'ECU", "Décoder les valeurs nominales", "Respecter les limites et les valeurs invalides",
  "Contrôler l'intégrité des messages", "Superviser fraîcheur et séquence", "Traiter un défaut signalé par la source",
  "Appliquer la logique de sécurité de la mission", "Reprendre après un défaut",
];
const cpLines = (fail: number[] = []): string[] => CPS.map((l, k) => `CP-${k + 1} — ${l} : ${fail.includes(k + 1) ? "FAIL" : "PASS"}`);
const stepsOf = (states: string[], current: number | null) =>
  NAMES.map((name, i) => ({ name, state: states[i] ?? "À FAIRE", since: null, current: i === current }));
const ALL_PASSED = ["VALIDÉ", "VALIDÉ", "VALIDÉ", "VALIDÉ", "VALIDÉ"];
const RUNNING = "Essai en cours — 28 s. Ne débranchez pas la carte et ne fermez pas l'application.";
const frozenIncident = (): IncidentView => ({
  rank: 1, editable: false, grandeur: null, units: [], hint: "", consigned: null, categories: [], states: [], rules: [], parts: [], pins: [],
});

describe("redesign: verdict model", () => {
  it("pins the public texts of the core (workflow/session.rs)", () => {
    expect(DETAIL_HIDDEN).toBe("Le détail n'est pas communiqué. Reprenez vos mesures et vos essais.");
    expect(CLOSED_LINE).toBe("Mission clôturée.");
    expect(EXPORTED).toBe("La fiche finale est exportée dans le dossier Documents.");
    expect(NOT_EXPORTED).toBe("La fiche finale n'a pas pu être exportée : utilisez « Ouvrir la fiche finale » pour réessayer.");
    expect(NOT_SAVED).toBe("La progression n'a pas pu être enregistrée sur ce poste : prévenez l'encadrant.");
  });

  it("reads an E0 failure: a warn block, a head, two parts toned by their value, the hint", () => {
    const lines = ["Échec à corriger.", "Fiche nominale : non conforme", "Essai nominal : réussi", DETAIL_HIDDEN];
    const m = feedbackModel(lines, stepsOf(["ÉCHEC À CORRIGER"], 0), 0);
    expect(m.tone).toBe("warn");
    expect(m.lines.map((l) => l.kind)).toEqual(["head", "part", "part", "hint"]);
    expect(m.lines.map((l) => l.tone)).toEqual(["warn", "warn", "ok", undefined]);
    expect(m.lines[1].parts).toEqual(["Fiche nominale", " : ", "non conforme"]);
    expect(m.lines.map((l) => (l.parts ? l.parts.join("") : l.text))).toEqual(lines);
  });

  it("reads a pass, the qualification, the closure and the export", () => {
    const pass = feedbackModel(
      ["Vérification réussie. Étape validée le 15/10/2026 à 14:05.", "Incident 1 ouvert. Le véhicule revient d'essai avec une anomalie."],
      stepsOf(["VALIDÉ", "EN COURS"], 1), 1,
    );
    expect([pass.tone, pass.lines.map((l) => l.kind)]).toEqual(["ok", ["head", "next"]]);
    const e4 = feedbackModel(cpLines([3, 5]), stepsOf(["VALIDÉ", "VALIDÉ", "VALIDÉ", "VALIDÉ", "ÉCHEC À CORRIGER"], 4), 4);
    expect(e4.tone).toBe("warn");
    expect(e4.lines.every((l) => l.kind === "cp")).toBe(true);
    expect(e4.lines.filter((l) => l.tone === "warn").length).toBe(2);
    const closed = feedbackModel([...cpLines(), CLOSED_LINE, EXPORTED], stepsOf(ALL_PASSED, null), null);
    expect([closed.tone, closed.lines.slice(-2).map((l) => [l.kind, l.tone])]).toEqual(["ok", [["closure", "ok"], ["export", "ok"]]]);
    // on the verdict lines alone (no step data): all PASS and the closure
    expect(feedbackModel([...cpLines(), CLOSED_LINE], [], 4).tone).toBe("ok");
    expect(feedbackModel(cpLines([1]), [], 4).tone).toBe("warn");
    const failedExport = feedbackModel([...cpLines(), CLOSED_LINE, NOT_EXPORTED], stepsOf(ALL_PASSED, null), null);
    expect(failedExport.lines[9]).toMatchObject({ kind: "export", tone: "err" });
  });

  it("reads an interruption, an unsaved progression and an unknown line", () => {
    const stopped = ["Vérification interrompue (banc fermé). Elle n'est pas comptée. Relancez quand le banc est prêt."];
    expect(feedbackModel(stopped, stepsOf(["VALIDÉ", "EN COURS"], 1), 1)).toMatchObject({ tone: "info", lines: [{ kind: "head", tone: "info" }] });
    const unsaved = ["Vérification interrompue (progression non enregistrée). Elle n'est pas comptée.", NOT_SAVED];
    expect(feedbackModel(unsaved, stepsOf(["EN COURS"], 0), 0).tone).toBe("err");
    expect(feedbackModel(["Une ligne inconnue."], stepsOf(["EN COURS"], 0), 0)).toEqual({ tone: null, lines: [{ text: "Une ligne inconnue.", kind: "text" }] });
  });

  it("draws one line per core line, exact text, CP values as badges, the block toned", () => {
    const e = elements();
    const panel = new ProgressPanel(e, quiet);
    panel.mount();
    const lines = cpLines([2]);
    panel.setView(view({ steps: stepsOf(["VALIDÉ", "VALIDÉ", "VALIDÉ", "VALIDÉ", "ÉCHEC À CORRIGER"], 4), current: 4, feedback: lines }));
    const fb = e.feedback as unknown as FakeNode;
    expect(fb.children.map((c) => c.textContent)).toEqual(lines);
    expect(fb.className).toBe("progress-feedback is-warn");
    expect(fb.children.map((c) => c.className)).toEqual(lines.map(() => "feedback-line"));
    expect(fb.byClass("fl-value").map((v) => v.className)).toEqual(lines.map((_, k) => `fl-value badge badge--${k === 1 ? "warn" : "ok"}`));
    expect(fb.children[1].dataset).toEqual({ kind: "cp", tone: "warn" });
  });

  it("rebuilds the verdict only when its lines change, and brings a new one into view", () => {
    const e = elements();
    const revealed: [unknown, boolean][] = [];
    const panel = new ProgressPanel(e, { ...quiet, reveal: (el, focus) => void revealed.push([el, focus]) });
    panel.mount();
    const lines = ["Échec à corriger.", "Fiche nominale : conforme", "Essai nominal : non réussi", DETAIL_HIDDEN];
    panel.setView(view({ running: true, ready: false, blocked: RUNNING }));
    panel.setView(view({ feedback: lines, steps: stepsOf(["ÉCHEC À CORRIGER"], 0) }));
    const first = (e.feedback as unknown as FakeNode).children[0];
    expect(revealed).toEqual([[e.feedback, true]]);
    panel.setView(view({ feedback: lines, steps: stepsOf(["ÉCHEC À CORRIGER"], 0), blocked: "Prochaine vérification possible dans 1 min 40 s." }));
    expect((e.feedback as unknown as FakeNode).children[0]).toBe(first);
    expect(revealed.length).toBe(1);
  });
});

describe("redesign: action bar and dock", () => {
  it("marks the steps as a list, names the current step in the consigne, E5 at the end", () => {
    const e = { ...elements(), briefStep: new FakeNode("span") as unknown as HTMLElement };
    const panel = new ProgressPanel(e, quiet);
    panel.mount();
    panel.setView(view());
    const boxes = (e.steps as unknown as FakeNode).children;
    expect(boxes.length).toBe(6);
    expect(boxes.map((b) => prop(b, "role"))).toEqual(Array(6).fill("listitem"));
    expect(boxes.map((b) => prop(b, "ariaCurrent"))).toEqual(["step", undefined, undefined, undefined, undefined, undefined]);
    expect(e.briefStep.textContent).toBe("E0 · Essai nominal");
    panel.setView(view({ current: null, steps: stepsOf(ALL_PASSED, null) }));
    expect(e.briefStep.textContent).toBe("E5 · Clôture");
  });

  it("words the message under the button by state, mirrors it in the disabled button's title", () => {
    const e = elements();
    const panel = new ProgressPanel(e, quiet);
    panel.mount();
    panel.setView(view());
    expect([e.blocked.className, prop(e.blocked, "ariaLive"), e.verify.title]).toEqual(["progress-blocked is-warn", "polite", view().blocked]);
    panel.setView(view({ running: true, blocked: RUNNING, current: 1, incident: frozenIncident() }));
    expect([e.blocked.className, prop(e.blocked, "ariaLive"), e.verify.title, e.reproduce.title]).toEqual(["progress-blocked is-run", "off", RUNNING, RUNNING]);
    panel.setView(view({ ready: true, blocked: null }));
    expect(e.verify.title).toBe("");
  });

  it("gives the primary emphasis to the fiche toggle only when the verification cannot run and the part is editable", async () => {
    const e = elements();
    const panel = new ProgressPanel(e, quiet);
    panel.mount();
    panel.setView(view(), "apps|G01");
    expect(e.ficheToggle.className).toBe("btn btn--primary ico ico--doc");
    (e.ficheToggle as unknown as FakeNode).fire("click");
    await settle();
    expect(e.ficheToggle.className).toBe("btn ico ico--doc"); // the fiche is open
    (e.ficheToggle as unknown as FakeNode).fire("click");
    expect(e.ficheToggle.className).toBe("btn btn--primary ico ico--doc");
    panel.setView(view({ ready: true, blocked: null }), "apps|G01");
    expect(e.ficheToggle.className).toBe("btn ico ico--doc"); // the verification can run
    panel.setView(view({ running: true, ready: false }), "apps|G01");
    expect(e.ficheToggle.className).toBe("btn ico ico--doc");
    panel.setView(view({ nominal_editable: false, current: 4 }), "apps|G01");
    expect(e.ficheToggle.className).toBe("btn ico ico--doc"); // nothing left to write
    panel.setView(view({ current: null }), "apps|G01");
    expect(e.ficheToggle.className).toBe("btn ico ico--doc");
  });

  it("asks the dock for the fiche tab on open, gives it back on close, and follows the dock's tabs", async () => {
    const e = { ...elements(), ficheTab: new FakeNode("button") as unknown as HTMLElement };
    e.fiche.hidden = true; // as in index.html
    const calls: string[] = [];
    let loads = 0;
    const panel: ProgressPanel = new ProgressPanel(e, {
      ...quiet,
      loadFiche: async () => (loads++, emptyPart()),
      // the dock answers with a tab change, as main.ts does: re-entrant, and nothing more for the panel to do
      showFiche: () => (calls.push("show"), panel.tabShown(true)),
      hideFiche: () => (calls.push("hide"), panel.tabShown(false)),
    });
    panel.mount();
    expect(e.ficheTab.hidden).toBe(true);
    panel.setView(view(), "apps|G01");
    expect(e.ficheTab.hidden).toBe(false);
    (e.ficheToggle as unknown as FakeNode).fire("click");
    await settle();
    expect([calls, loads, e.fiche.hidden, e.ficheToggle.textContent]).toEqual([["show"], 1, false, "Fermer la fiche"]);
    const fiche = e.fiche as unknown as FakeNode;
    fiche.byClass("fiche-justification")[0].value = "mes notes";
    // another dock tab: hidden, kept, nothing asked back
    panel.tabShown(false);
    expect([calls, e.fiche.hidden, e.ficheToggle.textContent]).toEqual([["show"], true, "Ouvrir la fiche"]);
    // the « Fiche » tab again: shown with its edits, nothing loaded again
    panel.tabShown(true);
    await settle();
    expect([loads, e.fiche.hidden, fiche.byClass("fiche-justification")[0].value]).toEqual([1, false, "mes notes"]);
    (e.ficheToggle as unknown as FakeNode).fire("click");
    expect(calls).toEqual(["show", "show", "hide"]);
    // the pack closes: the fiche goes, its tab gives way and hides
    panel.tabShown(true);
    await settle();
    panel.setView(null);
    expect([calls.at(-1), e.fiche.hidden, e.ficheTab.hidden]).toEqual(["hide", true, true]);
    // the « Fiche » tab with no fiche loaded yet: one load, even when asked twice
    const e2 = elements();
    let loads2 = 0;
    const p2 = new ProgressPanel(e2, { ...quiet, loadFiche: async () => (loads2++, emptyPart()) });
    p2.mount();
    p2.setView(view(), "apps|G01");
    p2.tabShown(true);
    p2.tabShown(true);
    await settle();
    expect([loads2, e2.fiche.hidden, e2.ficheToggle.textContent]).toEqual([1, false, "Fermer la fiche"]);
  });

  it("consigns from the bench through the open incident form (saved first), or directly", async () => {
    const incident: IncidentView = { ...frozenIncident(), editable: true };
    const calls: string[] = [];
    const e = elements();
    const panel = new ProgressPanel(e, { ...quiet, saveIncident: async () => void calls.push("save"), consign: async () => void calls.push("consign") });
    panel.mount();
    panel.setView(view({ current: 1, incident }), "apps|G01");
    await panel.consignFromBench();
    expect(calls).toEqual(["consign"]);
    (e.ficheToggle as unknown as FakeNode).fire("click");
    await settle();
    calls.length = 0;
    await panel.consignFromBench();
    expect(calls).toEqual(["save", "consign"]);
    // a wrongly shaped identifier: refused, nothing sent
    const fiche = e.fiche as unknown as FakeNode;
    const kind = fiche.byClass("incident-target-kind")[0];
    kind.value = "IDENTIFIANT";
    kind.fire("change");
    fiche.byClass("incident-identifier")[0].value = "0x900";
    calls.length = 0;
    await expect(panel.consignFromBench()).rejects.toBe(SHAPE_REFUSED);
    expect(calls).toEqual([]);
  });

  it("words the status-bar segment by state", () => {
    expect(progressStatus(null)).toBeNull();
    expect(progressStatus(view())).toEqual({ text: "E0 · Essai nominal — EN COURS", tone: "", title: view().blocked });
    expect(progressStatus(view({ running: true, blocked: "Qualification en cours — 480 s." }))).toMatchObject({ tone: "accent", title: "Qualification en cours — 480 s." });
    expect(progressStatus(view({ steps: stepsOf(["ÉCHEC À CORRIGER"], 0) }))?.tone).toBe("warn");
    expect(progressStatus(view({ current: null, blocked: null, steps: stepsOf(ALL_PASSED, null) }))).toEqual({ text: "E5 · Clôture", tone: "ok", title: "" });
  });
});

// ------------------------------------------------------------------------------- redesign: the fiche document

/** Every node under `n`, depth first. */
const walk = (n: FakeNode): FakeNode[] => n.children.flatMap((c) => [c, ...walk(c)]);
const tokens = (n: FakeNode): string[] => n.className.split(" ");
const ancestors = (n: FakeNode): FakeNode[] => (n.parent ? [n.parent, ...ancestors(n.parent)] : []);
const FIELDS = ["input", "select", "textarea"];
/** The format error under a field (its label.ff holds it). */
const errorOf = (c: FakeNode): FakeNode => c.parent!.byClass("ff-error")[0];

function filledPart(): NominalPart {
  const p = emptyPart();
  p.documents_utilises.push({ document: "doc-1", pages: "p. 3" });
  p.trames.push({
    identifiant: 0x7a5, format: "STANDARD_11_BITS", dlc: 8, periode_ms: 20,
    protection: { presente: true, crc: { ordre: "INTEL", lsb_bit: 0, longueur: 8 }, compteur: { ordre: "INTEL", lsb_bit: 8, longueur: 4 } },
    compteur_de_vie: { ordre: null, lsb_bit: null, longueur: null },
  });
  p.signaux.push({
    nom: "course", identifiant: 0x7a5, slots: [0, 1], ordre: "MOTOROLA", lsb_bit: 28, longueur: 12, signe: "NON_SIGNÉ", facteur: 0.1,
    offset: -5, code_invalide: "AUCUN",
  });
  p.statuts.push({ identifiant: 0x7a5, slots: [1], ordre: "INTEL", lsb_bit: 12, longueur: 2, code_ok: 0 });
  p.supervision = { premier_timeout_ms: 240, timeout_ms: 120, peremption_ms: 80, pertes_consecutives_tolerees: 2 };
  p.decisions = ["PUBLIER", "UTILISER_SES_DONNÉES", "BORNER", "PUBLIER_LA_DERNIÈRE"];
  p.justification = "libre";
  return p;
}

const incidentOf = (over: Partial<IncidentView> = {}): IncidentView => ({
  rank: 1, editable: true, grandeur: null, units: [], hint: "h", consigned: null,
  categories: [{ value: "MESSAGE/E2E", definition: "d" }], states: [{ value: "PROTECTION_NON_CONFORME", definition: "d" }],
  rules: [{ state: "PROTECTION_NON_CONFORME", component: "p", other: "p", units: ["%", "fraction"] }],
  parts: ["Q3", "R7"], pins: ["Q3.in", "R7.p2"], ...over,
});

function nominalForm(save: (p: NominalPart) => Promise<void> = async () => {}, reveal?: (el: HTMLElement, focus: boolean) => void) {
  const root = new FakeNode("div");
  const form = new FicheForm(root as unknown as HTMLElement, save, { reveal });
  form.render(filledPart(), [{ id: "doc-1", title: "Norme A" }], true);
  return { root, form };
}

async function incidentPanel(over: Partial<IncidentView> = {}) {
  const e = elements();
  const calls: string[] = [];
  const panel = new ProgressPanel(e, {
    ...quiet,
    verify: async () => void calls.push("verify"),
    saveIncident: async (p) => void calls.push(`save ${p.mesure_cle.valeur}`),
    consign: async () => void calls.push("consign"),
  });
  panel.mount();
  const inc = incidentOf(over);
  panel.setView(view({ ready: true, blocked: null, current: 1, incident: inc }), "apps|G01");
  (e.ficheToggle as unknown as FakeNode).fire("click");
  await settle();
  return { e, panel, inc, calls, fiche: e.fiche as unknown as FakeNode };
}

describe("redesign: the fiche as a document", () => {
  afterEach(() => vi.useRealTimers());

  it("has a head with the index of the seven sections, numbered sections of named records, and a foot", () => {
    const { root } = nominalForm();
    expect(root.children.map((c) => c.tagName)).toEqual(["header", "div", "footer"]);
    expect(root.byClass("fiche-nav-item").map((b) => b.textContent)).toEqual([
      "Documents utilisés", "Trames", "Signaux", "Statuts", "Supervision de la trame de données", "Décisions de conception", "Justification",
    ]);
    expect(root.byClass("fiche-section").map((h) => h.textContent)).toEqual([
      "Documents utilisés", "Trames", "Signaux", "Statuts", "Supervision de la trame de données", "Décisions de conception",
      "Justification (non évaluée automatiquement)",
    ]);
    expect(root.byClass("fiche-add").map((b) => b.textContent)).toEqual(["Ajouter un document", "Ajouter une trame", "Ajouter un signal", "Ajouter un statut"]);
    const records = root.byClass("fiche-record");
    expect(records.map((r) => [prop(r, "role"), prop(r, "ariaLabel")])).toEqual([
      ["group", "Document 1"], ["group", "Trame 1"], ["group", "Signal 1"], ["group", "Statut 1"],
    ]);
    expect(root.byClass("fiche-record-id").map((s) => s.textContent)).toEqual(["0x7A5", "0x7A5", "0x7A5"]);
    expect(root.byClass("ff-legend").map((l) => l.textContent)).toEqual(["CRC de protection", "Compteur de protection", "Compteur de vie", "Position", "Position"]);
    expect([root.byClass("fiche-save").length, root.byClass("fiche-status").length, prop(root.byClass("fiche-status")[0], "role")]).toEqual([1, 1, "status"]);
    expect(root.byClass("fiche-id")[0].value).toBe("0x7A5");
  });

  it("gives every control its design-system class and an accessible name (its label, or an aria-label)", async () => {
    const { root } = nominalForm();
    const { fiche } = await incidentPanel();
    const kind = fiche.byClass("incident-target-kind")[0];
    kind.value = "CONDUCTEUR";
    kind.fire("change");
    const need: Record<string, string> = { button: "btn", input: "input", select: "select", textarea: "textarea" };
    for (const n of [...walk(root), ...walk(fiche)]) {
      if (!need[n.tagName]) continue;
      expect(tokens(n)).toContain(need[n.tagName]);
      if (FIELDS.includes(n.tagName)) expect(ancestors(n).some((a) => tokens(a).includes("ff")) || !!prop(n, "ariaLabel")).toBe(true);
    }
    expect(fiche.byClass("ff-label").map((l) => l.textContent)).toEqual(["Broche A", "Broche B", "Valeur", "Unité"]);
  });

  it("shows the identifier as typed in the head of its record", () => {
    const { root } = nominalForm();
    const id = root.byClass("fiche-id")[0];
    id.value = "0x1";
    id.fire("input");
    expect(root.byClass("fiche-record-id")[0].textContent).toBe("0x1");
    id.value = "";
    id.fire("input");
    expect(root.byClass("fiche-record-id")[0].textContent).toBe("—");
  });

  it("marks a wrongly shaped field under itself and its section in the index, then brings it forward with the focus", async () => {
    let saves = 0;
    const revealed: [unknown, boolean][] = [];
    const { root, form } = nominalForm(async () => void saves++, (el, focus) => void revealed.push([el, focus]));
    const id = root.byClass("fiche-id")[0];
    id.value = "0x900";
    id.fire("change"); // leaving the field checks its shape
    expect([tokens(id).includes("invalid"), prop(id, "ariaInvalid"), errorOf(id).hidden, errorOf(id).textContent]).toEqual([true, "true", false, FORMAT.identifier]);
    const nav = root.byClass("fiche-nav-item");
    expect(nav.map((n) => n.dataset.invalid)).toEqual([undefined, "true", undefined, undefined, undefined, undefined, undefined]);
    root.byClass("fiche-save")[0].fire("click");
    await settle();
    expect(saves).toBe(0);
    const status = root.byClass("fiche-status")[0];
    expect([status.className, status.textContent]).toEqual(["fiche-status err", SHAPE_REFUSED]);
    // its label and message come into view, then the field takes the focus
    expect(revealed.length).toBe(2);
    expect(revealed[0][0]).toBe(id.parent);
    expect(revealed[1][0]).toBe(id);
    expect(revealed.map((r) => r[1])).toEqual([false, true]);
    // a redraw keeps what was typed, still marked
    form.setDocuments([{ id: "doc-2", title: "Norme B" }]);
    const again = root.byClass("fiche-id")[0];
    expect(again).not.toBe(id);
    expect([again.value, tokens(again).includes("invalid"), errorOf(again).textContent]).toEqual(["0x900", true, FORMAT.identifier]);
    // typing again clears the mark and the index; a good shape saves
    again.value = "0x1A4";
    again.fire("input");
    expect([tokens(again).includes("invalid"), prop(again, "ariaInvalid"), errorOf(again).hidden]).toEqual([false, null, true]);
    expect(root.byClass("fiche-nav-item")[1].dataset.invalid).toBeUndefined();
    root.byClass("fiche-save")[0].fire("click");
    await settle();
    expect(saves).toBe(1);
    expect(root.byClass("fiche-status")[0].className).toBe("fiche-status ok");
    // the other shapes have their own message
    const dlc = root.byClass("fiche-int")[0];
    dlc.value = "9";
    dlc.fire("change");
    expect(errorOf(dlc).textContent).toBe("Entier de 0 à 8.");
    const slots = root.byClass("fiche-slots")[0];
    slots.value = "0, x";
    slots.fire("change");
    expect(errorOf(slots).textContent).toBe(FORMAT.slots);
    expect(root.byClass("fiche-nav-item").map((n) => n.dataset.invalid)).toEqual([undefined, "true", "true", undefined, undefined, undefined, undefined]);
  });

  it("deletes a record in two clicks, the second one within 5 s", () => {
    vi.useFakeTimers();
    const { root, form } = nominalForm();
    const del = (): FakeNode => root.byClass("fiche-remove")[1];
    expect([del().textContent, prop(del(), "ariaLabel")]).toEqual(["Supprimer", "Supprimer la trame 1"]);
    del().fire("click");
    expect([del().className, del().textContent, prop(del(), "ariaLabel")]).toEqual([
      "btn btn--danger fiche-remove", "Confirmer la suppression", "Confirmer la suppression de la trame 1",
    ]);
    expect(form.current().trames.length).toBe(1);
    vi.advanceTimersByTime(5000);
    expect([del().className, del().textContent, prop(del(), "ariaLabel")]).toEqual(["btn btn--ghost fiche-remove", "Supprimer", "Supprimer la trame 1"]);
    del().fire("click");
    del().fire("click");
    expect(root.byClass("fiche-record-name").map((n) => n.textContent)).toEqual(["Document 1", "Signal 1", "Statut 1"]);
    expect(form.current().trames).toEqual([]);
    expect(root.byClass("fiche-dirty")[0].hidden).toBe(false);
  });

  it("adds a record, named after the core's numbering", () => {
    const { root } = nominalForm();
    root.byClass("fiche-add")[1].fire("click");
    expect(root.byClass("fiche-record-name").map((n) => n.textContent)).toEqual(["Document 1", "Trame 1", "Trame 2", "Signal 1", "Statut 1"]);
    expect(root.byClass("fiche-record-id")[1].textContent).toBe("—");
  });
});

describe("redesign: saving and the read-only states", () => {
  afterEach(() => vi.useRealTimers());

  it("marks unsaved changes next to « Enregistrer la fiche » until they are saved, and saves nothing by itself", async () => {
    vi.useFakeTimers();
    const saved: string[] = [];
    let refuse = false;
    const { root, form } = nominalForm(async (p) => {
      if (refuse) throw "Refus du cœur.";
      saved.push(p.justification);
    });
    const dirty = (): FakeNode => root.byClass("fiche-dirty")[0];
    const save = (): FakeNode => root.byClass("fiche-save")[0];
    const status = (): FakeNode => root.byClass("fiche-status")[0];
    expect([dirty().hidden, dirty().textContent, save().className]).toEqual([true, DIRTY, "btn fiche-save"]);
    const just = root.byClass("fiche-justification")[0];
    just.value = "nouvelle";
    just.fire("input");
    expect([dirty().hidden, save().className]).toEqual([false, "btn btn--primary fiche-save"]);
    // no autosave: time passes, a select changes, nothing is sent
    vi.advanceTimersByTime(10_000);
    const decision = root.byClass("fiche-decision")[0];
    decision.value = "REJETER";
    decision.fire("change");
    vi.advanceTimersByTime(10_000);
    await settle();
    expect(saved).toEqual([]);
    // a redraw keeps the marker
    form.setDocuments([{ id: "doc-2", title: "Norme B" }]);
    expect(dirty().hidden).toBe(false);
    // a refusal of the core: still unsaved
    refuse = true;
    save().fire("click");
    await settle();
    expect([status().className, status().textContent, dirty().hidden]).toEqual(["fiche-status err", "Refus du cœur.", false]);
    refuse = false;
    save().fire("click");
    await settle();
    expect(saved).toEqual(["nouvelle"]);
    expect([dirty().hidden, save().className, status().className, status().textContent]).toEqual([true, "btn fiche-save", "fiche-status ok", "Enregistré"]);
    // the next change: « Enregistré » no longer holds
    just.fire("input");
    expect([dirty().hidden, status().textContent]).toEqual([false, ""]);
  });

  it("is saved, and clean again, when the verification saves it", async () => {
    const e = elements();
    const calls: string[] = [];
    const panel = new ProgressPanel(e, {
      ...quiet, loadFiche: async () => filledPart(), verify: async () => void calls.push("verify"), saveFiche: async (p) => void calls.push(`save ${p.justification}`),
    });
    panel.mount();
    panel.setView(view({ ready: true, blocked: null }), "apps|G01");
    (e.ficheToggle as unknown as FakeNode).fire("click");
    await settle();
    const fiche = e.fiche as unknown as FakeNode;
    const just = fiche.byClass("fiche-justification")[0];
    just.value = "à jour";
    just.fire("input");
    expect(fiche.byClass("fiche-dirty")[0].hidden).toBe(false);
    (e.verify as unknown as FakeNode).fire("click");
    await settle();
    expect(calls).toEqual(["save à jour", "verify"]);
    expect(fiche.byClass("fiche-dirty")[0].hidden).toBe(true);
  });

  it("keeps unsaved what is typed while the verification saves the fiche", async () => {
    const e = elements();
    let release!: () => void;
    const panel = new ProgressPanel(e, {
      ...quiet, loadFiche: async () => filledPart(), verify: async () => {},
      saveFiche: () => new Promise<void>((resolve) => { release = resolve; }),
    });
    panel.mount();
    panel.setView(view({ ready: true, blocked: null }), "apps|G01");
    (e.ficheToggle as unknown as FakeNode).fire("click");
    await settle();
    const just = (e.fiche as unknown as FakeNode).byClass("fiche-justification")[0];
    just.value = "avant la vérification";
    just.fire("input");
    (e.verify as unknown as FakeNode).fire("click");
    await settle();
    just.value = "tapé pendant l'enregistrement";
    just.fire("input");
    release();
    await settle();
    expect(panel.unsaved).toBe(true);
  });

  it("tells a close about its unsaved changes and saves them when asked, as « Enregistrer la fiche » does", async () => {
    const e = elements();
    const saved: string[] = [];
    let refuse = false;
    const panel = new ProgressPanel(e, {
      ...quiet, loadFiche: async () => filledPart(), saveFiche: async (p) => {
        if (refuse) throw "Refus du cœur.";
        saved.push(p.justification);
      },
    });
    panel.mount();
    panel.setView(view({ ready: true, blocked: null }), "apps|G01");
    (e.ficheToggle as unknown as FakeNode).fire("click");
    await settle();
    expect(panel.unsaved).toBe(false);
    expect(await panel.saveUnsaved()).toBe(true);
    expect(saved).toEqual([]);
    const fiche = e.fiche as unknown as FakeNode;
    const just = fiche.byClass("fiche-justification")[0];
    just.value = "avant de fermer";
    just.fire("input");
    expect(panel.unsaved).toBe(true);
    refuse = true;
    expect(await panel.saveUnsaved()).toBe(false);
    expect(panel.unsaved).toBe(true);
    refuse = false;
    expect(await panel.saveUnsaved()).toBe(true);
    expect([saved, panel.unsaved, fiche.byClass("fiche-dirty")[0].hidden]).toEqual([["avant de fermer"], false, true]);
  });

  it("freezes everything during a run, and opens read-only once the mission is closed, with the core's own note", async () => {
    const e = elements();
    let saves = 0;
    const panel = new ProgressPanel(e, { ...quiet, loadFiche: async () => filledPart(), saveFiche: async () => void saves++ });
    panel.mount();
    panel.setView(view(), "apps|G01");
    (e.ficheToggle as unknown as FakeNode).fire("click");
    await settle();
    const fiche = e.fiche as unknown as FakeNode;
    const frozen = (): boolean[] => walk(fiche).filter((n) => FIELDS.includes(n.tagName)).map((n) => n.disabled);
    expect(frozen().some(Boolean)).toBe(false);
    expect(fiche.byClass("fiche-note")).toEqual([]);
    panel.setView(view({ running: true, blocked: RUNNING }), "apps|G01");
    expect([fiche.byClass("fiche-note")[0].textContent, fiche.byClass("fiche-save")[0].disabled, frozen().every(Boolean)]).toEqual([RUNNING_NOTE, true, true]);
    // E5: the closed mission (the fiche opened then)
    panel.setView(view({ current: null, nominal_editable: false, steps: stepsOf(ALL_PASSED, null) }), "apps|G01");
    expect(fiche.byClass("fiche-note")[0].textContent).toBe(CLOSED_FICHE);
    expect([fiche.byClass("fiche-save")[0].disabled, fiche.byClass("fiche-justification")[0].disabled, frozen().every(Boolean)]).toEqual([true, true, true]);
    expect([fiche.byClass("fiche-add"), fiche.byClass("fiche-remove")]).toEqual([[], []]);
    fiche.byClass("fiche-save")[0].fire("click");
    await settle();
    expect(saves).toBe(0);
    // the core's text (workflow/session.rs CLOSED_FICHE), pinned
    expect(CLOSED_FICHE).toBe("La mission est clôturée : la fiche n'est plus modifiable.");
    // opened at E5 directly
    const e2 = elements();
    const p2 = new ProgressPanel(e2, { ...quiet, loadFiche: async () => filledPart() });
    p2.mount();
    p2.setView(view({ current: null, nominal_editable: false, steps: stepsOf(ALL_PASSED, null) }), "apps|G01");
    (e2.ficheToggle as unknown as FakeNode).fire("click");
    await settle();
    const f2 = e2.fiche as unknown as FakeNode;
    expect([f2.byClass("fiche-note")[0].textContent, f2.byClass("fiche-save")[0].disabled]).toEqual([CLOSED_FICHE, true]);
    expect(walk(f2).filter((n) => FIELDS.includes(n.tagName)).every((n) => n.disabled)).toBe(true);
  });
});

describe("redesign: the incident form", () => {
  it("patches the form in place when only the core's state changes, and words its note by cause", async () => {
    const { panel, inc, fiche } = await incidentPanel();
    const kind = fiche.byClass("incident-target-kind")[0];
    const note = (): FakeNode => fiche.byClass("fiche-note")[0];
    expect(note().hidden).toBe(true);
    expect(fiche.byClass("fiche-block").map((b) => b.className)).toEqual([
      "fiche-block incident-cible", "fiche-block incident-categorie", "fiche-block incident-etat", "fiche-block incident-mesure", "fiche-block incident-justif",
    ]);
    // the measure recorded: the same nodes (focus and typing kept), the record shown
    panel.setView(view({ ready: true, blocked: null, current: 1, incident: { ...inc, consigned: "le 15/10/2026 à 10:42" } }), "apps|G01");
    expect(fiche.byClass("incident-target-kind")[0]).toBe(kind);
    const consigned = fiche.byClass("incident-consigned")[0];
    expect([consigned.textContent, consigned.className]).toEqual(["Mesure consignée le 15/10/2026 à 10:42.", "incident-consigned ok"]);
    // a run: frozen, the run's note
    panel.setView(view({ running: true, blocked: RUNNING, current: 1, incident: { ...inc, editable: false } }), "apps|G01");
    expect(fiche.byClass("incident-target-kind")[0]).toBe(kind);
    expect([kind.disabled, note().hidden, note().textContent, fiche.byClass("fiche-save")[0].disabled, fiche.byClass("incident-consign")[0].disabled])
      .toEqual([true, false, RUNNING_NOTE, true, true]);
    // validated: its own note
    panel.setView(view({ current: 1, incident: { ...inc, editable: false } }), "apps|G01");
    expect(note().textContent).toBe(VALIDATED_INCIDENT);
    expect(VALIDATED_INCIDENT).toBe("Partie validée : la fiche n'est pas modifiable.");
    // other lists from the bench: redrawn
    panel.setView(view({ current: 1, incident: { ...inc, pins: ["Z1.a"] } }), "apps|G01");
    expect(fiche.byClass("incident-target-kind")[0]).not.toBe(kind);
    expect(fiche.byClass("incident-target-kind")[0].disabled).toBe(false);
  });

  it("marks a wrong value under itself, and keeps unsaved changes marked until « Enregistrer la fiche »", async () => {
    const { fiche, calls } = await incidentPanel();
    const value = fiche.byClass("incident-value")[0];
    value.value = "4,1 V";
    value.fire("input");
    value.fire("change");
    expect([tokens(value).includes("invalid"), errorOf(value).textContent]).toEqual([true, FORMAT.value]);
    expect([fiche.byClass("fiche-dirty")[0].hidden, fiche.byClass("fiche-save")[0].className]).toEqual([false, "btn btn--primary fiche-save"]);
    fiche.byClass("fiche-save")[0].fire("click");
    await settle();
    expect(calls).toEqual([]);
    expect(fiche.byClass("fiche-status")[0].className).toBe("fiche-status err");
    value.value = "4,1";
    value.fire("input");
    expect(tokens(value).includes("invalid")).toBe(false);
    fiche.byClass("fiche-save")[0].fire("click");
    await settle();
    expect(calls).toEqual(["save 4.1"]);
    expect([fiche.byClass("fiche-dirty")[0].hidden, fiche.byClass("fiche-status")[0].textContent]).toEqual([true, "Enregistré"]);
  });
});

describe("polish: the verification follows the fiche", () => {
  const hintOf = (e: ProgressElements): FakeNode => e.hint as unknown as FakeNode;
  const withHint = () => ({ ...elements(), hint: new FakeNode("p") as unknown as HTMLElement });

  it("waits for a wrongly shaped field, says that unsaved changes are saved first, and saves then verifies", async () => {
    const e = withHint();
    const calls: string[] = [];
    const panel = new ProgressPanel(e, {
      ...quiet, loadFiche: async () => filledPart(), verify: async () => void calls.push("verify"), saveFiche: async (p) => void calls.push(`save ${p.trames[0].dlc}`),
    });
    panel.mount();
    panel.setView(view({ ready: true, blocked: null }), "apps|G01");
    (e.ficheToggle as unknown as FakeNode).fire("click");
    await settle();
    const fiche = e.fiche as unknown as FakeNode;
    expect([e.verify.disabled, hintOf(e).hidden]).toEqual([false, true]);
    // a wrong shape: the button waits, the reason under it and in its title, the toggle stays plain (the fiche is open)
    const dlc = fiche.all("input").find((i) => i.dataset.key === "trames.0.dlc")!;
    dlc.value = "9";
    dlc.fire("input");
    dlc.fire("change");
    expect([e.verify.disabled, hintOf(e).hidden, hintOf(e).textContent, hintOf(e).className, e.verify.title])
      .toEqual([true, false, FIX_FIELDS, "progress-hint is-warn", FIX_FIELDS]);
    expect(e.blocked.hidden).toBe(true);
    (e.verify as unknown as FakeNode).fire("click");
    await settle();
    expect(calls).toEqual([]);
    // corrected but not saved: enabled, the fiche will be saved first
    dlc.value = "6";
    dlc.fire("input");
    dlc.fire("change");
    expect([e.verify.disabled, hintOf(e).textContent, hintOf(e).className, e.verify.title]).toEqual([false, SAVED_FIRST, "progress-hint", ""]);
    (e.verify as unknown as FakeNode).fire("click");
    await settle();
    expect(calls).toEqual(["save 6", "verify"]);
    expect(hintOf(e).hidden).toBe(true);
    // the core says not ready: its message, no hint about saving
    dlc.value = "7";
    dlc.fire("input");
    panel.setView(view(), "apps|G01");
    expect([e.verify.disabled, e.blocked.textContent, hintOf(e).hidden]).toEqual([true, view().blocked, true]);
    // a wrong field closed with the fiche: the toggle takes the emphasis
    dlc.value = "x";
    dlc.fire("change");
    panel.setView(view({ ready: true, blocked: null }), "apps|G01");
    (e.ficheToggle as unknown as FakeNode).fire("click"); // « Fermer la fiche »
    expect([e.verify.disabled, hintOf(e).textContent, e.ficheToggle.className]).toEqual([true, FIX_FIELDS, "btn btn--primary ico ico--doc"]);
  });

  it("clears a format refusal once the field is corrected, and gates the incident form too", async () => {
    const e = withHint();
    const panel = new ProgressPanel(e, { ...quiet, loadFiche: async () => filledPart() });
    panel.mount();
    panel.setView(view({ ready: true, blocked: null }), "apps|G01");
    (e.ficheToggle as unknown as FakeNode).fire("click");
    await settle();
    const fiche = e.fiche as unknown as FakeNode;
    // typed without leaving the field: the verification refuses it (SHAPE_REFUSED), no second message under it
    fiche.byClass("fiche-id")[0].value = "0x900";
    (e.verify as unknown as FakeNode).fire("click");
    await settle();
    expect([e.blocked.textContent, hintOf(e).hidden, e.verify.disabled]).toEqual([SHAPE_REFUSED, true, true]);
    fiche.byClass("fiche-id")[0].value = "0x7A5";
    fiche.byClass("fiche-id")[0].fire("input");
    expect([e.blocked.hidden, e.verify.disabled]).toEqual([true, false]);

    const { e: ei, fiche: fi } = await incidentPanel();
    const value = fi.byClass("incident-value")[0];
    value.value = "4,1 V";
    value.fire("change");
    expect(ei.verify.disabled).toBe(true);
    value.value = "4,1";
    value.fire("input");
    expect(ei.verify.disabled).toBe(false);
    // the identifier only counts when it is the target
    const kind = fi.byClass("incident-target-kind")[0];
    kind.value = "IDENTIFIANT";
    kind.fire("change");
    const ident = fi.byClass("incident-identifier")[0];
    ident.value = "0x900";
    ident.fire("change");
    expect(ei.verify.disabled).toBe(true);
    kind.value = "COMPOSANT";
    kind.fire("change");
    expect(ei.verify.disabled).toBe(false);
  });
});

describe("polish: messages and states of the progression", () => {
  it("draws the missing-fields message as a list of field groups, its text unchanged", () => {
    const fields = [
      "Documents utilisés : au moins un document", "Trames : au moins une trame", "Supervision : premier timeout", "Supervision : timeout",
      "Décision DD-1", "Décision DD-2", "Cible",
    ];
    const message = `${MISSING_LEAD}${MISSING_LABEL}${fields.join(", ")}.`;
    expect(missingFields(message)).toEqual(fields);
    expect(missingFields("Connectez la carte et vérifiez que votre programme est chargé.")).toBeNull();
    const e = elements();
    const panel = new ProgressPanel(e, quiet);
    panel.mount();
    panel.setView(view({ blocked: message }));
    const blocked = e.blocked as unknown as FakeNode;
    expect(blocked.textContent).toBe(message);
    expect(blocked.textContent.startsWith("Complétez la fiche de cette étape avant la vérification.")).toBe(true);
    const items = blocked.byClass("pb-item");
    const visible = (n: FakeNode): string => n.children.filter((c) => !tokens(c).includes("sr")).map((c) => c.textContent).join("");
    expect(items.map(visible)).toEqual([
      "Documents utilisés : au moins un document", "Trames : au moins une trame", "Supervision : premier timeout, timeout", "Décision DD-1, DD-2", "Cible",
    ]);
    // another message: plain text again; the same message is not redrawn
    const list = blocked.byClass("pb-list")[0];
    panel.setView(view({ blocked: message, feedback: ["x"] }));
    expect(blocked.byClass("pb-list")[0]).toBe(list);
    panel.setView(view({ blocked: "Prochaine vérification possible dans 1 min 40 s." }));
    expect([blocked.children.length, blocked.textContent]).toEqual([0, "Prochaine vérification possible dans 1 min 40 s."]);
  });

  it("hides the previous verdict during a run, and brings it back when the same verdict comes again", () => {
    const e = elements();
    const revealed: [unknown, boolean][] = [];
    const panel = new ProgressPanel(e, { ...quiet, reveal: (el, focus) => void revealed.push([el, focus]) });
    panel.mount();
    const lines = ["Échec à corriger.", "Fiche nominale : conforme", "Essai nominal : non réussi", DETAIL_HIDDEN];
    const failed = { feedback: lines, steps: stepsOf(["ÉCHEC À CORRIGER"], 0) };
    panel.setView(view(failed));
    expect(e.feedback.hidden).toBe(false);
    panel.setView(view({ ...failed, running: true, blocked: RUNNING }));
    expect([e.feedback.hidden, e.blocked.textContent]).toEqual([true, RUNNING]);
    panel.setView(view(failed));
    expect(e.feedback.hidden).toBe(false);
    expect(revealed).toEqual([[e.feedback, true]]);
  });

  it("puts a failed export forward with « Réessayer l'export », « Ouvrir la fiche finale » kept without the violet", async () => {
    const e = {
      ...elements(), exportAlert: new FakeNode("div") as unknown as HTMLElement, exportRetry: new FakeNode("button") as unknown as HTMLButtonElement,
    };
    let opened = 0;
    let refuse = false;
    const panel = new ProgressPanel(e, {
      ...quiet,
      openFinal: async () => {
        opened++;
        if (refuse) throw "Dossier Documents introuvable.";
      },
    });
    panel.mount();
    const closed = { current: null, nominal_editable: false, steps: stepsOf(ALL_PASSED, null) };
    panel.setView(view({ ...closed, feedback: [...cpLines(), CLOSED_LINE, NOT_EXPORTED], fiche_state: EXPORT_PENDING }));
    expect([e.exportAlert.hidden, e.final.hidden, e.final.className, e.verify.hidden]).toEqual([false, false, "btn ico ico--doc", true]);
    refuse = true;
    (e.exportRetry as unknown as FakeNode).fire("click");
    expect(e.exportRetry.disabled).toBe(true);
    await settle();
    expect([opened, e.exportRetry.disabled, e.blocked.textContent]).toEqual([1, false, "Dossier Documents introuvable."]);
    // after a restart the export line may be gone: the fiche box still says it
    panel.setView(view({ ...closed, feedback: [], fiche_state: EXPORT_PENDING }));
    expect(e.exportAlert.hidden).toBe(false);
    // exported: the alert goes, the final fiche is the main action again
    panel.setView(view({ ...closed, feedback: [...cpLines(), CLOSED_LINE, EXPORTED], fiche_state: "Exportée le 15/10/2026 à 16:41" }));
    expect([e.exportAlert.hidden, e.final.className]).toEqual([true, "btn btn--primary ico ico--doc"]);
    // the core's text of the fiche box, pinned
    expect(EXPORT_PENDING).toBe("Clôturée, exportation à reprendre");
  });

  it("hides the foot of the fiche once the mission is closed, and chips a measure not recorded", async () => {
    const e = elements();
    const panel = new ProgressPanel(e, { ...quiet, loadFiche: async () => filledPart() });
    panel.mount();
    panel.setView(view({ current: null, nominal_editable: false, steps: stepsOf(ALL_PASSED, null) }), "apps|G01");
    (e.ficheToggle as unknown as FakeNode).fire("click");
    await settle();
    const fiche = e.fiche as unknown as FakeNode;
    expect([fiche.byClass("fiche-foot")[0].hidden, fiche.byClass("fiche-save").length]).toEqual([true, 1]);
    const { fiche: fi } = await incidentPanel();
    expect(fi.byClass("incident-consigned")[0].className).toBe("incident-consigned warn");
  });
});
