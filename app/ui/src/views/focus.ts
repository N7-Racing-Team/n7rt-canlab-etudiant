// Focus kept across a redraw of the fiche forms (views/progress.ts, views/incident.ts). Every control of a form carries
// a stable key in data-key (« trames.1.dlc »); before a redraw the focused control's key and caret are noted, after it
// the control of the same key gets them back. Without a real document (the fake DOM of the unit tests has neither
// document.activeElement nor querySelectorAll) both functions do nothing.

export interface FocusMemo {
  key: string;
  start: number | null;
  end: number | null;
}

/** The focused control of `root` (its key and caret), or null. */
export function captureFocus(root: HTMLElement): FocusMemo | null {
  const doc = typeof document === "undefined" ? undefined : (document as Partial<Document>);
  const active = doc?.activeElement as HTMLElement | null | undefined;
  if (!active || typeof root.contains !== "function" || !root.contains(active)) return null;
  const key = active.dataset?.key;
  if (!key) return null;
  let start: number | null = null;
  let end: number | null = null;
  try {
    const t = active as HTMLInputElement;
    start = typeof t.selectionStart === "number" ? t.selectionStart : null;
    end = typeof t.selectionEnd === "number" ? t.selectionEnd : null;
  } catch {
    // a select or a button has no caret
  }
  return { key, start, end };
}

/** Focuses the control of `root` that has the memo's key, caret included. `scroll`: let the browser bring it into view
 * (a new record); otherwise the scroll position is kept (a redraw). */
export function restoreFocus(root: HTMLElement, memo: FocusMemo | null, scroll = false): void {
  if (!memo || typeof root.querySelectorAll !== "function") return;
  const target = Array.from(root.querySelectorAll<HTMLElement>("[data-key]")).find((e) => e.dataset.key === memo.key);
  if (!target || (target as HTMLInputElement).disabled || target.closest("[hidden]")) return;
  target.focus({ preventScroll: !scroll });
  if (memo.start === null) return;
  try {
    (target as HTMLInputElement).setSelectionRange(memo.start, memo.end ?? memo.start);
  } catch {
    // not a text field
  }
}
