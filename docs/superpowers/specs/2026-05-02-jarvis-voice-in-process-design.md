# F4 — jarvis-voice in-process consolidation (design spec)

**Date:** 2026-05-02
**Branch:** `jarvis-arch-os`
**Predecessor:** F3b (closed at commit `048abc4f`)
**Author:** Horelvis Castillo Mendoza
**Status:** approved
**Implementation status:** B1+B2+B4 shipped en host dev; B3 (AEC propio) y validación física en Asus pendientes.

| Phase | Range | Notas |
|---|---|---|
| B1 | `0c5a9ef3..c5ccde2e` | scaffold + subprocess launcher placeholder + shim detrás de feature `voice-in-process` |
| B2 | `08cb9b26..67908532` | in-process orchestrator, jarvis_voice_daemon borrado, systemd unit borrada |
| B3 | — | DEFERRED: requiere `cmake`+`clang` en host y validación AEC en Asus. Plan completo en `docs/superpowers/plans/2026-05-03-jarvis-voice-in-process-impl.md` Tasks 3.1-3.5. |
| B4 | `5d1370c3` | TtsPcmFrame variant borrado + threading limpio + ElevenLabsIpcBackend borrado |
| B5 | _este commit_ | docs + memory |


---

## 1. Problema

F3b validó la arquitectura de eventos audio (TtsBackend trait + EventBus + AudioLevel) pero quedó con dos procesos independientes:

1. `ironclaw` — core agent + UI gateway + IPC server
2. `jarvis-voice-daemon` — cliente ElevenLabs Convai + cpal I/O

Comunicados por UNIX socket (`/run/user/<uid>/ironclaw.sock`) usando `ClientCommand::TtsPcmFrame`.

Esto introdujo modos de fallo silenciosos:

- Daemon vivo pero IPC roto (broken pipe sin reconexión).
- PipeWire echo-cancel module no cargado al boot → daemon abre el `default` source (no AEC) → ElevenLabs detecta self-echo del speaker como user input → `agent.interrupted_by_user` → frase TTS truncada y luego repetida.
- Dependencias implícitas entre procesos sin contrato explícito.

El usuario explícitamente rechazó este modelo: *"tener los demonios desconectados de un sistema y que dependa de que funcione o no"* no es la forma correcta de trabajar.

## 2. Objetivo

Absorber el voice daemon como librería in-process dentro del binario `ironclaw`, eliminando IPC para audio y reemplazando la dependencia de PipeWire echo-cancel module por un AEC propio (WebRTC AEC3) en código que controlamos.

Resultado: 1 proceso, 1 modo de fallo, AEC garantizado.

## 3. Decisiones tomadas durante brainstorming

| ID | Pregunta | Decisión | Justificación |
|----|----------|----------|---------------|
| D1 | Scope F4 | **B**: absorción + AEC propio | (A) deja el bug raíz abierto; (C) overkill |
| D2 | Nombre del crate | **A**: `crates/jarvis_voice` | Patrón `jarvis_*` para crates añadidos por jarvis-os; voice nació en jarvis-os, no es extracción de IronClaw |
| D3 | Boundary con `src/audio/` | **α**: `VoiceEngine` en crate, `ElevenLabsLocalBackend` shim en core | TtsBackend trait estable post-F3b, no tocar; shim ~80 líneas; queda preparado para Piper/Kokoro |
| D4 | Librería AEC | **I**: `webrtc-audio-processing` v0.5+ | AEC3 es state-of-the-art para speakerphone (nuestro caso); Speex inadecuado sin headset |
| D5 | Estrategia de migración | **M2**: staged con sistema funcional en cada commit | Reduce blast radius; throwaway placeholder de B1 desaparece en B2 antes del merge final |

## 4. Arquitectura objetivo

