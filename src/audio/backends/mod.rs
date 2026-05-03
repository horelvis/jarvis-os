//! Concrete `TtsBackend` implementations.
//!
//! `none` (TTS disabled), `elevenlabs_ipc` (legacy daemon over local
//! UNIX socket — borrado en F4/B4), y `elevenlabs_local` (voice engine
//! in-process via `jarvis_voice` crate — ruta única tras F4/B2).

pub mod elevenlabs_ipc;
pub mod elevenlabs_local;
pub mod none;

pub use elevenlabs_ipc::ElevenLabsIpcBackend;
pub use elevenlabs_local::ElevenLabsLocalBackend;
pub use none::NoneBackend;
