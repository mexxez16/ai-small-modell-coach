//! Goal assistant: LLM-backed advisor for the training-data pipeline.
//!
//! The assistant answers questions about the current training goal, the
//! pipeline step configs (1-6) and the accumulated data. It NEVER applies
//! anything on its own: configuration changes arrive as *proposals* that the
//! workbench shows with Accept/Reject actions; accepting applies them through
//! the normal config write path (`<data_dir>/config/...`).
//!
//! Tool protocol (plain JSON inside the model answer, fenced or not):
//! `{"text": "<user-facing answer>", "actions": [...]}` with actions:
//! - `{"tool": "propose_step_config", "step": "<step>", "config": {...}}`
//! - `{"tool": "propose_goal", "goal": {...}}`
//!
//! This deliberately uses no function-calling API: the JSON protocol keeps
//! the assistant compatible with every model the provider surfed offers.

use crate::api_error::ApiError;
use crate::providers::ProviderKind;
use crate::state::AppState;
use axum::{extract::Query, extract::State, Json};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// Pipeline steps mirrored into the assistant context (`CONFIG_STEPS` order).
const CONTEXT_STEPS: &[&str] = &[
    "input",
    "preprocessing",
    "output_settings",
    "processing",
    "curation",
];

/// Tools the assistant may invoke (gated per profile).
const KNOWN_TOOLS: &[&str] = &[
    "docs_search",
    "get_data_profile",
    "get_app_status",
    "get_run_status",
    "propose_step_config",
    "propose_goal",
    "handoff",
];

/// One assistant personality, stored in `<data_dir>/assistant/profiles.json`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Profile {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub system_prompt: String,
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    #[serde(default)]
    pub starter_prompts: Vec<String>,
}

fn profiles_path() -> std::path::PathBuf {
    assistant_dir().join("profiles.json")
}

/// Read all profiles, seeding the shipped three on first access so the
/// store never boots empty.
fn read_profiles() -> Vec<Profile> {
    if let Ok(bytes) = std::fs::read(profiles_path()) {
        if let Ok(profiles) = serde_json::from_slice::<Vec<Profile>>(&bytes) {
            if !profiles.is_empty() {
                return profiles;
            }
        }
    }
    let seeded = default_profiles();
    let _ = ensure_assistant_dir();
    if let Ok(text) = serde_json::to_string_pretty(&seeded) {
        let _ = std::fs::write(profiles_path(), text);
    }
    seeded
}

fn ensure_assistant_dir() -> std::io::Result<std::path::PathBuf> {
    std::fs::create_dir_all(assistant_dir())?;
    Ok(assistant_dir())
}

/// The shipped three personalities; replaced only when the file must be seeded.
fn default_profiles() -> Vec<Profile> {
    vec![
        Profile {
            id: "coach".into(),
            name: "Small Model Coach".into(),
            description: "Organisator: erklärt die App und leitet an die Spezialisten weiter."
                .into(),
            system_prompt: "Du bist der Small Model Coach-Assistent: Du kennst die App \
                 (Overview/Dashboard, Finetuning: Sessions/Runs/Models, Data: Training Data \
                 Sessions + Runs mit den Assistent-Stufen, Visual Analysis, Workbench mit \
                 Settings). Du erklärst Zusammenhänge, stellst Fragen, wenn Ziele unklar sind, \
                 und empfiehlst, welcher Spezialist-Assistent (Data, Finetuning) weiterhelfen \
                 kann. Du antwortest auf Deutsch, prägnant und konkret."
                .into(),
            allowed_tools: vec!["handoff".into(), "get_app_status".into()],
            starter_prompts: vec![
                "Wofür ist welcher Assistent zuständig?".into(),
                "Erkläre mir den Trainingsdaten-Ablauf der App.".into(),
            ],
        },
        Profile {
            id: "data".into(),
            name: "Data".into(),
            description: "Trainingsdaten-Pipelines: Import, Annotation, Kuratierung, Export."
                .into(),
            system_prompt: "Du bist der Trainingsdaten-Assistent des AI Small Modell Coach, \
                 ein Berater (kein Entscheider). Der Nutzer bereitet Trainingsdaten für spaCy \
                 Komponenten vor (textcat, ner, relation extraction). Antworte auf Deutsch, \
                 prägnant und konkret."
                .into(),
            allowed_tools: vec![
                "docs_search".into(),
                "get_data_profile".into(),
                "get_app_status".into(),
                "propose_step_config".into(),
                "propose_goal".into(),
            ],
            starter_prompts: vec![
                "Bewerte mein Datenprofil für eine Textklassifikation.".into(),
                "Schlage mir ein Trainings-Ziel vor.".into(),
            ],
        },
        Profile {
            id: "finetuning".into(),
            name: "Finetuning".into(),
            description: "Sessions, Runs und Modelle im Überblick.".into(),
            system_prompt: "Du bist der Fine-Tuning-Assistent: Du erläuterst Sessions, Runs \
                 und Modellstände der App, liest Status zusammen und gibst Handlungsempfehlungen. \
                 Antworte auf Deutsch, prägnant und konkret."
                .into(),
            allowed_tools: vec![
                "docs_search".into(),
                "get_app_status".into(),
                "get_run_status".into(),
            ],
            starter_prompts: vec![
                "Gib mir einen Überblick über meine Sessions und Runs.".into(),
                "Welche Modelle sind bereit, was fehlt?".into(),
            ],
        },
    ]
}


/// How many model round-trips one turn may use for the tool loop.
const MAX_TOOL_ROUNDS: usize = 4;

fn data_dir() -> std::path::PathBuf {
    crate::state::resolve_data_dir()
}

fn goal_path() -> std::path::PathBuf {
    data_dir().join("config").join("goal.json")
}

fn assistant_dir() -> std::path::PathBuf {
    data_dir().join("assistant")
}

