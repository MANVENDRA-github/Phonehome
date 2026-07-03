// @perf — PROOF §M4 measurement protocol. CI-excluded (grep-invert @perf):
// frame rates are only meaningful on real GPUs, run headed via the
// chromium-webgpu project (append ?gl=1 via PERF_GL=1 for the WebGL A/B).
//
// Protocol per cell: load → 10 s warm-up → reset stats → 30 s measure with
// the camera auto-rotating (worst-case overdraw) → dump frame-time
// percentiles + the GPU adapter info (named-hardware requirement, D-008)
// to e2e-results/perf-*.json and a markdown row to stdout.

import { test } from "@playwright/test";
import { mkdirSync, writeFileSync } from "node:fs";

/* eslint-disable @typescript-eslint/no-explicit-any */

const STRESS_LEVELS = [0, 2500, 5000, 10000];
const WARMUP_MS = 10_000;
const MEASURE_MS = 30_000;
const forceGl = process.env.PERF_GL === "1";

for (const stress of STRESS_LEVELS) {
  test(`perf ${stress === 0 ? "fixture volume" : `${stress} arcs`} @perf`, async ({ page }) => {
    test.setTimeout(WARMUP_MS + MEASURE_MS + 120_000);

    const query = new URLSearchParams();
    if (stress > 0) query.set("stress", String(stress));
    if (forceGl) query.set("gl", "1");
    await page.goto(`/?${query}`);
    await page.waitForFunction(
      (min) => (window as any).__phonehome?.ready && (window as any).__phonehome.arcCount >= min,
      stress || 1,
      { timeout: 90_000 },
    );

    await page.waitForTimeout(WARMUP_MS);
    await page.evaluate(() => (window as any).__phonehome.resetStats());
    await page.waitForTimeout(MEASURE_MS);

    const stats = await page.evaluate(() => (window as any).__phonehome.frameStats());
    const adapter = await page.evaluate(async () => {
      const a = await (navigator as any).gpu?.requestAdapter?.();
      const info = a?.info;
      return info
        ? { vendor: info.vendor, architecture: info.architecture, description: info.description }
        : null;
    });
    const result = {
      requested_arcs: stress === 0 ? "fixture" : stress,
      measured: stats,
      adapter,
      userAgent: await page.evaluate(() => navigator.userAgent),
      viewport: page.viewportSize(),
      recordedAt: new Date().toISOString(),
    };

    mkdirSync("e2e-results", { recursive: true });
    const name = `perf-${stats.backend}-${stress === 0 ? "fixture" : stress}${forceGl ? "-gl" : ""}.json`;
    writeFileSync(`e2e-results/${name}`, JSON.stringify(result, null, 2));

    // PROOF-ready markdown row.
    console.log(
      `| ${stress === 0 ? "fixture (~29)" : stress.toLocaleString()} | ${stats.backend} | ` +
        `${stats.fps.toFixed(1)} | ${stats.avg_ms.toFixed(2)} | ${stats.p50_ms.toFixed(2)} | ` +
        `${stats.p95_ms.toFixed(2)} | ${stats.p99_ms.toFixed(2)} | dpr ${stats.dpr} |`,
    );
  });
}
