//! Audio output configuration — selects the TTS backend and tuning.
//!
//! Resolution is env-var-driven (no DB-backed setting yet — TTS is a
//! deployment-level capability, not a per-user preference). When/if
//! per-user TTS becomes desirable (different users picking Piper vs
//! ElevenLabs voices), promote this to settings.json like other
//! subsystem configs.

use crate::audio::tts::TtsBackendKind;
use crate::error::ConfigError;

/// Default broadcast channel capacity for PCM frames flowing from the
/// IPC reader to the analysis pipeline. ~half a second of 16 kHz audio
/// at typical 30 ms chunks (= ~16 chunks). Doubled to 64 so that a
/// brief analyzer stall doesn't immediately drop frames.
const DEFAULT_FRAME_BUFFER: usize = 64;

#[derive(Debug, Clone)]
pub struct AudioConfig {
    pub tts_backend: TtsBackendKind,
    /// Lag tolerance for the backend's broadcast channel. Subscribers
    /// more than this many frames behind lose the oldest entries.
    pub frame_buffer: usize,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            tts_backend: TtsBackendKind::None,
            frame_buffer: DEFAULT_FRAME_BUFFER,
        }
    }
}

impl AudioConfig {
    /// Resolve from env vars.
    ///
    /// `JARVIS_TTS_BACKEND`: `none` | `elevenlabs_local` (aliases:
    /// `elevenlabs-local`, `elevenlabs_ipc`, `elevenlabs-ipc`,
    /// `elevenlabs`, `voice_in_process`). Los aliases `elevenlabs*_ipc`
    /// se conservan por compat con .env preF4 — todos mapean al
    /// backend in-process. Unknown values fall back to `none`.
    ///
    /// `JARVIS_TTS_FRAME_BUFFER`: positive integer; falls back to
    /// [`DEFAULT_FRAME_BUFFER`] on parse error or 0.
    pub fn resolve() -> Result<Self, ConfigError> {
        let tts_backend = std::env::var("JARVIS_TTS_BACKEND")
            .ok()
            .map(|raw| Self::parse_backend(&raw))
            .unwrap_or(TtsBackendKind::None);

        let frame_buffer = std::env::var("JARVIS_TTS_FRAME_BUFFER")
            .ok()
            .and_then(|raw| raw.trim().parse::<usize>().ok())
            .filter(|n| *n > 0)
            .unwrap_or(DEFAULT_FRAME_BUFFER);

        Ok(Self {
            tts_backend,
            frame_buffer,
        })
    }

    fn parse_backend(raw: &str) -> TtsBackendKind {
        match raw.trim().to_ascii_lowercase().as_str() {
            "" | "none" | "off" | "false" | "0" | "disabled" => TtsBackendKind::None,
            "elevenlabs_local"
            | "elevenlabs-local"
            | "elevenlabs_ipc"
            | "elevenlabs-ipc"
            | "elevenlabs"
            | "voice_in_process" => TtsBackendKind::ElevenlabsLocal,
            other => {
                tracing::warn!(
                    value = other,
                    "Unknown JARVIS_TTS_BACKEND value, falling back to 'none'"
                );
                TtsBackendKind::None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_backend_recognises_known_values() {
        assert_eq!(AudioConfig::parse_backend("none"), TtsBackendKind::None);
        assert_eq!(AudioConfig::parse_backend("off"), TtsBackendKind::None);
        assert_eq!(AudioConfig::parse_backend(""), TtsBackendKind::None);
        // Todos los aliases (incluido legacy elevenlabs_ipc) mapean al
        // único backend in-process post-F4.
        assert_eq!(
            AudioConfig::parse_backend("elevenlabs"),
            TtsBackendKind::ElevenlabsLocal
        );
        assert_eq!(
            AudioConfig::parse_backend("elevenlabs_ipc"),
            TtsBackendKind::ElevenlabsLocal
        );
        assert_eq!(
            AudioConfig::parse_backend("ELEVENLABS-IPC"),
            TtsBackendKind::ElevenlabsLocal
        );
    }

    #[test]
    fn parse_backend_recognises_local() {
        assert_eq!(
            AudioConfig::parse_backend("elevenlabs_local"),
            TtsBackendKind::ElevenlabsLocal
        );
        assert_eq!(
            AudioConfig::parse_backend("ELEVENLABS-LOCAL"),
            TtsBackendKind::ElevenlabsLocal
        );
        assert_eq!(
            AudioConfig::parse_backend("voice_in_process"),
            TtsBackendKind::ElevenlabsLocal
        );
    }

    #[test]
    fn parse_backend_unknown_falls_back_to_none() {
        assert_eq!(
            AudioConfig::parse_backend("piper_local"),
            TtsBackendKind::None,
            "future backends fall back until wired up"
        );
        assert_eq!(
            AudioConfig::parse_backend("garbage"),
            TtsBackendKind::None
        );
    }

    #[test]
    fn default_is_disabled() {
        let cfg = AudioConfig::default();
        assert_eq!(cfg.tts_backend, TtsBackendKind::None);
        assert!(cfg.frame_buffer > 0);
    }
}
