//! Render-worker lifecycle endpoints.
//!
//! The Rust API never imports spaCy; it only manages the small Python render
//! worker process (`python -m coach_spacy.render`) and forwards render
//! requests to it.

use axum::{extract::Path as AxumPath, extract::State, Json};
use coach_core::model::{new_id, now, RenderWorker, WorkerStatus};
use serde::{Deserialize, Serialize};

use crate::api_error::ApiError;
use crate::state::AppState;

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct SpawnWorkerRequest {
    /// Path to a local spaCy pipeline or a model name (e.g. `en_core_web_sm`).
    pub model_path: String,
    #[serde(default)]
    pub port: Option<u16>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct SpawnWorkerResponse {
    pub worker: RenderWorker,
}

#[utoipa::path(post, path = "/api/workers", request_body = SpawnWorkerRequest, responses((status = 200, body = SpawnWorkerResponse)))]
pub async fn spawn_worker(
    State(state): State<AppState>,
    Json(body): Json<SpawnWorkerRequest>,
) -> Result<Json<SpawnWorkerResponse>, ApiError> {
    let port = body.port.unwrap_or(0); // 0 = let the worker bind to a free port
    let id = new_id();

    // Spawn managed by the OS so the process outlives handler scope; the API
    // supervises it (health check + restart) in a later increment.
    let python = std::env::var("COACH_PY").unwrap_or_else(|_| "python3".into());
    let mut cmd = std::process::Command::new(python);
    cmd.args(["-m", "coach_spacy.render", "--model", &body.model_path]);
    if port != 0 {
        cmd.args(["--port", &port.to_string()]);
    }
    cmd.stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let pid = match cmd.spawn() {
        Ok(child) => Some(child.id()),
        Err(e) => {
            tracing::warn!(model = %body.model_path, error = %e, "failed to spawn render worker");
            None
        }
    };

    let status = if pid.is_some() {
        WorkerStatus::Starting
    } else {
        WorkerStatus::Stopped
    };
    let worker = RenderWorker {
        id: id.clone(),
        model_path: body.model_path,
        port,
        pid,
        status,
        started_at: now(),
    };
    state.workers.insert(worker.clone());
    Ok(Json(SpawnWorkerResponse { worker }))
}

#[utoipa::path(get, path = "/api/workers", responses((status = 200)))]
pub async fn list_workers(State(state): State<AppState>) -> Json<Vec<RenderWorker>> {
    Json(state.workers.list())
}

#[utoipa::path(get, path = "/api/workers/{id}/health", responses((status = 200)))]
pub async fn worker_health(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let worker = state
        .workers
        .list()
        .into_iter()
        .find(|w| w.id == id)
        .ok_or(coach_core::CoreError::NotFound(format!("worker {id}")))?;
    // Liveness probe of the worker's /health endpoint lands with the
    // render-worker feature; scaffold reports the recorded status.
    Ok(Json(serde_json::json!({"id": id, "status": worker.status})))
}

#[utoipa::path(delete, path = "/api/workers/{id}", responses((status = 204)))]
pub async fn stop_worker(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<axum::http::StatusCode, ApiError> {
    match state.workers.remove(&id) {
        Some(entry) => {
            if let Some(pid) = entry.pid {
                unsafe {
                    // Targeted TERM to the worker we spawned only.
                    let _ = libc_kill_term(pid);
                }
            }
            Ok(axum::http::StatusCode::NO_CONTENT)
        }
        None => Err(coach_core::CoreError::NotFound(format!("worker {id}")).into()),
    }
}

#[cfg(unix)]
unsafe fn libc_kill_term(pid: u32) -> i32 {
    // Minimal, dependency-free SIGTERM send for our own child processes.
    extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    kill(pid as i32, 15)
}

#[cfg(not(unix))]
unsafe fn libc_kill_term(_pid: u32) -> i32 {
    -1
}
