# Phonehome — Agent Operating Context

Read this first in every session. It exists so any AI coding agent (Claude Code, Cursor, or other) can pick up this project cold and work correctly.

## What this project is

**Phonehome** — a self-hosted privacy radar for home networks. It ingests DNS query logs from Pi-hole v6 / AdGuard Home (never packet capture), attributes queries to named LAN devices, tags destinations against tracker blocklists + GeoIP + a company-entity map, and renders per-device privacy scorecards with weekly diffs plus a WebGPU globe of device→destination arcs. 100% local, zero telemetry, ships as one `docker compose up`.

Current phase: **pre-code — docs foundation merged, next work item is SPEC.md M0 (scaffold).**

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

## Stack (intended — confirm against code once M0 lands)

Rust workspace: `daemon/` (bin) + `core/` (lib) — Axum, rusqlite (WAL), rust-embed. UI: `ui/` — Vite + React + TypeScript, Three.js `WebGPURenderer` (WebGL fallback), Tailwind. Data: bundled IEEE OUI table, oisd + StevenBlack snapshots, GeoLite2-Country, `entities.toml`. Deploy: Dockerfile + docker-compose. Commands will be listed here once they exist (M0 updates this section).

## Owner context

Owner: Manvendra (GitHub: MANVENDRA-github) — professional software engineer; this is a deliberate Rust-depth project and a distribution-optimized OSS play (see PRD §5 metrics). Provenance and the wider idea-selection research live in his knowledge vault (`claude-memory` repo): `decisions/fresh-standalone-project-ideas.md`.
