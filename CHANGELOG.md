# Changelog

All notable changes to Phonehome. Format loosely follows [Keep a Changelog](https://keepachangelog.com/); this project uses milestone-based development (see [SPEC.md](SPEC.md)).

## [Unreleased]

### Fixed
- **Source polls can no longer hang forever.** The Pi-hole and AdGuard HTTP clients had no timeout (reqwest's default), so a source that accepted the connection and never answered parked the ingest loop on its `await` permanently — ingestion for that source died silently until the daemon restarted. Both clients now carry a 20s request and 5s connect timeout.
- **`?window=` no longer overflows.** A large `window` (e.g. `i64::MAX`) overflowed `hours × 3_600_000` — a panic in debug, a silently wrong time window in release. Oversized windows now return `400`.
- **AdGuard no longer drops devices in silence.** Query-log entries whose `client` is a ClientID/hostname rather than an IP were discarded with no counter and no log line, so a whole device's traffic could vanish invisibly. They are now counted and warned about, matching the Pi-hole adapter.
- **A failed `set_home` write is now logged.** `POST /api/sources` matched only the `JoinError` from `spawn_blocking`, discarding the inner database error, so a home coordinate could silently fail to persist.
- **Replacing a source no longer double-counts.** `spawn_source` started the new ingest loop *before* aborting the old one; both could poll the same cursor and additively apply the same events. The old loop is now aborted first, under the registry lock.
- **`score()` honors its zero-traffic contract.** The entity- and country-spread components skipped the `total > 0` guard, so a device with no queries but nonzero spreads scored above 0 (40/100 with default weights).

### Changed
- Docs corrected to match the code: GeoIP/GeoLite2 is not used anywhere (D-011) but was still described as part of the enrichment stack; D-009 misstated the fixture as 15 devices over 8 days (it is 18 over 14); CLAUDE.md omitted the `playwright-smoke` CI job and the `/api/diffs` endpoint.

## [0.1.0] — first public release (M5)

The first shippable version: `git clone` → `docker compose up` → point it at your Pi-hole/AdGuard → meet your house, in two commands.

### Added
- **First-run setup wizard** — paste your Pi-hole v6 / AdGuard Home address + token, test the connection live, and start ingesting with no process restart (`POST /api/sources` probes → persists → starts a runtime ingest loop). `GET /api/config` reports `needs_setup` for first-run detection.
- **Weekly diff** (`GET /api/diffs` + view) — per-device week-over-week change: score delta, count deltas, and the domains new this week ("+N new tracker domains"), computed from hourly rollups.
- **WebGPU globe** (WebGL2 fallback) — instanced device→country arcs, tracker-share coloring, device filter, click-through from any arc to its domains and raw hourly rollups, SSE live pulses. Measured smooth at ≥10k arcs on integrated graphics.
- **Device identity** — MAC/IP keying, OUI vendor lookup, DHCP/mDNS name intake, rename + merge.
- **Enrichment + scorecards** — tracker classification (oisd/StevenBlack seed), domain→company/country entity map, explainable 0–100 privacy-risk score with visible inputs, weekly snapshots.
- **Ingestion** — Pi-hole v6 and AdGuard Home adapters behind one `Ingestor` trait, plus a JSONL fixture replayer; exactly-once cursor semantics across restarts.
- **Hardened one-container deploy** — pinned base images, non-root, `read_only` root filesystem with a `/data` volume, `cap_drop: ALL`, `no-new-privileges`, resource limits, and a built-in `--healthcheck`.

### Notes
- **100% local, zero telemetry** (D-005). The only outbound traffic is the poll to your own DNS filter. Raw queries are never retained — only hourly rollups.
- Wizard credentials are stored plaintext in the local-only SQLite DB and never returned by any API ([D-014](DECISIONS.md)).
- The committed dev fixture and any demo media are **synthetic-realistic and labeled as such** ([D-009](DECISIONS.md)); a real anonymized household capture is planned before broad launch claims.
- Destination **country comes from the entity map, not GeoIP** ([D-011](DECISIONS.md)) — no MaxMind key needed.

[0.1.0]: https://github.com/MANVENDRA-github/Phonehome/releases/tag/v0.1.0
