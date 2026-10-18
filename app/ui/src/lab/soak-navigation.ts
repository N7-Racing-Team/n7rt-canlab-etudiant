// The sustained test owns its view until completion. Normal application routing is untouched.
// Only view-navigation controls are isolated; canvas/actuator/journal events still exercise the real UI.
const NAVIGATION = '.activity button[data-show-view], #mission-code, #ide-cockpit, #ide-phys';

export interface SoakNavigationEvent {
  button_id: string;
  view: string | null;
  is_trusted: boolean;
  timestamp_ms: number;
}

export function isolateSoakNavigation(root: Pick<Document, "querySelectorAll" | "addEventListener" | "removeEventListener"> = document,
  now: () => number = () => performance.now()): { events: SoakNavigationEvent[]; restore: () => void } {
  const buttons = Array.from(root.querySelectorAll<HTMLButtonElement>(NAVIGATION));
  const original = buttons.map((button) => ({ button, disabled: button.disabled }));
  const events: SoakNavigationEvent[] = [];
  let restored = false;
  const intercept: EventListener = (event) => {
    const target = event.target as Element | null;
    const button = target?.closest?.<HTMLButtonElement>(NAVIGATION);
    if (!button) return;
    if (event.type === "keydown" && !["Enter", " "].includes((event as KeyboardEvent).key)) return;
    events.push({ button_id: button.id, view: button.getAttribute("data-show-view"),
      is_trusted: event.isTrusted, timestamp_ms: Math.round(now()) });
    if (events.length > 32) events.splice(0, events.length - 32);
    if (event.isTrusted) {
      event.preventDefault();
      event.stopImmediatePropagation();
    }
  };
  for (const button of buttons) button.disabled = true;
  root.addEventListener("click", intercept, true);
  root.addEventListener("pointerdown", intercept, true);
  root.addEventListener("keydown", intercept, true);
  return { events, restore: () => {
    if (restored) return;
    restored = true;
    root.removeEventListener("click", intercept, true);
    root.removeEventListener("pointerdown", intercept, true);
    root.removeEventListener("keydown", intercept, true);
    for (const { button, disabled } of original) button.disabled = disabled;
  } };
}
