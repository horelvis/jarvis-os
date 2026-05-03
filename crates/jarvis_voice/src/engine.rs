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
