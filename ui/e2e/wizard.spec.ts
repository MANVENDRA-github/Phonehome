// The first-run setup wizard (M5). Runs in the normal smoke suite: it drives the
// REAL compiled UI (served by the smoke daemon) but stubs the /api surface with
// page.route, so it is independent of the daemon's source state. That stubbing
// is the only faithful way to exercise a fresh-install screen, which by
// definition cannot coexist with the fixture-configured smoke daemon (whose
// /api/config reports needs_setup:false).

import { expect, test } from "@playwright/test";

/** Route the endpoints the wizard flow touches. `saved` flips config to
 * needs_setup:false to model the post-save transition back to the app. */
async function stubApi(
  page: import("@playwright/test").Page,
  opts: { probeOk: boolean; probeError?: string },
) {
  const state = { saved: false };

  await page.route("**/api/config", (route) =>
    route.fulfill({
      json: { home: null, version: "test", needs_setup: !state.saved },
    }),
  );
  await page.route("**/api/health", (route) =>
    route.fulfill({ json: { status: "alive", version: "test" } }),
  );
  // Quiet the app's other startup fetches once it leaves the wizard.
  await page.route("**/api/devices", (route) => route.fulfill({ json: [] }));
  await page.route("**/api/stats", (route) =>
    route.fulfill({
      json: {
        total_queries: 0,
        total_blocked: 0,
        distinct_domains: 0,
        distinct_clients: 0,
        distinct_devices: 0,
        rollup_rows: 0,
        sources: [],
      },
    }),
  );
  await page.route("**/api/arcs*", (route) =>
    route.fulfill({ json: { arcs: [], unmapped_queries: 0 } }),
  );

  await page.route("**/api/sources/test", (route) =>
    opts.probeOk
      ? route.fulfill({ status: 200, json: { ok: true } })
      : route.fulfill({ status: 502, json: { ok: false, error: opts.probeError ?? "boom" } }),
  );
  await page.route("**/api/sources", (route) => {
    if (route.request().method() === "POST") {
      state.saved = true;
      return route.fulfill({
        status: 201,
        json: {
          id: "pihole-main",
          kind: "pihole",
          base_url: "http://pi.hole",
          username: null,
          interval_s: 15,
          enabled: true,
        },
      });
    }
    return route.fulfill({ json: [] });
  });
}

test("fresh install shows the wizard and surfaces a failed connection test", async ({
  page,
}) => {
  await stubApi(page, { probeOk: false, probeError: "pihole auth rejected (bad password?)" });
  await page.goto("/");

  await expect(page.getByTestId("setup-wizard")).toBeVisible();

  // Test is disabled until the form validates.
  await expect(page.getByTestId("setup-test")).toBeDisabled();
  await page.getByTestId("setup-url").fill("http://pi.hole");
  await page.getByTestId("setup-secret").fill("wrongpass");
  await expect(page.getByTestId("setup-test")).toBeEnabled();

  // Failed probe → rose error, Start stays disabled.
  await page.getByTestId("setup-test").click();
  await expect(page.getByTestId("setup-test-result")).toContainText("bad password");
  await expect(page.getByTestId("setup-submit")).toBeDisabled();
});

test("adguard reveals a username field and requires it", async ({ page }) => {
  await stubApi(page, { probeOk: true });
  await page.goto("/");

  await expect(page.getByTestId("setup-username")).toBeHidden();
  await page.getByTestId("setup-kind-adguard").click();
  await expect(page.getByTestId("setup-username")).toBeVisible();

  await page.getByTestId("setup-url").fill("http://adguard.local");
  await page.getByTestId("setup-secret").fill("pw");
  // Username still empty → cannot test yet.
  await expect(page.getByTestId("setup-test")).toBeDisabled();
  await page.getByTestId("setup-username").fill("admin");
  await expect(page.getByTestId("setup-test")).toBeEnabled();
});

test("a good test then Start transitions out of the wizard", async ({ page }) => {
  await stubApi(page, { probeOk: true });
  await page.goto("/");

  await page.getByTestId("setup-url").fill("http://pi.hole");
  await page.getByTestId("setup-secret").fill("token");
  await page.getByTestId("setup-test").click();
  await expect(page.getByTestId("setup-test-result")).toContainText("connected");

  // Start is enabled only after a green test.
  await expect(page.getByTestId("setup-submit")).toBeEnabled();
  await page.getByTestId("setup-submit").click();

  // onDone refetches config (now needs_setup:false) → wizard gone, app shown.
  await expect(page.getByTestId("setup-wizard")).toBeHidden();
  await expect(page.locator("main")).toContainText("Meet everything your house talks to");
});
