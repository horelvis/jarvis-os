# jarvis-voice In-Process Consolidation — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Absorber `jarvis_voice_daemon` como librería in-process dentro del binario `ironclaw`, eliminando IPC PCM y reemplazando la dependencia de PipeWire `module-echo-cancel` por un AEC propio (WebRTC AEC3 + `rubato` resample).

**Architecture:** Crate nuevo `crates/jarvis_voice/` expone `VoiceEngine::start(VoiceConfig) -> VoiceHandle`. Internamente: cpal mic → resample 16k → AEC.process_stream → tokio-tungstenite WS a ElevenLabs Convai; WS inbound audio → AEC.process_reverse_stream → resample → cpal speaker; `VoiceEvent::AgentAudio` se broadcastea para que un shim `ElevenLabsLocalBackend` en `src/audio/backends/` lo entregue al `TtsAudioPipeline` que ya emite `AppEvent::AudioLevel` para el orbe.

**Tech Stack:**
- Audio I/O: `cpal 0.15`, `ringbuf 0.4`
- WebSocket: `tokio-tungstenite 0.24` (rustls-tls-webpki-roots)
- AEC: `webrtc-audio-processing 0.5` (AEC3, NS aggressive, AGC adaptive)
- Resample: `rubato 0.16` (FFT polyphase)
- Async: `tokio` (workspace), `futures-util 0.3`
- Wire: `serde`, `serde_json`, `base64 0.22`
- Errores: `thiserror`, `anyhow`
- Logging: `tracing`

**Spec:** `docs/superpowers/specs/2026-05-02-jarvis-voice-in-process-design.md`

**Branch:** `jarvis-arch-os` (working branch, todos los commits van directo aquí; merges al final).

**Validación física:** todos los smoke-tests E2E (B1, B2, B3 final) requieren ejecutar en la máquina **Asus ZenBook con Arch + Hyprland** con mic+speakerphone abiertos. No marcar B1/B2/B3 como completos sin esa validación humana.

---

## File Structure

### Nuevos

| Path | Responsabilidad |
|------|-----------------|
| `crates/jarvis_voice/Cargo.toml` | Metadatos + deps (cpal, tokio-tungstenite, rubato, webrtc-audio-processing) |
| `crates/jarvis_voice/src/lib.rs` | Re-exports públicos: `VoiceEngine`, `VoiceHandle`, `VoiceConfig`, `VoiceError`, `PcmFrame`, `VoiceEvent`, `ConversationId`, `SampleRate`, `InterruptionReason`, `ToolCallRequest`, `ToolCallResult` |
| `crates/jarvis_voice/src/types.rs` | Newtypes (`ConversationId`, `SampleRate`) + DTOs (`PcmFrame`, `VoiceEvent`, `InterruptionReason`, `ToolCallRequest`, `ToolCallResult`) |
| `crates/jarvis_voice/src/error.rs` | `VoiceError` con `thiserror` (`Transport`, `AudioDevice`, `AecInit`, `Validation`, `Spawn`) |
| `crates/jarvis_voice/src/config.rs` | `VoiceConfig::from_env()` — única fuente de verdad de envs (`ELEVENLABS_AGENT_ID`, `ELEVENLABS_API_KEY`, `JARVIS_VOICE_*`) |
| `crates/jarvis_voice/src/engine.rs` | `VoiceEngine`, `VoiceHandle` (subscribe/stop/send_tool_result) |
| `crates/jarvis_voice/src/spawn.rs` | **B1 only — borrado en B2.** Subprocess launcher placeholder |
| `crates/jarvis_voice/src/audio_io/mod.rs` | **B2.** cpal mic + speaker; ex `daemon/audio.rs` |
| `crates/jarvis_voice/src/elevenlabs/mod.rs` | **B2.** WS connect + read/write tasks; ex `daemon/ws_client.rs` |
| `crates/jarvis_voice/src/elevenlabs/protocol.rs` | **B2.** ElevenLabs Convai DTOs; ex `daemon/protocol.rs` |
| `crates/jarvis_voice/src/orchestrator.rs` | **B2.** Tie audio_io + ws + (B3) AEC; emite `VoiceEvent` por broadcast |
| `crates/jarvis_voice/src/aec.rs` | **B3.** Wrapper de `webrtc_audio_processing::Processor` |
| `crates/jarvis_voice/src/resample.rs` | **B3.** `MicResampler`, `SpeakerResampler` con `rubato` polyphase |
| `crates/jarvis_voice/tests/aec_smoke.rs` | **B3.** Test sintético de convergencia AEC |
| `src/audio/backends/elevenlabs_local.rs` | Shim `TtsBackend` que arranca `VoiceEngine` y traduce `VoiceEvent::AgentAudio` → `crate::audio::types::PcmFrame` |

### Modificados

| Path | Cambio |
|------|--------|
| `Cargo.toml` (workspace) | B1: añadir `crates/jarvis_voice`. B2: quitar `crates/jarvis_voice_daemon` |
| `src/audio/backends/mod.rs` | Añadir mod + re-export `ElevenLabsLocalBackend`; B4 quitar `ElevenLabsIpcBackend` |
| `src/audio/tts.rs` | Añadir `TtsBackendKind::ElevenlabsLocal`; B4 quitar `ElevenlabsIpc` |
| `src/config/audio.rs` | Resolver alias `elevenlabs_local`; en B2 delegar a `VoiceConfig::from_env()` |
| `src/main.rs` | Branch en `tts_backend` ya selecciona `ElevenLabsLocalBackend`; threading `Option<Arc<ElevenLabsIpcBackend>>` borrado en B4 |
| `src/channels/local_ipc/{mod,client,channel_impl,socket}.rs` | B4: borrar parámetro `tts_backend` y rama `ClientCommand::TtsPcmFrame` |
| `src/channels/local_ipc/protocol.rs` | B4: borrar variant `TtsPcmFrame` + tests |
| `src/audio/backends/elevenlabs_ipc.rs` | B4: borrado |
| `arch/install.sh`, `arch/update.sh` | B2: borrar referencias a `jarvis-voice-daemon` binary; B3: borrar pasos `module-echo-cancel` |
| `arch/systemd-user/jarvis-voice-daemon.service` | B2: borrado |
| `arch/configs/pipewire/echo-cancel.conf` | B3: borrado |
| `arch/templates/ironclaw.env` | B2: ajustar comentarios; envs ahora consumidas in-process |
| `CLAUDE.md` | B5: project structure + extracted crates |

### Borrados

| Path | Cuándo |
|------|--------|
| `crates/jarvis_voice_daemon/` (todo el crate) | B2 |
| `crates/jarvis_voice/src/spawn.rs` | B2 |
| `arch/systemd-user/jarvis-voice-daemon.service` | B2 |
| `arch/configs/pipewire/echo-cancel.conf` | B3 |
| `src/audio/backends/elevenlabs_ipc.rs` | B4 |
| `ClientCommand::TtsPcmFrame` (variant) | B4 |

---

## Convenciones globales

- **Sin `unwrap()`/`expect()` en código de producción.** Tests OK.
- **Newtypes con `try_from`** según `.claude/rules/types.md`. Validación compartida en `validate(&str)`.
- **Logging:** `debug!` para internals; `info!` solo para eventos puntuales (`session.ready`, `session.disconnected`). NO `info!` en hot paths de audio (corrompen la TUI).
- **Errores en boundary:** WS errors HTTP/5xx → `VoiceError::Transport(String)`, sin exponer el mensaje raw del provider al caller. cpal device errors → `VoiceError::AudioDevice`.
- **Imports cross-module:** `crate::` (no `super::`).
- **Comentarios:** explican el WHY no obvio. Nada de "added for X bug" ni "used by Y".
- **`tracing::info!` y `warn!`** corrompen REPL/TUI según CLAUDE.md — preferir `debug!`.

---

# B1 — `feat(voice)[F4/B1]: scaffold crates/jarvis_voice as daemon launcher`

**Estado al final de B1:** `crates/jarvis_voice/` existe con superficie pública estable. `VoiceEngine::start` lanza el binario `jarvis-voice-daemon` como subprocess hijo y delega la entrega de PCM al canal IPC existente. Path único en producción sigue siendo el daemon vía systemd; el flag `voice-in-process` opt-in conmuta a "ironclaw lanza el daemon".

---

### Task 1.1: Crear esqueleto del crate

**Files:**
- Create: `crates/jarvis_voice/Cargo.toml`
- Create: `crates/jarvis_voice/src/lib.rs`
- Create: `crates/jarvis_voice/src/types.rs`
- Create: `crates/jarvis_voice/src/error.rs`
- Create: `crates/jarvis_voice/src/config.rs`
- Create: `crates/jarvis_voice/src/engine.rs`
- Create: `crates/jarvis_voice/src/spawn.rs`
- Modify: `Cargo.toml` (workspace `members`)

- [ ] **Step 1.1.1: Crear `Cargo.toml` del crate**

```toml
[package]
name = "jarvis_voice"
version = "0.1.0"
edition = "2024"
license = "MIT OR Apache-2.0"
description = "jarvis-os voice engine: ElevenLabs Conversational AI client in-process (audio + WS + AEC)"

[dependencies]
tokio = { workspace = true, features = ["full"] }
serde = { workspace = true, features = ["derive"] }
serde_json = { workspace = true }
tracing = { workspace = true }
thiserror = { workspace = true }
anyhow = { workspace = true }

[dev-dependencies]
tokio = { workspace = true, features = ["macros", "rt-multi-thread", "time"] }
```

> NOTA B1: las deps de runtime (cpal/tokio-tungstenite/base64/futures-util/rubato/webrtc-audio-processing) entran en B2 y B3. En B1 el subprocess launcher solo necesita tokio para `Command::new`.

- [ ] **Step 1.1.2: Añadir el crate al workspace**

Edit `Cargo.toml` raíz: añadir `"crates/jarvis_voice"` al array `members`. Mantener `crates/jarvis_voice_daemon` (sigue vivo hasta B2).

```toml
members = [".", "crates/ironclaw_common", "crates/ironclaw_safety", "crates/ironclaw_skills", "crates/ironclaw_engine", "crates/ironclaw_gateway", "crates/ironclaw_tui", "crates/jarvis_policies", "crates/jarvis_system_tools", "crates/jarvis_voice", "crates/jarvis_voice_daemon"]
```

- [ ] **Step 1.1.3: Escribir `lib.rs` con módulos privados + re-exports**

`crates/jarvis_voice/src/lib.rs`:

```rust
//! `jarvis_voice` — voice engine in-process para jarvis-os.
//!
//! Encapsula la conversación con ElevenLabs Convai (audio I/O, WS, AEC,
//! resample). En B1 el `VoiceEngine` lanza el binario legacy
//! `jarvis-voice-daemon` como subprocess; en B2 el binario desaparece y
//! todo corre dentro del proceso de IronClaw.
//!
//! Superficie pública estable a partir de B1 — el comportamiento
//! interno cambia entre B1 y B2 sin tocar la API.

pub use config::VoiceConfig;
pub use engine::{VoiceEngine, VoiceHandle};
pub use error::VoiceError;
pub use types::{
    ConversationId, InterruptionReason, PcmFrame, SampleRate, ToolCallRequest, ToolCallResult,
    VoiceEvent,
};

mod config;
mod engine;
mod error;
mod spawn;
mod types;
```

- [ ] **Step 1.1.4: Escribir `types.rs` con newtypes + DTOs**

`crates/jarvis_voice/src/types.rs`:

```rust
//! Tipos públicos de `jarvis_voice`. Cumplen `.claude/rules/types.md`:
//! identifiers son newtypes con validación compartida; enums wire-stable
//! son `#[serde(rename_all = "snake_case")]`.

use crate::error::VoiceError;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct ConversationId(String);

