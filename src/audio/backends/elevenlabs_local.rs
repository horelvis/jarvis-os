//! `ElevenLabsLocalBackend` — TtsBackend respaldado por el crate
//! `jarvis_voice`.
//!
//! En B1 el `VoiceEngine` lanza el binario `jarvis-voice-daemon` como
//! subprocess. El daemon, a su vez, sigue publicando los frames PCM por
//! el canal IPC `ClientCommand::TtsPcmFrame` que recibe
//! `ElevenLabsIpcBackend::push_frame`. Por eso el shim B1 envuelve un
//! `ElevenLabsIpcBackend` interno: la única novedad funcional es quién
//! lanza el daemon.
//!
//! En B2 este archivo cambia su implementación: `VoiceEngine::start`
//! arranca el orquestador in-process y emite `VoiceEvent::AgentAudio`,
//! que se traduce a `crate::audio::types::PcmFrame` y se broadcastea a
//! los suscriptores del trait. La firma pública (`start` + `TtsBackend`)
//! no cambia entre B1 y B2.

use crate::audio::backends::ElevenLabsIpcBackend;
use crate::audio::tts::TtsBackend;
use crate::audio::types::PcmFrame;
use crate::error::ConfigError;
use jarvis_voice::{VoiceConfig, VoiceEngine, VoiceHandle};
use std::sync::Arc;
use tokio::sync::broadcast;

pub struct ElevenLabsLocalBackend {
    /// Canal IPC reaprovechado en B1 — desaparece en B2 cuando los frames
    /// llegan vía `VoiceEvent` directamente.
    ipc: Arc<ElevenLabsIpcBackend>,
    /// Mantiene el subprocess vivo. Se libera al `Drop` del backend.
    _voice_handle: Arc<VoiceHandle>,
}

impl ElevenLabsLocalBackend {
    pub async fn start(buffer: usize) -> Result<Self, ConfigError> {
        let cfg = VoiceConfig::from_env()
            .map_err(|e| ConfigError::ParseError(format!("voice config: {e}")))?;
        let handle = VoiceEngine::start(cfg)
            .await
            .map_err(|e| ConfigError::ParseError(format!("voice engine start: {e}")))?;
        let ipc = Arc::new(ElevenLabsIpcBackend::new(buffer));
        Ok(Self {
            ipc,
            _voice_handle: Arc::new(handle),
        })
    }

    /// Acceso al backend IPC subyacente — el local_ipc channel sigue
    /// invocando `push_frame` sobre el `ElevenLabsIpcBackend` que
    /// envolvemos. En B2 esta función desaparece y `dispatch_command`
    /// deja de tocar el TtsBackend.
    pub fn ipc_backend(&self) -> Arc<ElevenLabsIpcBackend> {
        Arc::clone(&self.ipc)
    }
}

impl TtsBackend for ElevenLabsLocalBackend {
    fn name(&self) -> &str {
        "elevenlabs_local"
    }
    fn subscribe_frames(&self) -> broadcast::Receiver<PcmFrame> {
        self.ipc.subscribe_frames()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_stream::StreamExt;

    /// El shim tiene que delegar `subscribe_frames` al
    /// `ElevenLabsIpcBackend` envuelto: los frames pushed via IPC tienen
    /// que llegar a los suscriptores del trait.
    #[tokio::test]
    async fn delegates_subscribe_to_inner_ipc_backend() {
        let inner = Arc::new(ElevenLabsIpcBackend::new(8));
        let handle = make_dummy_handle().await;
        let backend = ElevenLabsLocalBackend {
            ipc: Arc::clone(&inner),
            _voice_handle: Arc::new(handle),
        };
        let mut stream =
            tokio_stream::wrappers::BroadcastStream::new(backend.subscribe_frames());
        inner.push_frame(PcmFrame {
            samples: vec![10, 20, 30],
            sample_rate: 16_000,
        });
        let frame = tokio::time::timeout(std::time::Duration::from_secs(1), stream.next())
            .await
            .expect("frame within 1s")
            .expect("stream not closed")
            .expect("not lagged");
        assert_eq!(frame.samples, vec![10, 20, 30]);
        assert_eq!(backend.name(), "elevenlabs_local");
    }

    /// Construye un `VoiceHandle` sin lanzar el daemon real — usa
    /// `/usr/bin/true` como stand-in del binario igual que el test del crate.
    async fn make_dummy_handle() -> VoiceHandle {
        // SAFETY: env mutation in tests; el subprocess hereda copia de
        // estos valores y el set/remove se ejecuta secuencialmente.
        unsafe {
            std::env::set_var("JARVIS_VOICE_DAEMON_BIN", "/usr/bin/true");
            std::env::set_var("ELEVENLABS_AGENT_ID", "agent_dummy");
            std::env::set_var("ELEVENLABS_API_KEY", "key_dummy");
        }
        let cfg = VoiceConfig::from_env().expect("env vars set above");
        let handle = VoiceEngine::start(cfg).await.expect("dummy spawn");
        unsafe {
            std::env::remove_var("JARVIS_VOICE_DAEMON_BIN");
            std::env::remove_var("ELEVENLABS_AGENT_ID");
            std::env::remove_var("ELEVENLABS_API_KEY");
        }
        handle
    }
}
