//! `EventBus` — re-export of the cross-channel broadcast hub.
//!
//! The struct itself currently lives at
//! `crate::channels::web::platform::sse` for historical reasons (it grew
//! up inside the web gateway before becoming the universal event bus).
//! It has been renamed there to `EventBus`; the old name `SseManager` is
//! preserved as a legacy `pub type` alias in that file so the existing
//! ~36 import sites keep working without churn.
//!
//! New code should import `crate::events::EventBus` from here.

pub use crate::channels::web::platform::sse::EventBus;
