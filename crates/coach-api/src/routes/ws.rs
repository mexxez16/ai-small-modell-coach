//! WebSocket endpoints for streams that are not yet feature-complete.
//!
//! - `/ws/metrics` — live training metric stream (echo stub)
//! - `/ws/jobs`    — conversion job progress (echo stub)
//! Real terminal: `src/terminal.rs`, real chat: `src/chat.rs`.

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::Response;
use futures_util::{SinkExt, StreamExt};

/// Echo implementation used for the stub streams. Text frames starting with
/// `meta:` get a structured acknowledgment; others are echoed.
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

pub async fn jobs_ws(ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(ws_echo)
}
