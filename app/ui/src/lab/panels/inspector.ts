// Inspector of the Physical Lab, right panel (contract docs/physical-lab-v1.md §8.4). For the selected component:
// designator and kind, parameters (the typed text goes to the core, which parses and validates it: §4), interaction
// controls, pinout with live values, measures, operating state and sources. For a wire: its net and the net
// voltage. The structure is rebuilt only when the selected item changes; values are written in place during the
// animation frame, and a field being edited is never overwritten.

import { clear, el, setText } from "../../dom";
import { pinKey, type LabController, type LabState } from "../state";
import type { CompId, Component, Edit, KindDef, ParamDef, PhysError, StateDef, Value, Wire, WorldQuantity } from "../types";
import { exactText, fieldText, rangeText, readingView, type ReadingView } from "../units";
import { choiceText, opStateLines, pinName, ROLE_FR } from "./text";

type Field =
  | { kind: "text"; def: ParamDef; input: HTMLInputElement; focusText: string | null; refused: string | null }
  | { kind: "select"; def: ParamDef; input: HTMLSelectElement }
  | { kind: "check"; def: ParamDef; input: HTMLInputElement }
  | { kind: "fixed"; def: ParamDef; cell: HTMLElement };

interface PinCells {
  net: HTMLElement;
  v: HTMLElement;
  i: HTMLElement;
}

interface Controls {
  checks: { name: string; input: HTMLInputElement }[];
  readOnly: { name: string; cell: HTMLElement }[];
  hold: HTMLButtonElement | null;
  pot: { range: HTMLInputElement; text: HTMLInputElement; dragging: boolean; focusText: string | null } | null;
  worlds: WorldControl[];
}

interface WorldControl {
  def: StateDef;
  quantity: WorldQuantity;
  range: HTMLInputElement;
  text: HTMLInputElement;
  dragging: boolean;
  focusText: string | null;
}

/** Only catalogued mechanical controls can issue set_world; sensors remain read-only. */
export function worldQuantity(kind: Component["kind"], name: string): WorldQuantity | null {
  if (kind === "front_axle" && (name === "speed_mps" || name === "steering_deg")) return name;
  if (kind === "steering_column" && name === "angle_deg") return name;
  if (kind === "brake_hydraulics" && name === "pressure_bar") return name;
  return null;
}

/** Public mechanical stops narrow the catalog range. Invalid stops never widen it. */
export function worldBounds(c: Component, s: StateDef): { min: number; max: number } {
  const names = c.kind === "steering_column" ? ["min_angle_deg", "max_angle_deg"]
    : c.kind === "brake_hydraulics" ? ["min_pressure_bar", "max_pressure_bar"] : [];
  const lo = c.params[names[0]], hi = c.params[names[1]];
  const min = typeof lo === "number" && Number.isFinite(lo) ? Math.max(s.min, lo) : s.min;
  const max = typeof hi === "number" && Number.isFinite(hi) ? Math.min(s.max, hi) : s.max;
  return min <= max ? { min, max } : { min: s.min, max: s.min };
}

export function parseWorldValue(text: string, min: number, max: number): number | null {
  const t = text.trim().replace(",", ".");
  if (!/^[+-]?(\d+\.?\d*|\.\d+)$/.test(t)) return null;
  const v = Number(t);
  return Number.isFinite(v) && v >= min && v <= max ? v : null;
}

/** A parenthesised note stays on one line (« (0 = A) »): a long command label breaks before it, not inside. */
export function keepNotes(label: string): string {
  return label.replace(/\([^)]*\)/g, (note) => note.replace(/ /g, " "));
}

/** Position of a potentiometer typed by the reader: "0.7", "0,7", ".7"; null outside [0, 1]. */
export function parsePosition(text: string): number | null {
  const t = text.trim().replace(",", ".");
  if (!/^[+]?(\d+\.?\d*|\.\d+)$/.test(t)) return null;
  const v = Number(t);
  return Number.isFinite(v) && v >= 0 && v <= 1 ? v : null;
}

