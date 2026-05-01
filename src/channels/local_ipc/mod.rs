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

// pub use channel_impl::LocalIpcChannel;  // populated by Task E2
pub use error::LocalIpcError;
// pub use socket::resolve_socket_path;    // populated by Task C1