impl ConversationId {
    fn validate(s: &str) -> Result<(), VoiceError> {
        let len = s.chars().count();
        if !(1..=128).contains(&len) {
            return Err(VoiceError::Validation(format!(
                "ConversationId length {} out of 1..=128",
                len
            )));
        }
        if !s
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Err(VoiceError::Validation(
                "ConversationId must be ASCII alphanumeric, underscore or hyphen".into(),
            ));
        }
        Ok(())
    }

    pub fn new(raw: impl Into<String>) -> Result<Self, VoiceError> {
        let s = raw.into();
        Self::validate(&s)?;
        Ok(Self(s))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

impl TryFrom<String> for ConversationId {
    type Error = VoiceError;
    fn try_from(value: String) -> Result<Self, VoiceError> {
        Self::validate(&value)?;
        Ok(Self(value))
    }
}

impl fmt::Display for ConversationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SampleRate(u32);

impl SampleRate {
    pub const ELEVENLABS: Self = SampleRate(16_000);

    pub fn new(hz: u32) -> Result<Self, VoiceError> {
        match hz {
            8_000 | 16_000 | 22_050 | 32_000 | 44_100 | 48_000 => Ok(SampleRate(hz)),
            other => Err(VoiceError::Validation(format!(
                "unsupported sample rate {other} (allowed: 8000/16000/22050/32000/44100/48000)"
            ))),
        }
    }

    pub fn hz(self) -> u32 {
        self.0
    }
}

#[derive(Debug, Clone)]
pub struct PcmFrame {
    pub samples: Arc<[i16]>,
    pub sample_rate: SampleRate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InterruptionReason {
    User,
    Server,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct ToolCallRequest {
    pub tool_call_id: String,
    pub tool_name: String,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone)]
pub struct ToolCallResult {
    pub tool_call_id: String,
    pub result: String,
    pub is_error: bool,
}

#[derive(Debug, Clone)]
pub enum VoiceEvent {
    Connected { conversation_id: ConversationId },
    Disconnected,
    UserTranscript(String),
    AgentTranscript(String),
    AgentTranscriptCorrection { original: String, corrected: String },
    Interrupted { reason: InterruptionReason },
    ToolCallRequested(ToolCallRequest),
    AgentAudio(PcmFrame),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversation_id_accepts_valid() {
        assert!(ConversationId::new("conv_abc123").is_ok());
        assert!(ConversationId::new("a").is_ok());
        assert!(ConversationId::new("a-b_c-1").is_ok());
    }

    #[test]
    fn conversation_id_rejects_invalid() {
        assert!(ConversationId::new("").is_err());
        assert!(ConversationId::new("conv with space").is_err());
        assert!(ConversationId::new("conv/slash").is_err());
        let too_long = "a".repeat(129);
        assert!(ConversationId::new(too_long).is_err());
    }

    #[test]
    fn conversation_id_serde_validates() {
        let json = r#""conv_xyz""#;
        let id: ConversationId = serde_json::from_str(json).unwrap();
        assert_eq!(id.as_str(), "conv_xyz");

        let bad = r#""bad space""#;
        assert!(serde_json::from_str::<ConversationId>(bad).is_err());
    }

    #[test]
    fn sample_rate_accepts_known() {
        assert!(SampleRate::new(16_000).is_ok());
        assert!(SampleRate::new(48_000).is_ok());
    }

    #[test]
    fn sample_rate_rejects_unknown() {
        assert!(SampleRate::new(0).is_err());
        assert!(SampleRate::new(12_345).is_err());
    }
}
```

- [ ] **Step 1.1.5: Escribir `error.rs`**

`crates/jarvis_voice/src/error.rs`:

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum VoiceError {
    #[error("voice transport error: {0}")]
    Transport(String),

    #[error("audio device error: {0}")]
    AudioDevice(String),

    #[error("AEC initialization failed: {0}")]
    AecInit(String),

    #[error("invalid voice config/value: {0}")]
    Validation(String),

    #[error("subprocess spawn failed: {0}")]
    Spawn(String),
}
```

- [ ] **Step 1.1.6: Escribir `config.rs` con `VoiceConfig::from_env`**

`crates/jarvis_voice/src/config.rs`:

```rust
//! `VoiceConfig` — única fuente de verdad de envs del voice engine.
//!
//! Hoy (B1) lee las mismas envs que el daemon legacy
//! (`ELEVENLABS_AGENT_ID`, `ELEVENLABS_API_KEY`,
//! `JARVIS_VOICE_SYSTEM_PROMPT_OVERRIDE`, `JARVIS_VOICE_VARS`). En B3 se
//! añaden envs de AEC (`JARVIS_VOICE_AEC_DELAY_MS`).

use crate::error::VoiceError;
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct VoiceConfig {
    pub agent_id: String,
    pub api_key: String,
    pub system_prompt_override: Option<String>,
    pub dynamic_variables: BTreeMap<String, String>,
    pub aec_delay_ms: u32,
}

impl VoiceConfig {
    pub fn from_env() -> Result<Self, VoiceError> {
        let agent_id = std::env::var("ELEVENLABS_AGENT_ID")
            .map_err(|_| VoiceError::Validation("ELEVENLABS_AGENT_ID not set".into()))?;
        let api_key = std::env::var("ELEVENLABS_API_KEY")
            .map_err(|_| VoiceError::Validation("ELEVENLABS_API_KEY not set".into()))?;

        if agent_id.trim().is_empty() {
            return Err(VoiceError::Validation("ELEVENLABS_AGENT_ID empty".into()));
        }
        if api_key.trim().is_empty() {
            return Err(VoiceError::Validation("ELEVENLABS_API_KEY empty".into()));
        }

        let system_prompt_override =
            std::env::var("JARVIS_VOICE_SYSTEM_PROMPT_OVERRIDE").ok();
        let dynamic_variables = parse_kv_list(
            std::env::var("JARVIS_VOICE_VARS").ok().as_deref(),
        );

        let aec_delay_ms = std::env::var("JARVIS_VOICE_AEC_DELAY_MS")
            .ok()
            .and_then(|raw| raw.trim().parse::<u32>().ok())
            .filter(|n| *n > 0 && *n <= 1_000)
            .unwrap_or(50);

        Ok(Self {
            agent_id,
            api_key,
            system_prompt_override,
            dynamic_variables,
            aec_delay_ms,
        })
    }

    pub fn agent_id_redacted(&self) -> String {
        let head: String = self.agent_id.chars().take(12).collect();
        if self.agent_id.len() > 12 {
            format!("{head}…")
        } else {
            head
        }
    }
}

fn parse_kv_list(raw: Option<&str>) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let Some(s) = raw else {
        return out;
    };
    for entry in s.split(',') {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        if let Some((k, v)) = entry.split_once('=') {
            out.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_dynamic_vars() {
        let parsed = parse_kv_list(Some("display_name=Horelvis,foo=bar"));
        assert_eq!(parsed.get("display_name").map(String::as_str), Some("Horelvis"));
        assert_eq!(parsed.get("foo").map(String::as_str), Some("bar"));
    }

    #[test]
    fn parse_handles_blanks() {
        let parsed = parse_kv_list(Some(" k=v , , ,k2=v2 "));
        assert_eq!(parsed.len(), 2);
    }

    #[test]
    fn redacts_long_agent_id() {
        let cfg = VoiceConfig {
            agent_id: "agent_abcdefghijklmnop".into(),
            api_key: "x".into(),
            system_prompt_override: None,
            dynamic_variables: BTreeMap::new(),
            aec_delay_ms: 50,
        };
        assert!(cfg.agent_id_redacted().ends_with('…'));
    }
}
```

- [ ] **Step 1.1.7: Escribir esqueleto de `engine.rs` (queda completo en 1.2)**

`crates/jarvis_voice/src/engine.rs`:

```rust
//! `VoiceEngine` — punto de entrada del crate.
//!
//! En B1 lanza `jarvis-voice-daemon` como subprocess (ver
//! [`crate::spawn`]). En B2 sustituye el subprocess por orquestador
//! in-process. La superficie pública (`VoiceEngine::start` →
//! `VoiceHandle`) es estable entre B1 y B2.

use crate::config::VoiceConfig;
use crate::error::VoiceError;
use crate::spawn::DaemonChild;
use crate::types::{ToolCallResult, VoiceEvent};
use tokio::sync::{broadcast, mpsc};

/// Capacidad del bus de eventos. Suficiente para 1s de audio en frames
/// 50ms (~20) más eventos de control. Lagged subscribers se re-sincronizan.
const EVENT_BUS_CAPACITY: usize = 64;

pub struct VoiceEngine;

impl VoiceEngine {
    /// Arranca el voice engine. En B1 lanza el subprocess
    /// `jarvis-voice-daemon` y devuelve un handle que mantiene el child
    /// vivo y permite suscribirse a `VoiceEvent` (broadcast vacío en B1
    /// porque el daemon publica PCM por IPC, no por este bus — el shim
    /// `ElevenLabsLocalBackend` sigue leyendo del IPC en B1).
    pub async fn start(cfg: VoiceConfig) -> Result<VoiceHandle, VoiceError> {
        let (events_tx, _events_rx) = broadcast::channel::<VoiceEvent>(EVENT_BUS_CAPACITY);
        let (tool_tx, _tool_rx) = mpsc::channel::<ToolCallResult>(8);
        let child = DaemonChild::spawn(&cfg).await?;

        Ok(VoiceHandle {
            events_tx,
            tool_tx,
            _child: Some(child),
        })
    }
}

pub struct VoiceHandle {
    events_tx: broadcast::Sender<VoiceEvent>,
    tool_tx: mpsc::Sender<ToolCallResult>,
    _child: Option<DaemonChild>,
}

impl VoiceHandle {
    pub fn subscribe(&self) -> broadcast::Receiver<VoiceEvent> {
        self.events_tx.subscribe()
    }

    pub async fn send_tool_result(&self, result: ToolCallResult) -> Result<(), VoiceError> {
        self.tool_tx
            .send(result)
            .await
            .map_err(|e| VoiceError::Transport(format!("tool channel closed: {e}")))
    }

    pub async fn stop(self) -> Result<(), VoiceError> {
        if let Some(child) = self._child {
            child.shutdown().await?;
        }
        Ok(())
    }
}
```

- [ ] **Step 1.1.8: Esqueleto vacío de `spawn.rs` (queda completo en 1.2)**

`crates/jarvis_voice/src/spawn.rs`:

```rust
//! **B1 only — borrado en B2.**
//!
//! Subprocess launcher para el binario `jarvis-voice-daemon` legacy. Se
//! mantiene mientras la migración al orquestador in-process está en
//! curso. La función completa se implementa en Task 1.2.

use crate::config::VoiceConfig;
use crate::error::VoiceError;

pub(crate) struct DaemonChild;

impl DaemonChild {
    pub(crate) async fn spawn(_cfg: &VoiceConfig) -> Result<Self, VoiceError> {
        Err(VoiceError::Spawn("not implemented yet — see Task 1.2".into()))
    }

    pub(crate) async fn shutdown(self) -> Result<(), VoiceError> {
        Ok(())
    }
}
```

- [ ] **Step 1.1.9: Verificar compilación**

Run:

```bash
cargo build -p jarvis_voice
cargo test -p jarvis_voice --lib
```

Expected: zero warnings, todos los tests del módulo `types` y `config` pasan.

- [ ] **Step 1.1.10: Commit**

```bash
git add crates/jarvis_voice/ Cargo.toml
git commit -m "$(cat <<'EOF'
feat(voice)[F4/B1.1]: scaffold crates/jarvis_voice

Crate nuevo con superficie pública estable: VoiceEngine, VoiceHandle,
VoiceConfig, VoiceEvent, PcmFrame, ConversationId, SampleRate.
Implementación interna pendiente (subprocess launcher en B1.2,
in-process en B2).

ConversationId y SampleRate son newtypes validados según
.claude/rules/types.md. VoiceConfig::from_env consolidará el parsing
de envs que hoy está duplicado entre el daemon y src/config/audio.rs.

Spec: docs/superpowers/specs/2026-05-02-jarvis-voice-in-process-design.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 1.2: Implementar subprocess launcher (`spawn.rs`)

**Files:**
- Modify: `crates/jarvis_voice/src/spawn.rs`
- Modify: `crates/jarvis_voice/Cargo.toml` (añadir `tokio` features ya están en "full")

- [ ] **Step 1.2.1: Escribir el test de spawn (failing)**

Edit `crates/jarvis_voice/src/spawn.rs`, añadir al final:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn cfg_for_test() -> VoiceConfig {
        VoiceConfig {
            agent_id: "agent_test".into(),
            api_key: "key_test".into(),
            system_prompt_override: None,
            dynamic_variables: Default::default(),
            aec_delay_ms: 50,
        }
    }

    #[tokio::test]
    async fn spawn_fails_when_binary_missing() {
        // Apunta a una ruta inexistente vía env, así el test no depende
        // del PATH de la máquina del CI.
        unsafe {
            std::env::set_var(
                "JARVIS_VOICE_DAEMON_BIN",
                "/this/path/does/not/exist/jarvis-voice-daemon",
            );
        }
        let cfg = cfg_for_test();
        let err = DaemonChild::spawn(&cfg).await.unwrap_err();
        assert!(
            matches!(err, VoiceError::Spawn(_)),
            "expected Spawn error, got {err:?}"
        );
        unsafe {
            std::env::remove_var("JARVIS_VOICE_DAEMON_BIN");
        }
    }
}
```

Run:

```bash
cargo test -p jarvis_voice --lib spawn::tests::spawn_fails_when_binary_missing
```

Expected: FAIL — `DaemonChild::spawn` is hardcoded to return error so it passes; pero el `Validation` error message no es el esperado. Ajustar al loop.

- [ ] **Step 1.2.2: Implementación real de `DaemonChild::spawn`**

Reemplaza `crates/jarvis_voice/src/spawn.rs` completo:

```rust
//! **B1 only — borrado en B2.**
//!
//! Subprocess launcher para `jarvis-voice-daemon`. Resuelve el binario
//! desde `JARVIS_VOICE_DAEMON_BIN` (default: `jarvis-voice-daemon` en
//! `$PATH`), exporta las envs del config (`ELEVENLABS_*`,
//! `JARVIS_VOICE_*`) heredando además el entorno actual, y devuelve un
//! `DaemonChild` que mata el proceso al `shutdown` o al drop.
//!
//! En B1 el daemon sigue siendo la fuente de verdad del audio; el
//! shim `ElevenLabsLocalBackend` recibe PCM por el canal IPC existente.
//! En B2 todo este archivo se borra.

use crate::config::VoiceConfig;
use crate::error::VoiceError;
use std::process::Stdio;
use tokio::process::{Child, Command};

const DEFAULT_BIN: &str = "jarvis-voice-daemon";

pub(crate) struct DaemonChild {
    child: Child,
}

impl DaemonChild {
    pub(crate) async fn spawn(cfg: &VoiceConfig) -> Result<Self, VoiceError> {
        let bin = std::env::var("JARVIS_VOICE_DAEMON_BIN")
            .unwrap_or_else(|_| DEFAULT_BIN.to_string());

        let mut command = Command::new(&bin);
        command
            .env("ELEVENLABS_AGENT_ID", &cfg.agent_id)
            .env("ELEVENLABS_API_KEY", &cfg.api_key)
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .kill_on_drop(true);

        if let Some(prompt) = cfg.system_prompt_override.as_deref() {
            command.env("JARVIS_VOICE_SYSTEM_PROMPT_OVERRIDE", prompt);
        }
        if !cfg.dynamic_variables.is_empty() {
            let kv: Vec<String> = cfg
                .dynamic_variables
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect();
            command.env("JARVIS_VOICE_VARS", kv.join(","));
        }

        let child = command.spawn().map_err(|e| {
            VoiceError::Spawn(format!("failed to spawn '{bin}': {e}"))
        })?;

        tracing::debug!(
            bin = %bin,
            agent = %cfg.agent_id_redacted(),
            "voice.daemon_subprocess.spawned"
        );

        Ok(Self { child })
    }

    pub(crate) async fn shutdown(mut self) -> Result<(), VoiceError> {
        // kill_on_drop(true) cubre el drop, pero pedimos terminación
        // explícita para tener un await observable. SIGKILL es fine
        // aquí porque el daemon no persiste estado relevante.
        if let Err(e) = self.child.kill().await {
            return Err(VoiceError::Spawn(format!("kill voice daemon: {e}")));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg_for_test() -> VoiceConfig {
        VoiceConfig {
            agent_id: "agent_test".into(),
            api_key: "key_test".into(),
            system_prompt_override: None,
            dynamic_variables: Default::default(),
            aec_delay_ms: 50,
        }
    }

    #[tokio::test]
    async fn spawn_fails_when_binary_missing() {
        unsafe {
            std::env::set_var(
                "JARVIS_VOICE_DAEMON_BIN",
                "/this/path/does/not/exist/jarvis-voice-daemon",
            );
        }
        let cfg = cfg_for_test();
        let err = DaemonChild::spawn(&cfg).await.unwrap_err();
        assert!(
            matches!(err, VoiceError::Spawn(_)),
            "expected Spawn error, got {err:?}"
        );
        unsafe {
            std::env::remove_var("JARVIS_VOICE_DAEMON_BIN");
        }
    }

    #[tokio::test]
    async fn spawn_succeeds_with_dummy_binary() {
        // /usr/bin/true existe en linux, vive un instante y exit 0. Sirve
        // como stand-in del daemon: spawn debe tener éxito.
        unsafe {
            std::env::set_var("JARVIS_VOICE_DAEMON_BIN", "/usr/bin/true");
        }
        let cfg = cfg_for_test();
        let child = DaemonChild::spawn(&cfg)
            .await
            .expect("spawn must succeed against /usr/bin/true");
        child.shutdown().await.expect("shutdown must succeed");
        unsafe {
            std::env::remove_var("JARVIS_VOICE_DAEMON_BIN");
        }
    }
}
```

- [ ] **Step 1.2.3: Run tests**

```bash
cargo test -p jarvis_voice --lib
```

Expected: PASS (incluye `spawn_fails_when_binary_missing` y `spawn_succeeds_with_dummy_binary`).

- [ ] **Step 1.2.4: Commit**

```bash
git add crates/jarvis_voice/src/spawn.rs
git commit -m "$(cat <<'EOF'
feat(voice)[F4/B1.2]: implement subprocess launcher placeholder

DaemonChild::spawn lanza jarvis-voice-daemon como child de IronClaw,
heredando envs y matando al drop. Throwaway: el archivo entero se borra
en B2 cuando el orquestador in-process reemplaza al subprocess.

Tests cubren spawn-fails-when-binary-missing y
spawn-succeeds-with-dummy-binary (/usr/bin/true).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 1.3: Shim `ElevenLabsLocalBackend` en core

**Files:**
- Create: `src/audio/backends/elevenlabs_local.rs`
- Modify: `src/audio/backends/mod.rs`
- Modify: `src/audio/tts.rs` (add `ElevenlabsLocal` variant)
- Modify: `Cargo.toml` raíz (`[dependencies] jarvis_voice = { path = "crates/jarvis_voice" }`)

- [ ] **Step 1.3.1: Añadir dep al binario raíz**

Edit raíz `Cargo.toml`. Bajo la sección `[dependencies]` del crate `ironclaw` (no del workspace), añadir:

```toml
jarvis_voice = { path = "crates/jarvis_voice" }
```

Verifica con:

```bash
grep -n "jarvis_voice" Cargo.toml
```

- [ ] **Step 1.3.2: Añadir variant `ElevenlabsLocal` al enum**

Edit `src/audio/tts.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TtsBackendKind {
    None,
    /// Voice daemon legacy bridging ElevenLabs Convai over local UNIX
    /// socket IPC. Ruta de producción hasta que el feature flag
    /// `voice-in-process` cierre.
    ElevenlabsIpc,
    /// `ElevenLabsLocalBackend` — voice engine in-process (B1: lanza el
    /// daemon como subprocess; B2: orquestador in-process puro).
    ElevenlabsLocal,
}

impl TtsBackendKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::ElevenlabsIpc => "elevenlabs_ipc",
            Self::ElevenlabsLocal => "elevenlabs_local",
        }
    }
}
```

- [ ] **Step 1.3.3: Implementar el shim**

`src/audio/backends/elevenlabs_local.rs`:

```rust
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
            .map_err(|e| ConfigError::Invalid(format!("voice config: {e}")))?;
        let handle = VoiceEngine::start(cfg)
            .await
            .map_err(|e| ConfigError::Invalid(format!("voice engine start: {e}")))?;
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
```

> NOTA: si `ConfigError` no tiene un variant `Invalid(String)`, abre `src/error.rs` y añade `#[error("invalid config: {0}")] Invalid(String),` antes de continuar.

