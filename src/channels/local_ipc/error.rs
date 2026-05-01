use std::path::PathBuf;

use thiserror::Error;

use crate::error::ChannelError;

#[derive(Debug, Error)]
pub enum LocalIpcError {
    #[error("socket bind failed at {path}: {reason}")]
    BindFailed { path: PathBuf, reason: String },

    #[error("another IronClaw instance owns the socket at {path}")]
    SocketBusy { path: PathBuf },

    #[error("socket file at {path} could not be cleaned up: {reason}")]
    CleanupFailed { path: PathBuf, reason: String },

    #[error("unable to resolve local user id: {reason}")]
    LocalUserResolve { reason: String },

    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

impl From<LocalIpcError> for ChannelError {
    fn from(e: LocalIpcError) -> Self {
        // ChannelError uses struct variants ({ name, reason }), not tuple
        // variants. There is no Io(io::Error) variant on ChannelError —
        // io errors collapse into StartupFailed at the channel boundary,
        // sanitized to a string. Reference: src/error.rs:115.
        ChannelError::StartupFailed {
            name: "local_ipc".into(),
            reason: e.to_string(),
        }
    }
}
