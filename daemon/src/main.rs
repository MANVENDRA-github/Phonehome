//! Phonehome daemon: one binary serving the JSON API, the embedded UI, and the
//! ingestion loops (D-002/D-006).

mod adguard;
mod ingest;
mod pihole;
mod store;

use axum::{
    extract::{FromRef, Path, Query, State},
    http::{header, StatusCode, Uri},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    routing::{get, post},
    Json, Router,
};
use phonehome_core::{FixtureReplayer, Ingestor};
use rust_embed::RustEmbed;
use serde::Deserialize;
use std::collections::HashMap;
use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use store::{DeviceError, Store};
use tokio::sync::{broadcast, Mutex};
use tokio::task::JoinHandle;
use tokio_stream::{wrappers::BroadcastStream, Stream, StreamExt};

/// `ui/dist` compiled into the binary. Build order: `npm run build` in `ui/`
/// first, then `cargo build` (see CLAUDE.md commands; Dockerfile and CI follow it).
#[derive(RustEmbed)]
#[folder = "../ui/dist"]
struct Assets;

const DEFAULT_PORT: u16 = 8480;
const DEFAULT_DB: &str = "data/phonehome.db";
const FIXTURE_CHUNK: usize = 1000;
/// Live-source poll cadence when the wizard/env doesn't override it.
const DEFAULT_POLL_INTERVAL_SECS: u64 = 15;
/// Live-pulse fan-out buffer; a lagging SSE subscriber drops pulses (they are
/// hints, not state — the globe refetches /api/arcs to reconcile).
const PULSE_BUFFER: usize = 256;

/// The home network's location on the globe (arc origin). Config-only data —
/// never derived from traffic.
#[derive(Clone, Copy, serde::Serialize)]
struct Home {
    lat: f64,
    lon: f64,
}

/// Client-facing runtime config (`GET /api/config`). Computed per request now
/// (M5): `home` and `needs_setup` can change at runtime once the wizard writes
/// config, so this is no longer a cached-at-startup snapshot.
#[derive(serde::Serialize)]
struct ApiConfig {
    home: Option<Home>,
    version: &'static str,
    /// True on a fresh install with no source configured any way — the UI shows
    /// the first-run setup wizard. Keyed on source *existence*, not data volume,
    /// so a valid-but-quiet source never re-triggers the wizard.
    needs_setup: bool,
}

/// Runtime handles of the per-source ingest loops, keyed by source id. Lets the
/// wizard start a new source (or replace one) without a process restart.
type IngestRegistry = Arc<Mutex<HashMap<String, JoinHandle<()>>>>;

#[derive(Clone)]
struct AppState {
    store: Store,
    pulses: broadcast::Sender<store::Pulse>,
    ingestors: IngestRegistry,
    /// Whether any source was configured from env at boot (`PHONEHOME_FIXTURE`
    /// / `_PIHOLE_*` / `_ADGUARD_*`). Env-configured deployments skip the wizard.
    has_startup_source: bool,
    /// Home origin from `PHONEHOME_HOME_LAT/LON`, if set (fallback under the
    /// wizard-persisted home).
    env_home: Option<Home>,
    version: &'static str,
}

/// Lets the seven pre-M4 handlers keep extracting `State<Store>` unchanged.
impl FromRef<AppState> for Store {
    fn from_ref(state: &AppState) -> Store {
        state.store.clone()
    }
}

