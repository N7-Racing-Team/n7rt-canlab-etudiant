import { describe, expect, it } from "vitest";
import type { Pt } from "../types";
import type { FPt } from "./geom";
import { Gestures, type KeyInput, type Mods } from "./gestures";
import { FakeController, comp, makeEnv, makeModel, makeState, refusal, wire } from "./testkit";

// S1 switch (0,0), toggle zone [−1,1]×[−2,0]; R1 (10,0), pins (7,0) (13,0); J1 junction (20,0); C1 (30,0), pins
// (27,0) (33,0); R2 (20,10) at 90°, pins (20,7) (20,13); BP1 push button (0,20), zone [−1,1]×[17,18], pins (±3,20);
// RV1 pot (60,0), zone [58,62]×[−2,−1], pins a (57,0) b (63,0) w (60,−3); R3 (45,5), pins (42,5) (48,5); RV2 pot
// (70,0) at 90°, zone [71,72]×[−2,2]. Wires 10: S1.p2–R1.p1, 11: R1.p2–J1, 12: J1–C1.p1, 13: J1–R2.p1.
function setup() {
  const state = makeState(
    [comp(1, "switch", [0, 0], 0, "S1"), comp(2, "resistor", [10, 0], 0, "R1"), comp(3, "junction", [20, 0]),
      comp(4, "capacitor", [30, 0], 0, "C1"), comp(5, "resistor", [20, 10], 90, "R2"),
      comp(6, "push_button", [0, 20], 0, "BP1"), comp(7, "potentiometer", [60, 0], 0, "RV1"),
      comp(8, "resistor", [45, 5], 0, "R3"), comp(9, "potentiometer", [70, 0], 90, "RV2")],
    [wire(10, [1, "p2"], [2, "p1"]), wire(11, [2, "p2"], [3, "j"]), wire(12, [3, "j"], [4, "p1"]), wire(13, [3, "j"], [5, "p1"])],
  );
  const model = makeModel(state);
  const ctl = new FakeController();
  const env = makeEnv();
  let now = 0;
  const g = new Gestures(state, ctl, model, env, () => now);
  return { state, ctl, env, g, setNow: (ms: number) => { now = ms; } };
}

const NO: Mods = { shift: false, ctrl: false };
const SHIFT: Mods = { shift: true, ctrl: false };
const key = (k: string, extra: Partial<KeyInput> = {}): KeyInput =>
  ({ key: k, code: "", shift: false, ctrl: false, alt: false, repeat: false, ...extra });
const click = (g: Gestures, p: FPt, m: Mods = NO): void => {
  g.down(p, m);
  g.up(p);
};
const ghostPath = (g: Gestures): Pt[] | undefined => {
  const gh = g.view()?.ghost;
  return gh?.t === "path" ? gh.path : undefined;
};
const settle = (): Promise<void> => new Promise((r) => setTimeout(r, 0));

describe("placement", () => {
  it("previews under the pointer, turns with R, places on click", () => {
    const { g, ctl } = setup();
    g.armPlacement("resistor");
    expect(g.gesture).toBe("place");
    g.move([40.3, 19.6]);
    expect(ctl.lastPreview).toEqual({ op: "add_component", kind: "resistor", pos: [40, 20], rot: 0 });
    expect(g.key(key("r"))).toBe(true);
    expect(ctl.lastPreview).toEqual({ op: "add_component", kind: "resistor", pos: [40, 20], rot: 90 });
    ctl.answer({ patch: { components: [], wires: [] } });
    expect(g.view()).toMatchObject({ status: "ok", ghost: { t: "place", comp: { kind: "resistor", pos: [40, 20], rot: 90 } } });
    g.down([40.4, 20.2], NO);
    g.up([40.4, 20.2]);
    expect(ctl.edits).toEqual([{ op: "add_component", kind: "resistor", pos: [40, 20], rot: 90 }]);
    expect(g.gesture).toBe("idle");
    expect(ctl.cancels).toBeGreaterThan(0);
  });

  it("sends nothing on a refused preview, shows the reason and stays armed until Escape", () => {
    const { g, ctl, state } = setup();
    g.armPlacement("capacitor");
    g.move([27.2, 0.1]);
    ctl.answer({ refusal: refusal("Deux broches au même point", [27, 0]) });
    expect(g.view()).toMatchObject({ status: "refused", refusal: { at: [27, 0] } });
    g.down([27.2, 0.1], NO);
    expect(ctl.edits).toEqual([]);
    expect(state.message).toEqual({ kind: "error", text: "Deux broches au même point" });
    expect(g.gesture).toBe("place");
    expect(g.key(key("Escape"))).toBe(true);
    expect(g.gesture).toBe("idle");
    g.down([27.2, 0.1], NO);
    expect(ctl.edits).toEqual([]);
  });

  it("cannot arm the junction (not placeable)", () => {
    const { g } = setup();
    g.armPlacement("junction");
    expect(g.gesture).toBe("idle");
  });
});

