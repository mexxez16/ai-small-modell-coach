//! Settings REST routes: chat provider management (persisted in
//! `<data_dir>/providers.json`).

use crate::api_error::ApiError;
use crate::providers::{new_provider, ProviderKind};
use crate::state::AppState;
use axum::{extract::Path as AxumPath, extract::State, http::StatusCode, Json};
use serde::Deserialize;
use serde_json::json;

use serde::Serialize;

#[derive(Debug, Deserialize)]
pub struct UpsertProvider {
    /// Present for updates; absent creates a new provider.
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    pub kind: String,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
}

pub async fn list_providers(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(json!({"providers": state.provider_store.list(true)}))
}

pub async fn upsert_provider(
    State(state): State<AppState>,
    Json(body): Json<UpsertProvider>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let kind = match body.kind.as_str() {
        "openai_compatible" | "openai-compatible" | "openai" => ProviderKind::OpenAiCompatible,
        "vibe" => ProviderKind::Vibe,
        other => {
            return Err(coach_core::CoreError::InvalidQuery(format!(
                "unknown provider kind {other:?} (use openai_compatible or vibe)"
            ))
            .into())
        }
    };
    let mut provider = match body.id.as_deref() {
        Some(existing_id) if !existing_id.is_empty() => {
            let mut existing = state
                .provider_store
                .get(existing_id)
                .ok_or(coach_core::CoreError::NotFound(format!("provider {existing_id}")))?;
            existing.name = body.name;
            existing.kind = kind;
            existing.base_url = body.base_url.filter(|u| !u.is_empty());
            existing.model = body.model.filter(|m| !m.is_empty());
            existing.api_key = body.api_key.filter(|k| !k.is_empty() && !k.ends_with('…'));
            existing
        }
        _ => new_provider(body.name, kind, body.base_url, body.api_key, body.model),
    };
    if provider.kind == ProviderKind::OpenAiCompatible && provider.base_url.is_none() {
        provider.base_url = Some("https://api.mistral.ai/v1".into());
    }
    let saved = state.provider_store.upsert(provider);
    Ok(Json(json!({"provider": saved.masked()})))
}

pub async fn delete_provider(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<StatusCode, ApiError> {
    if state.provider_store.delete(&id) {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(coach_core::CoreError::NotFound(format!("provider {id}")).into())
    }
}

/// Connectivity test: OpenAI-compatible → GET {base}/models with the key;
/// vibe → binary present on PATH.
pub async fn test_provider(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let provider = state
        .provider_store
        .get(&id)
        .ok_or(coach_core::CoreError::NotFound(format!("provider {id}")))?;
    match &provider.kind {
        ProviderKind::Vibe => {
            let available = std::env::var("PATH")
                .map(|path| {
                    std::env::split_paths(&path)
                        .map(|dir| dir.join("vibe"))
                        .find(|candidate| candidate.is_file())
                        .is_some()
                })
                .unwrap_or(false)
                || std::env::var("COACH_VIBE_BIN")
                    .map(|bin| std::path::Path::new(&bin).exists())
                    .unwrap_or(false);
            Ok(Json(json!({
                "ok": available,
                "detail": if available { "vibe binary found" } else { "vibe binary not on PATH (or set COACH_VIBE_BIN)" },
            })))
        }
        ProviderKind::OpenAiCompatible => {
            let base = provider
                .base_url
                .clone()
                .unwrap_or_else(|| "https://api.mistral.ai/v1".into());
            let url = format!("{}/models", base.trim_end_matches('/'));
            let mut request = state.http_client.get(&url);
            if let Some(key) = provider.api_key.as_deref() {
                if !key.is_empty() && !key.ends_with('…') {
                    request = request.bearer_auth(key);
                }
            }
            let result = tokio::time::timeout(std::time::Duration::from_secs(10), request.send()).await;
            match result {
                Ok(Ok(response)) => {
                    let status = response.status();
                    Ok(Json(json!({
                        "ok": status.is_success(),
                        "detail": format!("GET {url} -> {status}"),
                    })))
                }
                Ok(Err(err)) => Ok(Json(json!({
                    "ok": false,
                    "detail": format!("connection failed: {err}"),
                }))),
                Err(_) => Ok(Json(json!({
                    "ok": false,
                    "detail": "timeout after 10s",
                }))),
            }
        }
    }
}

/// Path of the Vibe CLI user config (`~/.vibe/config.toml`).
fn vibe_config_path() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").map(|home| {
        std::path::PathBuf::from(home).join(".vibe").join("config.toml")
    })
}

/// Read the `active_model` line out of the Vibe CLI config, if present.
fn read_vibe_active_model(path: &std::path::Path) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("active_model") {
            let value = trimmed
                .split_once('=')
                .and_then(|(_, rhs)| rhs.trim().trim_matches('"').to_string().into());
            return value;
        }
    }
    None
}

#[derive(Debug, Serialize)]
pub struct VibeSettings {
    pub config_path: String,
    pub config_exists: bool,
    pub active_model: Option<String>,
}

pub async fn get_vibe_settings() -> Json<VibeSettings> {
    let path = vibe_config_path();
    let config_path = path
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "~/.vibe/config.toml".into());
    let active_model = path.as_ref().and_then(|p| read_vibe_active_model(p));
    Json(VibeSettings {
        config_path,
        config_exists: path.as_ref().map(|p| p.exists()).unwrap_or(false),
        active_model,
    })
}

#[derive(Debug, Deserialize)]
pub struct VibeModelBody {
    pub active_model: String,
}

/// Write `active_model` into the Vibe CLI config (create the file with just
/// this key when it does not exist yet; otherwise replace the existing line
/// in place so all other settings are preserved).
pub async fn set_vibe_model(
    Json(body): Json<VibeModelBody>,
) -> Result<Json<VibeSettings>, ApiError> {
    let model = body.active_model.trim().to_string();
    if model.is_empty() {
        return Err(coach_core::CoreError::InvalidQuery(
            "active_model must not be empty".into(),
        )
        .into());
    }
    let Some(path) = vibe_config_path() else {
        return Err(coach_core::CoreError::InvalidQuery(
            "cannot resolve $HOME — no Vibe config location".into(),
        )
        .into());
    };
    let content = std::fs::read_to_string(&path).unwrap_or_default();
    let mut replaced = false;
    let mut lines: Vec<String> = Vec::new();
    for line in content.lines() {
        if !replaced && line.trim_start().starts_with("active_model") {
            lines.push(format!("active_model = \"{}\"", model.replace('"', "")));
            replaced = true;
        } else {
            lines.push(line.to_string());
        }
    }
    if !replaced {
        lines.insert(0, format!("active_model = \"{}\"", model.replace('"', "")));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| coach_core::CoreError::Io(e))?;
    }
    std::fs::write(&path, lines.join("\n") + "\n")
        .map_err(|e| coach_core::CoreError::Io(e))?;
    Ok(Json(VibeSettings {
        config_path: path.display().to_string(),
        config_exists: true,
        active_model: Some(model),
    }))
}
