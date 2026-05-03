//! `jarvis_voice` — voice engine in-process para jarvis-os.
//!
//! Encapsula la conversación con ElevenLabs Convai (audio I/O, WS, AEC,
//! resample). El `VoiceEngine` arranca el orquestador como tokio task
//! dentro del proceso `ironclaw` y emite `VoiceEvent` por broadcast.
//!
//! Histórico: en B1 el motor lanzaba el binario legacy
//! `jarvis-voice-daemon` como subprocess; B2 absorbió todo y borró el
//! daemon. AEC propio (WebRTC AEC3) llega en B3.

pub use config::VoiceConfig;
pub use engine::{VoiceEngine, VoiceHandle};
pub use error::VoiceError;
pub use types::{
    ConversationId, InterruptionReason, PcmFrame, SampleRate, ToolCallRequest, ToolCallResult,
    VoiceEvent,
};

mod audio_io;
mod config;
mod elevenlabs;
mod engine;
mod error;
mod orchestrator;
mod types;
