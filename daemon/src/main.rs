//! Phonehome daemon: one binary serving the JSON API, the embedded UI, and the
//! ingestion loops (D-002/D-006).

mod ingest;
mod pihole;
mod store;

use axum::{
    extract::State,
    http::{header, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use phonehome_core::{FixtureReplayer, Ingestor};
use rust_embed::RustEmbed;
use std::path::PathBuf;
use std::time::Duration;
use store::Store;

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

/// Build the configured ingestors from env (M1 config surface; the setup
/// wizard arrives at M5). PHONEHOME_FIXTURE and PHONEHOME_PIHOLE_URL(+_PASSWORD)
/// may both be set — each becomes its own source.
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

    out
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c().await.ok();
    tracing::info!("shutting down");
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
}
