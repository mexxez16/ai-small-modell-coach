//! WebSocket endpoints.
//!
//! - `/ws/metrics` — live training metric stream (broadcast of ingested
//!   points as JSON lines)
//! - `/ws/jobs`    — conversion job progress (echo stub)
//! Real terminal: `src/terminal.rs`, real chat: `src/chat.rs`.

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::Response;
use futures_util::{SinkExt, StreamExt};

use crate::state::AppState;

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

/// Live metric stream: every ingested MetricPoint is broadcast as one JSON
/// text frame. Late joiners receive a `hello` marker; lagged receivers get
/// notified instead of being dropped silently.
pub async fn metrics_ws(
    State(state): State<AppState>,
    ws: WebSocketUpgrade,
) -> Response {
    ws.on_upgrade(move |socket| async move {
        let mut rx = state.metric_bus.subscribe();
        let (mut tx, mut rxin) = socket.split();
        let _ = tx
            .send(Message::Text(
                serde_json::json!({"type":"hello","bus":"metrics"}).to_string().into(),
            ))
            .await;
        // Send loop only; metric ingest is the producer via append_metrics.
        loop {
            tokio::select! {
                frame = rxin.next() => {
                    let Some(Ok(frame)) = frame else { break };
                    if matches!(frame, Message::Close(_)) { break; }
                }
                point = rx.recv() => {
                    match point {
                        Ok(metrics) => {
                            if tx.send(Message::Text(
                                serde_json::to_string(&metrics).unwrap_or_default().into(),
                            )).await.is_err() {
                                break;
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            let _ = tx.send(Message::Text(
                                serde_json::json!({"type":"lagged","skipped":n}).to_string().into(),
                            )).await;
                        }
                        Err(_) => break,
                    }
                }
            }
        }
    })
}

pub async fn jobs_ws(ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(ws_echo)
}
