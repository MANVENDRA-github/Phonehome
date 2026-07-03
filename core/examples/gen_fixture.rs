//! Deterministic generator for the committed dev fixture (D-009).
//!
//! Regenerate with:
//!   cargo run -p phonehome-core --example gen_fixture > fixtures/household-01.jsonl
//!
//! The output is SYNTHETIC-REALISTIC and clearly labeled as such (see
//! fixtures/README.md): a plausible 18-device household over 8 days, modeled on
//! public knowledge of what consumer devices query (vendor telemetry, ad
//! networks, CDNs). It is NOT a capture of a real network. Seeded LCG → the
//! same fixture bytes on every run.

use phonehome_core::QueryEvent;

/// Minimal deterministic PRNG (Numerical Recipes LCG) — no external deps.
struct Lcg(u64);

impl Lcg {
    fn next_u32(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 32) as u32
    }
    /// Uniform in [0, n).
    fn below(&mut self, n: usize) -> usize {
        (self.next_u32() as usize) % n.max(1)
    }
    fn chance(&mut self, percent: u32) -> bool {
        self.next_u32() % 100 < percent
    }
}

struct Device {
    ip: &'static str,
    mac: Option<&'static str>,
    /// (domain, qtype, is_tracker) — trackers get blocked most of the time.
    domains: &'static [(&'static str, &'static str, bool)],
    /// Relative chattiness: average queries per hour.
    rate_per_hour: usize,
    /// Active hours (local): devices don't query uniformly around the clock.
    active: (u32, u32),
}

