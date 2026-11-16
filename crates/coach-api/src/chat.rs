//! Chat connectivity: connects the chat dock to configured providers.
//!
//! Protocol on `/ws/chat` (JSON text frames both ways):
//!
//! client → server:
//!   {"type":"chat","provider_id":"…","text":"…"}
//! server → client:
//!   {"type":"system",...}            — lifecycle info, provider list
//!   {"type":"delta","content":"…"}   — streamed provider output
//!   {"type":"done"}                  — turn finished
//!   {"type":"error","message":"…"}   — provider/socket error
//!
//! Provider kinds:
//! - `openai_compatible`: POST {base_url}/chat/completions with Bearer key,
//!   streaming SSE, `choices[0].delta.content` forwarded as deltas. Covers
//!   Mistral API, Ollama (`http://localhost:11434/v1`), LM Studio, llama.cpp
//!   server, vLLM — every OpenAI-compatible endpoint.
//! - `vibe`: runs the Mistral Vibe CLI headless (`vibe -p <text> --output
//!   streaming`) scoped to the workspace dir, one run per chat message, and
//!   parses its JSONL protocol into delta frames. Never host-wide: cwd is the
//!   workspace, no elevated context.

use crate::providers::{Provider, ProviderKind};
use crate::state::AppState;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::Response;
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;

pub async fn chat_ws(State(state): State<AppState>, ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(move |socket| async move {
        if let Err(err) = run_chat_socket(state, socket).await {
            tracing::warn!("chat socket ended: {err:#}");
        }
    })
}

async fn run_chat_socket(state: AppState, socket: WebSocket) -> anyhow::Result<()> {
    let (mut sender, mut receiver) = socket.split();

    // Announce available providers so the dock can populate its selector.
    let providers: Vec<_> = state
        .provider_store
        .list(true)
        .iter()
        .map(|p| json!({"id": p.id, "name": p.name, "kind": p.kind}))
        .collect();
    sender
        .send(Message::Text(
            json!({"type":"system","providers":providers}).to_string().into(),
        ))
        .await?;

    loop {
        let Some(Ok(msg)) = receiver.next().await else { break };
        let text = match msg {
            Message::Text(text) => text,
            Message::Ping(payload) => {
                sender.send(Message::Pong(payload)).await?;
                continue;
            }
            Message::Close(_) => break,
            _ => continue,
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        match value.get("type").and_then(|v| v.as_str()) {
            Some("chat") => {}
            // Client can re-request the provider list (e.g. after
            // adding a provider in Settings mid-session).
            Some("providers") => {
                let providers: Vec<_> = state
                    .provider_store
                    .list(true)
                    .iter()
                    .map(|p| json!({"id": p.id, "name": p.name, "kind": p.kind}))
                    .collect();
                sender
                    .send(Message::Text(
                        json!({"type":"system","providers":providers})
                            .to_string()
                            .into(),
                    ))
                    .await?;
                continue;
            }
            _ => continue,
        }
        let user_text = value.get("text").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let Some(provider_id) = value.get("provider_id").and_then(|v| v.as_str()) else {
            send_error(&mut sender, "provider_id missing").await?;
            continue;
        };
        let Some(provider) = state.provider_store.get(provider_id) else {
            send_error(&mut sender, &format!("unknown provider {provider_id}")).await?;
            continue;
        };

        match &provider.kind {
            ProviderKind::OpenAiCompatible => {
                if let Err(err) = stream_openai(&state, &provider, &user_text, &mut sender).await {
                    send_error(&mut sender, &format!("stream failed: {err:#}")).await?;
                }
            }
            // A provider failure must never tear down the socket; both
            // runners report their own errors as error frames.
            ProviderKind::Vibe => {
                if let Err(err) = run_vibe_headless(&state, &user_text, &mut sender).await {
                    send_error(&mut sender, &format!("vibe failed: {err:#}")).await?;
                }
            }
        }
        sender
            .send(Message::Text(json!({"type":"done"}).to_string().into()))
            .await?;
    }

    Ok(())
}

async fn send_error(
    sender: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    message: &str,
) -> anyhow::Result<()> {
    sender
        .send(Message::Text(
            json!({"type":"error","message":message}).to_string().into(),
        ))
        .await
        .map_err(anyhow::Error::from)
}

/// Run one headless vibe turn: `vibe -p <text> --output streaming`, scoped to
/// the workspace directory, one process per chat message. The streaming
/// protocol is JSON lines; assistant message entries carry their full text as
/// a growing snapshot, so only the newly appended suffix is forwarded.
async fn run_vibe_headless(
    state: &AppState,
    user_text: &str,
    sender: &mut futures_util::stream::SplitSink<WebSocket, Message>,
) -> anyhow::Result<()> {
    use std::process::Stdio;
    use tokio::io::AsyncBufReadExt;

    let Some(vibe_bin) = find_vibe(state) else {
        send_error(
            sender,
            "'vibe' binary not found — install Mistral Vibe, set COACH_VIBE_BIN, \
             or use an OpenAI-compatible provider",
        )
        .await?;
        return Ok(());
    };

    let mut command = tokio::process::Command::new(&vibe_bin);
    command
        .arg("-p")
        .arg(user_text)
        .args(["--output", "streaming"])
        .arg("--workdir")
        .arg(&state.workspace_dir)
        .env("COACH_WORKSPACE", &state.workspace_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(err) => {
            send_error(sender, &format!("vibe spawn failed: {err}")).await?;
            return Ok(());
        }
    };

    // Drain stderr in the background (keyring notices, errors) so the pipe
    // cannot fill up and block the child; keep a small tail for diagnostics.
    let stderr_tail: Arc<tokio::sync::Mutex<Vec<String>>> = Arc::default();
    let stderr_task = match child.stderr.take() {
        Some(stderr) => {
            let tail = Arc::clone(&stderr_tail);
            Some(tokio::spawn(async move {
                let mut lines = tokio::io::BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let mut buf = tail.lock().await;
                    if buf.len() >= 8 {
                        buf.remove(0);
                    }
                    buf.push(line);
                }
            }))
        }
        None => None,
    };

    let Some(stdout) = child.stdout.take() else {
        send_error(sender, "vibe produced no stdout").await?;
        let _ = child.kill().await;
        return Ok(());
    };
    let mut reader = tokio::io::BufReader::new(stdout).lines();
    // Assistant text arrives as snapshot updates keyed by entry id; remember
    // how many bytes of each snapshot were already forwarded.
    let mut emitted: HashMap<String, usize> = HashMap::new();

    while let Ok(Some(line)) = reader.next_line().await {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
            // Not protocol output — forward as plain text.
            sender
                .send(Message::Text(
                    json!({"type":"delta","content":line}).to_string().into(),
                ))
                .await?;
            continue;
        };
        let is_assistant_message = value.get("type").and_then(|v| v.as_str()) == Some("message")
            && value.get("role").and_then(|v| v.as_str()) == Some("assistant");
        if !is_assistant_message {
            // user echoes, reasoning entries and housekeeping are skipped.
            continue;
        }
        let full: String = value
            .get("content")
            .and_then(|c| c.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.get("text").and_then(|t| t.as_str()))
                    .collect::<Vec<_>>()
                    .join("")
            })
            .unwrap_or_default();
        let id = value.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let sent = emitted.entry(id).or_insert(0);
        if full.len() <= *sent {
            continue;
        }
        // Snapshots only ever grow, but guard against mid-UTF-8 offsets.
        let start = if full.is_char_boundary(*sent) { *sent } else { 0 };
        *sent = full.len();
        let suffix = &full[start..];
        if !suffix.is_empty() {
            sender
                .send(Message::Text(
                    json!({"type":"delta","content":suffix}).to_string().into(),
                ))
                .await?;
        }
    }

    let status = child.wait().await?;
    if let Some(task) = stderr_task {
        let _ = task.await;
    }
    if !status.success() {
        let tail = stderr_tail
            .lock()
            .await
            .join("\n");
        send_error(
            sender,
            &format!("vibe exited with {status}{}", {
                if tail.is_empty() {
                    String::new()
                } else {
                    format!(": {}", truncate(&tail, 400))
                }
            }),
        )
        .await?;
    }
    Ok(())
}

