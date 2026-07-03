# Phonehome — Decision Log

One entry per decision that shapes the product or architecture. Newest at the bottom. Reversals get a new entry referencing the old one — history is never edited.

Format: `D-xxx · date · decision — rationale (alternatives considered)`

---

**D-001 · 2026-07-02 · DNS-log ingestion only — no packet capture, no ARP spoofing, no inline gateway mode.**
Rationale: this is the verified-open competitive wedge (everything destination-aware today intercepts traffic or sells hardware — see RESEARCH.md §2), it means zero changes to the user's network trust model, and it keeps v1 buildable by one person. Known cost: DoH/DoT-bypassing devices are partially invisible — disclosed in UI/README rather than papered over. (Alternatives: pcap sidecar — rejected for scope + trust; router firmware — rejected, that's SPR's lane.)

**D-002 · 2026-07-02 · Rust backend: single binary, Axum + rusqlite, UI embedded via rust-embed.**
Rationale: single-artifact distribution is the verified easy-install pattern; a long-running low-footprint daemon on home hardware (often a Pi) suits Rust; and this is a deliberate Rust-depth project for the owner. (Alternatives: Node/TS backend — faster for the owner but weaker deploy story; Go — fine, but no learning payoff.)

**D-003 · 2026-07-02 · Pi-hole v6 and AdGuard Home are the first-class v1 sources, behind one `Ingestor` trait; unbound/dnsmasq deferred to v2.**
Rationale: the two cover the overwhelming share of self-hosted DNS filtering; the trait keeps the core backend-blind so v2 adapters are additive. Fixture replayer is a mandatory third implementation (dev/CI/demo without a live network).

**D-004 · 2026-07-02 · Enrichment stack: oisd + StevenBlack blocklists, MaxMind GeoLite2-Country, curated in-repo `entities.toml` for domain→company mapping.**
Rationale: all freely licensable, bundleable as snapshots, and community-editable — entity mapping is the weakest public dataset, so making it a PR-able data file turns the weakness into a contribution surface. GeoLite2 needs a free MaxMind key for updates; documented, with bundled fallback.

**D-005 · 2026-07-02 · Privacy stance: 100% local, zero telemetry, no cloud path, raw queries not retained.**
Rationale: the product's entire moral position — it cannot itself be a phone-home. Only outbound traffic is user-initiated dataset refresh; `strict_local` config disables even that. Raw events roll up to hourly counts and drop (privacy doubling as scaling strategy). Non-negotiable; any exception requires a new D-xxx and a very good reason.

**D-006 · 2026-07-02 · Ships as one `docker compose up` — one container, one volume; no external DB/broker/workers.**
Rationale: install friction ≤2 commands is a hard PRD requirement; multi-service self-hosted stacks are a documented adoption killer even for popular tools. SQLite (WAL) is sufficient for household scale by design.

**D-007 · 2026-07-02 · MIT license.**
Rationale: maximum-adoption default for a distribution-optimized OSS project; matches the owner's other public work. (Alternative AGPL — reconsider only if cloud-resale becomes a real concern; would be a new D-xxx.)

