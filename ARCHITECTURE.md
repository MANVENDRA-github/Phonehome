# Phonehome — Architecture

Design-phase document (pre-M0). This is the intended v1 shape; deviations get logged in [DECISIONS.md](DECISIONS.md) and reflected back here in the same change (docs-sync rule, see [CLAUDE.md](CLAUDE.md)).

## 1. System overview

One Rust service (daemon + API + embedded static UI) plus a SQLite file. The browser does the heavy visual lifting on the GPU.

```
                       ┌──────────────────────────── phonehome (single Rust binary) ───────────────────────────┐
                       │                                                                                        │
 Pi-hole v6 API ──┐    │  ┌──────────┐   normalized    ┌──────────┐   ┌────────────┐   ┌──────────────────┐    │
                  ├──────▶│ pollers  │──QueryEvent────▶│ pipeline │──▶│  SQLite    │◀──│ scorecard + diff │    │
 AdGuard Home API ┘    │  │ (per-    │                 │ device   │   │ (rusqlite) │   │ engine (weekly   │    │
 (adapters behind      │  │  source) │                 │ identity │   └─────┬──────┘   │ snapshots)       │    │
  one Ingestor trait;  │  └──────────┘                 │ + enrich │         │          └──────────────────┘    │
  fixture replayer     │        ▲                      └──────────┘         │                                  │
  is a third impl)     │        │ static datasets: OUI table · GeoLite2 ·   │        ┌───────────────────┐     │
                       │        │ tracker blocklists · entity map          └───────▶│ Axum HTTP API     │     │
                       │        └──────────────────────────────────────────────────▶│ /api/* + SSE      │     │
                       │                                                            │ /stream + embedded│     │
                       │                                                            │ UI (rust-embed)   │     │
                       └────────────────────────────────────────────────────────────┴───────────┬───────┘     │
                                                                                                 │
                                                                              browser ◀──────────┘
                                                                              React + Three.js/WebGPU globe,
                                                                              scorecards, device registry UI
```

## 2. Components

### 2.1 Ingestion (`Ingestor` trait)
- **Contract (as implemented at M1):** `poll(cursor: Option<&str>) -> Batch { events: Vec<QueryEvent>, next_cursor }` where `QueryEvent = { ts, client_ip, client_mac?, domain, qtype, blocked, source }`. Exactly-once is a two-party contract: the adapter guarantees a re-poll from any returned cursor never re-yields delivered events (Pi-hole: monotonic FTL query id filters the inclusive time-boundary overlap), and the store commits events + cursor **in one transaction** (`Store::apply_batch`), so a crash replays a whole batch or none.
- **Pi-hole v6 adapter:** session auth against the v6 REST API, query history endpoint. Version-pinned; failures degrade to a visible "source stale since …" badge, never a crash loop.
- **AdGuard Home adapter:** `/control/querylog` with cursor pagination. Proves the trait boundary — nothing outside the adapters may know which backend is in use.
- **Fixture replayer:** third `Ingestor` impl reading a committed JSONL fixture (anonymized real capture) at configurable speed. This is how all dev/test/demo happens without a live network, and how CI stays deterministic.

### 2.2 Device identity
- Registry keyed by MAC when available, else stable client IP (flagged lower-confidence).
- Naming resolution order (implemented at M2, `core::naming`): user-assigned > DHCP hostname > mDNS name > OUI vendor + short MAC suffix (`"Samsung Electronics · 22:33"`) > raw identity.
- OUI lookup from a bundled curated table (`core/data/oui.csv`, `core::oui`) — same CSV shape as the IEEE registry so the full table drops in. `name_dhcp`/`name_mdns` columns exist and are honored by precedence; **automated DHCP/mDNS discovery is deferred** (documented follow-up), as is joining Pi-hole `/api/network/devices` for MAC+hostname.
- Devices are an **overlay** (D-010): each raw device's `identity_key` = the rollup `client_key`. Rename sets `name_user`; merge sets `merged_into` (an alias pointing at the canonical device). Ingestion only upserts identity/last-seen — never `merged_into` or names — so re-discovery never resurrects a merged device. Device-level activity is folded at read time via `COALESCE(merged_into, id)`.

### 2.3 Enrichment (implemented at M3, `core::enrich`)
Pure and **offline** — no network, so it runs inside the ingestion transaction (D-005 intact). Each distinct domain is enriched once and cached in `destinations`:
- **Entity + category:** curated `core/data/entities.toml` maps domain suffixes → owning company + category (`first_party` · `functional` · `cdn` · `telemetry` · `analytics` · `advertising`). Longest-suffix match wins, so specific hostnames override base-domain fallbacks. Deliberately editable data, PR-friendly.
- **Tracker classification:** a destination `is_tracker` when its category is tracking (advertising/analytics/telemetry) **or** it appears in the bundled `core/data/trackers.txt` blocklist seed (oisd/StevenBlack shape — drop in the full list to widen). Unmapped domains resolve explicitly to `Category::Unknown` (SPEC M3 acceptance).
- **Country:** comes from the entity map's ISO-2 field, **not** GeoIP (D-011 — no answer-IP in the log, MaxMind licensing, CDN-IP inaccuracy). Optional user-provided GeoLite2 for unmapped domains is a documented follow-up.