fn proposals_path() -> std::path::PathBuf {
    assistant_dir().join("proposals.json")
}

fn docs_root() -> std::path::PathBuf {
    data_dir().join("docs").join("spacy")
}

/// `GET /api/assistant/profiles` — all profiles (seeds ensured on first call).
pub async fn list_profiles(State(_state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let profiles = read_profiles();
    Ok(Json(json!({ "profiles": profiles })))
}

/// `PUT /api/assistant/profiles/{id}` — replace one profile; tool names are
/// validated against the known set so the editor cannot inject anything else.
pub async fn upsert_profile(
    State(_state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
    Json(mut profile): Json<Profile>,
) -> Result<Json<Value>, ApiError> {
    if profile.id != id {
        return Err(ApiError(coach_core::CoreError::InvalidQuery(format!(
            "profile id mismatch: body `{}` vs path `{id}`",
            profile.id
        ))));
    }
    if !KNOWN_TOOLS.contains(&profile.id.as_str())
        && !["coach", "data", "finetuning"].contains(&profile.id.as_str())
    {
        return Err(ApiError(coach_core::CoreError::InvalidQuery(format!(
            "unknown profile id `{id}`"
        ))));
    }
    profile
        .allowed_tools
        .retain(|tool| KNOWN_TOOLS.contains(&tool.as_str()));
    let mut profiles = read_profiles();
    match profiles.iter_mut().find(|p| p.id == id) {
        Some(existing) => *existing = profile.clone(),
        None => profiles.push(profile.clone()),
    }
    ensure_assistant_dir().map_err(coach_core::CoreError::Io)?;
    let text = serde_json::to_string_pretty(&profiles)
        .map_err(coach_core::CoreError::Serialization)?;
    std::fs::write(profiles_path(), text).map_err(coach_core::CoreError::Io)?;
    Ok(Json(json!({ "profile": profile })))
}

// ---------------------------------------------------------------------------
// Status endpoints (all cheap; no subprocess of their own)
// ---------------------------------------------------------------------------

/// `GET /api/assistant/status` — mirrors the goal, the step configs
/// (styles: just presence + key summary), provider availability and the
/// docs mirror state in one call for the workbench header.
pub async fn assistant_status(State(state): State<AppState>) -> Json<Value> {
    let goal = std::fs::read_to_string(goal_path())
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok());

    let mut steps = serde_json::Map::new();
    for step in CONTEXT_STEPS {
        let path = data_dir().join("config").join(format!("{step}.json"));
        let present = path.exists();
        steps.insert(
            step.to_string(),
            json!({
                "present": present,
                "bytes": std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0),
            }),
        );
    }

    let providers = state.provider_store.list(true);
    let openai_compatible = providers
        .iter()
        .filter(|p| p.kind == ProviderKind::OpenAiCompatible)
        .count();

    let mut mirror_versions = Vec::new();
    if let Ok(read) = std::fs::read_dir(docs_root()) {
        for entry in read.flatten() {
            if entry.path().is_dir() {
                mirror_versions.push(entry.file_name().to_string_lossy().to_string());
            }
        }
    }
    mirror_versions.sort();

    Json(json!({
        "goal": goal,
        "config_steps": steps,
        "providers_openai_compatible": openai_compatible,
        "docs_mirror_versions": mirror_versions,
    }))
}

/// Python probe script: prints one JSON line with the spaCy facts that the
/// assistant grounds its advice on. Fails soft on every error.
const SPACY_PROBE_SCRIPT: &str = r#"import json, sys
out = {"python": sys.version.split()[0], "spacy_version": None, "installed_models": [], "error": None}
try:
    import spacy
    out["spacy_version"] = spacy.__version__
    out["installed_models"] = sorted(spacy.util.get_installed_models())
except Exception as exc:
    out["error"] = str(exc)
print(json.dumps(out))
"#;

#[derive(Debug, Deserialize)]
pub struct SpacyProbeQuery {
    /// Python interpreter to probe; defaults to `COACH_SPACY_PYTHON` or
    /// `python3`. Keep it an explicit parameter so the UI can offer
    /// environments instead of guessing.
    #[serde(default)]
    pub python: Option<String>,
}

