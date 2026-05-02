//! `EventBus` — neutral alias for the cross-channel broadcast hub.
//!
//! Implementation currently lives at
//! `crate::channels::web::platform::sse::SseManager`. Re-exported here
//! so producers and consumers can use a name that does not lie about
//! the bus being a web/SSE concern.
//!
//! A subsequent commit moves the implementation into this file and
//! turns `SseManager` into the legacy alias. Until then, this is a
//! compile-time alias only — same struct, same broadcast channel.

pub use crate::channels::web::platform::sse::SseManager as EventBus;
