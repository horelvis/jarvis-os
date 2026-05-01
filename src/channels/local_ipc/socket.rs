use std::path::PathBuf;

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

use std::time::Duration;
use tokio::net::UnixStream;
use tokio::time::timeout;

use crate::channels::local_ipc::error::LocalIpcError;

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
