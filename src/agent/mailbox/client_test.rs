use super::client::{BrokerClient, BrokerClientError};
use super::protocol::{ClientFrame, ServerFrame};
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;

fn temp_socket_path() -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    
    std::env::temp_dir().join(format!(
        "nu-agent-test-{}-{}-{}.sock",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

#[tokio::test]
async fn client_connects_and_authenticates() {
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        let path = temp_socket_path();
        let listener = UnixListener::bind(&path).unwrap();

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (read_half, mut write_half) = stream.into_split();
            let mut reader = BufReader::new(read_half);

            // Read auth frame
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            let frame: ClientFrame = serde_json::from_str(line.trim()).unwrap();
            assert!(matches!(frame, ClientFrame::Auth { .. }));

            // Send AuthOk
            let ok = serde_json::to_string(&ServerFrame::AuthOk {
                name: "test-agent".to_string(),
            })
            .unwrap()
                + "\n";
            write_half.write_all(ok.as_bytes()).await.unwrap();
        });

        let client = BrokerClient::connect(&path, "test-token").await.unwrap();
        assert_eq!(client.name, "test-agent");

        server.await.unwrap();
        let _ = std::fs::remove_file(&path);
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn client_auth_rejected() {
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        let path = temp_socket_path();
        let listener = UnixListener::bind(&path).unwrap();

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (read_half, mut write_half) = stream.into_split();
            let mut reader = BufReader::new(read_half);

            // Read auth frame
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            let frame: ClientFrame = serde_json::from_str(line.trim()).unwrap();
            assert!(matches!(frame, ClientFrame::Auth { .. }));

            // Send AuthRejected
            let rejected = serde_json::to_string(&ServerFrame::AuthRejected {
                reason: "bad token".to_string(),
            })
            .unwrap()
                + "\n";
            write_half.write_all(rejected.as_bytes()).await.unwrap();
        });

        let result = BrokerClient::connect(&path, "test-token").await;
        
        assert!(result.is_err());
        match result.unwrap_err() {
            BrokerClientError::AuthRejected(reason) => {
                assert!(reason.contains("bad token"));
            }
            other => panic!("Expected AuthRejected, got {:?}", other),
        }

        server.await.unwrap();
        let _ = std::fs::remove_file(&path);
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn client_sends_message() {
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        let path = temp_socket_path();
        let listener = UnixListener::bind(&path).unwrap();

        let server = tokio::spawn(async move {
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

            // Read message frame
            let mut msg_line = String::new();
            reader.read_line(&mut msg_line).await.unwrap();
            let msg_frame: ClientFrame = serde_json::from_str(msg_line.trim()).unwrap();

            match msg_frame {
                ClientFrame::Message { to, message } => {
                    assert_eq!(to, "target");
                    assert_eq!(message, "hello");
                }
                other => panic!("Expected Message frame, got {:?}", other),
            }
        });

        let mut client = BrokerClient::connect(&path, "test-token").await.unwrap();
        client.send("target", "hello").await.unwrap();

        server.await.unwrap();
        let _ = std::fs::remove_file(&path);
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn client_receives_message() {
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        let path = temp_socket_path();
        let listener = UnixListener::bind(&path).unwrap();

        let server = tokio::spawn(async move {
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

            // Send Message frame
            let msg = serde_json::to_string(&ServerFrame::Message {
                from: "other".to_string(),
                message: "hi".to_string(),
            })
            .unwrap()
                + "\n";
            write_half.write_all(msg.as_bytes()).await.unwrap();
        });

        let mut client = BrokerClient::connect(&path, "test-token").await.unwrap();
        let frame = client.recv().await.unwrap();

        match frame {
            ServerFrame::Message { from, message } => {
                assert_eq!(from, "other");
                assert_eq!(message, "hi");
            }
            other => panic!("Expected Message frame, got {:?}", other),
        }

        server.await.unwrap();
        let _ = std::fs::remove_file(&path);
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn client_detects_disconnect() {
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        let path = temp_socket_path();
        let listener = UnixListener::bind(&path).unwrap();

        let server = tokio::spawn(async move {
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

            // Drop connection entirely - both halves
            drop(write_half);
            drop(reader);
        });

        let mut client = BrokerClient::connect(&path, "test-token").await.unwrap();
        
        // Small sleep to let server drop the connection
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        
        let result = client.recv().await;

        assert!(result.is_err());
        match result.unwrap_err() {
            BrokerClientError::Disconnected => {
                // Expected
            }
            other => panic!("Expected Disconnected, got {:?}", other),
        }

        server.await.unwrap();
        let _ = std::fs::remove_file(&path);
    })
    .await
    .expect("test timed out");
}