/// `GET /api/assistant/spacy-status` — probe one Python environment for
/// spaCy. spaCy is explicitly NOT a hard requirement of the coach; the
/// answer says so (`installed: false`) instead of failing.
pub async fn spacy_status(Query(q): Query<SpacyProbeQuery>) -> Json<Value> {
    let python = q
        .python
        .or_else(|| std::env::var("COACH_SPACY_PYTHON").ok())
        .unwrap_or_else(|| "python3".to_string());

    use tokio::process::Command;
    let probe = tokio::time::timeout(
        std::time::Duration::from_secs(20),
        Command::new(&python).arg("-c").arg(SPACY_PROBE_SCRIPT).output(),
    )
    .await;

    match probe {
        Err(_) => Json(json!({
            "python": python,
            "installed": false,
            "error": "probe timed out after 20s",
        })),
        Ok(Err(err)) => Json(json!({
            "python": python,
            "installed": false,
            "error": format!("cannot spawn python: {err}"),
        })),
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let stderr_tail: String = String::from_utf8_lossy(&output.stderr)
                .lines()
                .last()
                .unwrap_or("")
                .chars()
                .take(200)
                .collect();
            match serde_json::from_str::<Value>(&stdout) {
                Ok(mut value) => {
                    value["python"] = json!(python);
                    if value["error"].is_null() {
                        value["error"] = Value::Null;
                    }
                    Json(value)
                }
                Err(_) => Json(json!({
                    "python": python,
                    "installed": false,
                    "error": "probe did not print JSON",
                    "stderr_head": stderr_tail,
                })),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Data profile (deterministic, no LLM)
// ---------------------------------------------------------------------------

/// `GET /api/assistant/data-profile` — aggregate stats across all datasets:
/// per-dataset item counts by status, by kind and the label distribution,
/// plus global sanity signals (items without text, duplicate texts).
fn data_profile_core(state: &AppState) -> Result<Value, ApiError> {
    let datasets = state
        .storage
        .list_datasets()
        .map_err(ApiError::from)?;
    let mut out = Vec::new();
    for dataset in datasets {
        let items = state
            .storage
            .list_data_items(&dataset.id, None)
            .map_err(ApiError::from)?;
        let mut by_status: BTreeMap<String, usize> = BTreeMap::new();
        let mut by_kind: BTreeMap<String, usize> = BTreeMap::new();
        let mut by_label: BTreeMap<String, usize> = BTreeMap::new();
        let mut missing_text = 0usize;
        let mut dup_texts = 0usize;
        let mut seen_texts: std::collections::HashSet<String> = std::collections::HashSet::new();
        for item in &items {
            *by_status.entry(format!("{:?}", item.status).to_lowercase()).or_default() += 1;
            *by_kind.entry(format!("{:?}", item.kind).to_lowercase()).or_default() += 1;
            *by_label.entry(item.label.clone()).or_default() += 1;
            let text = item
                .input_fields
                .text
                .clone()
                .or_else(|| item.input_fields.subject.clone());
            match text {
                None => missing_text += 1,
                Some(text) => {
                    if !seen_texts.insert(text) {
                        dup_texts += 1;
                    }
                }
            }
        }
        out.push(json!({
            "dataset_id": dataset.id,
            "name": dataset.name,
            "source_ref": dataset.source_ref,
            "updated_at": dataset.updated_at.to_rfc3339(),
            "items": items.len(),
            "by_status": by_status,
            "by_kind": by_kind,
            "by_label": by_label,
            "items_without_text": missing_text,
            "duplicate_texts": dup_texts,
        }));
    }
    Ok(json!({ "datasets": out }))
}

/// `GET /api/assistant/data-profile`
pub async fn data_profile(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(data_profile_core(&state)?))
}

// ---------------------------------------------------------------------------
// Assistant turn
// ---------------------------------------------------------------------------

/// One pending proposal produced by the assistant, persisted under
/// `<data_dir>/assistant/proposals.json` until accepted or rejected.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Proposal {
    pub id: String,
    pub created_at: String,
    /// "step_config" or "goal" — where the proposal would be applied.
    pub kind: String,
    /// Step name for `step_config` proposals, absent for `goal`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<String>,
    pub summary: String,
    pub payload: Value,
}

/// Aggregated app status for the coach assistant: sessions, runs and models.
fn app_status_core(state: &AppState) -> Value {
    let sessions = state.storage.list_sessions().unwrap_or_default();
    let runs = state.storage.list_runs(None).unwrap_or_default();
    let models = state.storage.list_model_entries().unwrap_or_default();
    json!({
        "sessions": { "count": sessions.len(),
            "items": sessions.iter().map(|s| json!({"id": s.id, "name": s.name})).collect::<Vec<_>>() },
        "runs": { "count": runs.len(),
            "items": runs.iter().map(|r| json!({"id": r.id, "name": r.name, "status": format!("{:?}", r.status), "model": r.model})).collect::<Vec<_>>() },
        "models": { "count": models.len(),
            "items": models.iter().map(|m| json!({"id": m.id, "name": m.name, "kind": m.kind, "path": m.path})).collect::<Vec<_>>() },
    })
}

/// Run status for the finetuning assistant: sessions + their runs.
fn run_status_core(state: &AppState) -> Value {
    let sessions = state.storage.list_sessions().unwrap_or_default();
    let runs = state.storage.list_runs(None).unwrap_or_default();
    json!({
        "sessions": sessions.iter().map(|s| json!({"id": s.id, "name": s.name, "status": s.status})).collect::<Vec<_>>(),
        "runs": runs.iter().map(|r| json!({"id": r.id, "name": r.name, "status": format!("{:?}", r.status), "model": r.model, "dataset_id": r.dataset_id})).collect::<Vec<_>>(),
    })
}

/// Pending proposals, read from `<data_dir>/assistant/proposals.json`.
fn read_proposals() -> Vec<Proposal> {
    std::fs::read(proposals_path())
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn write_proposals(all: &[Proposal]) -> Result<(), ApiError> {
    std::fs::create_dir_all(assistant_dir()).map_err(coach_core::CoreError::Io)?;
    let text = serde_json::to_string_pretty(all)
        .map_err(coach_core::CoreError::Serialization)?;
    std::fs::write(proposals_path(), text).map_err(coach_core::CoreError::Io)?;
    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct TurnRequest {
    /// Which provider to query. Empty/absent resolves through the
    /// default-assistant setting (fallback: first openai_compatible).
    #[serde(default)]
    pub provider_id: String,
    /// Which assistant profile drives this turn (default: "coach").
    #[serde(default = "default_profile_id")]
    pub profile_id: String,
    /// Conversation so far, oldest first, WITHOUT the system prompt
    /// (it is prepended here). Roles: "user" | "assistant".
    pub messages: Vec<Value>,
}

fn default_profile_id() -> String {
    "coach".to_string()
}

/// `POST /api/assistant/turn` — one advising turn, streamed as NDJSON lines:
/// `round_start` per model call, `tool` per executed read action,
/// `thinking` with intermediate opportunities to see the reasoning,
/// `handoff` for the coach's referral card, `final` with the finished
/// reply. Write actions (`propose_*`) never execute — they land in the
/// pending-proposal queue and are reported via a `proposals` event.
pub async fn assistant_turn(
    State(state): State<AppState>,
    Json(body): Json<TurnRequest>,
) -> Result<axum::response::Response, ApiError> {
    if body.messages.is_empty() {
        return Err(ApiError(coach_core::CoreError::InvalidQuery(
            "messages must not be empty".into(),
        )));
    }
    let provider = match resolve_provider(&state, &body.provider_id) {
        Some(provider) => provider,
        None => {
            return Err(ApiError(coach_core::CoreError::NotFound(
                "no usable OpenAI-compatible provider (set a default in Settings)".into(),
            )));
        }
    };
    if provider.kind != ProviderKind::OpenAiCompatible {
        return Err(ApiError(coach_core::CoreError::InvalidQuery(
            "assistant requires an OpenAI-compatible provider (vibe CLI cannot do the JSON protocol)".into(),
        )));
    }
    let profile = read_profiles()
        .into_iter()
        .find(|p| p.id == body.profile_id)
        .ok_or_else(|| {
            ApiError(coach_core::CoreError::NotFound(format!(
                "profile {}",
                body.profile_id
            )))
        })?;

    let (tx, rx) = tokio::sync::mpsc::channel::<Value>(8);
    tokio::spawn(async move {
        run_turn(&state, &provider, &profile, body.messages, &tx).await;
    });

    let stream = futures_util::stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|event| {
            let line = serde_json::to_string(&event).unwrap_or_else(|_| "{}".into());
            (Ok::<_, std::convert::Infallible>(format!("{line}\n")), rx)
        })
    });
    Ok(axum::response::Response::builder()
        .header("content-type", "application/x-ndjson")
        .body(axum::body::Body::from_stream(stream))
        .expect("static response builder"))
}

