use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::RwLock;

use super::broker::Broker;
use super::protocol::{ClientFrame, ServerFrame};
use super::registry::AgentRegistry;

#[tokio::test]
async fn broker_starts_and_accepts_auth() {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let registry = Arc::new(RwLock::new(AgentRegistry::new()));
        
        // Register pending token
        {
            let mut reg = registry.write().await;
            reg.register_pending("test-token".to_string(), "agent1".to_string());
        }
        
        let broker = Broker::start(registry.clone()).unwrap();
        let socket_path = broker.socket_path().to_path_buf();
        
        // Connect and authenticate
        let stream = UnixStream::connect(&socket_path).await.unwrap();
        let (read_half, mut write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);
        
        // Send auth frame
        let auth_frame = ClientFrame::Auth {
            token: "test-token".to_string(),
        };
        let auth_json = serde_json::to_string(&auth_frame).unwrap();
        write_half
            .write_all(format!("{}\n", auth_json).as_bytes())
            .await
            .unwrap();
        
        // Read response
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        
        let response: ServerFrame = serde_json::from_str(line.trim()).unwrap();
        match response {
            ServerFrame::AuthOk { name } => {
                assert_eq!(name, "agent1");
            }
            _ => panic!("Expected AuthOk"),
        }
        
        // Verify agent is connected
        let reg = registry.read().await;
        assert!(reg.is_connected("agent1"));
    }).await.expect("test timed out");
}

#[tokio::test]
async fn broker_rejects_invalid_token() {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let registry = Arc::new(RwLock::new(AgentRegistry::new()));
        let broker = Broker::start(registry.clone()).unwrap();
        let socket_path = broker.socket_path().to_path_buf();
        
        // Connect with invalid token
        let stream = UnixStream::connect(&socket_path).await.unwrap();
        let (read_half, mut write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);
        
        // Send auth frame with wrong token
        let auth_frame = ClientFrame::Auth {
            token: "wrong-token".to_string(),
        };
        let auth_json = serde_json::to_string(&auth_frame).unwrap();
        write_half
            .write_all(format!("{}\n", auth_json).as_bytes())
            .await
            .unwrap();
        
        // Read response
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        
        let response: ServerFrame = serde_json::from_str(line.trim()).unwrap();
        match response {
            ServerFrame::AuthRejected { reason } => {
                assert!(reason.contains("Invalid token"));
            }
            _ => panic!("Expected AuthRejected"),
        }
    }).await.expect("test timed out");
}

#[tokio::test]
async fn broker_routes_message_between_agents() {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let registry = Arc::new(RwLock::new(AgentRegistry::new()));
        
        // Register two agents
        {
            let mut reg = registry.write().await;
            reg.register_pending("token1".to_string(), "agent1".to_string());
            reg.register_pending("token2".to_string(), "agent2".to_string());
        }
        
        let broker = Broker::start(registry.clone()).unwrap();
        let socket_path = broker.socket_path().to_path_buf();
        
        // Connect agent1
        let stream1 = UnixStream::connect(&socket_path).await.unwrap();
        let (read_half1, mut write_half1) = stream1.into_split();
        let mut reader1 = BufReader::new(read_half1);
        
        let auth1 = ClientFrame::Auth {
            token: "token1".to_string(),
        };
        write_half1
            .write_all(format!("{}\n", serde_json::to_string(&auth1).unwrap()).as_bytes())
            .await
            .unwrap();
        
        // Read auth response for agent1
        let mut line = String::new();
        reader1.read_line(&mut line).await.unwrap();
        
        // Connect agent2
        let stream2 = UnixStream::connect(&socket_path).await.unwrap();
        let (read_half2, mut write_half2) = stream2.into_split();
        let mut reader2 = BufReader::new(read_half2);
        
        let auth2 = ClientFrame::Auth {
            token: "token2".to_string(),
        };
        write_half2
            .write_all(format!("{}\n", serde_json::to_string(&auth2).unwrap()).as_bytes())
            .await
            .unwrap();
        
        // Read auth response for agent2
        let mut line2 = String::new();
        reader2.read_line(&mut line2).await.unwrap();
        
        // Agent1 sends message to agent2
        let message = ClientFrame::Message {
            to: "agent2".to_string(),
            message: "hello from agent1".to_string(),
            kind: "message".to_string(),
        };
        write_half1
            .write_all(format!("{}\n", serde_json::to_string(&message).unwrap()).as_bytes())
            .await
            .unwrap();
        
        // Agent2 should receive the message
        let mut msg_line = String::new();
        reader2.read_line(&mut msg_line).await.unwrap();
        
        let received: ServerFrame = serde_json::from_str(msg_line.trim()).unwrap();
        match received {
            ServerFrame::Message { from, message, kind } => {
                assert_eq!(from, "agent1");
                assert_eq!(message, "hello from agent1");
                assert_eq!(kind, "message");
            }
            _ => panic!("Expected Message frame"),
        }
    }).await.expect("test timed out");
}

#[tokio::test]
async fn broker_handles_disconnect() {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let registry = Arc::new(RwLock::new(AgentRegistry::new()));
        
        {
            let mut reg = registry.write().await;
            reg.register_pending("token1".to_string(), "agent1".to_string());
        }
        
        let broker = Broker::start(registry.clone()).unwrap();
        let socket_path = broker.socket_path().to_path_buf();
        
        // Connect agent
        let stream = UnixStream::connect(&socket_path).await.unwrap();
        let (read_half, mut write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);
        
        // Authenticate
        let auth = ClientFrame::Auth {
            token: "token1".to_string(),
        };
        write_half
            .write_all(format!("{}\n", serde_json::to_string(&auth).unwrap()).as_bytes())
            .await
            .unwrap();
        
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        
        // Verify connected
        assert!(registry.read().await.is_connected("agent1"));
        
        // Drop connection
        drop(write_half);
        drop(reader);
        
        // Wait a bit for cleanup
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        
        // Verify agent removed
        assert!(!registry.read().await.is_connected("agent1"));
    }).await.expect("test timed out");
}

#[tokio::test]
async fn broker_cleanup_on_drop() {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let registry = Arc::new(RwLock::new(AgentRegistry::new()));
        let broker = Broker::start(registry.clone()).unwrap();
        let socket_path = broker.socket_path().to_path_buf();
        let socket_dir = socket_path.parent().unwrap().to_path_buf();
        
        // Verify socket exists
        assert!(socket_path.exists());
        assert!(socket_dir.exists());
        
        // Drop broker
        drop(broker);
        
        // Wait a bit for cleanup
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        
        // Verify cleaned up
        assert!(!socket_path.exists());
        assert!(!socket_dir.exists());
    }).await.expect("test timed out");
}