const DEVICES: &[Device] = &[
    Device {
        ip: "192.168.1.20",
        mac: Some("f0:5c:77:11:22:33"),
        domains: &[
            ("log-ingestion.samsungacr.com", "A", true),
            ("samsungads.com", "A", true),
            ("ads.samsung.com", "A", true),
            ("acr0.samsungcloudsolution.com", "A", true),
            ("api.samsungcloudsolution.com", "A", false),
            ("cdn.samsungcloudsolution.net", "A", false),
            ("netflix.com", "A", false),
            ("nflxvideo.net", "A", false),
            ("youtube.com", "HTTPS", false),
            ("googlevideo.com", "A", false),
            ("widget.criteo.com", "A", true),
        ],
        rate_per_hour: 40,
        active: (7, 23),
    },
    Device {
        ip: "192.168.1.21",
        mac: Some("00:62:6e:44:55:66"),
        domains: &[
            ("fw.ring.com", "A", false),
            ("api.ring.com", "A", false),
            ("app-analytics.ring.com", "A", true),
            ("device-metrics-us.amazon.com", "A", true),
            ("kinesis.us-east-1.amazonaws.com", "A", false),
        ],
        rate_per_hour: 22,
        active: (0, 24),
    },
    Device {
        ip: "192.168.1.22",
        mac: Some("68:54:fd:77:88:99"),
        domains: &[
            ("api.amazonalexa.com", "A", false),
            ("device-metrics-us-2.amazon.com", "A", true),
            ("avs-alexa-4-na.amazon.com", "A", false),
            ("ntp-g7g.amazon.com", "A", false),
            ("unagi-na.amazon.com", "A", true),
        ],
        rate_per_hour: 18,
        active: (6, 24),
    },
    Device {
        ip: "192.168.1.23",
        mac: Some("50:14:79:aa:bb:cc"),
        domains: &[
            ("prod-eu.iot.irobotapi.com", "A", false),
            ("logs.iot.irobotapi.com", "A", true),
            ("mqtt-prod.iot.irobotapi.com", "A", false),
        ],
        rate_per_hour: 4,
        active: (9, 18),
    },
    Device {
        ip: "192.168.1.24",
        mac: Some("18:b4:30:dd:ee:ff"),
        domains: &[
            ("nexus.nestlabs.com", "A", false),
            ("logsink.devices.nest.com", "A", true),
            ("time.nestlabs.com", "A", false),
            ("weather.nest.com", "A", false),
        ],
        rate_per_hour: 8,
        active: (0, 24),
    },
    Device {
        ip: "192.168.1.30",
        mac: Some("a8:51:ab:10:20:30"),
        domains: &[
            ("graph.instagram.com", "A", false),
            ("graph.facebook.com", "A", true),
            ("app-measurement.com", "A", true),
            ("firebaselogging-pa.googleapis.com", "A", true),
            ("api.spotify.com", "A", false),
            ("i.instagram.com", "A", false),
            ("chat.signal.org", "HTTPS", false),
            ("mtalk.google.com", "A", false),
            ("doubleclick.net", "A", true),
            ("googleads.g.doubleclick.net", "A", true),
            ("log.tiktokv.com", "A", true),
            ("api16-normal-c-alisg.tiktokv.com", "A", false),
            ("mc.yandex.ru", "A", true),
            ("api.hotstar.com", "A", false),
            ("api-v2.soundcloud.com", "A", false),
        ],
        rate_per_hour: 60,
        active: (7, 24),
    },
    Device {
        ip: "192.168.1.31",
        mac: Some("f4:0f:24:40:50:60"),
        domains: &[
            ("gateway.icloud.com", "HTTPS", false),
            ("api-adservices.apple.com", "A", true),
            ("xp.apple.com", "A", true),
            ("configuration.ls.apple.com", "A", false),
            ("mask.icloud.com", "HTTPS", false),
            ("weather-data.apple.com", "A", false),
        ],
        rate_per_hour: 45,
        active: (6, 24),
    },
    Device {
        ip: "192.168.1.32",
        mac: Some("dc:41:a9:70:80:90"),
        domains: &[
            ("self.events.data.microsoft.com", "A", true),
            ("v10.events.data.microsoft.com", "A", true),
            ("settings-win.data.microsoft.com", "A", true),
            ("update.googleapis.com", "A", false),
            ("github.com", "A", false),
            ("api.github.com", "A", false),
            ("marketplace.visualstudio.com", "A", false),
            ("crates.io", "A", false),
            ("static.rust-lang.org", "A", false),
            ("bitbucket.org", "A", false),
            ("id.atlassian.com", "A", false),
            ("mail.proton.me", "HTTPS", false),
            ("www.bbc.co.uk", "A", false),
        ],
        rate_per_hour: 35,
        active: (9, 23),
    },
    Device {
        ip: "192.168.1.33",
        mac: Some("3c:22:fb:a1:b2:c3"),
        domains: &[
            ("courier.push.apple.com", "A", false),
            ("api-adservices.apple.com", "A", true),
            ("ocsp2.apple.com", "A", false),
            ("smoot.apple.com", "A", true),
            ("icloud-content.com", "A", false),
        ],
        rate_per_hour: 25,
        active: (8, 23),
    },
    Device {
        ip: "192.168.1.40",
        mac: Some("7c:bb:8a:d4:e5:f6"),
        domains: &[
            ("telemetry.nintendo.com", "A", true),
            ("conntest.nintendowifi.net", "A", false),
            ("atum.hac.lp1.d4c.nintendo.net", "A", false),
            ("accounts.nintendo.com", "A", false),
        ],
        rate_per_hour: 6,
        active: (16, 22),
    },
    Device {
        ip: "192.168.1.41",
        mac: Some("00:80:92:0a:1b:2c"),
        domains: &[
            ("epson.com", "A", false),
            ("gdmf.epson.com", "A", true),
            ("firmware.epson.com", "A", false),
        ],
        rate_per_hour: 1,
        active: (0, 24),
    },
    Device {
        ip: "192.168.1.42",
        mac: Some("64:16:66:3d:4e:5f"),
        domains: &[
            ("api.wemo.com", "A", false),
            ("heartbeat.xwemo.com", "A", true),
            ("nat.xwemo.com", "A", false),
        ],
        rate_per_hour: 12,
        active: (0, 24),
    },
    Device {
        ip: "192.168.1.43",
        mac: Some("b0:be:76:6f:70:81"),
        domains: &[
            ("devs.tplinkcloud.com", "A", false),
            ("use1-api.tplinkra.com", "A", true),
            ("ipcserv.tplinkcloud.com", "A", false),
        ],
        rate_per_hour: 10,
        active: (0, 24),
    },
    Device {
        ip: "192.168.1.44",
        mac: Some("ac:84:c6:92:a3:b4"),
        domains: &[
            ("eu-central-1.aws.data.mongodb-api.com", "A", false),
            ("analytics.tuya.com", "A", true),
            ("a2.tuyaeu.com", "A", false),
            ("m2.tuyaeu.com", "A", false),
        ],
        rate_per_hour: 15,
        active: (0, 24),
    },
    Device {
        ip: "192.168.1.45",
        mac: Some("00:17:88:0b:cd:ef"),
        domains: &[
            ("api.meethue.com", "A", false),
            ("time.meethue.com", "A", false),
            ("data.meethue.com", "A", true),
        ],
        rate_per_hour: 6,
        active: (0, 24),
    },
    Device {
        ip: "192.168.1.46",
        mac: Some("c8:d7:78:9a:bc:de"),
        domains: &[
            ("api.home-connect.com", "A", false),
            ("prod.reu.rest.homeconnectegw.com", "A", false),
        ],
        rate_per_hour: 2,
        active: (6, 22),
    },
    // The router itself is a DNS client too. pool.ntp.org deliberately has no
    // entity entry — it exercises explicit-unknown enrichment and the globe's
    // unmapped_queries disclosure.
    Device {
        ip: "192.168.1.1",
        mac: Some("04:d9:f5:12:34:56"),
        domains: &[
            ("nw-dlcdnet.asus.com", "A", false),
            ("dlcdnets.asus.com", "A", false),
            ("pool.ntp.org", "A", false),
        ],
        rate_per_hour: 5,
        active: (0, 24),
    },
    // A client the source only knows by IP (no MAC) — exercises the
    // client_key fallback path end to end.
    Device {
        ip: "192.168.1.50",
        mac: None,
        domains: &[
            ("connectivitycheck.gstatic.com", "A", false),
            ("android.apis.google.com", "A", false),
            ("app-measurement.com", "A", true),
        ],
        rate_per_hour: 9,
        active: (7, 22),
    },
];

