//! Storage traits — the stable contract all backends implement.
//!
//! Backends:
//! - `inmemory` (default): a reference [`InMemoryStore`], used by tests and
//!   single-process deployments that do not need crash safety yet.
//! - Parquet + DuckDB (planned behind the `duckdb-backend` feature): all
//!   entities are stored as Parquet files on local disk (or S3 later), queried
//!   via an embedded DuckDB connection. This avoids RocksDB entirely, which is
//!   what keeps Python >= 3.13 compatibility trivial.

use crate::error::{CoreError, Result};
use crate::model::{
    ConversionJob, DataItem, DataItemHistory, Dataset, Image, MetricPoint, Run, Session,
    Visualization,
};
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::RwLock;

/// Storage contract for the whole application.
pub trait Storage: Send + Sync {
    // Sessions
    fn create_session(&self, session: Session) -> Result<Session>;
    fn get_session(&self, id: &str) -> Result<Session>;
    fn list_sessions(&self) -> Result<Vec<Session>>;
    fn update_session(&self, session: Session) -> Result<Session>;
    fn delete_session(&self, id: &str) -> Result<()>;

    // Runs
    fn create_run(&self, run: Run) -> Result<Run>;
    fn get_run(&self, id: &str) -> Result<Run>;
    fn list_runs(&self, session_id: Option<&str>) -> Result<Vec<Run>>;
    fn update_run(&self, run: Run) -> Result<Run>;
    fn delete_run(&self, id: &str) -> Result<()>;

    // Metric points (batched ingest, streaming reads)
    fn append_metric_points(&self, points: &[MetricPoint]) -> Result<()>;
    fn query_metrics(&self, run_id: &str, name: Option<&str>) -> Result<Vec<MetricPoint>>;

    // Images
    fn put_image(&self, image: Image) -> Result<Image>;
    fn list_images(&self, run_id: Option<&str>) -> Result<Vec<Image>>;

    // Visualizations
    fn put_visualization(&self, viz: Visualization) -> Result<Visualization>;
    fn list_visualizations(&self, run_id: Option<&str>) -> Result<Vec<Visualization>>;

    // Datasets + curation
    fn create_dataset(&self, dataset: Dataset) -> Result<Dataset>;
    fn get_dataset(&self, id: &str) -> Result<Dataset>;
    fn list_datasets(&self) -> Result<Vec<Dataset>>;
    fn put_data_item(&self, item: DataItem) -> Result<DataItem>;
    fn list_data_items(
        &self,
        dataset_id: &str,
        status: Option<&crate::model::DataItemStatus>,
    ) -> Result<Vec<DataItem>>;
    fn append_item_history(&self, entry: DataItemHistory) -> Result<()>;
    fn list_item_history(&self, item_id: &str) -> Result<Vec<DataItemHistory>>;

    // Conversion jobs
    fn put_conversion_job(&self, job: ConversionJob) -> Result<ConversionJob>;
    fn get_conversion_job(&self, id: &str) -> Result<ConversionJob>;
    fn list_conversion_jobs(&self, dataset_id: &str) -> Result<Vec<ConversionJob>>;
}

/// Column-partitioned layout used by the Parquet backend (documented for
/// implementers; not used by the in-memory store):
///
/// ```text
/// data/
/// ├── runs/{YYYY-MM}/{run_id}/metrics.parquet    # MetricPoint rows
/// ├── runs/{YYYY-MM}/{run_id}/meta.json          # Run record
/// ├── images/{run_id}/{image_id}.png             # blob store payload
/// ├── sessions/sessions.parquet
/// ├── datasets/{dataset_id}/items.parquet
/// ├── datasets/{dataset_id}/history.parquet
/// └── jobs/conversions.parquet
/// ```
pub mod parquet_layout {
    pub const RUNS_DIR: &str = "runs";
    pub const IMAGES_DIR: &str = "images";
    pub const DATASETS_DIR: &str = "datasets";
    pub const JOBS_DIR: &str = "jobs";
}

/// Simple thread-safe in-memory reference store.
#[derive(Debug, Default)]
pub struct InMemoryStore {
    sessions: RwLock<HashMap<String, Session>>,
    runs: RwLock<HashMap<String, Run>>,
    metrics: RwLock<Vec<MetricPoint>>,
    images: RwLock<Vec<Image>>,
    visualizations: RwLock<Vec<Visualization>>,
    datasets: RwLock<HashMap<String, Dataset>>,
    data_items: RwLock<HashMap<String, DataItem>>,
    item_history: RwLock<Vec<DataItemHistory>>,
    conversion_jobs: RwLock<HashMap<String, ConversionJob>>,
}

