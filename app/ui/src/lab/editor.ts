// Interface between the schematic editor (lot C, ui/src/lab/canvas/) and the Physical Lab view (lot P).
// Contract: docs/physical-lab-v1.md §8.4 and §8.5. The editor draws LabState and turns gestures into
// LabController calls; it owns no document data and never validates geometry itself (previews do).

import type { LabController, LabState } from "./state";
import type { Kind, Pt } from "./types";

export interface EditorMetrics {
  /** JS time of the last frame() calls that drew something, ms (ring of the last 600). */
  renderMs: number[];
  /** Full repaints of the scene layer since creation. */
  fullRepaints: number;
  /** Partial (dirty rectangle) repaints of the scene layer since creation. */
  partialRepaints: number;
  /** Overlay repaints since creation. */
  overlayRepaints: number;
}

export interface Editor {
  /**
   * Draws the dirty layers. Called once per animation frame by the view's rAF loop (ui/src/main.ts), only
   * while the Physical Lab view is active; input handlers never draw.
   */
  frame(now: number): void;
  /** Selection (default, Escape) or wire tool (W). Mirrored in LabState.view.tool. */
  setTool(tool: "select" | "wire"): void;
  /** Pointer placement armed from the library panel: ghost under the pointer, click to place, Escape to cancel. */
  armPlacement(kind: Kind): void;
  /** Cancels the gesture in progress (preview dropped, nothing sent). */
  cancelGesture(): void;
  /** Fits the circuit to the view (F). */
  fit(): void;
  /** Zoom to a ladder step closest to percent (1 = 100 %). Mirrored in LabState.view.zoomPercent. */
  zoomTo(percent: number): void;
  zoomStep(direction: 1 | -1): void;
  /** World grid point → client CSS pixels, for synthetic input in e2e scenarios. */
  worldToClient(p: Pt): { x: number; y: number };
  /** Everything dirty (view shown again, fonts loaded, DPR change). */
  invalidate(): void;
  /** Hidden views do not draw and release pointer captures (a held push button is released). */
  setActive(active: boolean): void;
  readonly metrics: EditorMetrics;
  destroy(): void;
}

/** Implemented by ui/src/lab/canvas/index.ts as `export const createEditor: CreateEditor`. */
export type CreateEditor = (host: HTMLElement, state: LabState, controller: LabController) => Editor;
