//! WebSocket client al gateway de IronClaw con auto-reconnect.
//!
//! Cada mensaje recibido se broadcasta al canal `tx` para que el módulo
//! `socket` lo fan-out a todos los clientes UNIX conectados.
//!
//! En estado offline, escribe un evento sintético `{"type":"bridge_offline"}`
//! y otro `{"type":"bridge_online"}` al reconectar — el QML los usa para
//! dibujar el estado del orbe.

use std::time::Duration;

use futures_util::StreamExt;
use tokio::sync::broadcast;
use tokio::time::sleep;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::handshake::client::Request;
use tokio_tungstenite::tungstenite::http::Uri;
use tokio_tungstenite::tungstenite::protocol::Message;
use tracing::{debug, info, warn};

use crate::config::BridgeConfig;

/// Run the gateway client loop. On any disconnect, tries every
/// configured port (primary then fallbacks). On full-list failure,
/// sleeps with exponential backoff (1s → 30s cap) before retrying.
pub async fn run_gateway(cfg: BridgeConfig, tx: broadcast::Sender<String>) {
    let mut backoff_secs: u64 = 1;
    loop {
        let mut connected_ok = false;
        for port in cfg.ports_to_try() {
            match connect_once(&cfg, port, &tx).await {
                Ok(()) => {
                    // Connection closed cleanly. Reset backoff and break
                    // the port loop — we'll start over from primary.
                    connected_ok = true;
                    break;
                }
                Err(e) => {
                    warn!("[bridge] port {port} failed: {e}");
                }
            }
        }
        if !connected_ok {
            warn!(
                "[bridge] gateway unreachable on ports {:?}",
                cfg.ports_to_try()
            );
            let _ = tx.send(r#"{"type":"bridge_offline"}"#.to_string());
            debug!("[bridge] reconnecting in {}s", backoff_secs);
            sleep(Duration::from_secs(backoff_secs)).await;
            backoff_secs = (backoff_secs * 2).min(30);
        } else {
            backoff_secs = 1;
        }
    }
}

async fn connect_once(
    cfg: &BridgeConfig,
    port: u16,
    tx: &broadcast::Sender<String>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let url = cfg.ws_url_for_port(port);
    info!("[bridge] connecting to {}", url);

    // El handler del gateway requiere Origin header válido (loopback
    // origin). Construímos la request manualmente con ese header.
    let uri: Uri = url.parse()?;
    let host = format!("{}:{}", cfg.gateway_host, port);
    let req = Request::builder()
        .uri(uri)
        .header("Host", host)
        .header("Origin", format!("http://{}", cfg.gateway_host))
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .header("Sec-WebSocket-Version", "13")
        .header("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ==")
        .body(())?;

    let (ws_stream, response) = connect_async(req).await?;
    info!("[bridge] connected · status={}", response.status().as_u16());
    let _ = tx.send(r#"{"type":"bridge_online"}"#.to_string());

    let (_write, mut read) = ws_stream.split();

    while let Some(msg) = read.next().await {
        match msg? {
            Message::Text(text) => {
                debug!("[bridge] ← {} bytes", text.len());
                // Broadcast to socket clients. Send error means no
                // subscribers (all clients gone) — ignore.
                let _ = tx.send(text.to_string());
            }
            Message::Binary(b) => {
                debug!("[bridge] ← binary {} bytes (ignored)", b.len());
            }
            Message::Close(_) => {
                info!("[bridge] gateway closed the connection");
                break;
            }
            Message::Ping(p) => {
                debug!("[bridge] ping {} bytes", p.len());
            }
            _ => {}
        }
    }

    Ok(())
}