/// Provider resolution for the assistant: explicit id → default-assistant
/// setting → first OpenAI-compatible provider.
fn resolve_provider(state: &AppState, provider_id: &str) -> Option<crate::providers::Provider> {
    if !provider_id.trim().is_empty() {
        if let Some(provider) = state.provider_store.get(provider_id) {
            if provider.kind == ProviderKind::OpenAiCompatible {
                return Some(provider);
            }
            return None;
        }
        return None;
    }
    if let Some(default_id) = state.provider_store.default_assistant() {
        if let Some(provider) = state.provider_store.get(&default_id) {
            if provider.kind == ProviderKind::OpenAiCompatible {
                return Some(provider);
            }
        }
    }
    state
        .provider_store
        .list(true)
        .into_iter()
        .find(|p| p.kind == ProviderKind::OpenAiCompatible)
}

/// Full turn: model loop with live NDJSON event emission. Runs detached in a
/// spawned task; ending events (`final`/`error`) close the channel.
async fn run_turn(
    state: &AppState,
    provider: &crate::providers::Provider,
    profile: &Profile,
    user_messages: Vec<Value>,
    tx: &tokio::sync::mpsc::Sender<Value>,
) {
    let send = |tx: &tokio::sync::mpsc::Sender<Value>, event: Value| {
        let tx = tx.clone();
        async move {
            // Receiver gone (client cancelled): drop the rest silently.
            let _ = tx.send(event).await;
        }
    };

    let mut convo: Vec<Value> =
        vec![json!({"role": "system", "content": build_system_prompt(state, profile)})];
    convo.extend(user_messages.iter().cloned());

    let mut all = read_proposals();
    let mut queued: Vec<Proposal> = Vec::new();
    let mut tool_trace: Vec<Value> = Vec::new();
    let mut final_reply = String::new();

    for round in 0..MAX_TOOL_ROUNDS {
        send(tx, json!({"type": "round_start", "round": round + 1})).await;
        let raw_text = match call_model_once(state, provider, &convo).await {
            Ok(text) => text,
            Err(err) => {
                send(tx, json!({"type": "error", "message": err.0.to_string()})).await;
                return;
            }
        };
        let (text, actions) = parse_protocol(&raw_text);
        // Tool gate: only actions the profile allows survive.
        let actions: Vec<Value> = actions
            .into_iter()
            .filter(|action| {
                action["tool"]
                    .as_str()
                    .map(|tool| profile.allowed_tools.iter().any(|allowed| allowed == tool))
                    .unwrap_or(false)
            })
            .collect();

        // 1) Write actions: become proposals, nothing else.
        let mut has_writes = false;
        for action in &actions {
            match action["tool"].as_str() {
                Some("propose_step_config") => {
                    let step = action["step"].as_str().unwrap_or_default().to_string();
                    if !CONTEXT_STEPS.contains(&step.as_str()) {
                        continue;
                    }
                    let payload = json!({"tool": "propose_step_config", "step": step, "config": action["config"].clone()});
                    all.push(new_proposal("step_config", Some(step.clone()), &payload));
                    queued.push(all.last().cloned().expect("just pushed"));
                    has_writes = true;
                }
                Some("propose_goal") => {
                    let payload = json!({"tool": "propose_goal", "goal": action["goal"].clone()});
                    all.push(new_proposal("goal", None, &payload));
                    queued.push(all.last().cloned().expect("just pushed"));
                    has_writes = true;
                }
                _ => {}
            }
        }

        // 2) Read actions: execute now, feed results into the next call.
        let mut results: Vec<Value> = Vec::new();
        for action in &actions {
            match action["tool"].as_str() {
                Some("docs_search") => {
                    let query = action["query"]
                        .as_str()
                        .or_else(|| action["q"].as_str())
                        .unwrap_or("");
                    let version = action["version"].as_str();
                    let executed = docs_search_core(query, version, 10);
                    let (ok, value) = match executed {
                        Ok(value) => (true, value),
                        Err(err) => (false, json!({ "error": err.0.to_string() })),
                    };
                    tool_trace.push(json!({
                        "round": round + 1,
                        "tool": "docs_search",
                        "query": query,
                        "ok": ok,
                    }));
                    send(tx, json!({
                        "type": "tool",
                        "round": round + 1,
                        "tool": "docs_search",
                        "query": query,
                        "ok": ok,
                    })).await;
                    results.push(json!({"tool": "docs_search", "ok": ok, "result": value}));
                }
                Some("get_data_profile") => {
                    let executed = data_profile_core(&state);
                    let (ok, value) = match executed {
                        Ok(value) => (true, value),
                        Err(err) => (false, json!({ "error": err.0.to_string() })),
                    };
                    tool_trace.push(json!({
                        "round": round + 1,
                        "tool": "get_data_profile",
                        "ok": ok,
                    }));
                    send(tx, json!({
                        "type": "tool",
                        "round": round + 1,
                        "tool": "get_data_profile",
                        "ok": ok,
                    })).await;
                    results.push(json!({"tool": "get_data_profile", "ok": ok, "result": value}));
                }
                Some("get_app_status") => {
                    let status = app_status_core(state);
                    tool_trace.push(json!({
                        "round": round + 1,
                        "tool": "get_app_status",
                        "ok": true,
                    }));
                    send(tx, json!({
                        "type": "tool",
                        "round": round + 1,
                        "tool": "get_app_status",
                        "ok": true,
                    })).await;
                    results.push(json!({"tool": "get_app_status", "ok": true, "result": status}));
                }
                Some("get_run_status") => {
                    let status = run_status_core(state);
                    tool_trace.push(json!({
                        "round": round + 1,
                        "tool": "get_run_status",
                        "ok": true,
                    }));
                    send(tx, json!({
                        "type": "tool",
                        "round": round + 1,
                        "tool": "get_run_status",
                        "ok": true,
                    })).await;
                    results.push(json!({"tool": "get_run_status", "ok": true, "result": status}));
                }
                // The coach's referral: emitted as a card event for the dock,
                // never fed back into the model (the turn ends after this
                // round without further tool results).
                Some("handoff") => {
                    let target = action["profile"].as_str().unwrap_or("");
                    if target == "data" || target == "finetuning" {
                        tool_trace.push(json!({
                            "round": round + 1,
                            "tool": "handoff",
                            "profile": target,
                            "ok": true,
                        }));
                        send(tx, json!({
                            "type": "handoff",
                            "profile": target,
                            "reason": action["reason"].as_str().unwrap_or(""),
                            "opening": action["opening"].as_str().unwrap_or(""),
                        })).await;
                    }
                }
                _ => {}
            }
        }

        // No tool usage this round: the model's text is the final answer.
        if results.is_empty() {
            // Intermediate narrative first — the user watches it arrive.
            if !text.is_empty() {
                send(tx, json!({"type": "thinking", "round": round + 1, "text": text})).await;
            }
            final_reply = if has_writes && text.is_empty() {
                "Ich habe Vorschläge hinterlegt — sieh im Bereich „Vorschläge“ nach."
                    .to_string()
            } else {
                text
            };
            break;
        }

        // Feed the model's intermediate answer and the tool results back.
        convo.push(json!({"role": "assistant", "content": raw_text}));
        convo.push(json!({
            "role": "user",
            "content": format!(
                "TOOL RESULTS (vom System ausgeführt):\n{}",
                serde_json::to_string_pretty(&results)
                    .unwrap_or_else(|_| "[]".into())
            ),
        }));
        // The model continues in the next round.
    }

    if !queued.is_empty() {
        send(tx, json!({"type": "proposals", "count": queued.len()})).await;
        if let Err(err) = write_proposals(&all) {
            send(tx, json!({"type": "error", "message": err.0.to_string()})).await;
            return;
        }
    }

    let reply = if final_reply.is_empty() {
        "(Keine abschließende Antwort — Werkzeug-Limit erreicht.)".to_string()
    } else {
        final_reply
    };
    send(tx, json!({
        "type": "final",
        "reply": reply,
        "tool_trace": tool_trace,
        "proposals": queued,
    })).await;
}

