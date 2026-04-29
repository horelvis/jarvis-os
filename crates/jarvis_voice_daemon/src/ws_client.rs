//! Cliente WebSocket de ElevenLabs Conversational AI.

use anyhow::{Context, Result, anyhow};
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::header::{HeaderName, HeaderValue};
use tokio_tungstenite::tungstenite::protocol::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};

use crate::config::Config;
use crate::protocol::{
    ClientToolResult, ConversationInitiation, Pong, ServerMessage, UserAudioChunk,
};

pub type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

const ENDPOINT: &str = "wss://api.elevenlabs.io/v1/convai/conversation";
const HEADER_API_KEY: &str = "xi-api-key";

/// Mensajes que el orquestador envía al cliente WS.
#[derive(Debug)]
pub enum Outbound {
    Audio(Vec<i16>),
    Pong { event_id: u64 },
    ToolResult(ClientToolResult),
    Stop,
}

/// Eventos que el cliente WS emite hacia el orquestador.
#[derive(Debug)]
pub enum Inbound {
    AgentAudio(Vec<i16>),
    UserTranscript(String),
    AgentResponse(String),
    AgentResponseCorrection { original: String, corrected: String },
    Interruption { event_id: u64, reason: Option<String> },
    Ping { event_id: u64 },
    ToolCall(crate::protocol::ClientToolCall),
    Connected { conversation_id: String },
    Disconnected,
}

pub struct WsClient {
    pub outbound_tx: mpsc::Sender<Outbound>,
    pub inbound_rx: mpsc::Receiver<Inbound>,
}

/// Conecta al endpoint WS y arranca dos tasks (read/write) que median
/// con el orquestador vía canales mpsc.
pub async fn connect(cfg: &Config) -> Result<WsClient> {
    let url = format!("{ENDPOINT}?agent_id={}", cfg.agent_id);
    let mut request = url
        .as_str()
        .into_client_request()
        .context("building ws request")?;
    request.headers_mut().insert(
        HeaderName::from_static(HEADER_API_KEY),
        HeaderValue::from_str(&cfg.api_key).context("api_key contains invalid header bytes")?,
    );

    tracing::info!(
        agent_id = %cfg.agent_id_redacted(),
        "ws.connecting"
    );
    let (ws, response) = connect_async(request).await.context("ws.connect_async")?;
    tracing::info!(status = %response.status(), "ws.connected");

    let (outbound_tx, outbound_rx) = mpsc::channel::<Outbound>(64);
    let (inbound_tx, inbound_rx) = mpsc::channel::<Inbound>(64);

    // Mensaje inicial — overrides opcionales del system prompt y
    // variables dinámicas que el agente puede requerir.
    let init = ConversationInitiation::new(
        cfg.system_prompt_override.clone(),
        cfg.dynamic_variables.clone(),
    );
    let init_json = serde_json::to_string(&init).context("serialize init")?;

    let (mut sink, mut stream) = ws.split();
    sink.send(Message::Text(init_json.into()))
        .await
        .context("send init")?;

    // Task: outbound — lee del canal y envía por el WS.
    let outbound_task = tokio::spawn(async move {
        let mut rx = outbound_rx;
        while let Some(msg) = rx.recv().await {
            match msg {
                Outbound::Audio(pcm) => {
                    let bytes: Vec<u8> = pcm
                        .iter()
                        .flat_map(|s| s.to_le_bytes())
                        .collect();
                    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                    let chunk = UserAudioChunk { user_audio_chunk: &b64 };
                    let json = match serde_json::to_string(&chunk) {
                        Ok(j) => j,
                        Err(e) => {
                            tracing::warn!(error = %e, "ws.audio_chunk_serialize_failed");
                            continue;
                        }
                    };
                    if let Err(e) = sink.send(Message::Text(json.into())).await {
                        tracing::warn!(error = %e, "ws.audio_send_failed");
                        break;
                    }
                }
                Outbound::Pong { event_id } => {
                    let pong = Pong::new(event_id);
                    let json = serde_json::to_string(&pong).unwrap_or_default();
                    if let Err(e) = sink.send(Message::Text(json.into())).await {
                        tracing::warn!(error = %e, "ws.pong_send_failed");
                        break;
                    }
                }
                Outbound::ToolResult(result) => {
                    let json = match serde_json::to_string(&result) {
                        Ok(j) => j,
                        Err(e) => {
                            tracing::warn!(error = %e, "ws.tool_result_serialize_failed");
                            continue;
                        }
                    };
                    if let Err(e) = sink.send(Message::Text(json.into())).await {
                        tracing::warn!(error = %e, "ws.tool_result_send_failed");
                        break;
                    }
                }
                Outbound::Stop => {
                    let _ = sink.send(Message::Close(None)).await;
                    break;
                }
            }
        }
    });

    // Task: inbound — lee del WS y empuja por el canal.
    let inbound_tx_clone = inbound_tx.clone();
    let inbound_task = tokio::spawn(async move {
        while let Some(frame) = stream.next().await {
            match frame {
                Ok(Message::Text(text)) => {
                    handle_text(text.as_str(), &inbound_tx_clone).await;
                }
                Ok(Message::Binary(_)) => {
                    tracing::debug!("ws.unexpected_binary_frame");
                }
                Ok(Message::Close(frame)) => {
                    tracing::info!(?frame, "ws.close_received");
                    break;
                }
                Ok(Message::Ping(p)) => {
                    tracing::trace!(len = p.len(), "ws.ping_frame");
                }
                Ok(Message::Pong(_)) => {}
                Ok(Message::Frame(_)) => {}
                Err(e) => {
                    tracing::warn!(error = %e, "ws.recv_error");
                    break;
                }
            }
        }
        let _ = inbound_tx_clone.send(Inbound::Disconnected).await;
    });

    // Detached: las tasks corren hasta que el WS muera por sí mismo
    // (cliente cierra outbound_tx → outbound termina; server cierra
    // socket → inbound termina). No esperamos sus handles aquí.
    let _ = outbound_task;
    let _ = inbound_task;

    Ok(WsClient {
        outbound_tx,
        inbound_rx,
    })
}

