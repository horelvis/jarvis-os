//! jarvis-ui-bridge — daemon que conecta al gateway WS de IronClaw y
//! re-emite los eventos por un UNIX socket que el QML consume.
//!
//! Diseño:
//! 1. Lee config del entorno: GATEWAY_HOST/PORT/AUTH_TOKEN del .env de
//!    IronClaw, con fallbacks sensatos.
//! 2. Bind UNIX socket en `/run/user/<uid>/jarvis-ui-bridge.sock`.
//! 3. Conecta al gateway WS con auto-reconnect (exp backoff capped 30s).
//! 4. Por cada AppEvent recibido, escribe una línea JSON en TODOS los
//!    clientes UNIX conectados al socket (fan-out).
//! 5. Loglinea propia con `[bridge]` prefix; usa stderr (captado por
//!    journald si corremos como systemd-user).

mod config;
mod gateway;
mod socket;

use std::process::ExitCode;

use tokio::signal;
use tokio::sync::broadcast;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> ExitCode {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("jarvis_ui_bridge=info,info")),
        )
        .with_writer(std::io::stderr)
        .try_init();

    let cfg = match config::BridgeConfig::from_env() {
        Ok(c) => c,
        Err(e) => {
            error!("[bridge] config error: {e}");
            return ExitCode::from(2);
        }
    };

    info!(
        "[bridge] starting · gateway={}://{}:{} socket={}",
        if cfg.use_tls { "wss" } else { "ws" },
        cfg.gateway_host,
        cfg.gateway_port,
        cfg.socket_path.display()
    );

    // Broadcast channel: gateway task fans out events to all socket clients.
    // Capacity 256 covers bursts; slow clients drop frames silently.
    let (tx, _rx) = broadcast::channel::<String>(256);

    let socket_handle = tokio::spawn(socket::run_socket(cfg.socket_path.clone(), tx.clone()));
    let gateway_handle = tokio::spawn(gateway::run_gateway(cfg.clone(), tx));

    // Run until SIGINT/SIGTERM.
    let ctrl_c = async {
        let _ = signal::ctrl_c().await;
    };
    let term = async {
        if let Ok(mut s) = signal::unix::signal(signal::unix::SignalKind::terminate()) {
            s.recv().await;
        }
    };

    tokio::select! {
        _ = ctrl_c => info!("[bridge] SIGINT received, shutting down"),
        _ = term => info!("[bridge] SIGTERM received, shutting down"),
        r = socket_handle => match r {
            Ok(Ok(())) => info!("[bridge] socket loop exited cleanly"),
            Ok(Err(e)) => error!("[bridge] socket loop failed: {e}"),
            Err(e) => error!("[bridge] socket task panicked: {e}"),
        },
        r = gateway_handle => match r {
            Ok(()) => info!("[bridge] gateway loop exited"),
            Err(e) => error!("[bridge] gateway task panicked: {e}"),
        },
    }

    let _ = std::fs::remove_file(&cfg.socket_path);
    info!("[bridge] socket file removed; bye");
    ExitCode::SUCCESS
}
