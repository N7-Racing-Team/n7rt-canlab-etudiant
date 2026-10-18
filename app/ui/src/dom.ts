// DOM helpers. Rule of this UI: data coming from the board, the ports or the student's code is
// only ever written with textContent / attribute setters, never parsed as HTML
// (scripts/check-ui.mjs fails the build on innerHTML, outerHTML, insertAdjacentHTML, document.write).

export function $<T extends HTMLElement = HTMLElement>(selector: string, root: ParentNode = document): T {
  const el = root.querySelector<T>(selector);
  if (!el) throw new Error(`élément introuvable : ${selector}`);
  return el;
}

/** Creates an element with optional class and text. */
export function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  cls?: string,
  text?: string,
): HTMLElementTagNameMap[K] {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
}

/** Sets text only when it changed (avoids needless layout work at 20 Hz). */
export function setText(e: Element, text: string): void {
  if (e.textContent !== text) e.textContent = text;
}

export function clear(e: Element): void {
  while (e.firstChild) e.removeChild(e.firstChild);
}

/** Replaces the options of a <select>, keeping the selection when still present. */
export function setOptions(select: HTMLSelectElement, options: { value: string; label: string }[], keep: string): void {
  const same =
    select.options.length === options.length &&
    options.every((o, i) => select.options[i].value === o.value && select.options[i].text === o.label);
  if (!same) {
    clear(select);
    for (const o of options) {
      const opt = el("option", undefined, o.label);
      opt.value = o.value;
      select.appendChild(opt);
    }
  }
  if (options.some((o) => o.value === keep)) select.value = keep;
}