/// Fixture window: 8 days ending 2026-07-02 00:00:00 UTC (fixed — determinism).
const END_TS_MS: i64 = 1_782_950_400_000;
const DAYS: i64 = 8;
/// Keeps the committed fixture ~8k events / ~1.4 MB while preserving each
/// device's relative chattiness. Lower it to generate denser load locally.
const RATE_DIVISOR: usize = 5;

fn main() {
    let mut rng = Lcg(0x5eed_2026_0702);
    let start = END_TS_MS - DAYS * 24 * 3_600_000;
    let mut events: Vec<QueryEvent> = Vec::new();

    for day in 0..DAYS {
        for hour in 0..24u32 {
            let bucket_start = start + (day * 24 + hour as i64) * 3_600_000;
            for dev in DEVICES {
                let (from, to) = dev.active;
                if hour < from || hour >= to {
                    // idle hours: rare keep-alive chatter
                    if !rng.chance(15) {
                        continue;
                    }
                }
                let n = if hour < from || hour >= to {
                    1
                } else {
                    // ±50% jitter around the (scaled) nominal rate
                    let base = (dev.rate_per_hour / RATE_DIVISOR).max(1);
                    base / 2 + rng.below(base.max(2))
                };
                for _ in 0..n {
                    let (domain, qtype, tracker) = dev.domains[rng.below(dev.domains.len())];
                    // Pi-hole-style behavior: trackers are on blocklists and
                    // get blocked most of the time; the rest slip through.
                    let blocked = tracker && rng.chance(85);
                    events.push(QueryEvent {
                        ts: bucket_start + rng.below(3_600_000) as i64,
                        client_ip: dev.ip.parse().unwrap(),
                        client_mac: dev.mac.map(str::to_owned),
                        domain: domain.to_owned(),
                        qtype: qtype.to_owned(),
                        blocked,
                        source: "fixture".to_owned(),
                    });
                }
            }
        }
    }

    events.sort_by_key(|e| e.ts);
    for e in &events {
        println!("{}", serde_json::to_string(e).unwrap());
    }
    eprintln!(
        "generated {} events across {} devices",
        events.len(),
        DEVICES.len()
    );
}
