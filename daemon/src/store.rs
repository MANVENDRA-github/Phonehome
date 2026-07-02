//! SQLite storage (ARCHITECTURE §3, minimal M1 form).
//!
//! Exactly-once contract: [`Store::apply_batch`] commits a batch's rollups and
//! the source cursor in ONE transaction. A crash therefore replays a whole
//! batch or none of it — combined with adapter cursor semantics this yields
//! zero loss and zero duplication across restarts (property-tested below).
//!
//! Privacy (D-005): raw events are never persisted — only hourly rollups.
//! M1 note: rollups key on `client_key` (MAC, else IP); M2 replaces this with
//! the device registry's `device_id`.

use phonehome_core::QueryEvent;
use rusqlite::{params, Connection};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS sources (
    id         TEXT PRIMARY KEY,
    kind       TEXT NOT NULL,
    cursor     TEXT,
    last_ok_at INTEGER
);
CREATE TABLE IF NOT EXISTS query_rollups (
    source_id     TEXT NOT NULL,
    client_key    TEXT NOT NULL,
    domain        TEXT NOT NULL,
    bucket_hour   INTEGER NOT NULL,
    count         INTEGER NOT NULL DEFAULT 0,
    blocked_count INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (source_id, client_key, domain, bucket_hour)
);
INSERT OR IGNORE INTO meta (key, value) VALUES ('schema_version', '1');
";

/// Cheap-to-clone handle. rusqlite connections aren't `Sync`, so callers in
/// async context run store calls inside `tokio::task::spawn_blocking`.
#[derive(Clone)]
pub struct Store {
    conn: Arc<Mutex<Connection>>,
}

#[derive(Debug, serde::Serialize)]
pub struct SourceState {
    pub id: String,
    pub kind: String,
    pub cursor: Option<String>,
    pub last_ok_at: Option<i64>,
}

#[derive(Debug, serde::Serialize)]
pub struct Stats {
    pub total_queries: i64,
    pub total_blocked: i64,
    pub distinct_domains: i64,
    pub distinct_clients: i64,
    pub rollup_rows: i64,
    pub sources: Vec<SourceState>,
}

