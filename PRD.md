# Phonehome — Product Requirements Document

| | |
|---|---|
| **Product** | Phonehome — self-hosted privacy radar for home networks |
| **Author** | Manvendra (with Claude Code) |
| **Created** | 2026-07-02 |
| **Status** | Approved for v1 build (see [SPEC.md](SPEC.md)) |
| **Provenance** | Idea originated and adversarially novelty-checked in a multi-agent research pass on 2026-07-02; competitive evidence in [RESEARCH.md](RESEARCH.md) |

## 1. Problem

Modern households run dozens of network-connected devices — smart TVs, doorbells, speakers, robot vacuums, thermostats — and essentially all of them phone home to manufacturers, analytics services, and ad networks. People *feel* this ("what is my TV actually doing?") but have no way to *see* it:

- **Pi-hole / AdGuard Home users already collect the evidence** — every DNS query from every device — but their dashboards present it as a flat, paginated query table. The data is there; the picture is not. (An open Pi-hole feature request for a per-device world map remains unbuilt — demand signal, verified.)
- **Tools that do show destination intelligence require new trust or new hardware:** Firewalla is a commercial inline appliance; NYU's IoT Inspector 3 uses packet capture/ARP spoofing and is research-oriented; ntopng/Zenarmor need traffic-level access.
- **Device inventory tools (NetAlertX) tell you WHO is on the network, never WHERE devices send data.**

The unmet job: *"show me what every device in my house talks to, score how bad it is, and tell me when it gets worse — using data I already have, on infrastructure I already run."*

## 2. Target users

1. **Primary — the self-hoster.** Runs Pi-hole or AdGuard Home (Pi-hole alone: ~50k-star community), a Docker host, and frequents r/selfhosted. Wants insight, owns their data, converts to GitHub stars when a tool respects both.
2. **Secondary — the privacy-conscious tinkerer.** Comfortable following a docker-compose README if the payoff is visceral. Arrives via the shared globe screenshot, stays if install is ≤ 2 commands.
3. **Non-target (v1):** enterprises, MSPs, people with no DNS filter installed (v2 may add a standalone DNS-forwarder mode to serve them).

## 3. Value proposition & the 10-second demo

**One line:** *Your Pi-hole already knows what your house is doing — Phonehome shows you.*

**The 10-second demo (definition of "wow", drives all UI priorities):** a dark WebGPU globe; labeled devices ("Samsung TV", "Ring Doorbell") along the bottom; live arcs streaming from each device to endpoints across the planet, tracker arcs burning red; a counter — *"Samsung TV: 14 countries, 38 tracker domains today."* Rendered from **real data on a real network** (honesty rule: demo media must never use synthetic data without a visible label).

Secondary hook with durable utility: the **weekly diff** — *"your new doorbell added 6 tracker domains this week"* — which makes Phonehome a tool you keep, not a screensaver you try. (Lesson carried over from prior research: observability eye-candy must pair with a drill-down/actionable loop to retain users.)

## 4. v1 scope (MoSCoW)

### Must have
- **M-1. Pi-hole v6 API poller** — incremental ingestion of DNS query history (client IP/MAC, domain, timestamp, blocked/allowed status) into local storage. Replayable fixture for development without a live Pi-hole.
- **M-2. AdGuard Home poller** — same contract, second first-class backend (proves the source-agnostic ingestion boundary).
- **M-3. Device identity** — merge MAC OUI vendor lookup + DHCP hostnames + mDNS names into a persistent device registry; manual rename/merge via UI. Queries attribute to devices, not IPs.
- **M-4. Destination enrichment** — tag each queried domain against public tracker/ad blocklists (oisd, StevenBlack), GeoIP it (MaxMind GeoLite2), and map domain → owning entity (e.g., `samsungads.com` → Samsung Ads) via a curated entity list.
- **M-5. Per-device privacy scorecard** — score from tracker share, entity diversity, country spread, chattiness; explainable (show the inputs, never a black-box number).
- **M-6. Weekly diff** — snapshot per device per week; scorecard shows deltas ("+6 tracker domains, +2 countries") with the new domains listed.
- **M-7. The globe** — WebGPU (WebGL fallback) world map with device→destination arcs, live-updating, filter by device/category, click an arc → the underlying domains and queries.
- **M-8. Single docker-compose install** — one service (daemon + embedded UI) + volume. First-run setup wizard: paste Pi-hole/AdGuard URL + token, see data within 60 seconds.
- **M-9. 100% local operation** — no cloud calls except user-initiated data downloads (blocklists, GeoLite2 DB, OUI table — all cacheable/bundleable), zero telemetry.

