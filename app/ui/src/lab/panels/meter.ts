// Bench multimeter of the Physical Lab (right panel, under the inspector): function V DC or Ω, a COM probe and a VΩ
// probe on two pins of the circuit, and the display the core computes (V DC averaged over 100 frames, Ω only on a
// bench whose supplies are all off). It works in mission mode too, where net and pin values stay hidden: it is the
// student's instrument. The panel only sends the probes and the function and shows the core's display text.
//
// During an incident step of a 0.3 progression, « Mesure clé » under it records the measure where it is taken (the
// core's rule: « sans bouger les curseurs avant d'appuyer sur Consigner »): the same command and texts as the fiche's
// « Consigner la mesure ». While a verification runs, the probes are locked, and the core's message says why.

import { clear, el, setText } from "../../dom";
import type { ProgressionView } from "../../types";
import { CHOOSE_STATE, consignedText, imposedText } from "../../views/incident";
import type { LabController, LabState } from "../state";
import type { MeterFn, PinRef } from "../types";

/** The core's refusal of a measure outside the incident in progress (workflow/session.rs CONSIGN_REFUSED): the reason
 * shown on the disabled button once the incident part is validated. */
export const CONSIGN_REFUSED = "La mesure ne peut être consignée que pendant l'incident en cours, hors vérification.";

const FUNCTIONS: { value: MeterFn; label: string }[] = [
  { value: "v_dc", label: "V continu" },
  { value: "ohm", label: "Ω (banc hors tension)" },
];

/** The option value of a pin, and back. */
export function pinValue(p: PinRef | null): string {
  return p ? `${p.comp}:${p.pin}` : "";
}

export function parsePinValue(v: string): PinRef | null {
  const i = v.indexOf(":");
  if (i <= 0) return null;
  const comp = Number(v.slice(0, i));
  return Number.isInteger(comp) ? { comp, pin: v.slice(i + 1) } : null;
}

export class MeterPanel {
  private fn: HTMLSelectElement | null = null;
  private com: HTMLSelectElement | null = null;
  private vohm: HTMLSelectElement | null = null;
  private display: HTMLElement | null = null;
  private pinsKey = "";
  private shownKey = "";
  /** « Mesure clé » (incident steps) and the note that says why the probes are locked. */
  private mission: {
    block: HTMLElement; quantity: HTMLElement; consign: HTMLButtonElement; consigned: HTMLElement; error: HTMLElement; note: HTMLElement;
  } | null = null;
  private consignAction: (() => Promise<void>) | null = null;

  constructor(private root: HTMLElement, private state: LabState, private controller: LabController) {}

  mount(): void {
    clear(this.root);
    const row = (label: string, control: HTMLElement, probe?: string): void => {
      const r = el("div", "meter-row");
      const l = el("span", "meter-label", label);
      // the probe's colour swatch (black COM, red VΩ), drawn by CSS
      if (probe) l.dataset.probe = probe;
      r.appendChild(l);
      r.appendChild(control);
      this.root.appendChild(r);
    };
    this.display = el("div", "meter-display readout", "—");
    this.root.appendChild(this.display);
    this.fn = el("select", "select meter-function");
    for (const f of FUNCTIONS) {
      const o = el("option", undefined, f.label);
      o.value = f.value;
      this.fn.appendChild(o);
    }
    row("Fonction", this.fn);
    this.com = el("select", "select meter-com");
    row("COM", this.com, "com");
    this.vohm = el("select", "select meter-vohm");
    row("VΩ", this.vohm, "vohm");
    for (const s of [this.fn, this.com, this.vohm]) s.addEventListener("change", () => this.send());

    const note = el("p", "meter-note", "");
    note.hidden = true;
    this.root.appendChild(note);
    const block = el("div", "meter-mission");
    block.hidden = true;
    block.appendChild(el("p", "mission-head", "Mesure clé"));
    const quantity = el("p", "meter-quantity", "");
    block.appendChild(quantity);
    const consign = el("button", "btn meter-consign ico ico--meter", "Consigner la mesure");
    consign.type = "button";
    consign.addEventListener("click", () => {
      const action = this.consignAction;
      if (consign.disabled || !action || !this.mission) return;
      const error = this.mission.error;
      setText(error, "");
      action().then(
        () => setText(error, ""),
        (err) => setText(error, String(err)),
      );
    });
    block.appendChild(consign);
    const consigned = el("p", "meter-consigned", "");
    block.appendChild(consigned);
    const error = el("p", "meter-error", "");
    error.role = "status";
    block.appendChild(error);
    this.root.appendChild(block);
    this.mission = { block, quantity, consign, consigned, error, note };
  }

