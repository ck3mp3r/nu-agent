use super::*;
use serde_json::json;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// ---------------------------------------------------------------------------
// Message
// ---------------------------------------------------------------------------

#[test]
fn message_roundtrip() -> Result<()> {
    let msg = Message {
        role: Role::User,
        parts: vec![
            Part::Text {
                text: "Hello".to_string(),
            },
            Part::Data {
                data: DataContent {
                    media_type: "application/json".to_string(),
                    schema: json!({"key": 1}),
                },
            },
        ],
        message_id: uuid::Uuid::new_v4().to_string(),
        extensions: None,
        metadata: None,
    };

    let json = serde_json::to_value(&msg)?;
    assert_eq!(json["role"], "ROLE_USER");
    assert_eq!(json["parts"][0]["text"], "Hello");
    assert!(
        json["parts"][0].get("type").is_none(),
        "untagged Part should not have a type key"
    );
    assert_eq!(json["parts"][1]["data"]["mediaType"], "application/json");

    let back: Message = serde_json::from_value(json)?;
    assert_eq!(back, msg);
    Ok(())
}

#[test]
fn message_exact_json() -> Result<()> {
    let json_str =
        r#"{"role":"ROLE_AGENT","parts":[{"type":"text","text":"Sure!"}],"messageId":"msg-1"}"#;
    let msg: Message = serde_json::from_str(json_str)?;
    assert_eq!(msg.role, Role::Agent);
    assert_eq!(msg.parts.len(), 1);
    assert_eq!(msg.message_id, "msg-1");
    assert_eq!(
        msg.parts[0],
        Part::Text {
            text: "Sure!".to_string()
        }
    );
    Ok(())
}
