//! Unit tests for the turn-delta repair.
//!
//! `close_unanswered_tool_calls_with` closes the tool calls a cancelled or
//! failed turn never answered, preferring the result the turn observed.

use crate::session::repair::{UNANSWERED_RESULT_TEXT, close_unanswered_tool_calls_with};
use crate::types::{
    AssistantContent, AssistantMessage, CallId, Message, ToolCall, ToolFunction, ToolName,
    ToolResultContent, UserContent,
};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// An assistant message calling each id in `call_ids`, with no result anywhere.
fn assistant_tool_calls(call_ids: &[&str]) -> Result<Message> {
    let name = ToolName::new("read_file")?;
    let content = call_ids
        .iter()
        .map(|call_id| {
            AssistantContent::ToolCall(ToolCall::new(
                CallId::from_wire(*call_id),
                ToolFunction::new(name.clone(), serde_json::json!({})),
            ))
        })
        .collect();
    Ok(Message::Assistant(AssistantMessage::new(content)))
}

/// A call the turn settled closes with the real result text and error flag.
#[test]
fn close_unanswered_tool_calls_with_prefers_the_settled_result() -> Result<()> {
    // -- Setup & Fixtures
    let messages = vec![assistant_tool_calls(&["tc1"])?];
    let results = vec![(CallId::from_wire("tc1"), "boom".to_string(), true)];

    // -- Exec
    let closed = close_unanswered_tool_calls_with(messages, &results);

    // -- Check
    let Message::User { content } = &closed[1] else {
        return Err("the close must be a user message".into());
    };
    let UserContent::ToolResult(result) = &content[0] else {
        return Err("the close must be a tool result".into());
    };
    assert!(
        matches!(&result.content[0], ToolResultContent::Text(text) if text.text == "boom"),
        "the settled result text is kept; got {result:?}"
    );
    assert!(
        result.is_error,
        "the settled error flag is kept; got {result:?}"
    );
    Ok(())
}

/// A call the turn never settled closes with the unanswered marker.
#[test]
fn close_unanswered_tool_calls_with_marks_an_unsettled_call_unanswered() -> Result<()> {
    // -- Setup & Fixtures
    let messages = vec![assistant_tool_calls(&["tc1"])?];
    let results = vec![(CallId::from_wire("tc2"), "other".to_string(), false)];

    // -- Exec
    let closed = close_unanswered_tool_calls_with(messages, &results);

    // -- Check
    let Message::User { content } = &closed[1] else {
        return Err("the close must be a user message".into());
    };
    let UserContent::ToolResult(result) = &content[0] else {
        return Err("the close must be a tool result".into());
    };
    assert!(
        matches!(&result.content[0], ToolResultContent::Text(text) if text.text == UNANSWERED_RESULT_TEXT),
        "an unsettled call carries the unanswered marker; got {result:?}"
    );
    assert!(
        !result.is_error,
        "an interrupted close is not an error; got {result:?}"
    );
    Ok(())
}
