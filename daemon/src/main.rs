//! Phonehome daemon: one binary serving the JSON API and the embedded UI (D-002/D-006).

use axum::{
    http::{header, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use rust_embed::RustEmbed;

/// `ui/dist` compiled into the binary. Build order: `npm run build` in `ui/`
/// first, then `cargo build` (see CLAUDE.md commands; Dockerfile and CI follow it).
#[derive(RustEmbed)]
#[folder = "../ui/dist"]
struct Assets;

const DEFAULT_PORT: u16 = 8480;

fn app() -> Router {
    Router::new()
        .route("/api/health", get(health))
        .fallback(static_handler)
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "alive",
        "version": env!("CARGO_PKG_VERSION"),
    }))
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

    let port: u16 = std::env::var("PHONEHOME_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(DEFAULT_PORT);
    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .unwrap_or_else(|e| panic!("failed to bind {addr}: {e}"));
    tracing::info!("phonehome daemon listening on http://localhost:{port}");

    axum::serve(listener, app())
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

    #[tokio::test]
    async fn health_returns_alive_with_version() {
        let res = app()
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
    async fn root_serves_embedded_ui() {
        let res = app()
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
        let res = app()
            .oneshot(Request::get("/api/nope").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }
}
