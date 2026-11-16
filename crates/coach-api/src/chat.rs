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
//! - `vibe`: spawns the Mistral Vibe CLI in a PTY scoped to the workspace
//!   dir and relays raw output; input is written as lines. Never host-wide:
//!   cwd is the workspace, no elevated context.

use crate::providers::{Provider, ProviderKind};
use crate::state::AppState;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::Response;
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::pty;

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

    // Vibe relay: the session lives in its own task that forwards PTY output
    // as delta frames; the loop writes user input through the Arc handle.
    let (delta_tx, mut delta_rx) = mpsc::unbounded_channel::<String>();
    let mut vibe: Option<Arc<pty::PtySession>> = None;

    loop {
        tokio::select! {
            maybe_msg = receiver.next() => {
                let Some(Ok(msg)) = maybe_msg else { break };
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
                    sender
                        .send(Message::Text(
                            json!({"type":"error","message":"provider_id missing"}).to_string().into(),
                        ))
                        .await?;
                    continue;
                };
                let Some(provider) = state.provider_store.get(provider_id) else {
                    sender
                        .send(Message::Text(
                            json!({"type":"error","message":format!("unknown provider {provider_id}")})
                                .to_string()
                                .into(),
                        ))
                        .await?;
                    continue;
                };

                match &provider.kind {
                    ProviderKind::OpenAiCompatible => {
                        stream_openai(&state, &provider, &user_text, &mut sender).await?;
                        sender
                            .send(Message::Text(json!({"type":"done"}).to_string().into()))
                            .await?;
                    }
                    ProviderKind::Vibe => {
                        if vibe.is_none() {
                            match start_vibe_pty(&state).and_then(|session| {
                                let session = Arc::new(session);
                                spawn_vibe_relay(session.clone(), delta_tx.clone());
                                Ok(session)
                            }) {
                                Ok(session) => {
                                    sender
                                        .send(Message::Text(
                                            json!({"type":"system","message":"vibe session started (workspace-scoped PTY)"})
                                                .to_string().into(),
                                        ))
                                        .await?;
                                    vibe = Some(session);
                                }
                                Err(err) => {
                                    sender
                                        .send(Message::Text(
                                            json!({"type":"error","message":format!("vibe spawn failed: {err:#}")})
                                                .to_string()
                                                .into(),
                                        ))
                                        .await?;
                                    continue;
                                }
                            }
                        }
                        if let Some(session) = vibe.as_ref() {
                            session.write_line(&user_text);
                        }
                    }
                }
            }
            maybe_delta = delta_rx.recv() => {
                match maybe_delta {
                    Some(content) => {
                        let frame = json!({"type":"delta","content":content});
                        sender.send(Message::Text(frame.to_string().into())).await?;
                    }
                    None => {
                        // The vibe PTY ended (child exited). Report why and
                        // keep the socket alive so the user can retry or
                        // switch providers — a dead provider must not tear
                        // down the chat connection.
                        let exit_info = vibe
                            .as_ref()
                            .and_then(|session| session.exit_code());
                        let detail = match exit_info {
                            Some(Ok(code)) => format!("vibe exited with code {code}"),
                            Some(Err(err)) => format!("vibe exited: {err}"),
                            None => "vibe session ended".to_string(),
                        };
                        if let Some(session) = vibe.take() {
                            session.kill();
                        }
                        let frame = json!({"type":"system","message":detail});
                        sender.send(Message::Text(frame.to_string().into())).await?;
                    }
                }
            }
        }
    }

    drop(vibe); // relay task ends when the last Arc (and the channel) drops
    Ok(())
}

/// Spawn a task that pumps PTY output into the delta channel until the PTY
/// closes or all senders drop (i.e. the socket dies).
fn spawn_vibe_relay(session: Arc<pty::PtySession>, delta_tx: mpsc::UnboundedSender<String>) {
    tokio::spawn(async move {
        while let Some(chunk) = session.recv_output().await {
            if delta_tx.send(String::from_utf8_lossy(&chunk).into_owned()).is_err() {
                break;
            }
        }
        session.kill();
    });
}

/// Spawn the Mistral Vibe CLI in a PTY scoped to the workspace directory.
fn start_vibe_pty(state: &AppState) -> anyhow::Result<pty::PtySession> {
    let which_vibe = find_vibe(state).ok_or_else(|| {
        anyhow::anyhow!(
            "'vibe' binary not found — install Mistral Vibe, set COACH_VIBE_BIN, \
             or use an OpenAI-compatible provider"
        )
    })?;
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into());
    let term = std::env::var("TERM").unwrap_or_else(|_| "xterm-256color".into());
    pty::spawn_pty(
        &shell,
        &["-ic".into(), format!("exec {which_vibe}")],
        Some(state.workspace_dir.clone()),
        vec![
            (
                "COACH_WORKSPACE".into(),
                state.workspace_dir.display().to_string(),
            ),
            // The vibe CLI renders a TUI and dies immediately without TERM
            // (common when the server is started non-interactively).
            ("TERM".into(), term),
        ],
        100,
        30,
    )
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
