//! SQLite persistence backend.
//!
//! One database file (`coach.db`) in `COACH_DATA_DIR` holds every durable
//! entity. Queryable columns are typed; payload-shaped fields (configs,
//! hparams, json contexts, tags) are stored as JSON strings. The connection
//! runs in WAL mode with `synchronous=NORMAL`, so every mutation is committed
//! incrementally — no whole-store snapshot rewrites.

use crate::error::{CoreError, Result};
use crate::model::{
    ConversionJob, DataItem, DataItemHistory, DataItemStatus, DataRun, DataSession, Dataset,
    Image, MetricPoint, ModelEntry, Run, Scope, Session, SourceDoc, Visualization,
};
use crate::storage::Storage;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use std::sync::Mutex;

const SCHEMA_VERSION: i64 = 5;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    tags TEXT NOT NULL DEFAULT '[]'
);
CREATE TABLE IF NOT EXISTS runs (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    session_id TEXT,
    status TEXT NOT NULL,
    config TEXT,
    hparams TEXT NOT NULL,
    dataset_id TEXT,
    dataset_hash TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    tags TEXT NOT NULL DEFAULT '[]'
);
CREATE INDEX IF NOT EXISTS idx_runs_session ON runs(session_id);
CREATE TABLE IF NOT EXISTS metric_points (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id TEXT NOT NULL,
    name TEXT NOT NULL,
    context TEXT NOT NULL,
    step INTEGER NOT NULL,
    epoch INTEGER,
    value REAL NOT NULL,
    timestamp TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_metrics_run ON metric_points(run_id, name);
CREATE TABLE IF NOT EXISTS images (
    id TEXT PRIMARY KEY,
    run_id TEXT,
    session_id TEXT,
    name TEXT NOT NULL,
    step INTEGER,
    epoch INTEGER,
    png_base64 TEXT NOT NULL,
    caption TEXT NOT NULL DEFAULT '',
    meta TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_images_run ON images(run_id);
CREATE TABLE IF NOT EXISTS visualizations (
    id TEXT PRIMARY KEY,
    run_id TEXT,
    session_id TEXT,
    worker_model TEXT NOT NULL,
    text TEXT,
    dataset_item_id TEXT,
    style TEXT NOT NULL,
    options TEXT NOT NULL DEFAULT '{}',
    svg TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_viz_run ON visualizations(run_id);
CREATE TABLE IF NOT EXISTS datasets (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    source_ref TEXT NOT NULL,
    schema_version INTEGER NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    versions TEXT NOT NULL DEFAULT '[]'
);
CREATE TABLE IF NOT EXISTS data_items (
    id TEXT PRIMARY KEY,
    dataset_id TEXT NOT NULL,
    input_fields TEXT NOT NULL DEFAULT '{}',
    label TEXT NOT NULL,
    kind TEXT NOT NULL,
    source TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL,
    split TEXT NOT NULL,
    assignee TEXT,
    notes TEXT,
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_items_dataset ON data_items(dataset_id, status);
CREATE TABLE IF NOT EXISTS item_history (
    id TEXT PRIMARY KEY,
    item_id TEXT NOT NULL,
    timestamp TEXT NOT NULL,
    actor TEXT NOT NULL,
    field TEXT NOT NULL,
    old_value TEXT NOT NULL DEFAULT 'null',
    new_value TEXT NOT NULL DEFAULT 'null'
);
CREATE INDEX IF NOT EXISTS idx_history_item ON item_history(item_id);
CREATE TABLE IF NOT EXISTS conversion_jobs (
    id TEXT PRIMARY KEY,
    dataset_id TEXT NOT NULL,
    pipeline TEXT NOT NULL DEFAULT '[]',
    params TEXT NOT NULL DEFAULT '{}',
    status TEXT NOT NULL,
    artifacts TEXT NOT NULL DEFAULT '[]',
    stats TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_jobs_dataset ON conversion_jobs(dataset_id);
CREATE TABLE IF NOT EXISTS data_sessions (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    notes TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS data_runs (
    id TEXT PRIMARY KEY,
    session_id TEXT,
    name TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'draft',
    stages TEXT NOT NULL DEFAULT '[]',
    dataset_id TEXT,
    results TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_data_runs_session ON data_runs(session_id);
CREATE TABLE IF NOT EXISTS scopes (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    query TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS models (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    path TEXT NOT NULL,
    kind TEXT NOT NULL DEFAULT 'local',
    run_id TEXT,
    notes TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_models_run ON models(run_id);
CREATE TABLE IF NOT EXISTS source_docs (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    kind TEXT NOT NULL DEFAULT 'text',
    size INTEGER NOT NULL DEFAULT 0,
    sha256 TEXT NOT NULL DEFAULT '',
    content TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL
);
"#;

/// SQLite-backed [`Storage`] implementation. The connection sits behind a
/// mutex: `rusqlite::Connection` is not `Sync`, and the API layer is
/// single-writer anyway (WAL serializes the rest).
pub struct SqliteStore {
    conn: Mutex<Connection>,
}

impl SqliteStore {
    /// Open (creating if needed) the database at `path` and apply pragmas +
    /// schema. Parent directories are created when missing.
    pub fn open(path: &std::path::Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(CoreError::Io)?;
        }
        let conn = Connection::open(path)?;
        Self::with_connection(conn)
    }

    /// Wrap an existing connection (used by tests with in-memory databases).
    pub fn with_connection(conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.execute_batch(SCHEMA)?;
        // Additive migration (older dbs): runs.model column.
        let has_model_column = {
            let mut stmt = conn.prepare("PRAGMA table_info(runs)").ok();
            let mut found = false;
            if let Some(mut stmt) = stmt {
                let names: Vec<String> = stmt
                    .query_map([], |row| row.get::<_, String>(1))
                    .ok()
                    .map(|rows| rows.filter_map(|r| r.ok()).collect())
                    .unwrap_or_default();
                found = names.iter().any(|c| c == "model");
            }
            found
        };
        if !has_model_column {
            conn.execute_batch("ALTER TABLE runs ADD COLUMN model TEXT")?;
        }
        let has_tags_column = {
            let mut stmt = conn.prepare("PRAGMA table_info(datasets)").ok();
            let mut found = false;
            if let Some(mut stmt) = stmt {
                let names: Vec<String> = stmt
                    .query_map([], |row| row.get::<_, String>(1))
                    .ok()
                    .map(|rows| rows.filter_map(|r| r.ok()).collect())
                    .unwrap_or_default();
                found = names.iter().any(|c| c == "tags");
            }
            found
        };
        if !has_tags_column {
            conn.execute_batch("ALTER TABLE datasets ADD COLUMN tags TEXT NOT NULL DEFAULT '[]'")?;
        }
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        Ok(SqliteStore {
            conn: Mutex::new(conn),
        })
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>> {
        self.conn
            .lock()
            .map_err(|_| CoreError::Conflict("sqlite connection mutex poisoned".into()))
    }
}

// --- serde helpers -----------------------------------------------------------

fn ser<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_string(v).expect("stored values are serializable domain types")
}

fn de<T: serde::de::DeserializeOwned>(raw: String) -> Result<T> {
    serde_json::from_str(&raw).map_err(CoreError::Serialization)
}

/// Serialize enum values as JSON text (`"active"`) so [`de_enum`] can parse
/// them back losslessly.
fn ser_enum<T: serde::Serialize>(v: &T) -> Result<String> {
    serde_json::to_string(v).map_err(CoreError::Serialization)
}

fn de_enum<T: serde::de::DeserializeOwned>(raw: String) -> Result<T> {
    // Accepts both plain-string values (`"example"`) and any enum variant
    // representation serde emits (e.g. externally tagged newtype variants).
    serde_json::from_str(&raw).map_err(CoreError::Serialization)
}

fn ts_to_str(t: &DateTime<Utc>) -> String {
    t.to_rfc3339()
}

fn str_to_ts(s: String) -> Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(&s)
        .map_err(|e| CoreError::Serialization(serde::de::Error::custom(e)))
        .map(|t| t.with_timezone(&Utc))
}

// --- row mapping -------------------------------------------------------------

const SESSION_COLS: &str = "id, name, description, status, created_at, updated_at, tags";
const RUN_COLS: &str =
    "id, name, session_id, status, config, hparams, dataset_id, dataset_hash, model, created_at, updated_at, tags";
const METRIC_COLS: &str = "run_id, name, context, step, epoch, value, timestamp";
const IMAGE_COLS: &str =
    "id, run_id, session_id, name, step, epoch, png_base64, caption, meta, created_at";
const VIZ_COLS: &str =
    "id, run_id, session_id, worker_model, text, dataset_item_id, style, options, svg, created_at";
const DATASET_COLS: &str =
    "id, name, source_ref, schema_version, created_at, updated_at, versions, tags";
const ITEM_COLS: &str =
    "id, dataset_id, input_fields, label, kind, source, status, split, assignee, notes, updated_at";
const HISTORY_COLS: &str = "id, item_id, timestamp, actor, field, old_value, new_value";
const JOB_COLS: &str =
    "id, dataset_id, pipeline, params, status, artifacts, stats, created_at, updated_at";
const SCOPE_COLS: &str = "id, name, query, created_at";
const SOURCE_COLS: &str = "id, name, kind, size, sha256, content, created_at";

fn scope_from_row(row: &rusqlite::Row) -> Result<Scope> {
    Ok(Scope {
        id: row.get(0)?,
        name: row.get(1)?,
        query: row.get(2)?,
        created_at: str_to_ts(row.get(3)?)?,
    })
}

fn json_col<T: serde::de::DeserializeOwned>(raw: String) -> Result<T> {
    de(raw)
}

fn session_from_row(row: &rusqlite::Row) -> Result<Session> {
    Ok(Session {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
        status: de_enum(row.get(3)?)?,
        created_at: str_to_ts(row.get(4)?)?,
        updated_at: str_to_ts(row.get(5)?)?,
        tags: json_col(row.get(6)?)?,
    })
}

fn run_from_row(row: &rusqlite::Row) -> Result<Run> {
    let config_raw: Option<String> = row.get(4)?;
    Ok(Run {
        id: row.get(0)?,
        name: row.get(1)?,
        session_id: row.get(2)?,
        status: de_enum(row.get(3)?)?,
        config: match config_raw {
            Some(raw) => Some(serde_json::from_str(&raw).map_err(CoreError::Serialization)?),
            None => None,
        },
        hparams: json_col(row.get(5)?)?,
        dataset_id: row.get(6)?,
        dataset_hash: row.get(7)?,
        model: row.get(8)?,
        created_at: str_to_ts(row.get(9)?)?,
        updated_at: str_to_ts(row.get(10)?)?,
        tags: json_col(row.get(11)?)?,
    })
}

fn data_session_from_row(row: &rusqlite::Row) -> Result<DataSession> {
    Ok(DataSession {
        id: row.get(0)?,
        name: row.get(1)?,
        notes: row.get(2)?,
        created_at: str_to_ts(row.get(3)?)?,
        updated_at: str_to_ts(row.get(4)?)?,
    })
}

fn data_run_from_row(row: &rusqlite::Row) -> Result<DataRun> {
    Ok(DataRun {
        id: row.get(0)?,
        session_id: row.get(1)?,
        name: row.get(2)?,
        status: row.get(3)?,
        stages: json_col(row.get(4)?)?,
        dataset_id: row.get(5)?,
        results: json_col(row.get(6)?)?,
        created_at: str_to_ts(row.get(7)?)?,
        updated_at: str_to_ts(row.get(8)?)?,
    })
}

fn model_entry_from_row(row: &rusqlite::Row) -> Result<ModelEntry> {
    Ok(ModelEntry {
        id: row.get(0)?,
        name: row.get(1)?,
        path: row.get(2)?,
        kind: row.get(3)?,
        run_id: row.get(4)?,
        notes: row.get(5)?,
        created_at: str_to_ts(row.get(6)?)?,
    })
}

fn metric_from_row(row: &rusqlite::Row) -> Result<MetricPoint> {
    Ok(MetricPoint {
        run_id: row.get(0)?,
        name: row.get(1)?,
        context: json_col(row.get(2)?)?,
        step: row.get::<_, i64>(3)? as u64,
        epoch: row.get::<_, Option<i64>>(4)?.map(|e| e as u64),
        value: row.get(5)?,
        timestamp: str_to_ts(row.get(6)?)?,
    })
}

fn image_from_row(row: &rusqlite::Row) -> Result<Image> {
    Ok(Image {
        id: row.get(0)?,
        run_id: row.get(1)?,
        session_id: row.get(2)?,
        name: row.get(3)?,
        step: row.get::<_, Option<i64>>(4)?.map(|s| s as u64),
        epoch: row.get::<_, Option<i64>>(5)?.map(|e| e as u64),
        png_base64: row.get(6)?,
        caption: row.get(7)?,
        meta: json_col(row.get(8)?)?,
        created_at: str_to_ts(row.get(9)?)?,
    })
}

fn viz_from_row(row: &rusqlite::Row) -> Result<Visualization> {
    Ok(Visualization {
        id: row.get(0)?,
        run_id: row.get(1)?,
        session_id: row.get(2)?,
        worker_model: row.get(3)?,
        text: row.get(4)?,
        dataset_item_id: row.get(5)?,
        style: row.get(6)?,
        options: json_col(row.get(7)?)?,
        svg: row.get(8)?,
        created_at: str_to_ts(row.get(9)?)?,
    })
}

fn dataset_from_row(row: &rusqlite::Row) -> Result<Dataset> {
    Ok(Dataset {
        id: row.get(0)?,
        name: row.get(1)?,
        source_ref: row.get(2)?,
        schema_version: row.get::<_, i64>(3)? as u32,
        created_at: str_to_ts(row.get(4)?)?,
        updated_at: str_to_ts(row.get(5)?)?,
        versions: json_col(row.get(6)?)?,
        tags: row
            .get::<_, Option<String>>(7)?
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default(),
    })
}

fn item_from_row(row: &rusqlite::Row) -> Result<DataItem> {
    Ok(DataItem {
        id: row.get(0)?,
        dataset_id: row.get(1)?,
        input_fields: json_col(row.get(2)?)?,
        label: row.get(3)?,
        kind: de_enum(row.get(4)?)?,
        source: row.get(5)?,
        status: de_enum(row.get(6)?)?,
        split: de_enum(row.get(7)?)?,
        assignee: row.get(8)?,
        notes: row.get(9)?,
        updated_at: str_to_ts(row.get(10)?)?,
    })
}

fn history_from_row(row: &rusqlite::Row) -> Result<DataItemHistory> {
    Ok(DataItemHistory {
        id: row.get(0)?,
        item_id: row.get(1)?,
        timestamp: str_to_ts(row.get(2)?)?,
        actor: row.get(3)?,
        field: row.get(4)?,
        old_value: json_col(row.get(5)?)?,
        new_value: json_col(row.get(6)?)?,
    })
}

fn job_from_row(row: &rusqlite::Row) -> Result<ConversionJob> {
    Ok(ConversionJob {
        id: row.get(0)?,
        dataset_id: row.get(1)?,
        pipeline: json_col(row.get(2)?)?,
        params: json_col(row.get(3)?)?,
        status: de_enum(row.get(4)?)?,
        artifacts: json_col(row.get(5)?)?,
        stats: json_col(row.get(6)?)?,
        created_at: str_to_ts(row.get(7)?)?,
        updated_at: str_to_ts(row.get(8)?)?,
    })
}

fn source_doc_from_row(row: &rusqlite::Row) -> Result<SourceDoc> {
    Ok(SourceDoc {
        id: row.get(0)?,
        name: row.get(1)?,
        kind: row.get(2)?,
        size: row.get(3)?,
        sha256: row.get(4)?,
        content: row.get(5)?,
        created_at: str_to_ts(row.get(6)?)?,
    })
}


/// Adapt a CoreError-returning row mapper to the `rusqlite::Result` signature
/// that `query_row`/`query_map` expect.
fn wrap<T>(
    map: fn(&rusqlite::Row) -> Result<T>,
) -> impl Fn(&rusqlite::Row) -> rusqlite::Result<T> {
    move |row| map(row).map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
}

/// Map a UNIQUE constraint violation to the domain-level Conflict error so
/// `create_*` semantics match the in-memory reference store.
fn map_unique_violation(err: rusqlite::Error, kind: &str, id: &str) -> CoreError {
    if matches!(
        &err,
        rusqlite::Error::SqliteFailure(e, _)
            if e.code == rusqlite::ErrorCode::ConstraintViolation
    ) {
        CoreError::Conflict(format!("{kind} {id} already exists"))
    } else {
        err.into()
    }
}

impl Storage for SqliteStore {
    fn supports_persistence(&self) -> bool {
        true
    }

    fn export_json(&self) -> Result<serde_json::Value> {
        use serde_json::json;
        let conn = self.lock()?;

        fn collect<T: serde::Serialize>(
            conn: &Connection,
            cols: &str,
            table: &str,
            map: fn(&rusqlite::Row) -> Result<T>,
        ) -> Result<Vec<serde_json::Value>> {
            let mut stmt = conn.prepare(&format!("SELECT {cols} FROM {table}"))?;
            let rows = stmt
                .query_map([], wrap(map))?
                .map(|entity| {
                    serde_json::to_value(entity?).map_err(CoreError::Serialization)
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(rows)
        }

        Ok(json!({
            "format": "coach-export-1",
            "sessions": collect(&*conn, SESSION_COLS, "sessions", session_from_row)?,
            "runs": collect(&*conn, RUN_COLS, "runs", run_from_row)?,
            "metrics": collect(&*conn, METRIC_COLS, "metric_points", metric_from_row)?,
            "images": collect(&*conn, IMAGE_COLS, "images", image_from_row)?,
            "visualizations": collect(&*conn, VIZ_COLS, "visualizations", viz_from_row)?,
            "datasets": collect(&*conn, DATASET_COLS, "datasets", dataset_from_row)?,
            "data_items": collect(&*conn, ITEM_COLS, "data_items", item_from_row)?,
            "item_history": collect(&*conn, HISTORY_COLS, "item_history", history_from_row)?,
            "conversion_jobs": collect(&*conn, JOB_COLS, "conversion_jobs", job_from_row)?,
            "models": collect(&*conn, "id, name, path, kind, run_id, notes, created_at", "models", model_entry_from_row)?,
            "data_sessions": collect(&*conn, "id, name, notes, created_at, updated_at", "data_sessions", data_session_from_row)?,
            "data_runs": collect(&*conn, "id, session_id, name, status, stages, dataset_id, results, created_at, updated_at", "data_runs", data_run_from_row)?,
        }))
    }

    fn import_json(&self, bundle: serde_json::Value) -> Result<()> {
        let conn = self.lock()?;
        let list = |key: &str| -> Vec<serde_json::Value> {
            match bundle.get(key) {
                Some(serde_json::Value::Array(items)) => items.clone(),
                _ => vec![],
            }
        };
        let conn = &*conn;
        conn.execute_batch("BEGIN")?;
        let result = (|| -> Result<()> {
            for item in list("sessions") {
                let s: Session = serde_json::from_value(item).map_err(CoreError::Serialization)?;
                conn.execute(
                    "INSERT INTO sessions (id, name, description, status, created_at, updated_at, tags)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                     ON CONFLICT(id) DO UPDATE SET name=?2, description=?3, status=?4, updated_at=?6, tags=?7",
                    params![
                        s.id,
                        s.name,
                        s.description,
                        ser_enum(&s.status)?,
                        ts_to_str(&s.created_at),
                        ts_to_str(&s.updated_at),
                        ser(&s.tags)
                    ],
                )?;
            }
            for item in list("runs") {
                let r: Run = serde_json::from_value(item).map_err(CoreError::Serialization)?;
                conn.execute(
                    "INSERT INTO runs (id, name, session_id, status, config, hparams, dataset_id, dataset_hash, model, created_at, updated_at, tags)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
                     ON CONFLICT(id) DO UPDATE SET name=?2, session_id=?3, status=?4, config=?5, hparams=?6,
                       dataset_id=?7, dataset_hash=?8, model=?9, updated_at=?11, tags=?12",
                    params![
                        r.id,
                        r.name,
                        r.session_id,
                        ser_enum(&r.status)?,
                        r.config.as_ref().map(ser),
                        ser(&r.hparams),
                        r.dataset_id,
                        r.dataset_hash,
                        r.model,
                        ts_to_str(&r.created_at),
                        ts_to_str(&r.updated_at),
                        ser(&r.tags)
                    ],
                )?;
            }
            for item in list("metrics") {
                let m: MetricPoint =
                    serde_json::from_value(item).map_err(CoreError::Serialization)?;
                conn.execute(
                    "INSERT INTO metric_points (run_id, name, context, step, epoch, value, timestamp)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        m.run_id,
                        m.name,
                        ser(&m.context),
                        m.step as i64,
                        m.epoch.map(|e| e as i64),
                        m.value,
                        ts_to_str(&m.timestamp)
                    ],
                )?;
            }
            for item in list("images") {
                let i: Image = serde_json::from_value(item).map_err(CoreError::Serialization)?;
                conn.execute(
                    "INSERT INTO images (id, run_id, session_id, name, step, epoch, png_base64, caption, meta, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                     ON CONFLICT(id) DO UPDATE SET run_id=?2, session_id=?3, name=?4, step=?5, epoch=?6,
                       png_base64=?7, caption=?8, meta=?9",
                    params![
                        i.id,
                        i.run_id,
                        i.session_id,
                        i.name,
                        i.step.map(|s| s as i64),
                        i.epoch.map(|e| e as i64),
                        i.png_base64,
                        i.caption,
                        ser(&i.meta),
                        ts_to_str(&i.created_at)
                    ],
                )?;
            }
            for item in list("visualizations") {
                let v: Visualization =
                    serde_json::from_value(item).map_err(CoreError::Serialization)?;
                conn.execute(
                    "INSERT INTO visualizations (id, run_id, session_id, worker_model, text, dataset_item_id, style, options, svg, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                     ON CONFLICT(id) DO UPDATE SET run_id=?2, session_id=?3, worker_model=?4, text=?5,
                       dataset_item_id=?6, style=?7, options=?8, svg=?9",
                    params![
                        v.id,
                        v.run_id,
                        v.session_id,
                        v.worker_model,
                        v.text,
                        v.dataset_item_id,
                        v.style,
                        ser(&v.options),
                        v.svg,
                        ts_to_str(&v.created_at)
                    ],
                )?;
            }
            for item in list("datasets") {
                let d: Dataset = serde_json::from_value(item).map_err(CoreError::Serialization)?;
                conn.execute(
                    "INSERT INTO datasets (id, name, source_ref, schema_version, created_at, updated_at, versions, tags)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                     ON CONFLICT(id) DO UPDATE SET name=?2, source_ref=?3, schema_version=?4, updated_at=?6, versions=?7, tags=?8",
                    params![
                        d.id,
                        d.name,
                        d.source_ref,
                        d.schema_version as i64,
                        ts_to_str(&d.created_at),
                        ts_to_str(&d.updated_at),
                        ser(&d.versions),
                        ser(&d.tags)
                    ],
                )?;
            }
            for item in list("data_items") {
                let i: DataItem = serde_json::from_value(item).map_err(CoreError::Serialization)?;
                conn.execute(
                    "INSERT INTO data_items (id, dataset_id, input_fields, label, kind, source, status, split, assignee, notes, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                     ON CONFLICT(id) DO UPDATE SET dataset_id=?2, input_fields=?3, label=?4, kind=?5,
                       source=?6, status=?7, split=?8, assignee=?9, notes=?10, updated_at=?11",
                    params![
                        i.id,
                        i.dataset_id,
                        ser(&i.input_fields),
                        i.label,
                        ser_enum(&i.kind)?,
                        i.source,
                        ser_enum(&i.status)?,
                        ser_enum(&i.split)?,
                        i.assignee,
                        i.notes,
                        ts_to_str(&i.updated_at)
                    ],
                )?;
            }
            for item in list("item_history") {
                let h: DataItemHistory =
                    serde_json::from_value(item).map_err(CoreError::Serialization)?;
                conn.execute(
                    "INSERT INTO item_history (id, item_id, timestamp, actor, field, old_value, new_value)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                     ON CONFLICT(id) DO UPDATE SET item_id=?2, timestamp=?3, actor=?4, field=?5,
                       old_value=?6, new_value=?7",
                    params![
                        h.id,
                        h.item_id,
                        ts_to_str(&h.timestamp),
                        h.actor,
                        h.field,
                        ser(&h.old_value),
                        ser(&h.new_value)
                    ],
                )?;
            }
            for item in list("conversion_jobs") {
                let j: ConversionJob =
                    serde_json::from_value(item).map_err(CoreError::Serialization)?;
                conn.execute(
                    "INSERT INTO conversion_jobs (id, dataset_id, pipeline, params, status, artifacts, stats, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                     ON CONFLICT(id) DO UPDATE SET dataset_id=?2, pipeline=?3, params=?4, status=?5,
                       artifacts=?6, stats=?7, updated_at=?9",
                    params![
                        j.id,
                        j.dataset_id,
                        ser(&j.pipeline),
                        ser(&j.params),
                        ser_enum(&j.status)?,
                        ser(&j.artifacts),
                        ser(&j.stats),
                        ts_to_str(&j.created_at),
                        ts_to_str(&j.updated_at)
                    ],
                )?;
            }
            Ok(())
        })();
        match result {
            Ok(()) => conn.execute_batch("COMMIT")?,
            Err(err) => {
                let _ = conn.execute_batch("ROLLBACK");
                return Err(err);
            }
        }
        Ok(())
    }

    // --- Sessions ---

    fn create_session(&self, session: Session) -> Result<Session> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO sessions (id, name, description, status, created_at, updated_at, tags)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                session.id,
                session.name,
                session.description,
                ser_enum(&session.status)?,
                ts_to_str(&session.created_at),
                ts_to_str(&session.updated_at),
                ser(&session.tags)
            ],
        )
        .map_err(|e| map_unique_violation(e, "session", &session.id))?;
        Ok(session)
    }

    fn get_session(&self, id: &str) -> Result<Session> {
        let conn = self.lock()?;
        conn.query_row(
            &format!("SELECT {SESSION_COLS} FROM sessions WHERE id = ?1"),
            params![id],
            wrap(session_from_row),
        )
        .optional()?
        .ok_or_else(|| CoreError::NotFound(format!("session {id}")))
    }

    fn list_sessions(&self) -> Result<Vec<Session>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(&format!(
            "SELECT {SESSION_COLS} FROM sessions ORDER BY created_at"
        ))?;
        let rows = stmt
            .query_map([], wrap(session_from_row))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    fn update_session(&self, session: Session) -> Result<Session> {
        let conn = self.lock()?;
        let mut s = session;
        s.updated_at = Utc::now();
        let affected = conn.execute(
            "UPDATE sessions SET name=?2, description=?3, status=?4, updated_at=?5, tags=?6 WHERE id=?1",
            params![
                s.id,
                s.name,
                s.description,
                ser_enum(&s.status)?,
                ts_to_str(&s.updated_at),
                ser(&s.tags)
            ],
        )?;
        if affected == 0 {
            return Err(CoreError::NotFound(format!("session {}", s.id)));
        }
        Ok(s)
    }

    fn delete_session(&self, id: &str) -> Result<()> {
        let conn = self.lock()?;
        let affected = conn.execute("DELETE FROM sessions WHERE id = ?1", params![id])?;
        if affected == 0 {
            return Err(CoreError::NotFound(format!("session {id}")));
        }
        Ok(())
    }

    // --- Runs ---

    fn create_run(&self, run: Run) -> Result<Run> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO runs (id, name, session_id, status, config, hparams, dataset_id, dataset_hash, model, created_at, updated_at, tags)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                run.id,
                run.name,
                run.session_id,
                ser_enum(&run.status)?,
                run.config.as_ref().map(ser),
                ser(&run.hparams),
                run.dataset_id,
                run.dataset_hash,
                run.model,
                ts_to_str(&run.created_at),
                ts_to_str(&run.updated_at),
                ser(&run.tags)
            ],
        )
        .map_err(|e| map_unique_violation(e, "run", &run.id))?;
        Ok(run)
    }

    fn get_run(&self, id: &str) -> Result<Run> {
        let conn = self.lock()?;
        conn.query_row(
            &format!("SELECT {RUN_COLS} FROM runs WHERE id = ?1"),
            params![id],
            wrap(run_from_row),
        )
        .optional()?
        .ok_or_else(|| CoreError::NotFound(format!("run {id}")))
    }

    fn list_runs(&self, session_id: Option<&str>) -> Result<Vec<Run>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(&format!(
            "SELECT {RUN_COLS} FROM runs WHERE (?1 IS NULL OR session_id = ?1) ORDER BY created_at DESC"
        ))?;
        let rows = stmt
            .query_map(params![session_id], wrap(run_from_row))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    fn update_run(&self, run: Run) -> Result<Run> {
        let conn = self.lock()?;
        let mut r = run;
        r.updated_at = Utc::now();
        let affected = conn.execute(
            "UPDATE runs SET name=?2, session_id=?3, status=?4, config=?5, hparams=?6, dataset_id=?7, dataset_hash=?8, model=?9, updated_at=?10, tags=?11
             WHERE id=?1",
            params![
                r.id,
                r.name,
                r.session_id,
                ser_enum(&r.status)?,
                r.config.as_ref().map(ser),
                ser(&r.hparams),
                r.dataset_id,
                r.dataset_hash,
                r.model,
                ts_to_str(&r.updated_at),
                ser(&r.tags)
            ],
        )?;
        if affected == 0 {
            return Err(CoreError::NotFound(format!("run {}", r.id)));
        }
        Ok(r)
    }

    fn delete_run(&self, id: &str) -> Result<()> {
        let conn = self.lock()?;
        let affected = conn.execute("DELETE FROM runs WHERE id = ?1", params![id])?;
        if affected == 0 {
            return Err(CoreError::NotFound(format!("run {id}")));
        }
        Ok(())
    }

    // --- Metrics ---

    fn append_metric_points(&self, points: &[MetricPoint]) -> Result<()> {
        let mut conn = self.lock()?;
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO metric_points (run_id, name, context, step, epoch, value, timestamp)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )?;
            for p in points {
                stmt.execute(params![
                    p.run_id,
                    p.name,
                    ser(&p.context),
                    p.step as i64,
                    p.epoch.map(|e| e as i64),
                    p.value,
                    ts_to_str(&p.timestamp)
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    fn query_metrics(&self, run_id: &str, name: Option<&str>) -> Result<Vec<MetricPoint>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(&format!(
            "SELECT {METRIC_COLS} FROM metric_points
             WHERE run_id = ?1 AND (?2 IS NULL OR name = ?2)
             ORDER BY step, id"
        ))?;
        let rows = stmt
            .query_map(params![run_id, name], wrap(metric_from_row))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    fn list_metric_names(&self) -> Result<Vec<String>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT DISTINCT name FROM metric_points ORDER BY name",
        )?;
        let names = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(names)
    }

    // --- Images ---

    fn put_image(&self, image: Image) -> Result<Image> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO images (id, run_id, session_id, name, step, epoch, png_base64, caption, meta, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(id) DO UPDATE SET run_id=?2, session_id=?3, name=?4, step=?5, epoch=?6,
               png_base64=?7, caption=?8, meta=?9",
            params![
                image.id,
                image.run_id,
                image.session_id,
                image.name,
                image.step.map(|s| s as i64),
                image.epoch.map(|e| e as i64),
                image.png_base64,
                image.caption,
                ser(&image.meta),
                ts_to_str(&image.created_at)
            ],
        )?;
        Ok(image)
    }

    fn list_images(&self, run_id: Option<&str>) -> Result<Vec<Image>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(&format!(
            "SELECT {IMAGE_COLS} FROM images WHERE (?1 IS NULL OR run_id = ?1) ORDER BY created_at"
        ))?;
        let rows = stmt
            .query_map(params![run_id], wrap(image_from_row))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    // --- Visualizations ---

    fn put_visualization(&self, viz: Visualization) -> Result<Visualization> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO visualizations (id, run_id, session_id, worker_model, text, dataset_item_id, style, options, svg, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(id) DO UPDATE SET run_id=?2, session_id=?3, worker_model=?4, text=?5,
               dataset_item_id=?6, style=?7, options=?8, svg=?9",
            params![
                viz.id,
                viz.run_id,
                viz.session_id,
                viz.worker_model,
                viz.text,
                viz.dataset_item_id,
                viz.style,
                ser(&viz.options),
                viz.svg,
                ts_to_str(&viz.created_at)
            ],
        )?;
        Ok(viz)
    }

    fn list_visualizations(&self, run_id: Option<&str>) -> Result<Vec<Visualization>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(&format!(
            "SELECT {VIZ_COLS} FROM visualizations WHERE (?1 IS NULL OR run_id = ?1) ORDER BY created_at"
        ))?;
        let rows = stmt
            .query_map(params![run_id], wrap(viz_from_row))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    // --- Datasets + curation ---

    fn create_dataset(&self, dataset: Dataset) -> Result<Dataset> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO datasets (id, name, source_ref, schema_version, created_at, updated_at, versions, tags)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                dataset.id,
                dataset.name,
                dataset.source_ref,
                dataset.schema_version as i64,
                ts_to_str(&dataset.created_at),
                ts_to_str(&dataset.updated_at),
                ser(&dataset.versions),
                ser(&dataset.tags)
            ],
        )
        .map_err(|e| map_unique_violation(e, "dataset", &dataset.id))?;
        Ok(dataset)
    }

    fn get_dataset(&self, id: &str) -> Result<Dataset> {
        let conn = self.lock()?;
        conn.query_row(
            &format!("SELECT {DATASET_COLS} FROM datasets WHERE id = ?1"),
            params![id],
            wrap(dataset_from_row),
        )
        .optional()?
        .ok_or_else(|| CoreError::NotFound(format!("dataset {id}")))
    }

    fn list_datasets(&self) -> Result<Vec<Dataset>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(&format!(
            "SELECT {DATASET_COLS} FROM datasets ORDER BY created_at"
        ))?;
        let rows = stmt
            .query_map([], wrap(dataset_from_row))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    fn update_dataset(&self, dataset: Dataset) -> Result<Dataset> {
        let conn = self.lock()?;
        let mut d = dataset;
        d.updated_at = Utc::now();
        let affected = conn.execute(
            "UPDATE datasets SET name=?2, source_ref=?3, schema_version=?4, updated_at=?5, versions=?6, tags=?7 WHERE id=?1",
            params![
                d.id,
                d.name,
                d.source_ref,
                d.schema_version as i64,
                ts_to_str(&d.updated_at),
                ser(&d.versions),
                ser(&d.tags)
            ],
        )?;
        if affected == 0 {
            return Err(CoreError::NotFound(format!("dataset {}", d.id)));
        }
        Ok(d)
    }

    fn put_data_item(&self, item: DataItem) -> Result<DataItem> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO data_items (id, dataset_id, input_fields, label, kind, source, status, split, assignee, notes, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT(id) DO UPDATE SET dataset_id=?2, input_fields=?3, label=?4, kind=?5,
               source=?6, status=?7, split=?8, assignee=?9, notes=?10, updated_at=?11",
            params![
                item.id,
                item.dataset_id,
                ser(&item.input_fields),
                item.label,
                ser_enum(&item.kind)?,
                item.source,
                ser_enum(&item.status)?,
                ser_enum(&item.split)?,
                item.assignee,
                item.notes,
                ts_to_str(&item.updated_at)
            ],
        )?;
        Ok(item)
    }

    fn list_data_items(
        &self,
        dataset_id: &str,
        status: Option<&DataItemStatus>,
    ) -> Result<Vec<DataItem>> {
        let conn = self.lock()?;
        let status_str = match status {
            Some(s) => Some(ser_enum(s)?),
            None => None,
        };
        let mut stmt = conn.prepare(&format!(
            "SELECT {ITEM_COLS} FROM data_items
             WHERE dataset_id = ?1 AND (?2 IS NULL OR status = ?2)
             ORDER BY updated_at DESC"
        ))?;
        let rows = stmt
            .query_map(params![dataset_id, status_str], wrap(item_from_row))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    fn append_item_history(&self, entry: DataItemHistory) -> Result<()> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO item_history (id, item_id, timestamp, actor, field, old_value, new_value)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                entry.id,
                entry.item_id,
                ts_to_str(&entry.timestamp),
                entry.actor,
                entry.field,
                ser(&entry.old_value),
                ser(&entry.new_value)
            ],
        )?;
        Ok(())
    }

    fn list_item_history(&self, item_id: &str) -> Result<Vec<DataItemHistory>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(&format!(
            "SELECT {HISTORY_COLS} FROM item_history WHERE item_id = ?1 ORDER BY timestamp"
        ))?;
        let rows = stmt
            .query_map(params![item_id], wrap(history_from_row))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    // --- Conversion jobs ---

    fn put_conversion_job(&self, job: ConversionJob) -> Result<ConversionJob> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO conversion_jobs (id, dataset_id, pipeline, params, status, artifacts, stats, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(id) DO UPDATE SET dataset_id=?2, pipeline=?3, params=?4, status=?5,
               artifacts=?6, stats=?7, updated_at=?9",
            params![
                job.id,
                job.dataset_id,
                ser(&job.pipeline),
                ser(&job.params),
                ser_enum(&job.status)?,
                ser(&job.artifacts),
                ser(&job.stats),
                ts_to_str(&job.created_at),
                ts_to_str(&job.updated_at)
            ],
        )?;
        Ok(job)
    }

    fn get_conversion_job(&self, id: &str) -> Result<ConversionJob> {
        let conn = self.lock()?;
        conn.query_row(
            &format!("SELECT {JOB_COLS} FROM conversion_jobs WHERE id = ?1"),
            params![id],
            wrap(job_from_row),
        )
        .optional()?
        .ok_or_else(|| CoreError::NotFound(format!("conversion job {id}")))
    }

    fn list_conversion_jobs(&self, dataset_id: &str) -> Result<Vec<ConversionJob>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(&format!(
            "SELECT {JOB_COLS} FROM conversion_jobs WHERE dataset_id = ?1 ORDER BY created_at"
        ))?;
        let rows = stmt
            .query_map(params![dataset_id], wrap(job_from_row))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    // --- Scopes ---

    fn put_scope(&self, scope: Scope) -> Result<Scope> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO scopes (id, name, query, created_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(id) DO UPDATE SET name=?2, query=?3",
            params![scope.id, scope.name, scope.query, ts_to_str(&scope.created_at)],
        )?;
        Ok(scope)
    }

    fn list_scopes(&self) -> Result<Vec<Scope>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(&format!(
            "SELECT {SCOPE_COLS} FROM scopes ORDER BY created_at"
        ))?;
        let rows = stmt
            .query_map([], wrap(scope_from_row))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    fn delete_scope(&self, id: &str) -> Result<()> {
        let conn = self.lock()?;
        let affected = conn.execute("DELETE FROM scopes WHERE id = ?1", params![id])?;
        if affected == 0 {
            return Err(CoreError::NotFound(format!("scope {id}")));
        }
        Ok(())
    }

    // --- Source documents ---

    fn put_source_doc(&self, doc: SourceDoc) -> Result<SourceDoc> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO source_docs (id, name, kind, size, sha256, content, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO UPDATE SET name=?2, kind=?3, sha256=?4, content=?5",
            params![
                doc.id,
                doc.name,
                doc.kind,
                doc.size,
                doc.sha256,
                doc.content,
                ts_to_str(&doc.created_at)
            ],
        )?;
        Ok(doc)
    }

    fn get_source_doc(&self, id: &str) -> Result<SourceDoc> {
        let conn = self.lock()?;
        conn.query_row(
            &format!("SELECT {SOURCE_COLS} FROM source_docs WHERE id = ?1"),
            params![id],
            wrap(source_doc_from_row),
        )
        .optional()?
        .ok_or_else(|| CoreError::NotFound(format!("source doc {id}")))
    }

    fn list_source_docs(&self) -> Result<Vec<SourceDoc>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(&format!(
            "SELECT {SOURCE_COLS} FROM source_docs ORDER BY created_at DESC"
        ))?;
        let rows = stmt
            .query_map([], wrap(source_doc_from_row))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    fn delete_source_doc(&self, id: &str) -> Result<()> {
        let conn = self.lock()?;
        let affected = conn.execute("DELETE FROM source_docs WHERE id = ?1", params![id])?;
        if affected == 0 {
            return Err(CoreError::NotFound(format!("source doc {id}")));
        }
        Ok(())
        }

    // --- Model registry ---

    fn put_model_entry(&self, entry: ModelEntry) -> Result<ModelEntry> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO models (id, name, path, kind, run_id, notes, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO UPDATE SET name=?2, path=?3, kind=?4, run_id=?5, notes=?6",
            params![
                entry.id,
                entry.name,
                entry.path,
                entry.kind,
                entry.run_id,
                entry.notes,
                ts_to_str(&entry.created_at)
            ],
        )?;
        Ok(entry)
    }

    fn list_model_entries(&self) -> Result<Vec<ModelEntry>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT id, name, path, kind, run_id, notes, created_at FROM models ORDER BY created_at DESC",
        )?;
        let rows = stmt
            .query_map([], wrap(model_entry_from_row))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    fn delete_model_entry(&self, id: &str) -> Result<()> {
        let conn = self.lock()?;
        let affected = conn.execute("DELETE FROM models WHERE id = ?1", params![id])?;
        if affected == 0 {
            return Err(CoreError::NotFound(format!("model entry {id}")));
        }
        Ok(())
        }

    // --- Training-data sessions + runs ---

    fn put_data_session(&self, session: DataSession) -> Result<DataSession> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO data_sessions (id, name, notes, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(id) DO UPDATE SET name=?2, notes=?3",
            params![
                session.id,
                session.name,
                session.notes,
                ts_to_str(&session.created_at),
                ts_to_str(&session.updated_at)
            ],
        )?;
        Ok(session)
    }

    fn get_data_session(&self, id: &str) -> Result<DataSession> {
        let conn = self.lock()?;
        conn.query_row(
            "SELECT id, name, notes, created_at, updated_at FROM data_sessions WHERE id = ?1",
            params![id],
            wrap(data_session_from_row),
        )
        .optional()?
        .ok_or_else(|| CoreError::NotFound(format!("data session {id}")))
    }

    fn list_data_sessions(&self) -> Result<Vec<DataSession>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT id, name, notes, created_at, updated_at FROM data_sessions ORDER BY created_at DESC",
        )?;
        let rows = stmt
            .query_map([], wrap(data_session_from_row))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    fn delete_data_session(&self, id: &str) -> Result<()> {
        let conn = self.lock()?;
        let affected = conn.execute("DELETE FROM data_sessions WHERE id = ?1", params![id])?;
        if affected == 0 {
            return Err(CoreError::NotFound(format!("data session {id}")));
        }
        Ok(())
    }

    fn put_data_run(&self, run: DataRun) -> Result<DataRun> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO data_runs (id, session_id, name, status, stages, dataset_id, results, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(id) DO UPDATE SET session_id=?2, name=?3, status=?4, stages=?5,
               dataset_id=?6, results=?7, updated_at=?9",
            params![
                run.id,
                run.session_id,
                run.name,
                run.status,
                ser(&run.stages),
                run.dataset_id,
                ser(&run.results),
                ts_to_str(&run.created_at),
                ts_to_str(&run.updated_at)
            ],
        )?;
        Ok(run)
    }

    fn get_data_run(&self, id: &str) -> Result<DataRun> {
        let conn = self.lock()?;
        conn.query_row(
            "SELECT id, session_id, name, status, stages, dataset_id, results, created_at, updated_at
             FROM data_runs WHERE id = ?1",
            params![id],
            wrap(data_run_from_row),
        )
        .optional()?
        .ok_or_else(|| CoreError::NotFound(format!("data run {id}")))
    }

    fn list_data_runs(&self, session_id: Option<&str>) -> Result<Vec<DataRun>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT id, session_id, name, status, stages, dataset_id, results, created_at, updated_at
             FROM data_runs WHERE (?1 IS NULL OR session_id = ?1) ORDER BY created_at DESC",
        )?;
        let rows = stmt
            .query_map(params![session_id], wrap(data_run_from_row))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    fn delete_data_run(&self, id: &str) -> Result<()> {
        let conn = self.lock()?;
        let affected = conn.execute("DELETE FROM data_runs WHERE id = ?1", params![id])?;
        if affected == 0 {
            return Err(CoreError::NotFound(format!("data run {id}")));
        }
        Ok(())
    }
}

