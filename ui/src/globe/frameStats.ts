// Frame-time ring buffer feeding the ?hud=1 overlay and the Playwright perf
// harness (PROOF §M4 numbers come from FrameStats.summary()).

export type FrameSummary = {
  frames: number;
  seconds: number;
  fps: number;
  avg_ms: number;
  p50_ms: number;
  p95_ms: number;
  p99_ms: number;
  backend: string;
  dpr: number;
};

const CAPACITY = 4096;

export class FrameStats {
  private deltas = new Float32Array(CAPACITY);
  private count = 0;
  private index = 0;
  private last = 0;
  backend = "none";

  /** Call once per rendered frame with a high-res timestamp (ms). */
  tick(nowMs: number) {
    if (this.last > 0) {
      this.deltas[this.index] = nowMs - this.last;
      this.index = (this.index + 1) % CAPACITY;
      if (this.count < CAPACITY) this.count++;
    }
    this.last = nowMs;
  }

  /** Drop the window (used before a measured perf run). */
  reset() {
    this.count = 0;
    this.index = 0;
    this.last = 0;
  }

  summary(): FrameSummary {
    const n = this.count;
    const window = Array.from(this.deltas.slice(0, n)).sort((a, b) => a - b);
    const total = window.reduce((s, d) => s + d, 0);
    const pct = (p: number) => (n ? window[Math.min(n - 1, Math.floor((p / 100) * n))] : 0);
    return {
      frames: n,
      seconds: total / 1000,
      fps: total > 0 ? (n * 1000) / total : 0,
      avg_ms: n ? total / n : 0,
      p50_ms: pct(50),
      p95_ms: pct(95),
      p99_ms: pct(99),
      backend: this.backend,
      dpr: Math.min(globalThis.devicePixelRatio ?? 1, 2),
    };
  }
}
