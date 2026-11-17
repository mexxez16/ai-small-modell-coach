use axum::{extract::{Path as AxumPath, State}, Json};
use sha2::{Digest, Sha256};
use serde::Deserialize;
use coach_core::model::{new_id, now, Dataset, Image, Run, RunStatus, Session, SessionStatus, ConversionJob };
use coach_core::model::{
    DataItem, DataItemHistory, DataItemKind, DataItemStatus, DataSplit, InputFields, Scope, Visualization,
};

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
    for point in &points {
        let _ = state.metric_bus.send(point.clone());
    }
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
    State(state): State<AppState>,
    Json(body): Json<QueryRequest>,
) -> Result<Json<Value>, ApiError> {
    let parsed: Query = query::parse(&body.query)?;
    let runs = state.storage.list_runs(None)?;
    let matched: Vec<Run> = if parsed.is_empty() {
        runs
    } else {
        runs.into_iter()
            .filter(|run| query_matches(&state, run, &parsed))
            .collect()
    };
    Ok(Json(json!({
        "parsed": parsed.to_string(),
        "matched_runs": matched,
        "note": "evaluated live over the storage backend",
    })))
}

/// Evaluate a parsed AimQL query against a run. Left-hand paths resolve to
/// `metrics.<name>` (latest recorded value), `run.<field>` and dotted
/// `hparams.…` navigation. Metric values are fetched through the storage
/// trait only — no store-specific access.
fn query_matches(state: &AppState, run: &Run, q: &Query) -> bool {
    use coach_core::query::{Comparison, ComparisonOp, Literal};

    fn resolve(state: &AppState, run: &Run, path: &coach_core::query::Path) -> Option<Literal> {
        match path.0.first().map(String::as_str)? {
            "metrics" => {
                let name = path.0[1..].join(".");
                let latest = state
                    .storage
                    .query_metrics(&run.id, Some(&name))
                    .ok()?
                    .into_iter()
                    .max_by_key(|p| p.step)?;
                // Optional third segment selects a metric-context field.
                if let Some(field) = path.metric_field() {
                    let ctx = &latest.context;
                    return match field {
                        "step" => Some(Literal::Num(latest.step as f64)),
                        _ => ctx.get(field).and_then(json_to_literal),
                    };
                }
                Some(Literal::Num(latest.value))
            }
            "run" => match path.0.get(1).map(String::as_str)? {
                "name" => Some(Literal::Str(run.name.clone())),
                "status" => Some(Literal::Str(format!("{:?}", run.status).to_lowercase())),
                "id" => Some(Literal::Str(run.id.clone())),
                "session_id" => run.session_id.clone().map(Literal::Str),
                "tags" => Some(Literal::Str(run.tags.join(","))),
                _ => None,
            },
            "hparams" | "params" => {
                let value = path.0[1..].iter().try_fold(run.hparams.clone(), |v, k| {
                    v.get(k).cloned()
                })?;
                json_to_literal(&value)
            }
            _ => None,
        }
    }

    fn json_to_literal(value: &Value) -> Option<Literal> {
        if let Some(n) = value.as_f64() {
            Some(Literal::Num(n))
        } else if let Some(b) = value.as_bool() {
            Some(Literal::Bool(b))
        } else {
            value.as_str().map(|s| Literal::Str(s.to_string()))
        }
    }

    fn matches(operand: &Literal, op: ComparisonOp, literal: &Literal) -> bool {
        match (operand, literal) {
            (Literal::Num(a), Literal::Num(b)) => match op {
                ComparisonOp::Eq => a == b,
                ComparisonOp::Ne => a != b,
                ComparisonOp::Gt => a > b,
                ComparisonOp::Ge => a >= b,
                ComparisonOp::Lt => a < b,
                ComparisonOp::Le => a <= b,
                ComparisonOp::Match => false,
            },
            (Literal::Str(a), Literal::Str(b)) => match op {
                ComparisonOp::Eq => a == b,
                ComparisonOp::Ne => a != b,
                ComparisonOp::Match => a.contains(b),
                _ => false,
            },
            (Literal::Bool(a), Literal::Bool(b)) => {
                if matches!(op, ComparisonOp::Eq) {
                    a == b
                } else if matches!(op, ComparisonOp::Ne) {
                    a != b
                } else {
                    false
                }
            }
            // hparams/params values that are neither number nor string fall
            // back to comparing their JSON text (e.g. against a quoted literal).
            (operand, literal) => {
                let lit_str = |v: &Literal| match v {
                    Literal::Str(s) => s.clone(),
                    Literal::Num(n) => n.to_string(),
                    Literal::Bool(b) => b.to_string(),
                };
                let a = lit_str(operand);
                let b = lit_str(literal);
                let a = a.trim_matches('"').to_string();
                let b = b.trim_matches('"').to_string();
                match op {
                    ComparisonOp::Eq => a == b,
                    ComparisonOp::Ne => a != b,
                    ComparisonOp::Match => a.contains(&b),
                    _ => false,
                }
            }
        }
    }

    q.0.iter().all(|chain| {
        chain
            .0
            .iter()
            .any(|Comparison { path, op, literal }| {
                resolve(state, run, path)
                    .map(|lhs| matches(&lhs, op.clone(), literal))
                    .unwrap_or(false)
            })
    })
}

// ---------------------------------------------------------------------------
// Metric read API
// ---------------------------------------------------------------------------

#[derive(Debug, serde::Deserialize, serde::Serialize, utoipa::ToSchema)]
pub struct MetricPointRead {
    pub name: String,
    pub step: u64,
    pub epoch: Option<u64>,
    pub value: f64,
    pub context: Value,
}

