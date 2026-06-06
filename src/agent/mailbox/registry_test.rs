use super::protocol::ServerFrame;
use super::registry::AgentRegistry;

#[test]
fn register_pending_and_validate() {
    let mut registry = AgentRegistry::new();
    registry.register_pending("token1".to_string(), "agent1".to_string());
    
    // Token should exist before authentication
    let name = registry.authenticate("token1");
    assert_eq!(name, Some("agent1".to_string()));
}

#[test]
fn pending_consumed_on_connect() {
    let mut registry = AgentRegistry::new();
    registry.register_pending("token1".to_string(), "agent1".to_string());
    
    // First authentication succeeds
    let name1 = registry.authenticate("token1");
    assert_eq!(name1, Some("agent1".to_string()));
    
    // Second authentication fails (token consumed)
    let name2 = registry.authenticate("token1");
    assert_eq!(name2, None);
}

#[tokio::test]
async fn route_message_to_connected() {
    let mut registry = AgentRegistry::new();
    let (tx, mut rx) = tokio::sync::mpsc::channel(10);
    
    registry.add_connected("agent1".to_string(), tx);
    
    let frame = ServerFrame::Message {
        from: "agent2".to_string(),
        message: "hello".to_string(),
        kind: "message".to_string(),
    };
    
    let result = registry.route_message("agent1", frame);
    assert!(result.is_ok());
    
    // Verify message received
    let received = rx.recv().await.unwrap();
    match received {
        ServerFrame::Message { from, message, kind } => {
            assert_eq!(from, "agent2");
            assert_eq!(message, "hello");
            assert_eq!(kind, "message");
        }
        _ => panic!("Expected Message frame"),
    }
}

#[test]
fn route_message_unknown_agent_errors() {
    let registry = AgentRegistry::new();
    
    let frame = ServerFrame::Message {
        from: "agent1".to_string(),
        message: "hello".to_string(),
        kind: "message".to_string(),
    };
    
    let result = registry.route_message("unknown", frame);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("not connected"));
}

#[test]
fn connected_names_returns_all() {
    let mut registry = AgentRegistry::new();
    let (tx1, _rx1) = tokio::sync::mpsc::channel(10);
    let (tx2, _rx2) = tokio::sync::mpsc::channel(10);
    
    registry.add_connected("agent1".to_string(), tx1);
    registry.add_connected("agent2".to_string(), tx2);
    
    let names = registry.connected_names();
    assert_eq!(names.len(), 2);
    assert!(names.contains(&"agent1".to_string()));
    assert!(names.contains(&"agent2".to_string()));
}
