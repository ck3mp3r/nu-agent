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
    };
    let json = serde_json::to_string(&frame).unwrap();
    assert!(json.contains(r#""type":"message""#));
    assert!(json.contains(r#""to":"agent1""#));
    assert!(json.contains(r#""message":"hello""#));
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
