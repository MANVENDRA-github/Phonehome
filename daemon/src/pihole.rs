//! Pi-hole v6 REST API adapter (ARCHITECTURE §2.1).
//!
//! Incremental polling with exactly-once semantics:
//! - cursor = JSON `{ "ts_s": <last event's unix-seconds>, "id": <last FTL id> }`
//! - each poll asks the API for `from = ts_s` (inclusive, so a second shared by
//!   two polls is refetched) and then drops everything with `id <= cursor.id`
//!   client-side — FTL query ids increase monotonically, so the overlap never
//!   duplicates and a gap never loses events.
//!
//! Failures degrade to an error the ingest loop logs and retries; the daemon
//! never crash-loops on a source outage.

use crate::ingest::{SOURCE_CONNECT_TIMEOUT, SOURCE_HTTP_TIMEOUT};
use phonehome_core::{Batch, IngestError, Ingestor, QueryEvent};
use serde::{Deserialize, Serialize};

/// Pi-hole v6 statuses that mean the query was blocked. Everything else
/// (FORWARDED, CACHE, RETRIED, IN_PROGRESS, …) counts as allowed.
const BLOCKED_STATUSES: &[&str] = &[
    "GRAVITY",
    "REGEX",
    "DENYLIST",
    "GRAVITY_CNAME",
    "REGEX_CNAME",
    "DENYLIST_CNAME",
    "EXTERNAL_BLOCKED_IP",
    "EXTERNAL_BLOCKED_NULL",
    "EXTERNAL_BLOCKED_NXRA",
    "EXTERNAL_BLOCKED_EDE15",
    "SPECIAL_DOMAIN",
];

#[derive(Debug, Default, Serialize, Deserialize, Clone, Copy, PartialEq)]
pub struct PiholeCursor {
    pub ts_s: i64,
    pub id: i64,
}

pub struct PiholeIngestor {
    source_id: String,
    base_url: String,
    password: String,
    page_size: usize,
    http: reqwest::Client,
    sid: Option<String>,
}

#[derive(Deserialize)]
struct AuthResponse {
    session: AuthSession,
}

#[derive(Deserialize)]
struct AuthSession {
    valid: bool,
    sid: Option<String>,
}

#[derive(Deserialize)]
struct QueriesResponse {
    queries: Vec<PiholeQuery>,
}

#[derive(Deserialize)]
struct PiholeQuery {
    id: i64,
    /// Unix time in seconds (fractional).
    time: f64,
    #[serde(rename = "type")]
    qtype: String,
    domain: String,
    status: Option<String>,
    client: PiholeClient,
}

#[derive(Deserialize)]
struct PiholeClient {
    ip: String,
}

impl PiholeIngestor {
    pub fn new(
        source_id: impl Into<String>,
        base_url: impl Into<String>,
        password: impl Into<String>,
    ) -> Self {
        Self {
            source_id: source_id.into(),
            base_url: base_url.into().trim_end_matches('/').to_owned(),
            password: password.into(),
            page_size: 1000,
            http: reqwest::Client::builder()
                .timeout(SOURCE_HTTP_TIMEOUT)
                .connect_timeout(SOURCE_CONNECT_TIMEOUT)
                .build()
                .expect("reqwest client"),
            sid: None,
        }
    }

    /// Validates a URL + password without persisting anything — the setup
    /// wizard's "test connection" (M5). Reuses the real auth path so a green
    /// probe means real ingestion will authenticate too.
    pub async fn probe(base_url: &str, password: &str) -> Result<(), IngestError> {
        let mut ing = Self::new("probe", base_url, password);
        ing.authenticate().await.map(|_| ())
    }

    async fn authenticate(&mut self) -> Result<String, IngestError> {
        let res = self
            .http
            .post(format!("{}/api/auth", self.base_url))
            .json(&serde_json::json!({ "password": self.password }))
            .send()
            .await
            .map_err(|e| IngestError(format!("pihole auth request: {e}")))?;
        if !res.status().is_success() {
            return Err(IngestError(format!("pihole auth: HTTP {}", res.status())));
        }
        let auth: AuthResponse = res
            .json()
            .await
            .map_err(|e| IngestError(format!("pihole auth body: {e}")))?;
        match (auth.session.valid, auth.session.sid) {
            (true, Some(sid)) => {
                self.sid = Some(sid.clone());
                Ok(sid)
            }
            _ => Err(IngestError("pihole auth rejected (bad password?)".into())),
        }
    }

    async fn fetch_queries(&mut self, from_s: i64) -> Result<QueriesResponse, IngestError> {
        let sid = match &self.sid {
            Some(s) => s.clone(),
            None => self.authenticate().await?,
        };
        let url = format!(
            "{}/api/queries?from={}&length={}",
            self.base_url, from_s, self.page_size
        );
        let res = self
            .http
            .get(&url)
            .header("X-FTL-SID", &sid)
            .send()
            .await
            .map_err(|e| IngestError(format!("pihole queries request: {e}")))?;

        // Session expired: re-auth once, retry once.
        if res.status() == reqwest::StatusCode::UNAUTHORIZED {
            let sid = self.authenticate().await?;
            let res = self
                .http
                .get(&url)
                .header("X-FTL-SID", &sid)
                .send()
                .await
                .map_err(|e| IngestError(format!("pihole queries retry: {e}")))?;
            if !res.status().is_success() {
                return Err(IngestError(format!(
                    "pihole queries: HTTP {}",
                    res.status()
                )));
            }
            return res
                .json()
                .await
                .map_err(|e| IngestError(format!("pihole queries body: {e}")));
        }
        if !res.status().is_success() {
            return Err(IngestError(format!(
                "pihole queries: HTTP {}",
                res.status()
            )));
        }
        res.json()
            .await
            .map_err(|e| IngestError(format!("pihole queries body: {e}")))
    }
}

