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
- **Contract:** `poll(since: Cursor) -> Vec<QueryEvent>` where `QueryEvent = { ts, client_ip, client_mac?, domain, qtype, blocked, source }`. Cursor persisted per source; polling is incremental and idempotent (dedup key: source + native query id, else `hash(ts, client, domain)`).
- **Pi-hole v6 adapter:** session auth against the v6 REST API, query history endpoint. Version-pinned; failures degrade to a visible "source stale since …" badge, never a crash loop.
- **AdGuard Home adapter:** `/control/querylog` with cursor pagination. Proves the trait boundary — nothing outside the adapters may know which backend is in use.
- **Fixture replayer:** third `Ingestor` impl reading a committed JSONL fixture (anonymized real capture) at configurable speed. This is how all dev/test/demo happens without a live network, and how CI stays deterministic.

### 2.2 Device identity
- Registry keyed by MAC when available, else stable client IP (flagged lower-confidence).
- Naming resolution order: user-assigned > DHCP hostname > mDNS name > OUI vendor + short id (`"Espressif a4:cf"`).
- OUI lookup from the bundled IEEE table; mDNS via passive mDNS crate listener (optional feature flag — it's the only component that touches the LAN beyond the source APIs).
- Users can rename and merge devices in the UI; merges are recorded as aliases so re-discovery doesn't resurrect duplicates.

### 2.3 Enrichment
Runs per unique domain, cached in `destinations`:
- **Tracker classification:** membership in oisd / StevenBlack lists (bundled snapshots; user-refreshable). Store which list(s) matched.
- **GeoIP:** MaxMind GeoLite2-Country (user supplies license key for updates; bundled snapshot fallback) resolved against the domain's A/AAAA answer when the source provides it, else a lightweight lookup of the domain's current IP (local resolver, cacheable, off by default in strict-local mode).
- **Entity mapping:** curated `entities.toml` in-repo mapping domain suffixes → owning company + category (ads/analytics/telemetry/CDN/first-party). Deliberately editable data, PR-friendly.

### 2.4 Scorecard + diff engine
- Weekly snapshot per device: distinct domains, tracker share, entity set, country set, query volume.
- Score = weighted blend (weights are config, defaults set after the M3 real-household spike) — **always rendered with its inputs**; no unexplained numbers.
- Diff view = set differences between snapshots ("+6 tracker domains: …"), the retention feature.

### 2.5 API + UI
- **Axum** serving `/api/devices`, `/api/devices/:id/scorecard`, `/api/arcs?window=`, `/api/diffs`, and `/stream` (SSE: new enriched events for live globe updates). OpenAPI-documented from M1.
- UI is a **Vite + React + TypeScript** app compiled into the binary via `rust-embed` — the docker image and the binary are the same artifact philosophy: one thing to run.
- **Globe:** Three.js `WebGPURenderer` with WebGL fallback; instanced arcs (device→destination country centroid), color by category (tracker = red family), device filter rail, click-through arc → domain list → raw queries. Target: smooth at ≥10k visible arcs on integrated graphics (M4 perf spike gate).

## 3. Storage sketch (SQLite, WAL mode)

```sql
sources(id, kind, base_url, cursor, last_ok_at)
devices(id, mac, ip_hint, name_user, name_dhcp, name_mdns, oui_vendor, first_seen, merged_into)
destinations(domain PRIMARY KEY, entity, category, tracker_lists, country, resolved_ip, enriched_at)
query_rollups(device_id, domain, bucket_hour, count, blocked_count)   -- raw events roll up; no per-query retention by default
snapshots(device_id, week_start, distinct_domains, tracker_domains, entities_json, countries_json, volume, score)
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
