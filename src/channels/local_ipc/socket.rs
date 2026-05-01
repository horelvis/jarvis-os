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