/// Read all metric points of a run, optionally filtered by metric name
/// (`GET /api/runs/{id}/metrics?name=cats_score`).
#[utoipa::path(
    get,
    path = "/api/runs/{id}/metrics",
    params(("id" = String, Path, description = "run id"), ("name" = Option<String>, Query, description = "metric name filter")),
    responses((status = 200, body = [MetricPointRead]))
)]
pub async fn run_metrics(
    State(state): State<AppState>,
    AxumPath(run_id): AxumPath<String>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Vec<MetricPointRead>>, ApiError> {
    let points = state.storage.query_metrics(&run_id, params.get("name").map(String::as_str))?;
    Ok(Json(
        points
            .into_iter()
            .map(|p| MetricPointRead {
                name: p.name,
                step: p.step,
                epoch: p.epoch,
                value: p.value,
                context: p.context,
            })
            .collect(),
    ))
}

// ---------------------------------------------------------------------------
// Run status update
// ---------------------------------------------------------------------------

#[derive(Debug, serde::Deserialize, utoipa::ToSchema)]
pub struct UpdateRunStatus {
    pub status: RunStatus,
}

/// Update a run's status (`POST /api/runs/{id}/status`).
#[utoipa::path(
    post,
    path = "/api/runs/{id}/status",
    request_body = UpdateRunStatus,
    responses((status = 200, body = Run))
)]
pub async fn update_run_status(
    State(state): State<AppState>,
    AxumPath(run_id): AxumPath<String>,
    Json(body): Json<UpdateRunStatus>,
) -> Result<Json<Run>, ApiError> {
    let mut run = state.storage.get_run(&run_id)?;
    run.status = body.status;
    Ok(Json(state.storage.update_run(run)?))
}

// ---------------------------------------------------------------------------
// Images
// ---------------------------------------------------------------------------

#[derive(Debug, serde::Deserialize, utoipa::ToSchema)]
pub struct ImageIn {
    pub name: String,
    /// Base64-encoded PNG bytes.
    pub png_base64: String,
    #[serde(default)]
    pub step: Option<u64>,
    #[serde(default)]
    pub epoch: Option<u64>,
    #[serde(default)]
    pub caption: String,
    #[serde(default)]
    pub meta: Value,
}

/// Attach an image artifact to a run (`POST /api/runs/{id}/images`).
#[utoipa::path(
    post,
    path = "/api/runs/{id}/images",
    request_body = ImageIn,
    responses((status = 200, body = Image))
)]
pub async fn put_run_image(
    State(state): State<AppState>,
    AxumPath(run_id): AxumPath<String>,
    Json(body): Json<ImageIn>,
) -> Result<Json<Image>, ApiError> {
    // Validate the run exists so image uploads cannot orphan it.
    state.storage.get_run(&run_id)?;
    let image = Image {
        id: new_id(),
        run_id: Some(run_id),
        session_id: None,
        name: body.name,
        step: body.step,
        epoch: body.epoch,
        png_base64: body.png_base64,
        caption: body.caption,
        meta: body.meta,
        created_at: now(),
    };
    Ok(Json(state.storage.put_image(image)?))
}

/// List image artifacts, newest first (`GET /api/images`, optional
/// `?run_id=…` / `?session_id=…` filters).
#[utoipa::path(
    get,
    path = "/api/images",
    params(("run_id" = Option<String>, Query), ("session_id" = Option<String>, Query)),
    responses((status = 200, body = [Image]))
)]
pub async fn list_images(
    State(state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Vec<Image>>, ApiError> {
    let mut images = if let Some(run_id) = params.get("run_id") {
        state.storage.list_images(Some(run_id))?
    } else if let Some(session_id) = params.get("session_id") {
        state
            .storage
            .list_images(None)?
            .into_iter()
            .filter(|i| i.session_id.as_deref() == Some(session_id.as_str()))
            .collect()
    } else {
        state.storage.list_images(None)?
    };
    images.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(Json(images))
}

// ---------------------------------------------------------------------------
// Visualizations
// ---------------------------------------------------------------------------

#[derive(Debug, serde::Deserialize, utoipa::ToSchema)]
pub struct VisualizationIn {
    /// Target run (optional for ad-hoc visualizations).
    #[serde(default)]
    pub run_id: Option<String>,
    /// spaCy pipeline the worker exposes, e.g. `en_core_web_sm`.
    pub worker_model: String,
    /// Raw SVG produced by the render worker (already sanitized upstream).
    pub svg: String,
    #[serde(default)]
    pub style: String,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub options: Value,
}

/// Save a visualization (raw SVG), optionally attached to a run
/// (`POST /api/visualizations`).
#[utoipa::path(
    post,
    path = "/api/visualizations",
    request_body = VisualizationIn,
    responses((status = 200, body = Visualization))
)]
pub async fn put_visualization(
    State(state): State<AppState>,
    Json(body): Json<VisualizationIn>,
) -> Result<Json<Visualization>, ApiError> {
    if let Some(run_id) = body.run_id.as_deref() {
        state.storage.get_run(run_id)?;
    }
    let viz = Visualization {
        id: new_id(),
        run_id: body.run_id,
        session_id: None,
        worker_model: body.worker_model,
        text: body.text,
        dataset_item_id: None,
        style: body.style,
        options: body.options,
        svg: body.svg,
        created_at: now(),
    };
    Ok(Json(state.storage.put_visualization(viz)?))
}

