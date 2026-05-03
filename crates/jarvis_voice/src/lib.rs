//! `jarvis_voice` — voice engine in-process para jarvis-os.
//!
//! Encapsula la conversación con ElevenLabs Convai (audio I/O, WS, AEC,
//! resample). En B1 el `VoiceEngine` lanza el binario legacy
//! `jarvis-voice-daemon` como subprocess; en B2 el binario desaparece y
//! todo corre dentro del proceso de IronClaw.
//!
//! Superficie pública estable a partir de B1 — el comportamiento
//! interno cambia entre B1 y B2 sin tocar la API.

pub use config::VoiceConfig;
pub use engine::{VoiceEngine, VoiceHandle};
pub use error::VoiceError;
pub use types::{
    ConversationId, InterruptionReason, PcmFrame, SampleRate, ToolCallRequest, ToolCallResult,
    VoiceEvent,
};

mod config;
mod engine;
mod error;
mod spawn;
mod types;
