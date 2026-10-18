// UI-side measurements: batch cadence, render cost, frame gaps, and the latency from a stimulus
// change to the first displayed answer computed from it (UI -> core -> STM32 -> core -> UI).

import type { FrameRow } from "./types";

class Ring {
  private data: number[] = [];
  constructor(private cap: number) {}
  add(v: number): void {
    this.data.push(v);
    if (this.data.length > this.cap) this.data.shift();
  }
  values(): number[] {
    return this.data;
  }
  clear(): void {
    this.data = [];
  }
}

export function quantile(values: number[], q: number): number {
  if (!values.length) return 0;
  const s = [...values].sort((a, b) => a - b);
  return s[Math.min(s.length - 1, Math.max(0, Math.ceil(q * s.length) - 1))];
}

export interface Summary {
  count: number;
  p50: number;
  p95: number;
  p99: number;
  max: number;
}

export function summarize(values: number[]): Summary {
  return {
    count: values.length,
    p50: quantile(values, 0.5),
    p95: quantile(values, 0.95),
    p99: quantile(values, 0.99),
    max: values.length ? Math.max(...values) : 0,
  };
}

export class UiPerf {
  batchIntervals = new Ring(4000);
  renderMs = new Ring(4000);
  frameGaps = new Ring(4000);
  latencies = new Ring(4000);
  private lastBatch = 0;
  private lastFrame = 0;
  /** Times the page became hidden (no rendering while hidden). */
  hiddenEvents = 0;

  constructor() {
    document.addEventListener("visibilitychange", () => {
      if (document.visibilityState === "hidden") this.hiddenEvents += 1;
    });
  }
  private probes: { rev: number; t0: number }[] = [];

  onBatch(now: number): void {
    if (this.lastBatch) this.batchIntervals.add(now - this.lastBatch);
    this.lastBatch = now;
  }

  onAnimationFrame(now: number): void {
    if (this.lastFrame) this.frameGaps.add(now - this.lastFrame);
    this.lastFrame = now;
  }

  onRender(ms: number): void {
    this.renderMs.add(ms);
  }

  /** A stimulus revision was just sent to the core. */
  probe(rev: number): void {
    this.probes.push({ rev, t0: performance.now() });
    if (this.probes.length > 64) this.probes.shift();
  }

  /** Called after rows were put on screen. */
  displayed(rows: FrameRow[], now: number): void {
    if (!this.probes.length || !rows.length) return;
    const maxRev = rows.reduce((m, r) => (r.outcome === "result" ? Math.max(m, r.stim_rev) : m), -1);
    while (this.probes.length && this.probes[0].rev <= maxRev) {
      const p = this.probes.shift()!;
      this.latencies.add(now - p.t0);
    }
  }

  reset(): void {
    for (const r of [this.batchIntervals, this.renderMs, this.frameGaps, this.latencies]) r.clear();
    this.probes = [];
    this.hiddenEvents = 0;
  }

  summary() {
    return {
      hidden_events: this.hiddenEvents,
      batch_interval_ms: summarize(this.batchIntervals.values()),
      render_ms: summarize(this.renderMs.values()),
      frame_gap_ms: summarize(this.frameGaps.values()),
      stimulus_to_display_ms: summarize(this.latencies.values()),
    };
  }
}
