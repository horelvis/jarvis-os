//! Regression: tool status events must preserve per-tool identity fields
//! needed by the web UI to render live tool activity correctly.
//!
//! Events flow through `ChannelManager::send_status` (single producer for
//! the channel-dispatch flow). The browser SSE handler subscribes to the
//! bus owned by the gateway's `state.sse` — same `Arc` the
//! `ChannelManager` was wired to via `set_event_bus` at startup.

use std::sync::Arc;

use crate::channels::ChannelManager;
use crate::channels::StatusUpdate;
use crate::channels::web::GatewayChannel;
use crate::channels::web::sse::DEFAULT_BROADCAST_BUFFER;
use crate::config::GatewayConfig;
use futures::StreamExt;
use ironclaw_common::AppEvent;

fn test_gateway() -> GatewayChannel {
    GatewayChannel::new(
        GatewayConfig {
            host: "127.0.0.1".to_string(),
            port: 0,
            auth_token: Some("test-token".to_string()),
            max_connections: 100,
            broadcast_buffer: DEFAULT_BROADCAST_BUFFER,
            workspace_read_scopes: vec![],
            memory_layers: vec![],
            oidc: None,
        },
        "test-user".to_string(),
    )
}

#[tokio::test]
async fn gateway_send_status_preserves_tool_event_fields() {
    let gw = test_gateway();
    let bus = Arc::clone(&gw.state.sse);
    let mut stream = bus
        .subscribe_raw(Some("test-user".to_string()), false)
        .expect("subscribe should succeed");

    let mgr = ChannelManager::new();
    mgr.set_event_bus(Arc::clone(&bus)).await;
    mgr.add(Box::new(gw)).await;

    let metadata = serde_json::json!({
        "user_id": "test-user",
        "thread_id": "thread-123"
    });

    mgr.send_status(
        "gateway",
        StatusUpdate::ToolStarted {
            name: "shell".to_string(),
            detail: Some("ls -la".to_string()),
            call_id: Some("call_shell_1".to_string()),
        },
        &metadata,
    )
    .await
    .expect("tool_started should publish");

    mgr.send_status(
        "gateway",
        StatusUpdate::ToolCompleted {
            name: "shell".to_string(),
            success: true,
            error: None,
            parameters: None,
            call_id: Some("call_shell_1".to_string()),
            duration_ms: Some(42),
        },
        &metadata,
    )
    .await
    .expect("tool_completed should publish");

    mgr.send_status(
        "gateway",
        StatusUpdate::ToolResult {
            name: "shell".to_string(),
            preview: "file_a\nfile_b".to_string(),
            call_id: Some("call_shell_1".to_string()),
        },
        &metadata,
    )
    .await
    .expect("tool_result should publish");

    let started = tokio::time::timeout(std::time::Duration::from_secs(1), stream.next())
        .await
        .expect("should receive tool_started")
        .expect("stream should not be empty");
    let completed = tokio::time::timeout(std::time::Duration::from_secs(1), stream.next())
        .await
        .expect("should receive tool_completed")
        .expect("stream should not be empty");
    let result = tokio::time::timeout(std::time::Duration::from_secs(1), stream.next())
        .await
        .expect("should receive tool_result")
        .expect("stream should not be empty");

    assert!(matches!(
        started,
        AppEvent::ToolStarted {
            name,
            detail,
            call_id,
            thread_id,
        } if name == "shell"
            && detail.as_deref() == Some("ls -la")
            && call_id.as_deref() == Some("call_shell_1")
            && thread_id.as_deref() == Some("thread-123")
    ));
    assert!(matches!(
        completed,
        AppEvent::ToolCompleted {
            name,
            success,
            call_id,
            duration_ms,
            thread_id,
            ..
        } if name == "shell"
            && success
            && call_id.as_deref() == Some("call_shell_1")
            && duration_ms == Some(42)
            && thread_id.as_deref() == Some("thread-123")
    ));
    assert!(matches!(
        result,
        AppEvent::ToolResult {
            name,
            preview,
            call_id,
            thread_id,
        } if name == "shell"
            && preview == "file_a\nfile_b"
            && call_id.as_deref() == Some("call_shell_1")
            && thread_id.as_deref() == Some("thread-123")
    ));
}
