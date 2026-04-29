//! jarvis-os voice daemon — cliente de ElevenLabs Conversational AI.
//!
//! Captura audio del mic (cpal), lo manda como chunks PCM 16kHz a una
//! sesión WebSocket en `wss://api.elevenlabs.io/v1/convai/conversation`,
//! y reproduce la respuesta del agent en el speaker. ElevenLabs hace
//! todo el trabajo pesado (STT + LLM + TTS + turn-taking + barge-in);
//! este daemon es el cabledo de audio entre el hardware local y la API.

mod audio;
mod config;
mod orchestrator;
mod protocol;
mod ws_client;

use clap::Parser;
use config::Config;

#[derive(Parser, Debug)]
#[command(name = "jarvis-voice-daemon", version, about)]
struct Args {
    /// ID del agente en ElevenLabs (formato `agent_xxx...`).
    #[arg(long, env = "ELEVENLABS_AGENT_ID")]
    agent_id: String,

    /// API key de ElevenLabs.
    #[arg(long, env = "ELEVENLABS_API_KEY")]
    api_key: String,

    /// Ruta a un .env adicional (opcional). Por defecto se carga
    /// `~/.ironclaw/.env` y `/etc/jarvis/env` si existen.
    #[arg(long, env = "JARVIS_VOICE_ENV")]
    env_file: Option<std::path::PathBuf>,

    /// Sobrescribe el system prompt del agente para esta sesión.
    #[arg(long, env = "JARVIS_VOICE_SYSTEM_PROMPT_OVERRIDE")]
    system_prompt_override: Option<String>,
}

fn init_tracing() {
    use tracing_subscriber::{EnvFilter, fmt, prelude::*};
    let filter = EnvFilter::try_from_env("JARVIS_VOICE_LOG")
        .unwrap_or_else(|_| EnvFilter::new("info,jarvis_voice_daemon=debug"));
    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().json().with_target(true))
        .init();
}

fn load_env(extra: Option<&std::path::Path>) {
    let candidates: Vec<std::path::PathBuf> = [
        std::path::PathBuf::from("/etc/jarvis/env"),
        dirs_home(".ironclaw/.env"),
    ]
    .into_iter()
    .chain(extra.map(|p| p.to_path_buf()))
    .collect();

    for p in candidates {
        if p.is_file() {
            let _ = dotenvy::from_path(&p);
        }
    }
}

fn dirs_home(rel: &str) -> std::path::PathBuf {
    std::env::var_os("HOME")
        .map(|h| std::path::PathBuf::from(h).join(rel))
        .unwrap_or_else(|| std::path::PathBuf::from(rel))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // .env primero (puede definir las vars que clap leerá vía `env =`).
    let pre_args: Vec<String> = std::env::args().collect();
    let env_override = pre_args
        .iter()
        .position(|a| a == "--env-file")
        .and_then(|i| pre_args.get(i + 1).map(std::path::PathBuf::from));
    load_env(env_override.as_deref());

    init_tracing();

    let args = Args::parse();

    let cfg = Config {
        agent_id: args.agent_id,
        api_key: args.api_key,
        system_prompt_override: args.system_prompt_override,
        sample_rate: 16_000,
    };

    tracing::info!(
        agent_id = %cfg.agent_id_redacted(),
        sample_rate = cfg.sample_rate,
        "daemon.starting"
    );

    orchestrator::run(cfg).await
}