  /** The progression of the open pack (null: none): « Mesure clé » during an incident step, the probes locked while a
   * verification runs. `consign` records the measure (it saves an open incident form first) and rejects with the
   * refusal. Called at every state poll: only what changed is written. */
  setMission(p: ProgressionView | null, consign: () => Promise<void>): void {
    this.consignAction = consign;
    const m = this.mission;
    if (!m || !this.fn || !this.com || !this.vohm) return;
    const running = !!p?.running;
    const reason = running ? p?.blocked ?? "" : "";
    setText(m.note, reason);
    m.note.hidden = !reason;
    for (const s of [this.fn, this.com, this.vohm]) {
      if (s.disabled !== running) s.disabled = running;
      if (s.title !== reason) s.title = reason;
    }
    const incident = p?.incident ?? null;
    m.block.hidden = !incident;
    if (!incident) {
      setText(m.error, "");
      return;
    }
    setText(m.quantity, incident.grandeur ? imposedText(incident.grandeur) : CHOOSE_STATE);
    const disabled = !incident.editable || running;
    m.consign.disabled = disabled;
    const title = !disabled ? "" : running ? reason : CONSIGN_REFUSED;
    if (m.consign.title !== title) m.consign.title = title;
    setText(m.consigned, consignedText(incident.consigned));
    const cls = incident.consigned ? "meter-consigned ok" : "meter-consigned warn";
    if (m.consigned.className !== cls) m.consigned.className = cls;
  }

  /** Called every animation frame: options follow the circuit, the display follows the values. */
  render(): void {
    if (!this.fn || !this.com || !this.vohm || !this.display) return;
    const parts = [...this.state.components.values()].filter((c) => c.label !== "" && c.kind !== "junction");
    const pins = parts
      .flatMap((c) => (this.state.library.get(c.kind)?.pins ?? []).map((p) => ({ value: pinValue({ comp: c.id, pin: p.id }), label: `${c.label}.${p.id}` })))
      .sort((a, b) => a.label.localeCompare(b.label, "fr", { numeric: true }));
    const key = JSON.stringify(pins);
    if (key !== this.pinsKey) {
      this.pinsKey = key;
      for (const s of [this.com, this.vohm]) {
        const keep = s.value;
        clear(s);
        const none = el("option", undefined, "—");
        none.value = "";
        s.appendChild(none);
        for (const p of pins) {
          const o = el("option", undefined, p.label);
          o.value = p.value;
          s.appendChild(o);
        }
        s.value = pins.some((p) => p.value === keep) ? keep : "";
      }
      this.shownKey = "";
    }
    const m = this.state.values?.meter;
    const shown = JSON.stringify(m ?? null);
    if (shown === this.shownKey) return;
    setText(this.display, m?.display ?? "—");
    // the core's probes win over a stale selection (another document, a reload, a refused change); while a select
    // has the focus the synchronisation waits, and the key is not consumed
    const focused = [this.fn, this.com, this.vohm].some((s) => document.activeElement === s);
    if (m && focused) return;
    if (m) {
      this.fn.value = m.function;
      this.com.value = pinValue(m.com);
      this.vohm.value = pinValue(m.vohm);
    }
    this.shownKey = shown;
  }

  private send(): void {
    if (!this.fn || !this.com || !this.vohm) return;
    void this.controller.meter(parsePinValue(this.com.value), parsePinValue(this.vohm.value), this.fn.value as MeterFn).then((outcome) => {
      // refused (a verification holds the bench): the selects go back to the core's probes at the next frame
      if (!outcome.ok) this.shownKey = "";
    });
  }
}
