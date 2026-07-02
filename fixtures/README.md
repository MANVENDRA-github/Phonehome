# Fixtures

## household-01.jsonl

**Provenance: SYNTHETIC-REALISTIC, not a real capture (D-009).** Generated deterministically by `cargo run -p phonehome-core --example gen_fixture > fixtures/household-01.jsonl` — same bytes every run (seeded LCG, fixed time window).

It models a plausible 15-device household over 8 days ending 2026-07-02 00:00 UTC (~7.2k events): smart TV, doorbell, speaker, vacuum, thermostat, phones, laptops, console, printer, smart plugs, and one MAC-less client (exercises the IP-fallback attribution path). Domains are real vendor telemetry/ad/CDN hostnames from public blocklist knowledge; tracker domains are blocked ~85% of the time, mimicking a Pi-hole with standard lists.

Any demo media rendered from this file must carry a visible "replayed fixture" label (CLAUDE.md honesty rule). It should be replaced/supplemented with an **anonymized real capture** once a live Pi-hole network is available — tracked in DECISIONS.md D-009.

One `QueryEvent` JSON object per line (schema: `core/src/event.rs`).