describe("wire drawing", () => {
  it("fixes corners in the void, removes the last one with Backspace, ends on a pin", () => {
    const { g, ctl, state } = setup();
    g.setTool("wire");
    expect(state.view.tool).toBe("wire");
    g.down([33.1, 0.2], NO);
    g.up([33.1, 0.2]);
    expect(g.gesture).toBe("wire");
    g.move([36.2, 4.8]);
    // Leaves C1.p2 along its direction (right), then turns.
    expect(ghostPath(g)).toEqual([[33, 0], [36, 0], [36, 5]]);
    expect(ctl.previews).toEqual([]);
    g.down([36.2, 4.8], NO);
    g.move([40, 8]);
    expect(ghostPath(g)).toEqual([[33, 0], [36, 0], [36, 5], [40, 5], [40, 8]]);
    g.down([40, 8], NO);
    expect(g.key(key("Backspace"))).toBe(true);
    g.move([42.1, 5.2]);
    const edit = { op: "add_wire", a: { type: "pin", comp: 4, pin: "p2" }, b: { type: "pin", comp: 8, pin: "p1" }, bends: [[36, 0], [36, 5]] };
    expect(ctl.lastPreview).toEqual(edit);
    g.down([42.1, 5.2], NO);
    expect(ctl.edits).toEqual([edit]);
    expect(g.gesture).toBe("idle");
    expect(g.tool).toBe("wire");
  });

  it("flips the elbow with /", () => {
    const { g } = setup();
    g.setTool("wire");
    g.down([33, 0], NO);
    g.move([36, 5]);
    expect(ghostPath(g)).toEqual([[33, 0], [36, 0], [36, 5]]);
    expect(g.key(key("/"))).toBe(true);
    expect(ghostPath(g)).toEqual([[33, 0], [33, 5], [36, 5]]);
  });

  it("ends on a wire interior with an on_wire endpoint", () => {
    const { g, ctl } = setup();
    g.setTool("wire");
    g.down([3, 20], NO);
    g.move([5.2, 0.3]);
    g.down([5.2, 0.3], NO);
    expect(ctl.edits).toEqual([{ op: "add_wire", a: { type: "pin", comp: 6, pin: "p2" }, b: { type: "on_wire", wire: 10, at: [5, 0] }, bends: [[5, 20]] }]);
  });

  it("keeps drawing after a refused end, and Escape cancels without sending", () => {
    const { g, ctl, state } = setup();
    g.setTool("wire");
    g.down([33, 0], NO);
    g.move([42, 5]);
    ctl.answer({ refusal: refusal("Ce fil passerait sur la broche R2.1 sans s'y relier") });
    g.down([42, 5], NO);
    expect(ctl.edits).toEqual([]);
    expect(state.message?.text).toBe("Ce fil passerait sur la broche R2.1 sans s'y relier");
    expect(g.gesture).toBe("wire");
    g.key(key("Escape"));
    expect(g.gesture).toBe("idle");
    expect(g.tool).toBe("wire");
    g.key(key("Escape"));
    expect(g.tool).toBe("select");
    expect(ctl.edits).toEqual([]);
  });

  it("starts from a pin dragged with the selection tool and ends where it is released", () => {
    const { g, ctl, state } = setup();
    g.down([48, 5], NO);
    g.move([48.6, 8]);
    expect(g.gesture).toBe("wire");
    g.move([57.1, 0.1]);
    g.up([57.1, 0.1]);
    expect(ctl.edits).toEqual([{ op: "add_wire", a: { type: "pin", comp: 8, pin: "p2" }, b: { type: "pin", comp: 7, pin: "a" }, bends: [[57, 5]] }]);
    click(g, [48, 5]);
    expect([...state.selection.components]).toEqual([8]);
  });
});

