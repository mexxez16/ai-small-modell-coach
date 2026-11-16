//! Settings REST routes: chat provider management (persisted in
//! `<data_dir>/providers.json`).

use crate::api_error::ApiError;
use crate::providers::{new_provider, ProviderKind};
use crate::state::AppState;
use axum::{extract::Path as AxumPath, extract::State, http::StatusCode, Json};
use serde::Deserialize;
use serde_json::json;

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
