//! Tipos públicos de `jarvis_voice`. Cumplen `.claude/rules/types.md`:
//! identifiers son newtypes con validación compartida; enums wire-stable
//! son `#[serde(rename_all = "snake_case")]`.

use crate::error::VoiceError;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct ConversationId(String);

impl ConversationId {
    fn validate(s: &str) -> Result<(), VoiceError> {
        let len = s.chars().count();
        if !(1..=128).contains(&len) {
            return Err(VoiceError::Validation(format!(
                "ConversationId length {len} out of 1..=128"
            )));
        }
        if !s
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Err(VoiceError::Validation(
                "ConversationId must be ASCII alphanumeric, underscore or hyphen".into(),
            ));
        }
        Ok(())
    }

    pub fn new(raw: impl Into<String>) -> Result<Self, VoiceError> {
        let s = raw.into();
        Self::validate(&s)?;
        Ok(Self(s))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

impl TryFrom<String> for ConversationId {
    type Error = VoiceError;
    fn try_from(value: String) -> Result<Self, VoiceError> {
        Self::validate(&value)?;
        Ok(Self(value))
    }
}

impl fmt::Display for ConversationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SampleRate(u32);

impl SampleRate {
    pub const ELEVENLABS: Self = SampleRate(16_000);

    pub fn new(hz: u32) -> Result<Self, VoiceError> {
        match hz {
            8_000 | 16_000 | 22_050 | 32_000 | 44_100 | 48_000 => Ok(SampleRate(hz)),
            other => Err(VoiceError::Validation(format!(
                "unsupported sample rate {other} (allowed: 8000/16000/22050/32000/44100/48000)"
            ))),
        }
    }

    pub fn hz(self) -> u32 {
        self.0
    }
}

#[derive(Debug, Clone)]
pub struct PcmFrame {
    pub samples: Arc<[i16]>,
    pub sample_rate: SampleRate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InterruptionReason {
    User,
    Server,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct ToolCallRequest {
    pub tool_call_id: String,
    pub tool_name: String,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone)]
pub struct ToolCallResult {
    pub tool_call_id: String,
    pub result: String,
    pub is_error: bool,
}

#[derive(Debug, Clone)]
pub enum VoiceEvent {
    Connected {
        conversation_id: ConversationId,
    },
    Disconnected,
    UserTranscript(String),
    AgentTranscript(String),
    AgentTranscriptCorrection {
        original: String,
        corrected: String,
    },
    Interrupted {
        reason: InterruptionReason,
    },
    ToolCallRequested(ToolCallRequest),
    AgentAudio(PcmFrame),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversation_id_accepts_valid() {
        assert!(ConversationId::new("conv_abc123").is_ok());
        assert!(ConversationId::new("a").is_ok());
        assert!(ConversationId::new("a-b_c-1").is_ok());
    }

    #[test]
    fn conversation_id_rejects_invalid() {
        assert!(ConversationId::new("").is_err());
        assert!(ConversationId::new("conv with space").is_err());
        assert!(ConversationId::new("conv/slash").is_err());
        let too_long = "a".repeat(129);
        assert!(ConversationId::new(too_long).is_err());
    }

    #[test]
    fn conversation_id_serde_validates() {
        let json = r#""conv_xyz""#;
        let id: ConversationId = serde_json::from_str(json).unwrap();
        assert_eq!(id.as_str(), "conv_xyz");

        let bad = r#""bad space""#;
        assert!(serde_json::from_str::<ConversationId>(bad).is_err());
    }

    #[test]
    fn sample_rate_accepts_known() {
        assert!(SampleRate::new(16_000).is_ok());
        assert!(SampleRate::new(48_000).is_ok());
    }

    #[test]
    fn sample_rate_rejects_unknown() {
        assert!(SampleRate::new(0).is_err());
        assert!(SampleRate::new(12_345).is_err());
    }
}