export function errorText(e: PhysError): string {
  if (e.code === "refused") return e.refusal.message_fr;
  if (e.code === "engine_unavailable") return "moteur de simulation indisponible";
  return "opération refusée";
}

export function setReading(cell: HTMLElement, r: ReadingView, base: string): void {
  setText(cell, r.text);
  const cls = r.state === "word" ? `${base} lab-word` : r.state === "none" ? `${base} lab-none` : base;
  if (cell.className !== cls) cell.className = cls;
  if (cell.title !== r.title) cell.title = r.title;
}

export class Inspector {
  private key = "";
  private generation = 0;
  private valuesDirty = true;
  private comp: CompId | null = null;
  private wire: number | null = null;
  private fields = new Map<string, Field>();
  private labelInput: HTMLInputElement | null = null;
  private labelRefused: string | null = null;
  private labelFocus: string | null = null;
  private pins = new Map<string, PinCells>();
  private measuresBody: HTMLElement | null = null;
  private measuresKey = "";
  private measureCells = new Map<string, HTMLElement>();
  private opEl: HTMLElement | null = null;
  private opText = "";
  private errorEl: HTMLElement | null = null;
  private controls: Controls | null = null;
  private wireCells: { net: HTMLElement; v: HTMLElement } | null = null;
  /** Push button held from the inspector (released on pointerup, pointercancel, blur, view change). */
  private holding: CompId | null = null;

  constructor(private readonly root: HTMLElement, private readonly what: HTMLElement, private readonly state: LabState,
    private readonly controller: LabController) {}

  mount(): void {
    const values = () => {
      this.valuesDirty = true;
    };
    this.state.onValues.on(values);
    this.state.onDoc.on(values);
    this.state.onStatus.on(values);
    window.addEventListener("blur", () => this.releaseHold());
  }

  invalidate(): void {
    this.key = "";
    this.valuesDirty = true;
  }

  render(): void {
    const key = this.selectionKey();
    if (key !== this.key) {
      this.key = key;
      this.build();
      this.valuesDirty = true;
    }
    if (this.valuesDirty) {
      this.valuesDirty = false;
      this.update();
    }
  }

  /** Releases a push button held from the inspector (view hidden, window blurred, selection changed). */
  releaseHold(): void {
    const id = this.holding;
    if (id === null) return;
    this.holding = null;
    if (this.state.components.has(id)) this.controller.interact({ op: "set_button", comp: id, pressed: false });
    this.valuesDirty = true;
  }

  private selectionKey(): string {
    const { components, wires } = this.state.selection;
    if (components.size === 1 && wires.size === 0) {
      const id = components.values().next().value as CompId;
      const c = this.state.components.get(id);
      return c && this.state.library.has(c.kind) ? `c:${id}:${c.kind}` : "none";
    }
    if (components.size === 0 && wires.size === 1) return `w:${wires.values().next().value}`;
    if (components.size + wires.size === 0) return "none";
    return `m:${components.size}:${wires.size}`;
  }

  // ------------------------------------------------------------------ structure

  private build(): void {
    this.releaseHold();
    this.generation++;
    this.comp = null;
    this.wire = null;
    this.fields.clear();
    this.pins.clear();
    this.measureCells.clear();
    this.labelInput = null;
    this.labelRefused = null;
    this.labelFocus = null;
    this.measuresBody = null;
    this.measuresKey = "";
    this.opEl = null;
    this.opText = "";
    this.errorEl = null;
    this.controls = null;
    this.wireCells = null;
    clear(this.root);
    const [kind, a] = this.key.split(":");
    if (kind === "c") {
      const c = this.state.components.get(Number(a))!;
      this.comp = c.id;
      this.buildComponent(c, this.state.kindDef(c.kind));
    } else if (kind === "w") {
      this.wire = Number(a);
      this.buildWire();
    } else if (kind === "m") {
      const { components, wires } = this.state.selection;
      const parts = [];
      if (components.size) parts.push(`${components.size} composant${components.size > 1 ? "s" : ""}`);
      if (wires.size) parts.push(`${wires.size} fil${wires.size > 1 ? "s" : ""}`);
      this.root.appendChild(el("p", "empty", `${parts.join(" et ")} sélectionnés. Suppr les supprime, R les tourne, un glissement les déplace.`));
      setText(this.what, `${components.size + wires.size} éléments`);
    } else {
      this.root.appendChild(el("p", "empty", "Sélectionnez un composant ou un fil dans le schéma."));
      setText(this.what, "");
    }
  }

