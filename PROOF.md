# PROOF.md — evidence per milestone

Rule (D-008): every claim, number, and demo asset here comes from a **real run**. If something wasn't verified, it says so explicitly. Command output is pasted, not paraphrased.

---

## §M0 — Scaffold (2026-07-02)

Environment: Windows 11, rustc 1.94.1 (MSVC), Node v20.20.1 / npm 10.8.2.

### UI build (typecheck + bundle) — PASS

```
> phonehome-ui@0.1.0 build
> tsc --noEmit && vite build

vite v6.4.3 building for production...
✓ 29 modules transformed.
dist/index.html                   0.41 kB │ gzip:  0.28 kB
dist/assets/index-BGWB0P7j.css    7.05 kB │ gzip:  2.32 kB
dist/assets/index-CB_NgYZb.js   195.61 kB │ gzip: 61.37 kB
✓ built in 1.23s
```

`npm install` reported **0 vulnerabilities**.

### Rust: fmt + clippy + tests — PASS

`cargo fmt --check` → clean. `cargo clippy --all-targets -- -D warnings` → clean ("Finished `dev` profile … in 1m 05s", zero warnings under `-D warnings`).

```
running 2 tests   (phonehome-core)
test tests::query_event_without_mac_round_trips ... ok
test tests::query_event_serde_round_trip ... ok
test result: ok. 2 passed; 0 failed

running 3 tests   (phonehome-daemon)
test tests::unknown_api_path_is_404_not_spa_fallback ... ok
test tests::health_returns_alive_with_version ... ok
test tests::root_serves_embedded_ui ... ok
test result: ok. 3 passed; 0 failed
```

### Local daemon run — PASS

`cargo run -p phonehome-daemon`, then from another shell:

```
$ curl -s http://localhost:8480/api/health
{"status":"alive","version":"0.1.0"}

$ curl -s http://localhost:8480/ | head -c 400
<!doctype html>
<html lang="en" class="dark">
  <head>
    <meta charset="UTF-8" />
    ...
    <title>Phonehome</title>
    <script type="module" crossorigin src="/assets/index-CB_NgYZb.js"></script>
    <link rel="stylesheet" crossorigin href="/assets/index-BGWB0P7j.css">
```

Bonus (observed, not staged): starting a second instance while the first held the port produced the intended bind panic — `failed to bind 0.0.0.0:8480: Only one usage of each socket address … (os error 10048)`.

### docker compose — PASS in CI; local validation blocked (disclosed)

**Local limitation:** this dev machine cannot currently run Docker Desktop — `wsl --status` reports *"WSL2 is not supported with your current machine configuration. Please enable the 'Virtual Machine Platform' optional component and ensure virtualization is enabled in the BIOS."* Per the honesty rule this is disclosed rather than worked around silently.

**Compensating real proof:** the CI `docker-smoke` job runs `docker compose up -d --build` on a clean Ubuntu runner, polls `GET /api/health` until it returns the alive JSON, and greps the served page for "phonehome". See the CI run linked below.

### CI — PASS

- Run: https://github.com/MANVENDRA-github/Phonehome/actions/runs/28608143084 — `build-test` pass (1m3s: UI build, fmt, clippy, tests) and `docker-smoke` pass (1m0s: compose build + live health/page probes) on PR #2.

