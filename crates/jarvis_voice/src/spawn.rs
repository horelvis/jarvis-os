//! **B1 only — borrado en B2.**
//!
//! Subprocess launcher para `jarvis-voice-daemon`. Resuelve el binario
//! desde `JARVIS_VOICE_DAEMON_BIN` (default: `jarvis-voice-daemon` en
//! `$PATH`), exporta las envs del config (`ELEVENLABS_*`,
//! `JARVIS_VOICE_*`) heredando además el entorno actual, y devuelve un
//! `DaemonChild` que mata el proceso al `shutdown` o al drop.
//!
//! En B1 el daemon sigue siendo la fuente de verdad del audio; el
//! shim `ElevenLabsLocalBackend` recibe PCM por el canal IPC existente.
//! En B2 todo este archivo se borra.

use crate::config::VoiceConfig;
use crate::error::VoiceError;
use std::process::Stdio;
use tokio::process::{Child, Command};

const DEFAULT_BIN: &str = "jarvis-voice-daemon";

#[derive(Debug)]
pub(crate) struct DaemonChild {
    child: Child,
}

impl DaemonChild {
    pub(crate) async fn spawn(cfg: &VoiceConfig) -> Result<Self, VoiceError> {
        let bin = std::env::var("JARVIS_VOICE_DAEMON_BIN")
            .unwrap_or_else(|_| DEFAULT_BIN.to_string());

        let mut command = Command::new(&bin);
        command
            .env("ELEVENLABS_AGENT_ID", &cfg.agent_id)
            .env("ELEVENLABS_API_KEY", &cfg.api_key)
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .kill_on_drop(true);

        if let Some(prompt) = cfg.system_prompt_override.as_deref() {
            command.env("JARVIS_VOICE_SYSTEM_PROMPT_OVERRIDE", prompt);
        }
        if !cfg.dynamic_variables.is_empty() {
            let kv: Vec<String> = cfg
                .dynamic_variables
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect();
            command.env("JARVIS_VOICE_VARS", kv.join(","));
        }

        let child = command
            .spawn()
            .map_err(|e| VoiceError::Spawn(format!("failed to spawn '{bin}': {e}")))?;

        tracing::debug!(
            bin = %bin,
            agent = %cfg.agent_id_redacted(),
            "voice.daemon_subprocess.spawned"
        );

        Ok(Self { child })
    }

    pub(crate) async fn shutdown(mut self) -> Result<(), VoiceError> {
        // kill_on_drop(true) cubre el drop, pero pedimos terminación
        // explícita para tener un await observable. SIGKILL es fine
        // aquí porque el daemon no persiste estado relevante.
        if let Err(e) = self.child.kill().await {
            return Err(VoiceError::Spawn(format!("kill voice daemon: {e}")));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg_for_test() -> VoiceConfig {
        VoiceConfig {
            agent_id: "agent_test".into(),
            api_key: "key_test".into(),
            system_prompt_override: None,
            dynamic_variables: Default::default(),
            aec_delay_ms: 50,
        }
    }

    /// Both spawn cases live in one test because they mutate the shared
    /// process-wide env var `JARVIS_VOICE_DAEMON_BIN`. Splitting into
    /// `#[tokio::test]`s would race under cargo's default parallel
    /// runner — the missing-binary case can read the dummy override
    /// from the success case mid-flight.
    #[tokio::test]
    async fn spawn_resolves_binary_from_env() {
        let cfg = cfg_for_test();

        // Case 1: missing binary → Spawn error.
        // SAFETY: env mutation; sequential within this single test.
        unsafe {
            std::env::set_var(
                "JARVIS_VOICE_DAEMON_BIN",
                "/this/path/does/not/exist/jarvis-voice-daemon",
            );
        }
        let err = DaemonChild::spawn(&cfg).await.unwrap_err();
        assert!(
            matches!(err, VoiceError::Spawn(_)),
            "expected Spawn error, got {err:?}"
        );

        // Case 2: dummy binary (/usr/bin/true) → spawn succeeds.
        unsafe {
            std::env::set_var("JARVIS_VOICE_DAEMON_BIN", "/usr/bin/true");
        }
        let child = DaemonChild::spawn(&cfg)
            .await
            .expect("spawn must succeed against /usr/bin/true");
        child.shutdown().await.expect("shutdown must succeed");

        unsafe {
            std::env::remove_var("JARVIS_VOICE_DAEMON_BIN");
        }
    }
}
