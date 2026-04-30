//! # jarvis_system_tools
//!
//! IronClaw built-in tools for Linux system introspection: processes,
//! journald, systemd units, networkd, polkit, btrfs.
//!
//! Each tool implements `ironclaw::tools::Tool` and gates I/O
//! through `jarvis_policies::DefaultPolicy::evaluate` before executing.
//!
//! Register all tools at startup with [`register_in_registry`].
//!
//! # Status
//!
//! Scaffold (M2.0) + adapters moved (M2.1). Tools migrate in M2.2–M2.8.
//! `register_in_registry` body wired in M2.9.

pub mod adapter;
pub mod error;
pub mod tools;

use std::sync::Arc;

use ironclaw::tools::ToolRegistry;
use tracing::info;

pub use error::{Error, Result};

/// Initialize all system adapters and register the corresponding tools
/// in the provided registry.
///
/// Currently a no-op stub. Filled in by M2.9 once all 7 tools are
/// migrated.
pub async fn register_in_registry(_registry: &Arc<ToolRegistry>) -> Result<()> {
    info!("jarvis_system_tools: scaffold loaded; no tools registered yet (filled in M2.9)");
    Ok(())
}
