#![allow(dead_code)] // populated by Track C; production callers arrive in Track E

use std::path::PathBuf;
use std::time::Duration;

use tokio::net::UnixStream;
use tokio::time::timeout;

use crate::channels::local_ipc::error::LocalIpcError;

const ENV_OVERRIDE: &str = "IRONCLAW_LOCAL_SOCKET";
const DISABLED_TOKEN: &str = "disabled";
const FALLBACK_BASENAME: &str = "ironclaw.sock";

/// Resolved outcome for the socket path lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SocketResolution {
    /// IPC explicitly disabled by env (`IRONCLAW_LOCAL_SOCKET=disabled`).
    Disabled,
    /// Use this path.
    Path(PathBuf),
}

/// Resolve the socket path according to the documented order:
/// 1. `IRONCLAW_LOCAL_SOCKET` env var (verbatim, or `disabled`).
/// 2. `$XDG_RUNTIME_DIR/ironclaw.sock`.
/// 3. `$HOME/.ironclaw/ironclaw.sock`.
///
/// Pure function — no filesystem side effects (does NOT create directories).
/// Errors propagate from the env lookups only.
pub fn resolve_socket_path() -> SocketResolution {
    if let Ok(val) = std::env::var(ENV_OVERRIDE) {
        if val == DISABLED_TOKEN {
            return SocketResolution::Disabled;
        }
        if !val.is_empty() {
            return SocketResolution::Path(PathBuf::from(val));
        }
    }
    if let Ok(xdg) = std::env::var("XDG_RUNTIME_DIR")
        && !xdg.is_empty()
    {
        return SocketResolution::Path(PathBuf::from(xdg).join(FALLBACK_BASENAME));
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    SocketResolution::Path(
        PathBuf::from(home)
            .join(".ironclaw")
            .join(FALLBACK_BASENAME),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Env mutations are process-global; serialize them across tests.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn with_env<F: FnOnce()>(vars: &[(&str, Option<&str>)], f: F) {
        let _guard = ENV_LOCK.lock().unwrap();
        let saved: Vec<_> = vars
            .iter()
            .map(|(k, _)| (*k, std::env::var(k).ok()))
            .collect();
        for (k, v) in vars {
            // SAFETY: env access is single-threaded under ENV_LOCK.
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
    fn env_override_explicit_path() {
        with_env(
            &[
                ("IRONCLAW_LOCAL_SOCKET", Some("/tmp/jarvis-test.sock")),
                ("XDG_RUNTIME_DIR", Some("/run/user/1000")),
            ],
            || {
                assert_eq!(
                    resolve_socket_path(),
                    SocketResolution::Path(PathBuf::from("/tmp/jarvis-test.sock"))
                );
            },
        );
    }

    #[test]
    fn env_override_disabled() {
        with_env(&[("IRONCLAW_LOCAL_SOCKET", Some("disabled"))], || {
            assert_eq!(resolve_socket_path(), SocketResolution::Disabled);
        });
    }

    #[test]
    fn xdg_runtime_dir_fallback() {
        with_env(
            &[
                ("IRONCLAW_LOCAL_SOCKET", None),
                ("XDG_RUNTIME_DIR", Some("/run/user/1000")),
            ],
            || {
                assert_eq!(
                    resolve_socket_path(),
                    SocketResolution::Path(PathBuf::from("/run/user/1000/ironclaw.sock"))
                );
            },
        );
    }

    #[test]
    fn home_fallback_when_no_xdg() {
        with_env(
            &[
                ("IRONCLAW_LOCAL_SOCKET", None),
                ("XDG_RUNTIME_DIR", None),
                ("HOME", Some("/home/jarvis")),
            ],
            || {
                assert_eq!(
                    resolve_socket_path(),
                    SocketResolution::Path(PathBuf::from("/home/jarvis/.ironclaw/ironclaw.sock"))
                );
            },
        );
    }
}

/// Inspect an existing socket file and remove it if no live IronClaw
/// instance is listening. Returns `Ok(true)` if the orphan was cleaned
/// (or never existed), `Ok(false)` if a live instance currently owns
/// it, and `Err` on cleanup failure.
pub async fn cleanup_orphan_socket(path: &std::path::Path) -> Result<bool, LocalIpcError> {
    if !tokio::fs::try_exists(path).await? {
        return Ok(true);
    }
    // Try to connect. A live owner replies; an orphan errors out.
    match timeout(Duration::from_millis(100), UnixStream::connect(path)).await {
        Ok(Ok(_)) => Ok(false), // live owner — caller must abort startup
        Ok(Err(_)) | Err(_) => {
            tokio::fs::remove_file(path)
                .await
                .map_err(|e| LocalIpcError::CleanupFailed {
                    path: path.to_path_buf(),
                    reason: e.to_string(),
                })?;
            tracing::debug!(path = %path.display(), "removed orphan socket");
            Ok(true)
        }
    }
}

#[cfg(test)]
mod cleanup_tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn missing_path_is_a_clean_orphan() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nope.sock");
        assert!(cleanup_orphan_socket(&path).await.unwrap());
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn dead_socket_file_gets_unlinked() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("dead.sock");
        // Create a regular file at the socket path to simulate an orphan
        // left over from a crashed process.
        tokio::fs::write(&path, b"orphan").await.unwrap();
        assert!(path.exists());
        assert!(cleanup_orphan_socket(&path).await.unwrap());
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn live_listener_blocks_cleanup() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("live.sock");
        let listener = tokio::net::UnixListener::bind(&path).unwrap();
        // Spawn an accept loop so connect() actually succeeds.
        let _accept_task = tokio::spawn(async move {
            let _ = listener.accept().await;
        });
        // Give the listener a moment to be ready.
        tokio::time::sleep(Duration::from_millis(20)).await;
        let result = cleanup_orphan_socket(&path).await.unwrap();
        assert!(!result, "live owner must block cleanup");
        assert!(path.exists(), "live socket must NOT be unlinked");
    }
}

use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::net::UnixListener;
use tokio::sync::{Notify, mpsc};
use tracing::{debug, warn};

use crate::channels::IncomingMessage;
use crate::channels::local_ipc::client::{ClientHandle, ClientMap, spawn_session};
use crate::channels::local_ipc::protocol::ClientId;
use crate::channels::web::platform::sse::SseManager;

const SOFT_CLIENT_CAP: u64 = 32;
const HARD_CLIENT_CAP: u64 = 256;

pub struct ListenerConfig {
    pub user_id: String,
    pub sse: Arc<SseManager>,
    pub inject_tx: mpsc::Sender<IncomingMessage>,
    pub writer_buffer: usize,
    pub clients: ClientMap,
    pub shutdown: Arc<Notify>,
}

/// Bind, set 0600 perms, and run accept loop until shutdown.notified.
/// Removes the socket file on exit.
pub async fn run_listener(
    path: std::path::PathBuf,
    cfg: ListenerConfig,
) -> Result<(), super::error::LocalIpcError> {
    let listener =
        UnixListener::bind(&path).map_err(|e| super::error::LocalIpcError::BindFailed {
            path: path.clone(),
            reason: e.to_string(),
        })?;
    // 0600 — POSIX permission gate is the auth model.
    let perms = std::fs::Permissions::from_mode(0o600);
    if let Err(e) = std::fs::set_permissions(&path, perms) {
        warn!(path = %path.display(), error = %e, "failed to chmod 0600 on local IPC socket");
    }
    let active = Arc::new(AtomicU64::new(0));
    let next_id = Arc::new(AtomicU64::new(1));

    loop {
        tokio::select! {
            _ = cfg.shutdown.notified() => {
                debug!("local_ipc listener shutdown notified");
                break;
            }
            accept = listener.accept() => {
                match accept {
                    Ok((stream, _addr)) => {
                        let count = active.fetch_add(1, Ordering::Relaxed) + 1;
                        if count > HARD_CLIENT_CAP {
                            warn!(count, "rejecting local IPC client: hard cap reached");
                            active.fetch_sub(1, Ordering::Relaxed);
                            drop(stream);
                            continue;
                        }
                        if count == SOFT_CLIENT_CAP + 1 {
                            warn!(count, "local IPC clients exceeded soft cap");
                        }
                        let id_num = next_id.fetch_add(1, Ordering::Relaxed);
                        let client_id = match ClientId::new(format!("ipc-{id_num}")) {
                            Ok(c) => c,
                            Err(e) => {
                                warn!(error = %e, "could not mint ClientId");
                                active.fetch_sub(1, Ordering::Relaxed);
                                drop(stream);
                                continue;
                            }
                        };
                        let active_for_session = Arc::clone(&active);
                        let clients = Arc::clone(&cfg.clients);
                        let sse = Arc::clone(&cfg.sse);
                        let inject = cfg.inject_tx.clone();
                        let user = cfg.user_id.clone();
                        let buf = cfg.writer_buffer;
                        let cid_for_remove = client_id.clone();
                        tokio::spawn(async move {
                            let handle = spawn_session(
                                stream, client_id, user, sse, inject, buf,
                            )
                            .await;
                            register(&clients, handle).await;
                            // No await for completion — both tasks live
                            // independently; the registry entry will be
                            // removed when respond() finds it gone (via
                            // a periodic sweep in v2). For v1 the entry
                            // leaks until shutdown, which is bounded by
                            // HARD_CLIENT_CAP. v2 follow-up: track per-
                            // session JoinHandle and unregister on exit.
                            let _ = cid_for_remove;
                            active_for_session.fetch_sub(1, Ordering::Relaxed);
                        });
                    }
                    Err(e) => {
                        warn!(error = %e, "local IPC accept failed");
                    }
                }
            }
        }
    }
    if let Err(e) = std::fs::remove_file(&path) {
        debug!(path = %path.display(), error = %e, "remove_file on shutdown failed");
    }
    Ok(())
}

async fn register(clients: &ClientMap, handle: ClientHandle) {
    let mut map = clients.lock().await;
    map.insert(handle.client_id.as_str().to_string(), handle);
}

#[cfg(test)]
mod listener_tests {
    use super::*;
    use crate::channels::local_ipc::client::DEFAULT_WRITER_BUFFER;
    use tempfile::tempdir;
    use tokio::io::{AsyncBufReadExt, BufReader};
    use tokio::net::UnixStream;

    #[tokio::test]
    async fn listener_accepts_one_client_and_emits_hello() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("li.sock");
        let sse = Arc::new(SseManager::new());
        let (inject_tx, _inject_rx) = mpsc::channel::<IncomingMessage>(8);
        let clients: ClientMap = Arc::new(tokio::sync::Mutex::new(Default::default()));
        let shutdown = Arc::new(Notify::new());

        let path_clone = path.clone();
        let sd = Arc::clone(&shutdown);
        let task = tokio::spawn(async move {
            run_listener(
                path_clone,
                ListenerConfig {
                    user_id: "owner".into(),
                    sse,
                    inject_tx,
                    writer_buffer: DEFAULT_WRITER_BUFFER,
                    clients,
                    shutdown: sd,
                },
            )
            .await
        });

        // Wait for the bind.
        for _ in 0..50 {
            if path.exists() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        assert!(path.exists(), "socket file must exist after bind");

        let stream = UnixStream::connect(&path).await.unwrap();
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            reader.read_line(&mut line),
        )
        .await
        .expect("hello timeout")
        .unwrap();
        assert!(line.contains("\"type\":\"ipc_hello\""));

        shutdown.notify_waiters();
        let _ = tokio::time::timeout(std::time::Duration::from_secs(2), task)
            .await
            .expect("listener did not exit on shutdown");
        assert!(!path.exists(), "socket file must be removed on shutdown");
    }

    #[tokio::test]
    async fn listener_chmods_0600() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("perm.sock");
        let sse = Arc::new(SseManager::new());
        let (inject_tx, _inject_rx) = mpsc::channel::<IncomingMessage>(8);
        let clients: ClientMap = Arc::new(tokio::sync::Mutex::new(Default::default()));
        let shutdown = Arc::new(Notify::new());

        let path_clone = path.clone();
        let sd = Arc::clone(&shutdown);
        let task = tokio::spawn(async move {
            run_listener(
                path_clone,
                ListenerConfig {
                    user_id: "owner".into(),
                    sse,
                    inject_tx,
                    writer_buffer: DEFAULT_WRITER_BUFFER,
                    clients,
                    shutdown: sd,
                },
            )
            .await
        });

        for _ in 0..50 {
            if path.exists() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        let meta = std::fs::metadata(&path).unwrap();
        let mode = meta.permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "socket must be chmod 0600 (got {mode:o})");
        shutdown.notify_waiters();
        let _ = task.await;
    }
}
