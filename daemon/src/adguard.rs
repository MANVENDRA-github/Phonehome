//! AdGuard Home adapter (ARCHITECTURE §2.1) — the second live backend, which
//! exists to prove the source-agnostic `Ingestor` boundary (D-003): everything
//! downstream of this file is identical to the Pi-hole path.
//!
//! AdGuard's `/control/querylog` returns entries newest-first, paginated
//! backwards via `older_than`, with no monotonic id — so the exactly-once cursor
//! is the newest RFC3339 timestamp seen, and each poll keeps only entries
//! strictly newer than it (paginating back until it reaches the cursor). Auth is
//! a session cookie from `POST /control/login`; a 401/403 triggers one re-login
//! and retry. Failures degrade to an error the ingest loop logs and retries.

use phonehome_core::{Batch, IngestError, Ingestor, QueryEvent};
use serde::Deserialize;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

/// Bound a single poll's backward pagination (the log is finite, but guard
/// against a misbehaving server).
const MAX_PAGES: usize = 100;

pub struct AdguardIngestor {
    source_id: String,
    base_url: String,
    username: String,
    password: String,
    page_size: usize,
    http: reqwest::Client,
    logged_in: bool,
}

#[derive(Deserialize)]
struct QueryLog {
    #[serde(default)]
    data: Vec<LogEntry>,
}

#[derive(Deserialize)]
struct LogEntry {
    /// RFC3339 nanosecond timestamp.
    time: String,
    question: Question,
    client: String,
    #[serde(default)]
    reason: String,
}

#[derive(Deserialize)]
struct Question {
    name: String,
    #[serde(default, rename = "type")]
    qtype: String,
}

/// Parses AdGuard's RFC3339 timestamp to unix nanoseconds for ordering.
fn parse_nanos(s: &str) -> Result<i128, IngestError> {
    OffsetDateTime::parse(s, &Rfc3339)
        .map(|dt| dt.unix_timestamp_nanos())
        .map_err(|e| IngestError(format!("bad adguard time {s:?}: {e}")))
}

impl AdguardIngestor {
    pub fn new(
        source_id: impl Into<String>,
        base_url: impl Into<String>,
        username: impl Into<String>,
        password: impl Into<String>,
    ) -> Self {
        Self {
            source_id: source_id.into(),
            base_url: base_url.into().trim_end_matches('/').to_owned(),
            username: username.into(),
            password: password.into(),
            page_size: 500,
            // cookie_store keeps the session cookie from /control/login.
            http: reqwest::Client::builder()
                .cookie_store(true)
                .build()
                .expect("reqwest client"),
            logged_in: false,
        }
    }

    async fn login(&mut self) -> Result<(), IngestError> {
        let res = self
            .http
            .post(format!("{}/control/login", self.base_url))
            .json(&serde_json::json!({ "name": self.username, "password": self.password }))
            .send()
            .await
            .map_err(|e| IngestError(format!("adguard login request: {e}")))?;
        if !res.status().is_success() {
            return Err(IngestError(format!(
                "adguard login rejected: HTTP {} (bad credentials?)",
                res.status()
            )));
        }
        self.logged_in = true;
        Ok(())
    }

    async fn fetch_page(&mut self, older_than: Option<&str>) -> Result<QueryLog, IngestError> {
        if !self.logged_in {
            self.login().await?;
        }
        let mut url = format!(
            "{}/control/querylog?limit={}",
            self.base_url, self.page_size
        );
        if let Some(ot) = older_than {
            url.push_str(&format!("&older_than={ot}"));
        }
        let res = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| IngestError(format!("adguard querylog request: {e}")))?;

        // Session expired: re-login once, retry once.
        if matches!(
            res.status(),
            reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN
        ) {
            self.logged_in = false;
            self.login().await?;
            let res = self
                .http
                .get(&url)
                .send()
                .await
                .map_err(|e| IngestError(format!("adguard querylog retry: {e}")))?;
            if !res.status().is_success() {
                return Err(IngestError(format!(
                    "adguard querylog: HTTP {}",
                    res.status()
                )));
            }
            return res
                .json()
                .await
                .map_err(|e| IngestError(format!("adguard querylog body: {e}")));
        }
        if !res.status().is_success() {
            return Err(IngestError(format!(
                "adguard querylog: HTTP {}",
                res.status()
            )));
        }
        res.json()
            .await
            .map_err(|e| IngestError(format!("adguard querylog body: {e}")))
    }

    fn to_event(&self, entry: &LogEntry) -> Option<QueryEvent> {
        let client_ip = entry.client.parse().ok()?;
        let blocked = entry.reason.starts_with("Filtered");
        let ts = (parse_nanos(&entry.time).ok()? / 1_000_000) as i64;
        Some(QueryEvent {
            ts,
            client_ip,
            client_mac: None, // AdGuard query log carries no MAC
            domain: entry.question.name.trim_end_matches('.').to_owned(),
            qtype: entry.question.qtype.clone(),
            blocked,
            source: self.source_id.clone(),
        })
    }
}

#[async_trait::async_trait]
impl Ingestor for AdguardIngestor {
    fn source_id(&self) -> &str {
        &self.source_id
    }