- [ ] **Step 1.3.4: Re-exportar el shim**

Edit `src/audio/backends/mod.rs`:

```rust
//! Concrete `TtsBackend` implementations.
//!
//! `none` (TTS disabled), `elevenlabs_ipc` (legacy daemon over local
//! UNIX socket — borrado en F4/B4), y `elevenlabs_local` (voice engine
//! in-process via `jarvis_voice` crate — ruta única tras F4/B2).

pub mod elevenlabs_ipc;
pub mod elevenlabs_local;
pub mod none;

pub use elevenlabs_ipc::ElevenLabsIpcBackend;
pub use elevenlabs_local::ElevenLabsLocalBackend;
pub use none::NoneBackend;
```

- [ ] **Step 1.3.5: Test del shim**

Añade al final de `src/audio/backends/elevenlabs_local.rs`:

```rust
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
        // Construye sin pasar por VoiceEngine::start (no queremos lanzar
        // un subprocess en este test).
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

    /// Helper para construir un `VoiceHandle` sin lanzar subprocess —
    /// usa el dummy `/usr/bin/true` igual que el test del crate.
    async fn make_dummy_handle() -> VoiceHandle {
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
```

- [ ] **Step 1.3.6: Compile + test**

```bash
cargo build
cargo test --lib audio::backends::elevenlabs_local
```

Expected: tests verdes.

- [ ] **Step 1.3.7: Commit**

```bash
git add Cargo.toml src/audio/backends/ src/audio/tts.rs src/error.rs
git commit -m "$(cat <<'EOF'
feat(voice)[F4/B1.3]: ElevenLabsLocalBackend shim wrapping VoiceEngine

Nuevo TtsBackend que arranca jarvis_voice::VoiceEngine y delega la
recepción de PCM al ElevenLabsIpcBackend interno. En B1 el voice engine
lanza el daemon legacy como subprocess; el wire de PCM sigue siendo IPC.
En B2 la implementación interna cambia a VoiceEvent::AgentAudio y el
ElevenLabsIpcBackend desaparece.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 1.4: Wire del shim detrás de feature flag `voice-in-process`

**Files:**
- Modify: `Cargo.toml` (añadir `[features]` si no existe)
- Modify: `src/main.rs` (extender `match config.audio.tts_backend`)
- Modify: `src/config/audio.rs` (parser reconoce `elevenlabs_local`)

- [ ] **Step 1.4.1: Añadir feature flag**

Edit `Cargo.toml` raíz, en la tabla `[features]`:

```toml
[features]
default = []
voice-in-process = []
```

Si el bloque `[features]` ya existe, simplemente añadir `voice-in-process = []`.

- [ ] **Step 1.4.2: Reconocer `elevenlabs_local` en el parser de config**

Edit `src/config/audio.rs::parse_backend`:

```rust
fn parse_backend(raw: &str) -> TtsBackendKind {
    match raw.trim().to_ascii_lowercase().as_str() {
        "" | "none" | "off" | "false" | "0" | "disabled" => TtsBackendKind::None,
        "elevenlabs_ipc" | "elevenlabs-ipc" | "elevenlabs" => TtsBackendKind::ElevenlabsIpc,
        "elevenlabs_local" | "elevenlabs-local" | "voice_in_process" => {
            TtsBackendKind::ElevenlabsLocal
        }
        other => {
            tracing::warn!(
                value = other,
                "Unknown JARVIS_TTS_BACKEND value, falling back to 'none'"
            );
            TtsBackendKind::None
        }
    }
}
```

Y añade un test en el `mod tests`:

```rust
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
}
```

- [ ] **Step 1.4.3: Switch en `main.rs`**

Edit `src/main.rs` alrededor de línea 1067 — el bloque `let tts_backend = match config.audio.tts_backend { ... }`:

```rust
let tts_backend: Option<Arc<dyn ironclaw::audio::TtsBackend + Send + Sync>> =
    match config.audio.tts_backend {
        ironclaw::audio::TtsBackendKind::ElevenlabsIpc => {
            let backend = Arc::new(
                ironclaw::audio::backends::ElevenLabsIpcBackend::new(
                    config.audio.frame_buffer,
                ),
            );
            let _pipeline_handle = ironclaw::audio::TtsAudioPipeline::spawn(
                backend.clone(),
                Arc::clone(&sse_for_local),
            );
            tracing::info!(
                backend = "elevenlabs_ipc",
                frame_buffer = config.audio.frame_buffer,
                "tts audio pipeline started"
            );
            // Compat con local_ipc::create — necesita el Arc concreto.
            ipc_tts_backend = Some(backend.clone());
            Some(backend as Arc<dyn ironclaw::audio::TtsBackend + Send + Sync>)
        }
        ironclaw::audio::TtsBackendKind::ElevenlabsLocal => {
            let backend = Arc::new(
                ironclaw::audio::backends::ElevenLabsLocalBackend::start(
                    config.audio.frame_buffer,
                )
                .await
                .map_err(|e| anyhow::anyhow!("elevenlabs_local backend: {e}"))?,
            );
            let _pipeline_handle = ironclaw::audio::TtsAudioPipeline::spawn(
                backend.clone(),
                Arc::clone(&sse_for_local),
            );
            tracing::info!(
                backend = "elevenlabs_local",
                frame_buffer = config.audio.frame_buffer,
                "tts audio pipeline started"
            );
            // En B1 el shim sigue exponiendo su ElevenLabsIpcBackend
            // interno para que local_ipc::create reciba PCM por IPC.
            ipc_tts_backend = Some(backend.ipc_backend());
            Some(backend as Arc<dyn ironclaw::audio::TtsBackend + Send + Sync>)
        }
        ironclaw::audio::TtsBackendKind::None => {
            ipc_tts_backend = None;
            None
        }
    };
```

> NOTA: introduce `let mut ipc_tts_backend: Option<Arc<ironclaw::audio::backends::ElevenLabsIpcBackend>> = None;` antes del match, y pásalo después al `local_ipc::create(... ipc_tts_backend)` reemplazando el `tts_backend` que iba antes. Conserva el comportamiento actual: solo `ElevenlabsIpc` y `ElevenlabsLocal` populan ese parámetro.

- [ ] **Step 1.4.4: `cargo build` y `cargo build --features voice-in-process`**

```bash
cargo build
cargo build --features voice-in-process
cargo clippy --all --benches --tests --examples --all-features
```

Expected: zero warnings.

- [ ] **Step 1.4.5: Validación Asus (manual, M2 invariant)**

En la máquina Asus, con `~/.ironclaw/.env` apuntando a `JARVIS_TTS_BACKEND=elevenlabs_local`:

```bash
systemctl --user stop jarvis-voice-daemon
RUST_LOG=ironclaw=info,jarvis_voice=debug cargo run --features voice-in-process --release
```

Expected:
- ironclaw arranca
- `voice.daemon_subprocess.spawned` aparece en logs
- conversación funciona (mic → respuesta TTS audible, orbe reacciona)
- `ps aux | grep jarvis-voice-daemon` muestra el proceso como hijo de `ironclaw` (PPID = ironclaw)

Si la validación falla, debug y NO commitear.

- [ ] **Step 1.4.6: Commit**

```bash
git add Cargo.toml src/main.rs src/config/audio.rs
git commit -m "$(cat <<'EOF'
feat(voice)[F4/B1.4]: wire ElevenLabsLocalBackend behind voice-in-process flag

JARVIS_TTS_BACKEND=elevenlabs_local (con cargo feature voice-in-process)
arranca el voice engine in-process (B1: subprocess wrapper). systemd
unit jarvis-voice-daemon.service ya no necesita estar activo. Path
default sigue siendo elevenlabs_ipc para no romper deployments existentes
hasta B2.

Validación Asus: stop jarvis-voice-daemon + start ironclaw
--features voice-in-process → conversación end-to-end OK.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

# B2 — `feat(voice)[F4/B2]: in-process cpal + ElevenLabs WS`

**Estado al final de B2:** `VoiceEngine` ya no spawnea proceso. Hace cpal + WS in-process. AEC todavía es de PipeWire (no tocamos `echo-cancel.conf` aún). El crate `jarvis_voice_daemon` se borra completo, igual que `spawn.rs`, `elevenlabs_ipc.rs` y la systemd unit. La feature flag `voice-in-process` se borra (path único).

> ⚠️ **Antes de empezar B2**, asegurarse de que B1 se validó en Asus. B2 es el commit de mayor blast radius del plan.

---

### Task 2.1: Mover audio I/O al crate

**Files:**
- Create: `crates/jarvis_voice/src/audio_io/mod.rs` (cuerpo de `daemon/src/audio.rs`)
- Modify: `crates/jarvis_voice/Cargo.toml` (añadir `cpal`, `ringbuf`)
- Modify: `crates/jarvis_voice/src/lib.rs` (declarar `mod audio_io;`)

