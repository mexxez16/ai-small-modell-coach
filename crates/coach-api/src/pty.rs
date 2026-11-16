//! Shared PTY plumbing for the terminal dock (`/ws/terminal`) and the Vibe
//! provider (`/ws/chat`): allocate a pseudo-terminal, spawn a command in it,
//! and pump output to a channel.

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use std::io::Read;
use std::io::Write;
use std::sync::{Arc, Mutex};

pub struct PtySession {
    // portable-pty exposes the master as Box<dyn MasterPty + Send> only; the
    // mutex wrapper makes the session Sync so it can live inside tokio tasks.
    pub master: Mutex<Box<dyn MasterPty + Send>>,
    pub writer: Mutex<Box<dyn Write + Send>>,
    pub child: Mutex<Box<dyn Child + Send + Sync>>,
    /// Pending output pushed by a background thread; consumed via
    /// [`PtySession::recv_output`].
    output_rx: Arc<tokio::sync::Mutex<tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>>>,
}

/// Spawn `program` inside a PTY. Output is pumped into an unbounded channel
/// by a dedicated reader thread (blocking reads are fine there).
pub fn spawn_pty(
    program: &str,
    args: &[String],
    cwd: Option<std::path::PathBuf>,
    env: Vec<(String, String)>,
    cols: u16,
    rows: u16,
) -> anyhow::Result<PtySession> {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;

    let mut cmd = CommandBuilder::new(program);
    for arg in args {
        cmd.arg(arg);
    }
    if let Some(dir) = cwd {
        cmd.cwd(dir);
    }
    for (key, value) in env {
        cmd.env(key, value);
    }
    let child = pair.slave.spawn_command(cmd)?;
    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| anyhow::anyhow!("pty reader clone failed: {e}"))?;
    let writer = pair
        .master
        .take_writer()
        .map_err(|e| anyhow::anyhow!("pty writer take failed: {e}"))?;

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if tx.send(buf[..n].to_vec()).is_err() {
                        break; // socket closed
                    }
                }
                Err(_) => break,
            }
        }
    });

    // Keep the slave handle alive: dropping it closes the PTY on some
    // platforms while the child is still using it.
    std::mem::forget(pair.slave);

    Ok(PtySession {
        master: Mutex::new(pair.master),
        writer: Mutex::new(writer),
        child: Mutex::new(child),
        output_rx: Arc::new(tokio::sync::Mutex::new(rx)),
    })
}

impl PtySession {
    pub async fn recv_output(&self) -> Option<Vec<u8>> {
        self.output_rx.lock().await.recv().await
    }

    pub fn resize(&self, cols: u16, rows: u16) {
        if let Ok(master) = self.master.lock() {
            let _ = master.resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            });
        }
    }

    pub fn write_line(&self, line: &str) {
        if let Ok(mut writer) = self.writer.lock() {
            let _ = writer.write_all(line.as_bytes());
            let _ = writer.write_all(b"\r");
            let _ = writer.flush();
        }
    }

    pub fn kill(&self) {
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
        }
    }
}
