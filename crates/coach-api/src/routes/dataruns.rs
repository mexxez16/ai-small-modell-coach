//! Training-data sessions + runs: the configurable curation pipeline.
//!
//! A DataRun is a user-defined list of stages; `execute` walks them in order
//! and writes produced items (with kind + provenance) into the run's dataset.
//! Stage kinds mirror the SGDiRAG Track B_1 reference pipeline:
//!
//! - `ssot_import`: structured JSON source (entities with
//!   examples/negative_examples/contrastive_examples, relations with
//!   examples) -> data items with kind example/negative/contrastive/pair,
//!   kebab labels, skipped-with-reason records, per-kind stats.
//! - `chunk`: plain-text chunking into raw items.
//! - `ai_annotate`: OpenAI-compatible provider call; results stay draft.
//! - `export`: per-head view over the dataset (kind filters, triple
//!   serialization) — artifact stats recorded on the run.

use axum::extract::{Path as AxumPath, State};
use axum::Json;
use coach_core::model::{new_id, now, DataItem, DataItemHistory, DataItemKind, DataItemStatus, DataRun, DataSession, DataSplit, Dataset, InputFields};
use coach_core::CoreError;
use serde_json::{json, Value};

use crate::api_error::ApiError;
use crate::state::AppState;

// --- Sessions CRUD -----------------------------------------------------------

#[derive(Debug, serde::Deserialize, utoipa::ToSchema)]
pub struct CreateDataSession {
    pub name: String,
    #[serde(default)]
    pub notes: String,
}

#[utoipa::path(post, path = "/api/data-sessions", request_body = CreateDataSession, responses((status = 200, body = DataSession)))]
pub async fn create_data_session(
    State(state): State<AppState>,
    Json(body): Json<CreateDataSession>,
) -> Result<Json<DataSession>, ApiError> {
    let ts = now();
    let session = DataSession {
        id: new_id(),
        name: body.name.trim().to_string(),
        notes: body.notes,
        created_at: ts,
        updated_at: ts,
    };
    if session.name.is_empty() {
        return Err(CoreError::InvalidQuery("session name required".into()).into());
    }
    Ok(Json(state.storage.put_data_session(session)?))
}

#[utoipa::path(get, path = "/api/data-sessions", responses((status = 200, body = [DataSession])))]
pub async fn list_data_sessions(
    State(state): State<AppState>,
) -> Result<Json<Vec<DataSession>>, ApiError> {
    Ok(Json(state.storage.list_data_sessions()?))
}