    fn kind(&self) -> &'static str {
        "adguard"
    }

    async fn poll(&mut self, cursor: Option<&str>) -> Result<Batch, IngestError> {
        let cursor_nanos: i128 = match cursor {
            Some(c) => parse_nanos(c)?,
            None => i128::MIN, // first poll: ingest all retained history
        };

        let mut newest: Option<String> = None;
        let mut events = Vec::new();
        let mut older_than: Option<String> = None;

        'pages: for _ in 0..MAX_PAGES {
            let page = self.fetch_page(older_than.as_deref()).await?;
            if page.data.is_empty() {
                break;
            }
            let full = page.data.len() >= self.page_size;
            let oldest_time = page.data.last().map(|e| e.time.clone());

            for entry in &page.data {
                if newest.is_none() {
                    newest = Some(entry.time.clone()); // first entry overall = newest
                }
                if parse_nanos(&entry.time)? <= cursor_nanos {
                    break 'pages; // reached already-delivered territory
                }
                if let Some(ev) = self.to_event(entry) {
                    events.push(ev);
                }
            }

            if !full {
                break; // last page
            }
            older_than = oldest_time; // page all-new; go further back
        }

        // Keep the old cursor if this poll saw nothing new.
        let next_cursor = newest.or_else(|| cursor.map(str::to_owned));
        Ok(Batch {
            events,
            next_cursor,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn login_ok() -> ResponseTemplate {
        ResponseTemplate::new(200).insert_header("Set-Cookie", "agh_session=test; Path=/")
    }

    fn entry(time: &str, name: &str, client: &str, reason: &str) -> serde_json::Value {
        serde_json::json!({
            "time": time,
            "question": { "name": name, "type": "A", "class": "IN" },
            "client": client,
            "reason": reason,
        })
    }

    #[tokio::test]
    async fn logs_in_then_polls_and_maps_entries() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/control/login"))
            .respond_with(login_ok())
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/control/querylog"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [
                    entry("2026-07-03T10:00:02Z", "doubleclick.net.", "192.168.1.30", "FilteredBlackList"),
                    entry("2026-07-03T10:00:01Z", "github.com.", "192.168.1.30", "NotFilteredNotFound"),
                ]
            })))
            .mount(&server)
            .await;

        let mut ing = AdguardIngestor::new("adguard", server.uri(), "admin", "pw");
        let batch = ing.poll(None).await.unwrap();

        assert_eq!(batch.events.len(), 2);
        assert_eq!(batch.events[0].domain, "doubleclick.net"); // trailing dot trimmed
        assert!(batch.events[0].blocked); // Filtered* => blocked
        assert!(!batch.events[1].blocked);
        assert_eq!(batch.next_cursor.as_deref(), Some("2026-07-03T10:00:02Z"));
    }

    #[tokio::test]
    async fn cursor_keeps_only_strictly_newer_entries() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/control/login"))
            .respond_with(login_ok())
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/control/querylog"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [
                    entry("2026-07-03T10:00:03Z", "c.example.", "192.168.1.9", "NotFilteredNotFound"),
                    entry("2026-07-03T10:00:02Z", "b.example.", "192.168.1.9", "NotFilteredNotFound"),
                    entry("2026-07-03T10:00:01Z", "a.example.", "192.168.1.9", "NotFilteredNotFound"),
                ]
            })))
            .mount(&server)
            .await;

        let mut ing = AdguardIngestor::new("adguard", server.uri(), "admin", "pw");
        let batch = ing.poll(Some("2026-07-03T10:00:02Z")).await.unwrap();
        assert_eq!(batch.events.len(), 1, "only the 10:00:03 entry is new");
        assert_eq!(batch.events[0].domain, "c.example");
        assert_eq!(batch.next_cursor.as_deref(), Some("2026-07-03T10:00:03Z"));
    }

    #[tokio::test]
    async fn expired_session_relogins_once_and_retries() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/control/login"))
            .respond_with(login_ok())
            .expect(2) // initial login + re-login after the 401
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/control/querylog"))
            .respond_with(ResponseTemplate::new(401))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/control/querylog"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [entry("2026-07-03T11:00:00Z", "ok.example.", "192.168.1.9", "NotFilteredNotFound")]
            })))
            .mount(&server)
            .await;

        let mut ing = AdguardIngestor::new("adguard", server.uri(), "admin", "pw");
        let batch = ing.poll(None).await.unwrap();
        assert_eq!(batch.events.len(), 1);
    }

    #[tokio::test]
    async fn bad_credentials_is_a_clean_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/control/login"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&server)
            .await;

        let mut ing = AdguardIngestor::new("adguard", server.uri(), "admin", "wrong");
        let err = ing.poll(None).await.unwrap_err();
        assert!(err.to_string().contains("login rejected"));
    }

    #[tokio::test]
    async fn empty_log_keeps_cursor() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/control/login"))
            .respond_with(login_ok())
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/control/querylog"))
            .and(query_param("limit", "500"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({ "data": [] })),
            )
            .mount(&server)
            .await;

        let mut ing = AdguardIngestor::new("adguard", server.uri(), "admin", "pw");
        let batch = ing.poll(Some("2026-07-03T09:00:00Z")).await.unwrap();
        assert!(batch.events.is_empty());
        assert_eq!(batch.next_cursor.as_deref(), Some("2026-07-03T09:00:00Z"));
    }
}
