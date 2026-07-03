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

use phonehome_core::score::{ScoreInputs, ScoreWeights, Scorecard};
use phonehome_core::{enrich, naming, oui, score, QueryEvent};
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
CREATE TABLE IF NOT EXISTS destinations (
    domain       TEXT PRIMARY KEY,
    entity       TEXT,
    category     TEXT NOT NULL,
    country      TEXT,
    is_tracker   INTEGER NOT NULL,
    on_blocklist INTEGER NOT NULL,
    enriched_at  INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS snapshots (
    device_id          INTEGER NOT NULL,
    week_start         INTEGER NOT NULL,
    distinct_domains   INTEGER NOT NULL,
    tracker_domains    INTEGER NOT NULL,
    distinct_entities  INTEGER NOT NULL,
    distinct_countries INTEGER NOT NULL,
    volume             INTEGER NOT NULL,
    blocked            INTEGER NOT NULL,
    score              INTEGER NOT NULL,
    computed_at        INTEGER NOT NULL,
    PRIMARY KEY (device_id, week_start)
);
INSERT OR IGNORE INTO meta (key, value) VALUES ('schema_version', '3');
UPDATE meta SET value = '3' WHERE key = 'schema_version';
";

/// Millis per week; snapshot buckets align to whole weeks from the unix epoch.
const WEEK_MS: i64 = 7 * 24 * 3_600_000;

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
    pub tracker_queries: i64,
    pub distinct_domains: i64,
    pub first_seen: i64,
    pub last_seen: i64,
}

/// A device's scorecard plus the device it describes.
#[derive(Debug, serde::Serialize)]
pub struct DeviceScorecard {
    pub device_id: i64,
    pub display_name: String,
    #[serde(flatten)]
    pub card: Scorecard,
}

/// One aggregated arc: a canonical device's traffic into one destination
/// country (M4 globe). Only country-mapped destinations appear as arcs;
/// unmapped traffic is disclosed via [`ArcsResponse::unmapped_queries`].
#[derive(Debug, serde::Serialize)]
pub struct ArcRow {
    pub device_id: i64,
    pub device_name: String,
    pub country: String,
    pub queries: i64,
    pub blocked: i64,
    pub tracker_queries: i64,
    pub domains: i64,
}

#[derive(Debug, serde::Serialize)]
pub struct ArcsResponse {
    pub arcs: Vec<ArcRow>,
    /// Queries to destinations with no mapped country — shown, not hidden
    /// (partial-visibility honesty, D-001/D-011).
    pub unmapped_queries: i64,
}

/// One destination domain behind an arc (click-through level 1).
#[derive(Debug, serde::Serialize)]
pub struct ArcDomainRow {
    pub domain: String,
    pub entity: Option<String>,
    pub category: String,
    pub is_tracker: bool,
    pub queries: i64,
    pub blocked: i64,
    pub last_bucket_hour: i64,
}

/// A raw hourly rollup bucket (click-through level 2 — the rawest data kept,
/// D-005).
#[derive(Debug, serde::Serialize)]
pub struct RollupRow {
    pub bucket_hour: i64,
    pub count: i64,
    pub blocked_count: i64,
}

/// A live-ingest hint emitted per (device, domain) after a batch commits;
/// feeds `/api/stream` (SSE). Best-effort: slow subscribers may drop pulses.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Pulse {
    pub device_id: i64,
    pub device_name: String,
    pub domain: String,
    pub country: Option<String>,
    pub is_tracker: bool,
    pub count: i64,
}

