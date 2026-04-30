//! # jarvis_system_tools
//!
//! Adapters for Linux system introspection: processes, journald, systemd
//! units, networkd, polkit, btrfs.
//!
//! These adapters are **infrastructure only** — they speak D-Bus, read
//! procfs, etc. They do NOT implement IronClaw's `Tool` trait. The `Tool`
//! impls live inside the root `ironclaw` crate at
//! `src/tools/builtin/jarvis_system/` to avoid a cyclic dependency.

pub mod adapter;
pub mod error;

pub use error::{Error, Result};