impl Store {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        Self::init(conn)
    }

    #[cfg(test)]
    pub fn open_in_memory() -> rusqlite::Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> rusqlite::Result<Self> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub fn cursor(&self, source_id: &str) -> rusqlite::Result<Option<String>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn.prepare_cached("SELECT cursor FROM sources WHERE id = ?1")?;
        let mut rows = stmt.query(params![source_id])?;
        match rows.next()? {
            Some(row) => row.get(0),
            None => Ok(None),
        }
    }

    /// Applies a polled batch atomically: rollup upserts + cursor + last_ok_at
    /// in one transaction (the exactly-once half owned by the store).
    pub fn apply_batch(
        &self,
        source_id: &str,
        kind: &str,
        events: &[QueryEvent],
        next_cursor: Option<&str>,
        now_ms: i64,
    ) -> rusqlite::Result<()> {
        // Aggregate in memory first: one upsert per (client, domain, hour).
        let mut agg: HashMap<(String, String, i64), (i64, i64)> = HashMap::new();
        for e in events {
            let entry = agg
                .entry((e.client_key(), e.domain.clone(), e.bucket_hour()))
                .or_insert((0, 0));
            entry.0 += 1;
            if e.blocked {
                entry.1 += 1;
            }
        }

        let mut conn = self.conn.lock().expect("store mutex poisoned");
        let tx = conn.transaction()?;
        {
            let mut upsert = tx.prepare_cached(
                "INSERT INTO query_rollups
                     (source_id, client_key, domain, bucket_hour, count, blocked_count)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT (source_id, client_key, domain, bucket_hour)
                 DO UPDATE SET count = count + excluded.count,
                               blocked_count = blocked_count + excluded.blocked_count",
            )?;
            for ((client_key, domain, bucket), (n, blocked_n)) in &agg {
                upsert.execute(params![source_id, client_key, domain, bucket, n, blocked_n])?;
            }
            tx.prepare_cached(
                "INSERT INTO sources (id, kind, cursor, last_ok_at)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (id)
                 DO UPDATE SET kind = excluded.kind,
                               cursor = excluded.cursor,
                               last_ok_at = excluded.last_ok_at",
            )?
            .execute(params![source_id, kind, next_cursor, now_ms])?;
        }
        tx.commit()
    }

    pub fn stats(&self) -> rusqlite::Result<Stats> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let (total_queries, total_blocked, distinct_domains, distinct_clients, rollup_rows) = conn
            .query_row(
                "SELECT COALESCE(SUM(count), 0),
                        COALESCE(SUM(blocked_count), 0),
                        COUNT(DISTINCT domain),
                        COUNT(DISTINCT client_key),
                        COUNT(*)
                 FROM query_rollups",
                [],
                |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, i64>(2)?,
                        r.get::<_, i64>(3)?,
                        r.get::<_, i64>(4)?,
                    ))
                },
            )?;
        let mut stmt =
            conn.prepare_cached("SELECT id, kind, cursor, last_ok_at FROM sources ORDER BY id")?;
        let sources = stmt
            .query_map([], |r| {
                Ok(SourceState {
                    id: r.get(0)?,
                    kind: r.get(1)?,
                    cursor: r.get(2)?,
                    last_ok_at: r.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Stats {
            total_queries,
            total_blocked,
            distinct_domains,
            distinct_clients,
            rollup_rows,
            sources,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use std::net::IpAddr;

    fn event(ts: i64, client: u8, domain_id: u8, blocked: bool) -> QueryEvent {
        QueryEvent {
            ts,
            client_ip: format!("192.168.1.{client}").parse::<IpAddr>().unwrap(),
            client_mac: None,
            domain: format!("d{domain_id}.example"),
            qtype: "A".into(),
            blocked,
            source: "test".into(),
        }
    }

    #[test]
    fn apply_batch_is_atomic_with_cursor() {
        let store = Store::open_in_memory().unwrap();
        let events = vec![event(0, 1, 1, true), event(1, 1, 1, false)];
        store
            .apply_batch("s1", "fixture", &events, Some("2"), 42)
            .unwrap();

        assert_eq!(store.cursor("s1").unwrap().as_deref(), Some("2"));
        let stats = store.stats().unwrap();
        assert_eq!(stats.total_queries, 2);
        assert_eq!(stats.total_blocked, 1);
        assert_eq!(stats.rollup_rows, 1, "same client/domain/hour rolls up");
    }

    #[test]
    fn restart_resumes_from_persisted_cursor_without_loss_or_dup() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("t.db");
        let all: Vec<QueryEvent> = (0..100)
            .map(|i| event(i * 60_000, (i % 5) as u8, (i % 7) as u8, i % 3 == 0))
            .collect();

        // First run: apply 4 of 10 batches, then "crash" (drop the store).
        {
            let store = Store::open(&db).unwrap();
            for (i, chunk) in all.chunks(10).take(4).enumerate() {
                let cursor = ((i + 1) * 10).to_string();
                store
                    .apply_batch("s1", "fixture", chunk, Some(&cursor), 0)
                    .unwrap();
            }
        }

        // Second run: resume exactly where the cursor says.
        let store = Store::open(&db).unwrap();
        let resume: usize = store.cursor("s1").unwrap().unwrap().parse().unwrap();
        assert_eq!(resume, 40);
        for (i, chunk) in all[resume..].chunks(10).enumerate() {
            let cursor = (resume + (i + 1) * 10).to_string();
            store
                .apply_batch("s1", "fixture", chunk, Some(&cursor), 0)
                .unwrap();
        }

        let stats = store.stats().unwrap();
        assert_eq!(stats.total_queries, 100, "zero loss, zero duplication");
        assert_eq!(
            stats.total_blocked,
            all.iter().filter(|e| e.blocked).count() as i64
        );
    }

    proptest! {
        /// Dedup/cursor invariant (SPEC M1): however a stream is split into
        /// batches, the final rollup totals equal a single-shot aggregation.
        #[test]
        fn rollups_are_invariant_under_batch_splitting(
            splits in prop::collection::vec(1usize..20, 1..10),
            blocked_mask in prop::collection::vec(any::<bool>(), 200),
        ) {
            let all: Vec<QueryEvent> = blocked_mask
                .iter()
                .enumerate()
                .map(|(i, &b)| event((i as i64) * 30_000, (i % 4) as u8, (i % 6) as u8, b))
                .collect();

            let store = Store::open_in_memory().unwrap();
            let mut pos = 0usize;
            let mut split_iter = splits.iter().cycle();
            while pos < all.len() {
                let take = (*split_iter.next().unwrap()).min(all.len() - pos);
                let chunk = &all[pos..pos + take];
                pos += take;
                store
                    .apply_batch("s1", "fixture", chunk, Some(&pos.to_string()), 0)
                    .unwrap();
            }

            let stats = store.stats().unwrap();
            prop_assert_eq!(stats.total_queries, all.len() as i64);
            prop_assert_eq!(
                stats.total_blocked,
                all.iter().filter(|e| e.blocked).count() as i64
            );
            prop_assert_eq!(store.cursor("s1").unwrap().unwrap(), all.len().to_string());
        }
    }
}