describe("move and reshape", () => {
  it("selects on press, previews the move, sends one edit at release", async () => {
    const { g, ctl, state } = setup();
    g.down([10, 0.2], NO);
    expect([...state.selection.components]).toEqual([2]);
    g.move([13.1, 0.3]);
    g.move([13.3, 0.1]);
    expect(ctl.previews).toEqual([{ op: "move", components: [2], wires: [], delta: [3, 0] }]);
    ctl.answer({ patch: { components: [], wires: [] } });
    g.up([13.3, 0.1]);
    expect(ctl.edits).toEqual([{ op: "move", components: [2], wires: [], delta: [3, 0] }]);
    // The previewed ghost stays until the reply has been applied, then goes.
    expect(g.view()?.status).toBe("ok");
    await settle();
    expect(g.view()).toBeNull();
  });

  it("restores the scene and shows the reason on a refused drop", () => {
    const { g, ctl, state } = setup();
    g.down([10, 0.2], NO);
    g.move([10.1, 3.2]);
    ctl.answer({ refusal: refusal("Le fil traverserait le corps de C1") });
    g.up([10.1, 3.2]);
    expect(ctl.edits).toEqual([]);
    expect(state.message).toEqual({ kind: "error", text: "Le fil traverserait le corps de C1" });
    expect(g.gesture).toBe("idle");
    expect(g.view()).toBeNull();
  });

  it("cancels with Escape: nothing is sent at release", () => {
    const { g, ctl } = setup();
    g.down([10, 0.2], NO);
    g.move([12, 0.1]);
    // R and Delete are swallowed under a drag: the document does not change mid-gesture.
    expect(g.key(key("r"))).toBe(true);
    expect(g.key(key("Delete"))).toBe(true);
    expect(g.key(key("Escape"))).toBe(true);
    g.up([12, 0.1]);
    expect(ctl.edits).toEqual([]);
    expect(g.view()).toBeNull();
  });

  it("drops a drag whose release was lost: the next click never commits it", () => {
    const { g, ctl, state } = setup();
    g.down([10, 0.2], NO);
    g.move([14, 0.1]);
    expect(g.gesture).toBe("move");
    // No pointerup reached the editor; the next press starts afresh on the empty canvas.
    click(g, [40, 30]);
    expect(ctl.edits).toEqual([]);
    expect(g.gesture).toBe("idle");
    expect(state.selection.components.size).toBe(0);
  });

  it("moves the whole selection when any of its items is dragged", () => {
    const { g, ctl, state } = setup();
    click(g, [10, 0.2]);
    click(g, [5, 0.1], SHIFT);
    expect([...state.selection.components, ...state.selection.wires]).toEqual([2, 10]);
    g.down([5, 0.1], NO);
    g.move([5.1, 2.2]);
    expect(ctl.lastPreview).toEqual({ op: "move", components: [2], wires: [10], delta: [0, 2] });
  });

  it("drags the segment of a lone selected wire into set_bends", () => {
    const { g, ctl } = setup();
    click(g, [20.1, 3.4]);
    g.down([20.1, 3.4], NO);
    g.move([22.2, 3.4]);
    expect(ctl.lastPreview).toEqual({ op: "set_bends", wire: 13, bends: [[22, 0], [22, 7]] });
    g.up([22.2, 3.4]);
    expect(ctl.edits).toEqual([{ op: "set_bends", wire: 13, bends: [[22, 0], [22, 7]] }]);
  });

  it("drags the end of a selected wire onto another pin (move_wire_end), never into the void", () => {
    const { g, ctl } = setup();
    click(g, [5, 0.1]);
    g.down([3, 0], NO);
    g.move([26.9, 0.2]);
    const edit = { op: "move_wire_end", wire: 10, end: "a", to: { type: "pin", comp: 4, pin: "p1" } };
    expect(ctl.lastPreview).toEqual(edit);
    g.up([26.9, 0.2]);
    expect(ctl.edits).toEqual([edit]);
    g.down([3, 0], NO);
    g.move([3, 9]);
    g.up([3, 9]);
    expect(ctl.edits).toEqual([edit]);
  });

  it("selects the wires of a junction, and moves the junction itself when dragged", () => {
    const { g, ctl, state } = setup();
    click(g, [20.1, 0.1]);
    expect([...state.selection.wires].sort()).toEqual([11, 12, 13]);
    expect(state.selection.components.size).toBe(0);
    g.down([20.1, 0.1], NO);
    g.move([20.2, -2.1]);
    expect(ctl.lastPreview).toEqual({ op: "move", components: [3], wires: [], delta: [0, -2] });
  });
});

