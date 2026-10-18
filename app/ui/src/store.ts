// UI state, independent of any rendering. Batches from the core (20 Hz) are applied here; views
// read the store on the next animation frame and only touch what the dirty flags name. A future
// Canvas2D view (Physical Lab) plugs into the same store and render loop.

import type { ErrorRow, FrameRow, LogRow, SlotView, Stats, Status, UiBatch } from "./types";

export const ROWS_KEPT = 200;
export const LOGS_KEPT = 300;
export const ERRORS_KEPT = 200;

export interface Dirty {
  status: boolean;
  stats: boolean;
  rows: boolean;
  logs: boolean;
  errors: boolean;
  slots: boolean;
}

export class Store {
  status: Status | null = null;
  stats: Stats | null = null;
  slots: SlotView[] = [];
  /** Newest last. */
  rows: FrameRow[] = [];
  logs: LogRow[] = [];
  errors: ErrorRow[] = [];
  /** Items received since the last render, consumed by the views. */
  newRows: FrameRow[] = [];
  newLogs: LogRow[] = [];
  newErrors: ErrorRow[] = [];
  rowsReceived = 0;
  logsReceived = 0;
  errorsReceived = 0;
  dirty: Dirty = { status: false, stats: false, rows: false, logs: false, errors: false, slots: false };

  apply(b: UiBatch): void {
    this.status = b.status;
    this.stats = b.stats;
    this.dirty.status = this.dirty.stats = true;
    if (b.slots.length || this.slots.length) {
      this.slots = b.slots;
      this.dirty.slots = true;
    }
    if (b.rows.length) {
      this.rowsReceived += b.rows.length;
      push(this.rows, b.rows, ROWS_KEPT);
      push(this.newRows, b.rows, ROWS_KEPT);
      this.dirty.rows = true;
    }
    if (b.logs.length) {
      this.logsReceived += b.logs.length;
      push(this.logs, b.logs, LOGS_KEPT);
      push(this.newLogs, b.logs, LOGS_KEPT);
      this.dirty.logs = true;
    }
    if (b.errors.length) {
      this.errorsReceived += b.errors.length;
      push(this.errors, b.errors, ERRORS_KEPT);
      push(this.newErrors, b.errors, ERRORS_KEPT);
      this.dirty.errors = true;
    }
  }

  anyDirty(): boolean {
    const d = this.dirty;
    return d.status || d.stats || d.rows || d.logs || d.errors || d.slots;
  }

  clean(): void {
    this.newRows = [];
    this.newLogs = [];
    this.newErrors = [];
    this.dirty = { status: false, stats: false, rows: false, logs: false, errors: false, slots: false };
  }
}

function push<T>(into: T[], items: T[], cap: number): void {
  into.push(...items);
  if (into.length > cap) into.splice(0, into.length - cap);
}
