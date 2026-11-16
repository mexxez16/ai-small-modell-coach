//! Shared application state for coach-api.

use coach_core::model::RenderWorker;
use coach_core::storage::{InMemoryStore, Storage};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

/// Server configuration from the environment.
#[derive(Debug, Clone)]
pub struct AppConfig {
    pub port: u16,
    /// Single-user auth token. Empty disables auth (localhost dev default).
    #[allow(dead_code)] // enforced by middleware when auth lands
    pub token: Option<String>,
    /// Directory with the built frontend (apps/web/dist) to serve at `/`.
    pub web_dist: Option<std::path::PathBuf>,
}

impl AppConfig {
    pub fn from_env() -> Self {
        let port = std::env::var("COACH_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(8000);
        let token = std::env::var("COACH_TOKEN").ok().filter(|t| !t.is_empty());
        let web_dist = std::env::var("COACH_WEB_DIST")
            .ok()
            .map(std::path::PathBuf::from)
            .filter(|p| p.is_dir());
        AppConfig {
            port,
            token,
            web_dist,
        }
    }
}

/// Live render-worker registry keyed by worker id. Workers are process-scoped
/// and respawned on demand; they are not persisted across restarts.
#[derive(Debug, Default)]
pub struct WorkerRegistry {
    inner: RwLock<HashMap<String, RenderWorker>>,
}

impl WorkerRegistry {
    pub fn insert(&self, worker: RenderWorker) {
        self.inner.write().unwrap().insert(worker.id.clone(), worker);
    }

    pub fn remove(&self, id: &str) -> Option<RenderWorker> {
        self.inner.write().unwrap().remove(id)
    }

    pub fn list(&self) -> Vec<RenderWorker> {
        self.inner.read().unwrap().values().cloned().collect()
    }
}

/// Global application state handed to all axum handlers.
#[derive(Clone)]
pub struct AppState {
    pub config: AppConfig,
    /// In-memory store in the scaffold; swapped for the Parquet/DuckDB
    /// backend behind a feature later on.
    pub storage: Arc<dyn Storage>,
    pub workers: Arc<WorkerRegistry>,
    /// Serializes PTY allocations until the multi-tab terminal lands.
    pub pty_lock: Arc<Mutex<()>>,
}

impl AppState {
    pub fn new(config: AppConfig) -> Self {
        AppState {
            config,
            storage: Arc::new(InMemoryStore::new()),
            workers: Arc::new(WorkerRegistry::default()),
            pty_lock: Arc::new(Mutex::new(())),
        }
    }
}
