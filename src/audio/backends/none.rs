//! `NoneBackend` — placeholder for "TTS disabled" deployments.
//!
//! Holds an empty broadcast channel so `subscribe_frames()` is always
//! callable, but no producer ever pushes. The pipeline will subscribe,
//! park on `recv().await`, and never wake — equivalent to the feature
//! being off without special-casing the pipeline lifecycle.

use crate::audio::tts::TtsBackend;
use crate::audio::types::PcmFrame;
use tokio::sync::broadcast;

pub struct NoneBackend {
    tx: broadcast::Sender<PcmFrame>,
}

impl NoneBackend {
    pub fn new() -> Self {
        // Capacity 1 is enough — no producer will ever send.
        let (tx, _) = broadcast::channel(1);
        Self { tx }
    }
}

impl Default for NoneBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl TtsBackend for NoneBackend {
    fn name(&self) -> &str {
        "none"
    }
    fn subscribe_frames(&self) -> broadcast::Receiver<PcmFrame> {
        self.tx.subscribe()
    }
}
