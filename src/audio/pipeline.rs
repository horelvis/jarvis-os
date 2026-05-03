//! `TtsAudioPipeline` — typed source log for `AppEvent::AudioLevel`.
//!
//! Subscribes to a [`TtsBackend`]'s PCM stream, runs each frame
//! through [`analyze_pcm`] and broadcasts the resulting `(rms, bands)`
//! over the global [`EventBus`] as a single typed wire event. The
//! pipeline is the canonical projection function that
//! `gateway-events.md` requires — every `AudioLevel` event reaching
//! SSE/WS originates here.

use crate::audio::analysis::analyze_pcm;
use crate::audio::tts::TtsBackend;
use crate::events::EventBus;
use ironclaw_common::AppEvent;
use std::sync::Arc;
use tokio::sync::broadcast::error::RecvError;
use tokio::task::JoinHandle;
use tracing::debug;

/// Marker type for the audio analysis pipeline. Stateless — the
/// associated [`spawn`](Self::spawn) function owns its receiver and
/// lives entirely inside the spawned task.
pub struct TtsAudioPipeline;

impl TtsAudioPipeline {
    /// Spawn the analysis loop. The backend subscription is created
    /// synchronously before the task starts so a `push_frame` racing
    /// with `spawn` cannot silently drop the first frame.
    ///
    /// Returns a `JoinHandle<()>` that resolves when the backend's
    /// broadcast sender drops (typically: process shutdown).
    pub fn spawn(backend: Arc<dyn TtsBackend>, event_bus: Arc<EventBus>) -> JoinHandle<()> {
        let mut rx = backend.subscribe_frames();
        let name = backend.name().to_string();
        tokio::spawn(async move {
            debug!(backend = %name, "tts_pipeline.started");
            loop {
                match rx.recv().await {
                    Ok(frame) => {
                        let (rms, bands) = analyze_pcm(&frame.samples);
                        // projection-exempt: transport-only, audio_level
                        event_bus.broadcast(AppEvent::AudioLevel {
                            rms,
                            bands: bands.to_vec(),
                        });
                    }
                    Err(RecvError::Lagged(skipped)) => {
                        // Audio path can outrun the analyzer briefly;
                        // skipping is acceptable since the orb only
                        // needs the most recent level.
                        debug!(backend = %name, skipped, "tts_pipeline.lagged");
                    }
                    Err(RecvError::Closed) => {
                        debug!(backend = %name, "tts_pipeline.backend_closed");
                        break;
                    }
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::tts::TtsBackend;
    use crate::audio::types::PcmFrame;
    use tokio::sync::broadcast;
    use tokio_stream::StreamExt;

    /// Mock backend para tests del pipeline. No depende de IPC ni del
    /// VoiceEngine real; expone un broadcast::Sender para que los
    /// tests pushen frames sintéticos a la pipeline.
    struct TestBackend {
        tx: broadcast::Sender<PcmFrame>,
    }
    impl TestBackend {
        fn new(buf: usize) -> Self {
            let (tx, _) = broadcast::channel(buf);
            Self { tx }
        }
        fn push(&self, frame: PcmFrame) {
            let _ = self.tx.send(frame);
        }
    }
    impl TtsBackend for TestBackend {
        fn name(&self) -> &str {
            "test"
        }
        fn subscribe_frames(&self) -> broadcast::Receiver<PcmFrame> {
            self.tx.subscribe()
        }
    }

    /// A pure-tone frame fed through the pipeline produces an
    /// `AudioLevel` event on the EventBus with non-zero RMS.
    #[tokio::test]
    async fn pipeline_emits_audio_level_on_frame() {
        let event_bus = Arc::new(EventBus::new());
        let backend = Arc::new(TestBackend::new(16));

        // Subscribe BEFORE spawn so the test's bus subscription is
        // ready when the pipeline broadcasts. The pipeline itself
        // subscribes to the backend synchronously inside spawn(), so
        // the first push below cannot race past it.
        let mut sub = event_bus
            .subscribe_raw(None, false)
            .expect("subscribe to event bus");
        let _handle = TtsAudioPipeline::spawn(backend.clone(), event_bus.clone());

        // 500 Hz tone (lands in band 2 per analyze_pcm tests).
        let n = 1024;
        let freq_hz = 500.0;
        let samples: Vec<i16> = (0..n)
            .map(|i| {
                let t = i as f32 / 16_000.0;
                let amp = 0.5 * (2.0 * std::f32::consts::PI * freq_hz * t).sin();
                (amp * 32767.0) as i16
            })
            .collect();
        backend.push(PcmFrame {
            samples,
            sample_rate: 16_000,
        });

        let event = tokio::time::timeout(std::time::Duration::from_secs(2), sub.next())
            .await
            .expect("pipeline must emit within 2s")
            .expect("event stream returned None");

        match event {
            AppEvent::AudioLevel { rms, bands } => {
                assert!(rms > 0.0, "expected non-zero rms for tone, got {rms}");
                assert_eq!(bands.len(), 5, "expected 5 bands");
            }
            other => panic!("expected AudioLevel, got {other:?}"),
        }
    }
}
