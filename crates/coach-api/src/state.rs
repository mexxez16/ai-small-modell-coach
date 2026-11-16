//! Shared application state for coach-api.

use coach_core::model::{MetricPoint, RenderWorker, WorkerStatus};
use coach_core::storage::{InMemoryStore, Storage};
#[cfg(feature = "sqlite-backend")]
use coach_core::sqlite_backend::SqliteStore;
use tokio::sync::broadcast;
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
    /// Override for the data directory (tests); `from_env` leaves it `None`
    /// and [`AppState::new`] falls back to `COACH_DATA_DIR`/`./data`.
    #[allow(dead_code)] // consumed by AppState::new
    pub data_dir: Option<PathBuf>,
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
            data_dir: None,
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

    #[allow(dead_code)] // reattached once the API exposes single-worker lookup
    pub fn get(&self, id: &str) -> Option<RenderWorker> {
        self.inner.read().unwrap().get(id).cloned()
    }

    pub fn update_status(&self, id: &str, status: WorkerStatus) {
        if let Some(worker) = self.inner.write().unwrap().get_mut(id) {
            if worker.status != status {
                worker.status = status;
            }
        }
    }
}

/// Global application state handed to all axum handlers.
#[derive(Clone)]
pub struct AppState {
    pub config: AppConfig,
    /// SQLite store (`coach.db`) by default; falls back to the in-memory
    /// reference store when the database cannot be opened.
    pub storage: Arc<dyn Storage>,
    /// Data directory: `coach.db`, `providers.json` and migrated legacy
    /// snapshots live here (COACH_DATA_DIR, default `./data`).
    #[allow(dead_code)] // surfaced in Settings once the UI shows storage info
    pub data_dir: PathBuf,
    /// Live metric fan-out for `/ws/metrics`; receivers that subscribe before
    /// the first append see every subsequent point.
    pub metric_bus: broadcast::Sender<MetricPoint>,
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
        let data_dir = config
            .data_dir
            .clone()
            .or_else(|| std::env::var("COACH_DATA_DIR").ok().map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("./data"));
        let workspace_dir = std::env::var("COACH_WORKSPACE")
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        // Create the data dir so the database + providers land there even on
        // a completely fresh COACH_DATA_DIR.
        let _ = std::fs::create_dir_all(&data_dir);
        let store = open_storage(&data_dir);
        let provider_store = Arc::new(crate::providers::ProviderStore::load(&data_dir));
        AppState {
            config,
            storage: store,
            data_dir,
            metric_bus: broadcast::channel(1024).0,
            workers: Arc::new(WorkerRegistry::default()),
            provider_store,
            http_client: reqwest::Client::new(),
            workspace_dir,
        }
    }
}

/// Open the durable storage backend for `data_dir`.
///
/// Default: SQLite (`coach.db`, WAL mode) with a one-shot migration of a
/// legacy per-file JSON snapshot. When the `sqlite-backend` feature is off or
/// the database cannot be opened, fall back to the in-memory reference store
/// (which reloads the JSON snapshot if present).
fn open_storage(data_dir: &std::path::Path) -> Arc<dyn Storage> {
    #[cfg(feature = "sqlite-backend")]
    {
        let db_path = data_dir.join("coach.db");
        let fresh_db = !db_path.exists();
        match SqliteStore::open(&db_path) {
            Ok(store) => {
                if fresh_db {
                    match store.import_legacy_snapshot(data_dir) {
                        Ok(0) => {}
                        Ok(n) => {
                            tracing::info!(
                                migrated_files = n,
                                "migrated legacy JSON snapshot into coach.db"
                            );
                        }
                        Err(err) => {
                            eprintln!("legacy snapshot migration failed: {err:#}");
                        }
                    }
                }
                return Arc::new(store);
            }
            Err(err) => {
                eprintln!(
                    "sqlite store unavailable ({err:#}); falling back to in-memory store"
                );
            }
        }
    }
    let store = InMemoryStore::new();
    if let Err(err) = store.load_from_dir(data_dir) {
        eprintln!("snapshot load failed: {err:#}");
    }
    Arc::new(store)
}
