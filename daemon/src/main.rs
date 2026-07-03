//! Phonehome daemon: one binary serving the JSON API, the embedded UI, and the
//! ingestion loops (D-002/D-006).

mod adguard;
mod ingest;
mod pihole;
mod store;

use axum::{
    extract::{Path, State},
    http::{header, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use phonehome_core::{FixtureReplayer, Ingestor};
use rust_embed::RustEmbed;
use serde::Deserialize;
use std::path::PathBuf;
use std::time::Duration;
use store::{DeviceError, Store};

/// `ui/dist` compiled into the binary. Build order: `npm run build` in `ui/`
/// first, then `cargo build` (see CLAUDE.md commands; Dockerfile and CI follow it).
#[derive(RustEmbed)]
#[folder = "../ui/dist"]
struct Assets;

const DEFAULT_PORT: u16 = 8480;
const DEFAULT_DB: &str = "data/phonehome.db";
const FIXTURE_CHUNK: usize = 1000;

fn app(store: Store) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/stats", get(stats))
        .route("/api/devices", get(devices))
        .route("/api/devices/rename", post(rename_device))
        .route("/api/devices/merge", post(merge_devices))
        .route("/api/devices/{id}/scorecard", get(scorecard))
        .route("/api/snapshots", get(snapshots))
        .fallback(static_handler)
        .with_state(store)
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

fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[tokio::main]
async fn main() {
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
    tracing::info!(db = %db_path.display(), "store opened");

    for (ingestor, interval) in configured_ingestors() {
        tokio::spawn(ingest::run(store.clone(), ingestor, interval));
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

    axum::serve(listener, app(store))
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

    fn test_app() -> Router {
        app(Store::open_in_memory().unwrap())
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
        let res = app(seed_store())
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
        let res = app(store.clone())
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
        let res = app(store.clone())
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
        let res = app(seed_store())
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
        let res = app(store)
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
        let res = app(store)
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
        let res = app(seed_store())
            .oneshot(
                Request::get("/api/devices/9999/scorecard")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn snapshots_endpoint_lists_after_job() {
        let store = seed_store();
        store.snapshot_all_weeks(0).unwrap();
        let res = app(store)
            .oneshot(Request::get("/api/snapshots").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let v = json_body(res).await;
        assert!(!v.as_array().unwrap().is_empty());
        assert!(v[0]["score"].is_number());
    }
}
