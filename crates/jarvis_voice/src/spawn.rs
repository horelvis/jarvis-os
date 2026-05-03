//! **B1 only — borrado en B2.**
//!
//! Subprocess launcher para el binario `jarvis-voice-daemon` legacy. Se
//! mantiene mientras la migración al orquestador in-process está en
//! curso. La función completa se implementa en B1.2.

use crate::config::VoiceConfig;
use crate::error::VoiceError;

pub(crate) struct DaemonChild;

impl DaemonChild {
    pub(crate) async fn spawn(_cfg: &VoiceConfig) -> Result<Self, VoiceError> {
        Err(VoiceError::Spawn(
            "subprocess launcher not implemented yet".into(),
        ))
    }

    pub(crate) async fn shutdown(self) -> Result<(), VoiceError> {
        Ok(())
    }
}