### 2.4 Scorecard + diff engine (scorecard + snapshots at M3, `core::score`)
- **Live scorecard** per device (`GET /api/devices/{id}/scorecard`): a 0–100 privacy-risk score (higher = more concerning), blended from four normalized components — tracker share (0.45), distinct tracker companies (0.25), country spread (0.15), chattiness/volume (0.15). Weights are PROVISIONAL (D-012) and live in one `ScoreWeights` struct. The `Scorecard` **always carries its component values, raw inputs, and the weights** — no unexplained number (SPEC M3). `blocked` is shown as context but excluded from the score.
- **Weekly snapshots** (`snapshots` table, refreshed by a periodic idempotent job; `GET /api/snapshots`): per canonical device per ISO week — distinct/tracker domains, distinct entities/countries, volume, blocked, score. These persist the history the M6 week-over-week **diff** consumes ("+6 tracker domains: …"), the retention feature.

### 2.5 API + UI
- **Axum** serving (as implemented through M4): `/api/health`, `/api/config` (home lat/lon from `PHONEHOME_HOME_LAT`/`_LON` — the globe's arc origin, config-only data), `/api/stats`, `/api/devices` (+ `/rename`, `/merge`), `/api/devices/:id/scorecard`, `/api/snapshots`, `/api/arcs?window=<hours>` (device→country arc aggregates; destinations without a mapped country are excluded but disclosed as `unmapped_queries`), `/api/arcs/domains?device=&country=` (click-through level 1), `/api/rollups?device=&domain=` (level 2 — the raw hourly buckets, the rawest data retained per D-005), and `/api/stream` (SSE). The `/api/diffs` endpoint arrives with the M6 diff UI. Endpoints are documented here and in CLAUDE.md rather than via OpenAPI (an OpenAPI spec is a possible follow-up, not an M1 artifact as originally planned).
- **SSE live updates:** `Store::apply_batch` returns one `Pulse` per (device, domain) committed — derived inside the ingest transaction — which the ingest loop fans out through a `tokio::sync::broadcast` channel (buffer 256) to `/api/stream`. Pulses are **hints, not state**: a lagging subscriber silently drops them, and the globe reconciles by refetching `/api/arcs`.
- UI is a **Vite + React + TypeScript** app compiled into the binary via `rust-embed` — the docker image and the binary are the same artifact philosophy: one thing to run.
- **Globe:** Three.js `WebGPURenderer` with WebGL fallback; instanced arcs (device→destination country centroid), color by category (tracker = red family), device filter rail, click-through arc → domain list → raw queries. Target: smooth at ≥10k visible arcs on integrated graphics (M4 perf spike gate).

## 3. Storage sketch (SQLite, WAL mode)

```sql
-- implemented at M1–M3 (schema_version 3):
sources(id PRIMARY KEY, kind, cursor, last_ok_at)          -- base_url lives in env config until the M5 wizard
query_rollups(source_id, client_key, domain, bucket_hour,  -- client_key = MAC else IP; stays the key (D-010)
              count, blocked_count)                        -- raw events roll up; no per-query retention (D-005)
devices(id, identity_key UNIQUE, is_mac, mac, ip_hint,     -- overlay: identity_key = client_key (D-010)
        oui_vendor, name_user, name_dhcp, name_mdns,       -- names by precedence (core::naming)
        first_seen, last_seen, merged_into)                -- merged_into = alias -> canonical device
destinations(domain PRIMARY KEY, entity, category,         -- enriched once per domain (core::enrich)
             country, is_tracker, on_blocklist, enriched_at) -- country from entity map, not GeoIP (D-011)
snapshots(device_id, week_start, distinct_domains,         -- weekly per device; feeds the M6 diff
          tracker_domains, distinct_entities, distinct_countries,
          volume, blocked, score, computed_at, PRIMARY KEY (device_id, week_start))
```

Raw `QueryEvent`s are transient: they update rollups + trigger enrichment, then drop. Privacy stance doubles as a scaling strategy — a year of a busy household stays comfortably in one SQLite file.

## 4. Constraints (load-bearing, from DECISIONS.md)

1. **DNS-log ingestion only** (D-001). No pcap, no ARP spoofing, no inline placement. This is the product's trust posture *and* what keeps v1 solo-sized. Named limitation, not hidden: DoH/DoT-bypassing devices are surfaced as "partially visible" in the UI.
2. **Everything local, zero telemetry** (D-005). The only outbound calls are user-initiated dataset refreshes; a `strict_local = true` config disables even those (bundled snapshots only).
3. **One artifact** (D-006). `docker compose up` → one container, one volume. No external DB, no message broker, no worker fleet.
4. **Source-agnostic core** (D-003). Everything downstream of the `Ingestor` trait is backend-blind; adding unbound in v2 must touch adapters only.

## 5. Testing strategy
- Adapter tests against recorded HTTP fixtures (wiremock); the JSONL replayer powers integration tests end-to-end (ingest → enrich → scorecard → API assertions).
- Property tests on dedup/cursor logic (no event loss or double-count across restarts).
- Playwright smoke: compose up with replayer → globe renders arcs → scorecard shows expected fixture values. This is also the demo-GIF harness.
- Perf gate (M4): arc-count benchmark on integrated GPU recorded in PROOF.md with real numbers.