- [ ] **Step 2.1.1: Añadir deps de audio al crate**

Edit `crates/jarvis_voice/Cargo.toml` `[dependencies]`:

```toml
cpal = "0.15"
ringbuf = "0.4"
```

- [ ] **Step 2.1.2: Mover `daemon/audio.rs`**

```bash
mkdir -p crates/jarvis_voice/src/audio_io
git mv crates/jarvis_voice_daemon/src/audio.rs crates/jarvis_voice/src/audio_io/mod.rs
```

> Nota: `git mv` mantiene la historia (review-friendly). El archivo está intacto en este paso.

- [ ] **Step 2.1.3: Hacer público lo que el orquestador necesita**

Edit `crates/jarvis_voice/src/audio_io/mod.rs`:
- Cambiar visibilidad de `pub fn start() -> Result<AudioIo>` para que sea visible desde el módulo padre (ya es `pub`).
- Cambiar las constantes `pub const SAMPLE_RATE` y `pub const CHUNK_SAMPLES` a re-exportadas (ya están).
- Cambiar `use anyhow::{Context, Result, anyhow}` por `use crate::error::VoiceError; use anyhow::Context;` y propagar `Result<_, VoiceError>` desde `start()`. Los `anyhow!` internos cambian a `VoiceError::AudioDevice(format!(...))`.

Reescribe `start` y `pick_*_device` para devolver `Result<_, VoiceError>`. Búsqueda mecánica:

```bash
sed -i 's/Result<AudioIo>/Result<AudioIo, VoiceError>/' crates/jarvis_voice/src/audio_io/mod.rs
```

Y a mano: cambiar cada `anyhow!("...")` a `VoiceError::AudioDevice("...".into())` y cada `.context("...")` a `.map_err(|e| VoiceError::AudioDevice(format!("...: {e}")))`.

- [ ] **Step 2.1.4: Declarar el módulo en `lib.rs`**

Edit `crates/jarvis_voice/src/lib.rs`:

```rust
mod audio_io;
mod config;
mod elevenlabs;     // creado en Task 2.2
mod engine;
mod error;
mod orchestrator;   // creado en Task 2.3
mod types;
```

Borra `mod spawn;` (lo borraremos en 2.7, queda momentáneamente sin referencia → cambia `engine.rs` para no usarlo en 2.4).

- [ ] **Step 2.1.5: `cargo build -p jarvis_voice`**

Expected: PASS (todavía sin orchestrator/elevenlabs reales — sólo audio_io declarado).

> Si falla por imports faltantes que esperaban estar en el daemon, mover/ajustar al estilo `crate::audio_io::...`.

- [ ] **Step 2.1.6: Commit**

```bash
git add crates/jarvis_voice/ crates/jarvis_voice_daemon/
git commit -m "$(cat <<'EOF'
feat(voice)[F4/B2.1]: move cpal audio I/O into crates/jarvis_voice

git mv del audio.rs del daemon legacy al nuevo crate, ajustando errores
a VoiceError::AudioDevice. Sin cambio funcional. El daemon queda sin
audio.rs y NO compila — se borra entero en Task 2.7.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 2.2: Mover WS client + protocol al crate

**Files:**
- Create: `crates/jarvis_voice/src/elevenlabs/mod.rs` (ex `daemon/ws_client.rs`)
- Create: `crates/jarvis_voice/src/elevenlabs/protocol.rs` (ex `daemon/protocol.rs`)
- Modify: `crates/jarvis_voice/Cargo.toml` (añadir `tokio-tungstenite`, `futures-util`, `base64`)

- [ ] **Step 2.2.1: Añadir deps WS al crate**

Edit `crates/jarvis_voice/Cargo.toml` `[dependencies]`:

```toml
tokio-tungstenite = { version = "0.24", features = ["rustls-tls-webpki-roots"] }
futures-util = "0.3"
base64 = "0.22"
```

- [ ] **Step 2.2.2: Mover protocol.rs**

```bash
mkdir -p crates/jarvis_voice/src/elevenlabs
git mv crates/jarvis_voice_daemon/src/protocol.rs crates/jarvis_voice/src/elevenlabs/protocol.rs
```

- [ ] **Step 2.2.3: Mover ws_client.rs**

```bash
git mv crates/jarvis_voice_daemon/src/ws_client.rs crates/jarvis_voice/src/elevenlabs/mod.rs
```

- [ ] **Step 2.2.4: Ajustar imports**

Edit `crates/jarvis_voice/src/elevenlabs/mod.rs`:
- Cambiar `use crate::config::Config` por `use crate::config::VoiceConfig` y todos los usos de `cfg.agent_id`/`cfg.api_key` siguen igual (los campos coinciden).
- Cambiar `use crate::protocol::{...}` por `use crate::elevenlabs::protocol::{...}`.
- Cambiar `Result<...>` (anyhow) por `Result<..., VoiceError>` para la API pública (`connect`).

Edit `crates/jarvis_voice/src/elevenlabs/protocol.rs`: nada que cambiar; el archivo solo declara DTOs.

- [ ] **Step 2.2.5: Compilar y commit**

```bash
cargo build -p jarvis_voice
```

Expected: PASS (audio_io + elevenlabs todavía sin pegar — orchestrator viene en 2.3).

```bash
git add crates/jarvis_voice/ crates/jarvis_voice_daemon/
git commit -m "$(cat <<'EOF'
feat(voice)[F4/B2.2]: move ElevenLabs WS client + protocol into crates/jarvis_voice

git mv de ws_client.rs y protocol.rs. Ajustes: use crate::config::VoiceConfig,
use crate::elevenlabs::protocol, Result<_, VoiceError> en la API pública.
Sin cambio funcional.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 2.3: Reescribir orchestrator → emite `VoiceEvent` por broadcast

**Files:**
- Create: `crates/jarvis_voice/src/orchestrator.rs`

- [ ] **Step 2.3.1: Test del orchestrator (failing)**

Crear `crates/jarvis_voice/src/orchestrator.rs` con un test inicial que asegure el contrato (ignorado, requiere envs reales):

```rust
//! Orquestador in-process.
//!
//! Reemplaza `jarvis_voice_daemon::orchestrator::run`. Tie audio_io + WS
//! y emite `VoiceEvent` por un broadcast::Sender<VoiceEvent> que provee
//! `VoiceEngine::start`. La diferencia clave con el daemon legacy es
//! que NO publica por IPC — el shim `ElevenLabsLocalBackend` se
//! suscribe directamente al broadcast.

use crate::audio_io::{self, AudioIo};
use crate::config::VoiceConfig;
use crate::elevenlabs::{
    self,
    protocol::{ClientToolCall, ClientToolResult},
    Inbound, Outbound,
};
use crate::error::VoiceError;
use crate::types::{
    ConversationId, InterruptionReason, PcmFrame, SampleRate, ToolCallRequest, ToolCallResult,
    VoiceEvent,
};
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc};

/// Frame size hint para el broadcast: 50 ms @ 16 kHz = 800 samples
/// mono. Lo usa el shim en core para sizing del pipeline downstream.
pub(crate) const FRAME_SAMPLES_HINT: usize = 800;

pub(crate) struct OrchestratorTask {
    pub events_tx: broadcast::Sender<VoiceEvent>,
    pub tool_rx: mpsc::Receiver<ToolCallResult>,
    pub stop_rx: mpsc::Receiver<()>,
}

impl OrchestratorTask {
    pub async fn run(mut self, cfg: VoiceConfig) -> Result<(), VoiceError> {
        let AudioIo {
            mut mic_rx,
            speaker_tx,
            ..
        } = audio_io::start()?;

        let mut ws = elevenlabs::connect(&cfg).await?;
        let outbound_tx = ws.outbound_tx.clone();

        // Forwarder mic → ws outbound.
        let outbound_for_mic = outbound_tx.clone();
        tokio::spawn(async move {
            while let Some(chunk) = mic_rx.recv().await {
                if outbound_for_mic
                    .send(Outbound::Audio(chunk))
                    .await
                    .is_err()
                {
                    tracing::debug!("orchestrator.mic_outbound_channel_closed");
                    break;
                }
            }
        });

        loop {
            tokio::select! {
                _ = self.stop_rx.recv() => {
                    tracing::debug!("orchestrator.stop_signal_received");
                    let _ = outbound_tx.send(Outbound::Stop).await;
                    break;
                }
                Some(result) = self.tool_rx.recv() => {
                    let _ = outbound_tx.send(Outbound::ToolResult(ClientToolResult {
                        kind: "client_tool_result",
                        tool_call_id: result.tool_call_id,
                        result: result.result,
                        is_error: result.is_error,
                    })).await;
                }
                evt = ws.inbound_rx.recv() => {
                    match evt {
                        Some(Inbound::AgentAudio(pcm)) => {
                            let frame = PcmFrame {
                                samples: Arc::from(pcm.clone().into_boxed_slice()),
                                sample_rate: SampleRate::ELEVENLABS,
                            };
                            // silent-ok: orb is decorative, lagged subscribers re-sync
                            let _ = self.events_tx.send(VoiceEvent::AgentAudio(frame));
                            speaker_tx.play(pcm);
                        }
                        Some(Inbound::UserTranscript(text)) => {
                            let _ = self.events_tx.send(VoiceEvent::UserTranscript(text));
                        }
                        Some(Inbound::AgentResponse(text)) => {
                            let _ = self.events_tx.send(VoiceEvent::AgentTranscript(text));
                        }
                        Some(Inbound::AgentResponseCorrection { original, corrected }) => {
                            let _ = self.events_tx.send(VoiceEvent::AgentTranscriptCorrection {
                                original,
                                corrected,
                            });
                        }
                        Some(Inbound::Interruption { reason, .. }) => {
                            speaker_tx.flush();
                            let r = match reason.as_deref() {
                                Some("user") => InterruptionReason::User,
                                Some("server") => InterruptionReason::Server,
                                _ => InterruptionReason::Unknown,
                            };
                            let _ = self.events_tx.send(VoiceEvent::Interrupted { reason: r });
                        }
                        Some(Inbound::Ping { event_id }) => {
                            let _ = outbound_tx.send(Outbound::Pong { event_id }).await;
                        }
                        Some(Inbound::ToolCall(call)) => {
                            let _ = self.events_tx.send(VoiceEvent::ToolCallRequested(
                                ToolCallRequest {
                                    tool_call_id: call.tool_call_id.clone(),
                                    tool_name: call.tool_name.clone(),
                                    parameters: call.parameters.clone(),
                                },
                            ));
                            // Placeholder mientras F5 no cabledea tools — devuelve mensaje
                            // informativo para que el agente pueda continuar sin colgarse.
                            let result = placeholder_tool_result(call);
                            let _ = outbound_tx.send(Outbound::ToolResult(result)).await;
                        }
                        Some(Inbound::Connected { conversation_id }) => {
                            match ConversationId::new(conversation_id) {
                                Ok(id) => {
                                    let _ = self
                                        .events_tx
                                        .send(VoiceEvent::Connected { conversation_id: id });
                                }
                                Err(e) => {
                                    tracing::warn!(error = %e, "orchestrator.invalid_conversation_id");
                                }
                            }
                        }
                        Some(Inbound::Disconnected) => {
                            let _ = self.events_tx.send(VoiceEvent::Disconnected);
                            break;
                        }
                        None => break,
                    }
                }
            }
        }

        Ok(())
    }
}

fn placeholder_tool_result(call: ClientToolCall) -> ClientToolResult {
    ClientToolResult::ok(
        call.tool_call_id,
        format!(
            "Tool '{}' aún no cabledada en jarvis-os. Cableado a IronClaw planificado para F5.",
            call.tool_name
        ),
    )
}
```

> Nota — la conversión `Arc::from(pcm.into_boxed_slice())` es obligatoria para que el `PcmFrame` del crate (`Arc<[i16]>`) no copie los samples. `pcm.clone()` antes del `Arc::from` es imprescindible porque también se pasa por valor a `speaker_tx.play(pcm)`.

- [ ] **Step 2.3.2: Compilar**

```bash
cargo build -p jarvis_voice
```

Expected: PASS.

- [ ] **Step 2.3.3: Commit**

```bash
git add crates/jarvis_voice/src/orchestrator.rs
git commit -m "$(cat <<'EOF'
feat(voice)[F4/B2.3]: orchestrator emits VoiceEvent over broadcast

Reescritura del orquestador para emitir VoiceEvent (Connected,
AgentAudio, UserTranscript, etc.) por un broadcast::Sender en lugar de
publicar PCM por IPC. Tool calls siguen con el placeholder mientras F5
no cabledea ToolDispatcher.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 2.4: `VoiceEngine::start` arranca orchestrator in-process

**Files:**
- Modify: `crates/jarvis_voice/src/engine.rs`
- Delete: `crates/jarvis_voice/src/spawn.rs`

- [ ] **Step 2.4.1: Reescribir `engine.rs`**

```rust
//! `VoiceEngine` — punto de entrada del crate.
//!
//! `start(cfg)` arranca el orquestador in-process como tokio task y
//! devuelve un `VoiceHandle` que permite suscribirse al stream de
//! `VoiceEvent`, enviar `ToolCallResult` de vuelta al server, y parar
//! el motor (drop o `stop().await`).

use crate::config::VoiceConfig;
use crate::error::VoiceError;
use crate::orchestrator::OrchestratorTask;
use crate::types::{ToolCallResult, VoiceEvent};
use tokio::sync::{broadcast, mpsc};
use tokio::task::JoinHandle;

const EVENT_BUS_CAPACITY: usize = 128;
const TOOL_CHANNEL_CAPACITY: usize = 8;

pub struct VoiceEngine;

