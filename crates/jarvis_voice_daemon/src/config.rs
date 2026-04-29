//! Configuración runtime del daemon.

#[derive(Debug, Clone)]
pub struct Config {
    pub agent_id: String,
    pub api_key: String,
    pub system_prompt_override: Option<String>,
    pub sample_rate: u32,
    /// Variables dinámicas que el agente requiere para resolver
    /// placeholders `{{var}}` en su system prompt. Las declaras en
    /// consola de ElevenLabs y aquí las pasas con valor concreto. Si
    /// el agente las exige y no las mandas, el WS se cierra con
    /// `Policy: Missing required dynamic variables`.
    pub dynamic_variables: std::collections::BTreeMap<String, String>,
}

impl Config {
    /// Versión redactada del agent_id para logs (no es secreto pero
    /// preferimos no llenar la traza con IDs completos).
    pub fn agent_id_redacted(&self) -> String {
        let head: String = self.agent_id.chars().take(12).collect();
        if self.agent_id.len() > 12 {
            format!("{head}…")
        } else {
            head
        }
    }
}
