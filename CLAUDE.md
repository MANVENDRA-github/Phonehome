# Phonehome — Agent Operating Context

Read this first in every session. It exists so any AI coding agent (Claude Code, Cursor, or other) can pick up this project cold and work correctly.

## What this project is

**Phonehome** — a self-hosted privacy radar for home networks. It ingests DNS query logs from Pi-hole v6 / AdGuard Home (never packet capture), attributes queries to named LAN devices, tags destinations against tracker blocklists + GeoIP + a company-entity map, and renders per-device privacy scorecards with weekly diffs plus a WebGPU globe of device→destination arcs. 100% local, zero telemetry, ships as one `docker compose up`.

Current phase: **M4 (the globe) in progress — backend (PR: arcs/drill-down/config/SSE) and globe UI (PR: three.js WebGPU+TSL globe, fixture widened to 18 devices / 15 countries) built; remaining: Playwright smoke + perf harness, measured FPS on iGPU/dGPU, hero GIF, PROOF §M4.** Open follow-ups: live Pi-hole/AdGuard validation + a real anonymized fixture (D-009, before the M4 GIF ideally); real-household scorecard weight tuning (D-012); optional user-provided GeoLite2 for unmapped domains (D-011); automated DHCP/mDNS discovery (deferred at M2); local Docker blocked on machine virtualization (PROOF §M0).

## Read order for context

1. This file — conventions and state
2. [SPEC.md](SPEC.md) — what to build next (milestones M0–M5, acceptance criteria, proof rules)
3. [PRD.md](PRD.md) — product scope: MoSCoW list, success metrics, risks; scope changes require a PRD edit + DECISIONS entry
4. [ARCHITECTURE.md](ARCHITECTURE.md) — intended system shape: `Ingestor` trait, SQLite schema, enrichment stack, globe
5. [DECISIONS.md](DECISIONS.md) — D-001…D-008 are load-bearing constraints; check before proposing anything that touches ingestion mode, privacy stance, deployment shape, or licensing
6. [RESEARCH.md](RESEARCH.md) — competitive wedge + what would invalidate it; consult before positioning/launch work

## Hard constraints (do not violate without a new D-xxx)

- **DNS-log ingestion only** (D-001): never add packet capture, ARP spoofing, or inline traffic interception.
- **Everything local** (D-005): no telemetry, no cloud calls except user-initiated dataset refreshes; raw query events are not retained (rollups only).
- **One artifact** (D-006): single container + volume; no external databases, brokers, or worker services.
- **Backend-blind core** (D-003): only ingestion adapters may know whether the source is Pi-hole or AdGuard.

## Working conventions

- **Production-quality code by default**: typed, tested, error paths handled; match existing style once code exists.
- **Plan before building**: for any milestone or non-trivial change, present a plan first.
- **Git**: never push to `main`. Branch → focused PR → merge. Small PRs over big ones. Conventional-ish commit subjects (`feat:`, `fix:`, `docs:`, `test:`, `chore:`).
- **Docs-sync rule**: a PR that changes behavior/architecture/scope updates the affected doc(s) *in the same PR*. A doc that no longer matches the code is a bug.
- **PROOF discipline** (D-008): `PROOF.md` (created at M0) records real command output, real measurements, real screenshots per milestone. Never write a performance number, coverage claim, or demo asset from anything but an actual run. If something wasn't verified, say so explicitly.
- **Honest demos**: published GIFs/screenshots use real data from a real network, or carry a visible "replayed fixture" label.
- **Fixtures over live dependencies**: development and CI run on the JSONL replayer (ARCHITECTURE §2.1); anonymize any committed capture (hash MACs, keep OUI prefix; drop non-load-bearing domains).
- **Testing bar**: adapter fixtures (wiremock), property tests on cursor/dedup logic, an end-to-end replayer integration test, and the Playwright smoke that doubles as the GIF harness (ARCHITECTURE §5).

## Stack & commands (confirmed at M0)