```
┌─────────────────────── ironclaw process ───────────────────────────┐
│                                                                     │
│  src/audio/                                                         │
│   ├── pipeline.rs ─── TtsAudioPipeline (existente)                  │
│   ├── analysis.rs ─── analyze_pcm RMS+FFT (existente)               │
│   ├── types.rs    ─── TtsBackend trait (existente)                  │
│   └── backends/                                                     │
│        ├── none.rs                                                  │
│        └── elevenlabs_local.rs ── shim TtsBackend → VoiceHandle    │
│                                                                     │
│           ▲                                                         │
│           │ PcmFrame broadcast                                      │
│           │                                                         │
│  crates/jarvis_voice/    (nuevo)                                    │
│   ├── lib.rs      ─── VoiceEngine, VoiceHandle, VoiceConfig         │
│   ├── elevenlabs/ ─── WS client (era ws_client.rs + protocol.rs)    │
│   ├── audio_io/   ─── cpal mic + speaker (era audio.rs)             │
│   ├── aec.rs      ─── webrtc-audio-processing wrapper               │
│   ├── resample.rs ─── rubato 16k ↔ device rate                      │
│   ├── orchestrator.rs── tie WS + IO + AEC + barge-in                │
│   ├── config.rs   ─── VoiceConfig::from_env() — única fuente envs   │
│   ├── types.rs    ─── PcmFrame, VoiceEvent, ConversationId, ...     │
│   └── error.rs                                                      │
│                                                                     │
│  src/app.rs ─── arranca VoiceEngine cuando AudioConfig.enabled      │
└─────────────────────────────────────────────────────────────────────┘

Pipeline interno de jarvis_voice:

  [cpal mic cb] ─► [resample → 16k] ─► [AEC.process_stream(near)] ─► [WS Outbound::Audio]
                                                ▲
                                                │ far-end alineado (delay-compensated)
                                                │
  [WS Inbound::AgentAudio] ─► [AEC.process_reverse_stream(far)] ─► [resample → device]
                                          │                                     │
                                          └─► [PcmFrame broadcast]              └─► [cpal speaker cb]
                                                       │
                                                       └─► (a TtsAudioPipeline en core para orb)
```

Diferencias clave vs F3b:

- **AEC explícito**: mic frame pasa por `AudioProcessing::process_stream`, far-end alineado por `process_reverse_stream`. Eliminamos la dependencia del módulo PipeWire echo-cancel.
- **Sin IPC PCM**: la PCM va directo al broadcast in-process. `ipc_publisher.rs`, `ClientCommand::TtsPcmFrame`, dispatch de frame routing — todo se borra.
- **Resampler real**: `rubato` (polyphase) reemplaza el resampler por decimación lineal actual. Necesario para que el AEC adaptive filter converja.
- **VoiceEngine es task de tokio**: arranca/para con el resto de IronClaw. cpal sigue usando sus threads propios; bridgeamos via `mpsc`.

## 5. Tipos públicos del crate

Todos los tipos siguen `.claude/rules/types.md` (newtype canónico, no `String` suelto en cruces de módulo).

### `crates/jarvis_voice/Cargo.toml` (deps)

```toml
[package]
name = "jarvis_voice"
version = "0.1.0"
edition = "2024"

[dependencies]
# Audio I/O
cpal = "0.15"
ringbuf = "0.4"

# WS client
tokio-tungstenite = { version = "0.24", features = ["rustls-tls-webpki-roots"] }
tokio = { workspace = true, features = ["full"] }
futures-util = "0.3"

# AEC + resampling (nuevos)
webrtc-audio-processing = "0.5"   # cmake + clang + libstdc++ en build
rubato = "0.16"

# Wire / serde
serde = { workspace = true, features = ["derive"] }
serde_json = { workspace = true }
base64 = "0.22"

# Logging / errores
tracing = { workspace = true }
thiserror = { workspace = true }
anyhow = { workspace = true }
```

### `lib.rs` — superficie pública

```rust
pub use config::VoiceConfig;
pub use engine::{VoiceEngine, VoiceHandle};
pub use error::VoiceError;
pub use types::{PcmFrame, VoiceEvent, ConversationId, SampleRate, InterruptionReason};

mod aec;
mod audio_io;
mod config;
mod elevenlabs;
mod engine;
mod error;
mod orchestrator;
mod resample;
mod types;
```

### `types.rs`

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct ConversationId(String);
// Validación: no empty, ASCII alfanumérico + guion bajo + guion, longitud 1..=128.
// Construcción canónica: ConversationId::new(s) o serde::TryFrom<String>.