**D-008 · 2026-07-02 · Docs-first foundation; PROOF.md discipline from M0.**
Rationale: the repo starts with PRD/RESEARCH/ARCHITECTURE/SPEC so any agent or future session has full context; all published claims/numbers/GIFs must come from real runs recorded in PROOF.md (practice carried over from the owner's prior projects, where it worked).

**D-009 · 2026-07-02 · The committed dev fixture is SYNTHETIC-REALISTIC, not a real capture — deviation from SPEC M1's "anonymized real fixture", disclosed.**
Rationale: no live Pi-hole network was available at M1. `fixtures/household-01.jsonl` is generated deterministically (`core/examples/gen_fixture.rs`, seeded — same bytes every run): 15 plausible devices over 8 days using real vendor telemetry/ad-network hostnames. Labeled in `fixtures/README.md`; any demo media from it must carry a "replayed fixture" label. **Follow-up:** capture and anonymize a real household fixture (hash MACs, drop non-load-bearing domains) once a Pi-hole is running — ideally before the M4 hero GIF, mandatory before launch claims.

**D-010 · 2026-07-02 · Devices are a semantic OVERLAY on `client_key`-keyed rollups — `query_rollups` is NOT re-keyed to `device_id` (revises the ARCHITECTURE §3 sketch).**
Rationale: keeping rollups keyed on `client_key` (MAC else IP) and mapping identity → device via `devices.identity_key` makes rename/merge O(1), non-destructive edits to the overlay, and makes "merge survives re-ingestion" automatic — ingestion only upserts identity/last-seen, never `merged_into` or names, so a re-seen client can never resurrect a merged device or clobber a rename. Device-level activity is computed at read time by folding each raw device's rollups into its canonical device (`COALESCE(merged_into, id)`). Alternative (device_id in rollups, per the original sketch) would force a rollup rewrite on every merge and a migration on every keying change — more code, more failure modes, no benefit at household scale. ARCHITECTURE §2.1/§3 updated to match.

**D-011 · 2026-07-03 · Destination country comes from the entity map, not bundled GeoIP.**
Rationale: three reasons. (1) A `QueryEvent` carries no resolved answer-IP, so IP-based GeoIP would need a schema+adapter change or a live per-domain DNS resolution (network calls — flaky, and breaks strict-local by default). (2) MaxMind GeoLite2's `.mmdb` cannot be redistributed in a public repo (licensing), so it can't be bundled. (3) GeoIP of a CDN-fronted IP (Cloudflare/Akamai) is misleading anyway — the hosting IP's country ≠ where the *entity* takes your data. So each entry in `core/data/entities.toml` carries an ISO-3166 alpha-2 country (the entity's HQ/hosting jurisdiction), feeding the scorecard's country-spread input and the M4 globe. **Follow-up:** optionally support a user-provided GeoLite2 `.mmdb` (env-configured) to resolve *unmapped* domains — additive, not a replacement.

**D-012 · 2026-07-03 · Scorecard weights are PROVISIONAL, pending a real-household tuning pass.**
Rationale: SPEC M3 calls for tuning weights against ≥1 real household, but no real capture exists yet (same constraint as D-009). The defaults (`ScoreWeights` in `core/src/score.rs`: tracker-share 0.45, entity-spread 0.25, country-spread 0.15, chattiness 0.15) were chosen for face validity and sanity-checked against the synthetic fixture's ranking — chatty ad/analytics-heavy devices (phone, TV) score high; quiet functional devices (console, thermostat) score low (see PROOF §M3). Weights live in one struct so a real-data pass is a one-line change. `blocked` is reported as context but deliberately excluded from the risk score (a blocked query means the filter protected you — ambiguous to score). **Follow-up:** re-tune on real data, log the adjustment as a new decision.

**D-013 · 2026-07-03 · Globe rendering: three.js 0.185 (pinned) WebGPU+TSL with attribute-only instancing; rate-aggregated persistent arcs + SSE burst pulses; centroids and home origin are UI/config data.**
Rationale, four intertwined choices. (1) **three pinned at exactly 0.185.1**, materials written in TSL with `InstancedBufferAttribute`s ONLY (no storage buffers/compute) so one node graph compiles to both WGSL (WebGPU) and GLSL (the WebGL2 fallback, also reachable via `?gl=1`); the WebGPU/TSL API surface still shifts between releases, so the float pin prevents mid-milestone breakage. No `three/examples` addons (they import the classic `three` build and would bundle the core twice) — the orbit controller is ~60 hand-written lines. (2) **Arcs are rate-aggregated, persistent device→country ribbons** (one per canonical device × destination country from `/api/arcs`) with a traveling comet head; SSE pulses transiently boost an arc's brightness rather than spawning per-query geometry — resolves PRD §6's open question ("per-query spawn vs rate-aggregated flows") in favor of bounded instance counts and honest steady-state density. (3) **Country centroids live in the UI** (`ui/src/globe/countryCentroids.ts`, generated from the MIT `world-countries-centroids` dataset, Taiwan supplemented by hand) — presentation data, not queryable state, so it stays out of the backend. (4) **Home origin is config, never inference**: `PHONEHOME_HOME_LAT/LON` env → `/api/config`, optional browser localStorage override; unset renders a visible "set home" hint over a neutral mid-Atlantic origin. Published media uses a city-level coordinate only.
