# Phonehome — Market Research & Competitive Landscape

Snapshot date: **2026-07-02**. Method: multi-agent ideation with an independent adversarial verification pass — a verifier agent was instructed to *refute* the novelty claim via live web search across multiple phrasings, and returned a verdict with incumbent evidence. Claims below are tagged:

- ✅ **verified** — survived the adversarial refutation attempt
- ⚠ **single-source** — found during research but not independently verified; treat as directional

## 1. The verdict (verbatim conclusion, 2026-07-02)

✅ *"The specific wedge — self-hosted, DNS-log-only (no packet capture/ARP spoofing), multi-backend (Pi-hole/AdGuard/unbound) ingestion with device fingerprinting, tracker tagging, per-device privacy scorecards with weekly diffs, and a cinematic globe — has no shipped owner as of July 2026; six search phrasings surfaced no repo or product combining these."*

Demand signal: ✅ an open Pi-hole feature request for a per-device world-map view remains unbuilt.

## 2. Incumbents and why they don't own the wedge

| Incumbent | What it does | Why it isn't Phonehome |
|---|---|---|
| **Firewalla** (commercial hardware) | Per-device destination intel, country flags, maps, alerts — the closest incumbent *for the outcome* | Inline packet-based appliance you must buy and put in the traffic path. Phonehome: free, self-hosted, zero network changes. |
| **IoT Inspector 3** (NYU, active 2025 release) | Per-device third-party/advertiser/foreign-country identification | Desktop app using packet capture + ARP spoofing; research-oriented. **Correction from verification:** earlier research called it a "dated 2019 tool" — that was wrong; v3 is active. It still requires traffic interception. |
| **SPR** (Supernetworks, OSS) | Router platform with per-device DNS logs + traffic dashboard | You must replace your router with it. Phonehome rides your existing Pi-hole/AdGuard. |
| **NextDNS / Control D** (hosted DNS) | Per-device attribution + tracker classification | Cloud services — your DNS history lives on their servers. No scorecards, no globe. |
| **NetAlertX** (OSS, active) | LAN presence/inventory — who's on the network | ✅ Confirmed: no destination intelligence at all. Complementary, not competitive. |
| **adguard-dns-visualizer / DNS-Map** | Draw DNS query arcs on a map | Single-backend toys: no device identity, no tracker tagging, no scoring, no diffs. |
| **Sniffnet** (popular Rust app) | Beautiful traffic monitoring | Monitors only the machine it runs on — not the LAN. |
| **ntopng / Zenarmor** | Self-hosted per-device destination analytics | Require packet capture or inline gateway placement; ops-heavy, ops-audience. |
| **Pi-hole / AdGuard Home themselves** | Collect exactly the right data | Flat query tables. They are Phonehome's *data source*, and their communities are its distribution channel. |

## 3. Boundary conditions (what would invalidate this project)

Re-check before M1 and before launch:

1. **Pi-hole ships a native per-device map/scorecard view** — the feature request getting built would absorb much of the wedge. Watch Pi-hole release notes.
2. **NetAlertX adds destination intelligence** — it has the device-registry half already.
3. **A Firewalla-style OSS project ships DNS-log-only mode** — search "self-hosted device privacy dashboard DNS" quarterly.
4. **Widespread device DoH adoption** erodes DNS-log visibility faster than expected — if a typical household's smart devices majority-bypass the filter, the data source thins (see PRD risk table; also an argument to ship sooner).

## 4. Supporting market context (from the 2026-07 research pass)

- ✅ **Observability-category lesson (transfers here):** eye-candy alone doesn't retain; the drill-down/actionable loop does. Phonehome's retention loop is the weekly diff + explainable scorecard.
- ✅ **Easy-install pattern:** the fastest-growing self-hosted tools ship as `docker run`/single binary; heavyweight multi-service installs (e.g., ClickHouse+Redis+worker stacks) are cited as adoption friction even for popular tools.
- ⚠ **Reddit conversion:** a comparable OSS launch reported r/selfhosted / r/opensource / r/programming as its largest star source, ~5–8% conversion, 2,000+ stars in month one.
- ⚠ **HN expectations:** AI/dev tools average ~121 stars in 24h post-HN-launch (~289 in a week); front-page Show HN can do 500–2,000, but one comparable team called HN results unreliable — don't bet the launch on a single HN moment.
- ⚠ **Pre-existing traction predicts launch outcomes** — baseline stars are a top predictor of post-launch gains. Implication: soft-launch to the Pi-hole community first, build a baseline, then the bigger push.

## 5. Distribution plan

1. **README-as-landing-page** (shipped in this PR): one-line value prop, hero GIF slot above the fold, ≤5-step quickstart, doc map. ⚠ This four-element checklist is from a single case study but is low-cost and consistent with observed patterns.
2. **The GIF is the product's ad:** 10 seconds, real data, real network — the globe with labeled devices firing arcs at tracker endpoints. Produced at M4; nothing is broadcast before it exists.
3. **Channel sequence:** (a) Pi-hole discourse + r/pihole soft launch ("built this on top of your logs") → (b) r/selfhosted launch post with the GIF → (c) Show HN once issues from (a)/(b) are triaged and install is proven on strangers' setups.
4. **Positioning language:** "Your Pi-hole already knows. Phonehome shows you." Never claim interception-level completeness — the honesty about DNS-only blind spots is itself differentiating (see PRD §6).
5. **Post-launch loop:** the optional redacted "Hall of Shame" share card (PRD C-1) turns users into distributors; community-editable entity mappings turn users into contributors.

## 6. Provenance

Research artifacts live in the owner's knowledge vault (`claude-memory` repo): `decisions/fresh-standalone-project-ideas.md` (this idea's cohort + verdicts) and `decisions/side-project-pipeline-research.md` (the wider July 2026 market scan). Workflow run IDs: ideation+verification `wf_39846b49-82a`; market scan `wf_f8e0b211-fbd`.
