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

        let system_prompt_override = std::env::var("JARVIS_VOICE_SYSTEM_PROMPT_OVERRIDE").ok();
        let dynamic_variables =
            parse_kv_list(std::env::var("JARVIS_VOICE_VARS").ok().as_deref());

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
        assert_eq!(
            parsed.get("display_name").map(String::as_str),
            Some("Horelvis")
        );
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