fn app(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/config", get(config))
        .route("/api/stats", get(stats))
        .route("/api/devices", get(devices))
        .route("/api/devices/rename", post(rename_device))
        .route("/api/devices/merge", post(merge_devices))
        .route("/api/devices/{id}/scorecard", get(scorecard))
        .route("/api/snapshots", get(snapshots))
        .route("/api/diffs", get(diffs))
        .route("/api/sources", get(list_sources).post(create_source))
        .route("/api/sources/test", post(test_source))
        .route("/api/arcs", get(arcs))
        .route("/api/arcs/domains", get(arc_domains))
        .route("/api/rollups", get(rollups))
        .route("/api/stream", get(stream))
        .fallback(static_handler)
        .with_state(state)
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "alive",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

async fn stats(State(store): State<Store>) -> Result<Json<store::Stats>, Response> {
    let stats = tokio::task::spawn_blocking(move || store.stats())
        .await
        .map_err(internal_error)?
        .map_err(internal_error)?;
    Ok(Json(stats))
}

fn internal_error(e: impl std::fmt::Display) -> Response {
    tracing::error!(error = %e, "internal error");
    (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response()
}

async fn devices(State(store): State<Store>) -> Result<Json<Vec<store::DeviceRow>>, Response> {
    let devices = tokio::task::spawn_blocking(move || store.list_devices())
        .await
        .map_err(internal_error)?
        .map_err(internal_error)?;
    Ok(Json(devices))
}

#[derive(Deserialize)]
struct RenameReq {
    id: i64,
    name: String,
}

async fn rename_device(
    State(store): State<Store>,
    Json(req): Json<RenameReq>,
) -> Result<StatusCode, Response> {
    let renamed = tokio::task::spawn_blocking(move || store.rename_device(req.id, &req.name))
        .await
        .map_err(internal_error)?
        .map_err(internal_error)?;
    if renamed {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err((StatusCode::NOT_FOUND, "no such device").into_response())
    }
}

#[derive(Deserialize)]
struct MergeReq {
    source: i64,
    into: i64,
}

async fn merge_devices(
    State(store): State<Store>,
    Json(req): Json<MergeReq>,
) -> Result<StatusCode, Response> {
    let result = tokio::task::spawn_blocking(move || store.merge_devices(req.source, req.into))
        .await
        .map_err(internal_error)?;
    match result {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(DeviceError::NotFound) => {
            Err((StatusCode::NOT_FOUND, "no such device").into_response())
        }
        Err(DeviceError::BadMerge(m)) => Err((StatusCode::BAD_REQUEST, m).into_response()),
        Err(DeviceError::Db(e)) => Err(internal_error(e)),
    }
}

async fn scorecard(
    State(store): State<Store>,
    Path(id): Path<i64>,
) -> Result<Json<store::DeviceScorecard>, Response> {
    let card = tokio::task::spawn_blocking(move || store.device_scorecard(id))
        .await
        .map_err(internal_error)?
        .map_err(internal_error)?;
    match card {
        Some(c) => Ok(Json(c)),
        None => Err((StatusCode::NOT_FOUND, "no such device").into_response()),
    }
}

async fn snapshots(State(store): State<Store>) -> Result<Json<Vec<store::Snapshot>>, Response> {
    let rows = tokio::task::spawn_blocking(move || store.list_snapshots())
        .await
        .map_err(internal_error)?
        .map_err(internal_error)?;
    Ok(Json(rows))
}

/// Cap on the "new this week" domain list per device (M6 diff). Enough to tell
/// the story ("+6 tracker domains: …") without unbounded payloads.
const DIFF_NEW_DOMAIN_LIMIT: usize = 20;

async fn diffs(State(store): State<Store>) -> Result<Json<store::DiffsResponse>, Response> {
    let res = tokio::task::spawn_blocking(move || store.week_diffs(DIFF_NEW_DOMAIN_LIMIT))
        .await
        .map_err(internal_error)?
        .map_err(internal_error)?;
    Ok(Json(res))
}

async fn config(State(state): State<AppState>) -> Result<Json<ApiConfig>, Response> {
    let store = state.store.clone();
    let (persisted_home, source_configs) = tokio::task::spawn_blocking(move || {
        Ok::<_, rusqlite::Error>((store.get_home()?, store.source_config_count()?))
    })
    .await
    .map_err(internal_error)?
    .map_err(internal_error)?;
    // Wizard-set home (the latest explicit user intent) wins over the env fallback.
    let home = persisted_home
        .map(|(lat, lon)| Home { lat, lon })
        .or(state.env_home);
    let needs_setup = !state.has_startup_source && source_configs == 0;
    Ok(Json(ApiConfig {
        home,
        version: state.version,
        needs_setup,
    }))
}

/// The wizard's "test connection" and the persist path share one probe: build
/// the adapter and validate credentials without touching the DB. `Err` carries
/// the HTTP status the caller should return (400 for a client mistake like a
/// missing username or unknown kind, 502 for a source that rejected us).
async fn probe_source(
    kind: &str,
    base_url: &str,
    username: Option<&str>,
    secret: &str,
) -> Result<(), (StatusCode, String)> {
    match kind {
        "pihole" => pihole::PiholeIngestor::probe(base_url, secret)
            .await
            .map_err(|e| (StatusCode::BAD_GATEWAY, e.0)),
        "adguard" => {
            let user = username.ok_or((
                StatusCode::BAD_REQUEST,
                "adguard requires a username".to_string(),
            ))?;
            adguard::AdguardIngestor::probe(base_url, user, secret)
                .await
                .map_err(|e| (StatusCode::BAD_GATEWAY, e.0))
        }
        other => Err((
            StatusCode::BAD_REQUEST,
            format!("unknown source kind: {other}"),
        )),
    }
}

#[derive(Deserialize)]
struct SourceTestReq {
    kind: String,
    base_url: String,
    username: Option<String>,
    secret: String,
}

/// `POST /api/sources/test` — validate a source without persisting. Returns
/// `{ok:true}` on success, or the probe's status + `{ok:false, error}`.
async fn test_source(Json(req): Json<SourceTestReq>) -> Response {
    match probe_source(
        &req.kind,
        &req.base_url,
        req.username.as_deref(),
        &req.secret,
    )
    .await
    {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response(),
        Err((code, msg)) => {
            (code, Json(serde_json::json!({ "ok": false, "error": msg }))).into_response()
        }
    }
}

#[derive(Deserialize)]
struct SourceCreateReq {
    kind: String,
    base_url: String,
    username: Option<String>,
    secret: String,
    interval_s: Option<u64>,
    home_lat: Option<f64>,
    home_lon: Option<f64>,
}

/// `POST /api/sources` — probe, persist, and start ingesting immediately (no
/// restart) so data lands within one poll interval (≤60s time-to-wow, M5).
async fn create_source(
    State(state): State<AppState>,
    Json(req): Json<SourceCreateReq>,
) -> Response {
    // Probe first: never persist a source we can't reach, so the wizard's
    // "paste → data" promise is honest.
    if let Err((code, msg)) = probe_source(
        &req.kind,
        &req.base_url,
        req.username.as_deref(),
        &req.secret,
    )
    .await
    {
        return (code, Json(serde_json::json!({ "error": msg }))).into_response();
    }

    let input = store::SourceConfigInput {
        kind: req.kind.clone(),
        base_url: req.base_url.clone(),
        username: req.username.clone(),
        secret: req.secret.clone(),
        interval_s: req.interval_s.unwrap_or(DEFAULT_POLL_INTERVAL_SECS),
    };
    let store = state.store.clone();
    let summary = match tokio::task::spawn_blocking(move || store.save_source_config(&input)).await
    {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => return internal_error(e),
        Err(e) => return internal_error(e),
    };

    // Optional home origin (validated; an out-of-range pair is ignored, not an error).
    if let (Some(lat), Some(lon)) = (req.home_lat, req.home_lon) {
        if lat.abs() <= 90.0 && lon.abs() <= 180.0 {
            let store = state.store.clone();
            if let Err(e) = tokio::task::spawn_blocking(move || store.set_home(lat, lon)).await {
                tracing::error!(error = %e, "persist home task panicked");
            }
        }
    }

    if let Some((ingestor, interval)) = build_ingestor(
        &summary.id,
        &req.kind,
        &req.base_url,
        req.username.as_deref(),
        &req.secret,
        summary.interval_s,
    ) {
        spawn_source(&state, summary.id.clone(), ingestor, interval).await;
    }
    (StatusCode::CREATED, Json(summary)).into_response()
}

/// `GET /api/sources` — configured sources, secrets stripped (D-014).
async fn list_sources(
    State(state): State<AppState>,
) -> Result<Json<Vec<store::SourceSummary>>, Response> {
    let store = state.store.clone();
    let rows = tokio::task::spawn_blocking(move || store.list_source_summaries())
        .await
        .map_err(internal_error)?
        .map_err(internal_error)?;
    Ok(Json(rows))
}

/// Resolves an optional `window` query param (hours, counting back from now)
/// to the `[start, end)` millisecond range the store queries take.
fn resolve_window(hours: Option<i64>) -> Result<Option<(i64, i64)>, (StatusCode, &'static str)> {
    match hours {
        None => Ok(None),
        Some(h) if h > 0 => {
            let now = now_ms();
            Ok(Some((now - h * 3_600_000, now)))
        }
        Some(_) => Err((StatusCode::BAD_REQUEST, "window must be positive hours")),
    }
}

#[derive(Deserialize)]
struct ArcsQuery {
    window: Option<i64>,
}

async fn arcs(
    State(store): State<Store>,
    Query(q): Query<ArcsQuery>,
) -> Result<Json<store::ArcsResponse>, Response> {
    let window = resolve_window(q.window).map_err(IntoResponse::into_response)?;
    let arcs = tokio::task::spawn_blocking(move || store.arcs(window))
        .await
        .map_err(internal_error)?
        .map_err(internal_error)?;
    Ok(Json(arcs))
}

#[derive(Deserialize)]
struct ArcDomainsQuery {
    device: i64,
    country: String,
    window: Option<i64>,
}

async fn arc_domains(
    State(store): State<Store>,
    Query(q): Query<ArcDomainsQuery>,
) -> Result<Json<Vec<store::ArcDomainRow>>, Response> {
    let window = resolve_window(q.window).map_err(IntoResponse::into_response)?;
    let rows = tokio::task::spawn_blocking(move || store.arc_domains(q.device, &q.country, window))
        .await
        .map_err(internal_error)?
        .map_err(internal_error)?;
    Ok(Json(rows))
}

#[derive(Deserialize)]
struct RollupsQuery {
    device: i64,
    domain: String,
    window: Option<i64>,
}

async fn rollups(
    State(store): State<Store>,
    Query(q): Query<RollupsQuery>,
) -> Result<Json<Vec<store::RollupRow>>, Response> {
    let window = resolve_window(q.window).map_err(IntoResponse::into_response)?;
    let rows =
        tokio::task::spawn_blocking(move || store.domain_rollups(q.device, &q.domain, window))
            .await
            .map_err(internal_error)?
            .map_err(internal_error)?;
    Ok(Json(rows))
}

/// SSE live updates (S-1): one `pulse` event per (device, domain) committed by
/// ingestion. Lagged subscribers silently drop pulses — they are hints for the
/// globe's animations, not state.
async fn stream(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = state.pulses.subscribe();
    let events = BroadcastStream::new(rx).filter_map(|item| {
        // Lagged recv errors drop silently (pulses are hints); a serialization
        // failure is a bug worth a trace, not a blank frame on the wire.
        let pulse = item.ok()?;
        match Event::default().event("pulse").json_data(&pulse) {
            Ok(event) => Some(Ok(event)),
            Err(e) => {
                tracing::warn!(error = %e, "pulse serialization failed; frame dropped");
                None
            }
        }
    });
    Sse::new(events).keep_alive(KeepAlive::default())
}

/// Serve the embedded UI; unknown non-API paths fall back to `index.html` (SPA).
async fn static_handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    match Assets::get(path) {
        Some(file) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            ([(header::CONTENT_TYPE, mime.as_ref())], file.data).into_response()
        }
        None if !path.starts_with("api/") => match Assets::get("index.html") {
            Some(index) => ([(header::CONTENT_TYPE, "text/html")], index.data).into_response(),
            None => (StatusCode::NOT_FOUND, "ui not embedded").into_response(),
        },
        None => (StatusCode::NOT_FOUND, "no such endpoint").into_response(),
    }
}