/// One non-streaming chat completion against the given provider.
async fn call_model_once(
    state: &AppState,
    provider: &crate::providers::Provider,
    messages: &[Value],
) -> Result<String, ApiError> {
    let base = provider
        .base_url
        .clone()
        .unwrap_or_else(|| "https://api.mistral.ai/v1".into());
    let url = format!("{}/chat/completions", base.trim_end_matches('/'));
    let model = provider
        .model
        .clone()
        .unwrap_or_else(|| "mistral-small-latest".into());
    let payload = json!({
        "model": model,
        "messages": messages,
        "stream": false,
        "temperature": 0.2,
    });
    let mut request = state.http_client.post(&url).json(&payload);
    if let Some(key) = provider.api_key.as_deref() {
        if !key.is_empty() && !key.ends_with('…') {
            request = request.bearer_auth(key);
        }
    }
    let response = request
        .timeout(std::time::Duration::from_secs(120))
        .send()
        .await
        .map_err(|e| {
            ApiError(coach_core::CoreError::InvalidQuery(format!(
                "provider unreachable ({url}): {e}"
            )))
        })?;
    if !response.status().is_success() {
        let status = response.status();
        let detail = response.text().await.unwrap_or_default();
        return Err(ApiError(coach_core::CoreError::InvalidQuery(format!(
            "provider answered {status}: {}",
            truncate_chars(&detail, 300)
        ))));
    }
    let value: Value = response.json().await.map_err(|e| {
        ApiError(coach_core::CoreError::InvalidQuery(format!(
            "provider returned invalid JSON: {e}"
        )))
    })?;
    Ok(value["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("")
        .to_string())
}

fn new_proposal(kind: &str, step: Option<String>, payload: &Value) -> Proposal {
    Proposal {
        id: coach_core::model::new_id(),
        created_at: chrono::Utc::now().to_rfc3339(),
        kind: kind.to_string(),
        step,
        summary: match kind {
            "step_config" => format!("Step-Config für `{}`", payload["step"].as_str().unwrap_or("?")),
            _ => "Trainings-Ziel (goal.json)".to_string(),
        },
        payload: payload.clone(),
    }
}

/// Split a model answer into (user text, recognized actions).
fn parse_protocol(raw: &str) -> (String, Vec<Value>) {
    let text = strip_fence(raw);
    if let Ok(value) = serde_json::from_str::<Value>(&text) {
        if value.is_object() && value.get("text").is_some() {
            let reply = value["text"].as_str().unwrap_or("").to_string();
            let actions = value["actions"].as_array().cloned().unwrap_or_default();
            return (reply, actions);
        }
    }
    (raw.trim().to_string(), Vec::new())
}

fn strip_fence(text: &str) -> String {
    let trimmed = text.trim();
    let inner = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"));
    match inner {
        Some(rest) => rest.strip_suffix("```").unwrap_or(rest).trim().to_string(),
        None => trimmed.to_string(),
    }
}

fn truncate_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        format!("{}…", text.chars().take(max).collect::<String>())
    }
}

