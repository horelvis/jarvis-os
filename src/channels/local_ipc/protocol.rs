// File-level dead_code allow stays until Tracks D/E wire the consumers
// (client.rs reads ClientCommand, channel_impl.rs constructs IpcHello,
// control.rs uses ApprovalAction). Remove once every type has a caller.
#![allow(dead_code)]

use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ClientIdError {
    #[error("client id must not be empty")]
    Empty,
    #[error("client id must be <= 64 chars (got {0})")]
    TooLong(usize),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct ClientId(String);

impl ClientId {
    fn validate(s: &str) -> Result<(), ClientIdError> {
        if s.is_empty() {
            return Err(ClientIdError::Empty);
        }
        let count = s.chars().count();
        if count > 64 {
            return Err(ClientIdError::TooLong(count));
        }
        Ok(())
    }

    pub fn new(raw: impl Into<String>) -> Result<Self, ClientIdError> {
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

impl TryFrom<String> for ClientId {
    type Error = ClientIdError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::validate(&value)?;
        Ok(Self(value))
    }
}

impl AsRef<str> for ClientId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ClientId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<ClientId> for String {
    fn from(id: ClientId) -> Self {
        id.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_id_rejects_empty() {
        assert!(matches!(ClientId::new(""), Err(ClientIdError::Empty)));
    }

    #[test]
    fn client_id_rejects_too_long() {
        let s = "a".repeat(65);
        assert!(matches!(ClientId::new(s), Err(ClientIdError::TooLong(65))));
    }

    #[test]
    fn client_id_accepts_valid() {
        let id = ClientId::new("ipc-42").expect("valid id");
        assert_eq!(id.as_str(), "ipc-42");
    }

    #[test]
    fn client_id_serde_roundtrip() {
        let id = ClientId::new("c1").unwrap();
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "\"c1\"");
        let back: ClientId = serde_json::from_str(&json).unwrap();
        assert_eq!(back, id);
    }

    #[test]
    fn client_id_serde_rejects_invalid() {
        let res: Result<ClientId, _> = serde_json::from_str("\"\"");
        assert!(res.is_err());
    }
}

/// Wire-stable error kinds emitted to the client as a synthetic `error`
/// transport event. Snake_case on the wire (rule: types.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IpcErrorKind {
    CommandInvalid,
    CommandTooLarge,
    RateLimit,
    InternalError,
}

#[cfg(test)]
mod kind_tests {
    use super::IpcErrorKind;

    #[test]
    fn kind_serializes_snake_case() {
        let s = serde_json::to_string(&IpcErrorKind::CommandInvalid).unwrap();
        assert_eq!(s, "\"command_invalid\"");
        let s = serde_json::to_string(&IpcErrorKind::CommandTooLarge).unwrap();
        assert_eq!(s, "\"command_too_large\"");
        let s = serde_json::to_string(&IpcErrorKind::RateLimit).unwrap();
        assert_eq!(s, "\"rate_limit\"");
        let s = serde_json::to_string(&IpcErrorKind::InternalError).unwrap();
        assert_eq!(s, "\"internal_error\"");
    }

    #[test]
    fn kind_deserializes_snake_case() {
        let k: IpcErrorKind = serde_json::from_str("\"command_invalid\"").unwrap();
        assert_eq!(k, IpcErrorKind::CommandInvalid);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalAction {
    Approve,
    Deny,
}

/// Commands the client may send to the server. Wire-stable.
///
/// `thread_id` and `step_id` are kept as `String` here (not the engine's
/// `ThreadId` newtype) because the wire payload is untrusted and the
/// engine-facing constructors will validate at the call site.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientCommand {
    Message {
        content: String,
        #[serde(default)]
        thread_id: Option<String>,
    },
    Approval {
        request_id: String,
        action: ApprovalAction,
    },
    Cancel {
        #[serde(default)]
        step_id: Option<String>,
    },
    Ping,
    /// PCM frame from a TTS adapter (voice daemon, future bridges).
    /// Routed by `dispatch_command` to the configured TTS backend's
    /// `push_frame`, which fans out to the analysis pipeline. PCM is
    /// base64-encoded as little-endian `i16` so the wire stays a flat
    /// JSON line — same encoding ElevenLabs already uses to deliver
    /// audio to the voice daemon, so the daemon can forward without
    /// re-encoding.
    TtsPcmFrame {
        samples_b64: String,
        sample_rate: u32,
    },
}

#[cfg(test)]
mod command_tests {
    use super::*;

    #[test]
    fn message_roundtrip() {
        let raw = r#"{"type":"message","content":"hola","thread_id":"t1"}"#;
        let cmd: ClientCommand = serde_json::from_str(raw).unwrap();
        assert_eq!(
            cmd,
            ClientCommand::Message {
                content: "hola".into(),
                thread_id: Some("t1".into()),
            }
        );
    }

    #[test]
    fn message_thread_id_optional() {
        let raw = r#"{"type":"message","content":"hi"}"#;
        let cmd: ClientCommand = serde_json::from_str(raw).unwrap();
        assert_eq!(
            cmd,
            ClientCommand::Message {
                content: "hi".into(),
                thread_id: None,
            }
        );
    }

    #[test]
    fn approval_roundtrip() {
        let raw = r#"{"type":"approval","request_id":"r1","action":"approve"}"#;
        let cmd: ClientCommand = serde_json::from_str(raw).unwrap();
        assert_eq!(
            cmd,
            ClientCommand::Approval {
                request_id: "r1".into(),
                action: ApprovalAction::Approve,
            }
        );
    }

    #[test]
    fn cancel_roundtrip() {
        let raw = r#"{"type":"cancel"}"#;
        let cmd: ClientCommand = serde_json::from_str(raw).unwrap();
        assert_eq!(cmd, ClientCommand::Cancel { step_id: None });
    }

    #[test]
    fn ping_roundtrip() {
        let raw = r#"{"type":"ping"}"#;
        let cmd: ClientCommand = serde_json::from_str(raw).unwrap();
        assert_eq!(cmd, ClientCommand::Ping);
    }

    #[test]
    fn tts_pcm_frame_roundtrip() {
        let raw = r#"{"type":"tts_pcm_frame","samples_b64":"AAEC","sample_rate":16000}"#;
        let cmd: ClientCommand = serde_json::from_str(raw).unwrap();
        assert_eq!(
            cmd,
            ClientCommand::TtsPcmFrame {
                samples_b64: "AAEC".into(),
                sample_rate: 16_000,
            }
        );
    }

    #[test]
    fn unknown_type_rejected() {
        let raw = r#"{"type":"frobnicate"}"#;
        let res: Result<ClientCommand, _> = serde_json::from_str(raw);
        assert!(res.is_err());
    }
}

pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcHello {
    pub protocol_version: u32,
    pub local_user_id: String,
}

/// Envelope for transport-only synthetic events that don't originate
/// from the engine `AppEvent` log. Serialized with the same `{"type":
/// "...", ...}` shape so the QML / voice-daemon parser only needs one
/// case branch.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TransportEvent {
    IpcHello(IpcHello),
    Error { kind: IpcErrorKind, detail: String },
}

#[cfg(test)]
mod transport_tests {
    use super::*;

    #[test]
    fn hello_serializes_with_type_tag() {
        let ev = TransportEvent::IpcHello(IpcHello {
            protocol_version: 1,
            local_user_id: "owner".into(),
        });
        let s = serde_json::to_string(&ev).unwrap();
        assert!(s.contains("\"type\":\"ipc_hello\""));
        assert!(s.contains("\"protocol_version\":1"));
        assert!(s.contains("\"local_user_id\":\"owner\""));
    }

    #[test]
    fn error_serializes_snake_case_kind() {
        let ev = TransportEvent::Error {
            kind: IpcErrorKind::CommandInvalid,
            detail: "bad json".into(),
        };
        let s = serde_json::to_string(&ev).unwrap();
        assert!(s.contains("\"type\":\"error\""));
        assert!(s.contains("\"kind\":\"command_invalid\""));
    }
}
