//! Cross-channel event bus for jarvis-os.
//!
//! `EventBus` is the single broadcast channel that the agent loop, the
//! bridge (engine v2), and any future producer publishes `AppEvent`s to.
//! Channels (web, local_ipc, TUI, voice daemon) are *consumers* — they
//! subscribe to the bus and forward events to their respective surfaces.
//!
//! Today this module re-exports the existing `SseManager` under the
//! neutral name `EventBus` so callers can start migrating without code
//! motion. Subsequent commits move the implementation here, eliminate
//! the per-channel rebroadcast pattern (`WebChannel::send_status` etc.),
//! and add a TUI subscriber so engine v1 events reach the local_ipc
//! socket and the QML ring without going through the web gateway path.

pub mod event_bus;

pub use event_bus::EventBus;
