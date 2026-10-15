use super::*;

use agnt_core::types::{AssistantMessage, CallId, Opaque, Text, ToolName, ToolResultContent};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn test_utility_session_extract_message_info_system() {
    // -- Setup & Fixtures
    let msg = Message::System {
        content: "sys prompt".to_string(),
    };

    // -- Exec
    let (role, content) = extract_message_info(&msg);

    // -- Check
    assert_eq!(role, "system");
    assert_eq!(content, "sys prompt");
}

#[test]
fn test_utility_session_extract_message_info_user_text() {
    // -- Setup & Fixtures
    let msg = Message::User {
        content: vec![UserContent::Text(Text::new("hello"))],
    };

    // -- Exec
    let (role, content) = extract_message_info(&msg);

    // -- Check
    assert_eq!(role, "user");
    assert_eq!(content, "hello");
}

#[test]
fn test_utility_session_extract_message_info_user_media() {
    // -- Setup & Fixtures
    let msg = Message::User {
        content: vec![UserContent::image_url(
            "https://example.test/a.png",
            None,
            None,
        )],
    };

    // -- Exec
    let (role, content) = extract_message_info(&msg);

    // -- Check
    assert_eq!(role, "user");
    assert_eq!(content, "[Media content]");
}

#[test]
fn test_utility_session_extract_message_info_user_tool_result() -> Result<()> {
    // -- Setup & Fixtures
    let msg = Message::User {
        content: vec![UserContent::tool_result(
            CallId::from_wire("call-1"),
            ToolName::new("read").map_err(|e| format!("{e:?}"))?,
            vec![ToolResultContent::Text(Text::new("out"))],
        )],
    };

    // -- Exec
    let (role, content) = extract_message_info(&msg);

    // -- Check
    assert_eq!(role, "user");
    assert!(content.starts_with("Tool result: "), "content={content}");
    Ok(())
}

#[test]
fn test_utility_session_extract_message_info_assistant_text() {
    // -- Setup & Fixtures
    let msg = Message::Assistant(AssistantMessage::new(vec![AssistantContent::text(
        "hi there",
    )]));

    // -- Exec
    let (role, content) = extract_message_info(&msg);

    // -- Check
    assert_eq!(role, "assistant");
    assert_eq!(content, "hi there");
}

#[test]
fn test_utility_session_extract_message_info_assistant_tool_call() -> Result<()> {
    // -- Setup & Fixtures
    let msg = Message::Assistant(AssistantMessage::new(vec![AssistantContent::tool_call(
        "call-1",
        ToolName::new("read").map_err(|e| format!("{e:?}"))?,
        serde_json::json!({ "path": "/tmp/x" }),
    )]));

    // -- Exec
    let (role, content) = extract_message_info(&msg);

    // -- Check
    assert_eq!(role, "assistant");
    assert_eq!(content, "Tool call: read - {\"path\":\"/tmp/x\"}");
    Ok(())
}

#[test]
fn test_utility_session_extract_message_info_assistant_reasoning_and_opaque() -> Result<()> {
    // -- Setup & Fixtures
    let msg = Message::Assistant(AssistantMessage::new(vec![
        AssistantContent::reasoning("thinking"),
        AssistantContent::Opaque(Opaque {
            item: serde_json::json!({}),
            replay: false,
        }),
    ]));

    // -- Exec
    let (role, content) = extract_message_info(&msg);

    // -- Check
    assert_eq!(role, "assistant");
    let mut lines = content.lines();
    assert!(
        lines
            .next()
            .ok_or("should have reasoning line")?
            .starts_with("[Reasoning: ")
    );
    assert_eq!(lines.next().ok_or("should have opaque line")?, "[Opaque]");
    Ok(())
}

#[test]
fn test_utility_session_current_dir_matches_process_cwd() -> Result<()> {
    // -- Exec
    let dir = current_dir()?;

    // -- Check
    assert_eq!(dir, std::env::current_dir()?);
    Ok(())
}
