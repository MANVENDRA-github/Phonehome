# Fixtures

## household-01.jsonl

**Provenance: SYNTHETIC-REALISTIC, not a real capture (D-009).** Generated deterministically by `cargo run -p phonehome-core --example gen_fixture > fixtures/household-01.jsonl` — same bytes every run (seeded LCG, fixed time window).

It models a plausible 18-device household over **14 days** ending 2026-07-02 00:00 UTC (~6k events across 95 domains): smart TV, doorbell, speaker, vacuum, thermostat, phones, laptops, console, printer, smart plugs, Hue bridge, dishwasher, the router itself, and one MAC-less client (exercises the IP-fallback attribution path). Domains are real vendor telemetry/ad/CDN hostnames from public blocklist knowledge spanning 15 destination countries (widened at M4 so the globe shows a worldwide arc spread); tracker domains are blocked ~85% of the time, mimicking a Pi-hole with standard lists. One domain (`pool.ntp.org`) deliberately has no entity mapping — it exercises the explicit-unknown enrichment path and the globe's `unmapped_queries` disclosure.

The window is **two full epoch-aligned weeks** (both the start and end fall on a Thursday-00:00-UTC week boundary), with a **deliberate week-2 behavior change** for the M5 weekly-diff demo: the living-room Samsung TV (`192.168.1.20`) starts contacting two new Samsung ad endpoints (`samsungadhub.com`, `nmp.samsungqbe.com`) only in the second week, so the diff view shows a real "+2 new tracker domains this week" delta. Because the calendar week the diff labels "this week" is the fixture's newest week (2026-06-25 → 07-02), any diff media is **replayer time-warp** and must say so (SPEC M5, D-009).

Any demo media rendered from this file must carry a visible "replayed fixture" label (CLAUDE.md honesty rule). It should be replaced/supplemented with an **anonymized real capture** once a live Pi-hole network is available — tracked in DECISIONS.md D-009.

One `QueryEvent` JSON object per line (schema: `core/src/event.rs`).
