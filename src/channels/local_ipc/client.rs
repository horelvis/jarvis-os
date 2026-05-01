#![allow(dead_code)] // consumers wired in Track E

use std::sync::Arc;

use ironclaw_common::AppEvent;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::{Mutex, mpsc};
use tokio_stream::StreamExt;
use tracing::{debug, warn};

use crate::channels::IncomingMessage;
use crate::channels::local_ipc::control::{ControlError, build_control_submission};
use crate::channels::local_ipc::protocol::{
    ClientCommand, ClientId, IpcErrorKind, IpcHello, PROTOCOL_VERSION, TransportEvent,
};
use crate::channels::web::platform::sse::SseManager;

const MAX_LINE_BYTES: usize = 64 * 1024;
pub const DEFAULT_WRITER_BUFFER: usize = 256;

/// Single envelope for everything the writer task emits to the client.
///
/// Unifying AppEvent + TransportEvent lets the writer have one
/// serialize-then-write path and a single mpsc, and lets the reader push
/// transport errors (malformed line, oversized command) to the same writer.
#[derive(Debug, Clone)]
pub enum WireMessage {
    App(AppEvent),
    Transport(TransportEvent),
}

/// Owner half of a per-client session. Held by `LocalIpcChannel` so it
/// can route `respond()` / `send_status()` to the right writer.
#[derive(Debug)]
pub struct ClientHandle {
    pub client_id: ClientId,
    pub tx: mpsc::Sender<WireMessage>,
}

/// Run a fresh client session. Spawns reader + writer tasks and returns
/// the `ClientHandle` so the caller can register it before either task
/// ever yields. The session ends when the client closes the socket; both
/// tasks then terminate and the caller is expected to remove the handle.
pub async fn spawn_session(
    stream: UnixStream,
    client_id: ClientId,
    user_id: String,
    sse: Arc<SseManager>,
    inject_tx: mpsc::Sender<IncomingMessage>,
    writer_buffer: usize,
) -> ClientHandle {
    let (read_half, write_half) = stream.into_split();
    let (event_tx, event_rx) = mpsc::channel::<WireMessage>(writer_buffer);

    let writer_user_id = user_id.clone();
    let writer_sse = Arc::clone(&sse);
    let writer_id = client_id.clone();
    let writer_tx_for_reader = event_tx.clone();

    tokio::spawn(async move {
        run_writer_task(write_half, event_rx, writer_id, writer_user_id, writer_sse).await;
    });

    let reader_id = client_id.clone();
    tokio::spawn(async move {
        run_reader_task(
            read_half,
            reader_id,
            user_id,
            inject_tx,
            writer_tx_for_reader,
        )
        .await;
        // When the reader exits (client closed the socket), our extra
        // sender drops along with this future and the writer task
        // observes channel close (if it was the last sender) and ends.
    });

    ClientHandle {
        client_id,
        tx: event_tx,
    }
}

async fn run_reader_task(
    read_half: tokio::net::unix::OwnedReadHalf,
    client_id: ClientId,
    user_id: String,
    inject_tx: mpsc::Sender<IncomingMessage>,
    error_event_tx: mpsc::Sender<WireMessage>,
) {
    let mut buf = BufReader::new(read_half);
    let mut line = String::new();
    loop {
        line.clear();
        let read = buf.read_line(&mut line).await;
        match read {
            Ok(0) => {
                debug!(client = %client_id, "ipc client closed");
                break;
            }
            Ok(n) if n > MAX_LINE_BYTES => {
                emit_transport_error(
                    &error_event_tx,
                    IpcErrorKind::CommandTooLarge,
                    "command line exceeded 64 KiB",
                )
                .await;
                continue;
            }
            Ok(_) => {}
            Err(e) => {
                warn!(client = %client_id, error = %e, "ipc client read error");
                break;
            }
        }
        let trimmed = line.trim_end_matches(['\n', '\r']);
        if trimmed.is_empty() {
            continue; // silent-ok: empty line, continue session
        }
        let cmd: ClientCommand = match serde_json::from_str(trimmed) {
            Ok(c) => c,
            Err(e) => {
                warn!(client = %client_id, error = %e, "ipc command parse failed");
                emit_transport_error(
                    &error_event_tx,
                    IpcErrorKind::CommandInvalid,
                    "could not parse command",
                )
                .await;
                continue; // silent-ok: malformed line, continue session
            }
        };
        if let Err(e) = dispatch_command(cmd, &user_id, &client_id, &inject_tx).await {
            warn!(client = %client_id, error = %e, "ipc command dispatch failed");
            emit_transport_error(
                &error_event_tx,
                IpcErrorKind::CommandInvalid,
                "command dispatch failed",
            )
            .await;
        }
    }
}