  private head(text: string): void {
    this.root.appendChild(el("div", "panel-head", text));
  }

  private buildComponent(c: Component, def: KindDef): void {
    const id = el("div", "lab-id");
    if (def.prefix) {
      const input = el("input", "input");
      input.type = "text";
      input.spellcheck = false;
      input.autocomplete = "off";
      input.maxLength = 16;
      input.dataset.label = "";
      input.title = "Désignateur : une lettre, puis lettres, chiffres ou _ (16 caractères au plus), unique";
      input.setAttribute("aria-label", "Désignateur");
      this.bindLabel(input);
      this.labelInput = input;
      id.appendChild(input);
    }
    id.appendChild(el("span", "lab-kind", def.name_fr));
    this.root.appendChild(id);

    if (def.params.length) {
      this.head("Paramètres");
      const table = el("table", "grid-table lab-params");
      const cols = el("colgroup");
      cols.append(el("col"), el("col", "c-value"), el("col", "c-unit"));
      table.appendChild(cols);
      const body = el("tbody");
      for (const p of def.params) body.appendChild(this.paramRow(p));
      table.appendChild(body);
      this.root.appendChild(table);
    }
    this.errorEl = el("p", "error-text");
    this.errorEl.setAttribute("role", "alert");
    this.root.appendChild(this.errorEl);

    if (def.states.length) {
      this.head("Commandes");
      this.root.appendChild(this.controlsBlock(c, def.states));
    }

    this.head("Brochage");
    const pins = el("table", "grid-table lab-pins");
    const head = el("tr");
    const th = (text: string, cls: string, col?: string) => {
      const cell = el("th", cls, text);
      if (col) cell.dataset.col = col;
      head.appendChild(cell);
    };
    th("Broche", "c-pin");
    th("Nom", "c-name", "name");
    th("Rôle", "c-role", "role");
    th("Net", "c-net");
    th("V", "num c-v");
    th("I", "num c-i");
    const thead = el("thead");
    thead.appendChild(head);
    pins.appendChild(thead);
    const body = el("tbody");
    for (const p of def.pins) {
      const tr = el("tr");
      tr.dataset.pin = p.id;
      const full = `${p.name} (${p.id})`;
      const cell = (text: string, cls?: string, col?: string, title?: string) => {
        const td = el("td", cls, text);
        if (col) td.dataset.col = col;
        if (title) td.title = title;
        tr.appendChild(td);
        return td;
      };
      cell(p.short, undefined, undefined, full);
      cell(p.name, undefined, "name", full);
      cell(ROLE_FR[p.role] + (p.required ? " · obligatoire" : ""), "dim", "role");
      const net = cell("—");
      const v = cell("—", "num", "v");
      const i = cell("—", "num", "i");
      this.pins.set(p.id, { net, v, i });
      body.appendChild(tr);
    }
    pins.appendChild(body);
    this.root.appendChild(pins);

    this.head("Mesures");
    const measures = el("table", "grid-table lab-measures");
    const mcols = el("colgroup");
    mcols.append(el("col"), el("col", "c-value"));
    measures.appendChild(mcols);
    this.measuresBody = el("tbody");
    measures.appendChild(this.measuresBody);
    this.root.appendChild(measures);

    this.head("État de fonctionnement");
    this.opEl = el("div", "lab-text lab-op");
    this.root.appendChild(this.opEl);

    this.head("Sources");
    this.root.appendChild(el("p", "lab-text", def.sources_fr));
  }