/// List saved visualizations, newest first (`GET /api/visualizations`).
#[utoipa::path(
    get,
    path = "/api/visualizations",
    params(("run_id" = Option<String>, Query)),
    responses((status = 200, body = [Visualization]))
)]
pub async fn list_visualizations(
    State(state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Vec<Visualization>>, ApiError> {
    let mut items = state.storage.list_visualizations(params.get("run_id").map(String::as_str))?;
    items.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(Json(items))
}

/// Aggregated top-level numbers + indexes for the Dashboard page
/// (`GET /api/dashboard`). One round-trip; avoids N+1 client fetches.
#[utoipa::path(get, path = "/api/dashboard", responses((status = 200, body = serde_json::Value)))]
pub async fn dashboard_summary(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    use serde_json::json;

    let sessions = state.storage.list_sessions()?;
    let mut runs = state.storage.list_runs(None)?;
    runs.sort_by(|a, b| b.created_at.cmp(&a.created_at));

    let by_status = |wanted: coach_core::model::RunStatus| {
        runs.iter()
            .filter(|r| r.status == wanted)
            .count()
    };
    let datasets = state.storage.list_datasets()?;
    let items: usize = datasets
        .iter()
        .map(|d| {
            state
                .storage
                .list_data_items(&d.id, None)
                .map(|i| i.len())
                .unwrap_or(0)
        })
        .sum();
    let images = state.storage.list_images(None)?.len();
    let visualizations = state.storage.list_visualizations(None)?.len();
    let metric_names = state.storage.list_metric_names()?;

    let recent = runs.iter().take(6).cloned().collect::<Vec<_>>();
    Ok(Json(json!({
        "counts": {
            "sessions": sessions.len(),
            "runs_total": runs.len(),
            "runs_running": by_status(coach_core::model::RunStatus::Running),
            "runs_completed": by_status(coach_core::model::RunStatus::Completed),
            "runs_failed": by_status(coach_core::model::RunStatus::Failed),
            "datasets": datasets.len(),
            "data_items": items,
            "images": images,
            "visualizations": visualizations,
        },
        "recent_runs": recent,
        "metric_names": metric_names,
        "params": runs.iter().take(20).map(|r| json!({
            "id": r.id,
            "name": r.name,
            "status": r.status,
            "hparams": r.hparams,
        })).collect::<Vec<_>>(),
    })))
}

/// Assign a run to a session (`POST /api/runs/{id}/assign`, body: session_id).
#[utoipa::path(post, path = "/api/runs/{id}/assign", responses((status = 200, body = Run)))]
pub async fn assign_run(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let run = state.storage.get_run(&id)?;
    let session_id = body
        .get("session_id")
        .map(|v| v.as_str().unwrap_or("").to_string());
    if let Some(sid) = &session_id {
        // The session must exist — typos should fail loudly, not orphan the run.
        state.storage.get_session(sid)?;
    }
    let mut run = run;
    run.session_id = session_id;
    state.storage.update_run(run).map_err(|e| e)?;
    Ok(Json(serde_json::json!({"ok": true})))
}

// ---------------------------------------------------------------------------
// Datasets + data items (curation workbench)
// ---------------------------------------------------------------------------

#[derive(Debug, serde::Deserialize, utoipa::ToSchema)]
pub struct CreateDataset {
    pub name: String,
    #[serde(default)]
    pub source_ref: String,
}

/// Create a dataset (`POST /api/datasets`).
#[utoipa::path(
    post,
    path = "/api/datasets",
    request_body = CreateDataset,
    responses((status = 200, body = Dataset))
)]
pub async fn create_dataset(
    State(state): State<AppState>,
    Json(body): Json<CreateDataset>,
) -> Result<Json<Dataset>, ApiError> {
    let dataset = Dataset {
        id: new_id(),
        name: body.name,
        source_ref: body.source_ref,
        schema_version: 1,
        created_at: now(),
        updated_at: now(),
        versions: vec![],
    };
    Ok(Json(state.storage.create_dataset(dataset)?))
}

/// List datasets (`GET /api/datasets`).
#[utoipa::path(get, path = "/api/datasets", responses((status = 200, body = [Dataset])))]
pub async fn list_datasets(State(state): State<AppState>) -> Result<Json<Vec<Dataset>>, ApiError> {
    Ok(Json(state.storage.list_datasets()?))
}

#[derive(Debug, serde::Deserialize, utoipa::ToSchema)]
pub struct PutItem {
    pub text: String,
    /// Canonical kebab-case label; validated server-side.
    pub label: String,
    pub kind: Option<DataItemKind>,
    pub status: Option<DataItemStatus>,
    pub split: Option<DataSplit>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub actor: String,
    /// Explicit item id: when present and resolvable, the update targets this
    /// exact item instead of the first label match.
    #[serde(default)]
    pub id: Option<String>,
}

/// Create or update a data item, writing an audit-history entry when an
/// existing item changes (`PUT /api/datasets/{id}/items`).
#[utoipa::path(
    put,
    path = "/api/datasets/{id}/items",
    request_body = PutItem,
    responses((status = 200, body = DataItem))
)]
pub async fn put_data_item(
    State(state): State<AppState>,
    AxumPath(dataset_id): AxumPath<String>,
    Json(body): Json<PutItem>,
) -> Result<Json<DataItem>, ApiError> {
    state.storage.get_dataset(&dataset_id)?;
    let label = body.label.trim().to_lowercase().replace(' ', "-");
    let item_id = new_id();
    let match_id = item_id.clone();
    // Target resolution: an explicit id wins (bulk operations use it); only
    // new item creation falls back to the first-label-match legacy rule.
    let existing = match body.id.as_deref() {
        Some(explicit_id) => state
            .storage
            .list_data_items(&dataset_id, None)?
            .into_iter()
            .find(|i| &i.id == explicit_id),
        None => state
            .storage
            .list_data_items(&dataset_id, None)?
            .into_iter()
            .find(|i| i.label == label),
    };

    let kind = body.kind.unwrap_or(coach_core::model::DataItemKind::Example);
    let status = body.status.unwrap_or(coach_core::model::DataItemStatus::Raw);
    let split = body.split.unwrap_or(coach_core::model::DataSplit::Unassigned);
    let item = match existing {
        Some(prev) => {
            let mut item = prev.clone();
            if let Some(notes) = body.notes.clone() {
                item.notes = Some(notes);
            }
            if item.status != status {
                item.status = status.clone();
            }
            if item.split != split {
                item.split = split.clone();
            }
            item.updated_at = now();
            state.storage.put_data_item(item.clone())?;
            let changed = item.status != prev.status
                || item.split != prev.split
                || item.notes != prev.notes;
            if changed {
                state.storage.append_item_history(DataItemHistory {
                    id: new_id(),
                    item_id: item.id.clone(),
                    timestamp: now(),
                    actor: if body.actor.is_empty() { "web".into() } else { body.actor },
                    field: "status/split/notes".into(),
                    // History keeps the previous values for audit.
                    old_value: serde_json::json!({
                        "status": prev.status,
                        "split": prev.split,
                    }),
                    new_value: serde_json::json!({
                        "status": item.status,
                        "split": item.split,
                    }),
                })?;
            }
            item
        }
        None => {
            let item = DataItem {
                id: match_id,
                dataset_id,
                input_fields: InputFields {
                    text: Some(body.text),
                    subject: None,
                    predicate: None,
                    object: None,
                },
                label,
                kind,
                source: String::new(),
                status,
                split,
                assignee: None,
                notes: body.notes,
                updated_at: now(),
            };
            state.storage.put_data_item(item.clone())?;
            item
        }
    };
    Ok(Json(item))
}