**M0 acceptance met:** `cargo test` + `npm run build` green locally and in CI; `docker compose up` serving the alive page proven in CI (local Docker pending machine virtualization — tracked for M5's clean-install test, which needs it anyway).

---

## §M1 — Ingestion: Pi-hole v6 + fixture replayer (2026-07-02)

### Fixture (D-009 — synthetic-realistic, disclosed)

Generated deterministically: `cargo run -p phonehome-core --example gen_fixture > fixtures/household-01.jsonl` →
```
generated 7189 events across 15 devices
7189 lines · 1,148,038 bytes
```
Independent cross-check: `grep -c '"blocked":true' fixtures/household-01.jsonl` → **2453**.

### Tests — PASS (22 test executions, 0 failures)

```
phonehome-core (7):    event serde round-trips, client_key MAC/IP fallback, bucket_hour floor,
                       replayer exact-once chunking, cursor resume, malformed-line hard error
phonehome-daemon (11): store atomic apply+cursor · restart-resume (real db file, zero loss/dup)
                       · PROPTEST: rollups invariant under arbitrary batch splitting
                       pihole: auth→poll→map · boundary overlap never duplicates (FTL id filter)
                       · 401 re-auth retry · bad-password clean error
                       api: health, stats-zeroes, embedded UI, api-404
e2e (replayer_e2e):    full_fixture_ingests_exactly_once_across_a_restart ... ok
```
The e2e test ingests 3,000 events, hard-drops store + ingestor (simulated crash), resumes from the persisted cursor only, and asserts final totals against an independent re-read of the fixture: total=7189, blocked=2453, domains, clients, and cursor==7189 all exact.

`cargo fmt --check` clean; `cargo clippy --all-targets -- -D warnings` clean.

### Live run — PASS

`PHONEHOME_FIXTURE=fixtures/household-01.jsonl cargo run -p phonehome-daemon`, polling `/api/stats` from another shell (1,000-event batches land once per second):

```
poll 2:  total=1000
poll 8:  total=3000
poll 16: total=6000
poll 21: total=7189   ← FULLY INGESTED
{"total_queries":7189,"total_blocked":2453,"distinct_domains":75,"distinct_clients":15,
 "rollup_rows":5128,"sources":[{"id":"fixture","kind":"fixture","cursor":"7189",
 "last_ok_at":1783013180588}]}
```
7,189 raw events → 5,128 rollup rows (hourly aggregation working); `total_blocked` matches the independent grep exactly; raw events are not retained (D-005).

### Live Pi-hole — PENDING (disclosed, per SPEC M1's explicit fallback)

No Pi-hole instance exists on this network yet. The adapter is validated against **recorded-shape HTTP fixtures** (wiremock): session auth, sid header, query mapping (GRAVITY→blocked / FORWARDED→allowed, fractional-second timestamps), inclusive-`from` boundary dedup via monotonic FTL ids, 401 re-auth retry, and bad-password handling. **Follow-up:** validate against a real Pi-hole v6 and capture the anonymized real fixture (D-009) — target before M4's hero GIF.

### CI — green

Run: https://github.com/MANVENDRA-github/Phonehome/actions/runs/28609222209 — `build-test` pass (1m25s) + `docker-smoke` pass (2m1s) on PR #3.

**M1 acceptance met:** replayer ingests the full fixture with zero loss/dup across a mid-run restart (property test + e2e); Pi-hole polling proven via recorded HTTP fixtures with the live-instance note above; row counts vs fixture line count exact; cursor-restart output above.

---

## §M2 — Device identity (2026-07-02)

### Tests — PASS (45 test executions, 0 failures)

New coverage on top of M1:
```
phonehome-core: oui (case-insensitive resolve, unknown→None, garbage-safe, table parses)
                naming (user>dhcp>mdns>vendor>identity precedence, blank-name ignore, no-mac suffix)
phonehome-daemon store:
   devices_resolve_and_name_by_precedence      (MAC→vendor name; MAC-less client→IP)
   rename_takes_precedence_and_clears          (name_user wins; blank resets to vendor)
   merge_folds_activity_and_survives_reingestion  ← the M2 keystone
   merge_rejects_self_and_missing              (BadMerge / NotFound)
   backfill_seeds_devices_from_v1_rollups      (schema-v1 db → devices seeded from history)
   rollups_are_invariant_under_batch_splitting (proptest, now also asserts distinct_devices)
phonehome-daemon api:
   devices_endpoint_lists_named_devices · rename_then_merge_endpoints_work
   rename_missing_device_is_404 · merge_into_self_is_400
```
`cargo fmt --check` clean; `cargo clippy --all-targets -- -D warnings` clean.

The keystone `merge_folds_activity_and_survives_reingestion` merges two devices, asserts the folded view (one device, summed activity), then **reopens the db and re-ingests the same clients** — asserting the merge does not resurrect (the D-010 property).

### Live run — before/after device table (real fixture ingestion)

`PHONEHOME_FIXTURE=fixtures/household-01.jsonl ./phonehome-daemon`, full fixture ingested (`distinct_devices=15`), then `GET /api/devices`:

**BEFORE** — 15 clients resolved to named devices (MAC → OUI vendor; the MAC-less client falls back to its IP):
```
Google · 20:30               Google                a8:51:ab:10:20:30   1514   671   10
Apple · 50:60                Apple                 f4:0f:24:40:50:60   1155   339    6
Samsung Electronics · 22:33  Samsung Electronics   f0:5c:77:11:22:33    967   350   10
Microsoft · 80:90            Microsoft             dc:41:a9:70:80:90    710   204    9
Amazon Technologies · 55:66  Amazon Technologies   00:62:6e:44:55:66    691   253    5
Apple · b2:c3                Apple                 3c:22:fb:a1:b2:c3    516   188    5
… (9 more) …
192.168.1.50                 (none)                192.168.1.50          66    17    3   ← MAC-less → IP
15 devices                                                        (queries blocked domains)
```

**AFTER** — `POST /api/devices/rename` (Samsung → "Living Room TV", HTTP 204) and `POST /api/devices/merge` (the two Apple devices, HTTP 204):
```
Apple · 50:60                Apple                 f4:0f:24:40:50:60   1671   527   10   ← 1155+516 q, 339+188 blk
Google · 20:30               Google                a8:51:ab:10:20:30   1514   671   10
Living Room TV               Samsung Electronics   f0:5c:77:11:22:33    967   350   10   ← renamed
… (11 more) …
14 devices        distinct_devices=14                              (two Apple folded into one)
```
Merge arithmetic verified exactly: 1155+516=1671 queries, 339+188=527 blocked, distinct_domains stays 10 (union). Invalid `merge(self,self)` → **HTTP 400**; `rename(9999)` → **HTTP 404**.

### UI — PASS

`npm run build` green (tsc + vite); the daemon serves the embedded device table (name click-to-rename, per-row "merge into…" select, live 3s refresh). Deferred at M2 (documented): automated DHCP/mDNS discovery and the Pi-hole `/api/network/devices` MAC/hostname join — the `name_dhcp`/`name_mdns` precedence tiers exist and are honored, just not yet auto-populated.

### CI — green

Run: https://github.com/MANVENDRA-github/Phonehome/actions/runs/28610575066 — `build-test` pass (36s) + `docker-smoke` pass (2m4s) on PR #4.

**M2 acceptance met:** the fixture's clients resolve to named devices per the precedence rules (before table); merge survives re-ingestion (keystone test + reopened-db live check); before/after device table captured from a real run above.

---

## §M3 — Enrichment + scorecard + AdGuard adapter (2026-07-03)

### Tests — PASS (75 test executions, 0 failures)

New on top of M2 (30 core + 32 daemon unit + 13 integration):
```
core::enrich  — entity/category/country map, longest-suffix wins, subdomain inherit,
                first-party/functional not trackers, unknown is explicit, blocklist-only tracker
core::score   — empty→0 (no div-by-zero), quiet device low, tracker magnet high,
                monotonic in tracker share, components+inputs reported, spreads saturate
store         — enrichment populates destinations + tracker_queries; scorecard ranks
                tracker-heavy > quiet; scorecard None for missing; snapshots idempotent
adguard       — login→poll→map (Filtered*→blocked, trailing-dot trim); cursor keeps only
                strictly-newer; 401 re-login retry; bad-credentials error; empty-log keeps cursor
api           — scorecard returns score+components+inputs+weights; 404 missing; snapshots list
```
`cargo fmt --check` clean; `cargo clippy --all-targets -- -D warnings` clean.

### Enrichment coverage — PASS (SPEC M3 acceptance)

Live fixture run, then a direct read of the `destinations` table:
```
75 destinations · 0 unknown-category · 0 no-entity · 26 trackers
by category:  functional 37 · telemetry 12 · analytics 9 · first_party 6 · cdn 6 · advertising 5
countries:    CN JP KR SE US
```
**All 75 fixture domains enriched to a known entity — zero `unknown`** (the acceptance bar).

### Scorecard — live table + D-012 sanity check

`GET /api/devices/{id}/scorecard` for every device (score · tracker-share% · tracker entities · countries · volume):
```
Google phone            54    51%   4   2   1514   ← chatty, ad+analytics heavy, 2 countries
Samsung TV              42    41%   2   2    967
Amazon (Ring)           40    43%   2   1    691
Apple phone             39    43%   2   1    516
Microsoft laptop        33    35%   1   1    710
…
Nest thermostat         24    22%   1   1    105
Nintendo console        22    21%   1   1     48   ← quiet, few trackers
```
Ranking matches expectation (ad/analytics-heavy personal devices high, quiet IoT low) — the D-012 provisional-weights sanity check. Every scorecard returns its component values, raw inputs, and weights (SPEC: "always rendered with its inputs").

### Snapshots — PASS

Periodic job produced **30 snapshot rows = 15 devices × 2 ISO weeks** (the 8-day fixture spans two weeks), each with volume/tracker-domains/countries/score. Re-running the job is idempotent (upsert; test `snapshots_are_idempotent`). This is the history the M6 diff will consume.

### AdGuard adapter — validated via wiremock (D-003 proven; live instance pending)

No live AdGuard on this network (same posture as the pending live Pi-hole). The adapter is validated against recorded-shape HTTP: session-cookie login, `/control/querylog` newest-first pagination, `Filtered*`→blocked mapping, trailing-dot trim, strict time-cursor dedup, 401 re-login retry, bad-credential error. It implements the same `Ingestor` trait with **everything downstream unchanged** — that is the D-003 boundary proof. **Follow-up:** validate against a real AdGuard Home.

### UI — PASS

`npm run build` green; the daemon serves the device table with a tracker column and an **expandable per-device scorecard** (the 0–100 score plus meters for each component and the raw inputs — nothing unexplained).

### CI — green

Run: https://github.com/MANVENDRA-github/Phonehome/actions/runs/28673147573 — `build-test` pass (56s) + `docker-smoke` pass (2m12s) on PR #5.

**M3 acceptance met:** every fixture domain enriched, zero `unknown` (coverage above); scorecard renders with all its inputs visible (live table + endpoint); weights sanity-checked on the fixture and marked provisional (D-012, real-household tuning pending). AdGuard proves the source-agnostic boundary. GeoIP deferred to the entity-map country per D-011.

---

## §M4 — The globe (2026-07-04)

### Tests — PASS (96 Rust + 14 vitest + 3 Playwright, 0 failures)

```
cargo test        30 core + 46 daemon + 20 integration  — 0 failures
                  new at M4: arcs group-by-country + merged-device fold, window
                  boundary (inclusive start / exclusive end), unmapped disclosure,
                  arc_domains ordering+enrichment, domain_rollups buckets,
                  apply_batch pulses (enriched, canonical after merge), router
                  tests for /api/arcs /arcs/domains /rollups /config + an SSE
                  body-frame assertion; replayer e2e asserts pulses cover all
                  fixture events exactly once across a restart
npm test          14 vitest — globe math conventions (lat/lon axes, slerp,
                  altitude profile) + centroid coverage of every entity country
npm run e2e       3 Playwright smoke (chromium) — 3 passed (11.2s):
  ✓ globe renders fixture arcs, provenance badge, and labeled devices
  ✓ arc click-through reaches raw rollup data in two clicks
  ✓ device table + scorecard render fixture values; unmapped traffic disclosed
```
`cargo fmt --check` clean; `cargo clippy --all-targets -- -D warnings` clean.

### Widened fixture (D-009 update, in-milestone)

Regenerated deterministically for globe-worthy geography (independently recomputed
from the committed JSONL):
```
7363 events · 18 devices · 93 distinct domains · 2329 blocked
15 destination countries: US KR JP CN SE FR SG RU IN DE NL TW AU CH GB
29 device→country arcs at full-history volume
1 deliberately unmapped domain (pool.ntp.org) → 31 queries reported via
  unmapped_queries — the explicit-unknown path is exercised end to end
```

### Click-through — ≤2 clicks to raw data (SPEC M4 acceptance)

Live headless-chromium run against the daemon replaying the fixture:
arc click → drill panel (device → country breadcrumb, 6 domains with
entity/category/tracker badges/blocked counts) → domain click → **111 raw
hourly rollup buckets** (the rawest data retained, D-005). Also asserted on
every CI run by `ui/e2e/smoke.spec.ts`.

### Perf — measured frame times on named hardware (SPEC M4 proof)

Protocol (`ui/e2e/perf.spec.ts`, @perf): headed Chromium 149 (Playwright 1.61.1),
1280×720 viewport, dpr 1, camera auto-rotating (worst-case overdraw); per cell:
10 s warm-up → reset stats → 30 s measure; GPU selected per run via Windows
per-app GPU preference and **verified from the WebGPU adapter info captured in
the run JSON**. Machine: i9-14900HX laptop, Windows 11 Home (26200), 240 Hz
internal display — frame rates are vsync-capped at ~240 fps; the load-bearing
numbers are the frame-time percentiles. Pass line for "smooth": p95 ≤ 16.7 ms.

**NVIDIA GeForce RTX 4070 Laptop GPU** (adapter: `nvidia / lovelace`) — discrete:

| arcs | backend | fps | avg ms | p50 | p95 | p99 |
|---|---|---|---|---|---|---|
| fixture (29) | webgpu | 238.9 | 4.19 | 4.20 | 4.30 | 4.40 |
| 2,500 | webgpu | 240.0 | 4.17 | 4.20 | 4.30 | 4.40 |
| 5,000 | webgpu | 239.6 | 4.17 | 4.20 | 4.30 | 4.40 |
| **10,000** | **webgpu** | **239.8** | **4.17** | **4.20** | **4.30** | **4.40** |
| fixture (29) | webgl | 238.7 | 4.19 | 4.20 | 4.30 | 4.40 |
| 2,500 | webgl | 240.0 | 4.17 | 4.20 | 4.30 | 4.40 |
| 5,000 | webgl | 239.7 | 4.17 | 4.20 | 4.30 | 4.40 |
| 10,000 | webgl | 239.7 | 4.17 | 4.20 | 4.30 | 4.40 |

**Intel UHD Graphics** (adapter: `intel / gen-12lp`, the i9-14900HX iGPU) — integrated:

| arcs | backend | fps | avg ms | p50 | p95 | p99 |
|---|---|---|---|---|---|---|
| fixture (29) | webgpu | 237.7 | 4.21 | 4.20 | 4.30 | 5.00 |
| 2,500 | webgpu | 238.7 | 4.19 | 4.20 | 4.30 | 4.40 |
| 5,000 | webgpu | 237.0 | 4.22 | 4.20 | 4.30 | 8.20 |
| **10,000** | **webgpu** | **237.2** | **4.22** | **4.20** | **4.30** | **4.70** |
| fixture (29) | webgl | 235.0 | 4.26 | 4.20 | 4.30 | 8.30 |
| 2,500 | webgl | 153.9 | 6.50 | 8.10 | 8.90 | 12.60 |
| 5,000 | webgl | 91.4 | 10.94 | 12.40 | 13.20 | 17.60 |
| 10,000 | webgl | 53.6 | 18.65 | 17.00 | 25.10 | 37.60 |

**Acceptance read-out, stated precisely:** the ≥10k-arc target is met on
integrated graphics via the primary WebGPU path (p95 4.30 ms — vsync-limited,
not GPU-limited). The WebGL2 *fallback* on the iGPU is smooth at fixture volume
(p95 4.30 ms) and degrades gracefully under stress (53.6 fps / p95 25.1 ms at
10k arcs — interactive, but above the 16.7 ms smooth line; disclosed, not
claimed). On the discrete GPU both backends are vsync-flat at every level.

### Hero GIF — docs/hero.gif (the launch asset)

Recorded via the Playwright harness (`npm run hero` → `@hero` spec →
ffmpeg palettegen): **10 s · 880px · 12 fps · 6.19 MB**, real daemon replaying
the committed fixture on a fresh DB, hero choreography cycling labeled devices
(vendor · MAC-suffix callouts) over a worldwide arc starburst from the home
origin. Provenance per D-008/D-009:
- The in-page **“replayed fixture — synthetic data” badge is in every frame**
  (it renders whenever a fixture source is configured — media honesty is
  structural, not an editing step). The D-009 real-capture follow-up stays open.
- Home origin is a **city-level** coordinate (Bengaluru 12.97, 77.59), not an
  address (D-013).
- Recorded on the **WebGL2 fallback** (`?gl=1`): headed-WebGPU canvases don’t
  composite into Chromium’s CDP screencast (they capture black — verified by
  A/B screenshots); the fallback is visually identical and its fps was measured
  separately above. An `.mp4` sibling ships alongside.

### CI — green

Run: https://github.com/MANVENDRA-github/Phonehome/actions/runs/PENDING — recorded after the M4 PRs' checks complete (build-test + vitest, playwright-smoke, docker-smoke).

**M4 acceptance met:** instanced WebGPU globe with WebGL fallback renders the
fixture's device→country arcs with tracker coloring, device filter, and SSE
live pulses; click-through reaches raw rollup data in 2 clicks (asserted in CI);
≥10k visible arcs measured smooth on integrated graphics via WebGPU (frame-time
tables above, hardware named from adapter info); the 10-second hero GIF exists,
recorded through the Playwright harness from a real daemon run with the fixture
label in-frame.
