use super::{Message, MessageRole};
use serde_json;

#[test]
fn message_role_serializes_to_lowercase() {
    let user = MessageRole::User;
    let assistant = MessageRole::Assistant;
    let system = MessageRole::System;
    let tool = MessageRole::Tool;

    assert_eq!(
        serde_json::to_string(&user).unwrap(),
        "\"user\"",
        "User role should serialize to lowercase"
    );
    assert_eq!(
        serde_json::to_string(&assistant).unwrap(),
        "\"assistant\"",
        "Assistant role should serialize to lowercase"
    );
    assert_eq!(
        serde_json::to_string(&system).unwrap(),
        "\"system\"",
        "System role should serialize to lowercase"
    );
    assert_eq!(
        serde_json::to_string(&tool).unwrap(),
        "\"tool\"",
        "Tool role should serialize to lowercase"
    );
}

#[test]
fn message_role_deserializes_from_lowercase() {
    let user: MessageRole = serde_json::from_str("\"user\"").unwrap();
    let assistant: MessageRole = serde_json::from_str("\"assistant\"").unwrap();
    let system: MessageRole = serde_json::from_str("\"system\"").unwrap();
    let tool: MessageRole = serde_json::from_str("\"tool\"").unwrap();

    assert_eq!(user, MessageRole::User);
    assert_eq!(assistant, MessageRole::Assistant);
    assert_eq!(system, MessageRole::System);
    assert_eq!(tool, MessageRole::Tool);
}

#[test]
fn message_with_new_enum_role_serializes_correctly() {
    let msg = Message::new(MessageRole::User, "Hello world".to_string());

    let json = serde_json::to_string(&msg).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(
        parsed.get("role").and_then(|v| v.as_str()),
        Some("user"),
        "Role should serialize to lowercase string"
    );
    assert_eq!(
        parsed.get("content").and_then(|v| v.as_str()),
        Some("Hello world")
    );
}

#[test]
fn message_deserializes_old_format_with_string_role() {
    // Old format: {"role": "user", "content": "hello", "timestamp": "..."}
    let old_format = r#"{
        "role": "user",
        "content": "hello from old format",
        "timestamp": "2024-01-01T00:00:00Z"
    }"#;

    let msg: Message = serde_json::from_str(old_format).unwrap();

    assert_eq!(msg.role(), MessageRole::User);
    assert_eq!(msg.content(), "hello from old format");
}

#[test]
fn message_roundtrip_preserves_role() {
    let original = Message::new(MessageRole::Assistant, "Response text".to_string());

    let json = serde_json::to_string(&original).unwrap();
    let deserialized: Message = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.role(), MessageRole::Assistant);
    assert_eq!(deserialized.content(), "Response text");
}

#[test]
fn message_with_tool_role_supports_tool_fields() {
    let msg = Message::new(MessageRole::Tool, "tool result".to_string())
        .with_tool_call_id("call_123")
        .with_tool_name("calculator");

    assert_eq!(msg.role(), MessageRole::Tool);
    assert_eq!(msg.tool_call_id(), Some("call_123"));
    assert_eq!(msg.tool_name(), Some("calculator"));
}

#[test]
fn message_tool_fields_serialize_and_deserialize() {
    let original = Message::new(MessageRole::Tool, "42".to_string())
        .with_tool_call_id("call_456")
        .with_tool_name("math_eval");

    let json = serde_json::to_string(&original).unwrap();
    let deserialized: Message = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.role(), MessageRole::Tool);
    assert_eq!(deserialized.tool_call_id(), Some("call_456"));
    assert_eq!(deserialized.tool_name(), Some("math_eval"));
}

#[test]
fn message_role_has_string_accessor() {
    assert_eq!(MessageRole::User.as_str(), "user");
    assert_eq!(MessageRole::Assistant.as_str(), "assistant");
    assert_eq!(MessageRole::System.as_str(), "system");
    assert_eq!(MessageRole::Tool.as_str(), "tool");
}

#[test]
fn all_message_roles_deserialize_from_old_strings() {
    let roles = vec!["user", "assistant", "system", "tool"];

    for role_str in roles {
        let json = format!(
            r#"{{"role": "{}", "content": "test", "timestamp": "2024-01-01T00:00:00Z"}}"#,
            role_str
        );

        let msg: Message = serde_json::from_str(&json)
            .unwrap_or_else(|e| panic!("Failed to deserialize role '{}': {}", role_str, e));

        assert_eq!(msg.role().as_str(), role_str);
    }
}
