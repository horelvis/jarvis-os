//! UNIX socket server. Cada cliente conectado recibe los AppEvents en
//! formato NDJSON (un JSON por línea, terminado en `\n`).
//!
//! El path del socket queda en `BridgeConfig::socket_path`. Si el archivo
//! ya existe (proceso anterior no limpió) lo borramos antes de bind.

use std::path::PathBuf;

use tokio::io::AsyncWriteExt;
use tokio::net::UnixListener;
use tokio::sync::broadcast;
use tracing::{debug, info, warn};

pub async fn run_socket(
    socket_path: PathBuf,
    tx: broadcast::Sender<String>,
) -> std::io::Result<()> {
    // Limpiar socket residual de runs anteriores.
    if socket_path.exists() {
        let _ = tokio::fs::remove_file(&socket_path).await;
    }
    if let Some(parent) = socket_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let listener = UnixListener::bind(&socket_path)?;
    info!("[bridge] socket listening at {}", socket_path.display());

    loop {
        let (stream, _addr) = listener.accept().await?;
        debug!("[bridge] new client");
        let mut rx = tx.subscribe();
        tokio::spawn(async move {
            if let Err(e) = handle_client(stream, &mut rx).await {
                warn!("[bridge] client error: {e}");
            }
            debug!("[bridge] client disconnected");
        });
    }
}

async fn handle_client(
    mut stream: tokio::net::UnixStream,
    rx: &mut broadcast::Receiver<String>,
) -> std::io::Result<()> {
    while let Ok(msg) = rx.recv().await {
        // NDJSON: one JSON per line, line-terminated.
        if let Err(e) = stream.write_all(msg.as_bytes()).await {
            return Err(e);
        }
        if let Err(e) = stream.write_all(b"\n").await {
            return Err(e);
        }
    }
    Ok(())
}
