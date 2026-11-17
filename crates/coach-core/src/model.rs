//! Domain model for AI Small Modell Coach. See docs/DATA_MODEL.md for the
//! authoritative description of each entity and its lifecycle.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub fn now() -> DateTime<Utc> {
    Utc::now()
}

pub fn new_id() -> String {
    Uuid::new_v4().to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Active,
    Archived,
}

/// A training session groups one or more runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Session {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub status: SessionStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Running,
    Completed,
    Failed,
}

/// A single training run.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Run {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub session_id: Option<String>,
    pub status: RunStatus,
    /// Interpolated training config (JSON).
    #[serde(default)]
    pub config: Option<serde_json::Value>,
    #[serde(default)]
    pub hparams: serde_json::Value,
    #[serde(default)]
    pub dataset_id: Option<String>,
    /// sha256 of the dataset snapshot used by this run, if any.
    #[serde(default)]
    pub dataset_hash: Option<String>,
    /// Model (name or pipeline path) this run trains.
    #[serde(default)]
    pub model: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub tags: Vec<String>,
}

/// One metric sample. `name` plus `context` identifies the metric series
/// (e.g. `textcat_loss` with `{"type": "loss"}` or `other_scores` entries).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct MetricPoint {
    pub run_id: String,
    pub name: String,
    #[serde(default)]
    pub context: serde_json::Value,
    pub step: u64,
    #[serde(default)]
    pub epoch: Option<u64>,
    pub value: f64,
    pub timestamp: DateTime<Utc>,
}

/// An image artifact attached to a run (PNG bytes) — training-time displaCy
/// renders live here.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Image {
    pub id: String,
    pub run_id: Option<String>,
    pub session_id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub step: Option<u64>,
    #[serde(default)]
    pub epoch: Option<u64>,
    /// Base64-encoded PNG bytes.
    pub png_base64: String,
    #[serde(default)]
    pub caption: String,
    #[serde(default)]
    pub meta: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

/// An ad-hoc or run-attached visualization (raw SVG text from the render
/// worker).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Visualization {
    pub id: String,
    #[serde(default)]
    pub run_id: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    pub worker_model: String,
    /// Optional source: raw text, or a dataset item id.
    pub text: Option<String>,
    pub dataset_item_id: Option<String>,
    /// "ent" | "dep" | "span"
    pub style: String,
    #[serde(default)]
    pub options: serde_json::Value,
    pub svg: String,
    pub created_at: DateTime<Utc>,
}

/// A curated dataset with versions (hash chain).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Dataset {
    pub id: String,
    pub name: String,
    /// Provenance, e.g. "ssot/BFO_Entity_Classification.json#sha256:…"
    pub source_ref: String,
    pub schema_version: u32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub versions: Vec<DatasetVersion>,
    /// Free-form grouping; the Training-Data-Sessions view uses
    /// `session:<name>` entries to group datasets.
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct DatasetVersion {
    pub hash: String,
    pub created_at: DateTime<Utc>,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum DataItemStatus {
    Raw,
    Curated,
    Approved,
    Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum DataSplit {
    Train,
    Dev,
    Eval,
    Unassigned,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum DataItemKind {
    Example,
    Negative,
    Contrastive,
    Pair,
    Other(String),
}

/// One training-data item in the curation workbench.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct DataItem {
    pub id: String,
    pub dataset_id: String,
    /// Text payload — a plain text, or subject/predicate/object for triple
    /// items (serde flattens `input_fields` under DataItem).
    #[serde(flatten)]
    pub input_fields: InputFields,
    /// Canonical, kebab-case label.
    pub label: String,
    pub kind: DataItemKind,
    /// Provenance, e.g. "BFO_Entity_Classification.json#E27".
    pub source: String,
    pub status: DataItemStatus,
    pub split: DataSplit,
    pub assignee: Option<String>,
    pub notes: Option<String>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct InputFields {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub predicate: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object: Option<String>,
}

/// Audit-trail entry for a data item.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct DataItemHistory {
    pub id: String,
    pub item_id: String,
    pub timestamp: DateTime<Utc>,
    pub actor: String,
    pub field: String,
    pub old_value: serde_json::Value,
    pub new_value: serde_json::Value,
}

/// Status of a render worker managed by coach-api. Workers are process-scoped
/// and respawned on demand; they are not persisted.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum WorkerStatus {
    Starting,
    Ready,
    Unhealthy,
    Stopped,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RenderWorker {
    pub id: String,
    pub model_path: String,
    pub port: u16,
    #[serde(default)]
    pub pid: Option<u32>,
    pub status: WorkerStatus,
    pub started_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Pending,
    Running,
    Completed,
    Failed,
}

/// An SSOT -> seed JSONL -> DocBin conversion job.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ConversionJob {
    pub id: String,
    pub dataset_id: String,
    pub pipeline: Vec<String>,
    pub params: serde_json::Value,
    pub status: JobStatus,
    #[serde(default)]
    pub artifacts: Vec<serde_json::Value>,
    #[serde(default)]
    pub stats: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// An uploaded source document awaiting chunking into data items.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SourceDoc {
    pub id: String,
    pub name: String,
    /// e.g. "text" | "json" | "markdown" (kept free-form).
    pub kind: String,
    pub size: i64,
    /// sha256 hex of the raw content (dedup/audit).
    pub sha256: String,
    pub content: String,
    pub created_at: DateTime<Utc>,
}

/// A registered model (spaCy pipeline) — either shipped (installed package),
/// a local path added by the user, or a trained pipeline that came out of a
/// run. The registry is user-managed via Settings/Models UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ModelEntry {
    pub id: String,
    pub name: String,
    /// Path or spaCy-hub name used to load the pipeline.
    pub path: String,
    /// "shipped" | "local" | "finetuned"
    pub kind: String,
    /// For kind=finetuned: the run that produced this pipeline.
    pub run_id: Option<String>,
    pub notes: String,
    pub created_at: DateTime<Utc>,
}

/// A saved AimQL query ("scope") — a named, shareable Runs-dashboard filter.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Scope {
    pub id: String,
    pub name: String,
    pub query: String,
    pub created_at: DateTime<Utc>,
}
