// @hero — records the raw video for the 10-second launch GIF (SPEC M4 proof
// asset, RESEARCH §5). CI-excluded; run headed via chromium-webgpu so the real
// GPU renders it: `npm run hero` (scripts/record-hero.mjs drives this spec and
// the ffmpeg conversion).
//
// The fixture provenance badge is part of the page (D-009), so every frame of
// the recording carries the "replayed fixture" label with no editing step.

import { test } from "@playwright/test";

/* eslint-disable @typescript-eslint/no-explicit-any */

test("record hero footage @hero", async ({ browser }) => {
  test.setTimeout(300_000);
  const context = await browser.newContext({
    viewport: { width: 1280, height: 720 },
    recordVideo: { dir: "e2e-results/video", size: { width: 1280, height: 720 } },
  });
  const page = await context.newPage();
  // gl=1: headed-WebGPU canvases don't composite into CDP screencast (they
  // record black); the WebGL2 fallback is visually identical and captures
  // cleanly. Backend perf is measured separately in perf.spec.ts.
  await page.goto("http://localhost:8480/?hero=1&gl=1");
  await page.waitForFunction(
    () => (window as any).__phonehome?.ready && (window as any).__phonehome.arcCount > 0,
    undefined,
    { timeout: 90_000 },
  );
  // Let the replay pulse arcs and the hero choreography cycle device callouts;
  // record generously — ffmpeg trims to the best 10 s.
  await page.waitForTimeout(24_000);
  const video = page.video();
  await context.close(); // flushes the webm
  const path = await video?.path();
  console.log(`HERO_VIDEO=${path}`);
});