  private paramRow(p: ParamDef): HTMLTableRowElement {
    const tr = el("tr");
    tr.dataset.param = p.name;
    const name = el("td", undefined, p.label_fr);
    name.title = `${p.label_fr} (${p.name})`;
    const cell = el("td", "lab-cell");
    const unit = el("td", "dim", p.unit);
    tr.append(name, cell, unit);
    if (!p.editable) {
      const fixed = el("span", "lab-fixed");
      fixed.title = "valeur fixe (catalogue)";
      cell.appendChild(fixed);
      this.fields.set(p.name, { kind: "fixed", def: p, cell: fixed });
    } else if (p.vtype === "choice") {
      const select = el("select", "select");
      select.setAttribute("aria-label", p.label_fr);
      for (const c of p.choices) {
        const o = el("option", undefined, choiceText(p.name, c));
        o.value = c;
        select.appendChild(o);
      }
      select.addEventListener("change", () => this.sendParam(p, select.value, null));
      cell.appendChild(select);
      this.fields.set(p.name, { kind: "select", def: p, input: select });
    } else if (p.vtype === "bool") {
      const label = el("label", "check");
      const input = el("input");
      input.type = "checkbox";
      input.setAttribute("aria-label", p.label_fr);
      input.addEventListener("change", () => this.sendParam(p, input.checked, null));
      label.appendChild(input);
      cell.appendChild(label);
      this.fields.set(p.name, { kind: "check", def: p, input });
    } else {
      const input = el("input", "input");
      input.type = "text";
      input.spellcheck = false;
      input.autocomplete = "off";
      input.title = rangeText(p);
      input.setAttribute("aria-label", p.label_fr);
      const field: Field = { kind: "text", def: p, input, focusText: null, refused: null };
      this.bindParamText(field);
      cell.appendChild(input);
      this.fields.set(p.name, field);
    }
    return tr;
  }

  /** Text field: exact value while editing, committed on Enter or blur, Escape restores. */
  private bindParamText(field: Extract<Field, { kind: "text" }>): void {
    const { input, def } = field;
    input.addEventListener("focus", () => {
      input.value = field.refused ?? exactText(this.paramValue(def.name), def.vtype);
      field.focusText = input.value;
      input.select();
    });
    input.addEventListener("keydown", (e) => {
      if (e.key === "Enter") {
        e.preventDefault();
        this.commitParam(field);
      } else if (e.key === "Escape") {
        e.preventDefault();
        field.refused = null;
        field.focusText = null;
        input.classList.remove("invalid");
        this.setError("");
        input.blur();
      }
    });
    input.addEventListener("blur", () => {
      if (field.focusText !== null) this.commitParam(field);
      field.focusText = null;
      this.valuesDirty = true;
    });
  }

  private commitParam(field: Extract<Field, { kind: "text" }>): void {
    const text = field.input.value.trim();
    if (text === field.focusText) return;
    if (text === "") {
      field.refused = null;
      field.input.classList.remove("invalid");
      field.input.value = exactText(this.paramValue(field.def.name), field.def.vtype);
      field.focusText = field.input.value;
      return;
    }
    field.focusText = text;
    // Text is parsed by the core (§4); an integer parameter takes a number, so plain integers are sent as such.
    const value: Value = field.def.vtype === "int" && /^[+-]?\d+$/.test(text) ? Number(text) : text;
    this.sendParam(field.def, value, field);
  }

  private sendParam(def: ParamDef, value: Value, field: Extract<Field, { kind: "text" }> | null): void {
    const comp = this.comp;
    if (comp === null) return;
    const generation = this.generation;
    const edit: Edit = { op: "set_param", comp, name: def.name, value };
    void this.controller.edit(edit).then((o) => {
      if (generation !== this.generation) return;
      if (o.ok) {
        if (field) {
          field.refused = null;
          field.input.classList.remove("invalid");
        }
        this.setError("");
      } else {
        if (field) {
          field.refused = String(value);
          field.input.classList.add("invalid");
        }
        this.setError(`${def.label_fr} : ${errorText(o.error)}`);
      }
      this.valuesDirty = true;
    });
  }

