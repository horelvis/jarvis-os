//! Error types for adapter and tool layers.
//!
//! Migrated from the legacy `jarvis_linux_mcp` crate. Same variants —
//! the adapters and tools that depend on this enum import paths
//! unchanged (`use crate::error::{Error, Result}`).

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    /// zbus runtime errors (D-Bus timeouts, missing methods, permission
    /// denied at the D-Bus layer, etc.).
    #[error("D-Bus error: {0}")]
    Dbus(#[from] zbus::Error),

    /// Tool declared but not implemented yet (used during scaffolding).
    #[error("Tool not implemented: {tool}")]
    NotImplemented { tool: String },

    /// Args malformed (missing fields, wrong types, out-of-range values).
    #[error("Invalid arguments: {0}")]
    InvalidArguments(String),

    /// `jarvis_policies::Decision::Deny` surfaced as an error.
    #[error("Policy denied: {reason}")]
    PolicyDenied { reason: String },

    /// Generic I/O (files, sockets, etc.).
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON serde error (parsing args or serializing output).
    #[error("Serde JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// Anything that doesn't fit the variants above; use sparingly.
    #[error("Internal error: {0}")]
    Internal(String),
}

pub type Result<T> = std::result::Result<T, Error>;
