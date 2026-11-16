//! coach-api: axum HTTP + WebSocket server for AI Small Modell Coach.

mod api_error;
mod state;
mod routes;

use axum::{routing::{delete, get, post}, Router};
use state::{AppConfig, AppState};
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

async fn spa_fallback() -> axum::response::Response {
    (
        axum::http::StatusCode::NOT_FOUND,
        "Not found. Serve the built frontend by starting coach-api with COACH_WEB_DIST set.",
    )
        .into_response_compat()
}

// Small helper so the fallback returns a full response without pulling the
// IntoResponse trait explicitly at every call site.
trait IntoResponseCompat {
    fn into_response_compat(self) -> axum::response::Response;
}

impl<T: axum::response::IntoResponse> IntoResponseCompat for T {
    fn into_response_compat(self) -> axum::response::Response {
        axum::response::IntoResponse::into_response(self)
    }
}

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
        let serve = ServeDir::new(dist).fallback(get(spa_fallback));
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
    let port = config.port;
    let app = build_router(AppState::new(config)).layer(TraceLayer::new_for_http());

    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    tracing::info!("coach-api listening on http://{addr}");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
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
            port: 0,
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
            port: 0,
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
            port: 0,
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