describe("marquee", () => {
  it("selects enclosed items left to right, touched items right to left, Shift toggles", () => {
    const { g, state } = setup();
    g.down([6.5, -2.4], NO);
    g.move([13.5, 2.4]);
    expect(g.view()?.marquee?.enclosed).toBe(true);
    g.up([13.5, 2.4]);
    expect([...state.selection.components]).toEqual([2]);
    expect([...state.selection.wires]).toEqual([]);
    g.down([13.5, 2.4], NO);
    g.move([6.5, -2.4]);
    expect(g.view()?.marquee?.enclosed).toBe(false);
    g.up([6.5, -2.4]);
    expect([...state.selection.components]).toEqual([2]);
    expect([...state.selection.wires]).toEqual([10, 11]);
    g.down([6.5, -2.4], SHIFT);
    g.move([13.5, 2.4]);
    g.up([13.5, 2.4]);
    expect([...state.selection.components]).toEqual([]);
    expect([...state.selection.wires]).toEqual([10, 11]);
    click(g, [40, -10]);
    expect(state.selection.wires.size).toBe(0);
  });
});

describe("actuators", () => {
  it("toggles a switch from the state returned by the core, without selecting", () => {
    const { g, ctl, state } = setup();
    click(g, [0, -1]);
    expect(ctl.interactions).toEqual([{ op: "set_switch", comp: 1, closed: true }]);
    expect(state.selection.components.size).toBe(0);
    state.applyInteract({ rev: 1, comp: 1, state: { closed: true } });
    click(g, [0, -1]);
    expect(ctl.interactions[1]).toEqual({ op: "set_switch", comp: 1, closed: false });
    expect(ctl.edits).toEqual([]);
  });

  it("presses a push button and releases it on release, cancel, capture loss, blur or tool change", () => {
    const { g, ctl } = setup();
    const pressed = (): boolean[] => ctl.interactions.map((i) => (i.op === "set_button" ? i.pressed : false));
    g.down([0, 17.5], NO);
    g.up([0, 17.5]);
    g.down([0, 17.5], NO);
    g.cancelPointer();
    g.down([0, 17.5], NO);
    g.blur();
    g.down([0, 17.5], NO);
    g.cancel();
    g.down([0, 17.5], NO);
    g.setTool("wire");
    expect(pressed()).toEqual([true, false, true, false, true, false, true, false, true, false]);
    expect(ctl.interactions.every((i) => i.op === "set_button" && i.comp === 6)).toBe(true);
  });

  it("presses the selected button with Space, ignoring repeats", () => {
    const { g, ctl } = setup();
    click(g, [0, 20.5]);
    expect(g.key(key(" "))).toBe(true);
    expect(g.key(key(" ", { repeat: true }))).toBe(true);
    expect(g.keyUp(key(" "))).toBe(true);
    expect(ctl.interactions).toEqual([{ op: "set_button", comp: 6, pressed: true }, { op: "set_button", comp: 6, pressed: false }]);
  });

  it("drags the wiper: α from the pointer in the symbol frame, at most one send per frame", () => {
    const { g, ctl } = setup();
    g.down([59.2, -1.5], NO);
    expect(ctl.interactions).toEqual([]);
    g.frame();
    g.move([61, -1.5]);
    g.move([61.2, -1.5]);
    g.frame();
    g.frame();
    g.up([61.2, -1.5]);
    expect(ctl.interactions).toEqual([{ op: "set_pot", comp: 7, position: 0.3 }, { op: "set_pot", comp: 7, position: 0.8 }]);
    g.down([71.5, 1], NO);
    g.up([71.5, 1]);
    expect(ctl.interactions[2]).toEqual({ op: "set_pot", comp: 9, position: 0.75 });
  });

  it("steps the selected pot with the arrows: ±0.01, Shift ±0.1", () => {
    const { g, ctl, setNow } = setup();
    expect(g.key(key("ArrowRight"))).toBe(false);
    click(g, [60, 0.5]);
    expect(g.key(key("ArrowRight"))).toBe(true);
    expect(g.key(key("ArrowLeft", { shift: true }))).toBe(true);
    setNow(5000);
    g.key(key("ArrowUp"));
    expect(ctl.interactions.map((i) => (i.op === "set_pot" ? i.position : -1))).toEqual([0.51, 0.41, 0.51]);
  });
});