### Should have
- S-1. Live mode — SSE stream so new queries appear on the globe within seconds of polling.
- S-2. JSON export of scorecards/diffs (automation-friendly; future Home Assistant integration point).
- S-3. Dark/light theme (dark default — the globe is the hero).

### Could have
- C-1. "Hall of Shame" shareable card — a redacted-by-default PNG summary of your worst device (distribution loop).
- C-2. Basic anomaly flag — device contacts a never-before-seen country.

### Won't have in v1 (explicitly deferred → §7 roadmap)
- Packet capture / ARP spoofing / inline gateway mode (D-001 — the DNS-log-only constraint is what keeps this solo-buildable and zero-trust-change)
- unbound / dnsmasq / BIND log adapters
- Alerting & notifications (email/webhook/ntfy)
- Home Assistant integration
- Multi-user/auth beyond a single shared password
- Historical retention policies / data pruning UI

## 5. Success metrics

| Metric | Target | Why |
|---|---|---|
| Install friction | ≤ 2 commands, ≤ 5 min to first globe | Easy-install is the #1 verified distribution lever for self-hosted tools |
| Time-to-wow | < 60 s from setup wizard to populated globe | The demo moment must happen on the user's own data |
| Demo GIF | 10-sec real-data globe GIF in README at M4 | The shareable artifact IS the distribution strategy |
| Launch | r/selfhosted + Pi-hole community post at v1; HN after traction | Reddit converted ~5–8% to stars for comparable tools (single-source claim; treat as directional) |
| Stars | 500 in first month post-launch | Calibrated to comparable r/selfhosted launches; a stretch, not a vanity floor |
| Honesty rule | All published numbers/GIFs from real runs | Non-negotiable; carried from prior projects' PROOF discipline |

## 6. Risks & mitigations

| Risk | Severity | Mitigation |
|---|---|---|
| **Firewalla owns the outcome commercially** ("see where your camera sends data") | Med | Different segment: Phonehome is free, self-hosted, no hardware, no inline placement. Position as "the Pi-hole-native answer", never "Firewalla killer". |
| **DNS-only attribution limits** — devices using DoH/DoT or hardcoded IPs bypass the DNS filter and are invisible | Med-High | Be loud and honest about it in README/UI ("what Phonehome can't see"); surface *detected* DoH endpoints (DoH provider domains in queries) as a scorecard signal. Blind spots disclosed beat blind spots discovered. |
| **Pi-hole/AdGuard API churn** (Pi-hole v6 API is new) | Med | Version-pinned adapters behind one ingestion trait; replayable fixtures decouple dev from live APIs. |
| **Fast follow** — the wedge was verified open (July 2026) but is cheap to copy once demonstrated | Med | Speed + the globe's execution quality + weekly-diff utility loop; ship M1–M4 before broadcasting. |
| **Entity mapping quality** — domain→company data is the weakest public dataset | Med | Start with top-N tracker entities (covers most arcs); ship the mapping as editable data files; accept community PRs. |
| **GeoLite2 licensing** — requires (free) MaxMind account for updates | Low | Document it in setup; ship a bundled snapshot where license permits; degrade gracefully (globe needs country-level only). |

## 7. Post-v1 roadmap (v2 candidates, unordered)
- unbound/dnsmasq/BIND adapters; standalone DNS-forwarder mode (serves users with no Pi-hole)
- Alerting: webhook/ntfy/email on scorecard regressions or anomaly flags
- Home Assistant add-on + entity export
- Device fingerprinting improvements (DHCP fingerprints, UPnP/SSDP)
- Public device scorecard index — anonymized, opt-in "how does my TV model compare?"

## 8. Open questions (resolve during build, log in DECISIONS.md)
- Scorecard formula weights (M3 spike: validate against 2–3 real households' data)
- Arc animation semantics: per-query spawn vs. rate-aggregated flows (M4 perf spike decides)
- Whether the entity-mapping list lives in-repo or as a separately versioned data artifact
