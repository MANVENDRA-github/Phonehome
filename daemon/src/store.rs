//! SQLite storage (ARCHITECTURE §3).
//!
//! Exactly-once contract: [`Store::apply_batch`] commits a batch's rollups, the
//! device identities it observed, and the source cursor in ONE transaction. A
//! crash therefore replays a whole batch or none of it — combined with adapter
//! cursor semantics this yields zero loss and zero duplication across restarts
//! (property-tested below).
//!
//! Privacy (D-005): raw events are never persisted — only hourly rollups.
//!
//! Device model (D-010): `query_rollups` stays keyed on `client_key` (MAC else
//! IP). The `devices` table is a semantic OVERLAY whose `identity_key` equals
//! that `client_key`; rename/merge are O(1), non-destructive edits to the
//! overlay, so re-ingestion never resurrects a merged device — ingestion only
//! ever upserts identity/last-seen, never `merged_into` or names.

use phonehome_core::{naming, oui, QueryEvent};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

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
CREATE TABLE IF NOT EXISTS devices (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    identity_key TEXT NOT NULL UNIQUE,
    is_mac       INTEGER NOT NULL,
    mac          TEXT,
    ip_hint      TEXT,
    oui_vendor   TEXT,
    name_user    TEXT,
    name_dhcp    TEXT,
    name_mdns    TEXT,
    first_seen   INTEGER NOT NULL,
    last_seen    INTEGER NOT NULL,
    merged_into  INTEGER REFERENCES devices(id)
);
INSERT OR IGNORE INTO meta (key, value) VALUES ('schema_version', '2');
";

fn store_now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// True when a `client_key` is a MAC (aa:bb:cc:dd:ee:ff) rather than an IP.
fn is_mac_key(s: &str) -> bool {
    let parts: Vec<&str> = s.split(':').collect();
    parts.len() == 6
        && parts
            .iter()
            .all(|p| p.len() == 2 && p.chars().all(|c| c.is_ascii_hexdigit()))
}

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
    pub distinct_devices: i64,
    pub rollup_rows: i64,
    pub sources: Vec<SourceState>,
}

/// A canonical device with its aggregated activity (rollups of every device
/// merged into it are folded in).
#[derive(Debug, serde::Serialize)]
pub struct DeviceRow {
    pub id: i64,
    pub display_name: String,
    pub identity_key: String,
    pub is_mac: bool,
    pub mac: Option<String>,
    pub ip_hint: Option<String>,
    pub vendor: Option<String>,
    pub name_user: Option<String>,
    pub queries: i64,
    pub blocked: i64,
    pub distinct_domains: i64,
    pub first_seen: i64,
    pub last_seen: i64,
}

/// Errors from device mutations that the API maps to 400/404.
#[derive(Debug)]
pub enum DeviceError {
    NotFound,
    BadMerge(&'static str),
    Db(rusqlite::Error),
}

impl std::fmt::Display for DeviceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeviceError::NotFound => write!(f, "device not found"),
            DeviceError::BadMerge(m) => write!(f, "{m}"),
            DeviceError::Db(e) => write!(f, "db error: {e}"),
        }
    }
}

impl From<rusqlite::Error> for DeviceError {
    fn from(e: rusqlite::Error) -> Self {
        DeviceError::Db(e)
    }
}

