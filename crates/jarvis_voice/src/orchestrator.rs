//! Orquestador in-process.
//!
//! Reemplaza `jarvis_voice_daemon::orchestrator::run`. Tie audio_io + WS
//! y emite `VoiceEvent` por un broadcast::Sender que provee
//! `VoiceEngine::start`. La diferencia clave con el daemon legacy es
//! que NO publica por IPC — el shim `ElevenLabsLocalBackend` se
//! suscribe directamente al broadcast.

use crate::audio_io::{self, AudioIo};
use crate::config::VoiceConfig;
use crate::elevenlabs::protocol::{ClientToolCall, ClientToolResult};
use crate::elevenlabs::{self, Inbound, Outbound};
use crate::error::VoiceError;
use crate::types::{
    ConversationId, InterruptionReason, PcmFrame, SampleRate, ToolCallRequest, ToolCallResult,
    VoiceEvent,
};
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc};

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

        // Forwarder mic → ws outbound. Spawned aparte para no monopolizar
        // el select del loop principal con cada chunk PCM (50 ms tick).
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
                            // Placeholder mientras F5 no cabledea ToolDispatcher
                            // — devuelve mensaje informativo para que el
                            // agente pueda continuar sin colgarse.
                            let result = placeholder_tool_result(call);
                            let _ = outbound_tx.send(Outbound::ToolResult(result)).await;
                        }
                        Some(Inbound::Connected { conversation_id }) => {
                            match ConversationId::new(conversation_id) {
                                Ok(id) => {
                                    let _ = self.events_tx.send(VoiceEvent::Connected {
                                        conversation_id: id,
                                    });
                                }
                                Err(e) => {
                                    tracing::debug!(
                                        error = %e,
                                        "orchestrator.invalid_conversation_id"
                                    );
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