describe("keyboard", () => {
  it("deletes, rotates, undoes and redoes, selects all", () => {
    const { g, ctl, state } = setup();
    expect(g.key(key("Delete"))).toBe(false);
    click(g, [10, 0.2]);
    expect(g.key(key("Delete"))).toBe(true);
    expect(g.key(key("r"))).toBe(true);
    expect(ctl.edits).toEqual([{ op: "delete", components: [2], wires: [] }, { op: "rotate", components: [2], wires: [] }]);
    g.key(key("z", { ctrl: true }));
    g.key(key("y", { ctrl: true }));
    g.key(key("Z", { ctrl: true, shift: true }));
    expect([ctl.undos, ctl.redos]).toEqual([1, 2]);
    g.key(key("a", { ctrl: true }));
    expect([...state.selection.components]).toEqual([1, 2, 3, 4, 5, 6, 7, 8, 9]);
    expect([...state.selection.wires]).toEqual([10, 11, 12, 13]);
    g.key(key("Escape"));
    expect(state.selection.components.size + state.selection.wires.size).toBe(0);
  });

  it("switches tools, fits and zooms to 100 % (also from the AZERTY 1 key)", () => {
    const { g, env, state } = setup();
    g.key(key("w"));
    expect(g.tool).toBe("wire");
    expect(state.view.tool).toBe("wire");
    g.key(key("Escape"));
    expect(g.tool).toBe("select");
    g.key(key("f"));
    g.key(key("&", { code: "Digit1" }));
    expect(env.fits).toBe(1);
    expect(env.zooms).toEqual([1]);
    expect(g.key(key("Backspace"))).toBe(false);
  });
});

// The library highlights `placing`: it must end exactly when the editor leaves the placement, whatever ends it.
describe("armed placement seen from the library", () => {
  it("ends on a click that places, Escape, a tool change (W too), undo; a refused click keeps it", () => {
    const { g, ctl } = setup();
    expect(g.placing).toBeNull();
    g.armPlacement("resistor");
    expect(g.placing).toBe("resistor");
    g.key(key("Escape"));
    expect(g.placing).toBeNull();

    g.armPlacement("capacitor");
    g.setTool("wire");
    expect(g.placing).toBeNull();
    g.armPlacement("capacitor");
    expect(g.placing).toBe("capacitor");
    // Already on the wire tool: W still ends the placement.
    g.key(key("w"));
    expect(g.placing).toBeNull();

    g.armPlacement("resistor");
    g.key(key("z", { ctrl: true }));
    expect([g.placing, ctl.undos]).toEqual([null, 1]);

    g.armPlacement("capacitor");
    g.move([27.2, 0.1]);
    ctl.answer({ refusal: refusal("Deux broches au même point", [27, 0]) });
    g.down([27.2, 0.1], NO);
    expect([g.placing, ctl.edits.length]).toEqual(["capacitor", 0]);
    g.move([40, 20]);
    ctl.answer({ patch: { components: [], wires: [] } });
    g.down([40, 20], NO);
    expect([g.placing, ctl.edits]).toEqual([null, [{ op: "add_component", kind: "capacitor", pos: [40, 20], rot: 0 }]]);
  });

  it("is not armed for a kind the library cannot place", () => {
    const { g } = setup();
    g.armPlacement("resistor");
    g.armPlacement("junction");
    expect(g.placing).toBeNull();
  });
});

describe("document replaced (file opened, new circuit, reload)", () => {
  it("drops a wire in progress without sending anything, keeps the tool", () => {
    const { g, ctl } = setup();
    g.setTool("wire");
    click(g, [33.1, 0.2]);
    g.move([36.2, 4.8]);
    expect(ghostPath(g)).toEqual([[33, 0], [36, 0], [36, 5]]);
    g.docReplaced();
    expect([g.gesture, ghostPath(g), g.tool]).toEqual(["idle", undefined, "wire"]);
    g.down([42.1, 5.2], NO);
    expect(ctl.edits).toEqual([]);
  });

  it("keeps an armed placement and an actuator held by the pointer", () => {
    const { g, ctl } = setup();
    g.armPlacement("resistor");
    g.docReplaced();
    expect(g.placing).toBe("resistor");
    g.key(key("Escape"));
    g.down([59.2, -1.5], NO);
    g.docReplaced();
    expect(g.gesture).toBe("pot");
    g.up([61.2, -1.5]);
    expect(ctl.interactions).toEqual([{ op: "set_pot", comp: 7, position: 0.8 }]);
  });
});