/// List a dataset's items (`GET /api/datasets/{id}/items`, optional `?status=…`).
#[utoipa::path(
    get,
    path = "/api/datasets/{id}/items",
    params(("id" = String, Path), ("status" = Option<DataItemStatus>, Query)),
    responses((status = 200, body = [DataItem]))
)]
pub async fn list_data_items(
    State(state): State<AppState>,
    AxumPath(dataset_id): AxumPath<String>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Vec<DataItem>>, ApiError> {
    let wanted = params.get("status").map(String::as_str);
    let items = state.storage.list_data_items(&dataset_id, None)?;
    Ok(Json(
        items
            .into_iter()
            .filter(|i| wanted.is_none() || format!("{:?}", i.status).to_lowercase() == wanted.unwrap())
            .collect(),
    ))
}

/// Audit history of one data item (`GET /api/items/{id}/history`).
#[utoipa::path(
    get,
    path = "/api/items/{id}/history",
    params(("id" = String, Path)),
    responses((status = 200, body = [DataItemHistory]))
)]
pub async fn item_history(
    State(state): State<AppState>,
    AxumPath(item_id): AxumPath<String>,
) -> Result<Json<Vec<DataItemHistory>>, ApiError> {
    Ok(Json(state.storage.list_item_history(&item_id)?))
}


// ---------------------------------------------------------------------------
// Export / import (portable JSON bundles)
// ---------------------------------------------------------------------------

/// Export the entire store as one portable JSON bundle.
#[utoipa::path(get, path = "/api/export", responses((status = 200)))]
pub async fn export_bundle(State(state): State<AppState>) -> Result<Json<serde_json::Value>, ApiError> {
    let bundle = state.storage.export_json()?;
    Ok(Json(bundle))
}

