// === Imports ===

use super::broker::{MailboxHandle, socket_dir_for_path};
use super::client::{SendError, send_to};
use super::protocol::MessageFrame;
use std::time::Duration;
use tempfile::TempDir;

// === Tests: broker ===

#[tokio::test]
async fn mailbox_binds_socket_file() {
    let dir = TempDir::new().unwrap();
    let handle = MailboxHandle::prepare(dir.path(), "alpha").unwrap();
    let (_mailbox, _rx) = handle.start().unwrap();
    assert!(dir.path().join("alpha.sock").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn mailbox_receives_message() {
    let dir = TempDir::new().unwrap();
    let handle = MailboxHandle::prepare(dir.path(), "alpha").unwrap();
    let (_mailbox, rx) = handle.start().unwrap();
    send_to(dir.path(), "alpha", "beta", "hello", "message")
        .await
        .unwrap();
    let msg = rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(msg.from, "beta");
    assert_eq!(msg.message, "hello");
    assert_eq!(msg.kind, "message");
}

#[tokio::test]
async fn mailbox_drop_removes_socket() {
    let dir = TempDir::new().unwrap();
    let socket_path = dir.path().join("alpha.sock");
    let handle = MailboxHandle::prepare(dir.path(), "alpha").unwrap();
    let (mailbox, _rx) = handle.start().unwrap();
    assert!(socket_path.exists());
    drop(mailbox);
    assert!(!socket_path.exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn mailbox_multiple_messages_delivered() {
    let dir = TempDir::new().unwrap();
    let handle = MailboxHandle::prepare(dir.path(), "alpha").unwrap();
    let (_mailbox, rx) = handle.start().unwrap();
    for i in 0..3u32 {
        send_to(dir.path(), "alpha", "sender", &format!("msg{i}"), "message")
            .await
            .unwrap();
    }
    let mut received = Vec::new();
    for _ in 0..3 {
        received.push(rx.recv_timeout(Duration::from_secs(2)).unwrap().message);
    }
    assert_eq!(received.len(), 3);
}

#[tokio::test]
async fn mailbox_rebind_after_drop_succeeds() {
    let dir = TempDir::new().unwrap();
    let handle = MailboxHandle::prepare(dir.path(), "alpha").unwrap();
    let (mailbox, _rx) = handle.start().unwrap();
    drop(mailbox);
    let handle2 = MailboxHandle::prepare(dir.path(), "alpha").unwrap();
    let (_mailbox2, _rx2) = handle2.start().unwrap();
    assert!(dir.path().join("alpha.sock").exists());
}

// Regression: a top-level named agent (--name dave, no --parent-name) must
// keep its socket alive for the entire session. Before the fix, builder.rs
// prepared TWO handles for the same name — one via the orchestrator path,
// one via the child path. Dropping the unstarted orchestrator handle deleted
// the socket the started child mailbox was bound to. Net result: no socket.
//
// This test documents the failure mode: an unstarted handle's Drop removes
// the socket even if another started handle is listening on it.
// The fix in builder.rs (child handle only when parent_name.is_some())
// ensures only ONE handle is ever prepared for a given agent name.
#[tokio::test]
async fn unstarted_handle_drop_removes_socket_from_started_mailbox() {
    let dir = TempDir::new().unwrap();
    let socket_path = dir.path().join("dave.sock");

    // First handle: prepared but not started (simulates orch_handle in old code)
    let orch_handle = MailboxHandle::prepare(dir.path(), "dave").unwrap();
    assert!(socket_path.exists());

    // Second handle: steals the socket path (simulates child_handle in old code)
    let child_handle = MailboxHandle::prepare(dir.path(), "dave").unwrap();
    let (child_mailbox, _rx) = child_handle.start().unwrap();
    assert!(socket_path.exists());

    // Dropping the unstarted handle deletes the socket — this is the bug
    drop(orch_handle);
    assert!(
        !socket_path.exists(),
        "unstarted handle Drop removes the socket, breaking the started mailbox — \
         builder.rs must never create two handles for the same agent name"
    );

    drop(child_mailbox);
}

// Correct pattern: one prepare + one start = socket lives until AgentMailbox drop.
#[tokio::test]
async fn single_prepare_and_start_socket_lives_for_mailbox_lifetime() {
    let dir = TempDir::new().unwrap();
    let socket_path = dir.path().join("dave.sock");

    let handle = MailboxHandle::prepare(dir.path(), "dave").unwrap();
    assert!(socket_path.exists(), "prepare must create the socket file");

    let (mailbox, _rx) = handle.start().unwrap();
    assert!(socket_path.exists(), "socket must remain after start()");

    drop(mailbox);
    assert!(
        !socket_path.exists(),
        "socket must be removed when AgentMailbox is dropped"
    );
}

#[test]
fn socket_dir_for_path_is_deterministic() {
    use std::path::PathBuf;
    let cwd = PathBuf::from("/some/project");
    assert_eq!(socket_dir_for_path(&cwd), socket_dir_for_path(&cwd));
}

#[test]
fn socket_dir_for_path_differs_for_different_cwds() {
    use std::path::PathBuf;
    let a = socket_dir_for_path(&PathBuf::from("/project/a"));
    let b = socket_dir_for_path(&PathBuf::from("/project/b"));
    assert_ne!(a, b);
}

// === Tests: client ===

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

// === Tests: protocol ===

#[test]
fn message_frame_serializes() {
    let frame = MessageFrame {
        from: "alice".to_string(),
        message: "hello".to_string(),
        kind: "message".to_string(),
    };
    let json = serde_json::to_string(&frame).unwrap();
    assert!(json.contains("\"from\":\"alice\""));
    assert!(json.contains("\"message\":\"hello\""));
}

#[test]
fn message_frame_deserializes() {
    let json = r#"{"from":"bob","message":"hi","kind":"ping"}"#;
    let frame: MessageFrame = serde_json::from_str(json).unwrap();
    assert_eq!(frame.from, "bob");
    assert_eq!(frame.kind, "ping");
}

#[test]
fn kind_defaults_to_message_when_missing() {
    let json = r#"{"from":"bob","message":"hi"}"#;
    let frame: MessageFrame = serde_json::from_str(json).unwrap();
    assert_eq!(frame.kind, "message");
}

#[test]
fn roundtrip_preserves_all_fields() {
    let frame = MessageFrame {
        from: "x".to_string(),
        message: "y".to_string(),
        kind: "z".to_string(),
    };
    let rt: MessageFrame = serde_json::from_str(&serde_json::to_string(&frame).unwrap()).unwrap();
    assert_eq!(rt.from, frame.from);
    assert_eq!(rt.message, frame.message);
    assert_eq!(rt.kind, frame.kind);
}
