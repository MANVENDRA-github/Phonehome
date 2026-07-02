use serde::{Deserialize, Serialize};
use std::net::IpAddr;

/// One normalized DNS query observation, as emitted by an ingestion adapter.
///
/// This is the contract from ARCHITECTURE.md §2.1. Raw events are transient by
/// design (D-005): they update rollups and trigger enrichment, then drop.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueryEvent {
    /// Unix timestamp in milliseconds.
    pub ts: i64,
    /// LAN address of the client that made the query.
    pub client_ip: IpAddr,
    /// Client MAC when the source provides it (lowercase, colon-separated).
    pub client_mac: Option<String>,
    /// Queried domain, as reported by the source.
    pub domain: String,
    /// DNS record type as text ("A", "AAAA", "HTTPS", …).
    pub qtype: String,
    /// Whether the DNS filter blocked this query.
    pub blocked: bool,
    /// Identifier of the configured source this event came from.
    pub source: String,
}

impl QueryEvent {
    /// Attribution key until M2 introduces the device registry: MAC when the
    /// source provides it, otherwise the client IP.
    pub fn client_key(&self) -> String {
        self.client_mac
            .clone()
            .unwrap_or_else(|| self.client_ip.to_string())
    }

    /// Hour bucket for rollups (unix hours; floor division handles pre-1970 safely).
    pub fn bucket_hour(&self) -> i64 {
        self.ts.div_euclid(3_600_000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> QueryEvent {
        QueryEvent {
            ts: 1_782_950_400_000,
            client_ip: "192.168.1.37".parse().unwrap(),
            client_mac: Some("a4:cf:12:34:56:78".into()),
            domain: "samsungads.com".into(),
            qtype: "A".into(),
            blocked: true,
            source: "pihole-main".into(),
        }
    }

    #[test]
    fn query_event_serde_round_trip() {
        let event = sample();
        let json = serde_json::to_string(&event).unwrap();
        let back: QueryEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(event, back);
    }

    #[test]
    fn query_event_without_mac_round_trips() {
        let event = QueryEvent {
            client_mac: None,
            ..sample()
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"client_mac\":null"));
        let back: QueryEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back.client_mac, None);
    }

    #[test]
    fn client_key_prefers_mac_over_ip() {
        assert_eq!(sample().client_key(), "a4:cf:12:34:56:78");
        let no_mac = QueryEvent {
            client_mac: None,
            ..sample()
        };
        assert_eq!(no_mac.client_key(), "192.168.1.37");
    }

    #[test]
    fn bucket_hour_floors() {
        let e = QueryEvent {
            ts: 3_600_000 * 10 + 1,
            ..sample()
        };
        assert_eq!(e.bucket_hour(), 10);
    }
}
