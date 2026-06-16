use super::protocol::{ClientFrame, ServerFrame};

#[test]
fn client_auth_frame_serializes() {
    let frame = ClientFrame::Auth {
        token: "test-token".to_string(),
    };
    let json = serde_json::to_string(&frame).unwrap();
    assert!(json.contains(r#""type":"auth""#));
    assert!(json.contains(r#""token":"test-token""#));
}

#[test]
fn client_message_frame_serializes() {
    let frame = ClientFrame::Message {
        to: "agent1".to_string(),
        message: "hello".to_string(),
        kind: "message".to_string(),
    };
    let json = serde_json::to_string(&frame).unwrap();
    assert!(json.contains(r#""type":"message""#));
    assert!(json.contains(r#""to":"agent1""#));
    assert!(json.contains(r#""message":"hello""#));
    assert!(json.contains(r#""kind":"message""#));
}

#[test]
fn server_auth_ok_deserializes() {
    let json = r#"{"type":"auth_ok","name":"agent1"}"#;
    let frame: ServerFrame = serde_json::from_str(json).unwrap();
    match frame {
        ServerFrame::AuthOk { name } => {
            assert_eq!(name, "agent1");
        }
        _ => panic!("Expected AuthOk frame"),
    }
}

#[test]
fn roundtrip_client_frame() {
    let frame = ClientFrame::Auth {
        token: "test-token".to_string(),
    };
    let json = serde_json::to_string(&frame).unwrap();
    let parsed: ClientFrame = serde_json::from_str(&json).unwrap();
    match parsed {
        ClientFrame::Auth { token } => {
            assert_eq!(token, "test-token");
        }
        _ => panic!("Expected Auth frame"),
    }
}

#[test]
fn client_frame_message_with_kind_serializes() {
    let frame = ClientFrame::Message {
        to: "agent1".to_string(),
        message: "hello".to_string(),
        kind: "terminate".to_string(),
    };
    let json = serde_json::to_string(&frame).unwrap();
    assert!(json.contains(r#""kind":"terminate""#));
}

#[test]
fn client_frame_message_without_kind_defaults() {
    let json = r#"{"type":"message","to":"agent1","message":"hello"}"#;
    let frame: ClientFrame = serde_json::from_str(json).unwrap();
    match frame {
        ClientFrame::Message { to, message, kind } => {
            assert_eq!(to, "agent1");
            assert_eq!(message, "hello");
            assert_eq!(kind, "message");
        }
        _ => panic!("Expected Message frame"),
    }
}

#[test]
fn server_frame_message_with_kind_round_trips() {
    let frame = ServerFrame::Message {
        from: "agent1".to_string(),
        message: "hello".to_string(),
        kind: "terminate".to_string(),
    };
    let json = serde_json::to_string(&frame).unwrap();
    let parsed: ServerFrame = serde_json::from_str(&json).unwrap();
    match parsed {
        ServerFrame::Message {
            from,
            message,
            kind,
        } => {
            assert_eq!(from, "agent1");
            assert_eq!(message, "hello");
            assert_eq!(kind, "terminate");
        }
        _ => panic!("Expected Message frame"),
    }
}

#[test]
fn incoming_message_defaults_kind() {
    let json = r#"{"type":"message","from":"agent1","message":"hello"}"#;
    let frame: ServerFrame = serde_json::from_str(json).unwrap();
    match frame {
        ServerFrame::Message {
            from,
            message,
            kind,
        } => {
            assert_eq!(from, "agent1");
            assert_eq!(message, "hello");
            assert_eq!(kind, "message");
        }
        _ => panic!("Expected Message frame"),
    }
}