fn find_vibe(_state: &AppState) -> Option<String> {
    // Prefer an explicitly configured binary, else search PATH.
    if let Ok(configured) = std::env::var("COACH_VIBE_BIN") {
        if !configured.is_empty() && std::path::Path::new(&configured).exists() {
            return Some(configured);
        }
    }
    let path = std::env::var("PATH").ok()?;
    std::env::split_paths(&path)
        .map(|dir| dir.join("vibe"))
        .find(|candidate| candidate.is_file())
        .map(|p| p.display().to_string())
}

/// Stream an OpenAI-compatible completion as delta frames.
async fn stream_openai(
    state: &AppState,
    provider: &Provider,
    user_text: &str,
    sender: &mut futures_util::stream::SplitSink<WebSocket, Message>,
) -> anyhow::Result<()> {
    use futures_util::StreamExt;

    let base = provider
        .base_url
        .clone()
        .unwrap_or_else(|| "https://api.mistral.ai/v1".into());
    let url = format!("{}/chat/completions", base.trim_end_matches('/'));
    let model = provider
        .model
        .clone()
        .unwrap_or_else(|| "mistral-small-latest".into());

    let body = json!({
        "model": model,
        "messages": [{"role": "user", "content": user_text}],
        "stream": true,
    });

    let mut request = state.http_client.post(&url).json(&body);
    if let Some(key) = provider.api_key.as_deref() {
        if !key.is_empty() && !key.ends_with('…') {
            request = request.bearer_auth(key);
        }
    }

    let response = match request.send().await {
        Ok(response) => response,
        Err(err) => {
            let frame = json!({"type":"error","message":format!("provider unreachable ({url}): {err}")});
            sender.send(Message::Text(frame.to_string().into())).await?;
            return Ok(());
        }
    };

    if !response.status().is_success() {
        let status = response.status();
        let detail = response.text().await.unwrap_or_default();
        let frame =
            json!({"type":"error","message":format!("provider {status}: {}", truncate(&detail, 300))});
        sender.send(Message::Text(frame.to_string().into())).await?;
        return Ok(());
    }

    let mut stream = response.bytes_stream();
    let mut sse_buffer = String::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        sse_buffer.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(line_end) = sse_buffer.find('\n') {
            let line: String = sse_buffer.drain(..=line_end).collect();
            let line = line.trim();
            let Some(data) = line.strip_prefix("data:") else {
                continue;
            };
            let data = data.trim();
            if data == "[DONE]" {
                return Ok(());
            }
            let Ok(value) = serde_json::from_str::<serde_json::Value>(data) else {
                continue;
            };
            let delta = value["choices"][0]["delta"]["content"].as_str().unwrap_or("");
            if !delta.is_empty() {
                let frame = json!({"type":"delta","content":delta});
                sender.send(Message::Text(frame.to_string().into())).await?;
            }
        }
    }
    Ok(())
}

fn truncate(value: &str, max: usize) -> String {
    if value.len() <= max {
        return value.to_string();
    }
    let mut end = max;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &value[..end])
}
