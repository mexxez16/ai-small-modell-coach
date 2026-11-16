//! Shared application state for coach-api.

use coach_core::model::RenderWorker;
use coach_core::storage::{InMemoryStore, Storage};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

/// Preferred default bind port. When `COACH_PORT` is unset, the server scans
/// upward from here until it finds a free port.
pub const DEFAULT_PORT: u16 = 8901;
/// How many consecutive ports to try before giving up.
pub const PORT_SCAN_TRIES: u16 = 20;

/// Server configuration from the environment.
#[derive(Debug, Clone)]
pub struct AppConfig {
    /// Explicit bind port from `COACH_PORT`, or `None` to scan upward from
    /// `DEFAULT_PORT` (8901) for a free port.
    pub port: Option<u16>,
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
            .and_then(|p| p.parse().ok());
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
