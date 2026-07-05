use super::broker::MailboxHandle;
use super::client::{SendError, send_to};
use std::time::Duration;
use tempfile::TempDir;

#[tokio::test(flavor = "multi_thread")]
async fn send_to_delivers_message() {
    let dir = TempDir::new().unwrap();
    let handle = MailboxHandle::prepare(dir.path(), "target").unwrap();
    let (_mailbox, rx) = handle.start().unwrap();
    send_to(dir.path(), "target", "origin", "payload", "ping")
        .await
        .unwrap();
    let msg = rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(msg.from, "origin");
    assert_eq!(msg.message, "payload");
    assert_eq!(msg.kind, "ping");
}

#[tokio::test]
async fn send_to_returns_socket_not_found_when_target_absent() {
    let dir = TempDir::new().unwrap();
    let err = send_to(dir.path(), "nobody", "me", "hi", "message")
        .await
        .unwrap_err();
    assert!(matches!(err, SendError::SocketNotFound(_)));
}

#[tokio::test(flavor = "multi_thread")]
async fn send_to_custom_kind() {
    let dir = TempDir::new().unwrap();
    let handle = MailboxHandle::prepare(dir.path(), "t").unwrap();
    let (_mailbox, rx) = handle.start().unwrap();
    send_to(dir.path(), "t", "s", "body", "ping").await.unwrap();
    let msg = rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(msg.kind, "ping");
}
