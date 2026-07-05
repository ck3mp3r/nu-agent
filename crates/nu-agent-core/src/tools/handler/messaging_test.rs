use super::handle_send_message;
use crate::mailbox::MailboxHandle;
use std::time::Duration;
use tempfile::TempDir;

#[tokio::test(flavor = "multi_thread")]
async fn send_message_writes_frame() {
    let dir = TempDir::new().unwrap();
    let handle = MailboxHandle::prepare(dir.path(), "agent-1").unwrap();
    let (_mailbox, rx) = handle.start().unwrap();

    let args = serde_json::json!({
        "to": "agent-1",
        "message": "hello from test"
    });

    let result = handle_send_message(&args, dir.path(), "test-agent").await;
    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
    let val = result.unwrap();
    assert_eq!(val["sent"], true);

    let msg = rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(msg.from, "test-agent");
    assert_eq!(msg.message, "hello from test");
    assert_eq!(msg.kind, "message");
}

#[tokio::test]
async fn send_message_missing_to_errors() {
    let dir = TempDir::new().unwrap();
    let args = serde_json::json!({ "message": "hello" });

    let result = handle_send_message(&args, dir.path(), "me").await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.message.contains("Missing required field: to"));
}

#[tokio::test]
async fn send_message_missing_message_errors() {
    let dir = TempDir::new().unwrap();
    let args = serde_json::json!({ "to": "agent-1" });

    let result = handle_send_message(&args, dir.path(), "me").await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.message.contains("Missing required field: message"));
}
