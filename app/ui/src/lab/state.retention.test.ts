import { beforeEach, describe, expect, it, vi } from "vitest";
import { comp } from "./canvas/testkit";
import { LabController, LabState } from "./state";
import type { CommandReply, Component, EditReply, PhysCommand, SnapshotDto } from "./types";

const bridge = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: bridge.invoke }));

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

function snapshot(doc_id: number, components: Component[], rev = 1): SnapshotDto {
  return { doc_id, rev, name: `document-${doc_id}`, dirty: false, components, wires: [], nets: [],
    states: components.map((c) => [c.id, { position: 0.5 }]), can_undo: false, can_redo: false,
    run: "stop", sim_time_ns: 0, disclosure: "exploration" };
}

function patch(rev: number, before: Component | null, after: Component | null): EditReply {
  return { rev, dirty: true, can_undo: true, can_redo: false,
    patch: { components: [[(before ?? after)!.id, before, after]], wires: [] }, nets: null };
}

const reply = (id: number, position: number): CommandReply => ({ type: "interact", rev: 1, comp: id, state: { position } });
const settle = async () => { for (let i = 0; i < 8; i++) await Promise.resolve(); };

beforeEach(() => {
  bridge.invoke.mockReset();
  bridge.invoke.mockImplementation((command: string) => {
    if (command === "phys_values") return Promise.resolve(null);
    return Promise.reject({ code: "engine_unavailable" });
  });
});

function fixture() {
  const state = new LabState();
  const pot = comp(1, "potentiometer", [0, 0], 0, "P1");
  state.applySnapshot(snapshot(1, [pot]));
  const controller = new LabController(state);
  const commands: PhysCommand[] = [];
  const pending: ReturnType<typeof deferred<CommandReply>>[] = [];
  bridge.invoke.mockImplementation((command: string, args?: { command: PhysCommand }) => {
    if (command === "phys_values") return Promise.resolve(null);
    if (command === "phys_command") {
      commands.push(args!.command);
      const call = deferred<CommandReply>();
      pending.push(call);
      return call.promise;
    }
    return Promise.reject({ code: "engine_unavailable" });
  });
  return { state, pot, controller, commands, pending };
}