#[derive(Debug, Clone)]
pub struct PcmFrame {
    pub samples: Arc<[i16]>,    // mono
    pub sample_rate: SampleRate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SampleRate(u32);

impl SampleRate {
    pub const ELEVENLABS: Self = SampleRate(16_000);
    pub fn new(hz: u32) -> Result<Self, VoiceError>; // valida 8000/16000/22050/44100/48000
    pub fn hz(self) -> u32 { self.0 }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InterruptionReason {
    User,
    Server,
    Unknown,
}
```

### `engine.rs`

```rust
pub struct VoiceEngine;

impl VoiceEngine {
    /// Arranca cpal I/O + WS + AEC. Devuelve handle para enviar control y suscribirse a eventos.
    pub async fn start(cfg: VoiceConfig) -> Result<VoiceHandle, VoiceError>;
}

pub struct VoiceHandle {
    events_rx: broadcast::Receiver<VoiceEvent>,
    stop_tx:   mpsc::Sender<StopSignal>,
    tool_tx:   mpsc::Sender<ToolCallResult>,
}

impl VoiceHandle {
    pub fn subscribe(&self) -> broadcast::Receiver<VoiceEvent>;
    pub async fn stop(self) -> Result<(), VoiceError>;
    pub async fn send_tool_result(&self, result: ToolCallResult) -> Result<(), VoiceError>;
}
```

### `error.rs`

`VoiceError` con `thiserror`, sin `unwrap`/`expect` en código de producción. Boundaries:

- WS errores → `VoiceError::Transport(String)` (mapeado, sin exponer 5xx codes raw al caller — sigue `.claude/rules/error-handling.md` "Error Boundaries").
- cpal errores → `VoiceError::AudioDevice(String)`.
- AEC init errores → `VoiceError::AecInit(String)`.
- Validation → `VoiceError::Validation(String)`.

### `src/audio/backends/elevenlabs_local.rs` (shim en core)

```rust
pub struct ElevenLabsLocalBackend {
    handle: Arc<VoiceHandle>,
    pcm_tx: broadcast::Sender<core_audio::PcmFrame>,
}

impl TtsBackend for ElevenLabsLocalBackend {
    fn subscribe_pcm(&self) -> broadcast::Receiver<core_audio::PcmFrame> {
        self.pcm_tx.subscribe()
    }
}

impl ElevenLabsLocalBackend {
    pub async fn start(cfg: AudioConfig) -> Result<Self, AudioError> {
        let handle = VoiceEngine::start(VoiceConfig::from(cfg)).await?;
        let pcm_tx = broadcast::channel(64).0;
        let mut events = handle.subscribe();
        let pcm_tx_task = pcm_tx.clone();
        tokio::spawn(async move {
            while let Ok(evt) = events.recv().await {
                if let VoiceEvent::AgentAudio(frame) = evt {
                    let _ = pcm_tx_task.send(frame.into());
                }
            }
        });
        Ok(Self { handle: Arc::new(handle), pcm_tx })
    }
}
```

## 6. Lo que se borra (no dead code)

| Path | Motivo | Commit |
|------|--------|--------|
| `crates/jarvis_voice_daemon/` (todo el crate) | absorbido | B2 |
| `src/audio/backends/elevenlabs_ipc.rs` | reemplazado por `elevenlabs_local.rs` | B2/B4 |
| `src/channels/local_ipc/protocol.rs::ClientCommand::TtsPcmFrame` | sin productor | B4 |
| `src/channels/local_ipc/channel_impl.rs` rama `TtsPcmFrame` en `dispatch_command` | sin productor | B4 |
| `Option<Arc<ElevenLabsIpcBackend>>` threading en `app.rs` y `local_ipc/mod.rs::create()` | sin productor | B4 |
| `arch/configs/systemd/jarvis-voice-daemon.service` | sin binario | B2 |
| `arch/configs/pipewire/echo-cancel.conf` | sin consumidor (AEC ahora in-process) | B3 |
| install scripts entries para echo-cancel | mismo | B3 |
| `crates/jarvis_voice/src/spawn.rs` (placeholder de B1) | reemplazado por in-process | B2 |

## 7. Plan de migración M2 (commit-by-commit)

Cada commit deja el sistema funcional en Asus. Branch `jarvis-arch-os`.

### B1 — `feat(voice)[F4/B1]: scaffold crates/jarvis_voice as daemon launcher`

Estado: el binario `jarvis-voice-daemon` sigue existiendo y haciendo todo. El crate nuevo es un wrapper que arranca el binario como subproceso hijo.

- Crear `crates/jarvis_voice/` con `lib.rs`, `engine.rs`, `config.rs`, `error.rs`, `types.rs`.
- `VoiceEngine::start` hace `tokio::process::Command::new("jarvis-voice-daemon")` con args + env del config.
- `VoiceHandle` mantiene el `Child` y un task que lee stderr para detectar startup OK.
- En core: `src/audio/backends/elevenlabs_local.rs` nuevo, usa `VoiceEngine` pero internamente sigue cableado al socket UNIX para recibir PcmFrame (se conecta como cliente IPC al daemon hijo).
- `app.rs` switchea de `ElevenLabsIpcBackend` a `ElevenLabsLocalBackend` detrás de feature flag `voice-in-process` (default off).
- systemd unit `jarvis-voice-daemon.service` se mantiene; los usuarios que no activen feature flag siguen igual.

Validación Asus: `cargo build --features voice-in-process` + `systemctl --user stop jarvis-voice-daemon` + `systemctl --user restart ironclaw` → conversación funciona, orbe reacciona.

Throwaway aceptable: el subprocess launcher se borra en B2. Justifica como paso de migración, no estado final.

### B2 — `feat(voice)[F4/B2]: in-process cpal + ElevenLabs WS`

Estado: `VoiceEngine` ya no spawnea proceso. Hace cpal + WS in-process. AEC todavía es de PipeWire (no tocamos echo-cancel.conf).

- Mover `audio.rs` → `crates/jarvis_voice/src/audio_io/mod.rs`. Mantener `pick_input_device` con preferencia `jarvis-mic-aec` (legacy AEC sigue funcionando).
- Mover `ws_client.rs` → `crates/jarvis_voice/src/elevenlabs/mod.rs`. Mover `protocol.rs` → `crates/jarvis_voice/src/elevenlabs/protocol.rs`.
- Reescribir `orchestrator.rs` → `crates/jarvis_voice/src/orchestrator.rs` con la API nueva: bucle tokio que tie audio_io + WS + emite `VoiceEvent` por broadcast.
- `VoiceEngine::start` arranca el orchestrator como tokio task.
- Borrar feature flag `voice-in-process` (ahora es el path único).
- Borrar `crates/jarvis_voice_daemon/` completo (incluye `ipc_publisher.rs`, `ws_client.rs`, etc., todos absorbidos arriba).
- Borrar `crates/jarvis_voice/src/spawn.rs` (el launcher de B1).
- Borrar `arch/configs/systemd/jarvis-voice-daemon.service`.
- Borrar `src/audio/backends/elevenlabs_ipc.rs` (reemplazado por `elevenlabs_local.rs` en B1).
- Actualizar `Cargo.toml` workspace members (quitar `jarvis_voice_daemon`, añadir `jarvis_voice`).

Validación Asus: `systemctl --user disable --now jarvis-voice-daemon`, `systemctl --user restart ironclaw`. Conversación funciona end-to-end. Orbe reacciona. Bug del feedback de eco *sigue* (lo arregla B3).

### B3 — `feat(voice)[F4/B3]: WebRTC AEC + rubato resample`

Estado: AEC propio funcionando. El bug del feedback loop debe desaparecer.

- Añadir `webrtc-audio-processing = "0.5"` y `rubato = "0.16"` al Cargo.toml del crate.
- Crear `crates/jarvis_voice/src/aec.rs`:
  - Wrapper `AecProcessor` que envuelve `webrtc_audio_processing::Processor`.
  - Configura: AEC3 enabled, NS aggressive, AGC adaptive, VAD off (Convai hace su propio turn detection).
  - Frame size: 10ms @ 16kHz = 160 samples.
- Crear `crates/jarvis_voice/src/resample.rs`:
  - `MicResampler` (device_rate → 16k mono, polyphase via `rubato::FftFixedIn`).
  - `SpeakerResampler` (16k mono → device_rate, polyphase via `rubato::FftFixedOut`).
- Modificar `audio_io::mic` y `audio_io::speaker` para emitir/aceptar frames de 10ms (160 samples @ 16k) en lugar de 50ms.
- Modificar `orchestrator.rs`:
  - Sequence: `mic_frame_raw → resample → AEC.process_stream(near) → ws.send(Outbound::Audio)`.
  - En recv loop: `agent_audio → AEC.process_reverse_stream(far) → resample → speaker.play`.
  - Las llamadas `process_reverse_stream` y `process_stream` deben respetar el orden temporal (reverse antes que stream para el mismo "tick").
  - `set_stream_delay_ms` con default 50ms; expuesto vía env `JARVIS_VOICE_AEC_DELAY_MS` para tunning.
- Simplificar `pick_input_device` y `pick_output_device`: ahora abren el default device sin esquivar `jarvis-mic-aec` ni `jarvis-aec`.
- Borrar `arch/configs/pipewire/echo-cancel.conf` y referencias en install scripts.

Validación Asus: instalar (sin recargar PipeWire echo-cancel module), conversar, verificar que ya NO se interrumpe sola. Speakerphone abierto, mic libre.

### B4 — `chore(ipc)[F4/B4]: drop TtsPcmFrame from local_ipc protocol`

Estado: limpieza de la deuda técnica de F3b. Ningún productor usa `TtsPcmFrame` después de B2.

- Borrar variant `ClientCommand::TtsPcmFrame` de `src/channels/local_ipc/protocol.rs`.
- Borrar rama de dispatch en `src/channels/local_ipc/channel_impl.rs::dispatch_command`.
- Borrar threading de `Option<Arc<ElevenLabsIpcBackend>>` en `app.rs` y `channels/local_ipc/mod.rs::create()`.
- Borrar `src/audio/backends/elevenlabs_ipc.rs` si no se borró ya en B2.
- Actualizar tests de local_ipc (~3 tests que mencionan TtsPcmFrame).
- Si `reqwest` queda sin usuarios en `crates/jarvis_voice/Cargo.toml`, removerlo.

Validación Asus: `cargo test --lib local_ipc` 38/38 ✅, `cargo test --lib audio` 21/21 ✅, conversación end-to-end OK.

### B5 — `docs(voice)[F4/B5]: update CLAUDE.md, project structure, memory`

- Actualizar `CLAUDE.md` "Project Structure" — quitar `jarvis_voice_daemon`, añadir `jarvis_voice`.
- Actualizar este spec con "Implementation status: shipped commits SHAs" en el front matter.

### Invariantes M2

- B1 → sistema en producción usa subprocess (placeholder); flag opt-in.
- B2 → sistema en producción usa in-process; AEC viejo (PipeWire echo-cancel) sigue.
- B3 → AEC propio activado; bug raíz resuelto.
- B4 → limpieza de deuda IPC.
- B5 → docs.

Cada uno mergeable individualmente. Si B3 falla, el sistema sigue funcionando con B2 (peor calidad de AEC pero conversación intacta).

## 8. Riesgos

### R1 — `webrtc-audio-processing` no compila en Asus

**Probabilidad:** media. **Impacto:** alto (B3 bloqueado).

Mitigación:
- Validación temprana: `cargo add webrtc-audio-processing && cargo check -p jarvis_voice` antes de empezar B3 (~10 min).
- Si bundled feature falla: probar con system lib (AUR `webrtc-audio-processing`).
- Plan B: caer a `webrtc-audio-processing-sys` y escribir wrapper a mano (~50 líneas).
- Plan C: `speex-dsp-rs` con calidad inferior, decisión documentada.

### R2 — cpal threads + tokio runtime fricción

**Probabilidad:** baja. **Impacto:** medio.

Mitigación:
- Mantener `try_send` en mic callback.
- `mpsc::channel(64)` capacity (~3.2s buffer @ 50ms chunks).
- AEC corre en task tokio dedicado, no en cpal callback.

### R3 — AEC reverse stream alineamiento temporal

**Probabilidad:** media. **Impacto:** alto (AEC no converge).

Mitigación:
- `set_stream_delay_ms = 50` default.
- Reverse stream invocado en cuanto llega el WS frame, antes de pasar al speaker.
- Env var `JARVIS_VOICE_AEC_DELAY_MS` para tunear sin recompilar.

### R4 — Resampling 48k→16k introduce artifacts

**Probabilidad:** baja. **Impacto:** bajo.

Mitigación: `rubato` polyphase es alta calidad, mejora sobre el resampler decimación-lineal actual.

### R5 — `ToolCall` sigue siendo placeholder

**Probabilidad:** alta. **Impacto:** medio (UX).

Mitigación: explícito en spec que F4 NO cablea tool calls. Ese es F5. El placeholder se conserva idéntico — solo cambia de archivo.

### R6 — WS conecta al startup (consume créditos sin uso)

**Probabilidad:** baja. **Impacto:** bajo.

Mitigación: ninguna en F4. Lazy connection es trabajo de F5+.

### R7 — Settings/config split entre daemon y core

**Probabilidad:** alta. **Impacto:** bajo.

Mitigación: en B2, mover el parsing de envs a `crates/jarvis_voice/src/config.rs` con `VoiceConfig::from_env()`. `src/config/audio.rs` (core) llama a esa función al construir `AudioConfig`. Una sola fuente de verdad.

### R8 — Bug del feedback loop NO resuelto post-B3

**Probabilidad:** baja-media. **Impacto:** alto (objetivo principal).

Mitigación:
- AEC3 + NS aggressive + AGC adaptive es preset Google Meet — debe rendir bien.
- Si no resuelve: combinar AEC3 + VAD propia para mutear mic durante TTS playback. Trabajo de F4.1, no rollback de F4.
- Validación temprana: smoke test con frames sintéticos en `crates/jarvis_voice/tests/aec_smoke.rs`.

## 9. Scope NO incluido

| Item | Razón | Cuándo |
|------|-------|--------|
| Cablear tool calls de Convai → `ToolDispatcher` de IronClaw | Es trabajo grande con su propia spec (security, async wait, multi-turn) | F5 — "voice tool dispatch" |
| Reemplazar Convai por TTS+STT+LLM separados | F4 = mismo comportamiento, distinto deployment | F6+ si decidimos pivot |
| Push-to-talk / wake word | El mic está siempre abierto, igual que hoy | F7+ |
| Lazy WS connection | Cambio de UX, no de arquitectura | F5+ |
| Piper / Kokoro local TTS backend | El `TtsBackend` trait queda preparado pero no se implementa otro backend | Trabajo independiente |
| WebRTC AEC tunning fino (delay calibration runtime) | B3 ships con valores razonables | F4.1 si hace falta |
| Audio routing policy (qué device usar cuando hay headphones) | Hoy es el default de cpal | F4.2 |
| Telemetría de calidad de AEC (ERLE, double-talk detection) | Útil para debug pero no bloquea | F4.2 |

## 10. Criterios de éxito

F4 está cerrado cuando TODOS estos son ciertos en Asus:

1. `crates/jarvis_voice_daemon/` no existe (`git ls-files` lo confirma).
2. `systemctl --user status jarvis-voice-daemon` → "Unit not found".
3. `ps aux | grep jarvis-voice` → solo procesos `ironclaw`.
4. `cargo test --lib audio` → 21/21 ✅, sin tests modificados.
5. `cargo test --lib local_ipc` → tests verdes con TtsPcmFrame removido.
6. `cargo test -p jarvis_voice` → tests unitarios del crate verdes (incluye smoke test de AEC).
7. Conversación end-to-end con jarvis funciona en Asus, sin recargar PipeWire echo-cancel module previamente.
8. Bug del feedback loop reportado en F3b (jarvis se interrumpe sola) NO ocurre durante 5 min de prueba con speakerphone abierto.
9. Orbe sigue reaccionando al audio TTS — cero regresión visual desde F3b/B5.
10. `pactl list modules | grep echo-cancel` → vacío.
11. Este spec actualizado con "Implementation status: shipped + commits SHAs".
12. Memory `project_resume_2026_05_02_*` actualizada con cierre de F4.

## 11. Tamaño estimado

| Commit | Líneas nuevas | Líneas modificadas | Líneas borradas | Esfuerzo |
|--------|---------------|--------------------|-----------------| ---------|
| B1 | ~250 | ~50 | 0 | 3 h |
| B2 | ~200 | ~150 (mover) | ~1500 (daemon) | 5 h + 1h validación |
| B3 | ~400 | ~200 (orchestrator) | ~50 (pick_*) | 6 h + 2h validación + tunning |
| B4 | 0 | ~30 (tests) | ~100 | 1 h |
| B5 | ~30 | ~20 | 0 | 30 min |

**Total neto:** 16-20 h. Calendario: 2-3 días.

## 12. Cuestiones abiertas que NO bloquean implementación

- `ConversationId` typed con `validate` según types.md (resuelto: typed, no transparent).
- `reqwest` se borra del crate si no hay consumer post-B4 (resuelto: borrar).
- `aec_smoke.rs` usa frames sintéticos: seno 440Hz como far-end, mismo seno con delay artificial 50ms + atenuación como near-end; verifica que ERLE > 10dB después de 1s de adaptación.