  private bindLabel(input: HTMLInputElement): void {
    input.addEventListener("focus", () => {
      this.labelFocus = input.value;
    });
    const commit = () => {
      const text = input.value.trim();
      const comp = this.comp;
      if (comp === null || text === this.labelFocus) return;
      this.labelFocus = text;
      const generation = this.generation;
      void this.controller.edit({ op: "set_label", comp, label: text }).then((o) => {
        if (generation !== this.generation) return;
        this.labelRefused = o.ok ? null : text;
        input.classList.toggle("invalid", !o.ok);
        this.setError(o.ok ? "" : `Désignateur : ${errorText(o.error)}`);
        this.valuesDirty = true;
      });
    };
    input.addEventListener("keydown", (e) => {
      if (e.key === "Enter") {
        e.preventDefault();
        commit();
      } else if (e.key === "Escape") {
        e.preventDefault();
        this.labelRefused = null;
        this.labelFocus = null;
        input.classList.remove("invalid");
        input.blur();
      }
    });
    input.addEventListener("blur", () => {
      if (this.labelFocus !== null) commit();
      this.labelFocus = null;
      this.valuesDirty = true;
    });
  }

  private controlsBlock(c: Component, states: StateDef[]): HTMLElement {
    const block = el("div", "lab-block");
    const controls: Controls = { checks: [], readOnly: [], hold: null, pot: null, worlds: [] };
    const row = (label: string, control: HTMLElement) => {
      const field = el("div", "field");
      field.append(el("span", "lab-label", keepNotes(label)), control);
      block.appendChild(field);
    };
    for (const s of states) {
      const quantity = worldQuantity(c.kind, s.name);
      if (quantity !== null) {
        const wrap = el("div", "field-row");
        const range = el("input", "range");
        range.type = "range";
        const bounds = worldBounds(c, s);
        range.min = String(bounds.min);
        range.max = String(bounds.max);
        range.step = "0.1";
        range.setAttribute("aria-label", s.label_fr);
        const text = el("input", "input lab-pos");
        text.type = "text";
        text.spellcheck = false;
        text.autocomplete = "off";
        text.dataset.state = quantity;
        text.setAttribute("aria-label", `${s.label_fr} (${s.unit})`);
        const world: WorldControl = { def: s, quantity, range, text, dragging: false, focusText: null };
        this.bindWorld(world);
        wrap.append(range, text, el("span", "dim", s.unit));
        row(s.label_fr, wrap);
        controls.worlds.push(world);
      } else if (s.name === "closed" || s.name === "on" || s.name === "usb") {
        const label = el("label", "check");
        const input = el("input");
        input.type = "checkbox";
        input.dataset.state = s.name;
        input.addEventListener("change", () => {
          const id = this.comp;
          if (id === null) return;
          this.controller.interact(s.name === "closed"
            ? { op: "set_switch", comp: id, closed: input.checked }
            : { op: "set_power", comp: id, on: input.checked });
        });
        label.append(input, document.createTextNode(s.label_fr));
        block.appendChild(label);
        controls.checks.push({ name: s.name, input });
      } else if (s.name === "pressed") {
        const hold = el("button", "btn lab-hold", "Appuyer (maintenir)");
        hold.type = "button";
        hold.setAttribute("aria-pressed", "false");
        hold.title = "Maintenir appuyé : contact actionné tant que le bouton est tenu (souris ou Espace)";
        this.bindHold(hold, c.id);
        row(s.label_fr, hold);
        controls.hold = hold;
      } else if (s.name === "position") {
        const wrap = el("div", "field-row");
        const range = el("input", "range");
        range.type = "range";
        range.min = "0";
        range.max = "1";
        range.step = "0.001";
        range.setAttribute("aria-label", s.label_fr);
        const text = el("input", "input lab-pos");
        text.type = "text";
        text.spellcheck = false;
        text.autocomplete = "off";
        text.dataset.state = "position";
        text.title = c.kind === "mlx90377"
          ? "Course de la pédale, de 0 (repos) à 1 (pleine course), au millième"
          : "Position du curseur, de 0 (A) à 1 (B), au millième";
        const pot = { range, text, dragging: false, focusText: null as string | null };
        this.bindPot(pot);
        wrap.append(range, text);
        row(s.label_fr, wrap);
        controls.pot = pot;
      } else {
        const cell = el("span", "mono");
        row(s.label_fr, cell);
        controls.readOnly.push({ name: s.name, cell });
      }
    }
    this.controls = controls;
    return block;
  }