Rust workspace: `daemon/` (bin — Axum 0.8, tokio, rust-embed) + `core/` (lib — the normalized `QueryEvent` model). UI: `ui/` — Vite 6 + React 19 + TypeScript + Tailwind v4 + three.js 0.185 pinned exact (WebGPU + TSL, WebGL2 fallback — see D-013; `npm test` = vitest for globe math/centroids). Deploy: multi-stage Dockerfile + docker-compose (one service, one volume). CI: `.github/workflows/ci.yml` — `build-test` (UI build, fmt, clippy `-D warnings`, tests) + `docker-smoke` (compose up + live health/page probes).

**Build order matters:** the daemon embeds `ui/dist` at compile time — always build the UI before the daemon.

```sh
npm --prefix ui install        # once
npm --prefix ui run build      # typecheck (tsc --noEmit) + vite bundle -> ui/dist
cargo test                     # workspace tests (needs ui/dist to exist)
cargo run -p phonehome-daemon  # serve on http://localhost:8480 (PHONEHOME_PORT to override)
cargo fmt --check && cargo clippy --all-targets -- -D warnings   # lint gate, same as CI
docker compose up -d --build   # full container build + run (proven in CI; local Docker
                               #   currently blocked on this dev machine — see PROOF.md §M0)
npm --prefix ui run dev        # UI dev server with /api proxy to a running daemon

# Ingestion (M1) — sources configured via env until the M5 wizard:
PHONEHOME_FIXTURE=fixtures/household-01.jsonl cargo run -p phonehome-daemon   # replay the dev fixture
# PHONEHOME_PIHOLE_URL=http://pi.hole PHONEHOME_PIHOLE_PASSWORD=... [PHONEHOME_POLL_INTERVAL_SECS=15]
# PHONEHOME_DB=data/phonehome.db (default; container sets /data/phonehome.db)
curl localhost:8480/api/stats  # ingestion totals + per-source cursor state
curl localhost:8480/api/devices                                                   # M2: named device list (+ tracker_queries)
curl -XPOST localhost:8480/api/devices/rename -d '{"id":1,"name":"Living Room TV"}' -H content-type:application/json
curl -XPOST localhost:8480/api/devices/merge  -d '{"source":2,"into":1}' -H content-type:application/json
curl localhost:8480/api/devices/1/scorecard   # M3: privacy score + its component inputs
curl localhost:8480/api/snapshots             # M3: weekly per-device snapshot history
curl "localhost:8480/api/arcs?window=24"      # M4: device→country arcs (+ unmapped_queries); window in hours, omit for all data
curl "localhost:8480/api/arcs/domains?device=1&country=US"   # M4: domains behind one arc
curl "localhost:8480/api/rollups?device=1&domain=api.ring.com"  # M4: raw hourly buckets
curl localhost:8480/api/config                # M4: home lat/lon + version
curl -N localhost:8480/api/stream             # M4: SSE pulses while ingestion runs
# PHONEHOME_HOME_LAT=12.97 PHONEHOME_HOME_LON=77.59  (globe arc origin; unset -> UI hint)
# globe URL params: ?gl=1 force WebGL · ?hud=1 frame stats · ?stress=N synthetic-arc benchmark · ?hero=1 GIF choreography
npm --prefix ui test                          # vitest: globe math + centroid coverage
# AdGuard source (env, alongside/instead of Pi-hole; each is its own source):
# PHONEHOME_ADGUARD_URL=http://adguard PHONEHOME_ADGUARD_USERNAME=admin PHONEHOME_ADGUARD_PASSWORD=...
# regenerate the fixture (deterministic, D-009):
cargo run -p phonehome-core --example gen_fixture > fixtures/household-01.jsonl
```

Enrichment data (curated seeds, drop-in-replaceable with the real datasets): `core/data/entities.toml` (domain→entity/category/country) · `core/data/trackers.txt` (blocklist).

## Owner context

Owner: Manvendra (GitHub: MANVENDRA-github) — professional software engineer; this is a deliberate Rust-depth project and a distribution-optimized OSS play (see PRD §5 metrics). Provenance and the wider idea-selection research live in his knowledge vault (`claude-memory` repo): `decisions/fresh-standalone-project-ideas.md`.
