//! Concrete `TtsBackend` implementations.
//!
//! `none` (TTS disabled) y `elevenlabs_local` (voice engine in-process
//! via `jarvis_voice` crate — ruta única tras F4).

pub mod elevenlabs_local;
pub mod none;

pub use elevenlabs_local::ElevenLabsLocalBackend;
pub use none::NoneBackend;