  private bindHold(hold: HTMLButtonElement, id: CompId): void {
    hold.addEventListener("pointerdown", (e) => {
      if (e.button !== 0) return;
      try {
        hold.setPointerCapture(e.pointerId);
      } catch {
        /* synthetic or vanished pointer: the button still works without capture */
      }
      this.press(id);
    });
    for (const type of ["pointerup", "pointercancel", "lostpointercapture", "blur"]) {
      hold.addEventListener(type, () => this.releaseHold());
    }
    hold.addEventListener("keydown", (e) => {
      if (e.key !== " " && e.key !== "Enter") return;
      e.preventDefault();
      if (!e.repeat) this.press(id);
    });
    hold.addEventListener("keyup", (e) => {
      if (e.key !== " " && e.key !== "Enter") return;
      e.preventDefault();
      this.releaseHold();
    });
  }

  private press(id: CompId): void {
    if (this.holding === id) return;
    this.releaseHold();
    this.holding = id;
    this.controller.interact({ op: "set_button", comp: id, pressed: true });
    this.valuesDirty = true;
  }

  private bindPot(pot: NonNullable<Controls["pot"]>): void {
    const send = (position: number) => {
      const id = this.comp;
      if (id !== null) this.controller.interact({ op: "set_pot", comp: id, position });
    };
    pot.range.addEventListener("pointerdown", () => {
      pot.dragging = true;
    });
    for (const type of ["pointerup", "pointercancel", "change", "blur"]) {
      pot.range.addEventListener(type, () => {
        pot.dragging = false;
        this.valuesDirty = true;
      });
    }
    pot.range.addEventListener("input", () => send(Number(pot.range.value)));
    const commit = () => {
      const text = pot.text.value.trim();
      if (text === pot.focusText) return;
      pot.focusText = text;
      const v = parsePosition(text);
      pot.text.classList.toggle("invalid", v === null);
      if (v === null) {
        const pedal = this.comp !== null && this.state.components.get(this.comp)?.kind === "mlx90377";
        this.setError(pedal
          ? "Course de la pédale : nombre attendu entre 0 (repos) et 1 (pleine course)."
          : "Position du curseur : nombre attendu entre 0 (A) et 1 (B).");
        return;
      }
      this.setError("");
      send(v);
    };
    pot.text.addEventListener("focus", () => {
      pot.focusText = pot.text.value;
      pot.text.select();
    });
    pot.text.addEventListener("keydown", (e) => {
      if (e.key === "Enter") {
        e.preventDefault();
        commit();
      } else if (e.key === "Escape") {
        e.preventDefault();
        pot.focusText = null;
        pot.text.classList.remove("invalid");
        pot.text.blur();
      }
    });
    pot.text.addEventListener("blur", () => {
      if (pot.focusText !== null) commit();
      pot.focusText = null;
      this.valuesDirty = true;
    });
  }

