//! TTS audio output pipeline — core-native source of `AppEvent::AudioLevel`
//! events for the orb's reactive bands.
//!
//! IronClaw owns the analysis (FFT/RMS) and event emission. Concrete TTS
//! engines (cloud bridges like the voice daemon, or future local models
//! such as Piper / Kokoro) plug in behind the [`tts::TtsBackend`] trait
//! and feed PCM frames into the pipeline. The pipeline broadcasts
//! `AudioLevel` via the global `EventBus` exactly when the audio it
//! describes is about to be heard by the user.

pub mod analysis;
pub mod backends;
pub mod pipeline;
pub mod tts;
pub mod types;

pub use pipeline::TtsAudioPipeline;
pub use tts::{TtsBackend, TtsBackendKind};
pub use types::PcmFrame;
