//! Shared application state for coach-api.

use coach_core::model::RenderWorker;
use coach_core::storage::{InMemoryStore, Storage};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

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

    pub fn get(&self, id: &str) -> Option<RenderWorker> {
        self.inner.read().unwrap().get(id).cloned()
    }

    pub fn update_status(&self, id: &str, status: RenderWorkerStatus) {
        if let Some(worker) = self.inner.write().unwrap().get_mut(id) {
            if worker.status != status {
                worker.status = status;
            }
        }
    }
}

use coach_core::model::WorkerStatus as RenderWorkerStatus;

/// Global application state handed to all axum handlers.
#[derive(Clone)]
pub struct AppState {
    pub config: AppConfig,
    /// In-memory store in the scaffold; swapped for the Parquet/DuckDB
    /// backend behind a feature later on.
    pub storage: Arc<dyn Storage>,
    /// Dirty flag for snapshot writer.
    dirty: Arc<std::sync::Mutex<bool>>,
    /// Directory the storage snapshots persist to (COACH_DATA_DIR); `None`
    /// keeps the purely in-memory scaffold behavior.
    pub snapshot_dir: Option<std::path::PathBuf>,
    pub workers: Arc<WorkerRegistry>,
    /// Chat provider configs, persisted in `<data_dir>/providers.json`.
    pub provider_store: Arc<crate::providers::ProviderStore>,
    /// HTTP client for provider calls (streaming chat completions).
    pub http_client: reqwest::Client,
    /// All PTY/socket-based work is scoped to this directory (workspace
    /// confinement; the agent and terminal never operate outside it).
    pub workspace_dir: PathBuf,
}

impl AppState {
    pub fn new(config: AppConfig) -> Self {
        let data_dir = std::env::var("COACH_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("./data"));
        let workspace_dir = std::env::var("COACH_WORKSPACE")
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        // Create the data dir so snapshots + providers land there even on a
        // completely fresh COACH_DATA_DIR.
        let _ = std::fs::create_dir_all(&data_dir);
        let store = InMemoryStore::new();
        // Survive restarts: reload a previous snapshot when present. Missing
        // files are treated as an empty store (fresh install).
        if let Err(err) = store.load_from_dir(&data_dir) {
            eprintln!("snapshot load failed: {err:#}");
        }
        let store: Arc<dyn Storage> = Arc::new(store);
        AppState {
            config,
            storage: store,
            dirty: Arc::new(std::sync::Mutex::new(false)),
            snapshot_dir: Some(data_dir.clone()),
            workers: Arc::new(WorkerRegistry::default()),
            provider_store: Arc::new(crate::providers::ProviderStore::load(&data_dir)),
            http_client: reqwest::Client::new(),
            workspace_dir,
        }
    }

    /// Persist the full store into the snapshot dir (best-effort).
    pub fn persist_snapshot(&self) {
        eprintln!("persist_snapshot invoked");
        if let Some(dir) = self.snapshot_dir.as_ref() {
            if let Err(err) = self.storage.snapshot_to_dir(dir) {
                eprintln!("snapshot failed: {err:#}");
            }
        }
    }

    /// Whether a mutation happened since the last snapshot.
    pub fn snapshot_dirty(&self) -> bool {
        *self.dirty.lock().unwrap()
    }

    /// Reset the snapshot dirty flag.
    pub fn clear_dirty(&self) {
        *self.dirty.lock().unwrap() = false;
    }

    /// Flag the store as mutated so the snapshot writer persists it.
    pub fn mark_dirty(&self) {
        *self.dirty.lock().unwrap() = true;
    }
}
