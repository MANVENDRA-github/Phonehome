# Phonehome — v1 Build Spec

Milestone plan for v1. Each milestone has a **deliverable**, **acceptance criteria**, and a **proof requirement** — evidence comes from real command output / real data captured in `PROOF.md` (created at M0), never prose claims. A milestone isn't done until its proof section exists.

Ground rules for every milestone:
- Work happens on a feature branch → PR → merge; `main` stays green and is never pushed to directly.
- Docs (README/ARCHITECTURE/DECISIONS/this file) are updated **in the same PR** as the code that changes them.
- Any scope change or reversal gets a `D-xxx` entry in [DECISIONS.md](DECISIONS.md).

---

## M0 — Scaffold ✅ (done 2026-07-02 — evidence: PROOF.md §M0)
**Deliverable:** Rust workspace (`daemon/` binary crate + `core/` lib crate) and `ui/` (Vite + React + TS), CI (fmt + clippy + test + UI typecheck/build on push), `PROOF.md`, `.gitignore`, compose file that builds and starts the (empty) service.
**Accept:** `cargo test` and `npm run build` green locally and in CI; `docker compose up` serves a "phonehome alive" page with the embedded UI.
**Proof:** CI run link + pasted local command output in PROOF.md §M0.

## M1 — Ingestion: Pi-hole v6 + fixture replayer ✅ (done 2026-07-02 — evidence: PROOF.md §M1; fixture is synthetic-labeled per D-009; live Pi-hole validation pending per the fallback below)
**Deliverable:** `Ingestor` trait; Pi-hole v6 adapter (auth, incremental query-history polling, persisted cursor); JSONL fixture replayer as second impl; SQLite storage (`sources`, `query_rollups` minimal form); anonymized real fixture committed.
**Accept:** replayer run ingests the full fixture with zero loss/dup across a mid-run restart (property test); live Pi-hole poll shown working against a real instance (or, if no Pi-hole is reachable, a recorded-HTTP-fixture test plus an explicit PROOF note that live validation is pending).
**Proof:** test output + row counts vs fixture line count; cursor-restart test output. PROOF.md §M1.

## M2 — Device identity ✅ (done 2026-07-02 — evidence: PROOF.md §M2)
**Deliverable:** device registry (MAC/IP keying, OUI vendor lookup from bundled IEEE table, DHCP/mDNS name intake, naming precedence), rename + merge API, minimal device-list UI.
**Accept:** fixture's clients resolve to named devices per the precedence rules; merge survives re-ingestion (no duplicate resurrection — regression test).
**Proof:** before/after device table from a real run. PROOF.md §M2.

## M3 — Enrichment + scorecard ✅ (done 2026-07-03 — evidence: PROOF.md §M3; weights provisional per D-012, AdGuard validated via wiremock pending a live instance)
**Deliverable:** destination enrichment (oisd + StevenBlack membership, country, `entities.toml` mapping), weekly snapshot job, explainable scorecard + score config; AdGuard Home adapter (second live backend, proves D-003's boundary). *Shipped with country sourced from the entity map rather than the originally-specified GeoLite2 — see D-011.*
**Accept:** every fixture domain enriched (or explicitly `unknown`); scorecard renders with its inputs visible; **spike:** scorecards computed on ≥1 real household's data and sanity-reviewed — weights adjusted and logged as a D-xxx.
**Proof:** enrichment coverage stats + a real (redacted) scorecard screenshot. PROOF.md §M3.

## M4 — The globe ✅ (done 2026-07-04 — evidence: PROOF.md §M4; ≥10k arcs smooth on integrated via the WebGPU path with the WebGL-fallback stress numbers disclosed; hero GIF carries the D-009 "replayed fixture" label pending a real capture)
**Deliverable:** WebGPU globe (WebGL fallback): instanced device→country arcs, tracker coloring, device filter, arc click-through → domains → rollups; SSE live updates (S-1).
**Accept:** smooth on integrated graphics at the fixture's arc volume — measured, threshold set by the perf spike (target ≥10k visible arcs); click-through reaches raw rollup data in ≤2 clicks.
**Proof:** FPS/frame-time numbers on named hardware (discrete + integrated) + **the 10-second real-data hero GIF** recorded via the Playwright harness. PROOF.md §M4. *This GIF is the launch asset — M4 is the go/no-go gate for the distribution plan (RESEARCH.md §5).*

## M5 — Ship ✅ (done 2026-07-04 — evidence: PROOF.md §M5; merged to `main` via #10→#14 and released as **v0.1.0**. Setup wizard, weekly diff, hardened one-container compose, real README/install docs. Clean-install proof is the CI docker-smoke job — local Docker unavailable, disclosed per the M5 decision. v1 milestones M0–M5 all complete.)
**Deliverable:** first-run setup wizard (paste source URL + token → data in ≤60 s); weekly-diff UI (M-6); single `docker compose up` path hardened (one container, one volume, sane defaults); README quickstart flipped from "planned" to real; install docs; v0.1.0 tag + GitHub release.
**Accept:** a clean machine (or clean VM) goes from `git clone` → populated globe in ≤2 commands and ≤5 min, timed; diff view shows a real week-over-week delta (from replayer time-warp if the calendar hasn't cooperated, labeled as such).
**Proof:** timed clean-install transcript + diff screenshot. PROOF.md §M5.

---

## Sequencing notes
- M1 before M2: attribution needs events flowing first; the replayer unblocks everything else.
- AdGuard adapter is deliberately in M3, not M1 — one live backend + the replayer is enough to harden the trait; the second backend then validates the boundary cheaply.
- Distribution (RESEARCH.md §5) starts only after M4's GIF exists; soft-launch (Pi-hole community) can begin during M5.

## Out of scope for v1
See PRD §4 "Won't have" — pcap/inline modes, other DNS-server adapters, alerting, Home Assistant, multi-user auth, retention UI. Anything creeping in requires a PRD change + D-xxx first.
