// French texts of the Physical Lab panels: operating states, pin roles, severities, choices, journal entries.
// Values are formatted by ../units.ts; this module only names things.

import type { LabState } from "../state";
import { exactText, INFINITY_SIGN, simTimeText } from "../units";
import type {
  CompId, Edit, Endpoint, Interact, Kind, OpState, PhysCommand, PinRef, Role, RsMode, Severity, SupplyOutput, TxdState, Value,
} from "../types";

export const SEVERITY_FR: Record<Severity, string> = { error: "erreur", warning: "alerte", info: "info" };

export const ROLE_FR: Record<Role, string> = {
  passive: "passive",
  source: "source",
  ground: "masse",
  power_in: "alimentation",
  ground_pin: "masse locale",
  analog_in: "entrée analogique",
  logic_in: "entrée logique",
  logic_out: "sortie logique",
  canh: "CANH",
  canl: "CANL",
};

const SUPPLY_FR: Record<SupplyOutput, string> = {
  off: "sortie coupée",
  cv: "tension constante (CV)",
  cc: "courant limité (CC)",
  rev: "rétroalimentée (REV) : tension extérieure au-dessus de la consigne",
};

const RS_FR: Record<RsMode, string> = {
  high_speed: "haute vitesse",
  slope_control: "contrôle de pente",
  standby: "veille",
  undefined: "indéfini",
  floating: "flottant",
};

const TXD_FR: Record<TxdState, string> = {
  recessive: "récessif",
  dominant_requested: "dominant demandé (non modélisé en V1)",
  dominant_timeout: "dominant coupé par la minuterie",
};

/** Pedal travel in percent, one decimal: "45.7 %". */
export function travelText(position: number): string {
  return `${(Math.round(position * 1000) / 10).toFixed(1)} %`;
}

/** Lines describing the operating state, as the real part would show it (`kind`: the component's kind). */
export function opStateLines(op: OpState, kind?: Kind): string[] {
  switch (op.kind) {
    case "supply":
      return [`Sortie : ${SUPPLY_FR[op.output]}`];
    case "contact":
      return [op.closed ? "Contact fermé" : "Contact ouvert"];
    case "pot":
      return kind === "mlx90377"
        ? [`Course de la pédale : ${travelText(op.position)}`]
        : [`Curseur à ${op.position.toFixed(3)} (0 = A, 1 = B)`];
    case "ecu":
      if (kind === "ecu_apps" || kind === "ecu_wsf" || kind === "ecu_sas" || kind === "ecu_bps") return [op.powered ? "Alimenté" : "En reset"];
      return [op.powered ? "Carte alimentée" : "Carte non alimentée"];
    case "transceiver":
      return [
        op.powered ? "Alimenté (au-dessus du seuil POR)" : "Non alimenté (sous le seuil POR)",
        `Mode Rs : ${RS_FR[op.rs]}`,
        `TXD : ${TXD_FR[op.txd]}`,
      ];
    case "passive":
      if (kind === "front_axle" || kind === "steering_column" || kind === "brake_hydraulics") return ["État mécanique du banc"];
      return ["Composant passif"];
  }
}

const CHOICES_FR: Record<string, Record<string, string>> = {
  contact: { no: "NO (normalement ouvert)", nc: "NF (normalement fermé)" },
  mode: { momentary: "fugitif", latching: "à accrochage" },
  can_mode: { can: "FDCAN actif (image --can-bus)", hi_z: "haute impédance (image sans FDCAN)" },
  adc_bits: { 8: "8 bits", 10: "10 bits", 12: "12 bits", 14: "14 bits", 16: "16 bits" },
};

export function choiceText(param: string, choice: string): string {
  return CHOICES_FR[param]?.[choice] ?? choice;
}

/** "R1", or the kind name for parts without designator (ground, junction), or "#id" once deleted. */
export function compName(state: LabState, id: CompId): string {
  const c = state.components.get(id);
  if (!c) return `#${id}`;
  if (c.label) return c.label;
  return c.kind === "junction" ? `J${id}` : state.library.get(c.kind)?.name_fr.toLowerCase() ?? c.kind;
}

