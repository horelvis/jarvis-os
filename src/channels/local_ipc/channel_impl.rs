use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::{Mutex, Notify, mpsc};
use tokio_stream::wrappers::ReceiverStream;
use tracing::{debug, warn};

use crate::channels::local_ipc::client::{ClientMap, WireMessage};
use crate::channels::local_ipc::socket::{ListenerConfig, run_listener};
use crate::channels::web::platform::sse::SseManager;
use crate::channels::{Channel, IncomingMessage, MessageStream, OutgoingResponse, StatusUpdate};
use crate::error::ChannelError;

pub struct LocalIpcChannel {
    socket_path: PathBuf,
    user_id: String,
    sse: Arc<SseManager>,
    writer_buffer: usize,
    clients: ClientMap,
    shutdown: Arc<Notify>,
    // mpsc::Sender that the reader tasks will use to inject messages
    // into the agent loop. Materialized in `start()`.
    inject_tx: Mutex<Option<mpsc::Sender<IncomingMessage>>>,
}

impl LocalIpcChannel {
    pub fn new(
        socket_path: PathBuf,
        user_id: String,
        sse: Arc<SseManager>,
        writer_buffer: usize,
    ) -> Self {
        Self {
            socket_path,
            user_id,
            sse,
            writer_buffer,
            clients: Arc::new(Mutex::new(Default::default())),
            shutdown: Arc::new(Notify::new()),
            inject_tx: Mutex::new(None),
        }
    }

    fn build_response_event(response: OutgoingResponse) -> ironclaw_common::AppEvent {
        ironclaw_common::AppEvent::Response {
            content: response.content,
            // OutgoingResponse.thread_id is Option<ExternalThreadId>;
            // AppEvent::Response.thread_id is plain String. Empty string
            // when the caller didn't pin a thread (matches the web
            // channel's behavior at src/bridge/router.rs Response sites).
            thread_id: response
                .thread_id
                .map(|t| t.as_str().to_string())
                .unwrap_or_default(),
        }
    }

    fn extract_client_id(msg: &IncomingMessage) -> &str {
        // metadata is a serde_json::Value (default Value::Null), not
        // Option. Direct .get() on Null returns None safely.
        msg.metadata
            .get("client_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
    }
}

#[async_trait]
impl Channel for LocalIpcChannel {
    fn name(&self) -> &str {
        "local_ipc"
    }

    async fn start(&self) -> Result<MessageStream, ChannelError> {
        let (tx, rx) = mpsc::channel::<IncomingMessage>(64);
        {
            let mut guard = self.inject_tx.lock().await;
            *guard = Some(tx.clone());
        }
        let cfg = ListenerConfig {
            user_id: self.user_id.clone(),
            sse: Arc::clone(&self.sse),
            inject_tx: tx,
            writer_buffer: self.writer_buffer,
            clients: Arc::clone(&self.clients),
            shutdown: Arc::clone(&self.shutdown),
        };
        let path = self.socket_path.clone();
        tokio::spawn(async move {
            if let Err(e) = run_listener(path, cfg).await {
                warn!(error = %e, "local_ipc listener exited with error");
            }
        });
        let stream: MessageStream = Box::pin(ReceiverStream::new(rx));
        Ok(stream)
    }

    async fn respond(
        &self,
        msg: &IncomingMessage,
        response: OutgoingResponse,
    ) -> Result<(), ChannelError> {
        let client_id = Self::extract_client_id(msg);
        let map = self.clients.lock().await;
        if let Some(handle) = map.get(client_id) {
            let wire = WireMessage::App(Self::build_response_event(response));
            if handle.tx.send(wire).await.is_err() {
                debug!(client_id, "respond: writer mpsc closed");
            }
        } else {
            debug!(client_id, "respond: client_id not registered");
        }
        Ok(())
    }

    async fn send_status(
        &self,
        _status: StatusUpdate,
        _metadata: &serde_json::Value,
    ) -> Result<(), ChannelError> {
        // Writers also subscribe to SseManager directly; status events
        // routed through there reach the same client. respond() is the
        // only "directed" path we honour explicitly. Default no-op.
        Ok(())
    }

    async fn broadcast(
        &self,
        user_id: &str,
        response: OutgoingResponse,
    ) -> Result<(), ChannelError> {
        if user_id != self.user_id {
            return Ok(());
        }
        let event = Self::build_response_event(response);
        let map = self.clients.lock().await;
        for handle in map.values() {
            let _ = handle.tx.send(WireMessage::App(event.clone())).await;
        }
        Ok(())
    }

    async fn health_check(&self) -> Result<(), ChannelError> {
        if !self.socket_path.exists() {
            return Err(ChannelError::HealthCheckFailed {
                name: "local_ipc".into(),
            });
        }
        Ok(())
    }

    async fn shutdown(&self) -> Result<(), ChannelError> {
        self.shutdown.notify_waiters();
        Ok(())
    }
}