#[derive(Debug, thiserror::Error)]
enum DispatchError {
    #[error("control error: {0}")]
    Control(#[from] ControlError),
    #[error("inject channel closed")]
    InjectClosed,
}

async fn dispatch_command(
    cmd: ClientCommand,
    user_id: &str,
    client_id: &ClientId,
    inject_tx: &mpsc::Sender<IncomingMessage>,
) -> Result<(), DispatchError> {
    match cmd {
        ClientCommand::Message { content, thread_id } => {
            let metadata = serde_json::json!({
                "client_id": client_id.as_str(),
                "thread_id": thread_id,
            });
            let msg = IncomingMessage::new("local_ipc", user_id, content).with_metadata(metadata);
            inject_tx
                .send(msg)
                .await
                .map_err(|_| DispatchError::InjectClosed)?;
            Ok(())
        }
        ClientCommand::Ping => Ok(()),
        ClientCommand::Approval { .. } | ClientCommand::Cancel { .. } => {
            if let Some(msg) = build_control_submission(&cmd, user_id, client_id)? {
                inject_tx
                    .send(msg)
                    .await
                    .map_err(|_| DispatchError::InjectClosed)?;
            }
            Ok(())
        }
    }
}

async fn run_writer_task(
    mut write_half: tokio::net::unix::OwnedWriteHalf,
    mut event_rx: mpsc::Receiver<WireMessage>,
    client_id: ClientId,
    user_id: String,
    sse: Arc<SseManager>,
) {
    // Emit the synthetic ipc_hello before anything else.
    let hello = WireMessage::Transport(TransportEvent::IpcHello(IpcHello {
        protocol_version: PROTOCOL_VERSION,
        local_user_id: user_id.clone(),
    }));
    if !write_wire(&mut write_half, &hello).await {
        return;
    }

    // Subscribe to the SseManager scoped to this user. None means we hit
    // the global max_connections; the writer keeps serving direct
    // respond()/send_status traffic on event_rx only.
    let mut sse_stream = sse.subscribe_raw(Some(user_id), false);

    loop {
        let wire_opt: Option<WireMessage> = tokio::select! {
            biased;
            Some(msg) = event_rx.recv() => Some(msg),
            sse_event = async {
                match sse_stream.as_mut() {
                    Some(s) => s.next().await,
                    None => {
                        // Park forever — fall through to event_rx only.
                        std::future::pending::<()>().await;
                        None
                    }
                }
            } => sse_event.map(WireMessage::App),
            else => None,
        };
        let Some(wire) = wire_opt else { break };
        if !write_wire(&mut write_half, &wire).await {
            break;
        }
    }
    debug!(client = %client_id, "ipc writer terminated");
}

async fn write_wire(write_half: &mut tokio::net::unix::OwnedWriteHalf, msg: &WireMessage) -> bool {
    let bytes_result = match msg {
        WireMessage::App(ev) => serde_json::to_vec(ev),
        WireMessage::Transport(ev) => serde_json::to_vec(ev),
    };
    match bytes_result {
        Ok(mut bytes) => {
            bytes.push(b'\n');
            if let Err(e) = write_half.write_all(&bytes).await {
                debug!(error = %e, "ipc writer write failed");
                return false;
            }
            true
        }
        Err(e) => {
            // Serialization bug shouldn't kill the session — log and
            // skip the offending event.
            debug!(error = %e, "ipc writer serialize failed");
            true
        }
    }
}

/// Push a sanitized transport-error event back to the client. `try_send`
/// (not `send().await`) so a wedged writer mpsc can't backpressure the
/// reader. Drop is acceptable — the client will see protocol drift on
/// the next valid command anyway.
async fn emit_transport_error(tx: &mpsc::Sender<WireMessage>, kind: IpcErrorKind, detail: &str) {
    let ev = WireMessage::Transport(TransportEvent::Error {
        kind,
        detail: detail.to_string(),
    });
    if let Err(e) = tx.try_send(ev) {
        debug!(error = %e, "transport error event dropped (writer backpressured)");
    }
}

/// Holder used by the listener loop to remember active clients keyed by
/// id, so the channel impl can fan-out by `client_id`.
pub type ClientMap = Arc<Mutex<std::collections::HashMap<String, ClientHandle>>>;

#[cfg(test)]
mod tests {
    use super::*;
    use ironclaw_common::AppEvent;
    use tempfile::tempdir;
    use tokio::net::UnixListener;

