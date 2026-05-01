//! Configuration discovery — leemos del entorno + sensatas defaults.
//!
//! Variables de entorno reconocidas:
//! - `GATEWAY_HOST`        (default `127.0.0.1`)
//! - `GATEWAY_PORT`        (default `3000`, mismo `DEFAULT_GATEWAY_PORT`
//!                          que IronClaw)
//! - `HTTP_PORT`           (fallback si IronClaw reusa HTTP_PORT en este
//!                          binary)
//! - `GATEWAY_AUTH_TOKEN`  (sin default — si no está, conexión sin auth y
//!                          el gateway probablemente devolverá 401)
//! - `GATEWAY_USE_TLS`     (default `false`; loopback-only no requiere)
//! - `XDG_RUNTIME_DIR`     (para resolver el path del socket)

use std::env;
use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Clone)]
pub struct BridgeConfig {
    pub gateway_host: String,
    pub gateway_port: u16,
    /// Fallback ports to try if the primary `gateway_port` refuses
    /// connection. Probed in order. Lets us tolerate config drift between
    /// `GATEWAY_PORT` (3000) and `HTTP_PORT` (8080).
    pub fallback_ports: Vec<u16>,
    pub gateway_auth_token: Option<String>,
    pub use_tls: bool,
    pub socket_path: PathBuf,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("invalid GATEWAY_PORT: {0}")]
    InvalidPort(String),
    #[error("could not resolve UNIX socket directory; set XDG_RUNTIME_DIR")]
    NoRuntimeDir,
}

impl BridgeConfig {
    pub fn from_env() -> Result<Self, ConfigError> {
        let gateway_host = env::var("GATEWAY_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());

        // IronClaw default for the chat gateway is 3000 (`DEFAULT_GATEWAY_PORT`),
        // but in practice the deployment may serve it on HTTP_PORT (8080) when
        // GATEWAY_ENABLED=true. We try BOTH at runtime in `gateway::run_gateway`;
        // here we just resolve the *primary* hint. Order:
        // 1. `JARVIS_UI_BRIDGE_GATEWAY_PORT` — explicit override.
        // 2. `GATEWAY_PORT` — what IronClaw documents.
        // 3. `HTTP_PORT` — the fallback some deployments use.
        // 4. `3000`.
        let gateway_port = env::var("JARVIS_UI_BRIDGE_GATEWAY_PORT")
            .or_else(|_| env::var("GATEWAY_PORT"))
            .or_else(|_| env::var("HTTP_PORT"))
            .ok()
            .map(|raw| {
                raw.parse::<u16>()
                    .map_err(|_| ConfigError::InvalidPort(raw))
            })
            .transpose()?
            .unwrap_or(3000);

        let gateway_auth_token = env::var("GATEWAY_AUTH_TOKEN").ok();

        let use_tls = env::var("GATEWAY_USE_TLS")
            .ok()
            .map(|v| matches!(v.to_ascii_lowercase().as_str(), "true" | "1" | "yes"))
            .unwrap_or(false);

        let socket_path = resolve_socket_path()?;

        // Build fallback port list — every well-known port that's not the
        // primary, deduped. Order: primary first (handled by run_gateway),
        // then 3000 (gateway default), then 8080 (HTTP_PORT default).
        let mut fallback_ports: Vec<u16> = Vec::new();
        for p in [3000u16, 8080] {
            if p != gateway_port && !fallback_ports.contains(&p) {
                fallback_ports.push(p);
            }
        }

        Ok(Self {
            gateway_host,
            gateway_port,
            fallback_ports,
            gateway_auth_token,
            use_tls,
            socket_path,
        })
    }

    /// URL completa del WebSocket para un port concreto, incluyendo
    /// `?token=` si hay auth.
    pub fn ws_url_for_port(&self, port: u16) -> String {
        let scheme = if self.use_tls { "wss" } else { "ws" };
        let base = format!("{scheme}://{}:{}/api/chat/ws", self.gateway_host, port);
        match &self.gateway_auth_token {
            Some(t) => format!("{base}?token={}", urlencode(t)),
            None => base,
        }
    }

    /// Returns the primary port followed by every fallback (deduped).
    pub fn ports_to_try(&self) -> Vec<u16> {
        let mut ports = vec![self.gateway_port];
        for p in &self.fallback_ports {
            if !ports.contains(p) {
                ports.push(*p);
            }
        }
        ports
    }
}

fn resolve_socket_path() -> Result<PathBuf, ConfigError> {
    if let Ok(custom) = env::var("JARVIS_UI_BRIDGE_SOCKET") {
        return Ok(PathBuf::from(custom));
    }
    let runtime_dir = env::var("XDG_RUNTIME_DIR").map_err(|_| ConfigError::NoRuntimeDir)?;
    Ok(PathBuf::from(runtime_dir).join("jarvis-ui-bridge.sock"))
}

/// Encode minimal URL-unsafe chars (we only escape the characters that
/// could break a `?token=...` query string in practice).
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            ' ' => out.push_str("%20"),
            '#' => out.push_str("%23"),
            '&' => out.push_str("%26"),
            '?' => out.push_str("%3F"),
            '+' => out.push_str("%2B"),
            _ => out.push(c),
        }
    }
    out
}
