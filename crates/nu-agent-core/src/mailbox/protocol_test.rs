use super::protocol::MessageFrame;

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
