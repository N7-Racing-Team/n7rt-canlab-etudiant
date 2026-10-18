import { afterEach, describe, expect, it, vi } from "vitest";
import { Inspector } from "./inspector";
import { LabState, type LabController } from "../state";
import type { Component, Kind, KindDef, StateDef } from "../types";

class Node {
  children: Node[] = [];
  dataset: Record<string, string> = {};
  attributes = new Map<string, string>();
  value = ""; min = ""; max = ""; step = ""; type = ""; title = ""; textContent = ""; className = "";
  private listeners = new Map<string, ((event: { key?: string; preventDefault(): void }) => void)[]>();
  private classes = new Set<string>();
  classList = { add: (name: string) => this.classes.add(name), remove: (name: string) => this.classes.delete(name),
    toggle: (name: string, enabled: boolean) => enabled ? this.classes.add(name) : this.classes.delete(name),
    contains: (name: string) => this.classes.has(name) };
  constructor(readonly tagName = "div") {}
  get firstChild() { return this.children[0] ?? null; }
  append(...nodes: Node[]) { this.children.push(...nodes); }
  appendChild(node: Node) { this.children.push(node); return node; }
  removeChild(node: Node) { this.children.splice(this.children.indexOf(node), 1); }
  setAttribute(name: string, value: string) { this.attributes.set(name, value); }
  getAttribute(name: string) { return this.attributes.get(name) ?? null; }
  addEventListener(type: string, fn: (event: { key?: string; preventDefault(): void }) => void) {
    const list = this.listeners.get(type) ?? []; list.push(fn); this.listeners.set(type, list);
  }
  fire(type: string, key?: string) { for (const fn of this.listeners.get(type) ?? []) fn({ key, preventDefault() {} }); }
  select() {}
  blur() { fakeDocument.activeElement = null; this.fire("blur"); }
  all() { return this.children.flatMap((c): Node[] => [c, ...c.all()]); }
}
const fakeDocument = { activeElement: null as Node | null, createElement: (tag: string) => new Node(tag),
  createTextNode: (text: string) => Object.assign(new Node("text"), { textContent: text }) };

function fixture(kind: Kind, definitions: StateDef[], params = {}) {
  vi.stubGlobal("document", fakeDocument);
  fakeDocument.activeElement = null;
  const state = new LabState();
  const component: Component = { id: 1, kind, label: "M1", params, pos: [0, 0], rot: 0 };
  const def: KindDef = { kind, name_fr: kind, prefix: "M", body: { x0: -4, y0: -5, x1: 4, y1: 5 },
    pins: [], params: [], states: definitions, actuators: [], placeable: true, sources_fr: "Public" };
  state.library.set(kind, def);
  state.components.set(1, component);
  state.states.set(1, Object.fromEntries(definitions.map((s) => [s.name, 0])));
  state.selection.components.add(1);
  const interact = vi.fn();
  const controller = { interact } as unknown as LabController;
  const root = new Node();
  const inspector = new Inspector(root as unknown as HTMLElement, new Node() as unknown as HTMLElement, state, controller);
  inspector.render();
  return { state, root, inspector, interact, component };
}
const quantity = (name: string, min: number, max: number, unit: string): StateDef => ({ name, min, max, unit,
  label_fr: name, vtype: "number", default: 0, persistent: true });
afterEach(() => vi.unstubAllGlobals());

describe("Inspector world controls dispatch physical interactions", () => {
  it("routes WSF speed and steering sliders independently, with public units", () => {
    const f = fixture("front_axle", [quantity("speed_mps", 0, 100, "m/s"), quantity("steering_deg", -360, 360, "°")]);
    const ranges = f.root.all().filter((n) => n.type === "range");
    ranges[0].value = "20"; ranges[0].fire("input");
    ranges[1].value = "-15"; ranges[1].fire("input");
    expect(f.interact.mock.calls.map((c) => c[0])).toEqual([
      { op: "set_world", comp: 1, quantity: "speed_mps", value: 20 },
      { op: "set_world", comp: 1, quantity: "steering_deg", value: -15 },
    ]);
  });

  it("enforces SAS public stops, preserves editing and redraws actual mechanical state", () => {
    const f = fixture("steering_column", [quantity("angle_deg", -360, 360, "°")], { min_angle_deg: -120, max_angle_deg: 120 });
    const range = f.root.all().find((n) => n.type === "range")!;
    const text = f.root.all().find((n) => n.dataset.state === "angle_deg")!;
    expect([range.min, range.max]).toEqual(["-120", "120"]);
    fakeDocument.activeElement = text; text.fire("focus"); text.value = "-60,5"; text.fire("keydown", "Enter");
    expect(f.interact).toHaveBeenLastCalledWith({ op: "set_world", comp: 1, quantity: "angle_deg", value: -60.5 });
    text.value = "121"; text.fire("keydown", "Enter");
    expect(f.interact).toHaveBeenCalledTimes(1);
    expect(text.classList.contains("invalid")).toBe(true);
    f.state.states.set(1, { angle_deg: -2 }); f.inspector.invalidate(); f.inspector.render();
    // Selection rebuild deliberately releases editing; subsequent values display the actual measured world state.
    const updated = f.root.all().find((n) => n.dataset.state === "angle_deg")!;
    expect(updated.value).toBe("-2");
  });

  it("uses BPS pressure bounds and updates them after public parameter changes", () => {
    const f = fixture("brake_hydraulics", [quantity("pressure_bar", 0, 250, "bar")], { min_pressure_bar: 0, max_pressure_bar: 150 });
    const text = f.root.all().find((n) => n.dataset.state === "pressure_bar")!;
    fakeDocument.activeElement = text; text.fire("focus"); text.value = "149,9"; text.fire("keydown", "Enter");
    expect(f.interact).toHaveBeenLastCalledWith({ op: "set_world", comp: 1, quantity: "pressure_bar", value: 149.9 });
    f.component.params.max_pressure_bar = 100;
    text.value = "120"; text.fire("keydown", "Enter");
    expect(f.interact).toHaveBeenCalledTimes(1);
  });
});