describe("drag channels follow document/component lifetime", () => {
  it("shows the source-owned clock refusal verbatim and never claims that the refused RUN succeeded", async () => {
    const f = fixture();
    const message = "Arrêtez la séance CAN avant de commander l’horloge du banc.";
    bridge.invoke.mockImplementation((command: string) => command === "phys_values" ? Promise.resolve(null)
      : Promise.reject({ code: "refused", refusal: { code: "source_clock_owned", message_fr: message,
        ambiguity: null, at: null, items: [] } }));
    const result = await f.controller.runControl("run");
    expect(result.ok).toBe(false);
    expect(f.state.run).toBe("stop");
    expect(f.state.message).toEqual({ kind: "error", text: message });
  });
  it("coalesces two world quantities independently and cancels both on document replacement", async () => {
    const f = fixture();
    const axle: Component = { ...f.pot, kind: "front_axle" };
    f.state.applySnapshot(snapshot(1, [axle]));
    f.controller.interact({ op: "set_world", comp: 1, quantity: "speed_mps", value: 1 });
    f.controller.interact({ op: "set_world", comp: 1, quantity: "speed_mps", value: 9 });
    f.controller.interact({ op: "set_world", comp: 1, quantity: "steering_deg", value: -10 });
    f.controller.interact({ op: "set_world", comp: 1, quantity: "steering_deg", value: 20 });
    expect(f.controller.retainedGestureChannels).toBe(2);
    expect(f.commands).toHaveLength(2);
    f.pending[0].resolve({ type: "interact", rev: 1, comp: 1, state: { speed_mps: 1, steering_deg: 0 } });
    f.pending[1].resolve({ type: "interact", rev: 1, comp: 1, state: { speed_mps: 1, steering_deg: -10 } });
    await settle();
    expect(f.commands.slice(2)).toEqual([
      { cmd: "interact", interact: { op: "set_world", comp: 1, quantity: "speed_mps", value: 9 } },
      { cmd: "interact", interact: { op: "set_world", comp: 1, quantity: "steering_deg", value: 20 } },
    ]);
    f.state.applySnapshot(snapshot(2, [axle]));
    expect(f.controller.retainedGestureChannels).toBe(0);
    f.pending[2].resolve({ type: "interact", rev: 1, comp: 1, state: { speed_mps: 9 } });
    f.pending[3].resolve({ type: "interact", rev: 1, comp: 1, state: { steering_deg: 20 } });
    await settle();
    expect(f.state.states.get(1)).toEqual({ position: 0.5 });
  });

  it("drops angle/pressure channels on deletion and rejects arbitrary runtime quantity keys without retaining them", async () => {
    const f = fixture();
    for (const [kind, quantity] of [["steering_column", "angle_deg"], ["brake_hydraulics", "pressure_bar"]] as const) {
      const c: Component = { ...f.pot, kind };
      f.state.applySnapshot(snapshot(1, [c]));
      f.controller.interact({ op: "set_world", comp: 1, quantity, value: 10 });
      f.state.applyEdit(patch(2, c, null));
      expect(f.controller.retainedGestureChannels).toBe(0);
    }
    f.controller.interact({ op: "set_world", comp: 1, quantity: "unbounded-key" as never, value: 1 });
    expect(f.controller.retainedGestureChannels).toBe(0);
    await settle();
  });
  it("preserves the last pending drag of a live pot through a same-document snapshot", async () => {
    const f = fixture();
    f.controller.interact({ op: "set_pot", comp: 1, position: 0.1 });
    f.controller.interact({ op: "set_pot", comp: 1, position: 0.3 });
    f.controller.interact({ op: "set_pot", comp: 1, position: 0.9 });
    f.state.applySnapshot(snapshot(1, [f.pot]));
    f.pending[0].resolve(reply(1, 0.1));
    await settle();
    expect(f.commands).toEqual([
      { cmd: "interact", interact: { op: "set_pot", comp: 1, position: 0.1 } },
      { cmd: "interact", interact: { op: "set_pot", comp: 1, position: 0.9 } },
    ]);
    f.pending[1].resolve(reply(1, 0.9));
    await settle();
    expect(f.state.states.get(1)?.position).toBe(0.9);
    expect(f.controller.retainedPotChannels).toBe(1);
  });

  it("drops deleted IDs, pending drags, and late success/failure callbacks", async () => {
    const f = fixture();
    f.controller.interact({ op: "set_pot", comp: 1, position: 0.1 });
    f.controller.interact({ op: "set_pot", comp: 1, position: 0.9 });
    f.state.applyEdit(patch(2, f.pot, null));
    expect(f.controller.retainedPotChannels).toBe(0);
    expect(f.state.states.size).toBe(0);
    f.pending[0].resolve(reply(1, 0.1));
    await settle();
    expect(f.commands).toHaveLength(1);
    expect(f.state.states.size).toBe(0);
    f.state.applyInteract({ rev: 2, comp: 1, state: { position: 0.8 } });
    expect(f.state.states.size).toBe(0);

    f.state.applyEdit(patch(3, null, f.pot));
    f.controller.interact({ op: "set_pot", comp: 1, position: 0.2 });
    f.state.applyEdit(patch(4, f.pot, null));
    f.pending[1].reject({ code: "engine_unavailable" });
    await settle();
    expect(f.state.message).toBeNull();
    expect(f.controller.retainedPotChannels).toBe(0);
  });

  it("lets an undo restore a pot with a fresh channel, ignoring the former channel's reply", async () => {
    const f = fixture();
    f.controller.interact({ op: "set_pot", comp: 1, position: 0.1 });
    f.state.applyEdit(patch(2, f.pot, null));
    const undo = f.controller.undo();
    await settle();
    expect(f.commands[1]).toEqual({ cmd: "undo" });
    f.pending[1].resolve({ type: "edit", ...patch(3, null, f.pot) });
    expect((await undo).ok).toBe(true);
    f.controller.interact({ op: "set_pot", comp: 1, position: 0.8 });
    f.pending[2].resolve(reply(1, 0.8));
    await settle();
    f.pending[0].resolve(reply(1, 0.1));
    await settle();
    expect(f.state.states.get(1)?.position).toBe(0.8);
    expect(f.controller.retainedPotChannels).toBe(1);
  });

  it.each(["load", "new"])("clears old-document channels on %s even when IDs are reused", async (operation) => {
    const f = fixture();
    f.controller.interact({ op: "set_pot", comp: 1, position: 0.1 });
    f.controller.interact({ op: "set_pot", comp: 1, position: 0.9 });
    const next = snapshot(2, [f.pot]);
    const original = bridge.invoke.getMockImplementation()!;
    bridge.invoke.mockImplementation((command: string, args: unknown) => {
      if (command === "phys_load" || command === "phys_new") return Promise.resolve(next);
      return original(command, args);
    });
    const result = operation === "load" ? await f.controller.load("new-document") : await f.controller.newCircuit();
    expect(result.ok).toBe(true);
    expect(f.controller.retainedPotChannels).toBe(0);
    f.pending[0].resolve(reply(1, 0.1));
    await settle();
    expect(f.state.states.get(1)?.position).toBe(0.5);
    expect(f.commands).toHaveLength(1);
    f.controller.interact({ op: "set_pot", comp: 1, position: 0.7 });
    f.pending[1].resolve(reply(1, 0.7));
    await settle();
    expect(f.state.states.get(1)?.position).toBe(0.7);
    expect(f.controller.retainedPotChannels).toBe(1);
  });

  it("retains no per-ID state or channels after many create/drag/delete cycles", async () => {
    const f = fixture();
    f.state.applyEdit(patch(2, f.pot, null));
    for (let id = 2; id <= 500; id++) {
      const pot = comp(id, "potentiometer", [0, 0], 0, `P${id}`);
      f.state.applyEdit(patch(id * 2, null, pot));
      f.controller.interact({ op: "set_pot", comp: id, position: 0.5 });
      f.pending[f.pending.length - 1].resolve(reply(id, 0.5));
      await settle();
      f.state.applyEdit(patch(id * 2 + 1, pot, null));
      expect(f.controller.retainedPotChannels).toBe(0);
      expect(f.state.states.size).toBe(0);
    }
    expect(f.state.components.size).toBe(0);
  });
});
