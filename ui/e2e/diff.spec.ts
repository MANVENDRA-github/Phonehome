// The weekly-diff view (M6). Like the wizard spec, drives the real compiled UI
// with a page.route-stubbed /api surface so the assertion is deterministic and
// independent of the 60s server-side snapshot cadence. (The real week_diffs
// computation is proven by Rust unit tests; the reshaped fixture's real delta is
// captured in PROOF §M5.)

import { expect, test } from "@playwright/test";

async function stubApi(page: import("@playwright/test").Page) {
  await page.route("**/api/config", (route) =>
    route.fulfill({ json: { home: { lat: 12.97, lon: 77.59 }, version: "test", needs_setup: false } }),
  );
  await page.route("**/api/health", (route) =>
    route.fulfill({ json: { status: "alive", version: "test" } }),
  );
  await page.route("**/api/devices", (route) => route.fulfill({ json: [] }));
  await page.route("**/api/stats", (route) =>
    route.fulfill({
      json: {
        total_queries: 1,
        total_blocked: 0,
        distinct_domains: 1,
        distinct_clients: 1,
        distinct_devices: 1,
        rollup_rows: 1,
        sources: [{ id: "pihole-main", kind: "pihole", cursor: "x", last_ok_at: 1 }],
      },
    }),
  );
  await page.route("**/api/arcs*", (route) =>
    route.fulfill({ json: { arcs: [], unmapped_queries: 0 } }),
  );
  // The star of the show: the Samsung TV picked up two new trackers, risk up.
  await page.route("**/api/diffs", (route) =>
    route.fulfill({
      json: {
        current_week_start: 1_782_345_600_000,
        previous_week_start: 1_781_740_800_000,
        devices: [
          {
            device_id: 3,
            device_name: "Samsung Electronics · 22:33",
            current: {
              distinct_domains: 11,
              tracker_domains: 6,
              distinct_entities: 4,
              distinct_countries: 3,
              volume: 900,
              blocked: 500,
              score: 74,
            },
            previous: {
              distinct_domains: 9,
              tracker_domains: 4,
              distinct_entities: 3,
              distinct_countries: 3,
              volume: 850,
              blocked: 470,
              score: 66,
            },
            new_domains: [
              { domain: "samsungadhub.com", is_tracker: true, country: "KR", queries: 32 },
              { domain: "nmp.samsungqbe.com", is_tracker: true, country: "KR", queries: 33 },
            ],
          },
        ],
      },
    }),
  );
}

test("weekly-diff shows a device's week-over-week delta and new trackers", async ({ page }) => {
  await stubApi(page);
  await page.goto("/");

  const panel = page.getByTestId("weekly-diff");
  await expect(panel).toBeVisible();
  await expect(panel).toContainText("Samsung Electronics");

  const card = page.getByTestId("diff-device-3");
  // Risk rose 66 → 74: a +8 delta chip.
  await expect(card).toContainText("74");
  await expect(card).toContainText("+8");
  // Count deltas: +2 domains, +2 trackers.
  await expect(card).toContainText("trackers");

  // The two new tracker domains are listed.
  const newDomains = page.getByTestId("diff-new-domains");
  await expect(newDomains).toContainText("samsungadhub.com");
  await expect(newDomains).toContainText("nmp.samsungqbe.com");
  await expect(newDomains).toContainText("new this week (2)");
});

test("weekly-diff is hidden when there is no previous week", async ({ page }) => {
  await stubApi(page);
  await page.route("**/api/diffs", (route) =>
    route.fulfill({
      json: { current_week_start: 1_782_345_600_000, previous_week_start: null, devices: [] },
    }),
  );
  await page.goto("/");
  await expect(page.locator("main")).toContainText("The globe");
  await expect(page.getByTestId("weekly-diff")).toHaveCount(0);
});
