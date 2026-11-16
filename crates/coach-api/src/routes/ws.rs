//! WebSocket endpoints.
//!
//! Scaffold scope: echo dispatch. Real streams land per feature:
//! - `/ws/metrics`  — live training metric stream
//! - `/ws/terminal` — PTY attach (echo only in the scaffold)
//! - `/ws/chat`     — Mistral Vibe agent bridge (stub in the scaffold)
//! - `/ws/jobs`     — conversion job progress

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::Response;
use futures_util::{SinkExt, StreamExt};

use crate::state::AppState;

/// Shared echo implementation used by /ws/terminal (and re-used by the other
/// sockets until their streams land). Text frames are echoed back; text frames
/// starting with `meta:` get a small structured acknowledgment so the frontend
/// can smoke-test protocol behavior without a real PTY.
async fn ws_echo(socket: WebSocket) {
    let (mut tx, mut rx) = socket.split();
    while let Some(Ok(msg)) = rx.next().await {
        match msg {
            Message::Text(text) => {
                if text.starts_with("meta:") {
                    let reply = format!(
                        "{{\"echo\": {text:?}, \"server_ts\": {:?}}}",
                        chrono::Utc::now().to_rfc3339()
                    );
                    let _ = tx.send(Message::Text(reply.into())).await;
                } else {
                    let _ = tx.send(Message::Text(format!("echo: {text}").into())).await;
                }
            }
            Message::Binary(bin) => {
                let _ = tx.send(Message::Binary(bin)).await;
            }
            Message::Ping(payload) => {
                let _ = tx.send(Message::Pong(payload)).await;
            }
            Message::Close(_) => break,
            Message::Pong(_) => {}
        }
    }
}

pub async fn metrics_ws(ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(ws_echo)
}

pub async fn terminal_ws(State(state): State<AppState>, ws: WebSocketUpgrade) -> Response {
    // PTY allocation happens under the state lock while the scaffold only
    // echoes; the terminal feature replaces this with portable-pty sessions.
    {
        let _guard = state.pty_lock.lock().unwrap();
    }
    ws.on_upgrade(ws_echo)
}

pub async fn chat_ws(
    State(_state): State<AppState>,
    ws: WebSocketUpgrade,
) -> Response {
    ws.on_upgrade(ws_echo)
}

pub async fn jobs_ws(ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(ws_echo)
}