impl SqliteStore {
    /// One-shot migration from a legacy per-file JSON snapshot directory (the
    /// pre-SQLite `InMemoryStore::snapshot_to_dir` layout). Reads whichever
    /// snapshot files are present, imports them, then moves the processed
    /// files into `<dir>/snapshot-migrated` so they are not imported twice on
    /// the next boot. The directory itself (which also holds `coach.db`,
    /// `providers.json`, …) stays in place.
    pub fn import_legacy_snapshot(&self, dir: &std::path::Path) -> Result<usize> {
        let files = [
            "sessions",
            "runs",
            "metrics",
            "images",
            "visualizations",
            "datasets",
            "data_items",
            "item_history",
            "conversion_jobs",
        ];
        let mut bundle = serde_json::Map::new();
        let mut found = Vec::new();
        for name in files {
            let path = dir.join(format!("{name}.json"));
            if !path.exists() {
                continue;
            }
            let raw = std::fs::read_to_string(&path).map_err(CoreError::Io)?;
            let value: serde_json::Value =
                serde_json::from_str(&raw).map_err(CoreError::Serialization)?;
            bundle.insert(name.to_string(), value);
            found.push(path);
        }
        if found.is_empty() {
            return Ok(0);
        }
        let file_count = found.len();
        self.import_json(serde_json::Value::Object(bundle))?;
        // Only rename the processed files, never the surrounding data dir.
        let done = dir.join("snapshot-migrated");
        std::fs::create_dir_all(&done).map_err(CoreError::Io)?;
        for path in found {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            std::fs::rename(&path, done.join(name)).map_err(CoreError::Io)?;
        }
        Ok(file_count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{new_id, DataItemKind, DataSplit, InputFields, RunStatus, SessionStatus};
    use crate::storage::Storage;

    fn mem_store() -> SqliteStore {
        SqliteStore::with_connection(Connection::open_in_memory().unwrap()).unwrap()
    }

    fn ts() -> DateTime<Utc> {
        Utc::now()
    }

    fn sample_run(id: &str) -> Run {
        Run {
            id: id.into(),
            name: "baseline".into(),
            session_id: None,
            status: RunStatus::Running,
            config: Some(serde_json::json!({"steps": {"max_steps": 2}})),
            hparams: serde_json::json!({"lr": 0.001}),
            dataset_id: None,
            dataset_hash: None,
            model: Some("en_core_web_sm".into()),
            created_at: ts(),
            updated_at: ts(),
            tags: vec![],
        }
    }

    #[test]
    fn session_and_run_roundtrip() {
        let store = mem_store();
        let s = Session {
            id: new_id(),
            name: "sprint-1".into(),
            description: "d".into(),
            status: SessionStatus::Active,
            created_at: ts(),
            updated_at: ts(),
            tags: vec!["a".into()],
        };
        store.create_session(s.clone()).unwrap();
        assert_eq!(store.get_session(&s.id).unwrap().name, "sprint-1");
        assert_eq!(store.list_sessions().unwrap().len(), 1);

        let mut s2 = s.clone();
        s2.name = "renamed".into();
        store.update_session(s2).unwrap();
        assert_eq!(store.get_session(&s.id).unwrap().name, "renamed");

        store.delete_session(&s.id).unwrap();
        assert!(store.get_session(&s.id).is_err());
    }

    #[test]
    fn duplicate_run_conflicts() {
        let store = mem_store();
        let run = sample_run("r1");
        store.create_run(run.clone()).unwrap();
        assert!(matches!(
            store.create_run(run),
            Err(CoreError::Conflict(_))
        ));
    }

    #[test]
    fn metrics_filter_and_order() {
        let store = mem_store();
        store.create_run(sample_run("r1")).unwrap();
        store
            .append_metric_points(&[
                MetricPoint {
                    run_id: "r1".into(),
                    name: "score".into(),
                    context: serde_json::json!({}),
                    step: 2,
                    epoch: Some(0),
                    value: 0.9,
                    timestamp: ts(),
                },
                MetricPoint {
                    run_id: "r1".into(),
                    name: "loss".into(),
                    context: serde_json::json!({}),
                    step: 1,
                    epoch: None,
                    value: 1.1,
                    timestamp: ts(),
                },
                MetricPoint {
                    run_id: "r1".into(),
                    name: "score".into(),
                    context: serde_json::json!({}),
                    step: 1,
                    epoch: None,
                    value: 0.5,
                    timestamp: ts(),
                },
            ])
            .unwrap();
        assert_eq!(store.query_metrics("r1", None).unwrap().len(), 3);
        let scores = store.query_metrics("r1", Some("score")).unwrap();
        assert_eq!(scores.len(), 2);
        assert!(scores[0].step <= scores[1].step);
    }

    #[test]
    fn item_and_history_roundtrip() {
        let store = mem_store();
        let item = DataItem {
            id: "i1".into(),
            dataset_id: "d1".into(),
            input_fields: InputFields {
                text: Some("hello world".into()),
                ..Default::default()
            },
            label: "train-example".into(),
            kind: DataItemKind::Example,
            source: "bfo.json#E27".into(),
            status: DataItemStatus::Raw,
            split: DataSplit::Train,
            assignee: None,
            notes: None,
            updated_at: ts(),
        };
        store.put_data_item(item.clone()).unwrap();
        assert_eq!(store.list_data_items("d1", None).unwrap().len(), 1);
        assert_eq!(
            store
                .list_data_items("d1", Some(&DataItemStatus::Approved))
                .unwrap()
                .len(),
            0
        );
        store
            .append_item_history(DataItemHistory {
                id: "h1".into(),
                item_id: "i1".into(),
                timestamp: ts(),
                actor: "tester".into(),
                field: "status".into(),
                old_value: serde_json::json!("raw"),
                new_value: serde_json::json!("curated"),
            })
            .unwrap();
        assert_eq!(store.list_item_history("i1").unwrap().len(), 1);
        // Missing legacy snapshot dir is a no-op import.
        assert_eq!(
            store
                .import_legacy_snapshot(std::path::Path::new("/nonexistent-coach-dir"))
                .unwrap(),
            0
        );
    }

    #[test]
    fn legacy_snapshot_import_moves_files_once() {
        let dir = std::env::temp_dir().join(format!("coach-mig-{}", new_id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("runs.json"), r#"[{"id":"r1","name":"legacy","session_id":null,"status":"running","config":null,"hparams":{},"dataset_id":null,"dataset_hash":null,"created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z","tags":[]}]"#)
            .unwrap();
        std::fs::write(dir.join("unrelated.json"), "not a snapshot").unwrap();

        let store = mem_store();
        assert_eq!(store.import_legacy_snapshot(&dir).unwrap(), 1);
        assert_eq!(store.get_run("r1").unwrap().name, "legacy");
        assert!(dir.join("snapshot-migrated/runs.json").exists());
        assert!(!dir.join("runs.json").exists());

        // Second boot: nothing left to import.
        assert_eq!(store.import_legacy_snapshot(&dir).unwrap(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn export_import_is_lossless() {
        let store = mem_store();
        store.create_run(sample_run("r1")).unwrap();
        store
            .append_metric_points(&[MetricPoint {
                run_id: "r1".into(),
                name: "loss".into(),
                context: serde_json::json!({"type": "loss"}),
                step: 1,
                epoch: None,
                value: 1.0,
                timestamp: ts(),
            }])
            .unwrap();
        let bundle = store.export_json().unwrap();
        assert_eq!(bundle["format"], "coach-export-1");

        let other = mem_store();
        other.import_json(bundle).unwrap();
        assert_eq!(other.get_run("r1").unwrap().name, "baseline");
        assert_eq!(other.query_metrics("r1", None).unwrap().len(), 1);
    }
}
