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
