//! Integration tests for src/channels/local_ipc/.
//! See docs/superpowers/specs/2026-04-30-jarvis-os-local-ipc-design.md §11.2.

#![cfg(feature = "integration")]
#![allow(unused_imports)]

use std::sync::Arc;
use std::time::Duration;

use ironclaw::channels::Channel;
use ironclaw::channels::local_ipc::LocalIpcChannel;
use ironclaw::channels::web::sse::SseManager;
use ironclaw_common::AppEvent;
use tempfile::tempdir;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

/// Wait for the listener to bind, polling the socket path. Caps at 2s
/// (100 × 20ms). Caller passes the same path used at construction.
async fn wait_for_bind(path: &std::path::Path) {
    for _ in 0..100 {
        if path.exists() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("listener did not bind {} within 2s", path.display());
}

async fn spawn_channel(socket_path: std::path::PathBuf) -> (Arc<LocalIpcChannel>, Arc<SseManager>) {
    let sse = Arc::new(SseManager::new());
    let chan = Arc::new(LocalIpcChannel::new(
        socket_path.clone(),
        "owner".into(),
        Arc::clone(&sse),
        16,
    ));
    let _stream = chan.start().await.expect("start");
    wait_for_bind(&socket_path).await;
    (chan, sse)
}

#[tokio::test]
async fn test_bind_connect_hello_ping() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("h1.sock");
    let (_chan, _sse) = spawn_channel(path.clone()).await;
    assert!(
        path.exists(),
        "socket file must exist after spawn_channel returns"
    );

    let stream = UnixStream::connect(&path).await.unwrap();
    let mut reader = BufReader::new(stream);
    let mut hello = String::new();
    tokio::time::timeout(Duration::from_secs(2), reader.read_line(&mut hello))
        .await
        .expect("hello timeout")
        .unwrap();
    assert!(hello.contains("\"type\":\"ipc_hello\""));
    assert!(hello.contains("\"local_user_id\":\"owner\""));
    assert!(hello.contains("\"protocol_version\":1"));
}

async fn drain_hello(reader: &mut BufReader<UnixStream>) {
    let mut hello = String::new();
    tokio::time::timeout(Duration::from_secs(2), reader.read_line(&mut hello))
        .await
        .expect("hello timeout")
        .unwrap();
    assert!(hello.contains("ipc_hello"));
}

#[tokio::test]
async fn test_two_clients_receive_same_broadcast() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("h2.sock");
    let (_chan, sse) = spawn_channel(path.clone()).await;
    let mut a = BufReader::new(UnixStream::connect(&path).await.unwrap());
    let mut b = BufReader::new(UnixStream::connect(&path).await.unwrap());
    drain_hello(&mut a).await;
    drain_hello(&mut b).await;

    sse.broadcast(AppEvent::Heartbeat);
    let mut la = String::new();
    let mut lb = String::new();
    tokio::time::timeout(Duration::from_secs(2), a.read_line(&mut la))
        .await
        .unwrap()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(2), b.read_line(&mut lb))
        .await
        .unwrap()
        .unwrap();
    assert!(la.contains("heartbeat"));
    assert!(lb.contains("heartbeat"));
}

#[tokio::test]
async fn test_scoped_event_for_other_user_filtered() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("h3.sock");
    let (_chan, sse) = spawn_channel(path.clone()).await;
    let mut a = BufReader::new(UnixStream::connect(&path).await.unwrap());
    drain_hello(&mut a).await;

    // Push a scoped event for a DIFFERENT user.
    sse.broadcast_for_user("not-owner", AppEvent::Heartbeat);
    // Then push a global event we DO want to see.
    sse.broadcast(AppEvent::Heartbeat);

    let mut la = String::new();
    tokio::time::timeout(Duration::from_secs(2), a.read_line(&mut la))
        .await
        .unwrap()
        .unwrap();
    // The line we receive should be the global heartbeat — count proves
    // the filter worked: only ONE line should be in the pipe (the global
    // one). Read with a short timeout to confirm no extra event.
    let mut second = String::new();
    let res = tokio::time::timeout(Duration::from_millis(300), a.read_line(&mut second)).await;
    assert!(res.is_err(), "second read must time out (no extra event)");
    assert!(la.contains("heartbeat"));
}

use futures::StreamExt;
use ironclaw::agent::submission::Submission;
use ironclaw::channels::IncomingMessage;
use uuid::Uuid;

async fn spawn_channel_with_stream(
    socket_path: std::path::PathBuf,
) -> (
    Arc<LocalIpcChannel>,
    Arc<SseManager>,
    std::pin::Pin<Box<dyn futures::Stream<Item = IncomingMessage> + Send>>,
) {
    let sse = Arc::new(SseManager::new());
    let chan = Arc::new(LocalIpcChannel::new(
        socket_path.clone(),
        "owner".into(),
        Arc::clone(&sse),
        16,
    ));
    let stream = chan.start().await.expect("start");
    wait_for_bind(&socket_path).await;
    (chan, sse, stream)
}

#[tokio::test]
async fn test_approval_routes_through_to_inject_stream() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("h4.sock");
    let (_chan, _sse, mut stream) = spawn_channel_with_stream(path.clone()).await;

    let client = UnixStream::connect(&path).await.unwrap();
    let (client_r, mut client_w) = client.into_split();
    let mut reader = BufReader::new(client_r);
    let mut hello = String::new();
    reader.read_line(&mut hello).await.unwrap();

    let req_id = Uuid::new_v4();
    let payload =
        format!("{{\"type\":\"approval\",\"request_id\":\"{req_id}\",\"action\":\"approve\"}}\n");
    client_w.write_all(payload.as_bytes()).await.unwrap();

    let msg = tokio::time::timeout(Duration::from_secs(2), stream.next())
        .await
        .expect("stream timeout")
        .expect("stream ended");
    assert_eq!(msg.channel, "local_ipc");
    // Sideband path: Submission is on structured_submission, NOT in
    // content (content stays empty for control commands).
    assert_eq!(msg.content, "");
    match msg.structured_submission.expect("sideband set") {
        Submission::ExecApproval {
            request_id,
            approved,
            ..
        } => {
            assert_eq!(request_id, req_id);
            assert!(approved);
        }
        other => panic!("expected ExecApproval, got {other:?}"),
    }
}

#[tokio::test]
async fn test_cancel_routes_through_to_inject_stream() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("h5.sock");
    let (_chan, _sse, mut stream) = spawn_channel_with_stream(path.clone()).await;

    let client = UnixStream::connect(&path).await.unwrap();
    let (client_r, mut client_w) = client.into_split();
    let mut reader = BufReader::new(client_r);
    let mut hello = String::new();
    reader.read_line(&mut hello).await.unwrap();

    client_w
        .write_all(b"{\"type\":\"cancel\"}\n")
        .await
        .unwrap();

    let msg = tokio::time::timeout(Duration::from_secs(2), stream.next())
        .await
        .expect("stream timeout")
        .expect("stream ended");
    assert!(matches!(
        msg.structured_submission.expect("sideband set"),
        Submission::Interrupt
    ));
}
