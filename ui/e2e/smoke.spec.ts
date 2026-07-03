// The ARCHITECTURE §5 Playwright smoke: daemon + fixture replay → globe
// renders arcs → click-through reaches raw rollups in 2 clicks → scorecard
// shows fixture values. Functional assertions only — must pass on SwiftShader
// WebGL in CI; frame rate is never asserted here (that's the @perf suite).

import { expect, test } from "@playwright/test";

/* eslint-disable @typescript-eslint/no-explicit-any */

test("globe renders fixture arcs, provenance badge, and labeled devices", async ({ page }) => {
  await page.goto("/");
  await page.waitForFunction(
    () => (window as any).__phonehome?.ready && (window as any).__phonehome.arcCount > 0,
    undefined,
    { timeout: 90_000 },
  );

  // D-009: any recording of this app must carry the fixture label.
  await expect(page.getByTestId("fixture-badge")).toBeVisible();
  await expect(page.getByTestId("fixture-badge")).toContainText("replayed fixture");

  // The filter rail is the "labeled devices" surface.
  await expect(page.getByTestId("filter-rail")).toContainText("Samsung Electronics");
  await expect(page.getByTestId("filter-rail")).toContainText("ASUSTek Computer");

  const arcCount = await page.evaluate(() => (window as any).__phonehome.arcCount);
  expect(arcCount).toBeGreaterThanOrEqual(15);
});

test("arc click-through reaches raw rollup data in two clicks", async ({ page }) => {
  await page.goto("/");
  await page.waitForFunction(
    () => (window as any).__phonehome?.ready && (window as any).__phonehome.arcCount > 0,
    undefined,
    { timeout: 90_000 },
  );

  // Freeze auto-rotation so the sampled click target stays valid.
  await page.evaluate(() => (window as any).__phonehome.setAutoRotate(0));
  await page.waitForTimeout(500);

  const canvas = page.locator('[data-testid="globe"] canvas');
  const box = await canvas.boundingBox();
  expect(box).not.toBeNull();

  // Click 1: an arc → domain list. While the fixture is still replaying, a
  // debounced /api/arcs refetch can rebuild+reorder arcs between sampling the
  // click target and the click landing — re-sample fresh and retry.
  await expect(async () => {
    const point = await page.evaluate(() => (window as any).__phonehome.arcScreenPoint(0));
    expect(point).not.toBeNull();
    await page.mouse.click(box!.x + point!.x, box!.y + point!.y);
    await expect(page.getByTestId("drill-panel")).toBeVisible({ timeout: 2_000 });
  }).toPass({ timeout: 30_000 });
  const domains = page.locator('[data-testid="drill-domains"] li');
  await expect(domains.first()).toBeVisible();

  // Click 2: a domain → its raw hourly rollup buckets (rawest data, D-005).
  await domains.first().locator("button").click();
  await expect(page.getByTestId("drill-rollups")).toBeVisible();
  const rows = page.locator('[data-testid="drill-rollups"] tbody tr');
  expect(await rows.count()).toBeGreaterThan(0);
});

test("device table + scorecard render fixture values; unmapped traffic disclosed", async ({
  page,
}) => {
  await page.goto("/");
  await page.waitForFunction(
    () => (window as any).__phonehome?.ready && (window as any).__phonehome.arcCount > 0,
    undefined,
    { timeout: 90_000 },
  );

  // 18 fixture devices resolve to named rows.
  await expect(page.locator("main")).toContainText("Devices");
  await expect(async () => {
    const count = await page.locator("tbody tr").count();
    expect(count).toBeGreaterThanOrEqual(18);
  }).toPass({ timeout: 30_000 });

  // Expand the busiest device: its explainable scorecard renders with inputs.
  await page.locator("tbody tr").first().click();
  await expect(page.getByText("privacy risk")).toBeVisible();
  await expect(page.getByText("Tracker share")).toBeVisible();
  await expect(page.getByText(/queries to trackers/)).toBeVisible();

  // pool.ntp.org has no mapped country — disclosed, not hidden (D-001/D-011).
  await expect(page.getByTestId("unmapped-note")).toBeVisible();
});
