//! Orquestador end-to-end.
//!
//! Combina audio I/O y cliente WS:
//! - Mic chunks → WS outbound (audio del usuario al agente).
//! - WS inbound audio → speaker.
//! - WS inbound interruption → flush del speaker buffer (barge-in).
//! - WS inbound ping → outbound pong.
//! - WS inbound tool_call → log + respuesta no-op (placeholder hasta
//!   cabledar IronClaw como tool provider).

use crate::audio;
use crate::config::Config;
use crate::protocol::{ClientToolCall, ClientToolResult};
use crate::ws_client::{Inbound, Outbound, connect};
use anyhow::Result;

pub async fn run(cfg: Config) -> Result<()> {
    // Audio.
    let audio_io = audio::start()?;
    let audio::AudioIo {
        mut mic_rx,
        speaker_tx,
        ..
    } = audio_io;

    // WebSocket.
    let mut ws = connect(&cfg).await?;
    let outbound_tx = ws.outbound_tx.clone();

    // Forwarder mic → ws outbound.
    let outbound_clone = outbound_tx.clone();
    tokio::spawn(async move {
        while let Some(chunk) = mic_rx.recv().await {
            if outbound_clone.send(Outbound::Audio(chunk)).await.is_err() {
                tracing::info!("orchestrator.mic_outbound_channel_closed");
                break;
            }
        }
    });

    // Señal de stop (Ctrl+C, SIGTERM).
    let stop_signal = tokio::signal::ctrl_c();
    tokio::pin!(stop_signal);

    loop {
        tokio::select! {
            _ = &mut stop_signal => {
                tracing::info!("orchestrator.stop_signal_received");
                let _ = outbound_tx.send(Outbound::Stop).await;
                break;
            }
            evt = ws.inbound_rx.recv() => {
                match evt {
                    Some(Inbound::AgentAudio(pcm)) => {
                        speaker_tx.play(pcm);
                    }
                    Some(Inbound::UserTranscript(text)) => {
                        tracing::info!(speaker = "user", text = %text, "transcript");
                    }
                    Some(Inbound::AgentResponse(text)) => {
                        tracing::info!(speaker = "agent", text = %text, "transcript");
                    }
                    Some(Inbound::AgentResponseCorrection { original, corrected }) => {
                        tracing::info!(
                            speaker = "agent",
                            original = %original,
                            corrected = %corrected,
                            "transcript_correction"
                        );
                    }
                    Some(Inbound::Interruption { event_id, reason }) => {
                        tracing::info!(
                            event_id,
                            reason = ?reason,
                            "agent.interrupted_by_user"
                        );
                        speaker_tx.flush();
                    }
                    Some(Inbound::Ping { event_id }) => {
                        let _ = outbound_tx.send(Outbound::Pong { event_id }).await;
                    }
                    Some(Inbound::ToolCall(call)) => {
                        let result = handle_tool_call(call).await;
                        let _ = outbound_tx.send(Outbound::ToolResult(result)).await;
                    }
                    Some(Inbound::Connected { conversation_id }) => {
                        tracing::info!(%conversation_id, "session.ready");
                    }
                    Some(Inbound::Disconnected) => {
                        tracing::info!("session.disconnected");
                        break;
                    }
                    None => break,
                }
            }
        }
    }

    Ok(())
}

/// Placeholder: el agente puede llamar tools; aún no las cabledamos a
/// IronClaw. Respondemos con un mensaje informativo para que el agente
/// pueda continuar sin colgarse.
async fn handle_tool_call(call: ClientToolCall) -> ClientToolResult {
    tracing::info!(
        tool_name = %call.tool_name,
        tool_call_id = %call.tool_call_id,
        params = %call.parameters,
        "tool_call.received"
    );
    ClientToolResult::ok(
        call.tool_call_id,
        format!(
            "Tool '{}' aún no cabledada en jarvis-os. Cableado a IronClaw planificado para una iteración posterior.",
            call.tool_name
        ),
    )
}