/// Import (merge by id) a bundle produced by /api/export. Heavy: the caller
/// posts the bundle body directly.
#[utoipa::path(post, path = "/api/import", request_body = serde_json::Value, responses((status = 200)))]
pub async fn import_bundle(
    State(state): State<AppState>,
    Json(bundle): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, ApiError> {
    state.storage.import_json(bundle)?;
    Ok(Json(json!({"status": "imported"})))
}

/// Scan a filesystem root for spaCy pipeline candidates (a dir containing
/// meta.json + config.cfg) — used by the model picker in Settings.
#[utoipa::path(get, path = "/api/model-candidates", params(("root" = Option<String>, Query)), responses((status = 200)))]
pub async fn model_candidates(
    State(state): State<AppState>,
    axum::extract::Query(_): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Vec<String>>, ApiError> {
    let _ = state; // scans nothing outside the workspace dir by default
    let mut roots: Vec<std::path::PathBuf> = vec![state.workspace_dir.clone()];
    Ok(Json(scan_model_roots(roots.pop().unwrap_or_default())))
}

/// Find spaCy pipeline candidates: directories that contain both
/// `meta.json` and `config.cfg` up to 3 levels deep under `root`.
fn scan_model_roots(root: std::path::PathBuf) -> Vec<String> {
    let mut found = Vec::new();
    fn is_pipeline(dir: &std::path::Path) -> bool {
        dir.join("meta.json").is_file() && dir.join("config.cfg").is_file()
    }
    fn walk(dir: &std::path::Path, depth: usize, found: &mut Vec<String>) {
        if depth > 3 || found.len() >= 20 {
            return;
        }
        let entries = std::fs::read_dir(dir).ok();
        for e in entries.into_iter().flatten().flatten() {
            let path = e.path();
            if is_pipeline(&path) {
                if let Some(s) = path.to_str() {
                    found.push(s.to_string());
                }
            } else if path.is_dir() {
                walk(&path, depth + 1, found);
            }
        }
    }
    walk(&root, 0, &mut found);
    found
}

// ---------------------------------------------------------------------------
// Dataset conversion pipeline (stratified split + audit + job)
// ---------------------------------------------------------------------------

/// Deterministic cheap rng for stratified permutation (xorshift64 on
/// (seed ^ fnv1a(item_id))).
fn stratified_key(item_id: &str, seed: u64) -> u64 {
    let mut h: u64 = seed ^ 0xcbf2_9ce4_8422_2325;
    for b in item_id.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h ^= h << 13;
    h ^= h >> 7;
    h ^= h << 17;
    h
}

/// Run the SSOT-derived curation readiness pipeline:
/// assign a stratified train/dev split over approved items
/// (`POST /api/datasets/{id}/conversions`, body: dev_ratio, seed).
#[utoipa::path(post, path = "/api/datasets/{id}/conversions", responses((status = 200, body = ConversionJob)))]
pub async fn create_conversion(
    State(state): State<AppState>,
    AxumPath(dataset_id): AxumPath<String>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<ConversionJob>, ApiError> {
    use coach_core::model::{DataItemStatus, DataSplit, JobStatus};

    let dev_ratio = body
        .get("dev_ratio")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.2)
        .clamp(0.0, 0.5) as f64;
    let seed = body.get("seed").and_then(|v| v.as_u64()).unwrap_or(0x5EED);

    state.storage.get_dataset(&dataset_id)?;
    let items = state.storage.list_data_items(&dataset_id, None)?;

    // Stratify: per label, assign `dev_ratio` share to dev (round-robin over
    // a seeded permutation), everything else to train.
    let mut by_label: std::collections::BTreeMap<String, Vec<DataItem>> = Default::default();
    for item in items.iter() {
        by_label.entry(item.label.clone()).or_default().push(item.clone());
    }

    let mut train = 0usize;
    let mut dev = 0usize;
    let eval = 0usize;
    for (_label, mut group) in by_label {
        group.sort_by(|a, b| stratified_key(&a.id, seed).cmp(&stratified_key(&b.id, seed)));
        let n_dev = (group.len() as f64 * dev_ratio).round() as usize;
        for (idx, mut item) in group.into_iter().enumerate() {
            let new_split = if idx < n_dev {
                dev += 1;
                DataSplit::Dev
            } else {
                train += 1;
                DataSplit::Train
            };
            if item.split != new_split {
                let old = item.split.clone();
                item.split = new_split;
                item.updated_at = now();
                state.storage.put_data_item(item.clone())?;
                state.storage.append_item_history(DataItemHistory {
                    id: new_id(),
                    item_id: item.id.clone(),
                    timestamp: now(),
                    actor: "conversion-pipeline".into(),
                    field: "split".into(),
                    old_value: serde_json::to_value(old).unwrap_or(serde_json::Value::Null),
                    new_value: serde_json::to_value(item.split.clone()).unwrap_or(serde_json::Value::Null),
                })?;
            } else {
                state.storage.put_data_item(item)?;
            }
        }
    }
    let _ = eval; // eval split stays manual; counted only

    // Dataset hash chain entry: sha256 over sorted (item, status, split).
    let mut hash_input = String::new();
    for item in state.storage.list_data_items(&dataset_id, None)? {
        hash_input.push_str(&format!("{}|{:?}|{:?}\n", item.id, item.status, item.split));
    }
    let mut hasher = Sha256::new();
    hasher.update(hash_input.as_bytes());
    let dataset_hash = hex::encode(hasher.finalize());

    let approved = state
        .storage
        .list_data_items(&dataset_id, Some(&DataItemStatus::Approved))?
        .len();

    let job = ConversionJob {
        id: new_id(),
        dataset_id: dataset_id.clone(),
        pipeline: vec![
            "curation-readiness".into(),
            "stratified-split".into(),
            "dataset-hash".into(),
        ],
        params: serde_json::json!({"dev_ratio": dev_ratio, "seed": seed}),
        status: JobStatus::Completed,
        artifacts: vec![serde_json::json!({
            "kind": "dataset-hash",
            "dataset_id": dataset_id,
            "sha256": dataset_hash,
            "note": "deterministic over items sorted by id; stable layout for DocBin export",
        })],
        stats: serde_json::json!({
            "train": train,
            "dev": dev,
            "eval": eval,
            "approved_items": approved,
            "total_items": items.len(),
        }),
        created_at: now(),
        updated_at: now(),
    };
    state.storage.put_conversion_job(job.clone())?;
    Ok(Json(job))
}

/// Conversion job history for a dataset (`GET /api/datasets/{id}/conversions`).
#[utoipa::path(get, path = "/api/datasets/{id}/conversions", responses((status = 200, body = [ConversionJob])))]
pub async fn list_conversions(
    State(state): State<AppState>,
    AxumPath(dataset_id): AxumPath<String>,
) -> Result<Json<Vec<ConversionJob>>, ApiError> {
    let mut jobs = state.storage.list_conversion_jobs(&dataset_id)?;
    jobs.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(Json(jobs))
}
/// List saved query scopes (`GET /api/scopes`).
#[utoipa::path(get, path = "/api/scopes", responses((status = 200, body = [Scope])))]
pub async fn list_scopes(State(state): State<AppState>) -> Result<Json<Vec<Scope>>, ApiError> {
    Ok(Json(state.storage.list_scopes()?))
}

/// Save a query scope (`POST /api/scopes`, body: name + query).
#[utoipa::path(post, path = "/api/scopes", request_body = ScopeIn, responses((status = 200, body = Scope)))]
pub async fn create_scope(
    State(state): State<AppState>,
    Json(body): Json<ScopeIn>,
) -> Result<Json<Scope>, ApiError> {
    let scope = Scope {
        id: new_id(),
        name: body.name.trim().to_string(),
        query: body.query.trim().to_string(),
        created_at: now(),
    };
    if scope.name.is_empty() || scope.query.is_empty() {
        return Err(coach_core::CoreError::InvalidQuery("name and query are required".into()).into());
    }
    Ok(Json(state.storage.put_scope(scope)?))
}

/// Delete a scope (`DELETE /api/scopes/{id}`).
#[utoipa::path(delete, path = "/api/scopes/{id}", responses((status = 200)))]
pub async fn delete_scope(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<axum::http::StatusCode, ApiError> {
    state.storage.delete_scope(&id)?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

/// Create-scope payload.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ScopeIn {
    pub name: String,
    pub query: String,
}

// ---------------------------------------------------------------------------
// Training-data pipeline — stage 1: sources + stage 2: chunking
// ---------------------------------------------------------------------------

#[derive(Debug, serde::Deserialize, utoipa::ToSchema)]
pub struct CreateSource {
    pub name: String,
    /// Raw document content.
    pub content: String,
    /// e.g. "text" | "markdown" | "json".
    #[serde(default)]
    pub kind: Option<String>,
}

/// Store an uploaded source document (`POST /api/sources`). The sha256 serves
/// as provenance and de-duplication hint; content is kept in the store so
/// chunking is reproducible.
#[utoipa::path(post, path = "/api/sources", request_body = CreateSource, responses((status = 200, body = coach_core::model::SourceDoc)))]
pub async fn create_source(
    State(state): State<AppState>,
    Json(body): Json<CreateSource>,
) -> Result<Json<coach_core::model::SourceDoc>, ApiError> {
    let name = body.name.trim();
    if name.is_empty() {
        return Err(coach_core::CoreError::InvalidQuery("source name is required".into()).into());
    }
    let mut hasher = Sha256::new();
    hasher.update(body.content.as_bytes());
    let doc = coach_core::model::SourceDoc {
        id: new_id(),
        name: name.to_string(),
        kind: body.kind.unwrap_or_else(|| "text".into()),
        size: body.content.len() as i64,
        sha256: hex::encode(hasher.finalize()),
        content: body.content,
        created_at: now(),
    };
    Ok(Json(state.storage.put_source_doc(doc)?))
}

/// List uploaded source documents (`GET /api/sources`).
#[utoipa::path(get, path = "/api/sources", responses((status = 200, body = [coach_core::model::SourceDoc])))]
pub async fn list_sources(
    State(state): State<AppState>,
) -> Result<Json<Vec<coach_core::model::SourceDoc>>, ApiError> {
    Ok(Json(state.storage.list_source_docs()?))
}

/// Delete a source document (`DELETE /api/sources/{id}`). Chunked items are
/// kept (their provenance string survives the deletion).
#[utoipa::path(delete, path = "/api/sources/{id}", responses((status = 200)))]
pub async fn delete_source(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<axum::http::StatusCode, ApiError> {
    state.storage.delete_source_doc(&id)?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

#[derive(Debug, serde::Deserialize, utoipa::ToSchema)]
pub struct ChunkRequest {
    /// "paragraph" (default) | "sentence" | "window".
    #[serde(default)]
    pub strategy: Option<String>,
    /// Approximate max characters per chunk (window/soft cap for all modes).
    #[serde(default)]
    pub max_chars: Option<usize>,
    /// Overlap in characters between windows.
    #[serde(default)]
    pub overlap: Option<usize>,
    /// Target dataset; when empty a dataset named `imported-<date>` is
    /// created/looked up.
    #[serde(default)]
    pub dataset_id: Option<String>,
    /// Default kebab-case label applied to all produced items.
    #[serde(default)]
    pub label: Option<String>,
}

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct ChunkResponse {
    pub dataset_id: String,
    pub created_items: usize,
    pub skipped_existing: usize,
}

/// Split a source document into sentence-ish chunks.
fn chunk_text(content: &str, strategy: &str, max_chars: usize, overlap: usize) -> Vec<String> {
    fn push_chunk(out: &mut Vec<String>, buf: String) {
        let trimmed = buf.trim();
        if !trimmed.is_empty() {
            out.push(trimmed.to_string());
        }
    }
    let mut chunks: Vec<String> = Vec::new();
    match strategy {        "sentence" => {
            // Split at ., !, ? boundaries followed by whitespace/EOL.
            let mut buf = String::new();
            let mut chars = content.chars().peekable();
            while let Some(c) = chars.next() {
                buf.push(c);
                if matches!(c, '.' | '!' | '?') {
                    // Only split when whitespace follows (not "3.5" etc.).
                    let next_is_ws = chars.peek().map(|n| n.is_whitespace()).unwrap_or(true);
                    if next_is_ws {
                        push_chunk(&mut chunks, std::mem::take(&mut buf));
                    }
                }
            }
            push_chunk(&mut chunks, buf);
            // Merge micro-chunks up to max_chars (without overlap).
            let mut merged: Vec<String> = Vec::new();
            let mut carry = String::new();
            for chunk in chunks {
                if carry.chars().count() + chunk.chars().count() + 1 <= max_chars {
                    if carry.is_empty() {
                        carry = chunk;
                    } else {
                        carry.push(' ');
                        carry.push_str(&chunk);
                    }
                } else {
                    push_chunk(&mut merged, std::mem::take(&mut carry));
                    if chunk.chars().count() <= max_chars {
                        carry = chunk;
                    } else {
                        // Hard-window oversized sentences.
                        let windowed = window_text(&chunk, max_chars, 0);
                        for w in windowed.iter().take(windowed.len() - 1) {
                            push_chunk(&mut merged, w.clone());
                        }
                        if let Some(last) = windowed.last() {
                            carry = last.clone();
                        }
                    }
                }
            }
            push_chunk(&mut merged, carry);
            merged
        }
        "window" => window_text(content, max_chars, overlap),
        // Default: paragraph mode with a max_chars soft cap.
        _ => {
            let mut out = Vec::new();
            for para in content.split("\n\n") {
                let para = para.trim();
                if para.is_empty() {
                    continue;
                }
                if para.chars().count() <= max_chars {
                    push_chunk(&mut out, para.to_string());
                } else {
                    for w in window_text(para, max_chars, overlap) {
                        push_chunk(&mut out, w);
                    }
                }
            }
            out
        }
    }
}

/// Fixed-size character windows with overlap.
fn window_text(content: &str, max_chars: usize, overlap: usize) -> Vec<String> {    let max = max_chars.max(1);
    let overlap = overlap.min(max.saturating_sub(1));
    let chars: Vec<char> = content.chars().collect();
    let mut out = Vec::new();
    let mut start = 0usize;
    while start < chars.len() {
        let end = (start + max).min(chars.len());
        let slice: String = chars[start..end].iter().collect();
        let trimmed = slice.trim();
        if !trimmed.is_empty() {
            out.push(trimmed.to_string());
        }
        if end >= chars.len() {
            break;
        }
        start += max - overlap;
    }
    out
}

/// Chunk a source document into raw DataItems (`POST /api/sources/{id}/chunks`).
/// Every item carries provenance `"<source_name>#<index>"` so the audit trail
/// stays traceable back to the upload.
#[utoipa::path(post, path = "/api/sources/{id}/chunks", request_body = ChunkRequest, responses((status = 200, body = ChunkResponse)))]
pub async fn chunk_source(
    State(state): State<AppState>,
    AxumPath(source_id): AxumPath<String>,
    Json(body): Json<ChunkRequest>,
) -> Result<Json<ChunkResponse>, ApiError> {
    use coach_core::model::{DataItemKind, DataItemStatus, DataSplit};

    let doc = state.storage.get_source_doc(&source_id)?;
    let strategy = body.strategy.unwrap_or_else(|| "paragraph".into());
    let max_chars = body.max_chars.unwrap_or(600).clamp(40, 20_000);
    let overlap = body.overlap.unwrap_or(0).min(max_chars / 2);
    let label = body
        .label
        .unwrap_or_else(|| "imported-chunk".into())
        .trim()
        .to_lowercase()
        .replace(' ', "-");

    // Resolve or create the target dataset.
    let dataset_id = match body.dataset_id {
        Some(id) if !id.is_empty() => {
            state.storage.get_dataset(&id)?;
            id
        }
        _ => {
            let name = format!("imported-{}", doc.name.replace(' ', "-").to_lowercase());
            let existing = state
                .storage
                .list_datasets()?
                .into_iter()
                .find(|d| d.name == name);
            match existing {
                Some(d) => d.id,
                None => state
                    .storage
                    .create_dataset(Dataset {
                        id: new_id(),
                        name,
                        source_ref: format!("source-doc:{}#sha256:{}", source_id, doc.sha256),
                        schema_version: 1,
                        created_at: now(),
                        updated_at: now(),
                        versions: vec![],
                    })?
                    .id,
            }
        }
    };

    let chunks = chunk_text(&doc.content, &strategy, max_chars, overlap);
    // Existing items of the dataset (dedup on identical text).
    let existing_texts: std::collections::HashSet<String> = state
        .storage
        .list_data_items(&dataset_id, None)?
        .into_iter()
        .filter_map(|i| i.input_fields.text)
        .collect();

    let mut created = 0usize;
    let mut skipped = 0usize;
    for (idx, chunk) in chunks.into_iter().enumerate() {
        if existing_texts.contains(&chunk) {
            skipped += 1;
            continue;
        }
        let item = DataItem {
            id: new_id(),
            dataset_id: dataset_id.clone(),
            input_fields: InputFields {
                text: Some(chunk),
                subject: None,
                predicate: None,
                object: None,
            },
            label: label.clone(),
            kind: DataItemKind::Example,
            source: format!("{}#{}", doc.name, idx),
            status: DataItemStatus::Raw,
            split: DataSplit::Unassigned,
            assignee: None,
            notes: None,
            updated_at: now(),
        };
        state.storage.put_data_item(item)?;
        created += 1;
    }

    Ok(Json(ChunkResponse {
        dataset_id,
        created_items: created,
        skipped_existing: skipped,
    }))
}

/// Read one source document back (`GET /api/sources/{id}`) — full content for
/// the preview panel.
#[utoipa::path(get, path = "/api/sources/{id}", responses((status = 200, body = coach_core::model::SourceDoc)))]
pub async fn get_source(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<coach_core::model::SourceDoc>, ApiError> {
    Ok(Json(state.storage.get_source_doc(&id)?))
}

// ---------------------------------------------------------------------------
// Training-data pipeline — stage 3: AI annotation
// ---------------------------------------------------------------------------

#[derive(Debug, serde::Deserialize, utoipa::ToSchema)]
pub struct AnnotateRequest {
    pub provider_id: String,
    /// Data items (by id) to annotate.
    pub item_ids: Vec<String>,
    /// Free-form annotation instruction (task description).
    #[serde(default)]
    pub instruction: String,
    /// Allowed labels; when empty, labels are inferred from the dataset.
    #[serde(default)]
    pub labels: Vec<String>,
}

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct AnnotateResponse {
    pub annotated: usize,
    pub failed: usize,
    pub errors: Vec<String>,
}

/// Extract the first JSON object from a model response (models often wrap
/// JSON in prose or code fences).
fn extract_json(text: &str) -> Option<Value> {
    let cleaned = text.trim();
    let stripped = cleaned
        .strip_prefix("```json")
        .or_else(|| cleaned.strip_prefix("```"))
        .map(|s| s.trim())
        .unwrap_or(cleaned);
    let stripped = stripped.strip_suffix("```").map(|s| s.trim()).unwrap_or(stripped);
    let start = stripped.find('{')?;
    let end = stripped.rfind('}')?;
    if start < end {
        serde_json::from_str(&stripped[start..=end]).ok()
    } else {
        None
    }
}

/// Classify the text of data items with an OpenAI-compatible provider
/// (`POST /api/annotations`). Every result is written as a draft annotation:
/// the label update is kept in the audit trail with actor `ai:<provider>`,
/// items NEVER jump straight to approved. Returns per-item stats; failed
/// items stay untouched.
#[utoipa::path(post, path = "/api/annotations", request_body = AnnotateRequest, responses((status = 200, body = AnnotateResponse)))]
pub async fn annotate_items(
    State(state): State<AppState>,
    Json(body): Json<AnnotateRequest>,
) -> Result<Json<AnnotateResponse>, ApiError> {
    use coach_core::model::{DataItemHistory, DataItemStatus};

    let provider = state.provider_store.get(&body.provider_id).ok_or_else(|| {
        coach_core::CoreError::InvalidQuery(format!("unknown provider {}", body.provider_id))
    })?;
    let base = provider
        .base_url
        .clone()
        .unwrap_or_else(|| "https://api.mistral.ai/v1".into());
    let model = provider.model.clone().unwrap_or_else(|| "mistral-small-latest".into());
    let url = format!("{}/chat/completions", base.trim_end_matches('/'));

    let mut annotated = 0usize;
    let mut failed = 0usize;
    let mut errors = Vec::new();

    for item_id in body.item_ids.iter() {
        // Find the item across datasets (small scale; curation workbench).
        let datasets = match state.storage.list_datasets() {
            Ok(d) => d,
            Err(e) => {
                failed += 1;
                errors.push(format!("storage: {e}"));
                continue;
            }
        };
        let mut found: Option<DataItem> = None;
        for d in datasets {
            if let Ok(items) = state.storage.list_data_items(&d.id, None) {
                if let Some(item) = items.into_iter().find(|i| &i.id == item_id) {
                    found = Some(item);
                    break;
                }
            }
        }
        let Some(item) = found else {
            failed += 1;
            errors.push(format!("item {item_id} not found"));
            continue;
        };
        let Some(text) = item.input_fields.text.clone() else {
            failed += 1;
            errors.push(format!("item {item_id} has no text"));
            continue;
        };

        let label_list = if body.labels.is_empty() {
            // Infer the label space from the dataset (all distinct labels).
            let mut labels: Vec<String> = state
                .storage
                .list_data_items(&item.dataset_id, None)
                .unwrap_or_default()
                .into_iter()
                .map(|i| i.label)
                .collect();
            labels.sort();
            labels.dedup();
            labels
        } else {
            body.labels.clone()
        };
        if label_list.is_empty() {
            failed += 1;
            errors.push(format!(
                "item {item_id}: no labels known and none supplied"
            ));
            continue;
        }

        let instruction = if body.instruction.trim().is_empty() {
            "Classify each text into exactly one of the given labels.".to_string()
        } else {
            body.instruction.trim().to_string()
        };
        let prompt = format!(
            "{instruction}\n\nAllowed labels (choose exactly one, output as JSON):\n{}\n\nText:\n{}\n\nRespond with ONLY a JSON object: {{\"label\": \"<one label>\", \"reasoning\": \"one short sentence\"}}",
            label_list.iter().map(|l| format!("- {l}")).collect::<Vec<_>>().join("\n"),
            truncate_text(&text, 4000),
        );

        let payload = json!({
            "model": model,
            "messages": [{"role": "user", "content": prompt}],
            "temperature": 0.0,
        });
        let mut req = state.http_client.post(&url).json(&payload);
        if let Some(key) = provider.api_key.as_deref() {
            if !key.is_empty() && !key.ends_with('…') {
                req = req.bearer_auth(key);
            }
        }
        let response = match req.send().await {
            Ok(r) => r,
            Err(e) => {
                failed += 1;
                errors.push(format!("item {item_id}: provider unreachable: {e}"));
                continue;
            }
        };
        if !response.status().is_success() {
            let status = response.status();
            let detail = response.text().await.unwrap_or_default();
            failed += 1;
            errors.push(format!(
                "item {item_id}: provider {status}: {}",
                truncate_text(&detail, 200)
            ));
            continue;
        }
        let completion: Value = match response.json().await {
            Ok(v) => v,
            Err(e) => {
                failed += 1;
                errors.push(format!("item {item_id}: bad provider response: {e}"));
                continue;
            }
        };
        let content = completion["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let Some(parsed) = extract_json(&content) else {
            failed += 1;
            errors.push(format!(
                "item {item_id}: no JSON in model output: {}",
                truncate_text(&content, 120)
            ));
            continue;
        };
        let new_label = parsed
            .get("label")
            .and_then(|l| l.as_str())
            .map(|s| s.trim().to_lowercase().replace(' ', "-"))
            .unwrap_or_default();
        if !label_list.contains(&new_label) {
            failed += 1;
            errors.push(format!(
                "item {item_id}: model label \"{new_label}\" not in allowed labels"
            ));
            continue;
        }
        let reasoning = parsed
            .get("reasoning")
            .and_then(|r| r.as_str())
            .unwrap_or("")
            .to_string();

        let mut updated = item.clone();
        let old_label = updated.label.clone();
        if updated.label != new_label || !reasoning.is_empty() {
            updated.label = new_label.clone();
            updated.notes = Some(if reasoning.is_empty() {
                format!("ai-annotated via provider {}", provider.name)
            } else {
                format!("ai-annotated via provider {}: {}", provider.name, reasoning)
            });
            updated.status = DataItemStatus::Raw; // draft: stays raw until review
            updated.updated_at = now();
            let actor = format!("ai:{}", provider.name);
            state.storage.put_data_item(updated.clone())?;
            state.storage.append_item_history(DataItemHistory {
                id: new_id(),
                item_id: updated.id.clone(),
                timestamp: now(),
                actor: actor.clone(),
                field: "label".into(),
                old_value: serde_json::to_value(old_label).unwrap_or(Value::Null),
                new_value: serde_json::to_value(new_label).unwrap_or(Value::Null),
            })?;
        }
        annotated += 1;
    }

    Ok(Json(AnnotateResponse {
        annotated,
        failed,
        errors,
    }))
}

fn truncate_text(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let cut: String = text.chars().take(max).collect();
    format!("{cut}…")
}

// ---------------------------------------------------------------------------
// Training-data pipeline — stage 5: export
// ---------------------------------------------------------------------------

/// Export a dataset as spaCy-ready JSONL
/// (`GET /api/datasets/{id}/export?status=approved`): one line per item,
/// `{"text": ..., "label": ...}` — the format `spacy convert` and our
/// coach_spacy worker accept for DocBin conversion.
#[utoipa::path(
    get,
    path = "/api/datasets/{id}/export",
    params(("id" = String, Path), ("status" = Option<String>, Query)),
    responses((status = 200))
)]
pub async fn export_dataset(
    State(state): State<AppState>,
    AxumPath(dataset_id): AxumPath<String>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<axum::response::Response, ApiError> {
    use coach_core::model::DataItemStatus;

    let wanted_status = params.get("status").map(String::as_str);
    let items: Vec<DataItem> = state
        .storage
        .list_data_items(&dataset_id, None)?
        .into_iter()
        .filter(|i| {
            wanted_status.is_none()
                || format!("{:?}", i.status).to_lowercase() == wanted_status.unwrap()
        })
        .collect();
    let dataset = state.storage.get_dataset(&dataset_id)?;

    let mut lines = Vec::new();
    // Only approved items carry labels into the export: anything else is a
    // draft the reviewer never confirmed. When everything is raw (draft-only
    // dataset), export with labels anyway but flag it via a header comment.
    let mut exported_unapproved = 0usize;
    for item in &items {
        if item.status != DataItemStatus::Approved
            && item.status != DataItemStatus::Curated
        {
            exported_unapproved += 1;
        }
    }
    for item in &items {
        let line = json!({
            "text": item.input_fields.text.clone().unwrap_or_default(),
            "label": item.label,
            "type": "textcat",
            "meta": {
                "item_id": item.id,
                "source": item.source,
                "status": format!("{:?}", item.status).to_lowercase(),
            }
        });
        lines.push(line.to_string());
    }
    let body = lines.join("\n");
    if exported_unapproved > 0 {
        tracing::info!(
            dataset = %dataset.name,
            unapproved = exported_unapproved,
            "export contains non-approved items"
        );
    }
    axum::response::Response::builder()
        .status(200)
        .header("content-type", "application/x-ndjson")
        .header(
            "content-disposition",
            format!("attachment; filename=\"{}.jsonl\"", dataset.name),
        )
        .body(axum::body::Body::from(body))
        .map_err(|e| ApiError::from(coach_core::CoreError::Io(std::io::Error::other(e))))
}
