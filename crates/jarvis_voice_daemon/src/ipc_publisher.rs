//! Voice daemon → IronClaw IPC publisher.
//!
//! Owns a long-lived `UnixStream` to IronClaw's local IPC socket and
//! exposes a non-blocking [`IpcPublisher::publish_pcm`] API. The
//! orchestrator calls it just before `speaker_tx.play(pcm)`, so the
//! `TtsPcmFrame` JSON line reaches IronClaw at almost the same moment
//! the user starts hearing the audio.
//!
//! Robustness rules:
//! - Lazy connect: if IronClaw isn't running we don't fail the daemon,
//!   just drop frames. We retry on the next `publish_pcm` call.
//! - Drain the read half: IronClaw's server writes `ipc_hello` plus a
//!   continuous AppEvent stream. We don't consume it semantically, but
//!   we read-and-discard so the server's writer mpsc doesn't fill up
//!   and stall.
//! - try_send into the inner mpsc: never block the audio path. If the
//!   publisher is stuck reconnecting, the orb just stops moving for a
//!   few frames — preferable to stalling cpal playback.

use base64::Engine as _;
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::mpsc;
use tracing::{debug, warn};

const CHANNEL_CAPACITY: usize = 128;

#[derive(Debug, Clone)]
struct TtsCommand {
    samples_b64: String,
    sample_rate: u32,
}

#[derive(Clone)]
pub struct IpcPublisher {
    tx: mpsc::Sender<TtsCommand>,
}

impl IpcPublisher {
    /// Spawn the publisher background task. Returns handles even when
    /// IronClaw isn't reachable yet — connection is lazy.
    pub fn spawn(socket_path: PathBuf) -> Self {
        let (tx, rx) = mpsc::channel::<TtsCommand>(CHANNEL_CAPACITY);
        tokio::spawn(run_publisher(socket_path, rx));
        Self { tx }
    }

    /// Publish a PCM chunk. Non-blocking — if the inner channel is
    /// full (publisher stuck on reconnect, IronClaw down, …) the
    /// frame is dropped silently. The orb stops moving for a few
    /// frames; the audio playback path is never stalled.
    pub fn publish_pcm(&self, samples: &[i16], sample_rate: u32) {
        let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let samples_b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
        let cmd = TtsCommand {
            samples_b64,
            sample_rate,
        };
        if self.tx.try_send(cmd).is_err() {
            // silent-ok: orb is decorative; dropping a frame is preferable to
            //            stalling speaker_tx.play() via mpsc backpressure
        }
    }
}

/// Resolve the IronClaw IPC socket path the same way IronClaw itself does.
///
/// Order: `IRONCLAW_LOCAL_SOCKET` (literal path or `disabled`),
/// `$XDG_RUNTIME_DIR/ironclaw.sock`, `$HOME/.ironclaw/ironclaw.sock`.
/// Returns `None` if explicitly disabled.
pub fn resolve_socket_path() -> Option<PathBuf> {
    const ENV_OVERRIDE: &str = "IRONCLAW_LOCAL_SOCKET";
    const FALLBACK_BASENAME: &str = "ironclaw.sock";

    if let Ok(val) = std::env::var(ENV_OVERRIDE) {
        if val == "disabled" {
            return None;
        }
        if !val.is_empty() {
            return Some(PathBuf::from(val));
        }
    }
    if let Ok(xdg) = std::env::var("XDG_RUNTIME_DIR")
        && !xdg.is_empty()
    {
        return Some(PathBuf::from(xdg).join(FALLBACK_BASENAME));
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    Some(PathBuf::from(home).join(".ironclaw").join(FALLBACK_BASENAME))
}

async fn run_publisher(socket_path: PathBuf, mut rx: mpsc::Receiver<TtsCommand>) {
    let mut writer: Option<tokio::net::unix::OwnedWriteHalf> = None;
    let mut drain_handle: Option<tokio::task::JoinHandle<()>> = None;

    while let Some(cmd) = rx.recv().await {
        if writer.is_none() {
            match UnixStream::connect(&socket_path).await {
                Ok(stream) => {
                    let (read_half, write_half) = stream.into_split();
                    drain_handle = Some(tokio::spawn(drain_reader(read_half)));
                    writer = Some(write_half);
                    debug!(path = %socket_path.display(), "ipc_publisher.connected");
                }
                Err(e) => {
                    // Don't spam at warn — voice daemon may legitimately
                    // start before IronClaw is ready.
                    debug!(error = %e, "ipc_publisher.connect_failed");
                    continue;
                }
            }
        }

        let payload = serde_json::json!({
            "type": "tts_pcm_frame",
            "samples_b64": cmd.samples_b64,
            "sample_rate": cmd.sample_rate,
        });
        let mut bytes = match serde_json::to_vec(&payload) {
            Ok(b) => b,
            Err(_) => continue,
        };
        bytes.push(b'\n');

        if let Some(w) = writer.as_mut() {
            if let Err(e) = w.write_all(&bytes).await {
                warn!(error = %e, "ipc_publisher.write_failed; will reconnect");
                writer = None;
                if let Some(h) = drain_handle.take() {
                    h.abort();
                }
            }
        }
    }
}

async fn drain_reader(read_half: tokio::net::unix::OwnedReadHalf) {
    let mut reader = BufReader::new(read_half);
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) | Err(_) => break,
            Ok(_) => continue, // discard ipc_hello + AppEvent stream
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[allow(clippy::await_holding_lock)]
    fn with_env<F: FnOnce()>(vars: &[(&str, Option<&str>)], f: F) {
        let _guard = ENV_LOCK.lock().unwrap();
        let saved: Vec<_> = vars
            .iter()
            .map(|(k, _)| (*k, std::env::var(k).ok()))
            .collect();
        for (k, v) in vars {
            unsafe {
                match v {
                    Some(value) => std::env::set_var(k, value),
                    None => std::env::remove_var(k),
                }
            }
        }
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
        for (k, v) in saved {
            unsafe {
                match v {
                    Some(value) => std::env::set_var(k, value),
                    None => std::env::remove_var(k),
                }
            }
        }
        if let Err(e) = result {
            std::panic::resume_unwind(e);
        }
    }

    #[test]
    fn resolve_socket_path_disabled_returns_none() {
        with_env(&[("IRONCLAW_LOCAL_SOCKET", Some("disabled"))], || {
            assert!(resolve_socket_path().is_none());
        });
    }

    #[test]
    fn resolve_socket_path_uses_xdg_runtime_dir() {
        with_env(
            &[
                ("IRONCLAW_LOCAL_SOCKET", None),
                ("XDG_RUNTIME_DIR", Some("/run/user/1000")),
            ],
            || {
                let p = resolve_socket_path().unwrap();
                assert_eq!(p, PathBuf::from("/run/user/1000/ironclaw.sock"));
            },
        );
    }

    #[test]
    fn resolve_socket_path_explicit_override() {
        with_env(
            &[("IRONCLAW_LOCAL_SOCKET", Some("/tmp/custom.sock"))],
            || {
                assert_eq!(
                    resolve_socket_path().unwrap(),
                    PathBuf::from("/tmp/custom.sock")
                );
            },
        );
    }
}
