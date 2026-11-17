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
pub async fn data_profile(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
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
    Ok(Json(json!({ "datasets": out })))
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
    /// Which provider to query (must exist and be openai_compatible). The
    /// assistant uses the store's *unmasked* key like the chat dock does.
    pub provider_id: String,
    /// Conversation so far, oldest first, WITHOUT the system prompt
    /// (it is prepended here). Roles: "user" | "assistant".
    pub messages: Vec<Value>,
}

/// `POST /api/assistant/turn` — one advising turn. The reply is expected in
/// the JSON tool protocol; recognized write actions become pending
/// proposals (returned in the response, persisted for the workbench).
pub async fn assistant_turn(
    State(state): State<AppState>,
    Json(body): Json<TurnRequest>,
) -> Result<Json<Value>, ApiError> {
    if body.messages.is_empty() {
        return Err(ApiError(coach_core::CoreError::InvalidQuery(
            "messages must not be empty".into(),
        )));
    }
    let provider = state.provider_store.get(&body.provider_id).ok_or_else(|| {
        ApiError(coach_core::CoreError::NotFound(format!(
            "provider {}",
            body.provider_id
        )))
    })?;
    if provider.kind != ProviderKind::OpenAiCompatible {
        return Err(ApiError(coach_core::CoreError::InvalidQuery(
            "assistant requires an OpenAI-compatible provider (vibe CLI cannot do the JSON protocol)".into(),
        )));
    }

    // Non-streaming completion (assistant answers are small JSON envelopes).
    let base = provider
        .base_url
        .clone()
        .unwrap_or_else(|| "https://api.mistral.ai/v1".into());
    let url = format!("{}/chat/completions", base.trim_end_matches('/'));
    let model = provider
        .model
        .clone()
        .unwrap_or_else(|| "mistral-small-latest".into());
    let mut messages = vec![json!({"role": "system", "content": build_system_prompt(&state)})];
    messages.extend(body.messages.iter().cloned());
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
    let raw_text = value["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("")
        .to_string();

    // Parse the tool protocol; fall back to a plain answer when the model
    // forgot the envelope (never leave the user staring at raw JSON).
    let (reply_text, actions) = parse_protocol(&raw_text);

    // Queue write actions as proposals; they are NOT applied here.
    let mut all = read_proposals();
    let mut queued: Vec<Proposal> = Vec::new();
    for action in &actions {
        match action["tool"].as_str() {
            Some("propose_step_config") => {
                let step = action["step"].as_str().unwrap_or_default().to_string();
                if !CONTEXT_STEPS.contains(&step.as_str()) {
                    continue; // silently ignore unknown steps (log below)
                }
                let payload = json!({"tool": "propose_step_config", "step": step, "config": action["config"].clone()});
                all.push(new_proposal("step_config", Some(step.clone()), &payload));
                queued.push(all.last().cloned().expect("just pushed"));
            }
            Some("propose_goal") => {
                let payload = json!({"tool": "propose_goal", "goal": action["goal"].clone()});
                all.push(new_proposal("goal", None, &payload));
                queued.push(all.last().cloned().expect("just pushed"));
            }
            _ => {}
        }
    }
    if !queued.is_empty() {
        write_proposals(&all)?;
    }

    Ok(Json(json!({
        "reply": reply_text,
        "protocol": if actions.is_empty() { "plain" } else { "json" },
        "proposals": queued,
    })))
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

/// The system prompt: role, guard rails, JSON protocol and the CURRENT
/// context (goal, step configs, docs mirror versions). Rebuilt every turn so
/// edits from the workbench are visible to the next answer immediately.
fn build_system_prompt(state: &AppState) -> String {
    let mut parts: Vec<String> = Vec::new();
    parts.push(
        "Du bist der Trainingsdaten-Assistent des AI Small Modell Coach, ein Berater \
         (kein Entscheider). Der Nutzer hat Trainingsdaten-Pipelines vorbereitet und \
         will mit spaCy Komponenten trainingieren (z. B. textcat, textcat_multilabel, \
         ner, relation extraction). Antworte auf Deutsch, prägnant und konkret."
            .to_string(),
    );
    parts.push(
        "VERHALTEN: (1) Wenn der Zweck der Daten unklar ist, frage gezielt nach \
         (Ziel-Komponente, Klassenraum, Sprache, Menge). (2) Lies dann die \
         mitgelieferten Konfigurationen und empfiehl konkrete Änderungen. \
         (3) Ändere NIEMALS etwas direkt: gib Änderungswünsche als Aktionen \
         im JSON-Protokoll zurück. (4) Antworte AUSSCHLIESSLICH mit einem \
         JSON-Objekt: {\"text\": \"<Antwort>\", \"actions\": [...]}. \
         Für 'actions' gilt: entweder weglassen oder Array mit Einträgen der Form \
         {\"tool\": \"propose_step_config\", \"step\": \"<input|preprocessing|output_settings|processing|curation>\", \"config\": {...}} \
         oder {\"tool\": \"propose_goal\", \"goal\": {...}}."
            .to_string(),
    );

    // Current goal (may be missing — that is an important hint itself).
    match std::fs::read_to_string(goal_path()) {
        Ok(text) => match serde_json::from_str::<Value>(&text) {
            Ok(goal) => parts.push(format!("AKTUELLES ZIEL (config/goal.json):\n{}", serde_json::to_string_pretty(&goal).unwrap_or_default())),
            Err(_) => parts.push("AKTUELLES ZIEL: vorhanden, aber ungültiges JSON.".to_string()),
        },
        Err(_) => parts.push("AKTUELLES ZIEL: noch nicht gesetzt (config/goal.json fehlt) — bitte den Nutzer zuerst nach Zweck, Komponente und Klassenraum fragen.".to_string()),
    }

    // Step configs 1-6 (they are small; inline them verbatim so the model can
    // reason about the real values, not a summary).
    for step in CONTEXT_STEPS {
        let path = data_dir().join("config").join(format!("{step}.json"));
        match std::fs::read_to_string(&path) {
            Ok(text) => parts.push(format!("CONFIG {step}:\n{}", truncate_chars(&text, 6000))),
            Err(_) => parts.push(format!("CONFIG {step}: (fehlt)")),
        }
    }

    // Datasets (names + counts only; the full profile is a separate endpoint).
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

    // Docs mirror versions (the model may reference them; full-text comes
    // later via an explicit lookup action).
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
    let needle = q.q.trim().to_lowercase();
    if needle.is_empty() {
        return Err(ApiError(coach_core::CoreError::InvalidQuery(
            "q must not be empty".into(),
        )));
    }
    let version = q.version.unwrap_or_else(|| newest_mirror());
    let base = docs_root().join(&version);
    if !base.is_dir() {
        return Err(ApiError(coach_core::CoreError::NotFound(format!(
            "no docs mirror for version {version}"
        ))));
    }
    let limit = q.limit.unwrap_or(20).min(100);
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
    Ok(Json(json!({ "version": version, "matches": results })))
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
