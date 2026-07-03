// Playwright harness (SPEC M4): the smoke test doubles as the CI gate and the
// same setup records the hero GIF and produces the PROOF §M4 perf numbers.
//
// The system under test is the REAL daemon (embedded UI, not the vite dev
// server) replaying the committed fixture into a FRESH temp database each run
// — the cursor persists per-DB, so a fresh DB is what guarantees live SSE
// pulses fire while tests/recordings are running.

import { defineConfig } from "@playwright/test";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const CI = !!process.env.CI;
const freshDb = join(mkdtempSync(join(tmpdir(), "phonehome-e2e-")), "e2e.db");

export default defineConfig({
  testDir: "e2e",
  outputDir: "e2e-results/artifacts",
  fullyParallel: false, // one daemon, one replay timeline
  workers: 1,
  retries: CI ? 1 : 0,
  reporter: CI ? [["list"], ["html", { open: "never" }]] : [["list"]],
  timeout: 120_000,
  use: {
    baseURL: "http://localhost:8480",
    trace: CI ? "retain-on-failure" : "off",
  },
  projects: [
    {
      // CI-safe: headless; on runners without a GPU this lands on SwiftShader
      // WebGL — assertions are functional only, never about frame rate.
      name: "chromium",
      use: {
        browserName: "chromium",
        launchOptions: { args: CI ? ["--use-angle=swiftshader"] : [] },
      },
    },
    {
      // Local perf/GIF project: headed so the real GPU (and WebGPU) is used.
      name: "chromium-webgpu",
      use: {
        browserName: "chromium",
        headless: false,
        launchOptions: { args: ["--enable-unsafe-webgpu"] },
      },
    },
  ],
  webServer: {
    command: "cargo run -p phonehome-daemon",
    cwd: "..",
    url: "http://localhost:8480/api/health",
    reuseExistingServer: false,
    timeout: 300_000, // may include a cargo build
    env: {
      PHONEHOME_FIXTURE: "fixtures/household-01.jsonl",
      PHONEHOME_DB: freshDb,
      // City-level coordinates only in recordings (D-013); Bengaluru.
      PHONEHOME_HOME_LAT: process.env.PHONEHOME_HOME_LAT ?? "12.97",
      PHONEHOME_HOME_LON: process.env.PHONEHOME_HOME_LON ?? "77.59",
    },
  },
});
