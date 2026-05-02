//! Concrete `TtsBackend` implementations.
//!
//! Today: `none` (TTS disabled) and `elevenlabs_ipc` (voice daemon
//! bridge over local UNIX socket). Future in-process engines (Piper,
//! Kokoro, Sherpa-onnx, F5-TTS) will land here as additional modules.

pub mod elevenlabs_ipc;
pub mod none;

pub use elevenlabs_ipc::ElevenLabsIpcBackend;
pub use none::NoneBackend;
