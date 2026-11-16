//! Terminal dock backend: one real PTY per WebSocket connection, scoped to
//! the workspace directory.
//!
//! Wire protocol: input comes as raw text frames (line mode: text + newline)
//! or binary frames (raw keystrokes for xterm.js); `{"type":"resize","cols":…,"rows":…}`
//! text frames resize the PTY; output returns as binary frames with the raw
//! VT sequences, which xterm.js consumes directly.

use crate::pty;
use crate::state::AppState;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::response::Response;
use futures_util::{SinkExt, StreamExt};
use std::collections::HashMap;

pub async fn terminal_ws(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
    ws: WebSocketUpgrade,
) -> Response {
    let cols: u16 = params.get("cols").and_then(|c| c.parse().ok()).unwrap_or(80);
    let rows: u16 = params.get("rows").and_then(|r| r.parse().ok()).unwrap_or(24);
    ws.on_upgrade(move |socket| async move {
        if let Err(err) = run_terminal(state, socket, cols, rows).await {
            tracing::warn!("terminal socket ended: {err:#}");
        }
    })
}

async fn run_terminal(
    state: AppState,
    socket: WebSocket,
    cols: u16,
    rows: u16,
) -> anyhow::Result<()> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into());
    let workspace = state.workspace_dir.clone();
    let session = pty::spawn_pty(
        &shell,
        &["-i".into()],
        Some(workspace),
        vec![("TERM".into(), "xterm-256color".into())],
        cols,
        rows,
    )
    .map_err(|err| anyhow::anyhow!("PTY allocation failed: {err:#}"))?;

    let (mut sender, mut receiver) = socket.split();

    loop {
        tokio::select! {
            maybe_out = session.recv_output() => {
                match maybe_out {
                    Some(chunk) => {
                        // Raw VT sequences; xterm.js renders them directly.
                        sender.send(Message::Binary(chunk.into())).await?;
                    }
                    None => {
                        // Shell exited.
                        let _ = sender.send(Message::Text("{\"type\":\"exit\"}".into())).await;
                        break;
                    }
                }
            }
            maybe_msg = receiver.next() => {
                match maybe_msg {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
                            if value.get("type").and_then(|v| v.as_str()) == Some("resize") {
                                let c = value.get("cols").and_then(|v| v.as_u64()).unwrap_or(80) as u16;
                                let r = value.get("rows").and_then(|v| v.as_u64()).unwrap_or(24) as u16;
                                session.resize(c, r);
                                continue;
                            }
                        }
                        // Line mode input.
                        session.write_line(&text);
                    }
                    Some(Ok(Message::Binary(data))) => {
                        // Raw keystrokes (xterm wire mode).
                        if let Ok(mut writer) = session.writer.lock() {
                            use std::io::Write;
                            let _ = writer.write_all(&data);
                            let _ = writer.flush();
                        }
                    }
                    Some(Ok(Message::Ping(payload))) => {
                        sender.send(Message::Pong(payload)).await?;
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
        }
    }

    session.kill();
    Ok(())
}
