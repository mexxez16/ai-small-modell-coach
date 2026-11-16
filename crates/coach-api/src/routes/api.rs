use axum::{extract::{Path as AxumPath, State}, Json};
use coach_core::model::{new_id, now, Dataset, Image, Run, RunStatus, Session, SessionStatus};
use coach_core::model::{
    DataItem, DataItemHistory, DataItemKind, DataItemStatus, DataSplit, InputFields, Visualization,
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
    state.mark_dirty();
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
    state.mark_dirty();
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
    state.mark_dirty();
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
    state.mark_dirty();
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
    state.mark_dirty();
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
    state.mark_dirty();
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
    state.mark_dirty();
    Ok(Json(state.storage.create_dataset(dataset)?))
}

/// List datasets (`GET /api/datasets`).
#[utoipa::path(get, path = "/api/datasets", responses((status = 200, body = [Dataset])))]
pub async fn list_datasets(State(state): State<AppState>) -> Result<Json<Vec<Dataset>>, ApiError> {
    Ok(Json(state.storage.list_datasets()?))
}

#[derive(Debug, serde::Deserialize, utoipa::ToSchema)]
pub struct PutItem {
    /// Text payload of the item.
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
    let existing = state
        .storage
        .list_data_items(&dataset_id, None)?
        .into_iter()
        .find(|i| i.label == label);

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
    state.mark_dirty();
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
    use std::path::PathBuf;
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
