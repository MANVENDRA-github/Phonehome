# Installing Phonehome

Phonehome ships as **one container and one volume** (D-006). No external database, broker, or worker — SQLite (WAL) in a Docker volume is the whole datastore.

## Requirements

- Docker with Compose v2 (`docker compose`, not the legacy `docker-compose`).
- A running **Pi-hole v6** or **AdGuard Home** on your network whose API you can reach from the container.
- ~200 MB RAM and a little disk (a busy household is comfortably under a few hundred MB of SQLite for a year — raw queries are never retained, only hourly rollups, D-005).

## Install

```sh
git clone https://github.com/MANVENDRA-github/Phonehome.git && cd Phonehome
docker compose up -d --build
```

Open **http://localhost:8480**. The first-run **setup wizard** takes your source URL + token, tests the connection, and starts ingesting — the globe fills in within a poll interval (default 15 s). Nothing is sent anywhere but your own Pi-hole/AdGuard.

### Getting the token

- **Pi-hole v6** — Settings → *Web interface / API* → app password. Address is `http://pi.hole` or `http://<pi-hole-ip>`.
- **AdGuard Home** — your admin username + password. Address is `http://<ip>:3000` (or your configured port).

You can also configure sources without the wizard by setting environment variables in `docker-compose.yml` (`PHONEHOME_PIHOLE_URL` + `PHONEHOME_PIHOLE_PASSWORD`, or `PHONEHOME_ADGUARD_URL` + `PHONEHOME_ADGUARD_USERNAME` + `PHONEHOME_ADGUARD_PASSWORD`). Env-configured sources skip the wizard.

## Try it with the sample data

No DNS filter handy? Replay the committed synthetic fixture (clearly labeled in-app):

```sh
PHONEHOME_FIXTURE=fixtures/household-01.jsonl cargo run -p phonehome-daemon
# or set the same env var on the compose service
```

## LAN access

By default the container publishes to `127.0.0.1:8480` only — the globe is reachable from the host, not the rest of your network. To view it from another device, edit `docker-compose.yml`:

```yaml
    ports:
      - "8480:8480"   # was "127.0.0.1:8480:8480"
```

Phonehome has no authentication (it's a local, single-user tool), so only expose it on networks you trust. Do **not** put it on the public internet.

## Configuration reference

| Env var | Default | Purpose |
|---|---|---|
| `PHONEHOME_PORT` | `8480` | HTTP port. |
| `PHONEHOME_DB` | `/data/phonehome.db` | SQLite path (in the volume). |
| `PHONEHOME_HOME_LAT` / `_LON` | unset | Home location = the globe's arc origin. Also settable in the wizard. |
| `PHONEHOME_POLL_INTERVAL_SECS` | `15` | Live-source poll cadence. |
| `PHONEHOME_PIHOLE_URL` / `_PASSWORD` | unset | Configure a Pi-hole source from env (skips the wizard). |
| `PHONEHOME_ADGUARD_URL` / `_USERNAME` / `_PASSWORD` | unset | Configure an AdGuard source from env. |
| `PHONEHOME_FIXTURE` | unset | Replay a JSONL fixture instead of a live source. |

Credentials entered in the wizard are stored in the local SQLite DB (plaintext, owner-only file mode, never returned by any API — see [D-014](../DECISIONS.md)).

## Enrichment data & GeoLite2

Tracker classification and the domain→company/country map ship as editable seed data (`core/data/trackers.txt`, `core/data/entities.toml`). Destination **country comes from the entity map, not GeoIP** ([D-011](../DECISIONS.md)) — no MaxMind key is needed, and nothing phones home to resolve it. (A future optional user-provided GeoLite2 `.mmdb` for *unmapped* domains is a documented follow-up, not required.)

## Strict-local

Phonehome makes no cloud calls; the only outbound traffic is the poll to *your* Pi-hole/AdGuard (D-005). There is no telemetry to disable.

## Updating

```sh
git pull
docker compose up -d --build
```

Your data volume (`phonehome-data`) persists across rebuilds. The schema migrates forward automatically on start.

## Backups

Everything is in the `phonehome-data` volume. Back it up with:

```sh
docker run --rm -v phonehome-data:/data -v "$PWD":/backup busybox \
  tar czf /backup/phonehome-backup.tar.gz -C /data .
```

## Health & troubleshooting

- Health: `curl http://localhost:8480/api/health` → `{"status":"alive",...}`. The container's `HEALTHCHECK` runs `phonehome-daemon --healthcheck` internally.
- Ingestion state: `curl http://localhost:8480/api/stats` (per-source cursor + totals).
- Logs: `docker compose logs -f`.
- A source that goes unreachable is retried, never fatal — the daemon logs `poll failed; will retry` and keeps serving.