impl VoiceEngine {
    pub async fn start(cfg: VoiceConfig) -> Result<VoiceHandle, VoiceError> {
        let (events_tx, _) = broadcast::channel::<VoiceEvent>(EVENT_BUS_CAPACITY);
        let (tool_tx, tool_rx) = mpsc::channel::<ToolCallResult>(TOOL_CHANNEL_CAPACITY);
        let (stop_tx, stop_rx) = mpsc::channel::<()>(1);

        let orchestrator = OrchestratorTask {
            events_tx: events_tx.clone(),
            tool_rx,
            stop_rx,
        };
        let join: JoinHandle<Result<(), VoiceError>> =
            tokio::spawn(async move { orchestrator.run(cfg).await });

        Ok(VoiceHandle {
            events_tx,
            tool_tx,
            stop_tx,
            join: Some(join),
        })
    }
}

pub struct VoiceHandle {
    events_tx: broadcast::Sender<VoiceEvent>,
    tool_tx: mpsc::Sender<ToolCallResult>,
    stop_tx: mpsc::Sender<()>,
    join: Option<JoinHandle<Result<(), VoiceError>>>,
}

impl VoiceHandle {
    pub fn subscribe(&self) -> broadcast::Receiver<VoiceEvent> {
        self.events_tx.subscribe()
    }

    pub async fn send_tool_result(&self, result: ToolCallResult) -> Result<(), VoiceError> {
        self.tool_tx
            .send(result)
            .await
            .map_err(|e| VoiceError::Transport(format!("tool channel closed: {e}")))
    }

    pub async fn stop(mut self) -> Result<(), VoiceError> {
        let _ = self.stop_tx.send(()).await;
        if let Some(join) = self.join.take() {
            match join.await {
                Ok(Ok(())) => Ok(()),
                Ok(Err(e)) => Err(e),
                Err(e) => Err(VoiceError::Transport(format!("orchestrator panicked: {e}"))),
            }
        } else {
            Ok(())
        }
    }
}

impl Drop for VoiceHandle {
    fn drop(&mut self) {
        // Best-effort: avisa al orquestador que pare. La task se cancela
        // sola al cerrarse el broadcast/mpsc; no hacemos await aquí
        // porque drop es sync.
        let _ = self.stop_tx.try_send(());
    }
}
```

- [ ] **Step 2.4.2: Borrar `spawn.rs` y la línea `mod spawn;` de `lib.rs`**

```bash
git rm crates/jarvis_voice/src/spawn.rs
```

Verifica `crates/jarvis_voice/src/lib.rs` no tiene `mod spawn;`.

- [ ] **Step 2.4.3: Build**

```bash
cargo build -p jarvis_voice
cargo test -p jarvis_voice --lib
```

Expected: PASS. Tests del módulo `spawn` ya no existen — los del módulo `types`/`config` siguen verdes.

- [ ] **Step 2.4.4: Commit**

```bash
git add crates/jarvis_voice/
git commit -m "$(cat <<'EOF'
feat(voice)[F4/B2.4]: VoiceEngine::start spawns orchestrator in-process

Borrado de spawn.rs (subprocess launcher de B1). VoiceEngine::start ahora
arranca OrchestratorTask como tokio::spawn y devuelve un VoiceHandle con
subscribe / send_tool_result / stop. Drop pide stop best-effort.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 2.5: `ElevenLabsLocalBackend` consume `VoiceEvent::AgentAudio`

**Files:**
- Modify: `src/audio/backends/elevenlabs_local.rs`

- [ ] **Step 2.5.1: Reescribir el shim**

```rust
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
use jarvis_voice::{PcmFrame as VoicePcmFrame, VoiceConfig, VoiceEngine, VoiceEvent, VoiceHandle};
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
            .map_err(|e| ConfigError::Invalid(format!("voice config: {e}")))?;
        let handle = VoiceEngine::start(cfg)
            .await
            .map_err(|e| ConfigError::Invalid(format!("voice engine start: {e}")))?;
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
    use tokio_stream::StreamExt;

    /// Verifica el bridge a nivel de helper puro.
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
```

> NOTA crítica (regla `Test Through the Caller`): `into_core_frame` se invoca dentro de un `tokio::spawn` que no es trivial de testear. La cobertura caller-level adicional vendrá implícita por el test del orchestrator que se añade en Task 2.6.

- [ ] **Step 2.5.2: `cargo build` y `cargo test --lib audio::backends`**

Expected: PASS.

- [ ] **Step 2.5.3: Commit**

```bash
git add src/audio/backends/elevenlabs_local.rs
git commit -m "$(cat <<'EOF'
feat(voice)[F4/B2.5]: ElevenLabsLocalBackend consumes VoiceEvent::AgentAudio

Eliminado el wrap del ElevenLabsIpcBackend interno. El shim ahora
arranca VoiceEngine in-process, se suscribe al VoiceEvent broadcast, y
traduce AgentAudio → core::audio::PcmFrame para el TtsAudioPipeline.

ipc_backend() desaparece — local_ipc::create deja de necesitar
parámetro tts_backend (limpieza completa en B4).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 2.6: Borrar feature flag y deuda IPC en `main.rs`

**Files:**
- Modify: `Cargo.toml` (quitar `voice-in-process` feature)
- Modify: `src/main.rs` (quitar branch `ElevenlabsIpc` del match — pasa a B4)
- Modify: `src/audio/tts.rs` (DEPRECAR `ElevenlabsIpc` — pero se borra completo en B4 cuando el variant deja de tener consumidores)

- [ ] **Step 2.6.1: Borrar feature flag**

Edit `Cargo.toml`: quitar la línea `voice-in-process = []` del `[features]` (si era el único entry, quitar todo el bloque o dejar `default = []`).

- [ ] **Step 2.6.2: `main.rs` — `ElevenlabsLocal` es el path único**

Edit `src/main.rs`. El match queda:

```rust
let tts_backend: Option<Arc<dyn ironclaw::audio::TtsBackend + Send + Sync>> =
    match config.audio.tts_backend {
        ironclaw::audio::TtsBackendKind::ElevenlabsIpc
        | ironclaw::audio::TtsBackendKind::ElevenlabsLocal => {
            let backend = Arc::new(
                ironclaw::audio::backends::ElevenLabsLocalBackend::start(
                    config.audio.frame_buffer,
                )
                .await
                .map_err(|e| anyhow::anyhow!("elevenlabs_local backend: {e}"))?,
            );
            let _pipeline_handle = ironclaw::audio::TtsAudioPipeline::spawn(
                backend.clone(),
                Arc::clone(&sse_for_local),
            );
            tracing::info!(
                backend = "elevenlabs_local",
                frame_buffer = config.audio.frame_buffer,
                "tts audio pipeline started"
            );
            ipc_tts_backend = None;
            Some(backend as Arc<dyn ironclaw::audio::TtsBackend + Send + Sync>)
        }
        ironclaw::audio::TtsBackendKind::None => {
            ipc_tts_backend = None;
            None
        }
    };
```

> NOTA: `ElevenlabsIpc` y `ElevenlabsLocal` ahora hacen lo mismo (`ElevenLabsLocalBackend`). El alias se mantiene por compat con los `.env` de usuarios. La eliminación final del variant está en B4. `ipc_tts_backend` sigue declarado para no romper la firma de `local_ipc::create` — lo borraremos en B4.

- [ ] **Step 2.6.3: Borrar `arch/templates/ironclaw.env` referencias al daemon**

Edit `arch/templates/ironclaw.env`: cambiar comentario "Usado por crates/jarvis_voice_daemon. La key se lee también por el unit `jarvis-voice-daemon.service` vía EnvironmentFile=." → "Usado por crates/jarvis_voice in-process dentro de ironclaw."

- [ ] **Step 2.6.4: Build**

```bash
cargo clippy --all --benches --tests --examples --all-features
cargo test --lib
```

Expected: zero warnings, todos los tests pasan.

- [ ] **Step 2.6.5: Commit**

```bash
git add Cargo.toml src/main.rs arch/templates/ironclaw.env
git commit -m "$(cat <<'EOF'
feat(voice)[F4/B2.6]: drop voice-in-process feature flag

ElevenlabsLocal es el path único; ElevenlabsIpc en JARVIS_TTS_BACKEND
sigue aceptado como alias por compat con .env existentes — el variant
del enum se borra en B4. ironclaw.env actualizado.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 2.7: Borrar `crates/jarvis_voice_daemon/`, systemd unit y referencias en scripts

**Files:**
- Delete: `crates/jarvis_voice_daemon/` (todo el directorio)
- Delete: `arch/systemd-user/jarvis-voice-daemon.service`
- Modify: `arch/install.sh`, `arch/update.sh`
- Modify: `Cargo.toml` workspace `members`

- [ ] **Step 2.7.1: Borrar el crate del workspace**

```bash
git rm -r crates/jarvis_voice_daemon/
```

Edit raíz `Cargo.toml`: quitar `"crates/jarvis_voice_daemon"` del array `members`.

- [ ] **Step 2.7.2: Borrar systemd unit**

```bash
git rm arch/systemd-user/jarvis-voice-daemon.service
```

- [ ] **Step 2.7.3: Limpiar `arch/install.sh`**

Edit `arch/install.sh`:
- Borrar línea 91: `cargo build --release -p jarvis_voice_daemon --bin jarvis-voice-daemon`.
- Cambiar línea 88 `log "Compilando ironclaw + jarvis_voice_daemon (release)..."` → `log "Compilando ironclaw (release)..."`.
- Cambiar línea 93 `for bin in ironclaw jarvis-voice-daemon; do` → `for bin in ironclaw; do` (entonces el loop instala solo `ironclaw`).
- Borrar el bloque que menciona el systemd unit del daemon (busca `jarvis-voice-daemon.service`).

- [ ] **Step 2.7.4: Limpiar `arch/update.sh`**

Edit `arch/update.sh`:
- Líneas 52, 59, 182-185: borrar todas las referencias a `jarvis_voice_daemon` y `jarvis-voice-daemon` (build, install y restart). El script queda haciendo solo el binario `ironclaw`.

- [ ] **Step 2.7.5: Verificar no hay restos**

```bash
grep -rn "jarvis_voice_daemon\|jarvis-voice-daemon" --exclude-dir=target . || true
```

Expected: solo matches en docs (specs/plans/memory) y este plan, no en código/scripts.

- [ ] **Step 2.7.6: Build clean**

```bash
cargo clean -p jarvis_voice_daemon 2>/dev/null || true
cargo clippy --all --benches --tests --examples --all-features
cargo test --lib
```

Expected: zero warnings, tests verdes.

- [ ] **Step 2.7.7: Validación Asus (manual)**

```bash
# en Asus
systemctl --user disable --now jarvis-voice-daemon.service 2>/dev/null || true
systemctl --user reset-failed jarvis-voice-daemon.service 2>/dev/null || true
# fresh install:
cd ~/git/jarvis-os && bash arch/install.sh
systemctl --user restart ironclaw
journalctl --user -u ironclaw -f &
# validar conversación end-to-end. Speakerphone abierto → bug del feedback
# loop SIGUE existiendo (lo arreglamos en B3). Lo que NO debe pasar:
# - "jarvis-voice-daemon" en ps -e
# - errores de IPC PCM en logs
# - orbe inerte
```

Expected: conversación funciona; orbe reacciona al TTS; sólo proceso `ironclaw` (PPID systemd-user). Bug del feedback loop sigue → eso es B3.

- [ ] **Step 2.7.8: Commit**

```bash
git add crates/ arch/ Cargo.toml
git commit -m "$(cat <<'EOF'
feat(voice)[F4/B2.7]: drop jarvis_voice_daemon crate + systemd unit

Borrado del crate completo, su systemd unit, y todas las referencias en
arch/install.sh + arch/update.sh. El proceso jarvis-voice-daemon ya no
existe — todo corre dentro de ironclaw vía crates/jarvis_voice.

AEC todavía depende de PipeWire echo-cancel.conf — lo cierra B3.

Validado en Asus: conversación end-to-end OK, ps -e sin
jarvis-voice-daemon, orbe reactivo.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

# B3 — `feat(voice)[F4/B3]: WebRTC AEC + rubato resample`

**Estado al final de B3:** AEC propio activado dentro del crate. El bug del feedback loop reportado en F3b desaparece sin recargar el módulo PipeWire `module-echo-cancel`. `arch/configs/pipewire/echo-cancel.conf` se borra; `pick_*_device` se simplifica para abrir el default.

> ⚠️ R1: `webrtc-audio-processing 0.5` requiere `cmake` + `clang` + `libstdc++` en la build host. Validar antes de empezar.

---

### Task 3.1: Validar build de `webrtc-audio-processing`

**Files:** ninguno (validación temprana)

- [ ] **Step 3.1.1: Asegurar deps de sistema en Asus**

```bash
sudo pacman -S --needed cmake clang make
```

- [ ] **Step 3.1.2: Probar `cargo add` aislado**

```bash
cd /tmp && cargo new webrtc_test --bin && cd webrtc_test
cargo add webrtc-audio-processing
cargo build 2>&1 | tail -40
```

Expected: build pasa. Si falla:
- Plan B: `cargo add webrtc-audio-processing-sys` y wrapper a mano (~50 líneas).
- Plan C documentado: caer a `speex-dsp-rs`.

> NO commitear el crate aislado; es solo validación. Si falla, parar el plan y discutir con el usuario antes de seguir.

---

### Task 3.2: Implementar `aec.rs` con smoke test

**Files:**
- Create: `crates/jarvis_voice/src/aec.rs`
- Create: `crates/jarvis_voice/tests/aec_smoke.rs`
- Modify: `crates/jarvis_voice/Cargo.toml` (add dep)
- Modify: `crates/jarvis_voice/src/lib.rs` (add `mod aec;`)

- [ ] **Step 3.2.1: Añadir dep**

Edit `crates/jarvis_voice/Cargo.toml`:

```toml
webrtc-audio-processing = "0.5"
```

- [ ] **Step 3.2.2: Wrapper de Processor**

`crates/jarvis_voice/src/aec.rs`:

```rust
//! `AecProcessor` — wrapper de `webrtc_audio_processing::Processor`.
//!
//! Configurado para conferencia speakerphone:
//! - AEC3 enabled (state-of-the-art para mic+altavoz abiertos).
//! - Noise suppression aggressive.
//! - AGC adaptive digital.
//! - VAD off (Convai hace su propio turn detection server-side).
//! - Frame size: 10ms @ 16kHz mono = 160 samples.
//!
//! El llamador debe respetar el orden temporal: `process_reverse_stream`
//! (far-end, lo que va al speaker) ANTES de `process_stream` (near-end,
//! lo que viene del mic) para el mismo "tick".

