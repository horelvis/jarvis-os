//! Configuración runtime del daemon.

#[derive(Debug, Clone)]
pub struct Config {
    pub agent_id: String,
    pub api_key: String,
    pub system_prompt_override: Option<String>,
    pub sample_rate: u32,
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
