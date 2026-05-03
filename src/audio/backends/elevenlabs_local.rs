//! `ElevenLabsLocalBackend` — TtsBackend respaldado por el crate
//! `jarvis_voice` corriendo in-process.
//!
//! Se suscribe a `VoiceEvent` del `VoiceHandle` y, cada vez que llega
//! `AgentAudio(pcm)`, traduce a `crate::audio::types::PcmFrame` y lo
//! broadcastea por el canal del trait. El `TtsAudioPipeline` consume
//! ese broadcast para emitir `AppEvent::AudioLevel` al orbe.

use crate::audio::tts::TtsBackend;
use crate::audio::types::PcmFrame as CorePcmFrame;
use crate::error::ConfigError;
use jarvis_voice::{
    PcmFrame as VoicePcmFrame, VoiceConfig, VoiceEngine, VoiceEvent, VoiceHandle,
};
use std::sync::Arc;
use tokio::sync::broadcast;

/// Capacidad del bus de salida hacia el TtsAudioPipeline. Lag tolerance:
/// subscribers más de `buffer` frames atrás pierden los más viejos.
pub struct ElevenLabsLocalBackend {
    tx: broadcast::Sender<CorePcmFrame>,
    /// Mantiene el VoiceHandle vivo. Drop → orquestador para.
    _voice_handle: Arc<VoiceHandle>,
}

impl ElevenLabsLocalBackend {
    pub async fn start(buffer: usize) -> Result<Self, ConfigError> {
        let cfg = VoiceConfig::from_env()
            .map_err(|e| ConfigError::ParseError(format!("voice config: {e}")))?;
        let handle = VoiceEngine::start(cfg)
            .await
            .map_err(|e| ConfigError::ParseError(format!("voice engine start: {e}")))?;
        let (tx, _) = broadcast::channel::<CorePcmFrame>(buffer.max(1));

        // Bridge VoiceEvent::AgentAudio → broadcast<CorePcmFrame>.
        let mut events_rx = handle.subscribe();
        let bridge_tx = tx.clone();
        tokio::spawn(async move {
            loop {
                match events_rx.recv().await {
                    Ok(VoiceEvent::AgentAudio(frame)) => {
                        // silent-ok: orb is decorative, drop on lagged
                        let _ = bridge_tx.send(into_core_frame(frame));
                    }
                    Ok(_) => {}
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::debug!(missed = n, "voice_event.lagged");
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });

        Ok(Self {
            tx,
            _voice_handle: Arc::new(handle),
        })
    }
}

fn into_core_frame(frame: VoicePcmFrame) -> CorePcmFrame {
    CorePcmFrame {
        samples: frame.samples.iter().copied().collect(),
        sample_rate: frame.sample_rate.hz(),
    }
}

impl TtsBackend for ElevenLabsLocalBackend {
    fn name(&self) -> &str {
        "elevenlabs_local"
    }
    fn subscribe_frames(&self) -> broadcast::Receiver<CorePcmFrame> {
        self.tx.subscribe()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jarvis_voice::SampleRate;

    /// Verifica el bridge a nivel de helper puro: el shape del frame del
    /// crate (`Arc<[i16]>` + `SampleRate`) se traduce correctamente al
    /// shape del core (`Vec<i16>` + `u32`). El path live (subscribe →
    /// AgentAudio → core PcmFrame) requiere VoiceEngine real y lo
    /// cubre la validación física en Asus, no este unit test.
    #[test]
    fn into_core_frame_preserves_samples_and_rate() {
        let voice = VoicePcmFrame {
            samples: Arc::from(vec![1i16, -2, 3].into_boxed_slice()),
            sample_rate: SampleRate::ELEVENLABS,
        };
        let core = into_core_frame(voice);
        assert_eq!(core.samples, vec![1, -2, 3]);
        assert_eq!(core.sample_rate, 16_000);
    }
}