impl InMemoryStore {
    pub fn new() -> Self {
        Self::default()
    }
}

fn ts() -> DateTime<Utc> {
    Utc::now()
}

impl Storage for InMemoryStore {
    fn create_session(&self, session: Session) -> Result<Session> {
        let mut map = self.sessions.write().unwrap();
        if map.contains_key(&session.id) {
            return Err(CoreError::Conflict(format!(
                "session {} already exists",
                session.id
            )));
        }
        map.insert(session.id.clone(), session.clone());
        Ok(session)
    }

    fn get_session(&self, id: &str) -> Result<Session> {
        self.sessions
            .read()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or_else(|| CoreError::NotFound(format!("session {id}")))
    }

    fn list_sessions(&self) -> Result<Vec<Session>> {
        Ok(self.sessions.read().unwrap().values().cloned().collect())
    }

    fn update_session(&self, session: Session) -> Result<Session> {
        let mut map = self.sessions.write().unwrap();
        if !map.contains_key(&session.id) {
            return Err(CoreError::NotFound(format!("session {}", session.id)));
        }
        let mut s = session;
        s.updated_at = ts();
        map.insert(s.id.clone(), s.clone());
        Ok(s)
    }

    fn delete_session(&self, id: &str) -> Result<()> {
        self.sessions
            .write()
            .unwrap()
            .remove(id)
            .map(drop)
            .ok_or_else(|| CoreError::NotFound(format!("session {id}")))
    }

    fn create_run(&self, run: Run) -> Result<Run> {
        let mut map = self.runs.write().unwrap();
        if map.contains_key(&run.id) {
            return Err(CoreError::Conflict(format!("run {} already exists", run.id)));
        }
        map.insert(run.id.clone(), run.clone());
        Ok(run)
    }

    fn get_run(&self, id: &str) -> Result<Run> {
        self.runs
            .read()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or_else(|| CoreError::NotFound(format!("run {id}")))
    }

    fn list_runs(&self, session_id: Option<&str>) -> Result<Vec<Run>> {
        Ok(self
            .runs
            .read()
            .unwrap()
            .values()
            .filter(|r| session_id.is_none() || r.session_id.as_deref() == session_id)
            .cloned()
            .collect())
    }

    fn update_run(&self, run: Run) -> Result<Run> {
        let mut map = self.runs.write().unwrap();
        if !map.contains_key(&run.id) {
            return Err(CoreError::NotFound(format!("run {}", run.id)));
        }
        let mut r = run;
        r.updated_at = ts();
        map.insert(r.id.clone(), r.clone());
        Ok(r)
    }

    fn delete_run(&self, id: &str) -> Result<()> {
        self.runs
            .write()
            .unwrap()
            .remove(id)
            .map(drop)
            .ok_or_else(|| CoreError::NotFound(format!("run {id}")))
    }

    fn append_metric_points(&self, points: &[MetricPoint]) -> Result<()> {
        self.metrics.write().unwrap().extend_from_slice(points);
        Ok(())
    }

    fn query_metrics(&self, run_id: &str, name: Option<&str>) -> Result<Vec<MetricPoint>> {
        Ok(self
            .metrics
            .read()
            .unwrap()
            .iter()
            .filter(|p| p.run_id == run_id && name.is_none_or(|n| p.name == n))
            .cloned()
            .collect())
    }

    fn put_image(&self, image: Image) -> Result<Image> {
        self.images.write().unwrap().push(image.clone());
        Ok(image)
    }

    fn list_images(&self, run_id: Option<&str>) -> Result<Vec<Image>> {
        Ok(self
            .images
            .read()
            .unwrap()
            .iter()
            .filter(|i| run_id.is_none() || i.run_id.as_deref() == run_id)
            .cloned()
            .collect())
    }

    fn put_visualization(&self, viz: Visualization) -> Result<Visualization> {
        self.visualizations.write().unwrap().push(viz.clone());
        Ok(viz)
    }

    fn list_visualizations(&self, run_id: Option<&str>) -> Result<Vec<Visualization>> {
        Ok(self
            .visualizations
            .read()
            .unwrap()
            .iter()
            .filter(|v| run_id.is_none() || v.run_id.as_deref() == run_id)
            .cloned()
            .collect())
    }