#[async_trait::async_trait]
impl Ingestor for PiholeIngestor {
    fn source_id(&self) -> &str {
        &self.source_id
    }

    fn kind(&self) -> &'static str {
        "pihole"
    }

    async fn poll(&mut self, cursor: Option<&str>) -> Result<Batch, IngestError> {
        let cur: PiholeCursor = match cursor {
            None => PiholeCursor::default(),
            Some(c) => serde_json::from_str(c)
                .map_err(|e| IngestError(format!("bad pihole cursor {c:?}: {e}")))?,
        };

        let body = self.fetch_queries(cur.ts_s).await?;

        let mut next = cur;
        let mut events = Vec::new();
        let mut skipped_bad_ip = 0usize;
        for q in body.queries {
            if q.id <= cur.id {
                continue; // boundary overlap from the inclusive `from` — already delivered
            }
            let Ok(client_ip) = q.client.ip.parse() else {
                skipped_bad_ip += 1;
                continue;
            };
            let blocked = q
                .status
                .as_deref()
                .is_some_and(|s| BLOCKED_STATUSES.contains(&s));
            let ts = (q.time * 1000.0) as i64;
            next.id = next.id.max(q.id);
            next.ts_s = next.ts_s.max(q.time as i64);
            events.push(QueryEvent {
                ts,
                client_ip,
                client_mac: None, // v6 query log has no MAC; M2 joins /api/network/devices
                domain: q.domain,
                qtype: q.qtype,
                blocked,
                source: self.source_id.clone(),
            });
        }
        if skipped_bad_ip > 0 {
            tracing::warn!(
                skipped_bad_ip,
                "pihole: skipped events with unparseable client ip"
            );
        }

        let next_cursor = serde_json::to_string(&next)
            .map_err(|e| IngestError(format!("serialize cursor: {e}")))?;
        Ok(Batch {
            events,
            next_cursor: Some(next_cursor),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn auth_ok() -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "session": { "valid": true, "sid": "test-sid", "csrf": "x", "validity": 300 }
        }))
    }

    fn query(id: i64, time: f64, domain: &str, status: &str) -> serde_json::Value {
        serde_json::json!({
            "id": id, "time": time, "type": "A", "domain": domain,
            "status": status,
            "client": { "ip": "192.168.1.20", "name": "tv.lan" },
        })
    }

    #[tokio::test]
    async fn authenticates_then_polls_and_maps_events() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/auth"))
            .respond_with(auth_ok())
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/queries"))
            .and(header("X-FTL-SID", "test-sid"))
            .and(query_param("from", "0"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "queries": [
                    query(1, 1000.25, "samsungads.com", "GRAVITY"),
                    query(2, 1000.75, "netflix.com", "FORWARDED"),
                ]
            })))
            .mount(&server)
            .await;

        let mut ing = PiholeIngestor::new("pihole-main", server.uri(), "pw");
        let batch = ing.poll(None).await.unwrap();

        assert_eq!(batch.events.len(), 2);
        assert_eq!(batch.events[0].domain, "samsungads.com");
        assert!(batch.events[0].blocked);
        assert!(!batch.events[1].blocked);
        assert_eq!(batch.events[0].ts, 1_000_250);
        let cur: PiholeCursor =
            serde_json::from_str(batch.next_cursor.as_deref().unwrap()).unwrap();
        assert_eq!(cur, PiholeCursor { ts_s: 1000, id: 2 });
    }

    #[tokio::test]
    async fn boundary_overlap_never_duplicates() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/auth"))
            .respond_with(auth_ok())
            .mount(&server)
            .await;
        // Poll 2 asks from=1000 (inclusive) and the API re-serves id 1 and 2;
        // only id 3 must come through.
        Mock::given(method("GET"))
            .and(path("/api/queries"))
            .and(query_param("from", "1000"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "queries": [
                    query(1, 1000.25, "a.example", "FORWARDED"),
                    query(2, 1000.75, "b.example", "FORWARDED"),
                    query(3, 1000.90, "c.example", "GRAVITY"),
                ]
            })))
            .mount(&server)
            .await;

        let mut ing = PiholeIngestor::new("pihole-main", server.uri(), "pw");
        let cursor = serde_json::to_string(&PiholeCursor { ts_s: 1000, id: 2 }).unwrap();
        let batch = ing.poll(Some(&cursor)).await.unwrap();

        assert_eq!(batch.events.len(), 1, "ids <= cursor.id are dropped");
        assert_eq!(batch.events[0].domain, "c.example");
    }

    #[tokio::test]
    async fn expired_session_reauths_once_and_retries() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/auth"))
            .respond_with(auth_ok())
            .expect(2) // initial auth + re-auth after the 401
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/queries"))
            .respond_with(ResponseTemplate::new(401))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/queries"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "queries": [query(7, 2000.0, "ok.example", "CACHE")]
            })))
            .mount(&server)
            .await;

        let mut ing = PiholeIngestor::new("pihole-main", server.uri(), "pw");
        ing.poll(None).await.unwrap(); // consumes the 401 then succeeds
    }

    #[tokio::test]
    async fn bad_password_is_a_clean_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/auth"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "session": { "valid": false, "sid": null }
            })))
            .mount(&server)
            .await;

        let mut ing = PiholeIngestor::new("pihole-main", server.uri(), "wrong");
        let err = ing.poll(None).await.unwrap_err();
        assert!(err.to_string().contains("rejected"));
    }
}