impl Store {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        Self::init(Connection::open(path)?)
    }

    #[cfg(test)]
    pub fn open_in_memory() -> rusqlite::Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> rusqlite::Result<Self> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.execute_batch(SCHEMA)?;
        let store = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        store.backfill_devices()?;
        Ok(store)
    }

    /// Idempotently ensures every `client_key` already in `query_rollups` has a
    /// device row. No-op on a fresh db; on a schema-v1 db (rollups but no
    /// devices) it seeds identities from history. Runs on every open — cheap,
    /// since distinct clients are few and the insert is `OR IGNORE`.
    fn backfill_devices(&self) -> rusqlite::Result<()> {
        let now = store_now_ms();
        let mut conn = self.conn.lock().expect("store mutex poisoned");
        let keys: Vec<String> = {
            let mut stmt = conn.prepare(
                "SELECT DISTINCT client_key FROM query_rollups
                 WHERE client_key NOT IN (SELECT identity_key FROM devices)",
            )?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
            rows.collect::<Result<_, _>>()?
        };
        if keys.is_empty() {
            return Ok(());
        }
        let tx = conn.transaction()?;
        for key in &keys {
            let is_mac = is_mac_key(key);
            let mac = is_mac.then(|| key.clone());
            let ip = (!is_mac).then(|| key.clone());
            upsert_device(&tx, key, mac.as_deref(), ip.as_deref(), now)?;
        }
        tx.commit()
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

    /// Applies a polled batch atomically: rollup upserts + device identities +
    /// cursor + last_ok_at, all in one transaction.
    pub fn apply_batch(
        &self,
        source_id: &str,
        kind: &str,
        events: &[QueryEvent],
        next_cursor: Option<&str>,
        now_ms: i64,
    ) -> rusqlite::Result<()> {
        // Aggregate rollups in memory: one upsert per (client, domain, hour).
        let mut agg: HashMap<(String, String, i64), (i64, i64)> = HashMap::new();
        // Distinct client identities seen this batch: key -> (mac, ip).
        let mut identities: HashMap<String, (Option<String>, String)> = HashMap::new();
        for e in events {
            let entry = agg
                .entry((e.client_key(), e.domain.clone(), e.bucket_hour()))
                .or_insert((0, 0));
            entry.0 += 1;
            if e.blocked {
                entry.1 += 1;
            }
            identities
                .entry(e.client_key())
                .or_insert_with(|| (e.client_mac.clone(), e.client_ip.to_string()));
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
            for (key, (mac, ip)) in &identities {
                upsert_device(&tx, key, mac.as_deref(), Some(ip.as_str()), now_ms)?;
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

    /// Canonical devices with folded-in activity, busiest first.
    pub fn list_devices(&self) -> rusqlite::Result<Vec<DeviceRow>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT canon.id, canon.identity_key, canon.is_mac, canon.mac, canon.ip_hint,
                    canon.oui_vendor, canon.name_user, canon.name_dhcp, canon.name_mdns,
                    canon.first_seen, canon.last_seen,
                    COALESCE(SUM(r.count), 0)         AS queries,
                    COALESCE(SUM(r.blocked_count), 0) AS blocked,
                    COUNT(DISTINCT r.domain)          AS domains
             FROM devices d
             JOIN devices canon ON canon.id = COALESCE(d.merged_into, d.id)
             LEFT JOIN query_rollups r ON r.client_key = d.identity_key
             WHERE canon.merged_into IS NULL
             GROUP BY canon.id
             ORDER BY queries DESC, canon.id ASC",
        )?;
        let rows = stmt
            .query_map([], |r| {
                let identity_key: String = r.get(1)?;
                let is_mac: bool = r.get::<_, i64>(2)? != 0;
                let mac: Option<String> = r.get(3)?;
                let ip_hint: Option<String> = r.get(4)?;
                let vendor: Option<String> = r.get(5)?;
                let name_user: Option<String> = r.get(6)?;
                let name_dhcp: Option<String> = r.get(7)?;
                let name_mdns: Option<String> = r.get(8)?;
                let display_name = naming::display_name(
                    name_user.as_deref(),
                    name_dhcp.as_deref(),
                    name_mdns.as_deref(),
                    vendor.as_deref(),
                    mac.as_deref(),
                    &identity_key,
                );
                Ok(DeviceRow {
                    id: r.get(0)?,
                    display_name,
                    identity_key,
                    is_mac,
                    mac,
                    ip_hint,
                    vendor,
                    name_user,
                    first_seen: r.get(9)?,
                    last_seen: r.get(10)?,
                    queries: r.get(11)?,
                    blocked: r.get(12)?,
                    distinct_domains: r.get(13)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Sets (or, with a blank name, clears) a device's user-assigned name.
    /// Returns false if the id doesn't exist.
    pub fn rename_device(&self, id: i64, name: &str) -> rusqlite::Result<bool> {
        let trimmed = name.trim();
        let value: Option<&str> = (!trimmed.is_empty()).then_some(trimmed);
        let conn = self.conn.lock().expect("store mutex poisoned");
        let n = conn.execute(
            "UPDATE devices SET name_user = ?1 WHERE id = ?2",
            params![value, id],
        )?;
        Ok(n > 0)
    }

    /// Merges `source` into `into`. `into` is resolved to its canonical device
    /// first (so chains collapse), and any devices already merged into `source`
    /// are redirected to that canonical. Idempotent-safe against re-ingestion:
    /// ingestion never rewrites `merged_into`.
    pub fn merge_devices(&self, source: i64, into: i64) -> Result<(), DeviceError> {
        if source == into {
            return Err(DeviceError::BadMerge("cannot merge a device into itself"));
        }
        let mut conn = self.conn.lock().expect("store mutex poisoned");
        let tx = conn.transaction()?;
        if !device_exists(&tx, source)? || !device_exists(&tx, into)? {
            return Err(DeviceError::NotFound);
        }
        let canon = resolve_canonical(&tx, into)?;
        if canon == source {
            return Err(DeviceError::BadMerge(
                "target device resolves back to the source",
            ));
        }
        // Redirect source's existing aliases, then source itself.
        tx.execute(
            "UPDATE devices SET merged_into = ?1 WHERE merged_into = ?2",
            params![canon, source],
        )?;
        tx.execute(
            "UPDATE devices SET merged_into = ?1 WHERE id = ?2",
            params![canon, source],
        )?;
        tx.commit()?;
        Ok(())
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
        let distinct_devices: i64 = conn.query_row(
            "SELECT COUNT(*) FROM devices WHERE merged_into IS NULL",
            [],
            |r| r.get(0),
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
            distinct_devices,
            rollup_rows,
            sources,
        })
    }
}

/// Inserts or refreshes a device identity. Only ever touches discovery fields
/// (last_seen, ip_hint, mac, vendor) — never `merged_into` or user names — so a
/// re-ingested client cannot resurrect a merged device or clobber a rename.
fn upsert_device(
    tx: &rusqlite::Transaction<'_>,
    identity_key: &str,
    mac: Option<&str>,
    ip: Option<&str>,
    now_ms: i64,
) -> rusqlite::Result<()> {
    let vendor = mac.and_then(oui::vendor_for_mac);
    tx.prepare_cached(
        "INSERT INTO devices
             (identity_key, is_mac, mac, ip_hint, oui_vendor, first_seen, last_seen)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
         ON CONFLICT (identity_key) DO UPDATE SET
             last_seen  = ?6,
             ip_hint    = COALESCE(?4, ip_hint),
             mac        = COALESCE(?3, mac),
             oui_vendor = COALESCE(oui_vendor, ?5)",
    )?
    .execute(params![
        identity_key,
        mac.is_some() as i64,
        mac,
        ip,
        vendor,
        now_ms,
    ])?;
    Ok(())
}

fn device_exists(tx: &rusqlite::Transaction<'_>, id: i64) -> rusqlite::Result<bool> {
    tx.query_row("SELECT 1 FROM devices WHERE id = ?1", params![id], |_| {
        Ok(())
    })
    .optional()
    .map(|o| o.is_some())
}

/// Follows `merged_into` to the canonical device id (cap guards against a cycle).
fn resolve_canonical(tx: &rusqlite::Transaction<'_>, mut id: i64) -> rusqlite::Result<i64> {
    for _ in 0..64 {
        let parent: Option<i64> = tx.query_row(
            "SELECT merged_into FROM devices WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )?;
        match parent {
            Some(p) => id = p,
            None => return Ok(id),
        }
    }
    Ok(id)
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

    fn mac_event(ts: i64, mac: &str, ip: &str, domain: &str, blocked: bool) -> QueryEvent {
        QueryEvent {
            ts,
            client_ip: ip.parse::<IpAddr>().unwrap(),
            client_mac: Some(mac.into()),
            domain: domain.into(),
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

        {
            let store = Store::open(&db).unwrap();
            for (i, chunk) in all.chunks(10).take(4).enumerate() {
                let cursor = ((i + 1) * 10).to_string();
                store
                    .apply_batch("s1", "fixture", chunk, Some(&cursor), 0)
                    .unwrap();
            }
        }

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

    #[test]
    fn devices_resolve_and_name_by_precedence() {
        let store = Store::open_in_memory().unwrap();
        let events = vec![
            mac_event(
                0,
                "f0:5c:77:11:22:33",
                "192.168.1.20",
                "samsungads.com",
                true,
            ),
            mac_event(1, "f0:5c:77:11:22:33", "192.168.1.20", "netflix.com", false),
            // MAC-less client attributes to its IP.
            event(2, 50, 3, false),
        ];
        store
            .apply_batch("s1", "fixture", &events, Some("3"), 0)
            .unwrap();

        let devices = store.list_devices().unwrap();
        assert_eq!(devices.len(), 2);
        assert_eq!(store.stats().unwrap().distinct_devices, 2);

        let tv = devices.iter().find(|d| d.is_mac).unwrap();
        assert_eq!(tv.vendor.as_deref(), Some("Samsung Electronics"));
        assert_eq!(tv.display_name, "Samsung Electronics · 22:33");
        assert_eq!(tv.queries, 2);
        assert_eq!(tv.blocked, 1);

        let ip_only = devices.iter().find(|d| !d.is_mac).unwrap();
        assert_eq!(ip_only.display_name, "192.168.1.50");
    }

    #[test]
    fn rename_takes_precedence_and_clears() {
        let store = Store::open_in_memory().unwrap();
        store
            .apply_batch(
                "s1",
                "fixture",
                &[mac_event(
                    0,
                    "f0:5c:77:11:22:33",
                    "192.168.1.20",
                    "a.com",
                    false,
                )],
                Some("1"),
                0,
            )
            .unwrap();
        let id = store.list_devices().unwrap()[0].id;

        assert!(store.rename_device(id, "Living Room TV").unwrap());
        assert_eq!(
            store.list_devices().unwrap()[0].display_name,
            "Living Room TV"
        );

        // Blank clears back to the vendor tier.
        assert!(store.rename_device(id, "   ").unwrap());
        assert_eq!(
            store.list_devices().unwrap()[0].display_name,
            "Samsung Electronics · 22:33"
        );
        assert!(!store.rename_device(9999, "ghost").unwrap());
    }

    #[test]
    fn merge_folds_activity_and_survives_reingestion() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("m.db");
        // Two devices, one event each.
        let batch = vec![
            mac_event(0, "f0:5c:77:11:22:33", "192.168.1.20", "a.com", true),
            mac_event(0, "f4:0f:24:40:50:60", "192.168.1.31", "b.com", false),
        ];

        let store = Store::open(&db).unwrap();
        store
            .apply_batch("s1", "fixture", &batch, Some("1"), 0)
            .unwrap();
        let devices = store.list_devices().unwrap();
        assert_eq!(devices.len(), 2);
        let (a, b) = (devices[0].id, devices[1].id);

        // Merge b into a: one canonical device, both queries folded in.
        store.merge_devices(b, a).unwrap();
        let after = store.list_devices().unwrap();
        assert_eq!(after.len(), 1, "merged view shows one device");
        assert_eq!(after[0].queries, 2);
        assert_eq!(after[0].distinct_domains, 2);
        assert_eq!(store.stats().unwrap().distinct_devices, 1);

        // Re-ingest the SAME clients (simulating a fresh poll) — the merged
        // device must NOT resurrect, even across a store reopen.
        drop(store);
        let store = Store::open(&db).unwrap();
        store
            .apply_batch("s1", "fixture", &batch, Some("2"), 0)
            .unwrap();
        let reopened = store.list_devices().unwrap();
        assert_eq!(
            reopened.len(),
            1,
            "re-ingestion did not resurrect the merge"
        );
        assert_eq!(store.stats().unwrap().distinct_devices, 1);
    }

    #[test]
    fn merge_rejects_self_and_missing() {
        let store = Store::open_in_memory().unwrap();
        store
            .apply_batch(
                "s1",
                "fixture",
                &[mac_event(
                    0,
                    "f0:5c:77:11:22:33",
                    "192.168.1.20",
                    "a.com",
                    false,
                )],
                Some("1"),
                0,
            )
            .unwrap();
        let id = store.list_devices().unwrap()[0].id;
        assert!(matches!(
            store.merge_devices(id, id),
            Err(DeviceError::BadMerge(_))
        ));
        assert!(matches!(
            store.merge_devices(id, 9999),
            Err(DeviceError::NotFound)
        ));
    }

    #[test]
    fn backfill_seeds_devices_from_v1_rollups() {
        // Simulate a schema-v1 db: rollups present, devices table empty.
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("v1.db");
        {
            let conn = Connection::open(&db).unwrap();
            conn.execute_batch(SCHEMA).unwrap();
            conn.execute(
                "INSERT INTO query_rollups
                    (source_id, client_key, domain, bucket_hour, count, blocked_count)
                 VALUES ('s1', 'f0:5c:77:11:22:33', 'a.com', 1, 5, 2),
                        ('s1', '192.168.1.50', 'b.com', 1, 3, 0)",
                [],
            )
            .unwrap();
            conn.execute("DELETE FROM devices", []).unwrap();
        }
        // Opening runs backfill_devices.
        let store = Store::open(&db).unwrap();
        let devices = store.list_devices().unwrap();
        assert_eq!(devices.len(), 2);
        let tv = devices.iter().find(|d| d.is_mac).unwrap();
        assert_eq!(tv.vendor.as_deref(), Some("Samsung Electronics"));
        assert_eq!(tv.queries, 5);
    }

    proptest! {
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
            prop_assert_eq!(stats.distinct_devices, 4);
            prop_assert_eq!(store.cursor("s1").unwrap().unwrap(), all.len().to_string());
        }
    }
}