/// The system prompt: profile role, JSON protocol rules and the CURRENT
/// context, gated per the profile's tools. Rebuilt every turn so edits from
/// the workbench are visible to the next answer immediately.
fn build_system_prompt(state: &AppState, profile: &Profile) -> String {
    let mut parts: Vec<String> = Vec::new();
    parts.push(profile.system_prompt.clone());
    // Protocol rules (profile prompt comes first — rules stay fixed).
    parts.push(
        "VERHALTEN: Antworte im JSON-Protokoll \
         {\"text\": \"<Antwort>\", \"actions\": [...]}; schreibe nie direkt, \
         sondern nur über Aktionen; bleib in deiner Rolle."
            .to_string(),
    );

    // Action catalog only for the tools the profile actually has.
    let mut actions_docs: Vec<String> = Vec::new();
    for tool in &profile.allowed_tools {
        let doc = match tool.as_str() {
            "docs_search" => "{\"tool\": \"docs_search\", \"query\": \"<Text>\"} — durchsucht den lokalen spaCy-Doku-Spiegel;",
            "get_data_profile" => "{\"tool\": \"get_data_profile\"} — Datenprofil aller Datasets (Items, Label-Verteilung, ohne-Text, Duplikate);",
            "get_app_status" => "{\"tool\": \"get_app_status\"} — App-Status (Sessions, Runs, Modelle);",
            "get_run_status" => "{\"tool\": \"get_run_status\"} — Sessions und ihre Runs im Detail;",
            "propose_step_config" => "{\"tool\": \"propose_step_config\", \"step\": \"<input|preprocessing|output_settings|processing|curation>\", \"config\": {...}} — Step-Config als Vorschlag;",
            "propose_goal" => "{\"tool\": \"propose_goal\", \"goal\": {...}} — Trainings-Ziel als Vorschlag;",
            "handoff" => "{\"tool\": \"handoff\", \"profile\": \"<data|finetuning>\", \"reason\": \"<warum>\", \"opening\": \"<fertige Einstiegsfrage>\"} — Übergabekarte an den Nutzer;",
            _ => continue,
        };
        actions_docs.push(doc.to_string());
    }
    if !actions_docs.is_empty() {
        parts.push(format!("VERFÜGBARE AKTIONEN (nur diese):\n{}", actions_docs.join("\n")));
    }

    // Context blocks per the tools the profile can act on:
    if profile.allowed_tools.iter().any(|t| t == "get_data_profile") {
        // Step configs 1-6 inline (small files; reason, not summarize).
        for step in CONTEXT_STEPS {
            let path = data_dir().join("config").join(format!("{step}.json"));
            match std::fs::read_to_string(&path) {
                Ok(text) => parts.push(format!("CONFIG {step}:\n{}", truncate_chars(&text, 6000))),
                Err(_) => parts.push(format!("CONFIG {step}: (fehlt)")),
            }
        }
        // Datasets (names + counts only; the full profile is a tool away).
        let datasets: Vec<Value> = state
            .storage
            .list_datasets()
            .unwrap_or_default()
            .iter()
            .map(|d| json!({"id": d.id, "name": d.name, "source_ref": d.source_ref}))
            .collect();
        parts.push(format!(
            "DATASETS (übersicht):\n{}",
            serde_json::to_string_pretty(&datasets).unwrap_or_else(|_| "[]".into())
        ));
    }
    if profile.allowed_tools.iter().any(|t| t == "get_run_status") {
        // Run overview for the finetuning assistant (sessions + runs).
        let sessions = state.storage.list_sessions().unwrap_or_default();
        let runs = state.storage.list_runs(None).unwrap_or_default();
        let summary = json!({
            "sessions": { "count": sessions.len(),
                "items": sessions.iter().map(|s| json!({"id": s.id, "name": s.name})).collect::<Vec<_>>() },
            "runs": { "count": runs.len(),
                "items": runs.iter().map(|r| json!({"id": r.id, "name": r.name, "status": format!("{:?}", r.status), "model": r.model})).collect::<Vec<_>>() },
        });
        parts.push(format!(
            "RUN-STATUS (übersicht):\n{}",
            serde_json::to_string_pretty(&summary).unwrap_or_else(|_| "[]".into())
        ));
    }

    // Docs mirror versions (the model may reference them; full-text comes
    // later via an explicit docs_search action).
    let mut mirror = Vec::new();
    if let Ok(read) = std::fs::read_dir(docs_root()) {
        for entry in read.flatten() {
            if entry.path().is_dir() {
                mirror.push(entry.file_name().to_string_lossy().to_string());
            }
        }
    }
    if mirror.is_empty() {
        parts.push("SPACY-DOCS-MIRROR: nicht vorhanden — wenn Fragen die spaCy-Details betreffen, sage dem Nutzer, dass er den Mirror über den Button 'Docs spiegeln' erst anlegen sollte.".to_string());
    } else {
        parts.push(format!("SPACY-DOCS-MIRROR Versionen: {}", mirror.join(", ")));
    }

    parts.join("\n\n---\n\n")
}

