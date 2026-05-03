//! Error types del crate.
//!
//! Boundary mapping: errores de WS/HTTP del provider se colapsan en
//! `Transport(String)` para no exponer códigos 5xx ni mensajes raw al
//! caller (sigue `.claude/rules/error-handling.md` "Error Boundaries").

use thiserror::Error;

#[derive(Debug, Error)]
pub enum VoiceError {
    #[error("voice transport error: {0}")]
    Transport(String),

    #[error("audio device error: {0}")]
    AudioDevice(String),

    #[error("AEC initialization failed: {0}")]
    AecInit(String),

    #[error("invalid voice config/value: {0}")]
    Validation(String),

    #[error("subprocess spawn failed: {0}")]
    Spawn(String),
}