async fn handle_text(text: &str, tx: &mpsc::Sender<Inbound>) {
    let parsed: Result<ServerMessage, _> = serde_json::from_str(text);
    let event = match parsed {
        Ok(e) => e,
        Err(e) => {
            tracing::debug!(error = %e, snippet = %&text[..text.len().min(200)], "ws.unknown_message");
            return;
        }
    };

    match event {
        ServerMessage::ConversationInitiationMetadata { conversation_initiation_metadata_event: m } => {
            tracing::info!(
                conversation_id = %m.conversation_id,
                input_format = %m.user_input_audio_format,
                output_format = %m.agent_output_audio_format,
                "ws.conversation_initiated"
            );
            let _ = tx.send(Inbound::Connected { conversation_id: m.conversation_id }).await;
        }
        ServerMessage::Audio { audio_event } => {
            match decode_audio(&audio_event.audio_base_64) {
                Ok(pcm) => {
                    let _ = tx.send(Inbound::AgentAudio(pcm)).await;
                }
                Err(e) => {
                    tracing::warn!(error = %e, "ws.audio_decode_failed");
                }
            }
        }
        ServerMessage::UserTranscript { user_transcription_event } => {
            let _ = tx
                .send(Inbound::UserTranscript(user_transcription_event.user_transcript))
                .await;
        }
        ServerMessage::AgentResponse { agent_response_event } => {
            let _ = tx
                .send(Inbound::AgentResponse(agent_response_event.agent_response))
                .await;
        }
        ServerMessage::AgentResponseCorrection { agent_response_correction_event: c } => {
            let _ = tx
                .send(Inbound::AgentResponseCorrection {
                    original: c.original_agent_response,
                    corrected: c.corrected_agent_response,
                })
                .await;
        }
        ServerMessage::Interruption { interruption_event } => {
            let _ = tx
                .send(Inbound::Interruption {
                    event_id: interruption_event.event_id,
                    reason: interruption_event.reason,
                })
                .await;
        }
        ServerMessage::Ping { ping_event } => {
            let _ = tx.send(Inbound::Ping { event_id: ping_event.event_id }).await;
        }
        ServerMessage::ClientToolCall { client_tool_call } => {
            let _ = tx.send(Inbound::ToolCall(client_tool_call)).await;
        }
        ServerMessage::Other => {
            // Eventos internos / nuevos — ignorar silencioso.
        }
    }
}

fn decode_audio(b64: &str) -> Result<Vec<i16>> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .context("base64 decode")?;
    if bytes.len() % 2 != 0 {
        return Err(anyhow!("audio bytes not aligned to i16: {}", bytes.len()));
    }
    let mut pcm = Vec::with_capacity(bytes.len() / 2);
    for chunk in bytes.chunks_exact(2) {
        pcm.push(i16::from_le_bytes([chunk[0], chunk[1]]));
    }
    Ok(pcm)
}