    fn create_dataset(&self, dataset: Dataset) -> Result<Dataset> {
        let mut map = self.datasets.write().unwrap();
        if map.contains_key(&dataset.id) {
            return Err(CoreError::Conflict(format!(
                "dataset {} already exists",
                dataset.id
            )));
        }
        map.insert(dataset.id.clone(), dataset.clone());
        Ok(dataset)
    }

    fn get_dataset(&self, id: &str) -> Result<Dataset> {
        self.datasets
            .read()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or_else(|| CoreError::NotFound(format!("dataset {id}")))
    }

    fn list_datasets(&self) -> Result<Vec<Dataset>> {
        Ok(self.datasets.read().unwrap().values().cloned().collect())
    }

    fn put_data_item(&self, item: DataItem) -> Result<DataItem> {
        let mut map = self.data_items.write().unwrap();
        map.insert(item.id.clone(), item.clone());
        Ok(item)
    }

    fn list_data_items(
        &self,
        dataset_id: &str,
        status: Option<&crate::model::DataItemStatus>,
    ) -> Result<Vec<DataItem>> {
        Ok(self
            .data_items
            .read()
            .unwrap()
            .values()
            .filter(|i| {
                i.dataset_id == dataset_id && status.is_none_or(|s| &i.status == s)
            })
            .cloned()
            .collect())
    }

    fn append_item_history(&self, entry: DataItemHistory) -> Result<()> {
        self.item_history.write().unwrap().push(entry);
        Ok(())
    }

    fn list_item_history(&self, item_id: &str) -> Result<Vec<DataItemHistory>> {
        Ok(self
            .item_history
            .read()
            .unwrap()
            .iter()
            .filter(|h| h.item_id == item_id)
            .cloned()
            .collect())
    }

    fn put_conversion_job(&self, job: ConversionJob) -> Result<ConversionJob> {
        let mut map = self.conversion_jobs.write().unwrap();
        map.insert(job.id.clone(), job.clone());
        Ok(job)
    }

    fn get_conversion_job(&self, id: &str) -> Result<ConversionJob> {
        self.conversion_jobs
            .read()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or_else(|| CoreError::NotFound(format!("conversion job {id}")))
    }

    fn list_conversion_jobs(&self, dataset_id: &str) -> Result<Vec<ConversionJob>> {
        Ok(self
            .conversion_jobs
            .read()
            .unwrap()
            .values()
            .filter(|j| j.dataset_id == dataset_id)
            .cloned()
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        new_id, DataItemKind, DataItemStatus, DataSplit, InputFields, RunStatus, SessionStatus,
    };

    #[test]
    fn session_crud_roundtrip() {
        let store = InMemoryStore::new();
        let s = Session {
            id: new_id(),
            name: "sprint-1".into(),
            description: String::new(),
            status: SessionStatus::Active,
            created_at: ts(),
            updated_at: ts(),
            tags: vec![],
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
    fn run_and_metric_roundtrip() {
        let store = InMemoryStore::new();
        let run = Run {
            id: "r1".into(),
            name: "baseline".into(),
            session_id: None,
            status: RunStatus::Running,
            config: None,
            hparams: serde_json::json!({"max_steps": 2}),
            dataset_id: None,
            dataset_hash: None,
            created_at: ts(),
            updated_at: ts(),
            tags: vec![],
        };
        store.create_run(run).unwrap();
        store
            .append_metric_points(&[
                MetricPoint {
                    run_id: "r1".into(),
                    name: "textcat_loss".into(),
                    context: serde_json::json!({"type": "loss"}),
                    step: 1,
                    epoch: Some(0),
                    value: 1.2,
                    timestamp: ts(),
                },
                MetricPoint {
                    run_id: "r1".into(),
                    name: "score".into(),
                    context: serde_json::json!({"type": "score"}),
                    step: 2,
                    epoch: Some(0),
                    value: 0.9,
                    timestamp: ts(),
                },
            ])
            .unwrap();
        assert_eq!(store.query_metrics("r1", None).unwrap().len(), 2);
        assert_eq!(store.query_metrics("r1", Some("score")).unwrap().len(), 1);

        let item = DataItem {
            id: "i1".into(),
            dataset_id: "d1".into(),
            input_fields: InputFields {
                text: Some("hello world".into()),
                ..Default::default()
            },
            label: "train-example".into(),
            kind: DataItemKind::Example,
            source: "BFO.json#E27".into(),
            status: DataItemStatus::Raw,
            split: DataSplit::Train,
            assignee: None,
            notes: None,
            updated_at: ts(),
        };
        store.put_data_item(item).unwrap();
        assert_eq!(store.list_data_items("d1", None).unwrap().len(), 1);
    }
}
