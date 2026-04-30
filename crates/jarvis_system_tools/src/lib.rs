//! # jarvis_system_tools
//!
//! IronClaw built-in tools for Linux system introspection: processes,
//! journald, systemd units, networkd, polkit, btrfs.
//!
//! Each tool implements `ironclaw::tools::tool::Tool` and gates I/O
//! through `jarvis_policies::DefaultPolicy::evaluate` before executing.
//!
//! Register all tools at startup with [`register_in_registry`].
//!
//! # Status
//!
//! Scaffold (M2.0). The body of `register_in_registry` is filled in
//! M2.1–M2.9 as adapters and tools are migrated from the legacy
//! `jarvis_linux_mcp` crate.

pub mod adapter;
pub mod tools;

use std::sync::Arc;

use ironclaw::tools::ToolRegistry;
use tracing::info;

/// Initialize all system adapters and register the corresponding tools
/// in the provided registry.
///
/// Currently a no-op stub. Filled in by M2.9 once all 7 tools are
/// migrated.
pub async fn register_in_registry(_registry: &Arc<ToolRegistry>) -> Result<(), Error> {
    info!("jarvis_system_tools: scaffold loaded (M2.0); no tools registered yet");
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("systemd D-Bus connection failed: {0}")]
    SystemdConnect(#[source] zbus::Error),
    #[error("NetworkManager D-Bus connection failed: {0}")]
    NetworkManagerConnect(#[source] zbus::Error),
}
