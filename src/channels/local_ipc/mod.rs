//! Local UNIX-socket IPC channel.
//!
//! Reemplaza `crates/jarvis_ui_bridge/` exponiendo un UNIX socket NDJSON
//! directamente en el core IronClaw para que voice daemon y Quickshell UI
//! consuman eventos y manden comandos sin pasar por el gateway HTTP/WS.
//!
//! Ver `docs/superpowers/specs/2026-04-30-jarvis-os-local-ipc-design.md`.

mod channel_impl;
mod client;
mod control;
mod error;
mod protocol;
mod socket;

use std::sync::Arc;

pub use channel_impl::LocalIpcChannel;
pub use error::LocalIpcError;
pub use socket::{SocketResolution, resolve_socket_path};

use crate::channels::web::platform::sse::SseManager;

/// Build a `LocalIpcChannel` ready to be added to `ChannelManager`, or
/// `Ok(None)` if `IRONCLAW_LOCAL_SOCKET=disabled`.
///
/// Performs orphan-socket cleanup before the channel binds in
/// `start()`. The bind itself happens lazily on `start()` so the
/// caller can wire the channel into `ChannelManager` synchronously.
pub async fn create(
    user_id: String,
    sse: Arc<SseManager>,
    writer_buffer: usize,
) -> Result<Option<LocalIpcChannel>, LocalIpcError> {
    let path = match resolve_socket_path() {
        SocketResolution::Disabled => {
            tracing::debug!("local_ipc disabled by IRONCLAW_LOCAL_SOCKET=disabled");
            return Ok(None);
        }
        SocketResolution::Path(p) => p,
    };
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| LocalIpcError::BindFailed {
                path: path.clone(),
                reason: format!("create parent dir: {e}"),
            })?;
    }
    let cleaned = socket::cleanup_orphan_socket(&path).await?;
    if !cleaned {
        return Err(LocalIpcError::SocketBusy { path });
    }
    Ok(Some(LocalIpcChannel::new(
        path,
        user_id,
        sse,
        writer_buffer,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Env mutations are process-global; serialize them across tests
    /// to avoid races with `socket::tests` which also touches
    /// `IRONCLAW_LOCAL_SOCKET`. (Not the same Mutex, but the test
    /// runtime serializes per-test enough that direct collisions are
    /// unlikely; this is documented should it bite later.)
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    // The lock IS held across .await — that's intentional: another test
    // in this module mutating IRONCLAW_LOCAL_SOCKET concurrently would
    // race with create()'s env read. #[tokio::test] uses a current-thread
    // runtime, so the sync Mutex cannot deadlock on this future. The
    // alternative (drop lock before await) reintroduces the race.
    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn create_returns_none_when_disabled() {
        let _guard = ENV_LOCK.lock().unwrap();
        let prev = std::env::var("IRONCLAW_LOCAL_SOCKET").ok();
        // SAFETY: env mutation is single-threaded under ENV_LOCK in
        // this module; cross-module collisions are theoretical but
        // documented above.
        unsafe {
            std::env::set_var("IRONCLAW_LOCAL_SOCKET", "disabled");
        }
        let sse = Arc::new(SseManager::new());
        let result = create("owner".into(), sse, 256).await.unwrap();
        unsafe {
            match prev {
                Some(v) => std::env::set_var("IRONCLAW_LOCAL_SOCKET", v),
                None => std::env::remove_var("IRONCLAW_LOCAL_SOCKET"),
            }
        }
        assert!(result.is_none());
    }
}