#[utoipa::path(get, path = "/api/data-sessions/{id}", responses((status = 200, body = DataSession)))]
pub async fn get_data_session(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<DataSession>, ApiError> {
    Ok(Json(state.storage.get_data_session(&id)?))
}

#[utoipa::path(delete, path = "/api/data-sessions/{id}", responses((status = 200)))]
pub async fn delete_data_session(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<axum::http::StatusCode, ApiError> {
    state.storage.delete_data_session(&id)?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

#[utoipa::path(get, path = "/api/data-sessions/{id}/runs", responses((status = 200, body = [DataRun])))]
pub async fn list_data_session_runs(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<Vec<DataRun>>, ApiError> {
    state.storage.get_data_session(&id)?;
    Ok(Json(state.storage.list_data_runs(Some(&id))?))
}

// --- Runs CRUD ---------------------------------------------------------------

#[derive(Debug, serde::Deserialize, utoipa::ToSchema)]
pub struct CreateDataRun {
    pub name: String,
    #[serde(default)]
    pub session_id: Option<String>,
    /// Stage list: [{kind, config}]; editable later via PUT.
    #[serde(default)]
    pub stages: Vec<Value>,
    #[serde(default)]
    pub dataset_id: Option<String>,
}

#[utoipa::path(post, path = "/api/data-runs", request_body = CreateDataRun, responses((status = 200, body = DataRun)))]
pub async fn create_data_run(
    State(state): State<AppState>,
    Json(body): Json<CreateDataRun>,
) -> Result<Json<DataRun>, ApiError> {
    let ts = now();
    if let Some(sid) = &body.session_id {
        state.storage.get_data_session(sid)?;
    }
    let run = DataRun {
        id: new_id(),
        session_id: body.session_id,
        name: body.name.trim().to_string(),
        status: "draft".into(),
        stages: Value::Array(body.stages),
        dataset_id: body.dataset_id,
        results: json!({}),
        created_at: ts,
        updated_at: ts,
    };
    if run.name.is_empty() {
        return Err(CoreError::InvalidQuery("run name required".into()).into());
    }
    Ok(Json(state.storage.put_data_run(run)?))
}

#[utoipa::path(get, path = "/api/data-runs", params(("session_id" = Option<String>, Query)), responses((status = 200, body = [DataRun])))]
pub async fn list_data_runs(
    State(state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Vec<DataRun>>, ApiError> {
    Ok(Json(
        state
            .storage
            .list_data_runs(params.get("session_id").map(String::as_str))?,
    ))
}

#[utoipa::path(get, path = "/api/data-runs/{id}", responses((status = 200, body = DataRun)))]
pub async fn get_data_run(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<DataRun>, ApiError> {
    Ok(Json(state.storage.get_data_run(&id)?))
}

#[derive(Debug, serde::Deserialize, utoipa::ToSchema)]
pub struct UpdateDataRun {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub stages: Option<Value>,
}

#[utoipa::path(put, path = "/api/data-runs/{id}", request_body = UpdateDataRun, responses((status = 200, body = DataRun)))]
pub async fn update_data_run(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Json(body): Json<UpdateDataRun>,
) -> Result<Json<DataRun>, ApiError> {
    let mut run = state.storage.get_data_run(&id)?;
    if let Some(name) = body.name {
        run.name = name.trim().to_string();
    }
    if let Some(sid) = body.session_id {
        if !sid.is_empty() {
            state.storage.get_data_session(&sid)?;
        }
        run.session_id = if sid.is_empty() { None } else { Some(sid) };
    }
    if let Some(stages) = body.stages {
        run.stages = stages;
    }
    run.updated_at = now();
    Ok(Json(state.storage.put_data_run(run)?))
}

#[utoipa::path(delete, path = "/api/data-runs/{id}", responses((status = 200)))]
pub async fn delete_data_run(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<axum::http::StatusCode, ApiError> {
    state.storage.delete_data_run(&id)?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

// --- Pipeline engine ---------------------------------------------------------

fn kebab(name: &str) -> String {
    let cleaned: String = name
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let mut out = cleaned
        .split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if out.is_empty() {
        out = "unlabeled".into();
    }
    out
}

/// Result of one ssot_import extraction pass.
struct ExtractOutput {
    items: Vec<DataItem>,
    skipped: Vec<Value>,
    stats: Value,
}

/// Structured SSOT extractor: understands the BFO-style entity/relation
/// registries (Track B_1 reference layout). Unknown shapes are rejected with
/// a descriptive error — better than silently producing an empty dataset.
fn extract_ssot(content: &str, dataset_id: &str, label_map: &Value) -> Result<ExtractOutput, CoreError> {
    use std::collections::BTreeMap;

    let root: Value = serde_json::from_str(content).map_err(|e| {
        CoreError::InvalidQuery(format!("source is not valid JSON: {e}"))
    })?;
    let fallback_label = |code: &str, class: &str| -> String {
        // Optional per-code label overrides from stage config.
        label_map
            .get(code)
            .and_then(|v| v.as_str())
            .map(|s| kebab(s))
            .unwrap_or_else(|| kebab(class))
    };

    let mut items: Vec<DataItem> = Vec::new();
    let mut skipped: Vec<Value> = Vec::new();
    let mut per_kind: BTreeMap<String, usize> = BTreeMap::new();
    let mut per_label: BTreeMap<String, usize> = BTreeMap::new();

    let push = |dataset_id: &str,
                kind: DataItemKind,
                label: String,
                fields: InputFields,
                source: String,
                items: &mut Vec<DataItem>,
                per_kind: &mut BTreeMap<String, usize>,
                per_label: &mut BTreeMap<String, usize>| {
        let kind_key = format!("{kind:?}").to_lowercase();
        let label_key = label.clone();
        let item = DataItem {
            id: new_id(),
            dataset_id: dataset_id.to_string(),
            input_fields: fields,
            label,
            kind,
            source,
            status: DataItemStatus::Raw,
            split: DataSplit::Unassigned,
            assignee: None,
            notes: None,
            updated_at: now(),
        };
        *per_kind.entry(kind_key).or_default() += 1;
        *per_label.entry(label_key).or_default() += 1;
        items.push(item);
    };

    let mut matched = false;

    // Entities: {code -> {bfo_class, examples, negative_examples, contrastive_examples}}
    if let Some(entities) = root.get("entities").and_then(|v| v.as_object()) {
        for (code, ent) in entities {
            let class = ent.get("bfo_class").and_then(|v| v.as_str()).unwrap_or(code);
            let label = fallback_label(code, class);
            let src = format!("entity#{code}");
            let take_strs = |key: &str| -> Vec<String> {
                ent.get(key)
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x.as_str().map(|s| s.trim().to_string()))
                            .filter(|s| !s.is_empty())
                            .collect()
                    })
                    .unwrap_or_default()
            };
            for text in take_strs("examples") {
                push(dataset_id, DataItemKind::Example, label.clone(),
                     InputFields { text: Some(text), ..Default::default() }, src.clone(),
                     &mut items, &mut per_kind, &mut per_label);
            }
            for text in take_strs("negative_examples") {
                push(dataset_id, DataItemKind::Negative, "not-an-entity".into(),
                     InputFields { text: Some(text), ..Default::default() }, src.clone(),
                     &mut items, &mut per_kind, &mut per_label);
            }
            for text in take_strs("contrastive_examples") {
                push(dataset_id, DataItemKind::Contrastive, String::new(),
                     InputFields { text: Some(text), ..Default::default() }, src.clone(),
                     &mut items, &mut per_kind, &mut per_label);
            }
        }
        matched = true;
    }

    // Relations: {code -> {relation_name, edge_relation_type, examples}} ->
    // formal triples split at the surface infix; misses are skipped with reason.
    if let Some(relations) = root.get("relations").and_then(|v| v.as_object()) {
        for (code, rel) in relations {
            let name = rel
                .get("relation_name")
                .and_then(|v| v.as_str())
                .unwrap_or(code);
            let label = fallback_label(code, name);
            let src = format!("relation#{code}");
            let surfaces: Vec<String> = [
                rel.get("relation_name").and_then(|v| v.as_str()),
                rel.get("edge_relation_type").and_then(|v| v.as_str()),
            ]
            .into_iter()
            .flatten()
            .filter(|s| !s.trim().is_empty())
            .map(|s| s.trim().to_string())
            .collect();
            for text in rel
                .get("examples")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(|s| s.trim().to_string()))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
            {
                let padded = format!(" {text} ");
                let hit = surfaces
                    .iter()
                    .find(|surface| padded.contains(&format!(" {surface} ")));
                let Some(surface) = hit else {
                    skipped.push(json!({"code": code, "text": truncate(&text, 120), "reason": "relation surface not found in text"}));
                    continue;
                };
                let text_norm = text.trim().trim_matches(|c: char| !c.is_alphanumeric()).to_string();
                let mut parts = text_norm.splitn(2, surface.as_str());
                let subject = parts.next().unwrap_or("").trim().trim_matches(|c: char| "- :;.".contains(c)).to_string();
                let rest = parts.next().unwrap_or("");
                let object = rest.trim_start_matches(surface.as_str()).trim().trim_matches(|c: char| "- :;.".contains(c)).to_string();
                if subject.is_empty() || object.is_empty() {
                    skipped.push(json!({"code": code, "text": truncate(&text, 120), "reason": "empty subject or object"}));
                    continue;
                }
                push(dataset_id, DataItemKind::Pair, label.clone(),
                     InputFields { subject: Some(subject), predicate: Some(surface.clone()), object: Some(object), text: None },
                     src.clone(), &mut items, &mut per_kind, &mut per_label);
            }
        }
        matched = true;
    }

    if !matched {
        return Err(CoreError::InvalidQuery(
            "source JSON matches no known SSOT shape (expected top-level 'entities' and/or 'relations' objects)".into(),
        ));
    }

    let classes = per_label.len();
    let skipped_count = skipped.len();
    let skipped_examples: Vec<Value> = skipped.iter().take(10).cloned().collect();
    let per_label_values: Vec<usize> = per_label.values().copied().collect();
    let min = per_label_values.iter().min().copied().unwrap_or(0);
    let max = per_label_values.iter().max().copied().unwrap_or(0);
    let avg = if per_label_values.is_empty() {
        0.0
    } else {
        per_label_values.iter().sum::<usize>() as f64 / per_label_values.len() as f64
    };

    Ok(ExtractOutput {
        items,
        skipped,
        stats: json!({
            "per_kind": per_kind,
            "per_label": per_label,
            "label_classes": classes,
            "min_per_class": min,
            "max_per_class": max,
            "avg_per_class": (avg * 100.0).round() / 100.0,
            "skipped": skipped_count,
            "skipped_examples": skipped_examples,
        }),
    })
}

fn truncate(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_string();
    }
    let cut: String = value.chars().take(max).collect();
    format!("{cut}…")
}

fn chunk_text(content: &str, strategy: &str, max_chars: usize, overlap: usize) -> Vec<String> {
    fn push_generic(out: &mut Vec<String>, buf: &str) {
        let trimmed = buf.trim();
        if !trimmed.is_empty() {
            out.push(trimmed.to_string());
        }
    }
    let mut out = Vec::new();
    match strategy {
        "sentence" => {
            let mut buf = String::new();
            let mut chars = content.chars().peekable();
            while let Some(c) = chars.next() {
                buf.push(c);
                if matches!(c, '.' | '!' | '?') {
                    if chars.peek().map(|n| n.is_whitespace()).unwrap_or(true) {
                        push_generic(&mut out, &buf);
                        buf.clear();
                    }
                }
            }
            push_generic(&mut out, &buf);
        }
        "window" => {
            let max = max_chars.max(1);
            let overlap = overlap.min(max - 1);
            let chars: Vec<char> = content.chars().collect();
            let mut start = 0usize;
            while start < chars.len() {
                let end = (start + max).min(chars.len());
                push_generic(&mut out, &chars[start..end].iter().collect::<String>());
                if end >= chars.len() {
                    break;
                }
                start += max - overlap;
            }
        }
        _ => {
            for para in content.split("\n\n") {
                let para = para.trim();
                if para.chars().count() <= max_chars {
                    push_generic(&mut out, para);
                } else {
                    out.extend(chunk_text(para, "window", max_chars, overlap));
                }
            }
        }
    }
    out.retain(|s| !s.is_empty());
    out
}

/// Execute the run pipeline (`POST /api/data-runs/{id}/execute`). Stages run
/// in order; each produces items in the run's dataset and per-stage stats in
/// the run's `results`. Any stage failure aborts with status=failed and the
/// stage index in the error.
#[utoipa::path(post, path = "/api/data-runs/{id}/execute", responses((status = 200, body = DataRun)))]
pub async fn execute_data_run(
    State(state): State<AppState>,
    AxumPath(run_id): AxumPath<String>,
) -> Result<Json<DataRun>, ApiError> {
    let mut run = state.storage.get_data_run(&run_id)?;
    let stages = run.stages.clone();
    let Value::Array(stage_list) = &stages else {
        run.status = "failed".into();
        return Ok(Json(state.storage.put_data_run(run)?));
    };

    // Resolve (or lazily create) the output dataset for this run.
    let dataset_id = match &run.dataset_id {
        Some(id) => {
            state.storage.get_dataset(id)?;
            id.clone()
        }
        None => {
            let ds = state.storage.create_dataset(Dataset {
                id: new_id(),
                name: format!("tdrun-{}", run.name.replace(' ', "-").to_lowercase()),
                source_ref: format!("data-run:{run_id}"),
                schema_version: 1,
                created_at: now(),
                updated_at: now(),
                versions: vec![],
                tags: vec![],
            })?;
            run.dataset_id = Some(ds.id.clone());
            ds.id
        }
    };

    run.status = "running".into();
    run.updated_at = now();
    state.storage.put_data_run(run.clone())?;

    let mut results = serde_json::Map::new();
    let mut stage_errors = Vec::new();
    let mut existing_texts = state
        .storage
        .list_data_items(&dataset_id, None)?
        .into_iter()
        .filter_map(|i| i.input_fields.text)
        .collect::<std::collections::HashSet<_>>();

    for (idx, stage) in stage_list.iter().enumerate() {
        let kind = stage.get("kind").and_then(|v| v.as_str()).unwrap_or("");
        let config = stage.get("config").cloned().unwrap_or(json!({}));
        let outcome: Result<Value, CoreError> = match kind {
            "ssot_import" => self::stage_ssot_import(
                &state, &config, &dataset_id, &mut existing_texts,
            ),
            "chunk" => self::stage_chunk(
                &state, &config, &dataset_id, &mut existing_texts,
            ),
            "ai_annotate" => self::stage_ai_annotate(
                &state, &config, &dataset_id,
            ).await,
            "export" => self::stage_export(&state, &config, &dataset_id),
            unknown => Err(CoreError::InvalidQuery(format!("unknown stage kind: {unknown}"))),
        };
        match outcome {
            Ok(stats) => {
                results.insert(format!("stage_{idx}_{kind}"), stats);
            }
            Err(err) => {
                stage_errors.push(format!("stage {idx} ({kind}): {err}"));
                break;
            }
        }
    }

    if stage_errors.is_empty() {
        run.status = "completed".into();
    } else {
        run.status = "failed".into();
        results.insert(
            "errors".into(),
            Value::Array(stage_errors.iter().map(|e| Value::String(e.clone())).collect()),
        );
    }
    run.results = Value::Object(results);
    run.updated_at = now();
    Ok(Json(state.storage.put_data_run(run)?))
}

// --- Stage implementations ---------------------------------------------------

fn stage_ssot_import(
    state: &crate::state::AppState,
    config: &Value,
    dataset_id: &str,
    existing_texts: &mut std::collections::HashSet<String>,
) -> Result<Value, CoreError> {
    let source_id = config
        .get("source_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| CoreError::InvalidQuery("ssot_import needs source_id".into()))?;
    let doc = state.storage.get_source_doc(source_id)?;
    let label_map = config.get("label_overrides").cloned().unwrap_or(json!({}));
    let out = extract_ssot(&doc.content, dataset_id, &label_map)?;
    let mut added = 0usize;
    for item in &out.items {
        let text_key = item
            .input_fields
            .text
            .clone()
            .or_else(|| Some(triple_text(item)));
        if let Some(key) = text_key {
            if existing_texts.contains(&key) {
                continue;
            }
            existing_texts.insert(key);
        }
        state.storage.put_data_item(item.clone())?;
        state.storage.append_item_history(DataItemHistory {
            id: new_id(),
            item_id: item.id.clone(),
            timestamp: now(),
            actor: "ssot-import".into(),
            field: "created".into(),
            old_value: Value::Null,
            new_value: json!({"kind": item.kind, "source": item.source}),
        })?;
        added += 1;
    }
    Ok(json!({"added": added, "extracted_total": out.items.len(), "skipped": out.skipped, "stats": out.stats}))
}

fn triple_text(item: &DataItem) -> String {
    format!(
        "{} <SEP> {} <SEP> {}",
        item.input_fields.subject.as_deref().unwrap_or(""),
        item.input_fields.predicate.as_deref().unwrap_or(""),
        item.input_fields.object.as_deref().unwrap_or(""),
    )
}

fn stage_chunk(
    state: &crate::state::AppState,
    config: &Value,
    dataset_id: &str,
    existing_texts: &mut std::collections::HashSet<String>,
) -> Result<Value, CoreError> {
    let source_id = config
        .get("source_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| CoreError::InvalidQuery("chunk needs source_id".into()))?;
    let doc = state.storage.get_source_doc(source_id)?;
    let strategy = config.get("strategy").and_then(|v| v.as_str()).unwrap_or("paragraph");
    let max_chars = config.get("max_chars").and_then(|v| v.as_u64()).unwrap_or(600).clamp(40, 20_000) as usize;
    let overlap = config.get("overlap").and_then(|v| v.as_u64()).unwrap_or(0).min(10_000) as usize;
    let label = config
        .get("label")
        .and_then(|v| v.as_str())
        .map(kebab)
        .unwrap_or_else(|| "imported-chunk".into());

    let chunks = chunk_text(&doc.content, strategy, max_chars, overlap);
    let mut added = 0usize;
    for (idx, chunk) in chunks.into_iter().enumerate() {
        if existing_texts.contains(&chunk) {
            continue;
        }
        existing_texts.insert(chunk.clone());
        let item = DataItem {
            id: new_id(),
            dataset_id: dataset_id.to_string(),
            input_fields: InputFields { text: Some(chunk), ..Default::default() },
            label: label.clone(),
            kind: DataItemKind::Example,
            source: format!("{}#{}", doc.name, idx),
            status: DataItemStatus::Raw,
            split: DataSplit::Unassigned,
            assignee: None,
            notes: None,
            updated_at: now(),
        };
        state.storage.put_data_item(item)?;
        added += 1;
    }
    Ok(json!({"added": added, "strategy": strategy, "max_chars": max_chars, "overlap": overlap}))
}

async fn stage_ai_annotate(
    state: &crate::state::AppState,
    config: &Value,
    dataset_id: &str,
) -> Result<Value, CoreError> {
    let items = state.storage.list_data_items(dataset_id, None)?;
    let kinds = config
        .get("kinds")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|k| k.as_str())
                .map(|k| k.to_lowercase())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let limit = config.get("limit").and_then(|v| v.as_u64()).unwrap_or(u64::MAX) as usize;
    let candidates: Vec<DataItem> = items
        .into_iter()
        .filter(|i| {
            let item_kind = format!("{:?}", i.kind).to_lowercase();
            kinds.is_empty() || kinds.contains(&item_kind)
        })
        .take(limit)
        .collect();

    if candidates.is_empty() {
        return Ok(json!({"annotated": 0, "note": "no matching items"}));
    }

    // Call the shared annotate handler synchronously over the dataset.
    // The chosen example types (output-settings step) flow into the prompt:
    // the model is told which kinds of examples it is producing/labeling.
    let kinds_hint = if kinds.is_empty() {
        String::new()
    } else {
        format!(
            "Beispiel-Typen im Fokus: {}.\n",
            kinds.join(", ")
        )
    };
    let base_instruction = config.get("instruction").and_then(|v| v.as_str()).unwrap_or("");
    let payload = json!({
        "provider_id": config.get("provider_id").and_then(|v| v.as_str()).unwrap_or(""),
        "item_ids": candidates.iter().map(|i| i.id.clone()).collect::<Vec<_>>(),
        "instruction": format!("{kinds_hint}{base_instruction}"),
        "labels": config.get("labels").and_then(|v| v.as_array()).map(|a| a.clone()).unwrap_or_default(),
        "model": config.get("model").and_then(|v| v.as_str()).map(|s| s.to_string()),
    });
    let state = state.clone();
    let response = crate::routes::api::annotate_items_blocking(state, payload).await?;

    Ok(response)
}

fn stage_export(
    state: &crate::state::AppState,
    config: &Value,
    dataset_id: &str,
) -> Result<Value, CoreError> {
    // Default heads mirror the Track B_1 reference: bfo_type (examples),
    // relation_type (pairs), entity_relevance (examples + negatives).
    let heads = config
        .get("heads")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_else(|| {
            vec![
                json!({"name": "type", "kinds": ["example"]}),
                json!({"name": "relation", "kinds": ["pair"], "triple_sep": " <SEP> "}),
                json!({"name": "relevance", "kinds": ["example", "negative"], "labels": {"example": "entity", "negative": "not-an-entity"}}),
            ]
        });

    let items = state.storage.list_data_items(dataset_id, None)?;
    let mut artifacts = Vec::new();
    for head in heads {
        let name = head.get("name").and_then(|v| v.as_str()).unwrap_or("head").to_string();
        let kinds: Vec<String> = head
            .get("kinds")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|k| k.as_str()).map(|k| k.to_lowercase()).collect())
            .unwrap_or(vec!["example".into()]);
        let sep = head.get("triple_sep").and_then(|v| v.as_str()).unwrap_or(" <SEP> ").to_string();
        let label_map = head.get("labels").cloned().unwrap_or(json!({}));

        let mut lines = Vec::new();
        for item in &items {
            let item_kind = format!("{:?}", item.kind).to_lowercase();
            if !kinds.contains(&item_kind) {
                continue;
            }
            let label = label_map
                .get(&item_kind)
                .and_then(|v| v.as_str())
                .map(kebab)
                .unwrap_or(item.label.clone());
            let line = if item_kind == "pair" {
                let sep_text = format!(
                    "{}{sep}{}{sep}{}",
                    item.input_fields.subject.as_deref().unwrap_or(""),
                    item.input_fields.predicate.as_deref().unwrap_or(""),
                    item.input_fields.object.as_deref().unwrap_or(""),
                );
                json!({
                    "subject": item.input_fields.subject,
                    "predicate": item.input_fields.predicate,
                    "object": item.input_fields.object,
                    "text": sep_text,
                    "label": label,
                    "kind": item_kind,
                    "source": item.source,
                })
            } else {
                json!({
                    "text": item.input_fields.text,
                    "label": label,
                    "kind": item_kind,
                    "source": item.source,
                })
            };
            lines.push(line.to_string());
        }
        artifacts.push(json!({
            "head": name,
            "rows": lines.len(),
            "filename": format!("{dataset_id}.{name}.jsonl"),
        }));
        if !lines.is_empty() {
            state.storage.append_item_history(DataItemHistory {
                id: new_id(),
                item_id: format!("{dataset_id}#{name}"),
                timestamp: now(),
                actor: "export-stage".into(),
                field: "artifact".into(),
                old_value: Value::Null,
                new_value: json!({"head": name, "rows": lines.len()}),
            })?;
        }
    }

    Ok(json!({
        "heads": artifacts,
        "note": "JSONL artifacts served via /api/datasets/{id}/export; DocBin via /export/docbin",
    }))
}