// ---------------------------------------------------------------------------
// Proposals
// ---------------------------------------------------------------------------

/// `GET /api/assistant/proposals` — pending proposals for the workbench.
pub async fn list_proposals(State(_state): State<AppState>) -> Json<Value> {
    Json(json!({ "proposals": read_proposals() }))
}

#[derive(Debug, Deserialize)]
pub struct ProposalDecision {
    pub id: String,
}

/// `POST /api/assistant/proposals/accept` — apply a pending proposal through
/// the normal config write path and drop it from the pending list.
pub async fn accept_proposal(
    State(_state): State<AppState>,
    Json(body): Json<ProposalDecision>,
) -> Result<Json<Value>, ApiError> {
    let mut all = read_proposals();
    let idx = all
        .iter()
        .position(|p| p.id == body.id)
        .ok_or_else(|| ApiError(coach_core::CoreError::NotFound(format!("proposal {}", body.id))))?;
    let proposal = all.remove(idx);
    write_proposals_touched(&proposal, &all)?;

    match proposal.kind.as_str() {
        "step_config" => {
            let step = proposal
                .step
                .clone()
                .unwrap_or_else(|| proposal.payload["step"].as_str().unwrap_or_default().to_string());
            apply_step_config(&step, &proposal.payload["config"])?;
        }
        "goal" => apply_goal(&proposal.payload["goal"])?,
        other => {
            return Err(ApiError(coach_core::CoreError::InvalidQuery(format!(
                "proposal kind `{other}` is not applicable"
            ))));
        }
    }
    Ok(Json(json!({ "accepted": body.id })))
}

/// Mirror of the accepted proposal for audit (`accepted/` log file).
fn write_proposals_touched(proposal: &Proposal, remaining: &[Proposal]) -> Result<(), ApiError> {
    write_proposals(remaining)?;
    let log_path = assistant_dir().join("accepted.json");
    let mut log: Vec<serde_json::Value> = std::fs::read(&log_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    log.push(json!({
        "id": proposal.id,
        "accepted_at": chrono::Utc::now().to_rfc3339(),
        "kind": proposal.kind,
        "step": proposal.step,
        "summary": proposal.summary,
        "payload": proposal.payload,
    }));
    let text = serde_json::to_string_pretty(&log)
        .map_err(coach_core::CoreError::Serialization)?;
    std::fs::write(&log_path, text).map_err(coach_core::CoreError::Io)?;
    Ok(())
}

/// `POST /api/assistant/proposals/reject` — drop without applying.
pub async fn reject_proposal(
    State(_state): State<AppState>,
    Json(body): Json<ProposalDecision>,
) -> Result<Json<Value>, ApiError> {
    let mut all = read_proposals();
    let before = all.len();
    all.retain(|p| p.id != body.id);
    if all.len() == before {
        return Err(ApiError(coach_core::CoreError::NotFound(format!(
            "proposal {}",
            body.id
        ))));
    }
    write_proposals(&all)?;
    Ok(Json(json!({ "rejected": body.id })))
}

/// Apply a step-config proposal: same write conventions as the manual
/// `PUT /api/config/{step}` endpoint (pretty JSON, config dir auto-seeded).
fn apply_step_config(step: &str, config: &Value) -> Result<(), ApiError> {
    if !CONTEXT_STEPS.contains(&step) {
        return Err(ApiError(coach_core::CoreError::InvalidQuery(format!(
            "unknown step {step}"
        ))));
    }
    let path = data_dir().join("config").join(format!("{step}.json"));
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(coach_core::CoreError::Io)?;
    }
    let text = serde_json::to_string_pretty(config)
        .map_err(coach_core::CoreError::Serialization)?;
    std::fs::write(&path, text).map_err(coach_core::CoreError::Io)?;
    Ok(())
}

/// Apply a goal proposal: `<data_dir>/config/goal.json`.
fn apply_goal(goal: &Value) -> Result<(), ApiError> {
    std::fs::create_dir_all(goal_path().parent().expect("config dir")).map_err(coach_core::CoreError::Io)?;
    let text = serde_json::to_string_pretty(goal)
        .map_err(coach_core::CoreError::Serialization)?;
    std::fs::write(goal_path(), text).map_err(coach_core::CoreError::Io)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Docs mirror (git clone, versioned, cached under data_dir)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct DocsPullRequest {
    /// spaCy version tag to mirror, e.g. "v3.8.3" — used as the directory name.
    pub version: String,
    /// Repo to clone; override for tests. Defaults to explosion/spaCy.
    #[serde(default = "default_docs_repo")]
    pub repo: String,
    /// Sparse-checkout subdirectory. Defaults to `website/docs`.
    #[serde(default = "default_docs_subdir")]
    pub subdir: String,
}

fn default_docs_repo() -> String {
    "https://github.com/explosion/spaCy.git".to_string()
}

fn default_docs_subdir() -> String {
    "website/docs".to_string()
}

fn sanitize_version(version: &str) -> String {
    version
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '-' || *c == '_')
        .collect::<String>()
}

