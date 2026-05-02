//! Primitive PCM container shared by every TTS backend and the
//! analysis pipeline.
//!
//! Kept deliberately minimal — `Vec<i16>` plus the sample rate. Every
//! backend (cloud bridge, in-process Piper, future Kokoro) speaks this
//! same shape regardless of how it produces the audio internally, so
//! the pipeline doesn't have to special-case anything.

#[derive(Debug, Clone)]
pub struct PcmFrame {
    /// Mono signed 16-bit PCM samples in playback order.
    pub samples: Vec<i16>,
    /// Sample rate in Hz (e.g. 16_000 for ElevenLabs, 22_050 for Piper).
    /// Stored alongside the samples so future backends with different
    /// rates don't need a separate channel — the analyzer reads the
    /// per-frame rate when grouping FFT bins.
    pub sample_rate: u32,
}
