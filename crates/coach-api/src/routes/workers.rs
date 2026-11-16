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
    // Pre-allocate a free TCP port so the worker never races other listeners
    // and the registry knows the authoritative address up front.
    let port = body.port.unwrap_or_else(free_tcp_port);
    let id = new_id();

    // Spawn managed by the OS so the process outlives handler scope; the API
    // supervises it (health check + restart) in a later increment.
    let python = resolve_python();
    let mut cmd = std::process::Command::new(python);
    cmd.args(["-m", "coach_spacy.render", "--model", &body.model_path]);
    if port != 0 {
        cmd.args(["--port", &port.to_string()]);
    }
    // Inherit stdio so worker startup errors (import failures, model load
    // errors) surface directly in the coach-api log instead of vanishing.
    cmd.stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit());
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
    let _ = &status;
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
    let healthy = probe_worker(state.http_client.clone(), &format!("http://127.0.0.1:{}/health", worker.port)).await;
    if healthy {
        state.workers.update_status(&id, WorkerStatus::Ready);
    }
    Ok(Json(serde_json::json!({"id": id, "status": worker.status, "healthy": healthy})))
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



/// Proxy a render request to the worker process (`POST /api/visualize`).
/// Keeps the browser decoupled from worker addressing; request bodies are
/// forwarded as-is, response size is capped.
#[utoipa::path(post, path = "/api/visualize", request_body = serde_json::Value, responses((status = 200, body = serde_json::Value)))]
pub async fn visualize_proxy(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, ApiError> {
    forward_to_worker(state, body, false).await
}

#[utoipa::path(post, path = "/api/visualize_png", request_body = serde_json::Value, responses((status = 200, body = serde_json::Value)))]
pub async fn visualize_png_proxy(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, ApiError> {
    forward_to_worker(state, body, true).await
}

async fn forward_to_worker(
    state: AppState,
    body: serde_json::Value,
    png: bool,
) -> Result<Json<serde_json::Value>, ApiError> {
    use serde_json::json;

    // Resolve the target worker: explicit id, else the first Ready/Starting.
    let worker = if let Some(id) = body.get("worker_id").and_then(|v| v.as_str()) {
        state.workers.list().into_iter().find(|w| w.id == id)
    } else {
        state
            .workers
            .list()
            .into_iter()
            .find(|w| w.port != 0)
    }
    .ok_or(coach_core::CoreError::NotFound("no render worker registered".into()))?;

    let path = if png { "render_png" } else { "render" };
    let url = format!("http://127.0.0.1:{}/{path}", worker.port);
    let forward = json!({
        "text": body.get("text").and_then(|v| v.as_str()).unwrap_or(""),
        "style": body.get("style").and_then(|v| v.as_str()).unwrap_or("ent"),
        "options": body.get("options").cloned().unwrap_or(serde_json::Value::Object(Default::default())),
        "sentence_split": body.get("sentence_split").and_then(|v| v.as_bool()).unwrap_or(false),
    });

    let response = state
        .http_client
        .post(url)
        .json(&forward)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| ApiError(coach_core::CoreError::NotImplemented(format!("render worker unreachable: {e}"))))?;

    if !response.status().is_success() {
        let status = response.status();
        let detail = response.text().await.unwrap_or_default();
        return Err(ApiError(coach_core::CoreError::NotImplemented(format!(
            "render worker {status}: {}",
            detail.chars().take(300).collect::<String>()
        ))));
    }

    // Byte-bound guard before returning.
    let text = response
        .text()
        .await
        .map_err(|e| ApiError(coach_core::CoreError::Serialization(serde_json::Error::io(std::io::Error::new(std::io::ErrorKind::Other, e)))))?;
    if text.len() > 5 * 1024 * 1024 {
        return Err(ApiError(coach_core::CoreError::Conflict(
            "render worker response too large".into(),
        )));
    }
    let parsed: serde_json::Value =
        serde_json::from_str(&text).unwrap_or_else(|_| json!({"svg": text, "meta": {}}));
    Ok(Json(parsed))
}



/// Allocate a free TCP port by asking the OS for an ephemeral bind, then
/// dropping the listener. Small TOCTOU window; workers retry elsewhere.
fn free_tcp_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr().map(|a| a.port()))
        .unwrap_or(0)
}


/// Probe the worker's `/health` endpoint with a short timeout.
async fn probe_worker(
    client: reqwest::Client,
    url: &str,
) -> bool {
    client
        .get(url)
        .timeout(std::time::Duration::from_secs(2))
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}


/// Resolve the Python interpreter that runs the render worker: explicit
/// `COACH_PY`, then common venv layouts in the working directory, then PATH.
fn resolve_python() -> String {
    if let Ok(path) = std::env::var("COACH_PY") {
        if !path.is_empty() && std::path::Path::new(&path).exists() {
            return path;
        }
    }
    // Walk from the cwd (and the workspace dir) upward, so a venv next to the
    // project root (e.g. "<repo>/../.venv-coach" when the server runs in a
    // subdir) is found instead of falling back to a bare `python3` that has
    // neither spaCy nor coach_spacy installed.
    for base in [
        std::env::current_dir().unwrap_or_default(),
        std::path::PathBuf::from(std::env::var("COACH_WORKSPACE").unwrap_or_default()),
    ] {
        let mut dir: Option<&std::path::Path> = Some(base.as_path());
        while let Some(ancestor) = dir {
            for name in [".venv-coach", ".venv"] {
                for bin in ["python3", "python"] {
                    let candidate = ancestor.join(name).join("bin").join(bin);
                    if candidate.is_file() {
                        return candidate.display().to_string();
                    }
                }
            }
            dir = ancestor.parent();
        }
    }
    "python3".into()
}