/// A persisted weekly snapshot row (feeds the M6 week-over-week diff).
#[derive(Debug, serde::Serialize)]
pub struct Snapshot {
    pub device_id: i64,
    pub week_start: i64,
    pub distinct_domains: i64,
    pub tracker_domains: i64,
    pub distinct_entities: i64,
    pub distinct_countries: i64,
    pub volume: i64,
    pub blocked: i64,
    pub score: i64,
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
    /// cursor + last_ok_at, all in one transaction. Returns one [`Pulse`] per
    /// distinct (device, domain) in the batch for live SSE fan-out — derived
    /// inside the same transaction so device names and enrichment are the ones
    /// the batch actually committed.
    pub fn apply_batch(
        &self,
        source_id: &str,
        kind: &str,
        events: &[QueryEvent],
        next_cursor: Option<&str>,
        now_ms: i64,
    ) -> rusqlite::Result<Vec<Pulse>> {
        // Aggregate rollups in memory: one upsert per (client, domain, hour).
        let mut agg: HashMap<(String, String, i64), (i64, i64)> = HashMap::new();
        // Distinct client identities seen this batch: key -> (mac, ip).
        let mut identities: HashMap<String, (Option<String>, String)> = HashMap::new();
        // Distinct domains seen this batch, for destination enrichment.
        let mut domains: std::collections::HashSet<String> = std::collections::HashSet::new();
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
            domains.insert(e.domain.clone());
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
            for domain in &domains {
                upsert_destination(&tx, domain, now_ms)?;
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

        // Fold per-hour rollups down to per (client, domain) pulse counts.
        let mut pulse_counts: HashMap<(String, String), i64> = HashMap::new();
        for ((client_key, domain, _bucket), (n, _)) in &agg {
            *pulse_counts
                .entry((client_key.clone(), domain.clone()))
                .or_insert(0) += n;
        }
        // Canonical device per client_key, resolved once each (post-upsert, so
        // every key exists; merges are honored via the canonical walk).
        let mut device_cache: HashMap<String, (i64, String)> = HashMap::new();
        let mut pulses = Vec::with_capacity(pulse_counts.len());
        for ((client_key, domain), count) in pulse_counts {
            if !device_cache.contains_key(&client_key) {
                let id: i64 = tx.query_row(
                    "SELECT id FROM devices WHERE identity_key = ?1",
                    params![client_key],
                    |r| r.get(0),
                )?;
                let canon = resolve_canonical(&tx, id)?;
                let name = tx.query_row(
                    "SELECT identity_key, oui_vendor, name_user, name_dhcp, name_mdns, mac
                     FROM devices WHERE id = ?1",
                    params![canon],
                    |r| {
                        let identity_key: String = r.get(0)?;
                        let vendor: Option<String> = r.get(1)?;
                        let name_user: Option<String> = r.get(2)?;
                        let name_dhcp: Option<String> = r.get(3)?;
                        let name_mdns: Option<String> = r.get(4)?;
                        let mac: Option<String> = r.get(5)?;
                        Ok(naming::display_name(
                            name_user.as_deref(),
                            name_dhcp.as_deref(),
                            name_mdns.as_deref(),
                            vendor.as_deref(),
                            mac.as_deref(),
                            &identity_key,
                        ))
                    },
                )?;
                device_cache.insert(client_key.clone(), (canon, name));
            }
            let (device_id, device_name) = device_cache[&client_key].clone();
            // Same pure enrichment upsert_destination just committed.
            let e = enrich::enrich(&domain);
            pulses.push(Pulse {
                device_id,
                device_name,
                domain,
                country: e.country,
                is_tracker: e.is_tracker,
                count,
            });
        }

        tx.commit()?;
        Ok(pulses)
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
                    COALESCE(SUM(CASE WHEN dest.is_tracker = 1 THEN r.count ELSE 0 END), 0)
                                                      AS tracker_queries,
                    COUNT(DISTINCT r.domain)          AS domains
             FROM devices d
             JOIN devices canon ON canon.id = COALESCE(d.merged_into, d.id)
             LEFT JOIN query_rollups r ON r.client_key = d.identity_key
             LEFT JOIN destinations dest ON dest.domain = r.domain
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
                    tracker_queries: r.get(13)?,
                    distinct_domains: r.get(14)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Device→country arcs for the globe: canonical devices' activity grouped
    /// by destination country, busiest first, over all data (`window = None`)
    /// or a `[start, end)` millisecond window. Destinations without a mapped
    /// country are excluded from arcs but disclosed via `unmapped_queries`.
    pub fn arcs(&self, window: Option<(i64, i64)>) -> rusqlite::Result<ArcsResponse> {
        let (where_window, wp) = window_clause(window, 1);
        let conn = self.conn.lock().expect("store mutex poisoned");
        let sql = format!(
            "SELECT canon.id, canon.identity_key, canon.oui_vendor, canon.name_user,
                    canon.name_dhcp, canon.name_mdns, canon.mac, dest.country,
                    SUM(r.count)         AS queries,
                    SUM(r.blocked_count) AS blocked,
                    SUM(CASE WHEN dest.is_tracker = 1 THEN r.count ELSE 0 END)
                                         AS tracker_queries,
                    COUNT(DISTINCT r.domain) AS domains
             FROM devices d
             JOIN devices canon ON canon.id = COALESCE(d.merged_into, d.id)
             JOIN query_rollups r ON r.client_key = d.identity_key
             JOIN destinations dest ON dest.domain = r.domain
             WHERE canon.merged_into IS NULL AND dest.country IS NOT NULL
             {where_window}
             GROUP BY canon.id, dest.country
             ORDER BY queries DESC, canon.id ASC, dest.country ASC"
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut params: Vec<&dyn rusqlite::ToSql> = Vec::new();
        for p in &wp {
            params.push(p);
        }
        let arcs = stmt
            .query_map(params.as_slice(), |r| {
                let identity_key: String = r.get(1)?;
                let vendor: Option<String> = r.get(2)?;
                let name_user: Option<String> = r.get(3)?;
                let name_dhcp: Option<String> = r.get(4)?;
                let name_mdns: Option<String> = r.get(5)?;
                let mac: Option<String> = r.get(6)?;
                Ok(ArcRow {
                    device_id: r.get(0)?,
                    device_name: naming::display_name(
                        name_user.as_deref(),
                        name_dhcp.as_deref(),
                        name_mdns.as_deref(),
                        vendor.as_deref(),
                        mac.as_deref(),
                        &identity_key,
                    ),
                    country: r.get(7)?,
                    queries: r.get(8)?,
                    blocked: r.get(9)?,
                    tracker_queries: r.get(10)?,
                    domains: r.get(11)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        let unmapped_sql = format!(
            "SELECT COALESCE(SUM(r.count), 0)
             FROM query_rollups r
             LEFT JOIN destinations dest ON dest.domain = r.domain
             WHERE dest.country IS NULL
             {where_window}"
        );
        let unmapped_queries: i64 =
            conn.query_row(&unmapped_sql, params.as_slice(), |r| r.get(0))?;
        Ok(ArcsResponse {
            arcs,
            unmapped_queries,
        })
    }

    /// Click-through level 1: the domains behind one device→country arc,
    /// busiest first, folding in merged devices.
    pub fn arc_domains(
        &self,
        device_id: i64,
        country: &str,
        window: Option<(i64, i64)>,
    ) -> rusqlite::Result<Vec<ArcDomainRow>> {
        let (where_window, wp) = window_clause(window, 3);
        let conn = self.conn.lock().expect("store mutex poisoned");
        let sql = format!(
            "SELECT r.domain, dest.entity, dest.category, dest.is_tracker,
                    SUM(r.count), SUM(r.blocked_count), MAX(r.bucket_hour)
             FROM query_rollups r
             JOIN destinations dest ON dest.domain = r.domain
             WHERE dest.country = ?2
               AND r.client_key IN
                   (SELECT identity_key FROM devices WHERE COALESCE(merged_into, id) = ?1)
             {where_window}
             GROUP BY r.domain
             ORDER BY SUM(r.count) DESC, r.domain ASC"
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut params: Vec<&dyn rusqlite::ToSql> = vec![&device_id, &country];
        for p in &wp {
            params.push(p);
        }
        let rows = stmt
            .query_map(params.as_slice(), |r| {
                Ok(ArcDomainRow {
                    domain: r.get(0)?,
                    entity: r.get(1)?,
                    category: r.get(2)?,
                    is_tracker: r.get::<_, i64>(3)? != 0,
                    queries: r.get(4)?,
                    blocked: r.get(5)?,
                    last_bucket_hour: r.get(6)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Click-through level 2: a device's raw hourly rollup buckets for one
    /// domain — the rawest data retained (D-005), folding in merged devices.
    pub fn domain_rollups(
        &self,
        device_id: i64,
        domain: &str,
        window: Option<(i64, i64)>,
    ) -> rusqlite::Result<Vec<RollupRow>> {
        let (where_window, wp) = window_clause(window, 3);
        let conn = self.conn.lock().expect("store mutex poisoned");
        let sql = format!(
            "SELECT r.bucket_hour, SUM(r.count), SUM(r.blocked_count)
             FROM query_rollups r
             WHERE r.domain = ?2
               AND r.client_key IN
                   (SELECT identity_key FROM devices WHERE COALESCE(merged_into, id) = ?1)
             {where_window}
             GROUP BY r.bucket_hour
             ORDER BY r.bucket_hour ASC"
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut params: Vec<&dyn rusqlite::ToSql> = vec![&device_id, &domain];
        for p in &wp {
            params.push(p);
        }
        let rows = stmt
            .query_map(params.as_slice(), |r| {
                Ok(RollupRow {
                    bucket_hour: r.get(0)?,
                    count: r.get(1)?,
                    blocked_count: r.get(2)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Computes a device's live privacy scorecard over all available data,
    /// folding in any devices merged into it. `None` if the id isn't a
    /// canonical device.
    pub fn device_scorecard(&self, id: i64) -> rusqlite::Result<Option<DeviceScorecard>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let display_name: Option<String> = conn
            .query_row(
                "SELECT identity_key, oui_vendor, name_user, name_dhcp, name_mdns
                 FROM devices WHERE id = ?1 AND merged_into IS NULL",
                params![id],
                |r| {
                    let identity_key: String = r.get(0)?;
                    let vendor: Option<String> = r.get(1)?;
                    let name_user: Option<String> = r.get(2)?;
                    let name_dhcp: Option<String> = r.get(3)?;
                    let name_mdns: Option<String> = r.get(4)?;
                    Ok(naming::display_name(
                        name_user.as_deref(),
                        name_dhcp.as_deref(),
                        name_mdns.as_deref(),
                        vendor.as_deref(),
                        None,
                        &identity_key,
                    ))
                },
            )
            .optional()?;
        let Some(display_name) = display_name else {
            return Ok(None);
        };

        let agg = aggregate(&conn, id, None)?;
        let card = score::score(agg.inputs, ScoreWeights::default());
        Ok(Some(DeviceScorecard {
            device_id: id,
            display_name,
            card,
        }))
    }

    /// Recomputes and upserts a weekly snapshot per canonical device for every
    /// week present in the data. Idempotent — safe to run on a schedule.
    pub fn snapshot_all_weeks(&self, now_ms: i64) -> rusqlite::Result<usize> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let device_ids: Vec<i64> = {
            let mut stmt =
                conn.prepare("SELECT id FROM devices WHERE merged_into IS NULL ORDER BY id")?;
            let rows = stmt.query_map([], |r| r.get::<_, i64>(0))?;
            rows.collect::<Result<_, _>>()?
        };
        let weeks: Vec<i64> = {
            let mut stmt = conn
                .prepare("SELECT DISTINCT (bucket_hour * 3600000) / ?1 * ?1 FROM query_rollups")?;
            let rows = stmt.query_map(params![WEEK_MS], |r| r.get::<_, i64>(0))?;
            rows.collect::<Result<_, _>>()?
        };

        let mut written = 0usize;
        for &device_id in &device_ids {
            for &week_start in &weeks {
                let week_end = week_start + WEEK_MS;
                let agg = aggregate(&conn, device_id, Some((week_start, week_end)))?;
                if agg.inputs.total_queries == 0 {
                    continue;
                }
                let card = score::score(agg.inputs, ScoreWeights::default());
                conn.execute(
                    "INSERT INTO snapshots
                        (device_id, week_start, distinct_domains, tracker_domains,
                         distinct_entities, distinct_countries, volume, blocked, score, computed_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                     ON CONFLICT (device_id, week_start) DO UPDATE SET
                        distinct_domains   = excluded.distinct_domains,
                        tracker_domains    = excluded.tracker_domains,
                        distinct_entities  = excluded.distinct_entities,
                        distinct_countries = excluded.distinct_countries,
                        volume             = excluded.volume,
                        blocked            = excluded.blocked,
                        score              = excluded.score,
                        computed_at        = excluded.computed_at",
                    params![
                        device_id,
                        week_start,
                        agg.distinct_domains,
                        agg.tracker_domains,
                        agg.inputs.distinct_tracker_entities,
                        agg.distinct_countries,
                        agg.inputs.total_queries,
                        agg.inputs.blocked_queries,
                        card.score as i64,
                        now_ms,
                    ],
                )?;
                written += 1;
            }
        }
        Ok(written)
    }

    /// All persisted snapshots, newest week first.
    pub fn list_snapshots(&self) -> rusqlite::Result<Vec<Snapshot>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT device_id, week_start, distinct_domains, tracker_domains,
                    distinct_entities, distinct_countries, volume, blocked, score
             FROM snapshots ORDER BY week_start DESC, device_id ASC",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok(Snapshot {
                    device_id: r.get(0)?,
                    week_start: r.get(1)?,
                    distinct_domains: r.get(2)?,
                    tracker_domains: r.get(3)?,
                    distinct_entities: r.get(4)?,
                    distinct_countries: r.get(5)?,
                    volume: r.get(6)?,
                    blocked: r.get(7)?,
                    score: r.get(8)?,
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

/// Enriches a domain (pure, offline — no network, D-005 stays intact) and
/// upserts it into `destinations`. Re-runs refresh the enrichment (cheap; the
/// seeds may change between releases).
fn upsert_destination(
    tx: &rusqlite::Transaction<'_>,
    domain: &str,
    now_ms: i64,
) -> rusqlite::Result<()> {
    let e = enrich::enrich(domain);
    let category = serde_json::to_value(e.category)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".into());
    tx.prepare_cached(
        "INSERT INTO destinations
             (domain, entity, category, country, is_tracker, on_blocklist, enriched_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT (domain) DO UPDATE SET
             entity = excluded.entity, category = excluded.category,
             country = excluded.country, is_tracker = excluded.is_tracker,
             on_blocklist = excluded.on_blocklist, enriched_at = excluded.enriched_at",
    )?
    .execute(params![
        domain,
        e.entity,
        category,
        e.country,
        e.is_tracker as i64,
        e.on_blocklist as i64,
        now_ms,
    ])?;
    Ok(())
}

/// Optional `[start, end)` millisecond window predicate on rollup buckets.
/// `first_param` is the 1-based placeholder index of `start` in the final SQL.
fn window_clause(window: Option<(i64, i64)>, first_param: usize) -> (String, Vec<i64>) {
    match window {
        Some((start, end)) => (
            format!(
                "AND r.bucket_hour * 3600000 >= ?{first_param} \
                 AND r.bucket_hour * 3600000 < ?{}",
                first_param + 1
            ),
            vec![start, end],
        ),
        None => (String::new(), vec![]),
    }
}

/// Per-device aggregates over all data (`window = None`) or a `[start, end)`
/// millisecond window, folding in every device merged into `device_id`.
struct Aggregate {
    inputs: ScoreInputs,
    distinct_domains: i64,
    tracker_domains: i64,
    distinct_countries: i64,
}

fn aggregate(
    conn: &Connection,
    device_id: i64,
    window: Option<(i64, i64)>,
) -> rusqlite::Result<Aggregate> {
    let (where_window, wp) = window_clause(window, 2);
    let sql = format!(
        "SELECT COALESCE(SUM(r.count), 0),
                COALESCE(SUM(r.blocked_count), 0),
                COALESCE(SUM(CASE WHEN dest.is_tracker = 1 THEN r.count ELSE 0 END), 0),
                COUNT(DISTINCT CASE WHEN dest.is_tracker = 1 THEN dest.entity END),
                COUNT(DISTINCT r.domain),
                COUNT(DISTINCT CASE WHEN dest.is_tracker = 1 THEN r.domain END),
                COUNT(DISTINCT dest.country)
         FROM query_rollups r
         LEFT JOIN destinations dest ON dest.domain = r.domain
         WHERE r.client_key IN
             (SELECT identity_key FROM devices WHERE COALESCE(merged_into, id) = ?1)
         {where_window}"
    );
    let mut params: Vec<&dyn rusqlite::ToSql> = vec![&device_id];
    for p in &wp {
        params.push(p);
    }
    conn.query_row(&sql, params.as_slice(), |r| {
        Ok(Aggregate {
            inputs: ScoreInputs {
                total_queries: r.get(0)?,
                blocked_queries: r.get(1)?,
                tracker_queries: r.get(2)?,
                distinct_tracker_entities: r.get(3)?,
                distinct_countries: r.get(6)?,
            },
            distinct_domains: r.get(4)?,
            tracker_domains: r.get(5)?,
            distinct_countries: r.get(6)?,
        })
    })
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

    // --- M3 enrichment + scorecard ---

    #[test]
    fn enrichment_populates_destinations_and_tracker_queries() {
        let store = Store::open_in_memory().unwrap();
        store
            .apply_batch(
                "s1",
                "fixture",
                &[
                    mac_event(
                        0,
                        "f0:5c:77:11:22:33",
                        "192.168.1.20",
                        "samsungads.com",
                        true,
                    ),
                    mac_event(
                        1,
                        "f0:5c:77:11:22:33",
                        "192.168.1.20",
                        "api.github.com",
                        false,
                    ),
                ],
                Some("2"),
                0,
            )
            .unwrap();

        // destinations enriched.
        let conn = store.conn.lock().unwrap();
        let (cat, is_tracker, country): (String, i64, Option<String>) = conn
            .query_row(
                "SELECT category, is_tracker, country FROM destinations WHERE domain='samsungads.com'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(cat, "advertising");
        assert_eq!(is_tracker, 1);
        assert_eq!(country.as_deref(), Some("KR"));
        drop(conn);

        // tracker_queries counts only the tracker domain.
        let d = &store.list_devices().unwrap()[0];
        assert_eq!(d.queries, 2);
        assert_eq!(d.tracker_queries, 1);
    }

    #[test]
    fn scorecard_ranks_tracker_heavy_above_quiet() {
        let store = Store::open_in_memory().unwrap();
        let mut events = Vec::new();
        // A tracker magnet: many queries to ad/analytics across countries.
        let magnet = "a8:51:ab:10:20:30";
        for (i, dom) in [
            "doubleclick.net",
            "app-measurement.com",
            "graph.facebook.com",
            "analytics.tuya.com",
        ]
        .iter()
        .cycle()
        .take(80)
        .enumerate()
        {
            events.push(mac_event(
                i as i64 * 1000,
                magnet,
                "192.168.1.30",
                dom,
                i % 3 == 0,
            ));
        }
        // A quiet device: only first-party GitHub.
        let quiet = "dc:41:a9:70:80:90";
        for i in 0..80 {
            events.push(mac_event(
                i * 1000,
                quiet,
                "192.168.1.32",
                "api.github.com",
                false,
            ));
        }
        store
            .apply_batch("s1", "fixture", &events, Some("1"), 0)
            .unwrap();

        let devices = store.list_devices().unwrap();
        let magnet_id = devices
            .iter()
            .find(|d| d.identity_key == magnet)
            .unwrap()
            .id;
        let quiet_id = devices.iter().find(|d| d.identity_key == quiet).unwrap().id;

        let magnet_card = store.device_scorecard(magnet_id).unwrap().unwrap();
        let quiet_card = store.device_scorecard(quiet_id).unwrap().unwrap();
        assert!(
            magnet_card.card.score > quiet_card.card.score,
            "magnet {} should outscore quiet {}",
            magnet_card.card.score,
            quiet_card.card.score
        );
        // Inputs are visible and sane.
        assert_eq!(quiet_card.card.inputs.tracker_queries, 0);
        assert!(magnet_card.card.inputs.tracker_queries > 0);
        assert!(magnet_card.card.inputs.distinct_countries >= 2);
    }

    #[test]
    fn scorecard_none_for_missing_or_merged_device() {
        let store = Store::open_in_memory().unwrap();
        assert!(store.device_scorecard(9999).unwrap().is_none());
    }

    #[test]
    fn snapshots_are_idempotent() {
        let store = Store::open_in_memory().unwrap();
        let evs: Vec<QueryEvent> = (0..50)
            .map(|i| {
                mac_event(
                    i * 3_600_000,
                    "a8:51:ab:10:20:30",
                    "192.168.1.30",
                    if i % 2 == 0 {
                        "doubleclick.net"
                    } else {
                        "api.github.com"
                    },
                    i % 4 == 0,
                )
            })
            .collect();
        store
            .apply_batch("s1", "fixture", &evs, Some("50"), 0)
            .unwrap();

        let first = store.snapshot_all_weeks(0).unwrap();
        let rows1 = store.list_snapshots().unwrap();
        assert!(first > 0 && !rows1.is_empty());
        // Running again writes the same set (upsert), not duplicates.
        store.snapshot_all_weeks(0).unwrap();
        let rows2 = store.list_snapshots().unwrap();
        assert_eq!(
            rows1.len(),
            rows2.len(),
            "snapshot re-run must not duplicate"
        );
        assert!(rows2.iter().all(|s| s.volume > 0));
    }

    // --- M4 arcs + pulses ---

    /// Seeds one device with activity in two countries plus one unmapped
    /// domain: samsungads.com (KR, tracker, blocked), api.github.com (US,
    /// first-party), d1.example (no entity → no country).
    fn seed_arcs() -> Store {
        let store = Store::open_in_memory().unwrap();
        let mac = "f0:5c:77:11:22:33";
        let ip = "192.168.1.20";
        store
            .apply_batch(
                "s1",
                "fixture",
                &[
                    mac_event(0, mac, ip, "samsungads.com", true),
                    mac_event(1_000, mac, ip, "samsungads.com", true),
                    mac_event(2_000, mac, ip, "api.github.com", false),
                    mac_event(3_000, mac, ip, "d1.example", false),
                ],
                Some("4"),
                0,
            )
            .unwrap();
        store
    }

    #[test]
    fn arcs_group_by_country_and_disclose_unmapped() {
        let store = seed_arcs();
        let res = store.arcs(None).unwrap();
        assert_eq!(res.arcs.len(), 2, "KR and US arcs; d1.example is no arc");
        let kr = res.arcs.iter().find(|a| a.country == "KR").unwrap();
        assert_eq!(kr.queries, 2);
        assert_eq!(kr.blocked, 2);
        assert_eq!(kr.tracker_queries, 2);
        assert_eq!(kr.domains, 1);
        assert_eq!(kr.device_name, "Samsung Electronics · 22:33");
        let us = res.arcs.iter().find(|a| a.country == "US").unwrap();
        assert_eq!(us.queries, 1);
        assert_eq!(us.tracker_queries, 0);
        assert_eq!(res.unmapped_queries, 1, "d1.example disclosed, not hidden");
    }

    #[test]
    fn arcs_fold_merged_devices() {
        let store = Store::open_in_memory().unwrap();
        store
            .apply_batch(
                "s1",
                "fixture",
                &[
                    mac_event(
                        0,
                        "f0:5c:77:11:22:33",
                        "192.168.1.20",
                        "samsungads.com",
                        true,
                    ),
                    mac_event(
                        0,
                        "f4:0f:24:40:50:60",
                        "192.168.1.31",
                        "samsungads.com",
                        false,
                    ),
                ],
                Some("2"),
                0,
            )
            .unwrap();
        let devices = store.list_devices().unwrap();
        let (a, b) = (devices[0].id, devices[1].id);
        store.merge_devices(b, a).unwrap();

        let res = store.arcs(None).unwrap();
        assert_eq!(res.arcs.len(), 1, "merged devices fold to one arc");
        assert_eq!(res.arcs[0].device_id, a);
        assert_eq!(res.arcs[0].queries, 2);
    }

    #[test]
    fn arcs_window_is_inclusive_start_exclusive_end() {
        let store = Store::open_in_memory().unwrap();
        let hour_ms = 3_600_000i64;
        // One event in hour bucket 5.
        store
            .apply_batch(
                "s1",
                "fixture",
                &[mac_event(
                    5 * hour_ms + 60_000,
                    "f0:5c:77:11:22:33",
                    "192.168.1.20",
                    "samsungads.com",
                    false,
                )],
                Some("1"),
                0,
            )
            .unwrap();
        let hits = |w| store.arcs(Some(w)).unwrap().arcs.len();
        assert_eq!(hits((5 * hour_ms, 6 * hour_ms)), 1, "start is inclusive");
        assert_eq!(hits((6 * hour_ms, 7 * hour_ms)), 0, "after the bucket");
        assert_eq!(hits((0, 5 * hour_ms)), 0, "end is exclusive");
    }

    #[test]
    fn arc_domains_list_busiest_first_with_enrichment() {
        let store = seed_arcs();
        let id = store.list_devices().unwrap()[0].id;
        let rows = store.arc_domains(id, "KR", None).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].domain, "samsungads.com");
        assert_eq!(rows[0].entity.as_deref(), Some("Samsung Ads"));
        assert_eq!(rows[0].category, "advertising");
        assert!(rows[0].is_tracker);
        assert_eq!(rows[0].queries, 2);
        assert_eq!(rows[0].blocked, 2);
        assert!(store.arc_domains(id, "JP", None).unwrap().is_empty());
    }

    #[test]
    fn domain_rollups_return_raw_hourly_buckets() {
        let store = Store::open_in_memory().unwrap();
        let mac = "f0:5c:77:11:22:33";
        let hour_ms = 3_600_000i64;
        store
            .apply_batch(
                "s1",
                "fixture",
                &[
                    mac_event(0, mac, "192.168.1.20", "samsungads.com", true),
                    mac_event(1_000, mac, "192.168.1.20", "samsungads.com", false),
                    mac_event(hour_ms, mac, "192.168.1.20", "samsungads.com", false),
                ],
                Some("3"),
                0,
            )
            .unwrap();
        let id = store.list_devices().unwrap()[0].id;
        let rows = store.domain_rollups(id, "samsungads.com", None).unwrap();
        assert_eq!(rows.len(), 2, "two hour buckets");
        assert_eq!(
            (rows[0].bucket_hour, rows[0].count, rows[0].blocked_count),
            (0, 2, 1)
        );
        assert_eq!(
            (rows[1].bucket_hour, rows[1].count, rows[1].blocked_count),
            (1, 1, 0)
        );
    }

    #[test]
    fn apply_batch_returns_enriched_pulses() {
        let store = seed_arcs();
        // seed_arcs discards; run a fresh batch to inspect the pulses.
        let pulses = store
            .apply_batch(
                "s1",
                "fixture",
                &[
                    mac_event(
                        10_000,
                        "f0:5c:77:11:22:33",
                        "192.168.1.20",
                        "samsungads.com",
                        true,
                    ),
                    mac_event(
                        11_000,
                        "f0:5c:77:11:22:33",
                        "192.168.1.20",
                        "samsungads.com",
                        true,
                    ),
                    mac_event(
                        12_000,
                        "f0:5c:77:11:22:33",
                        "192.168.1.20",
                        "d1.example",
                        false,
                    ),
                ],
                Some("7"),
                0,
            )
            .unwrap();
        assert_eq!(pulses.len(), 2, "one pulse per (device, domain)");
        assert_eq!(pulses.iter().map(|p| p.count).sum::<i64>(), 3);
        let ads = pulses
            .iter()
            .find(|p| p.domain == "samsungads.com")
            .unwrap();
        assert_eq!(ads.country.as_deref(), Some("KR"));
        assert!(ads.is_tracker);
        assert_eq!(ads.count, 2);
        assert_eq!(ads.device_name, "Samsung Electronics · 22:33");
        let unknown = pulses.iter().find(|p| p.domain == "d1.example").unwrap();
        assert_eq!(unknown.country, None);
        assert!(!unknown.is_tracker);
    }

    #[test]
    fn pulses_attribute_to_the_canonical_device_after_merge() {
        let store = Store::open_in_memory().unwrap();
        let batch = vec![
            mac_event(0, "f0:5c:77:11:22:33", "192.168.1.20", "a.com", false),
            mac_event(0, "f4:0f:24:40:50:60", "192.168.1.31", "b.com", false),
        ];
        store
            .apply_batch("s1", "fixture", &batch, Some("1"), 0)
            .unwrap();
        let devices = store.list_devices().unwrap();
        let (a, b) = (devices[0].id, devices[1].id);
        store.merge_devices(b, a).unwrap();

        // Re-ingest the merged client: its pulse must point at the canonical.
        let pulses = store
            .apply_batch(
                "s1",
                "fixture",
                &[mac_event(
                    1_000,
                    "f4:0f:24:40:50:60",
                    "192.168.1.31",
                    "b.com",
                    false,
                )],
                Some("2"),
                0,
            )
            .unwrap();
        assert_eq!(pulses.len(), 1);
        assert_eq!(pulses[0].device_id, a, "pulse follows merged_into");
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
