//! `ElevenLabsIpcBackend` — voice daemon bridge over local UNIX socket.
//!
//! The voice daemon (`jarvis-voice-daemon`) connects to ElevenLabs
//! Convai, receives PCM, plays it locally via cpal, and forwards the
//! same PCM over the local IPC channel as `ClientCommand::TtsPcmFrame`.
//! `LocalIpcChannel`'s `dispatch_command` calls [`push_frame`] on this
//! backend; the broadcast channel fans out to the analysis pipeline
//! (and any other future subscribers).
//!
//! When/if the voice daemon goes away (e.g. in a future Piper-only
//! deployment), this backend simply has no producer and stays idle.

use crate::audio::tts::TtsBackend;
use crate::audio::types::PcmFrame;
use tokio::sync::broadcast;

pub struct ElevenLabsIpcBackend {
    tx: broadcast::Sender<PcmFrame>,
}

impl ElevenLabsIpcBackend {
    /// Create a new backend with a bounded broadcast channel.
    ///
    /// `buffer` sets the lag tolerance: subscribers more than `buffer`
    /// frames behind will lose the oldest entries. The pipeline already
    /// handles `Lagged` gracefully (logs and continues).
    pub fn new(buffer: usize) -> Self {
        let (tx, _) = broadcast::channel(buffer.max(1));
        Self { tx }
    }

    /// Push a PCM frame received over IPC into the broadcast channel.
    ///
    /// Non-blocking: if the only subscriber lagged or the receiver was
    /// dropped, the send returns an error which we discard — the orb
    /// is decorative, dropping a frame is preferable to backpressuring
    /// the IPC reader (which would then backpressure the voice daemon).
    pub fn push_frame(&self, frame: PcmFrame) {
        // silent-ok: orb is decorative; lagged subscribers re-sync on next frame
        let _ = self.tx.send(frame);
    }
}

impl TtsBackend for ElevenLabsIpcBackend {
    fn name(&self) -> &str {
        "elevenlabs_ipc"
    }
    fn subscribe_frames(&self) -> broadcast::Receiver<PcmFrame> {
        self.tx.subscribe()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_stream::StreamExt;

    #[tokio::test]
    async fn push_frame_reaches_subscriber() {
        let backend = ElevenLabsIpcBackend::new(8);
        let mut stream = tokio_stream::wrappers::BroadcastStream::new(backend.subscribe_frames());
        backend.push_frame(PcmFrame {
            samples: vec![1, 2, 3],
            sample_rate: 16_000,
        });
        let frame = tokio::time::timeout(std::time::Duration::from_secs(1), stream.next())
            .await
            .expect("frame within 1s")
            .expect("stream not closed")
            .expect("not lagged");
        assert_eq!(frame.samples, vec![1, 2, 3]);
        assert_eq!(frame.sample_rate, 16_000);
    }

    #[tokio::test]
    async fn push_frame_without_subscriber_does_not_panic() {
        let backend = ElevenLabsIpcBackend::new(8);
        // No subscriber — send must silently no-op.
        backend.push_frame(PcmFrame {
            samples: vec![],
            sample_rate: 16_000,
        });
    }
}
