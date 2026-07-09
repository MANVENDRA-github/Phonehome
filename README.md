# Phonehome

**Meet everything your house talks to.** Phonehome is a self-hosted privacy radar for your home network: it reads the DNS logs you already have (Pi-hole, AdGuard Home), figures out *which device* asked for *what*, and shows you — on a live 3D globe and per-device scorecards — exactly where your smart TV, doorbell, and every other gadget phones home.

> **Status: v0.1.0 — M5 (ship) complete.** One `docker compose up`, a first-run setup wizard (paste your Pi-hole/AdGuard URL + token → data flows in seconds, no restart), the WebGPU globe (WebGL fallback) of live device→country arcs, per-device privacy scorecards, and a weekly diff that shows what changed. Measured smooth at 10,000 arcs on integrated graphics (frame-time tables in [PROOF.md](PROOF.md) §M4), guarded in CI by a Playwright smoke + a container smoke, evidence in [PROOF.md](PROOF.md). The demo media below replays a synthetic-labeled fixture pending a real-household capture ([D-009](DECISIONS.md)). Star/watch to follow along.

![Phonehome globe — labeled household devices firing arcs at their real destination countries](docs/hero.gif)

<sub>**Replayed fixture** — this recording replays the committed synthetic-realistic fixture (real vendor/tracker hostnames, deterministic generator; see [D-009](DECISIONS.md)) through the real daemon and globe. It will be re-recorded from an anonymized real-household capture before launch.</sub>

## What it does

- **Ingests DNS query logs** from Pi-hole v6 or AdGuard Home over their APIs. No packet capture, no ARP spoofing, no interception hardware — if you run a DNS filter, you already have the data.
- **Identifies devices** on your LAN (MAC OUI vendor lookup, mDNS and DHCP hostnames) so queries belong to *"LG TV — living room"*, not `192.168.1.37`.
- **Tags every destination** against public tracker blocklists, and maps domains to the companies behind them — and to the country that company answers to (from the entity map, not GeoIP).
- **Scores each device** with a privacy scorecard, and diffs it weekly: *"your new doorbell added 6 tracker domains this week."*
- **Renders the globe** — a WebGPU world map with live arcs from each device to the ad networks and tracker endpoints it contacts, in your browser, entirely on your machine.

**Everything stays local.** No cloud, no accounts, no telemetry. Your DNS history never leaves your network.

## Quickstart

Two commands, under five minutes:

```sh
git clone https://github.com/MANVENDRA-github/Phonehome.git && cd Phonehome
docker compose up -d --build
```

Open **http://localhost:8480**. On first run the **setup wizard** asks for your Pi-hole or AdGuard Home address and app password — click *Test connection*, then *Start*, and the globe fills in as your DNS logs are read. Nothing leaves your machine.

- **Pi-hole v6:** address like `http://pi.hole` (or `http://<ip>`), and the app password from *Settings → Web interface / API*.
- **AdGuard Home:** address like `http://<ip>:3000` plus your admin username and password.

Just trying it out? The repo ships a synthetic-realistic fixture — run the daemon with `PHONEHOME_FIXTURE=fixtures/household-01.jsonl` and skip the wizard. See [docs/INSTALL.md](docs/INSTALL.md) for details (updating, LAN access, GeoLite2, strict-local, backups).

By default the container binds to `127.0.0.1` only. To reach the globe from another device on your LAN, see the install guide.

## Why this doesn't exist yet

Tools that know *who is on your network* (NetAlertX) don't know *where devices send data*. Tools that know destinations (Firewalla, IoT Inspector 3) need dedicated hardware or packet capture. Your Pi-hole knows both — but shows you a flat query table. Phonehome is the missing join: **device-centric attribution + tracker classification + privacy scoring + a map you can't unsee**, self-hosted, on data you already collect. The full competitive analysis (adversarially verified July 2026) is in [RESEARCH.md](RESEARCH.md).

## Documentation map

| Doc | What's in it |
|---|---|
| [PRD.md](PRD.md) | Product requirements — problem, users, v1 scope, success metrics, risks |
| [RESEARCH.md](RESEARCH.md) | Competitive landscape (verified), the open wedge, distribution plan |
| [ARCHITECTURE.md](ARCHITECTURE.md) | System design — Rust daemon, SQLite schema, enrichment, globe frontend |
| [SPEC.md](SPEC.md) | v1 build spec — milestones M0–M5 with acceptance criteria and proof rules |
| [DECISIONS.md](DECISIONS.md) | Decision log (D-001…) — what was decided and why |
| [PROOF.md](PROOF.md) | Per-milestone evidence — real command output, real measurements |
| [CLAUDE.md](CLAUDE.md) | Operating context for AI coding agents working in this repo |

## License

[MIT](LICENSE)