  private bindWorld(world: WorldControl): void {
    const bounds = () => this.comp === null ? null : this.state.components.get(this.comp);
    const send = (value: number) => {
      if (this.comp !== null) this.controller.interact({ op: "set_world", comp: this.comp, quantity: world.quantity, value });
    };
    world.range.addEventListener("pointerdown", () => { world.dragging = true; });
    for (const event of ["pointerup", "pointercancel", "change", "blur"]) {
      world.range.addEventListener(event, () => { world.dragging = false; this.valuesDirty = true; });
    }
    world.range.addEventListener("input", () => send(Number(world.range.value)));
    const commit = () => {
      const text = world.text.value.trim();
      if (text === world.focusText) return;
      world.focusText = text;
      const c = bounds();
      if (!c) return;
      const { min, max } = worldBounds(c, world.def);
      const value = parseWorldValue(text, min, max);
      world.text.classList.toggle("invalid", value === null);
      if (value === null) {
        this.setError(`${world.def.label_fr} : nombre attendu entre ${min} et ${max} ${world.def.unit}.`);
        return;
      }
      this.setError("");
      send(value);
    };
    world.text.addEventListener("focus", () => { world.focusText = world.text.value; world.text.select(); });
    world.text.addEventListener("keydown", (event) => {
      if (event.key === "Enter") { event.preventDefault(); commit(); }
      else if (event.key === "Escape") {
        event.preventDefault(); world.focusText = null; world.text.classList.remove("invalid"); world.text.blur();
      }
    });
    world.text.addEventListener("blur", () => {
      if (world.focusText !== null) commit();
      world.focusText = null; this.valuesDirty = true;
    });
  }

  private buildWire(): void {
    const w = this.state.wires.get(this.wire!);
    if (!w) return;
    this.head("Fil");
    const block = el("div", "lab-block");
    const kv = el("dl", "kv");
    block.appendChild(kv);
    const add = (k: string, v: string) => {
      kv.appendChild(el("dt", undefined, k));
      const dd = el("dd", undefined, v);
      kv.appendChild(dd);
      return dd;
    };
    add("Fil", `#${w.id}`);
    const net = add("Net", "—");
    const v = add("Tension", "—");
    add("De", pinName(this.state, w.a));
    add("Vers", pinName(this.state, w.b));
    add("Coudes", String(w.bends.length));
    this.root.appendChild(block);
    this.wireCells = { net, v };
    setText(this.what, `fil #${w.id}`);
  }

  // ------------------------------------------------------------------ values

  private paramValue(name: string): Value | undefined {
    return this.comp === null ? undefined : this.state.components.get(this.comp)?.params[name];
  }

  private setError(text: string): void {
    if (this.errorEl) setText(this.errorEl, text);
  }

  private update(): void {
    if (this.comp !== null) this.updateComponent(this.comp);
    else if (this.wire !== null) this.updateWire(this.wire);
  }

