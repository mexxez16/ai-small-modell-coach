use axum::{extract::{Path as AxumPath, State}, Json};
use coach_core::model::{new_id, now, Run, RunStatus, Session, SessionStatus};
use coach_core::query::{self, Query};
use serde_json::{json, Value};

use crate::api_error::ApiError;
use crate::state::AppState;

#[derive(serde::Serialize, utoipa::ToSchema)]
pub struct Health {
    pub status: String,
    pub version: String,
}

#[utoipa::path(get, path = "/api/health", responses((status = 200, body = Health)))]
pub async fn health() -> Json<Health> {
    Json(Health {
        status: "ok".into(),
        version: env!("CARGO_PKG_VERSION").into(),
    })
}

#[derive(Debug, serde::Deserialize, utoipa::ToSchema)]
pub struct CreateSession {
    pub name: String,
    #[serde(default)]
    pub description: String,
}

#[utoipa::path(
    post,
    path = "/api/sessions",
    request_body = CreateSession,
    responses((status = 200, body = Session))
)]
pub async fn create_session(
    State(state): State<AppState>,
    Json(body): Json<CreateSession>,
) -> Result<Json<Session>, ApiError> {
    let ts = now();
    let session = Session {
        id: new_id(),
        name: body.name,
        description: body.description,
        status: SessionStatus::Active,
        created_at: ts,
        updated_at: ts,
        tags: vec![],
    };
    Ok(Json(state.storage.create_session(session)?))
}

#[utoipa::path(get, path = "/api/sessions", responses((status = 200, body = [Session])))]
pub async fn list_sessions(State(state): State<AppState>) -> Result<Json<Vec<Session>>, ApiError> {
    Ok(Json(state.storage.list_sessions()?))
}

#[derive(Debug, serde::Deserialize, utoipa::ToSchema)]
pub struct CreateRun {
    pub name: String,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub config: Option<Value>,
    #[serde(default)]
    pub hparams: Value,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[utoipa::path(post, path = "/api/runs", request_body = CreateRun, responses((status = 200, body = Run)))]
pub async fn create_run(
    State(state): State<AppState>,
    Json(body): Json<CreateRun>,
) -> Result<Json<Run>, ApiError> {
    let ts = now();
    let run = Run {
        id: new_id(),
        name: body.name,
        session_id: body.session_id,
        status: RunStatus::Running,
        config: body.config,
        hparams: body.hparams,
        dataset_id: None,
        dataset_hash: None,
        created_at: ts,
        updated_at: ts,
        tags: body.tags,
    };
    Ok(Json(state.storage.create_run(run)?))
}

#[utoipa::path(get, path = "/api/runs", params(("session_id" = Option<String>, Query, description = "filter by session")), responses((status = 200, body = [Run])))]
pub async fn list_runs(
    State(state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Vec<Run>>, ApiError> {
    Ok(Json(
        state.storage.list_runs(params.get("session_id").map(String::as_str))?,
    ))
}

/// Batched metric ingest for a run (used by the spaCy logger and the PyO3
/// SDK).
#[utoipa::path(
    post,
    path = "/api/runs/{id}/metrics",
    request_body = [MetricPointDto],
    responses((status = 200))
)]
pub async fn append_metrics(
    State(state): State<AppState>,
    AxumPath(run_id): AxumPath<String>,
    Json(points): Json<Vec<MetricPointDto>>,
) -> Result<Json<Value>, ApiError> {
    // Validate run exists (bail with 404 otherwise).
    state.storage.get_run(&run_id)?;
    let points: Vec<coach_core::model::MetricPoint> = points
        .into_iter()
        .map(|p| coach_core::model::MetricPoint {
            run_id: run_id.clone(),
            name: p.name,
            context: p.context,
            step: p.step,
            epoch: p.epoch,
            value: p.value,
            timestamp: p.timestamp.unwrap_or_else(now),
        })
        .collect();    state.storage.append_metric_points(&points)?;
    Ok(Json(json!({"accepted": points.len()})))
}

#[derive(Debug, serde::Deserialize, utoipa::ToSchema)]
pub struct MetricPointDto {
    pub name: String,
    #[serde(default)]
    pub context: Value,
    pub step: u64,
    #[serde(default)]
    pub epoch: Option<u64>,
    pub value: f64,
    #[serde(default)]
    pub timestamp: Option<chrono::DateTime<chrono::Utc>>,
}
/// AimQL-like run query, e.g. `run.name == "x" and metrics.cats_score > 0.9`.
/// The scaffold parses and echoes the parsed AST; metric evaluation lands
/// with the Parquet/DuckDB backend.
#[derive(Debug, serde::Deserialize, utoipa::ToSchema)]
pub struct QueryRequest {
    #[serde(default)]
    pub query: String,
}

#[utoipa::path(post, path = "/api/query", request_body = QueryRequest, responses((status = 200)))]
pub async fn query_runs(
    State(_state): State<AppState>,
    Json(body): Json<QueryRequest>,
) -> Result<Json<Value>, ApiError> {
    let parsed: Query = query::parse(&body.query)?;
    Ok(Json(json!({
        "parsed": parsed.to_string(),
        "matched_runs": [],
        "note": "scaffold: metric evaluation lands with the Parquet/DuckDB backend",
    })))
}
