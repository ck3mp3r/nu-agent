use super::{handle_list_agents, handle_send_message};
use crate::agent::mailbox::{BrokerSender, BrokerClient, AgentRegistry, ClientFrame, ServerFrame};
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use std::path::PathBuf;

fn temp_socket_path() -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    std::env::temp_dir().join(format!(
        "nu-agent-msg-test-{}-{}-{}.sock",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

/// Create a mock BrokerSender for testing without actual network (for error-path tests)
fn create_mock_broker_sender() -> BrokerSender {
    let (tx, _rx) = std::os::unix::net::UnixStream::pair().expect("Failed to create socket pair");
    tx.set_nonblocking(true).expect("Failed to set nonblocking");
    let tokio_stream = tokio::net::UnixStream::from_std(tx).expect("Failed to convert to tokio stream");
    let (_read, write) = tokio_stream.into_split();
    BrokerSender::new_for_test(write)
}

#[tokio::test(flavor = "multi_thread")]
async fn send_message_writes_frame() {
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        let path = temp_socket_path();
        let listener = UnixListener::bind(&path).unwrap();

        let server = tokio::spawn({
            let path = path.clone();
            async move {
                let (stream, _) = listener.accept().await.unwrap();
                let (read_half, mut write_half) = stream.into_split();
                let mut reader = BufReader::new(read_half);

                // Read auth frame
                let mut line = String::new();
                reader.read_line(&mut line).await.unwrap();
                let _frame: ClientFrame = serde_json::from_str(line.trim()).unwrap();

                // Send AuthOk
                let ok = serde_json::to_string(&ServerFrame::AuthOk {
                    name: "test-agent".to_string(),
                })
                .unwrap()
                    + "\n";
                write_half.write_all(ok.as_bytes()).await.unwrap();

                // Read the message frame sent by handle_send_message
                let mut msg_line = String::new();
                reader.read_line(&mut msg_line).await.unwrap();
                let msg_frame: ClientFrame = serde_json::from_str(msg_line.trim()).unwrap();

                match msg_frame {
                    ClientFrame::Message { to, message, kind } => {
                        assert_eq!(to, "agent-1");
                        assert_eq!(message, "hello from test");
                        assert_eq!(kind, "message");
                    }
                    other => panic!("Expected Message frame, got {:?}", other),
                }

                drop(write_half);
                drop(reader);
                let _ = std::fs::remove_file(&path);
            }
        });

        let client = BrokerClient::connect(&path, "test-token").await.unwrap();
        let (mut sender, _receiver) = client.split();

        let args = serde_json::json!({
            "to": "agent-1",
            "message": "hello from test"
        });

        let result = handle_send_message(&args, &mut sender).await;

        assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
        let val = result.unwrap();
        assert_eq!(val["sent"], true);

        server.await.unwrap();
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn send_message_missing_to_errors() {
    let args = serde_json::json!({
        "message": "hello"
    });
    
    let mut sender = create_mock_broker_sender();
    let result = handle_send_message(&args, &mut sender).await;
    
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.message.contains("Missing required field: to"));
}

#[tokio::test]
async fn send_message_missing_message_errors() {
    let args = serde_json::json!({
        "to": "agent-1"
    });
    
    let mut sender = create_mock_broker_sender();
    let result = handle_send_message(&args, &mut sender).await;
    
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.message.contains("Missing required field: message"));
}

#[tokio::test(flavor = "multi_thread")]
async fn list_agents_returns_connected() {
    let registry = Arc::new(RwLock::new(AgentRegistry::new()));
    
    // Add two connected agents
    {
        let mut reg = registry.write().await;
        let (tx1, _rx1) = mpsc::channel(1);
        let (tx2, _rx2) = mpsc::channel(1);
        reg.add_connected("agent-1".to_string(), tx1);
        reg.add_connected("agent-2".to_string(), tx2);
    }
    
    let result = handle_list_agents(&registry).expect("list_agents failed");
    
    let agents = result.as_array().expect("Expected array result");
    assert_eq!(agents.len(), 2);
    
    let names: Vec<String> = agents
        .iter()
        .map(|a| a["name"].as_str().unwrap().to_string())
        .collect();
    
    assert!(names.contains(&"agent-1".to_string()));
    assert!(names.contains(&"agent-2".to_string()));
}

#[tokio::test(flavor = "multi_thread")]
async fn list_agents_empty_registry() {
    let registry = Arc::new(RwLock::new(AgentRegistry::new()));
    
    let result = handle_list_agents(&registry).expect("list_agents failed");
    
    let agents = result.as_array().expect("Expected array result");
    assert_eq!(agents.len(), 0);
}
