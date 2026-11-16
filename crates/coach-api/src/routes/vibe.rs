//! Mistral Vibe bridge.
//!
//! The chat window in the web app drives a Mistral Vibe CLI agent running as
//! a PTY child process of coach-api, scoped to the project/workspace
//! directory. Messages, tool calls and approval requests stream over
//! `/ws/chat`. The scaffold exposes the capability surface only; spawning the
//! actual PTY lands with the chat feature.

use axum::Json;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct VibeCapabilities {
    pub available: bool,
    /// Binary checked on the PATH at startup.
    pub binary: String,
    pub note: String,
}

fn which_vibe() -> bool {
    // Miniature `which`: check each PATH entry for the binary.
    let path = match std::env::var("PATH") {
        Ok(p) => p,
        Err(_) => return false,
    };
    std::env::split_paths(&path)
        .map(|dir| dir.join("vibe"))
        .any(|candidate| candidate.is_file())
}

pub async fn capabilities() -> Json<VibeCapabilities> {
    Json(VibeCapabilities {
        available: which_vibe(),
        binary: "vibe".into(),
        note: "PTY spawn + WS streaming lands with the chat feature".into(),
    })
}
