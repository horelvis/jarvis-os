//! Protocolo WebSocket de ElevenLabs Conversational AI.
//!
//! Los mensajes al server son JSON. El servidor responde con un stream
//! de mensajes JSON. Tres particularidades a tener en cuenta:
//!
//! - El audio del usuario va en un mensaje **sin** campo `type`, sólo
//!   con `user_audio_chunk` (string base64). Por eso definimos un
//!   tipo aparte `UserAudioChunk`.
//! - Los eventos del servidor anidan los datos bajo un campo cuyo nombre
//!   varía (`audio_event`, `user_transcription_event`, etc.). Lo
//!   reflejamos en las variantes de [`ServerMessage`].
//! - Algunos campos opcionales se omiten cuando no se envían — usamos
//!   `Option<T>` con `skip_serializing_if`.
//!
//! Referencia: https://elevenlabs.io/docs/conversational-ai/api-reference/conversational-ai/websocket

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Mensaje inicial. Lleva opcionalmente un override del prompt/voice y
/// un mapa de `dynamic_variables` que el agente requiere para resolver
/// los `{{placeholders}}` declarados en su system prompt en consola.
#[derive(Debug, Serialize)]
pub struct ConversationInitiation {
    #[serde(rename = "type")]
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_config_override: Option<ConfigOverride>,
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub dynamic_variables: std::collections::BTreeMap<String, String>,
}

impl ConversationInitiation {
    pub fn new(
        prompt_override: Option<String>,
        dynamic_variables: std::collections::BTreeMap<String, String>,
    ) -> Self {
        let override_block = prompt_override.map(|prompt| ConfigOverride {
            agent: Some(AgentOverride {
                prompt: Some(PromptOverride { prompt }),
            }),
        });
        Self {
            kind: "conversation_initiation_client_data",
            conversation_config_override: override_block,
            dynamic_variables,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ConfigOverride {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<AgentOverride>,
}

#[derive(Debug, Serialize)]
pub struct AgentOverride {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<PromptOverride>,
}

#[derive(Debug, Serialize)]
pub struct PromptOverride {
    pub prompt: String,
}

/// Audio del usuario: PCM 16kHz int16 mono codificado en base64.
/// El server espera chunks de ~50ms (1280 samples = 2560 bytes).
#[derive(Debug, Serialize)]
pub struct UserAudioChunk<'a> {
    pub user_audio_chunk: &'a str,
}

/// Pong de respuesta a un `ping` del servidor.
#[derive(Debug, Serialize)]
pub struct Pong {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub event_id: u64,
}

impl Pong {
    pub fn new(event_id: u64) -> Self {
        Self {
            kind: "pong",
            event_id,
        }
    }
}

/// Resultado de una tool del cliente — mandado de vuelta al server tras
/// ejecutar la acción que el agente pidió.
#[derive(Debug, Serialize)]
pub struct ClientToolResult {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub tool_call_id: String,
    pub result: String,
    pub is_error: bool,
}

impl ClientToolResult {
    pub fn ok(tool_call_id: String, result: String) -> Self {
        Self {
            kind: "client_tool_result",
            tool_call_id,
            result,
            is_error: false,
        }
    }
    pub fn error(tool_call_id: String, error: String) -> Self {
        Self {
            kind: "client_tool_result",
            tool_call_id,
            result: error,
            is_error: true,
        }
    }
}

/// Mensajes desde el servidor.
///
/// Las variantes "internas" (tentative_agent_response, etc.) se mapean
/// como `Other` para no fallar el deserialize cuando ElevenLabs añada
/// nuevos eventos en versiones futuras.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    ConversationInitiationMetadata {
        conversation_initiation_metadata_event: ConversationInitiationMetadataEvent,
    },
    Audio {
        audio_event: AudioEvent,
    },
    UserTranscript {
        user_transcription_event: UserTranscriptionEvent,
    },
    AgentResponse {
        agent_response_event: AgentResponseEvent,
    },
    AgentResponseCorrection {
        agent_response_correction_event: AgentResponseCorrectionEvent,
    },
    Interruption {
        interruption_event: InterruptionEvent,
    },
    Ping {
        ping_event: PingEvent,
    },
    ClientToolCall {
        client_tool_call: ClientToolCall,
    },
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
pub struct ConversationInitiationMetadataEvent {
    pub conversation_id: String,
    pub agent_output_audio_format: String,
    pub user_input_audio_format: String,
}

#[derive(Debug, Deserialize)]
pub struct AudioEvent {
    pub audio_base_64: String,
}

#[derive(Debug, Deserialize)]
pub struct UserTranscriptionEvent {
    pub user_transcript: String,
}

#[derive(Debug, Deserialize)]
pub struct AgentResponseEvent {
    pub agent_response: String,
}

#[derive(Debug, Deserialize)]
pub struct AgentResponseCorrectionEvent {
    pub original_agent_response: String,
    pub corrected_agent_response: String,
}

#[derive(Debug, Deserialize)]
pub struct InterruptionEvent {
    pub event_id: u64,
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PingEvent {
    pub event_id: u64,
}

#[derive(Debug, Deserialize)]
pub struct ClientToolCall {
    pub tool_call_id: String,
    pub tool_name: String,
    pub parameters: Value,
}