    async fn pair_unix() -> (UnixStream, UnixStream) {
        let dir = tempdir().unwrap();
        let path = dir.path().join("p.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let connect_handle = tokio::spawn({
            let path = path.clone();
            async move { UnixStream::connect(path).await.unwrap() }
        });
        let (server, _addr) = listener.accept().await.unwrap();
        let client = connect_handle.await.unwrap();
        // Keep dir alive by leaking it — closes when the test process
        // exits. Acceptable in tests because the OS reaps temp files.
        std::mem::forget(dir);
        (server, client)
    }

    #[tokio::test]
    async fn writer_emits_hello_first() {
        let (server, client) = pair_unix().await;
        let sse = Arc::new(SseManager::new());
        let (inject_tx, _inject_rx) = mpsc::channel::<IncomingMessage>(8);
        let _handle = spawn_session(
            server,
            ClientId::new("c1").unwrap(),
            "owner".into(),
            sse,
            inject_tx,
            DEFAULT_WRITER_BUFFER,
        )
        .await;

        let mut reader = BufReader::new(client);
        let mut first = String::new();
        reader.read_line(&mut first).await.unwrap();
        assert!(first.contains("\"type\":\"ipc_hello\""));
        assert!(first.contains("\"protocol_version\":1"));
        assert!(first.contains("\"local_user_id\":\"owner\""));
    }

    #[tokio::test]
    async fn writer_forwards_direct_event() {
        let (server, client) = pair_unix().await;
        let sse = Arc::new(SseManager::new());
        let (inject_tx, _inject_rx) = mpsc::channel::<IncomingMessage>(8);
        let handle = spawn_session(
            server,
            ClientId::new("c2").unwrap(),
            "owner".into(),
            sse,
            inject_tx,
            DEFAULT_WRITER_BUFFER,
        )
        .await;
        // Drain the hello.
        let mut reader = BufReader::new(client);
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        // Push a direct event via the per-client mpsc.
        handle
            .tx
            .send(WireMessage::App(AppEvent::Heartbeat))
            .await
            .expect("send heartbeat");
        line.clear();
        reader.read_line(&mut line).await.unwrap();
        assert!(line.contains("\"type\":\"heartbeat\""));
    }

    #[tokio::test]
    async fn malformed_line_emits_transport_error_to_client() {
        let (server, client) = pair_unix().await;
        let sse = Arc::new(SseManager::new());
        let (inject_tx, _inject_rx) = mpsc::channel::<IncomingMessage>(8);
        let _handle = spawn_session(
            server,
            ClientId::new("c-err").unwrap(),
            "owner".into(),
            sse,
            inject_tx,
            DEFAULT_WRITER_BUFFER,
        )
        .await;
        // Split the client side so we can read and write concurrently
        // without aliasing &mut.
        let (client_r, mut client_w) = client.into_split();
        let mut reader = BufReader::new(client_r);
        let mut hello = String::new();
        reader.read_line(&mut hello).await.unwrap();
        client_w.write_all(b"this is not json\n").await.unwrap();
        let mut err_line = String::new();
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            reader.read_line(&mut err_line),
        )
        .await
        .expect("transport error timeout")
        .unwrap();
        assert!(err_line.contains("\"type\":\"error\""));
        assert!(err_line.contains("\"kind\":\"command_invalid\""));
    }

    #[tokio::test]
    async fn reader_routes_message_to_inject_tx() {
        let (server, mut client) = pair_unix().await;
        let sse = Arc::new(SseManager::new());
        let (inject_tx, mut inject_rx) = mpsc::channel::<IncomingMessage>(8);
        let _handle = spawn_session(
            server,
            ClientId::new("c3").unwrap(),
            "owner".into(),
            sse,
            inject_tx,
            DEFAULT_WRITER_BUFFER,
        )
        .await;

        let payload = b"{\"type\":\"message\",\"content\":\"hola\"}\n";
        client.write_all(payload).await.unwrap();
        let msg = tokio::time::timeout(std::time::Duration::from_secs(2), inject_rx.recv())
            .await
            .expect("inject_rx timed out")
            .expect("inject channel closed");
        assert_eq!(msg.channel, "local_ipc");
        assert_eq!(msg.content, "hola");
        // CRITICAL: metadata is serde_json::Value (NOT Option<Value>) — index directly, no .unwrap()
        assert_eq!(msg.metadata["client_id"], "c3");
    }
}