/// `POST /api/assistant/docs/pull` — shallow git clone of the spaCy repo,
/// sparse to the docs subtree, copied into `<data_dir>/docs/spacy/<version>/`.
/// Replaces any existing mirror of that version.
pub async fn docs_pull(
    Json(body): Json<DocsPullRequest>,
) -> Result<Json<Value>, ApiError> {
    let version = sanitize_version(&body.version);
    if version.is_empty() {
        return Err(ApiError(coach_core::CoreError::InvalidQuery(
            "version must not be empty".into(),
        )));
    }
    let temp = std::env::temp_dir().join(format!("coach-docs-pull-{}", coach_core::model::new_id()));
    let target = docs_root().join(&version);

    // Guard rails before shell work: target must stay inside docs_root.
    if !target.starts_with(docs_root()) {
        return Err(ApiError(coach_core::CoreError::InvalidQuery(
            "illegal version path".into(),
        )));
    }
    std::fs::create_dir_all(docs_root()).map_err(coach_core::CoreError::Io)?;

    let target_display = target.display().to_string();
    tokio::task::spawn_blocking(move || -> Result<usize, String> {
        use std::process::Command;
        let run = |args: &[&std::ffi::OsStr]| -> Result<(), String> {
            let output = Command::new("git")
                .args(args)
                .output()
                .map_err(|e| format!("git failed: {e}"))?;
            if !output.status.success() {
                return Err(format!(
                    "git {:?} failed: {}",
                    args,
                    String::from_utf8_lossy(&output.stderr)
                ));
            }
            Ok(())
        };
        let temp_os = temp.clone().into_os_string();
        run(&[
            "clone".as_ref(),
            "--depth".as_ref(),
            "1".as_ref(),
            "--filter=blob:none".as_ref(),
            "--sparse".as_ref(),
            &temp_os,
        ])
        .map_err(|e| format!("clone: {e}"))?;
        run(&[
            "-C".as_ref(),
            &temp_os,
            "sparse-checkout".as_ref(),
            "set".as_ref(),
            body.subdir.as_ref(),
        ])
        .map_err(|e| format!("sparse-checkout: {e}"))?;

        let src = temp.join(&body.subdir);
        if !src.is_dir() {
            return Err(format!("subdir {} not found in repo", body.subdir));
        }
        // Replace any previous mirror of that version.
        if target.exists() {
            std::fs::remove_dir_all(&target).map_err(|e| format!("cleanup: {e}"))?;
        }
        std::fs::create_dir_all(&target).map_err(|e| format!("mktarget: {e}"))?;

        // Copy only the doc sources (md/mdx), flattened, to keep the mirror small.
        let mut count = 0usize;
        let mut stack = vec![src.clone()];
        while let Some(dir) = stack.pop() {
            let read = std::fs::read_dir(&dir).map_err(|e| format!("readdir: {e}"))?;
            for entry in read.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else if matches!(
                    path.extension().and_then(|e| e.to_str()),
                    Some("md") | Some("mdx") | Some("md.txt")
                ) {
                    let rel = path
                        .strip_prefix(&src)
                        .map_err(|e| format!("relpath: {e}"))?;
                    let dest_dir = target.join(rel.parent().unwrap_or(std::path::Path::new("")));
                    std::fs::create_dir_all(&dest_dir).map_err(|e| format!("mkdir: {e}"))?;
                    let file_name = path.file_name().expect("file");
                    std::fs::copy(&path, dest_dir.join(file_name))
                        .map_err(|e| format!("copy: {e}"))?;
                    count += 1;
                }
            }
        }
        let _ = std::fs::remove_dir_all(&temp);
        Ok(count)
    })
    .await
    .map_err(|e| ApiError(coach_core::CoreError::InvalidQuery(format!("mirror task failed: {e}"))))?
    .map_err(|e| ApiError(coach_core::CoreError::InvalidQuery(format!("docs pull failed: {e}"))))?;

    Ok(Json(json!({
        "version": version,
        "path": target_display,
    })))
}

#[derive(Debug, Deserialize)]
pub struct DocsSearchQuery {
    pub q: String,
    /// Mirror version; defaults to the newest pulled one.
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
}

/// `GET /api/assistant/docs/search` — naive substring search over the
/// mirrored docs; enough for the lookup use case, no index needed.
pub async fn docs_search(Query(q): Query<DocsSearchQuery>) -> Result<Json<Value>, ApiError> {
    Ok(Json(docs_search_core(
        &q.q,
        q.version.as_deref(),
        q.limit.unwrap_or(20),
    )?))
}

/// Readable full-text search over one docs mirror version. Shared by the
/// HTTP endpoint and the assistant tool loop.
fn docs_search_core(q: &str, version: Option<&str>, limit: usize) -> Result<Value, ApiError> {
    let needle = q.trim().to_lowercase();
    if needle.is_empty() {
        return Err(ApiError(coach_core::CoreError::InvalidQuery(
            "q must not be empty".into(),
        )));
    }
    let version = version
        .map(str::to_string)
        .unwrap_or_else(newest_mirror);
    let base = docs_root().join(&version);
    if !base.is_dir() {
        return Err(ApiError(coach_core::CoreError::NotFound(format!(
            "no docs mirror for version {version}"
        ))));
    }
    let limit = limit.min(100);
    let mut results: Vec<Value> = Vec::new();
    let mut stack = vec![base.clone()];
    while let Some(dir) = stack.pop() {
        if results.len() >= limit {
            break;
        }
        let read = std::fs::read_dir(&dir).map_err(coach_core::CoreError::Io)?;
        for entry in read.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let lower = text.to_lowercase();
            if !lower.contains(&needle) {
                continue;
            }
            // First matching line, with a bit of context.
            let snippet = text
                .lines()
                .find(|line| line.to_lowercase().contains(&needle))
                .map(|line| line.trim())
                .unwrap_or("");
            results.push(json!({
                "file": path.strip_prefix(&base).unwrap_or(&path).to_string_lossy(),
                "snippet": truncate_chars(snippet, 240),
            }));
            if results.len() >= limit {
                break;
            }
        }
    }
    Ok(json!({ "version": version, "matches": results }))
}

fn newest_mirror() -> String {
    let mut versions: Vec<String> = Vec::new();
    if let Ok(read) = std::fs::read_dir(docs_root()) {
        for entry in read.flatten() {
            if entry.path().is_dir() {
                versions.push(entry.file_name().to_string_lossy().to_string());
            }
        }
    }
    versions.sort();
    versions.pop().unwrap_or_default()
}
