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
