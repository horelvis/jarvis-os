//! `TtsBackend` trait — the contract every TTS engine implements.
//!
//! Backends produce PCM frames; the pipeline ([`crate::audio::pipeline`])
//! consumes them and broadcasts `AppEvent::AudioLevel` events. Multiple
//! pipeline subscribers are supported via `tokio::sync::broadcast`, so
//! future analyzers (waveform recorder, transcript-aligned subtitles,
//! …) can fan out from the same backend without copying.

use crate::audio::types::PcmFrame;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

/// Configurable kind of TTS backend.
///
/// Wire-stable snake_case (rule: `.claude/rules/types.md`). When you
/// add a new backend (Piper, Kokoro, Sherpa…) extend this enum AND
/// the resolver in `crate::config::audio` so settings reach it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TtsBackendKind {
    /// TTS disabled — no `AudioLevel` events emitted, orb stays idle.
    None,
    /// `ElevenLabsLocalBackend` — voice engine in-process via el crate
    /// `jarvis_voice`. Ruta única tras F4.
    ElevenlabsLocal,
    // Future:
    // PiperLocal,
    // KokoroLocal,
}

impl TtsBackendKind {
    /// Canonical wire string, matches the `#[serde(rename_all)]` form.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::ElevenlabsLocal => "elevenlabs_local",
        }
    }
}

/// Common contract every TTS backend implements.
///
/// Two-method surface:
/// - [`name`](Self::name) — short identifier for logs / status endpoints.
/// - [`subscribe_frames`](Self::subscribe_frames) — pull-side. The
///   pipeline (and any other observer) receives a fresh
///   `broadcast::Receiver`. Backends fan their internal PCM stream out
///   to all subscribers; if any subscriber lags, that subscriber
///   skips frames — the audio path is never blocked.
///
/// Push-side methods (e.g. `push_frame` on
/// [`crate::audio::backends::ElevenLabsLocalBackend`]) are intentionally
/// NOT part of the trait. Each backend chooses how it ingests audio
/// (IPC writer, ONNX inference, file decode…) and the call sites that
/// know how to ingest depend on the concrete type. The trait is
/// strictly the consumer surface.
pub trait TtsBackend: Send + Sync {
    /// Backend identifier ("elevenlabs_local", "piper_local", "none"…).
    fn name(&self) -> &str;

    /// Subscribe to the live PCM stream. Each subscriber gets its own
    /// receiver; lag is per-subscriber.
    fn subscribe_frames(&self) -> broadcast::Receiver<PcmFrame>;
}