/** "R1.2", "U1.PD0", "J12": designator and short pin name. */
export function pinName(state: LabState, p: PinRef): string {
  const c = state.components.get(p.comp);
  if (c?.kind === "junction") return `J${p.comp}`;
  const short = c ? state.library.get(c.kind)?.pins.find((d) => d.id === p.pin)?.short ?? p.pin : p.pin;
  return `${compName(state, p.comp)}.${short}`;
}

function endpointText(state: LabState, e: Endpoint): string {
  return e.type === "pin" ? pinName(state, e) : `fil ${e.wire} en (${e.at[0]}, ${e.at[1]})`;
}

function valueText(v: Value): string {
  if (typeof v === "number") return Number.isFinite(v) ? exactText(v, "number") : INFINITY_SIGN;
  if (typeof v === "boolean") return v ? "oui" : "non";
  return v;
}

function itemsText(state: LabState, components: CompId[], wires: number[]): string {
  const parts = components.map((id) => compName(state, id));
  if (wires.length) parts.push(wires.length === 1 ? "1 fil" : `${wires.length} fils`);
  return parts.join(", ") || "rien";
}

function editText(state: LabState, e: Edit): string {
  switch (e.op) {
    case "add_component":
      return `ajout : ${state.library.get(e.kind)?.name_fr ?? e.kind} en (${e.pos[0]}, ${e.pos[1]})${e.rot ? `, ${e.rot}°` : ""}`;
    case "delete":
      return `suppression : ${itemsText(state, e.components, e.wires)}`;
    case "move":
      return `déplacement de (${e.delta[0]}, ${e.delta[1]}) : ${itemsText(state, e.components, e.wires)}`;
    case "rotate":
      return `rotation de 90° : ${itemsText(state, e.components, e.wires)}`;
    case "set_param":
      return `${compName(state, e.comp)}.${e.name} = ${valueText(e.value)}`;
    case "set_label":
      return `désignateur : ${e.label}`;
    case "add_wire":
      return `fil : ${endpointText(state, e.a)} → ${endpointText(state, e.b)}`;
    case "move_wire_end":
      return `fil ${e.wire}, extrémité ${e.end} → ${endpointText(state, e.to)}`;
    case "set_bends":
      return `tracé du fil ${e.wire}`;
  }
}

function interactText(state: LabState, i: Interact): string {
  const name = compName(state, i.comp);
  switch (i.op) {
    case "set_pot":
      if (state.components.get(i.comp)?.kind === "mlx90377") return `${name} : course de la pédale à ${travelText(i.position)}`;
      return `${name} : curseur à ${i.position.toFixed(3)}`;
    case "set_button":
      return `${name} : ${i.pressed ? "appuyé" : "relâché"}`;
    case "set_switch":
      return `${name} : ${i.closed ? "fermé" : "ouvert"}`;
    case "set_power":
      return `${name} : ${i.on ? "sous tension" : "hors tension"}`;
    case "set_world": {
      const names = { speed_mps: "vitesse", steering_deg: "direction", angle_deg: "angle de volant", pressure_bar: "pression" };
      const units = { speed_mps: "m/s", steering_deg: "°", angle_deg: "°", pressure_bar: "bar" };
      return `${name} : ${names[i.quantity]} à ${i.value} ${units[i.quantity]}`;
    }
  }
}

/** Kind column and message of a journal entry. */
export function commandText(state: LabState, c: PhysCommand): { kind: string; text: string } {
  switch (c.cmd) {
    case "edit":
      return { kind: "édition", text: editText(state, c.edit) };
    case "undo":
      return { kind: "historique", text: "annuler" };
    case "redo":
      return { kind: "historique", text: "rétablir" };
    case "interact":
      return { kind: "action", text: interactText(state, c.interact) };
    case "run":
    case "stop":
    case "reset":
      return { kind: "simulation", text: c.cmd.toUpperCase() };
    case "meter":
      return { kind: "instrument", text: `multimètre ${c.function === "ohm" ? "Ω" : "V continu"}` };
  }
}

/** Simulated time of a frame index (1 ms frames): "12.345 s". */
export function frameTime(frame: number): string {
  return simTimeText(frame * 1e6);
}