/// Build the configured ingestors from env (config surface until the M5 setup
/// wizard). Fixture, Pi-hole, and AdGuard may all be set — each becomes its own
/// source, exercising the source-agnostic boundary (D-003).
fn configured_ingestors() -> Vec<(Box<dyn Ingestor>, Duration)> {
    let mut out: Vec<(Box<dyn Ingestor>, Duration)> = Vec::new();

    if let Ok(path) = std::env::var("PHONEHOME_FIXTURE") {
        match FixtureReplayer::from_path("fixture", &PathBuf::from(&path), FIXTURE_CHUNK) {
            Ok(replayer) => {
                tracing::info!(path, events = replayer.len(), "fixture source configured");
                // Fixtures replay fast: one chunk per second regardless of the
                // live-source poll interval.
                out.push((Box::new(replayer), Duration::from_secs(1)));
            }
            Err(e) => tracing::error!(path, error = %e, "fixture source NOT started"),
        }
    }

    if let Ok(url) = std::env::var("PHONEHOME_PIHOLE_URL") {
        match std::env::var("PHONEHOME_PIHOLE_PASSWORD") {
            Ok(password) => {
                let interval = std::env::var("PHONEHOME_POLL_INTERVAL_SECS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(15u64);
                tracing::info!(url, interval, "pihole source configured");
                out.push((
                    Box::new(pihole::PiholeIngestor::new("pihole-main", url, password)),
                    Duration::from_secs(interval),
                ));
            }
            Err(_) => {
                tracing::error!("PHONEHOME_PIHOLE_URL set but PHONEHOME_PIHOLE_PASSWORD missing");
            }
        }
    }

    if let Ok(url) = std::env::var("PHONEHOME_ADGUARD_URL") {
        match (
            std::env::var("PHONEHOME_ADGUARD_USERNAME"),
            std::env::var("PHONEHOME_ADGUARD_PASSWORD"),
        ) {
            (Ok(user), Ok(password)) => {
                let interval = std::env::var("PHONEHOME_POLL_INTERVAL_SECS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(15u64);
                tracing::info!(url, interval, "adguard source configured");
                out.push((
                    Box::new(adguard::AdguardIngestor::new(
                        "adguard-main",
                        url,
                        user,
                        password,
                    )),
                    Duration::from_secs(interval),
                ));
            }
            _ => tracing::error!(
                "PHONEHOME_ADGUARD_URL set but PHONEHOME_ADGUARD_USERNAME/_PASSWORD missing"
            ),
        }
    }

    out
}

/// Builds a `Box<dyn Ingestor>` for a persisted/wizard source. Returns `None`
/// for an unknown kind (logged, skipped — never fatal). D-003: this is the only
/// place besides `configured_ingestors` that maps a kind to a concrete adapter.
fn build_ingestor(
    id: &str,
    kind: &str,
    base_url: &str,
    username: Option<&str>,
    secret: &str,
    interval_s: u64,
) -> Option<(Box<dyn Ingestor>, Duration)> {
    let interval = Duration::from_secs(interval_s);
    match kind {
        "pihole" => Some((
            Box::new(pihole::PiholeIngestor::new(id, base_url, secret)),
            interval,
        )),
        "adguard" => Some((
            Box::new(adguard::AdguardIngestor::new(
                id,
                base_url,
                username.unwrap_or_default(),
                secret,
            )),
            interval,
        )),
        other => {
            tracing::error!(kind = other, id, "unknown persisted source kind; skipped");
            None
        }
    }
}

/// Spawns (or replaces) the ingest loop for `id`, tracking its handle so a later
/// wizard save for the same id aborts the old loop first. Abort is loss/dup-safe:
/// `apply_batch` is atomic and the cursor persists, so a re-spawn resumes cleanly.
async fn spawn_source(
    state: &AppState,
    id: String,
    ingestor: Box<dyn Ingestor>,
    interval: Duration,
) {
    let handle = tokio::spawn(ingest::run(
        state.store.clone(),
        state.pulses.clone(),
        ingestor,
        interval,
    ));
    let mut reg = state.ingestors.lock().await;
    if let Some(old) = reg.insert(id, handle) {
        old.abort();
    }
}

/// Reads `PHONEHOME_HOME_LAT` / `PHONEHOME_HOME_LON` (decimal degrees). Both
/// must be present and in range or the home stays unset — the UI then shows a
/// "set home location" hint instead of a wrong origin.
fn configured_home() -> Option<Home> {
    let lat = std::env::var("PHONEHOME_HOME_LAT").ok()?;
    let lon = std::env::var("PHONEHOME_HOME_LON").ok()?;
    match (lat.parse::<f64>(), lon.parse::<f64>()) {
        (Ok(lat), Ok(lon)) if lat.abs() <= 90.0 && lon.abs() <= 180.0 => Some(Home { lat, lon }),
        _ => {
            tracing::error!(lat, lon, "invalid PHONEHOME_HOME_LAT/LON ignored");
            None
        }
    }
}

/// Recomputes weekly snapshots on an interval so the scorecard history stays
/// fresh as ingestion proceeds. Idempotent (per device×week upsert).
async fn snapshot_loop(store: Store) {
    const EVERY: Duration = Duration::from_secs(60);
    let mut ticker = tokio::time::interval(EVERY);
    loop {
        ticker.tick().await;
        let store = store.clone();
        let now = now_ms();
        match tokio::task::spawn_blocking(move || store.snapshot_all_weeks(now)).await {
            Ok(Ok(n)) if n > 0 => tracing::debug!(snapshots = n, "weekly snapshots refreshed"),
            Ok(Ok(_)) => {}
            Ok(Err(e)) => tracing::error!(error = %e, "snapshot job failed"),
            Err(e) => tracing::error!(error = %e, "snapshot task panicked"),
        }
    }
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c().await.ok();
    tracing::info!("shutting down");
}

/// `phonehome-daemon --healthcheck`: probe the local `/api/health` and exit
/// 0/1. Used by the Docker/compose HEALTHCHECK so the slim runtime image needs
/// no curl/wget, and it works under a `read_only` root filesystem.
async fn run_healthcheck() -> i32 {
    let port: u16 = std::env::var("PHONEHOME_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(DEFAULT_PORT);
    let url = format!("http://127.0.0.1:{port}/api/health");
    match reqwest::Client::new()
        .get(&url)
        .timeout(Duration::from_secs(2))
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => 0,
        Ok(r) => {
            eprintln!("healthcheck: HTTP {}", r.status());
            1
        }
        Err(e) => {
            eprintln!("healthcheck: {e}");
            1
        }
    }
}

fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[tokio::main]
async fn main() {
    // Health-probe mode (Docker/compose HEALTHCHECK) — return before starting
    // the server or touching the DB.
    if std::env::args().any(|a| a == "--healthcheck") {
        std::process::exit(run_healthcheck().await);
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "phonehome_daemon=info".into()),
        )
        .init();

    let db_path =
        PathBuf::from(std::env::var("PHONEHOME_DB").unwrap_or_else(|_| DEFAULT_DB.into()));
    if let Some(parent) = db_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .unwrap_or_else(|e| panic!("create db dir {}: {e}", parent.display()));
        }
    }
    let store = Store::open(&db_path)
        .unwrap_or_else(|e| panic!("open sqlite db {}: {e}", db_path.display()));
    // Credentials live in this file (D-014): keep it owner-only on Unix.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&db_path, std::fs::Permissions::from_mode(0o600));
    }
    tracing::info!(db = %db_path.display(), "store opened");

    let (pulses, _) = broadcast::channel::<store::Pulse>(PULSE_BUFFER);
    let env_ingestors = configured_ingestors();
    let state = AppState {
        store: store.clone(),
        pulses,
        ingestors: Arc::new(Mutex::new(HashMap::new())),
        has_startup_source: !env_ingestors.is_empty(),
        env_home: configured_home(),
        version: env!("CARGO_PKG_VERSION"),
    };

    // Env-configured sources first, then any persisted wizard sources — both go
    // through the same registry so a runtime save can later replace either.
    for (ingestor, interval) in env_ingestors {
        let id = ingestor.source_id().to_owned();
        spawn_source(&state, id, ingestor, interval).await;
    }
    match store.list_source_configs() {
        Ok(configs) => {
            for cfg in configs.into_iter().filter(|c| c.enabled) {
                if let Some((ingestor, interval)) = build_ingestor(
                    &cfg.id,
                    &cfg.kind,
                    &cfg.base_url,
                    cfg.username.as_deref(),
                    &cfg.secret,
                    cfg.interval_s,
                ) {
                    tracing::info!(id = %cfg.id, kind = %cfg.kind, "persisted source configured");
                    spawn_source(&state, cfg.id.clone(), ingestor, interval).await;
                }
            }
        }
        Err(e) => tracing::error!(error = %e, "loading persisted source configs failed"),
    }
    tokio::spawn(snapshot_loop(store.clone()));

    let port: u16 = std::env::var("PHONEHOME_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(DEFAULT_PORT);
    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .unwrap_or_else(|e| panic!("failed to bind {addr}: {e}"));
    tracing::info!("phonehome daemon listening on http://localhost:{port}");

    axum::serve(listener, app(state))
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("server error");
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{to_bytes, Body};
    use axum::http::Request;
    use tower::ServiceExt;

    fn state_for(store: Store) -> AppState {
        // Most tests represent an already-configured deployment (data seeded
        // directly), so `has_startup_source` is true → the wizard stays hidden.
        AppState {
            store,
            pulses: broadcast::channel(PULSE_BUFFER).0,
            ingestors: Arc::new(Mutex::new(HashMap::new())),
            has_startup_source: true,
            env_home: Some(Home {
                lat: 12.97,
                lon: 77.59,
            }),
            version: env!("CARGO_PKG_VERSION"),
        }
    }

    /// A fresh-install state: no env source, no env home — the wizard should show.
    fn fresh_state(store: Store) -> AppState {
        AppState {
            store,
            pulses: broadcast::channel(PULSE_BUFFER).0,
            ingestors: Arc::new(Mutex::new(HashMap::new())),
            has_startup_source: false,
            env_home: None,
            version: env!("CARGO_PKG_VERSION"),
        }
    }

    fn app_for(store: Store) -> Router {
        app(state_for(store))
    }

    fn test_app() -> Router {
        app_for(Store::open_in_memory().unwrap())
    }

    #[tokio::test]
    async fn health_returns_alive_with_version() {
        let res = test_app()
            .oneshot(Request::get("/api/health").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let bytes = to_bytes(res.into_body(), 4096).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["status"], "alive");
        assert_eq!(v["version"], env!("CARGO_PKG_VERSION"));
    }

    #[tokio::test]
    async fn stats_on_fresh_store_is_all_zeroes() {
        let res = test_app()
            .oneshot(Request::get("/api/stats").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let bytes = to_bytes(res.into_body(), 65536).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["total_queries"], 0);
        assert_eq!(v["sources"], serde_json::json!([]));
    }

    #[tokio::test]
    async fn root_serves_embedded_ui() {
        let res = test_app()
            .oneshot(Request::get("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let bytes = to_bytes(res.into_body(), 1_048_576).await.unwrap();
        let html = String::from_utf8_lossy(&bytes);
        assert!(html.contains("Phonehome"), "index.html should be embedded");
    }

    #[tokio::test]
    async fn unknown_api_path_is_404_not_spa_fallback() {
        let res = test_app()
            .oneshot(Request::get("/api/nope").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    // --- M2 device endpoints ---

    use phonehome_core::QueryEvent;
    use std::net::IpAddr;

    fn seed_store() -> Store {
        let store = Store::open_in_memory().unwrap();
        let ev = |mac: &str, ip: &str, domain: &str, blocked: bool| QueryEvent {
            ts: 0,
            client_ip: ip.parse::<IpAddr>().unwrap(),
            client_mac: Some(mac.into()),
            domain: domain.into(),
            qtype: "A".into(),
            blocked,
            source: "fixture".into(),
        };
        store
            .apply_batch(
                "fixture",
                "fixture",
                &[
                    ev("f0:5c:77:11:22:33", "192.168.1.20", "samsungads.com", true),
                    ev("f4:0f:24:40:50:60", "192.168.1.31", "xp.apple.com", true),
                ],
                Some("2"),
                0,
            )
            .unwrap();
        store
    }

    async fn json_body(res: axum::response::Response) -> serde_json::Value {
        let bytes = to_bytes(res.into_body(), 1_048_576).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn devices_endpoint_lists_named_devices() {
        let res = app_for(seed_store())
            .oneshot(Request::get("/api/devices").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let v = json_body(res).await;
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert!(arr
            .iter()
            .any(|d| d["vendor"] == "Samsung Electronics" && d["queries"] == 1));
    }

    #[tokio::test]
    async fn rename_then_merge_endpoints_work() {
        let store = seed_store();
        let ids: Vec<i64> = store.list_devices().unwrap().iter().map(|d| d.id).collect();

        // Rename first device.
        let res = app_for(store.clone())
            .oneshot(
                Request::post("/api/devices/rename")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"id": ids[0], "name": "Living Room TV"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NO_CONTENT);
        assert_eq!(
            store.list_devices().unwrap()[0].display_name,
            "Living Room TV"
        );

        // Merge second into first.
        let res = app_for(store.clone())
            .oneshot(
                Request::post("/api/devices/merge")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"source": ids[1], "into": ids[0]}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NO_CONTENT);
        assert_eq!(store.list_devices().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn rename_missing_device_is_404() {
        let res = app_for(seed_store())
            .oneshot(
                Request::post("/api/devices/rename")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"id": 9999, "name": "ghost"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn merge_into_self_is_400() {
        let store = seed_store();
        let id = store.list_devices().unwrap()[0].id;
        let res = app_for(store)
            .oneshot(
                Request::post("/api/devices/merge")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"source": id, "into": id}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    // --- M3 scorecard + snapshots endpoints ---

    #[tokio::test]
    async fn scorecard_endpoint_returns_score_with_inputs() {
        let store = seed_store();
        let id = store.list_devices().unwrap()[0].id;
        let res = app_for(store)
            .oneshot(
                Request::get(format!("/api/devices/{id}/scorecard"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let v = json_body(res).await;
        // Score plus its explaining components and raw inputs are all present.
        assert!(v["score"].is_number());
        assert!(v["components"]["tracker_share"].is_number());
        assert!(v["inputs"]["tracker_queries"].is_number());
        assert!(v["weights"]["tracker_share"].is_number());
    }

    #[tokio::test]
    async fn scorecard_missing_device_is_404() {
        let res = app_for(seed_store())
            .oneshot(
                Request::get("/api/devices/9999/scorecard")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    // --- M4 arcs + config + SSE ---

    #[tokio::test]
    async fn arcs_endpoint_returns_device_country_rows() {
        let res = app_for(seed_store())
            .oneshot(Request::get("/api/arcs").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let v = json_body(res).await;
        let arcs = v["arcs"].as_array().unwrap();
        assert_eq!(arcs.len(), 2, "one device x one country each");
        assert!(arcs
            .iter()
            .any(|a| a["country"] == "KR" && a["tracker_queries"] == 1));
        assert!(arcs.iter().all(|a| a["device_name"].is_string()));
        assert_eq!(v["unmapped_queries"], 0);
    }

    #[tokio::test]
    async fn arcs_negative_window_is_400() {
        let res = app_for(seed_store())
            .oneshot(
                Request::get("/api/arcs?window=-5")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn arc_domains_endpoint_lists_domains_behind_an_arc() {
        let store = seed_store();
        let device = store
            .list_devices()
            .unwrap()
            .iter()
            .find(|d| d.vendor.as_deref() == Some("Samsung Electronics"))
            .unwrap()
            .id;
        let res = app_for(store)
            .oneshot(
                Request::get(format!("/api/arcs/domains?device={device}&country=KR"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let v = json_body(res).await;
        let rows = v.as_array().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["domain"], "samsungads.com");
        assert_eq!(rows[0]["is_tracker"], true);
        assert_eq!(rows[0]["queries"], 1);
    }

    #[tokio::test]
    async fn arc_domains_missing_params_is_400() {
        let res = app_for(seed_store())
            .oneshot(
                Request::get("/api/arcs/domains?device=1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn rollups_endpoint_returns_hourly_buckets() {
        let store = seed_store();
        let device = store
            .list_devices()
            .unwrap()
            .iter()
            .find(|d| d.vendor.as_deref() == Some("Samsung Electronics"))
            .unwrap()
            .id;
        let res = app_for(store)
            .oneshot(
                Request::get(format!(
                    "/api/rollups?device={device}&domain=samsungads.com"
                ))
                .body(Body::empty())
                .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let v = json_body(res).await;
        let rows = v.as_array().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["count"], 1);
        assert_eq!(rows[0]["blocked_count"], 1);
    }

    #[tokio::test]
    async fn config_endpoint_reflects_state() {
        let res = test_app()
            .oneshot(Request::get("/api/config").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let v = json_body(res).await;
        assert_eq!(v["home"]["lat"], 12.97);
        assert_eq!(v["home"]["lon"], 77.59);
        assert_eq!(v["version"], env!("CARGO_PKG_VERSION"));
    }

    #[tokio::test]
    async fn stream_emits_pulse_events() {
        let state = state_for(Store::open_in_memory().unwrap());
        let tx = state.pulses.clone();
        let res = app(state)
            .oneshot(Request::get("/api/stream").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert!(res
            .headers()
            .get(header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("text/event-stream"));

        // The handler subscribed while producing the response; send now.
        tx.send(store::Pulse {
            device_id: 1,
            device_name: "Samsung Electronics · 22:33".into(),
            domain: "samsungads.com".into(),
            country: Some("KR".into()),
            is_tracker: true,
            count: 3,
        })
        .unwrap();

        let mut body = res.into_body().into_data_stream();
        let frame = tokio::time::timeout(Duration::from_secs(2), body.next())
            .await
            .expect("SSE frame within 2s")
            .expect("stream still open")
            .unwrap();
        let text = String::from_utf8(frame.to_vec()).unwrap();
        assert!(text.contains("event: pulse"), "got frame: {text}");
        assert!(text.contains("\"domain\":\"samsungads.com\""));
        assert!(text.contains("\"country\":\"KR\""));
    }

    #[tokio::test]
    async fn snapshots_endpoint_lists_after_job() {
        let store = seed_store();
        store.snapshot_all_weeks(0).unwrap();
        let res = app_for(store)
            .oneshot(Request::get("/api/snapshots").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let v = json_body(res).await;
        assert!(!v.as_array().unwrap().is_empty());
        assert!(v[0]["score"].is_number());
    }

    #[tokio::test]
    async fn diffs_endpoint_returns_shape() {
        let store = seed_store();
        store.snapshot_all_weeks(0).unwrap();
        let res = app_for(store)
            .oneshot(Request::get("/api/diffs").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let v = json_body(res).await;
        assert!(v["devices"].is_array());
        // seed_store is a single week → no previous week to compare against.
        assert!(v["previous_week_start"].is_null());
        assert!(v["current_week_start"].is_number());
    }

    // --- M5 setup wizard: config surface + sources endpoints ---

    fn source_input(
        kind: &str,
        url: &str,
        user: Option<&str>,
        secret: &str,
    ) -> store::SourceConfigInput {
        store::SourceConfigInput {
            kind: kind.into(),
            base_url: url.into(),
            username: user.map(str::to_owned),
            secret: secret.into(),
            interval_s: 15,
        }
    }

    #[tokio::test]
    async fn config_needs_setup_true_on_fresh_install() {
        let res = app(fresh_state(Store::open_in_memory().unwrap()))
            .oneshot(Request::get("/api/config").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let v = json_body(res).await;
        assert_eq!(v["needs_setup"], true);
        assert!(v["home"].is_null(), "no env or persisted home yet");
    }

    #[tokio::test]
    async fn config_needs_setup_false_once_a_source_is_configured() {
        let store = Store::open_in_memory().unwrap();
        store
            .save_source_config(&source_input("pihole", "http://pi.hole", None, "pw"))
            .unwrap();
        store.set_home(12.0, 34.0).unwrap();
        let res = app(fresh_state(store))
            .oneshot(Request::get("/api/config").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let v = json_body(res).await;
        assert_eq!(
            v["needs_setup"], false,
            "a persisted source hides the wizard"
        );
        // Persisted home surfaces even without an env home.
        assert_eq!(v["home"]["lat"], 12.0);
        assert_eq!(v["home"]["lon"], 34.0);
    }

    #[tokio::test]
    async fn config_needs_setup_false_when_env_source_present() {
        // state_for() sets has_startup_source = true (env/fixture deployment).
        let res = test_app()
            .oneshot(Request::get("/api/config").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let v = json_body(res).await;
        assert_eq!(v["needs_setup"], false);
    }

    #[tokio::test]
    async fn sources_get_lists_configs_without_secrets() {
        let store = Store::open_in_memory().unwrap();
        store
            .save_source_config(&source_input("pihole", "http://pi.hole", None, "topsecret"))
            .unwrap();
        let res = app_for(store)
            .oneshot(Request::get("/api/sources").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let bytes = to_bytes(res.into_body(), 1_048_576).await.unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(
            !text.contains("topsecret"),
            "secret must never appear in the response body: {text}"
        );
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["kind"], "pihole");
        assert_eq!(arr[0]["base_url"], "http://pi.hole");
        assert!(arr[0].get("secret").is_none());
    }

    #[tokio::test]
    async fn test_source_unknown_kind_is_400() {
        let res = test_app()
            .oneshot(
                Request::post("/api/sources/test")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"kind":"bogus","base_url":"http://x","secret":"y"})
                            .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_source_adguard_without_username_is_400() {
        let res = test_app()
            .oneshot(
                Request::post("/api/sources/test")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"kind":"adguard","base_url":"http://x","secret":"y"})
                            .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_source_unreachable_is_502() {
        // Nothing is listening on 127.0.0.1:1 → probe fails to connect → 502.
        let res = test_app()
            .oneshot(
                Request::post("/api/sources/test")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"kind":"pihole","base_url":"http://127.0.0.1:1","secret":"y"})
                            .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_GATEWAY);
        let v = json_body(res).await;
        assert_eq!(v["ok"], false);
        assert!(v["error"].is_string());
    }

    #[tokio::test]
    async fn create_source_rejects_unreachable_and_persists_nothing() {
        let store = Store::open_in_memory().unwrap();
        let res = app(fresh_state(store.clone()))
            .oneshot(
                Request::post("/api/sources")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"kind":"pihole","base_url":"http://127.0.0.1:1","secret":"y"})
                            .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_GATEWAY);
        assert_eq!(
            store.source_config_count().unwrap(),
            0,
            "a source we can't reach is never persisted"
        );
    }

    /// A canned Ingestor: yields one event on the first poll, then stays quiet
    /// (same cursor, no new events) — lets us drive `spawn_source` at runtime.
    struct OneShot {
        id: String,
        yielded: bool,
    }

    #[async_trait::async_trait]
    impl Ingestor for OneShot {
        fn source_id(&self) -> &str {
            &self.id
        }
        fn kind(&self) -> &'static str {
            "fixture"
        }
        async fn poll(
            &mut self,
            _cursor: Option<&str>,
        ) -> Result<phonehome_core::Batch, phonehome_core::IngestError> {
            if self.yielded {
                return Ok(phonehome_core::Batch {
                    events: vec![],
                    next_cursor: Some("1".into()),
                });
            }
            self.yielded = true;
            Ok(phonehome_core::Batch {
                events: vec![QueryEvent {
                    ts: 0,
                    client_ip: "192.168.1.20".parse::<IpAddr>().unwrap(),
                    client_mac: Some("f0:5c:77:11:22:33".into()),
                    domain: "samsungads.com".into(),
                    qtype: "A".into(),
                    blocked: true,
                    source: "fixture".into(),
                }],
                next_cursor: Some("1".into()),
            })
        }
    }

    #[tokio::test]
    async fn spawn_source_ingests_at_runtime_and_replace_aborts_old() {
        let store = Store::open_in_memory().unwrap();
        let state = fresh_state(store.clone());

        spawn_source(
            &state,
            "pihole-main".into(),
            Box::new(OneShot {
                id: "pihole-main".into(),
                yielded: false,
            }),
            Duration::from_millis(10),
        )
        .await;

        // Wait (bounded) for the runtime-spawned loop to apply its one batch.
        let mut ok = false;
        for _ in 0..200 {
            if store.stats().unwrap().total_queries >= 1 {
                ok = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(ok, "runtime-spawned source did not ingest within 2s");
        assert_eq!(store.cursor("pihole-main").unwrap().as_deref(), Some("1"));
        assert_eq!(state.ingestors.lock().await.len(), 1);

        // Replacing the same id aborts the old handle and leaves a single entry.
        // (Loss/dup-free resume from the persisted cursor is proven separately by
        // the store's restart property test — a real adapter honors the cursor.)
        spawn_source(
            &state,
            "pihole-main".into(),
            Box::new(OneShot {
                id: "pihole-main".into(),
                yielded: false,
            }),
            Duration::from_millis(10),
        )
        .await;
        assert_eq!(
            state.ingestors.lock().await.len(),
            1,
            "same id replaced, not duplicated"
        );
    }
}
