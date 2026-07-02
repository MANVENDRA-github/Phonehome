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