use crate::error::VoiceError;
use webrtc_audio_processing::{
    Config, EchoCancellation, EchoCancellationSuppressionLevel, GainControl, GainControlMode,
    InitializationConfig, NoiseSuppression, NoiseSuppressionLevel, Processor, Stats,
};

pub const AEC_FRAME_SAMPLES: usize = 160;
pub const AEC_SAMPLE_RATE_HZ: u32 = 16_000;

pub struct AecProcessor {
    inner: Processor,
}

impl AecProcessor {
    pub fn new(stream_delay_ms: u32) -> Result<Self, VoiceError> {
        let init = InitializationConfig {
            num_capture_channels: 1,
            num_render_channels: 1,
            ..InitializationConfig::default()
        };
        let mut processor = Processor::new(&init)
            .map_err(|e| VoiceError::AecInit(format!("Processor::new: {e:?}")))?;

        let mut config = Config::default();
        config.echo_cancellation = Some(EchoCancellation {
            suppression_level: EchoCancellationSuppressionLevel::High,
            stream_delay_ms: Some(stream_delay_ms),
            enable_delay_agnostic: true,
            enable_extended_filter: true,
        });
        config.noise_suppression = Some(NoiseSuppression {
            suppression_level: NoiseSuppressionLevel::High,
        });
        config.gain_control = Some(GainControl {
            mode: GainControlMode::AdaptiveDigital,
            target_level_dbfs: 3,
            compression_gain_db: 9,
            enable_limiter: true,
        });
        config.enable_voice_detection = false;
        processor.set_config(config);

        Ok(Self { inner: processor })
    }

    /// Procesa un frame del speaker (far-end). Llamar ANTES de
    /// `process_stream` para el mismo "tick".
    pub fn process_reverse_stream(&mut self, frame: &mut [f32]) -> Result<(), VoiceError> {
        debug_assert_eq!(frame.len(), AEC_FRAME_SAMPLES);
        self.inner
            .process_render_frame(frame)
            .map_err(|e| VoiceError::AecInit(format!("process_render_frame: {e:?}")))
    }

    /// Procesa un frame del mic (near-end). Devuelve el frame con eco
    /// suprimido (sobre-escrito in-place).
    pub fn process_stream(&mut self, frame: &mut [f32]) -> Result<(), VoiceError> {
        debug_assert_eq!(frame.len(), AEC_FRAME_SAMPLES);
        self.inner
            .process_capture_frame(frame)
            .map_err(|e| VoiceError::AecInit(format!("process_capture_frame: {e:?}")))
    }

    pub fn stats(&self) -> Option<Stats> {
        self.inner.get_stats().ok()
    }
}
```

> NOTA: las APIs exactas de `webrtc-audio-processing 0.5` pueden variar. Si la doc en docs.rs muestra otros nombres (ej. `process_capture_frame_f32`), ajustar al match. La validación de Task 3.1 cubrió la build; aquí ajustar firmas si hace falta.

- [ ] **Step 3.2.3: Declarar `mod aec;` en `lib.rs`**

Edit `crates/jarvis_voice/src/lib.rs`:

```rust
mod aec;
mod audio_io;
mod config;
mod elevenlabs;
mod engine;
mod error;
mod orchestrator;
mod resample;     // creado en Task 3.3
mod types;
```

- [ ] **Step 3.2.4: Smoke test de convergencia**

`crates/jarvis_voice/tests/aec_smoke.rs`:

```rust
//! Smoke test del AEC: con un seno 440Hz como far-end (lo que va al
//! speaker) y el mismo seno atenuado + retardado 50ms como near-end
//! (lo que el mic captaría), después de 1s de adaptación la potencia
//! residual debe ser < 30% de la original. Es un piso burdo pero
//! detecta regresiones grandes (config rota, versión incompatible).

use jarvis_voice::*;
// `AecProcessor` y constantes son crate-private; el test las re-importa
// como un test integration que vive en `tests/`. Para acceder añadir
// `pub use aec::*;` temporal en lib.rs, o convertir el test a un
// `#[cfg(test)] mod` dentro de `aec.rs`. Preferimos lo segundo.
```

> Mejor mover este smoke test a `crates/jarvis_voice/src/aec.rs` como `#[cfg(test)] mod tests`. Reescribe el smoke en ese archivo:

Añadir al final de `crates/jarvis_voice/src/aec.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    fn seno_440hz(samples: usize, offset: usize) -> Vec<f32> {
        (0..samples)
            .map(|i| (TAU * 440.0 * (i + offset) as f32 / 16_000.0).sin() * 0.5)
            .collect()
    }

    fn power(frame: &[f32]) -> f32 {
        frame.iter().map(|s| s * s).sum::<f32>() / frame.len() as f32
    }

    #[test]
    fn aec_attenuates_synthetic_echo() {
        let mut aec = AecProcessor::new(50).expect("aec init");
        let total_frames = 100; // 1s a 10ms/frame
        let delay_frames = 5; // 50ms
        let attenuation = 0.4_f32;

        // Reservar muestras suficientes (referencia delayed).
        let total_samples = (total_frames + delay_frames + 1) * AEC_FRAME_SAMPLES;
        let reference = seno_440hz(total_samples, 0);

        let mut residual_first = 0.0_f32;
        let mut residual_last = 0.0_f32;

        for tick in 0..total_frames {
            let off = tick * AEC_FRAME_SAMPLES;
            let mut far: Vec<f32> = reference[off..off + AEC_FRAME_SAMPLES].to_vec();
            // near = far retardado * atenuación (simula eco capturado).
            let near_off = off.saturating_sub(delay_frames * AEC_FRAME_SAMPLES);
            let mut near: Vec<f32> = reference[near_off..near_off + AEC_FRAME_SAMPLES]
                .iter()
                .map(|s| s * attenuation)
                .collect();

            // ORDER MATTERS: reverse (far) before stream (near).
            aec.process_reverse_stream(&mut far).unwrap();
            aec.process_stream(&mut near).unwrap();

            if tick == 0 {
                residual_first = power(&near);
            }
            if tick == total_frames - 1 {
                residual_last = power(&near);
            }
        }

        let original = power(&seno_440hz(AEC_FRAME_SAMPLES, 0))
            * attenuation
            * attenuation;
        let ratio_first = residual_first / original;
        let ratio_last = residual_last / original;
        assert!(
            ratio_last < 0.3,
            "AEC should attenuate echo to <30% original power within 1s; \
             got ratio_first={ratio_first:.3} ratio_last={ratio_last:.3}"
        );
    }
}
```

- [ ] **Step 3.2.5: Run smoke test**

```bash
cargo test -p jarvis_voice --lib aec::tests::aec_attenuates_synthetic_echo
```

Expected: PASS. Si falla con ratio cerca de 1.0, revisar orden de llamadas y `stream_delay_ms`.

- [ ] **Step 3.2.6: Commit**

```bash
git add crates/jarvis_voice/Cargo.toml crates/jarvis_voice/src/aec.rs crates/jarvis_voice/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(voice)[F4/B3.2]: AecProcessor wrapping webrtc-audio-processing 0.5

AEC3 + NS High + AGC AdaptiveDigital, 160 samples/frame @ 16kHz. Smoke
test sintético verifica convergencia: ratio<30% del eco original tras
1s de adaptación con seno 440Hz retardado 50ms y atenuado 40%.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 3.3: Implementar `resample.rs` (rubato polyphase)

**Files:**
- Create: `crates/jarvis_voice/src/resample.rs`
- Modify: `crates/jarvis_voice/Cargo.toml` (add `rubato`)

- [ ] **Step 3.3.1: Añadir dep**

```toml
rubato = "0.16"
```

- [ ] **Step 3.3.2: Implementar `MicResampler` y `SpeakerResampler`**

`crates/jarvis_voice/src/resample.rs`:

```rust
//! Resamplers polyphase para el pipeline de voz.
//!
//! - `MicResampler`: device_rate (típicamente 48000) → 16000 mono.
//! - `SpeakerResampler`: 16000 mono → device_rate.
//!
//! `rubato::FftFixedIn` / `FftFixedOut` son polyphase de alta calidad.
//! Reemplazan el resampler por decimación lineal del daemon legacy —
//! necesario para que el adaptive filter del AEC converja.

use crate::error::VoiceError;
use rubato::{FftFixedIn, FftFixedOut, Resampler};

pub const VOICE_RATE_HZ: u32 = 16_000;
pub const FRAME_AT_VOICE_RATE: usize = 160; // 10ms @ 16kHz

pub struct MicResampler {
    inner: FftFixedIn<f32>,
}

impl MicResampler {
    pub fn new(device_rate: u32) -> Result<Self, VoiceError> {
        let inner = FftFixedIn::<f32>::new(
            device_rate as usize,
            VOICE_RATE_HZ as usize,
            // chunk size en device rate: aprox 10ms.
            (device_rate as usize / 100).max(1),
            1, // 1 sub-chunk
            1, // mono
        )
        .map_err(|e| VoiceError::AudioDevice(format!("mic resampler init: {e}")))?;
        Ok(Self { inner })
    }

    pub fn process(&mut self, input: &[f32]) -> Result<Vec<f32>, VoiceError> {
        let in_arr = vec![input.to_vec()];
        let out = self
            .inner
            .process(&in_arr, None)
            .map_err(|e| VoiceError::AudioDevice(format!("mic resampler process: {e}")))?;
        Ok(out.into_iter().next().unwrap_or_default())
    }
}

pub struct SpeakerResampler {
    inner: FftFixedOut<f32>,
}

impl SpeakerResampler {
    pub fn new(device_rate: u32) -> Result<Self, VoiceError> {
        let inner = FftFixedOut::<f32>::new(
            VOICE_RATE_HZ as usize,
            device_rate as usize,
            (device_rate as usize / 100).max(1),
            1,
            1,
        )
        .map_err(|e| VoiceError::AudioDevice(format!("speaker resampler init: {e}")))?;
        Ok(Self { inner })
    }

