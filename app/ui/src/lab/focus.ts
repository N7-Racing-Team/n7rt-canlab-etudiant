// Keys shared between the buttons and the schematic of the Physical Lab (contract §8.4 « Espace sur un bouton
// sélectionné », §8.5 « Espace+glisser »). Chromium (WebView2) leaves the focus on a button pressed with the mouse,
// and that button already matches :focus-visible at the next keydown: the next Space would press it again (Ajuster,
// RESET, Enregistrer…) instead of reaching the schematic. Mouse presses therefore leave the focus where it was.

/** Form fields keep their keys (§8.4). */
export function isField(t: EventTarget | null): boolean {
  return t instanceof HTMLInputElement || t instanceof HTMLSelectElement || t instanceof HTMLTextAreaElement ||
    (t instanceof HTMLElement && t.isContentEditable);
}

/**
 * mousedown while the view is shown: a press on a button leaves the focus where it was (the click still happens).
 * A focused field is left all the same, so that it commits its value before the click. Focus moved with the
 * keyboard (Tab) is unchanged, and such a button keeps its Space and Enter.
 */
export function keepFocusOnPress(e: MouseEvent): void {
  const b = e.target instanceof Element ? e.target.closest("button") : null;
  if (!b || b.disabled) return;
  e.preventDefault();
  const active = b.ownerDocument.activeElement;
  if (active instanceof HTMLElement && isField(active)) active.blur();
}
