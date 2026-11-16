//! coach-api: axum HTTP + WebSocket server for AI Small Modell Coach.

mod api_error;
mod state;
mod routes;

use axum::{routing::{delete, get, post}, Router};
use state::{AppConfig, DEFAULT_PORT, PORT_SCAN_TRIES, AppState};
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    info(title = "AI Small Modell Coach API", version = "0.1.0"),
    paths(
        routes::api::health,
        routes::api::create_session,
        routes::api::list_sessions,
        routes::api::create_run,
        routes::api::list_runs,
        routes::api::append_metrics,
        routes::api::query_runs,
        routes::workers::spawn_worker,
        routes::workers::list_workers,
        routes::workers::worker_health,
        routes::workers::stop_worker,
    ),
    components(schemas(
        routes::api::Health,
        routes::api::CreateSession,
        routes::api::CreateRun,
        routes::api::QueryRequest,
        routes::api::MetricPointDto,
        routes::workers::SpawnWorkerRequest,
        routes::workers::SpawnWorkerResponse,
        coach_core::model::Session,
        coach_core::model::Run,
        coach_core::model::RunStatus,
        coach_core::model::SessionStatus,
        coach_core::model::RenderWorker,
        coach_core::model::WorkerStatus,
    ))
)]
struct ApiDoc;

fn build_router(state: AppState) -> Router {
    let mut app = Router::new()
        .route("/api/health", get(routes::api::health))
        .route("/api/sessions", get(routes::api::list_sessions).post(routes::api::create_session))
        .route("/api/runs", get(routes::api::list_runs).post(routes::api::create_run))
        .route("/api/runs/{id}/metrics", post(routes::api::append_metrics))
        .route("/api/query", post(routes::api::query_runs))
        .route("/api/vibe/capabilities", get(routes::vibe::capabilities))
        .route(
            "/api/workers",
            get(routes::workers::list_workers).post(routes::workers::spawn_worker),
        )
        .route("/api/workers/{id}", delete(routes::workers::stop_worker))
        .route("/api/workers/{id}/health", get(routes::workers::worker_health))
        .route("/ws/metrics", get(routes::ws::metrics_ws))
        .route("/ws/terminal", get(routes::ws::terminal_ws))
        .route("/ws/chat", get(routes::ws::chat_ws))
        .route("/ws/jobs", get(routes::ws::jobs_ws))
        .route("/api/openapi.json", get(openapi_json))
        .with_state(state.clone());

    if let Some(dist) = state.config.web_dist.clone() {
        // Static assets from the built frontend; every unknown path (deep
        // links like /runs) falls back to index.html so the SPA router takes
        // over.
        let index_path = dist.join("index.html");
        let spa_fallback = tower::service_fn(move |_req: axum::extract::Request| {
            let index_path = index_path.clone();
            async move {
                let body = match tokio::fs::read(&index_path).await {
                    Ok(body) => (
                        axum::http::StatusCode::OK,
                        "text/html; charset=utf-8",
                        body,
                    ),
                    Err(_) => (
                        axum::http::StatusCode::NOT_FOUND,
                        "text/plain; charset=utf-8",
                        b"index.html not found in COACH_WEB_DIST".to_vec(),
                    ),
                };
                let (status, content_type, payload) = body;
                let response = axum::response::Response::builder()
                    .status(status)
                    .header("content-type", content_type)
                    .body(axum::body::Body::from(payload))
                    .expect("static fallback response is buildable");
                Ok::<_, std::convert::Infallible>(response)
            }
        });
        let serve = ServeDir::new(dist).fallback(spa_fallback);
        app = app.fallback_service(serve);
    }
    app
}

async fn openapi_json() -> axum::Json<utoipa::openapi::OpenApi> {
    axum::Json(ApiDoc::openapi())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,tower_http=info".into()),
        )
        .init();

    let config = AppConfig::from_env();
    let app = build_router(AppState::new(config.clone())).layer(TraceLayer::new_for_http());

    let (listener, port) = bind_listener(config.port).await?;
    tracing::info!("coach-api listening on http://127.0.0.1:{port}");
    axum::serve(listener, app).await?;
    Ok(())
}

/// Bind to the explicit `COACH_PORT` when set; otherwise scan upward from
/// `DEFAULT_PORT` (8901) to avoid conflicts with other local services.
async fn bind_listener(explicit_port: Option<u16>) -> anyhow::Result<(tokio::net::TcpListener, u16)> {
    if let Some(port) = explicit_port {
        let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .map_err(|e| anyhow::anyhow!("COACH_PORT {port} requested but not bindable: {e}"))?;
        return Ok((listener, port));
    }
    for offset in 0..PORT_SCAN_TRIES {
        let port = DEFAULT_PORT + offset;
        let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
        match tokio::net::TcpListener::bind(addr).await {
            Ok(listener) => return Ok((listener, port)),
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
                tracing::debug!(port, "port in use, trying next");
            }
            Err(e) => return Err(e.into()),
        }
    }
    Err(anyhow::anyhow!(
        "no free port found in {DEFAULT_PORT}..{}",
        DEFAULT_PORT + PORT_SCAN_TRIES
    ))
}

#[cfg(test)]
mod port_tests {
    use super::*;

    #[tokio::test]
    async fn explicit_port_binds_exactly() {
        // Off the scan range (8901..) so the two port tests never fight over
        // the same port when run in parallel.
        let explicit = DEFAULT_PORT + 100;
        let (listener, port) = bind_listener(Some(explicit)).await.unwrap();
        assert_eq!(port, explicit);
        drop(listener);
    }

    #[tokio::test]
    async fn scan_skips_occupied_default_port() {
        // Hold 8901, then bind without COACH_PORT: must land on 8902.
        let holder = tokio::net::TcpListener::bind(
            std::net::SocketAddr::from(([127, 0, 0, 1], DEFAULT_PORT)),
        )
        .await
        .unwrap();
        let (_listener, port) = bind_listener(None).await.unwrap();
        assert_eq!(port, DEFAULT_PORT + 1);
        drop(holder);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    #[tokio::test]
    async fn health_endpoint_responds() {
        let app = build_router(AppState::new(AppConfig {
            port: None,
            token: None,
            web_dist: None,
        }));
        let res = app
            .oneshot(Request::get("/api/health").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert!(res.status().is_success());
        let body = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], "ok");
    }

    #[tokio::test]
    async fn session_roundtrip_over_http() {
        let app = build_router(AppState::new(AppConfig {
            port: None,
            token: None,
            web_dist: None,
        }));
        let res = app
            .oneshot(
                Request::post("/api/sessions")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"name": "sprint-1"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(res.status().is_success());
    }

    #[tokio::test]
    async fn query_parses_aimql() {
        let app = build_router(AppState::new(AppConfig {
            port: None,
            token: None,
            web_dist: None,
        }));
        let res = app
            .oneshot(
                Request::post("/api/query")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"query": "run.name == \"x\" and metrics.cats_score > 0.9"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(res.status().is_success());
        let body = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            json["parsed"],
            "run.name == \"x\" and metrics.cats_score > 0.9"
        );
    }
}
