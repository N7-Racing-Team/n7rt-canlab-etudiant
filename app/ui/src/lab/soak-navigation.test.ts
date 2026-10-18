import { describe, expect, it, vi } from "vitest";
import { isolateSoakNavigation } from "./soak-navigation";

function fixture() {
  const active = { id: "view-cockpit", disabled: false, getAttribute: () => "cockpit" };
  const alreadyDisabled = { id: "mission-code", disabled: true, getAttribute: () => null };
  const listeners = new Map<string, EventListener>();
  const router = vi.fn();
  const root = { querySelectorAll: () => [active, alreadyDisabled],
    addEventListener: (name: string, fn: EventListener) => listeners.set(name, fn),
    removeEventListener: (name: string) => listeners.delete(name) } as unknown as Document;
  function dispatch(trusted: boolean, navigation = true, type = "click", key = "") {
    const event = { type, key, isTrusted: trusted, target: { closest: () => navigation ? active : null },
      preventDefault: vi.fn(), stopImmediatePropagation: vi.fn() };
    listeners.get(type)?.(event as unknown as Event);
    if (!event.stopImmediatePropagation.mock.calls.length) router();
    return event;
  }
  return { root, active, alreadyDisabled, router, listeners, dispatch };
}

describe("sustained test navigation isolation", () => {
  it("blocks native navigation and restores the exact original disabled states and listeners", () => {
    const f = fixture();
    const guard = isolateSoakNavigation(f.root, () => 1234.5);
    expect(f.active.disabled).toBe(true);
    const event = f.dispatch(true);
    expect(event.preventDefault).toHaveBeenCalledOnce();
    expect(f.router).not.toHaveBeenCalled();
    expect(guard.events).toEqual([{ button_id: "view-cockpit", view: "cockpit", is_trusted: true, timestamp_ms: 1235 }]);
    guard.restore();
    guard.restore();
    expect(f.active.disabled).toBe(false);
    expect(f.alreadyDisabled.disabled).toBe(true);
    expect(f.listeners.size).toBe(0);
    f.dispatch(true);
    expect(f.router).toHaveBeenCalledOnce();
  });

  it("leaves synthetic test routing and all non-navigation interactions observable and unblocked", () => {
    const f = fixture();
    const guard = isolateSoakNavigation(f.root);
    expect(f.dispatch(false).preventDefault).not.toHaveBeenCalled();
    expect(f.dispatch(true, false).preventDefault).not.toHaveBeenCalled();
    expect(f.router).toHaveBeenCalledTimes(2);
    expect(guard.events).toHaveLength(1);
    expect(guard.events[0].is_trusted).toBe(false);
    guard.restore();
  });

  it("blocks native Enter/Space activation only on navigation controls and bounds its trace", () => {
    const f = fixture();
    const guard = isolateSoakNavigation(f.root);
    expect(f.dispatch(true, true, "keydown", "Enter").preventDefault).toHaveBeenCalledOnce();
    expect(f.dispatch(true, true, "keydown", " ").preventDefault).toHaveBeenCalledOnce();
    expect(f.dispatch(true, true, "pointerdown").preventDefault).toHaveBeenCalledOnce();
    expect(f.dispatch(true, false, "keydown", "Enter").preventDefault).not.toHaveBeenCalled();
    expect(f.dispatch(true, true, "keydown", "x").preventDefault).not.toHaveBeenCalled();
    for (let i = 0; i < 50; i++) f.dispatch(true);
    expect(guard.events).toHaveLength(32);
    guard.restore();
  });
});
