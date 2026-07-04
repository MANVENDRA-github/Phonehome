# Phonehome

**Meet everything your house talks to.** Phonehome is a self-hosted privacy radar for your home network: it reads the DNS logs you already have (Pi-hole, AdGuard Home), figures out *which device* asked for *what*, and shows you — on a live 3D globe and per-device scorecards — exactly where your smart TV, doorbell, and every other gadget phones home.

> **Status: pre-v1 — M4 (the globe) done; M5 (ship) in progress.** The WebGPU globe (WebGL fallback) renders live device→country arcs — one instanced draw call, tracker-share coloring, device filter rail, click-through from any arc to its domains and their raw hourly rollups in two clicks, SSE live pulses as ingestion commits. Measured smooth at 10,000 arcs on integrated graphics (frame-time tables in [PROOF.md](PROOF.md) §M4), CI-guarded by a Playwright smoke, and captured in the hero GIF above. M5 in progress: a **first-run setup wizard** (paste your Pi-hole/AdGuard URL + token → data starts flowing, no restart) now lands the backend + UI. Next up in M5: weekly-diff view, hardened compose, and the v0.1.0 release. Star/watch to follow along.

![Phonehome globe — labeled household devices firing arcs at their real destination countries](docs/hero.gif)

<sub>**Replayed fixture** — this recording replays the committed synthetic-realistic fixture (real vendor/tracker hostnames, deterministic generator; see [D-009](DECISIONS.md)) through the real daemon and globe. It will be re-recorded from an anonymized real-household capture before launch.</sub>

## What it does

- **Ingests DNS query logs** from Pi-hole v6 or AdGuard Home over their APIs. No packet capture, no ARP spoofing, no interception hardware — if you run a DNS filter, you already have the data.
- **Identifies devices** on your LAN (MAC OUI vendor lookup, mDNS and DHCP hostnames) so queries belong to *"LG TV — living room"*, not `192.168.1.37`.
- **Tags every destination** against public tracker blocklists and GeoIP, and maps domains to the companies behind them.
- **Scores each device** with a privacy scorecard, and diffs it weekly: *"your new doorbell added 6 tracker domains this week."*
- **Renders the globe** — a WebGPU world map with live arcs from each device to the ad networks and tracker endpoints it contacts, in your browser, entirely on your machine.

**Everything stays local.** No cloud, no accounts, no telemetry. Your DNS history never leaves your network.

## Planned quickstart (v1 target)

```sh
# not yet functional — v1 install contract, see SPEC.md M5
docker compose up -d
# open http://localhost:8480, point it at your Pi-hole/AdGuard, meet your house
```

Install friction budget: **two commands, under five minutes** — that's a hard v1 requirement, not an aspiration.

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