  private updateComponent(id: CompId): void {
    const c = this.state.components.get(id);
    if (!c) return;
    const def = this.state.kindDef(c.kind);
    setText(this.what, c.label ? `${c.label} · ${def.name_fr}` : def.name_fr);
    const focused = document.activeElement;
    if (this.labelInput && focused !== this.labelInput) {
      this.labelInput.value = this.labelRefused ?? c.label;
    }
    for (const f of this.fields.values()) {
      const v = c.params[f.def.name];
      if (f.kind === "fixed") setText(f.cell, fieldText(v, f.def.vtype));
      else if (f.kind === "text") {
        if (focused !== f.input) f.input.value = f.refused ?? fieldText(v, f.def.vtype);
      } else if (f.kind === "select") {
        if (focused !== f.input && f.input.value !== String(v)) f.input.value = String(v);
      } else if (f.input.checked !== (v === true)) f.input.checked = v === true;
    }

    const values = this.state.compValues.get(id);
    const st = this.state.states.get(id) ?? values?.state ?? {};
    const ctl = this.controls;
    if (ctl) {
      for (const { name, input } of ctl.checks) if (input.checked !== (st[name] === true)) input.checked = st[name] === true;
      for (const { name, cell } of ctl.readOnly) setText(cell, typeof st[name] === "boolean" ? (st[name] ? "oui" : "non") : String(st[name] ?? "—"));
      if (ctl.hold) {
        const on = st.pressed === true;
        if (ctl.hold.getAttribute("aria-pressed") !== String(on)) ctl.hold.setAttribute("aria-pressed", String(on));
      }
      const pos = typeof st.position === "number" ? st.position : null;
      if (ctl.pot && pos !== null) {
        if (!ctl.pot.dragging && focused !== ctl.pot.range) ctl.pot.range.value = String(pos);
        if (focused !== ctl.pot.text) {
          ctl.pot.text.value = pos.toFixed(3);
          ctl.pot.text.classList.remove("invalid");
        }
      }
      for (const world of ctl.worlds) {
        const { min, max } = worldBounds(c, world.def);
        world.range.min = String(min);
        world.range.max = String(max);
        world.text.title = `${world.def.label_fr} : ${min} à ${max} ${world.def.unit}`;
        const value = st[world.quantity];
        if (typeof value !== "number" || !Number.isFinite(value)) continue;
        if (!world.dragging && focused !== world.range) world.range.value = String(value);
        if (focused !== world.text) {
          world.text.value = String(Number(value.toFixed(3)));
          world.text.classList.remove("invalid");
        }
      }
    }

    const mission = this.state.disclosure === "mission";
    const hidden: ReadingView = { text: "—", title: mission ? "masqué en mode mission" : "pas encore de valeur", state: "none" };
    for (const [pin, cells] of this.pins) {
      const net = this.state.netOf({ comp: id, pin });
      setText(cells.net, net?.name ?? "—");
      if (cells.net.title !== (net?.name ?? "")) cells.net.title = net?.name ?? "";
      const pv = values?.pins.find((p) => p.pin === pin);
      setReading(cells.v, pv ? readingView(pv.voltage, "V") : hidden, "num");
      setReading(cells.i, pv ? readingView(pv.current, "A") : hidden, "num");
    }

    if (this.measuresBody) {
      const measures = values?.measures ?? [];
      const key = measures.map((m) => `${m.name}|${m.label_fr}|${m.unit}`).join(";");
      if (key !== this.measuresKey) {
        this.measuresKey = key;
        this.measureCells.clear();
        clear(this.measuresBody);
        for (const m of measures) {
          const tr = el("tr");
          tr.dataset.measure = m.name;
          const label = el("td", undefined, m.label_fr);
          label.title = `${m.label_fr} (${m.name})`;
          const value = el("td", "num");
          value.dataset.col = "value";
          tr.append(label, value);
          this.measuresBody.appendChild(tr);
          this.measureCells.set(m.name, value);
        }
        if (!measures.length) {
          const tr = el("tr");
          tr.appendChild(el("td", "dim", mission ? "masquées en mode mission" : "aucune mesure publiée"));
          this.measuresBody.appendChild(tr);
        }
      }
      for (const m of measures) {
        const cell = this.measureCells.get(m.name);
        if (cell) setReading(cell, readingView(m.reading, m.unit), "num");
      }
    }

    if (this.opEl) {
      const lines = values ? opStateLines(values.op, c.kind) : ["pas encore de valeur"];
      const text = lines.join("\n");
      if (text !== this.opText) {
        this.opText = text;
        clear(this.opEl);
        for (const line of lines) this.opEl.appendChild(el("div", undefined, line));
      }
    }
  }

  private updateWire(id: number): void {
    const w: Wire | undefined = this.state.wires.get(id);
    if (!w || !this.wireCells) return;
    const net = this.state.netOf(w.a);
    setText(this.wireCells.net, net?.name ?? "—");
    const nv = net ? this.state.netValues.get(pinKey(net.key)) : undefined;
    setReading(this.wireCells.v, nv ? readingView(nv.voltage, "V") : readingView(null, "V"), "");
  }

}