    pub fn process(&mut self, input: &[f32]) -> Result<Vec<f32>, VoiceError> {
        let in_arr = vec![input.to_vec()];
        let out = self
            .inner
            .process(&in_arr, None)
            .map_err(|e| VoiceError::AudioDevice(format!("speaker resampler process: {e}")))?;
        Ok(out.into_iter().next().unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mic_resampler_48k_to_16k_changes_rate() {
        let mut r = MicResampler::new(48_000).expect("init");
        let input = vec![0.0_f32; 480]; // 10ms @ 48k
        let out = r.process(&input).expect("process");
        // 480 samples @ 48k corresponde a ~160 samples @ 16k (no exacto
        // por el chunking de FFT, dejamos margen ±10).
        assert!(
            (140..=180).contains(&out.len()),
            "expected ~160 samples at 16kHz, got {}",
            out.len()
        );
    }

    #[test]
    fn speaker_resampler_16k_to_48k_changes_rate() {
        let mut r = SpeakerResampler::new(48_000).expect("init");
        let input = vec![0.0_f32; FRAME_AT_VOICE_RATE];
        let out = r.process(&input).expect("process");
        assert!(out.len() > FRAME_AT_VOICE_RATE);
    }
}
```

> Las APIs exactas de `rubato` 0.16 — chequear contra docs.rs si los nombres no son `FftFixedIn`/`FftFixedOut`. Adjustar si la versión publicada los renombra.

- [ ] **Step 3.3.3: Run tests**

```bash
cargo test -p jarvis_voice --lib resample::tests
```

Expected: PASS.

- [ ] **Step 3.3.4: Commit**

```bash
git add crates/jarvis_voice/Cargo.toml crates/jarvis_voice/src/resample.rs crates/jarvis_voice/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(voice)[F4/B3.3]: rubato polyphase resamplers (mic + speaker)

MicResampler (device_rate→16k) y SpeakerResampler (16k→device_rate).
FFT polyphase, alta calidad. Reemplaza el resampler por decimación
lineal del daemon legacy — pre-requisito para que el adaptive filter
del AEC converja.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 3.4: Wire AEC + resample en orchestrator + audio_io 10ms

**Files:**
- Modify: `crates/jarvis_voice/src/audio_io/mod.rs` (frames de 10ms en lugar de 50ms)
- Modify: `crates/jarvis_voice/src/orchestrator.rs` (insertar AEC pipeline)

- [ ] **Step 3.4.1: Cambiar `CHUNK_SAMPLES` a 160 (10ms)**

Edit `crates/jarvis_voice/src/audio_io/mod.rs`:

```rust
/// 10ms @ 16kHz = 160 samples. Tamaño de frame del AEC.
pub const CHUNK_SAMPLES: usize = 160;
```

> El daemon legacy usaba 800 (50ms) porque ElevenLabs recomienda chunks 50-100ms y porque sin AEC no había razón para frames cortos. Con AEC 10ms es obligatorio (frame size del WebRTC processor).

- [ ] **Step 3.4.2: Reescribir orchestrator**

Edit `crates/jarvis_voice/src/orchestrator.rs`. La parte clave es el flujo:

```
mic_chunk_raw (160 samples @ 16k, mono) ─►
    process_reverse_stream(far)            (far ya alineado del último tick)
    process_stream(near=mic)               (eco suprimido in-place)
    ws.send(Outbound::Audio(near_as_i16))

ws.recv(Inbound::AgentAudio(pcm_16k)) ─►
    speaker_tx.play(pcm_16k)               (cpal output stream lo resamplea al device)
    next_far = pcm_16k                     (guardamos para el próximo reverse_stream)
    broadcast(VoiceEvent::AgentAudio)
```

```rust
use crate::aec::{AecProcessor, AEC_FRAME_SAMPLES};
// resto del módulo igual ...

impl OrchestratorTask {
    pub async fn run(mut self, cfg: VoiceConfig) -> Result<(), VoiceError> {
        let AudioIo {
            mut mic_rx,
            speaker_tx,
            ..
        } = audio_io::start()?;

        let mut aec = AecProcessor::new(cfg.aec_delay_ms)?;
        let mut ws = elevenlabs::connect(&cfg).await?;
        let outbound_tx = ws.outbound_tx.clone();

        // Last far frame buffered: el speaker reproduce al ritmo de la
        // red; almacenamos el último chunk del agente para alimentar
        // reverse_stream antes de cada near. Si todavía no hay agente
        // hablando → frame de silencio.
        let mut last_far_f32: Vec<f32> = vec![0.0; AEC_FRAME_SAMPLES];

        // Forwarder mic → AEC → ws outbound. Lo movemos a dentro del
        // loop principal porque ahora AEC es state-ful y vive aquí.
        loop {
            tokio::select! {
                _ = self.stop_rx.recv() => {
                    let _ = outbound_tx.send(Outbound::Stop).await;
                    break;
                }
                Some(result) = self.tool_rx.recv() => {
                    let _ = outbound_tx.send(Outbound::ToolResult(ClientToolResult {
                        kind: "client_tool_result",
                        tool_call_id: result.tool_call_id,
                        result: result.result,
                        is_error: result.is_error,
                    })).await;
                }
                Some(mic_chunk) = mic_rx.recv() => {
                    // mic_chunk: 160 samples @ 16k i16. Convertir a f32 [-1,1].
                    let mut far = last_far_f32.clone();
                    let mut near: Vec<f32> = mic_chunk.iter()
                        .map(|s| *s as f32 / i16::MAX as f32)
                        .collect();
                    if near.len() != AEC_FRAME_SAMPLES {
                        // No es un múltiplo exacto — saltarse este chunk
                        // (audio_io ya entrega 160 cuando CHUNK_SAMPLES=160).
                        continue;
                    }
                    aec.process_reverse_stream(&mut far)?;
                    aec.process_stream(&mut near)?;
                    // Re-encode a i16 para WS.
                    let cleaned: Vec<i16> = near.iter()
                        .map(|s| (s * i16::MAX as f32).clamp(i16::MIN as f32, i16::MAX as f32) as i16)
                        .collect();
                    if outbound_tx.send(Outbound::Audio(cleaned)).await.is_err() {
                        break;
                    }
                }
                evt = ws.inbound_rx.recv() => {
                    match evt {
                        Some(Inbound::AgentAudio(pcm)) => {
                            // Guardar como referencia far para el próximo tick.
                            // (Si pcm.len() != 160 por chunking del WS, tomar
                            // los primeros 160; el resto se reproduce normalmente.)
                            if pcm.len() >= AEC_FRAME_SAMPLES {
                                last_far_f32 = pcm[..AEC_FRAME_SAMPLES]
                                    .iter()
                                    .map(|s| *s as f32 / i16::MAX as f32)
                                    .collect();
                            }
                            let frame = PcmFrame {
                                samples: Arc::from(pcm.clone().into_boxed_slice()),
                                sample_rate: SampleRate::ELEVENLABS,
                            };
                            let _ = self.events_tx.send(VoiceEvent::AgentAudio(frame));
                            speaker_tx.play(pcm);
                        }
                        // resto de variantes idénticas a Task 2.3 ...
                        Some(Inbound::UserTranscript(text)) => {
                            let _ = self.events_tx.send(VoiceEvent::UserTranscript(text));
                        }
                        Some(Inbound::AgentResponse(text)) => {
                            let _ = self.events_tx.send(VoiceEvent::AgentTranscript(text));
                        }
                        Some(Inbound::AgentResponseCorrection { original, corrected }) => {
                            let _ = self.events_tx.send(VoiceEvent::AgentTranscriptCorrection {
                                original,
                                corrected,
                            });
                        }
                        Some(Inbound::Interruption { reason, .. }) => {
                            speaker_tx.flush();
                            let r = match reason.as_deref() {
                                Some("user") => InterruptionReason::User,
                                Some("server") => InterruptionReason::Server,
                                _ => InterruptionReason::Unknown,
                            };
                            let _ = self.events_tx.send(VoiceEvent::Interrupted { reason: r });
                        }
                        Some(Inbound::Ping { event_id }) => {
                            let _ = outbound_tx.send(Outbound::Pong { event_id }).await;
                        }
                        Some(Inbound::ToolCall(call)) => {
                            let _ = self.events_tx.send(VoiceEvent::ToolCallRequested(
                                ToolCallRequest {
                                    tool_call_id: call.tool_call_id.clone(),
                                    tool_name: call.tool_name.clone(),
                                    parameters: call.parameters.clone(),
                                },
                            ));
                            let result = placeholder_tool_result(call);
                            let _ = outbound_tx.send(Outbound::ToolResult(result)).await;
                        }
                        Some(Inbound::Connected { conversation_id }) => {
                            match ConversationId::new(conversation_id) {
                                Ok(id) => {
                                    let _ = self.events_tx.send(VoiceEvent::Connected { conversation_id: id });
                                }
                                Err(e) => {
                                    tracing::debug!(error = %e, "orchestrator.invalid_conversation_id");
                                }
                            }
                        }
                        Some(Inbound::Disconnected) => {
                            let _ = self.events_tx.send(VoiceEvent::Disconnected);
                            break;
                        }
                        None => break,
                    }
                }
            }
        }

        Ok(())
    }
}
```

> NOTA: la simplificación asume que el resampler vive dentro de `audio_io::start()` (ya entrega 160 samples @ 16k). Si decides exponerlo afuera, mover la creación de `MicResampler` aquí y aplicar antes del AEC. La opción in-`audio_io` es más simple para el orchestrator.

- [ ] **Step 3.4.3: Build + test**

```bash
cargo clippy --all --benches --tests --examples --all-features
cargo test -p jarvis_voice
cargo test --lib
```

Expected: zero warnings, todos los tests pasan.

- [ ] **Step 3.4.4: Commit**

```bash
git add crates/jarvis_voice/
git commit -m "$(cat <<'EOF'
feat(voice)[F4/B3.4]: orchestrator routes mic+far through AecProcessor

mic chunk (160 samples @ 16k) → AEC.process_reverse_stream(far) →
AEC.process_stream(near) → WS Outbound::Audio. Far buffer es el último
agent_audio recibido (silencio si todavía no hubo). El orden temporal
reverse-then-stream es invariante para el AEC adaptive filter.

stream_delay_ms tunable vía JARVIS_VOICE_AEC_DELAY_MS (default 50).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 3.5: Simplificar `pick_*_device` y borrar echo-cancel.conf

**Files:**
- Modify: `crates/jarvis_voice/src/audio_io/mod.rs` (`pick_input_device`, `pick_output_device`)
- Delete: `arch/configs/pipewire/echo-cancel.conf`
- Modify: `arch/install.sh` (borrar pasos echo-cancel)

- [ ] **Step 3.5.1: Simplificar `pick_input_device`**

Reemplaza:

```rust
fn pick_input_device(host: &cpal::Host) -> Result<cpal::Device, VoiceError> {
    use cpal::traits::HostTrait;
    host.default_input_device()
        .ok_or_else(|| VoiceError::AudioDevice("no default audio input device".into()))
}
```

(Borra la búsqueda por `PIPEWIRE_ECHO_CANCEL_NODE`; ya no es necesaria.)

- [ ] **Step 3.5.2: Simplificar `pick_output_device`**

```rust
fn pick_output_device(host: &cpal::Host) -> Result<cpal::Device, VoiceError> {
    use cpal::traits::HostTrait;
    host.default_output_device()
        .ok_or_else(|| VoiceError::AudioDevice("no default audio output device".into()))
}
```

(Borra el filtro `ECHO_CANCEL_PASSIVE` y la preferencia `alsa_output`/`bluez_output`.)

Borra también las constantes `PIPEWIRE_ECHO_CANCEL_NODE` y `ECHO_CANCEL_PASSIVE` arriba del archivo.

- [ ] **Step 3.5.3: Borrar la conf de PipeWire**

```bash
git rm arch/configs/pipewire/echo-cancel.conf
```

- [ ] **Step 3.5.4: Limpiar `arch/install.sh`**

Borra los pasos relacionados con `module-echo-cancel`:

```bash
grep -n "echo-cancel\|module-echo" arch/install.sh
```

Elimina las líneas (referencias, `cp` de la conf, `pactl load-module`, etc.). Mantener únicamente lo que queda relevante de PipeWire si hay alguna parte que no es del módulo.

- [ ] **Step 3.5.5: Build + test**

```bash
cargo clippy --all --benches --tests --examples --all-features
cargo test --lib
```

Expected: zero warnings.

- [ ] **Step 3.5.6: Validación Asus (manual, M2 invariant — la importante)**

```bash
# en Asus
pactl unload-module module-echo-cancel || true   # asegurar que no esté cargado
pactl list modules | grep echo-cancel             # debe estar vacío
cd ~/git/jarvis-os && bash arch/install.sh
systemctl --user restart ironclaw

# 5 minutos de prueba con speakerphone abierto:
# - hablar, parar, escuchar respuesta entera
# - hablar, parar, esperar respuesta entera, hablar otra vez
# - dejar que jarvis hable solo durante una respuesta larga
```

Expected:
- Conversación end-to-end OK.
- **Bug del feedback loop NO ocurre durante 5 min.**
- Orbe sigue reaccionando al TTS (sin regresión visual desde F3b/B5).

Si el bug persiste:
- Tunear `JARVIS_VOICE_AEC_DELAY_MS` (probar 30, 80, 100).
- Si tras varias pruebas el bug sigue: NO mergear B3 sin discutir; documentar como F4.1 (combinar AEC3 + VAD propia para mutear mic durante TTS playback).

- [ ] **Step 3.5.7: Commit**

```bash
git add arch/ crates/jarvis_voice/src/audio_io/mod.rs
git commit -m "$(cat <<'EOF'
feat(voice)[F4/B3.5]: drop PipeWire echo-cancel.conf — AEC is in-process

pick_input_device y pick_output_device abren el default device del host
ahora que el AEC vive dentro del crate. Borrado:
- arch/configs/pipewire/echo-cancel.conf
- pasos de install.sh que cargan module-echo-cancel
- preferencias jarvis-mic-aec / sink.jarvis-aec

Validación Asus: 5 min de speakerphone abierto, bug del feedback loop
ya NO ocurre. pactl list modules | grep echo-cancel = vacío.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

# B4 — `chore(ipc)[F4/B4]: drop TtsPcmFrame from local_ipc protocol`

**Estado al final de B4:** deuda IPC de F3b cerrada. `ClientCommand::TtsPcmFrame` no existe; ningún channel ni handler thread `Option<Arc<ElevenLabsIpcBackend>>`. Tests en `local_ipc` y `audio` siguen verdes.

---

### Task 4.1: Borrar variant `TtsPcmFrame` y rama de dispatch

**Files:**
- Modify: `src/channels/local_ipc/protocol.rs`
- Modify: `src/channels/local_ipc/client.rs`

- [ ] **Step 4.1.1: Borrar variant + tests asociados**

Edit `src/channels/local_ipc/protocol.rs`:
- Borrar la variant `ClientCommand::TtsPcmFrame { samples_b64, sample_rate }`.
- Borrar `tts_pcm_frame_roundtrip` test.
- Cualquier otro test que mencione `TtsPcmFrame`.

- [ ] **Step 4.1.2: Borrar rama de dispatch**

Edit `src/channels/local_ipc/client.rs`:
- Borrar el `match` arm `ClientCommand::TtsPcmFrame { samples_b64, sample_rate } => { ... }` en `dispatch_command`.
- Borrar el parámetro `tts_backend: Option<&ElevenLabsIpcBackend>` de `dispatch_command` y de los call sites en `client.rs`.
- Borrar `tts_backend.as_deref()` en la llamada a `dispatch_command`.
- Borrar el campo `tts_backend` de `ClientReader`/`ClientSession` (los structs locales del módulo).
- Borrar `use crate::audio::backends::ElevenLabsIpcBackend;` y todos los imports relacionados.

- [ ] **Step 4.1.3: `cargo build`**

Expected: probable error: `mod.rs`, `channel_impl.rs`, `socket.rs` siguen pasando `tts_backend`. Continuar en 4.2.

---

### Task 4.2: Borrar threading `Option<Arc<ElevenLabsIpcBackend>>` en `local_ipc/{mod,channel_impl,socket}.rs` y `main.rs`

**Files:**
- Modify: `src/channels/local_ipc/mod.rs`
- Modify: `src/channels/local_ipc/channel_impl.rs`
- Modify: `src/channels/local_ipc/socket.rs`
- Modify: `src/main.rs`

- [ ] **Step 4.2.1: `mod.rs`**

Quitar el parámetro `tts_backend` de `pub async fn create(...)`. La firma queda:

```rust
pub async fn create(
    owner_id: UserId,
    sse: Arc<EventBus>,
    writer_buffer: usize,
) -> Result<Option<LocalIpcChannel>, LocalIpcError> { ... }
```

Quitar `let tts_backend = ...;` del cuerpo y quitar el argumento al constructor de `LocalIpcChannel`.

- [ ] **Step 4.2.2: `channel_impl.rs`**

Quitar el campo `tts_backend` del struct `LocalIpcChannel` y el parámetro del constructor `new`. La función `dispatch_command` se llama ahora sin él.

- [ ] **Step 4.2.3: `socket.rs`**

Quitar el campo `tts_backend` del struct `SocketCfg` (o equivalente). Quitar las dos asignaciones `tts_backend: None` que aparecen en los tests/setup helpers.

- [ ] **Step 4.2.4: `main.rs`**

Quitar la variable `let mut ipc_tts_backend ...` y el segundo argumento que pasaba a `local_ipc::create(...)`. La llamada queda:

```rust
match ironclaw::channels::local_ipc::create(
    config.owner_id.clone(),
    sse_for_local,
    config.channels.local_ipc.writer_buffer,
)
.await { ... }
```

- [ ] **Step 4.2.5: Build**

```bash
cargo clippy --all --benches --tests --examples --all-features
cargo test --lib local_ipc
cargo test --lib audio
```

Expected: zero warnings, tests verdes.

> Si tests menciona `TtsPcmFrame` en algún string literal, ajustar.

---

### Task 4.3: Borrar `elevenlabs_ipc.rs` y variant `ElevenlabsIpc`

**Files:**
- Delete: `src/audio/backends/elevenlabs_ipc.rs`
- Modify: `src/audio/backends/mod.rs`
- Modify: `src/audio/tts.rs`
- Modify: `src/main.rs` (ya solo queda `ElevenlabsLocal` y `None`)
- Modify: `src/config/audio.rs`

- [ ] **Step 4.3.1: Borrar el backend**

```bash
git rm src/audio/backends/elevenlabs_ipc.rs
```

Edit `src/audio/backends/mod.rs`: borrar `pub mod elevenlabs_ipc;` y `pub use elevenlabs_ipc::ElevenLabsIpcBackend;`.

- [ ] **Step 4.3.2: Borrar variant `ElevenlabsIpc`**

Edit `src/audio/tts.rs`: borrar la variant y el arm correspondiente en `as_str`.

- [ ] **Step 4.3.3: Limpiar parser**

Edit `src/config/audio.rs::parse_backend`:

```rust
fn parse_backend(raw: &str) -> TtsBackendKind {
    match raw.trim().to_ascii_lowercase().as_str() {
        "" | "none" | "off" | "false" | "0" | "disabled" => TtsBackendKind::None,
        "elevenlabs_local" | "elevenlabs-local" | "elevenlabs_ipc" | "elevenlabs-ipc"
        | "elevenlabs" | "voice_in_process" => TtsBackendKind::ElevenlabsLocal,
        other => {
            tracing::warn!(value = other, "Unknown JARVIS_TTS_BACKEND, falling back to none");
            TtsBackendKind::None
        }
    }
}
```

> Mantener `elevenlabs_ipc` como alias para que los `.env` antiguos sigan funcionando — apunta al backend in-process.

Actualizar tests en el mismo archivo: el test `parse_backend_recognises_known_values` debe asertar que `elevenlabs_ipc` mapea a `TtsBackendKind::ElevenlabsLocal`.

- [ ] **Step 4.3.4: Limpiar `main.rs`**

Edit `src/main.rs`: el match queda con dos brazos, `ElevenlabsLocal => { ... }` y `None => None`.

- [ ] **Step 4.3.5: `pipeline.rs` test**

Edit `src/audio/pipeline.rs` (test linea 65-75): cambiar `ElevenLabsIpcBackend::new(16)` por algo equivalente que no dependa de IPC. Opciones:
1. Usar el backend `NoneBackend` (no produce frames — entonces el test pierde valor).
2. Crear un mock `TestBackend` inline que cumpla `TtsBackend` y exponga `push_frame` (recomendado).

Reemplaza el bloque del test con un mock:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::tts::TtsBackend;
    use crate::audio::types::PcmFrame;
    use std::sync::Arc;
    use tokio::sync::broadcast;

    struct TestBackend(broadcast::Sender<PcmFrame>);
    impl TestBackend {
        fn new() -> Self {
            Self(broadcast::channel(16).0)
        }
        fn push(&self, f: PcmFrame) {
            let _ = self.0.send(f);
        }
    }
    impl TtsBackend for TestBackend {
        fn name(&self) -> &str { "test" }
        fn subscribe_frames(&self) -> broadcast::Receiver<PcmFrame> { self.0.subscribe() }
    }

    // ... resto del test reescrito en términos de TestBackend.
}
```

- [ ] **Step 4.3.6: Build + test full**

```bash
cargo clippy --all --benches --tests --examples --all-features
cargo test --lib
cargo test --features integration
```

Expected: zero warnings, todos los tests verdes.

- [ ] **Step 4.3.7: Commit (todo B4 junto)**

```bash
git add src/ Cargo.toml
git commit -m "$(cat <<'EOF'
chore(ipc)[F4/B4]: drop TtsPcmFrame and ElevenLabsIpcBackend

ClientCommand::TtsPcmFrame variant + dispatch arm + threading de
Option<Arc<ElevenLabsIpcBackend>> a través de
local_ipc::create / LocalIpcChannel / dispatch_command / main.rs:
borrados.

src/audio/backends/elevenlabs_ipc.rs y variant TtsBackendKind::ElevenlabsIpc
borrados también. JARVIS_TTS_BACKEND=elevenlabs_ipc sigue siendo válido
como alias por compat con .env existentes — apunta al backend in-process.

Tests local_ipc 38/38 verdes, tests audio 21/21 verdes, conversación
end-to-end OK en Asus.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

# B5 — `docs(voice)[F4/B5]: update CLAUDE.md, project structure, memory`

---

### Task 5.1: Actualizar `CLAUDE.md`

**Files:**
- Modify: `CLAUDE.md`

- [ ] **Step 5.1.1: Project Structure**

Edit `CLAUDE.md` sección "Project Structure":
- Quitar `└── jarvis_voice_daemon/ # Rust + cpal + tokio-tungstenite + ElevenLabs Convai cloud`
- Añadir `└── jarvis_voice/        # Rust + cpal + tokio-tungstenite + WebRTC AEC3 + rubato (in-process voice engine)`

- [ ] **Step 5.1.2: Extracted Crates**

Sección "Extracted Crates" — añadir:

```
Voice engine in-process en `crates/jarvis_voice/`. **Import directly**
con `use jarvis_voice::{VoiceEngine, VoiceHandle, VoiceConfig};`. El
shim `src/audio/backends/elevenlabs_local.rs` traduce `VoiceEvent` →
`crate::audio::types::PcmFrame` para alimentar el `TtsAudioPipeline`.
```

- [ ] **Step 5.1.3: Configuration**

Si existe una tabla de envs, añadir o actualizar:

```
JARVIS_TTS_BACKEND          - none | elevenlabs_local (alias: elevenlabs_ipc, elevenlabs)
JARVIS_VOICE_AEC_DELAY_MS   - delay del AEC en ms (default 50)
JARVIS_VOICE_VARS           - dynamic_variables del agente: "k1=v1,k2=v2"
JARVIS_VOICE_SYSTEM_PROMPT_OVERRIDE - reemplaza el system prompt del agente
```

- [ ] **Step 5.1.4: `cargo doc` sanity**

```bash
cargo doc --no-deps -p jarvis_voice 2>&1 | grep -i "warning\|error" || echo "ok"
```

Expected: sin warnings.

- [ ] **Step 5.1.5: Commit**

```bash
git add CLAUDE.md
git commit -m "$(cat <<'EOF'
docs(voice)[F4/B5.1]: CLAUDE.md reflects in-process voice engine

Project Structure: quitar jarvis_voice_daemon, añadir jarvis_voice.
Extracted Crates: documentar import path. Configuration: envs nuevas
JARVIS_VOICE_AEC_DELAY_MS y semantics actualizadas de
JARVIS_TTS_BACKEND.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 5.2: Cerrar el spec con SHAs + actualizar memory

**Files:**
- Modify: `docs/superpowers/specs/2026-05-02-jarvis-voice-in-process-design.md`
- Create: `~/.claude/projects/-home-nexus-git-jarvis-os/memory/project_resume_2026_MM_DD_f4_closed.md`
- Modify: `~/.claude/projects/-home-nexus-git-jarvis-os/memory/MEMORY.md`

- [ ] **Step 5.2.1: Spec — añadir status block**

Edit `docs/superpowers/specs/2026-05-02-jarvis-voice-in-process-design.md`. En el header, debajo de `**Status:** approved`:

```
**Implementation status:** shipped (F4 closed)

| Commit phase | Range | Notes |
|---|---|---|
| B1 | <SHA1>..<SHA4> | scaffold + subprocess launcher |
| B2 | <SHA5>..<SHA12> | in-process orchestrator, jarvis_voice_daemon deleted |
| B3 | <SHA13>..<SHA17> | WebRTC AEC + rubato; echo-cancel.conf deleted |
| B4 | <SHA18> | TtsPcmFrame + ElevenLabsIpcBackend deleted |
| B5 | <SHA19>..<SHA20> | docs |
```

> Reemplazar SHAs con `git log --oneline jarvis-arch-os | head -25` post-merge.

- [ ] **Step 5.2.2: Memory — resume del cierre**

Crear `~/.claude/projects/-home-nexus-git-jarvis-os/memory/project_resume_2026_MM_DD_f4_closed.md` (sustituir `MM_DD` por la fecha real del cierre):

```markdown
---
name: F4 closed — voice in-process consolidation shipped
description: Cierre del bloque F4 — jarvis_voice_daemon absorbido como librería in-process, AEC propio, IPC PCM eliminado
type: project
---

F4 cerrado: voice engine vive dentro de `ironclaw` como `crates/jarvis_voice/`.

**Why:** el modelo de demonios desconectados producía silent failures
(broken pipe sin reconexión, dependencia del módulo PipeWire echo-cancel
que el usuario olvidaba cargar). Ahora 1 proceso, 1 modo de fallo, AEC
garantizado.

**How to apply:**
- Agregar nuevos backends de voice (Piper, Kokoro) como `TtsBackend`
  *separados* — no entran en `jarvis_voice`. El crate es ElevenLabs Convai-specific.
- Para tunear AEC: env `JARVIS_VOICE_AEC_DELAY_MS` (default 50).
- Tool calls de Convai siguen como placeholder; cablear a IronClaw es F5.
- jarvis-voice-daemon ya no existe — `systemctl --user status jarvis-voice-daemon`
  → "Unit not found" es la respuesta esperada.
```

- [ ] **Step 5.2.3: MEMORY.md — index entry**

Edit `~/.claude/projects/-home-nexus-git-jarvis-os/memory/MEMORY.md` — añadir línea bajo la sección de Resumes:

```
- [Resume: F4 cerrado in-process voice 2026-MM-DD](project_resume_2026_MM_DD_f4_closed.md) — voice engine in-process, AEC propio, daemon borrado
```

- [ ] **Step 5.2.4: Final commit**

```bash
git add docs/superpowers/specs/2026-05-02-jarvis-voice-in-process-design.md
git commit -m "$(cat <<'EOF'
docs(spec)[F4/B5.2]: mark F4 spec as shipped with commit ranges

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

# Criterios de éxito (verificación final)

Tras los 5 commits + push, validar 1×1 en Asus:

- [ ] `git ls-files crates/jarvis_voice_daemon/` → vacío.
- [ ] `systemctl --user status jarvis-voice-daemon` → "Unit not found".
- [ ] `ps aux | grep jarvis-voice` → solo procesos `ironclaw`.
- [ ] `cargo test --lib audio` → todo verde, sin tests modificados a la baja.
- [ ] `cargo test --lib local_ipc` → todo verde con `TtsPcmFrame` removido.
- [ ] `cargo test -p jarvis_voice` → tests unitarios + smoke AEC verdes.
- [ ] Conversación end-to-end con jarvis funciona en Asus, sin recargar `module-echo-cancel`.
- [ ] Bug del feedback loop reportado en F3b NO ocurre durante 5 min de prueba con speakerphone abierto.
- [ ] Orbe sigue reaccionando al audio TTS — cero regresión visual desde F3b/B5.
- [ ] `pactl list modules | grep echo-cancel` → vacío.
- [ ] Spec actualizado con "Implementation status: shipped + commits SHAs".
- [ ] Memory `project_resume_2026_MM_DD_f4_closed.md` creada y enlazada en `MEMORY.md`.

Si **cualquiera** de estos falla, detener el merge y abrir un seguimiento (probablemente F4.1 — tunning AEC).

---

# Notas para el agente que ejecute

1. **Validación física es bloqueante** en B1, B2 y B3. No marcar steps `Validación Asus` sin que el usuario confirme.
2. **Si Task 3.1 (build de webrtc-audio-processing) falla**, parar inmediato y preguntar al usuario por el plan B/C documentado en el spec sección 8 (R1).
3. **Cada commit debe pasar `cargo clippy --all --benches --tests --examples --all-features` con cero warnings** — política del repo.
4. **No usar `unwrap`/`expect` en código de producción.** Tests OK.
5. **`info!`/`warn!` corrompen el TUI** — usa `debug!` en hot paths.
6. **Si la API de `webrtc-audio-processing` o `rubato` no coincide con el plan**, ajusta a la doc real (puede haber drift entre versions). Lo importante es el flujo lógico.
7. **No cambiar el comportamiento del placeholder de tool calls** — F4 explícitamente NO cablea Convai tools a IronClaw; eso es F5.

