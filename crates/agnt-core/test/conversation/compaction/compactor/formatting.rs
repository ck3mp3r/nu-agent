//! `format_messages` depth tests.
//!
//! The summarizer input is the only place the evicted history is rendered, so
//! every content type must reach the prompt in a form the model can read. These
//! tests drive `compact()` and inspect the recorded summarizer request.

use super::*;

use crate::types::{
    AssistantContent, AssistantMessage, CallId, Opaque, Text, ToolCall, ToolFunction, ToolName,
    ToolResult, ToolResultContent,
};
use rig::completion::message::{Audio, Document, Image, Reasoning, Video};

// region:    --- Test Support

/// A batch holding one of every content type `format_messages` renders.
fn rich_batch() -> Result<Vec<Message>> {
    Ok(vec![
        Message::user("plain user text"),
        Message::Assistant(AssistantMessage::new(vec![
            AssistantContent::Text(Text::new("assistant text")),
            AssistantContent::ToolCall(ToolCall {
                id: CallId::from_wire("call-1"),
                function: ToolFunction::new(
                    ToolName::new("read")?,
                    serde_json::json!({"path": "a.txt"}),
                ),
                native: None,
            }),
            AssistantContent::Reasoning(Reasoning::new("thinking")),
            AssistantContent::Image(Image::default()),
            AssistantContent::Opaque(Opaque {
                item: serde_json::json!({"type": "hosted_tool"}),
                replay: false,
            }),
        ])),
        Message::User {
            content: vec![
                UserContent::ToolResult(ToolResult {
                    call: CallId::from_wire("call-1"),
                    name: ToolName::new("read")?,
                    content: vec![ToolResultContent::Text(Text::new("file body"))],
                    is_error: false,
                }),
                UserContent::Image(Image::default()),
                UserContent::Audio(Audio::default()),
                UserContent::Video(Video::default()),
                UserContent::Document(Document::default()),
            ],
        },
        Message::System {
            content: "system text".to_string(),
        },
    ])
}

// endregion: --- Test Support

#[tokio::test]
async fn format_messages_renders_every_content_type_into_the_summarizer_prompt() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::default();
    let model = stream_model(&["summary"]);
    let compactor = make_compactor(model.clone(), bus, None);
    let evicted = rich_batch()?;

    // -- Exec
    compactor
        .compact("conv-1", &evicted, None, "test_source")
        .await
        .map_err(|e| format!("compact should succeed: {e:?}"))?;

    // -- Check
    let prompt = last_request_prompt(&model)?;
    for expected in [
        "user: plain user text",
        "assistant: assistant text",
        "[Assistant tool call]: read(",
        "[Assistant reasoning]:",
        "[Attached image]",
        "[Provider item]",
        "[Tool result]: file body",
        "[Attached audio]",
        "[Attached video]",
        "[Attached document]",
        "system: system text",
    ] {
        assert!(
            prompt.contains(expected),
            "the summarizer prompt must contain {expected:?}, got: {prompt}"
        );
    }
    assert!(
        prompt.contains("a.txt"),
        "the tool-call arguments must reach the prompt, got: {prompt}"
    );
    Ok(())
}

#[tokio::test]
async fn format_messages_truncates_an_oversized_tool_result() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::default();
    let model = stream_model(&["summary"]);
    let compactor = make_compactor(model.clone(), bus, None);
    let long_result = "z".repeat(2_500);
    let evicted = vec![Message::User {
        content: vec![UserContent::ToolResult(ToolResult {
            call: CallId::from_wire("call-1"),
            name: ToolName::new("read")?,
            content: vec![ToolResultContent::Text(Text::new(long_result))],
            is_error: false,
        })],
    }];

    // -- Exec
    compactor
        .compact("conv-1", &evicted, None, "test_source")
        .await
        .map_err(|e| format!("compact should succeed: {e:?}"))?;

    // -- Check
    let prompt = last_request_prompt(&model)?;
    assert!(
        prompt.contains("[Tool result]: zzz"),
        "the tool result must be rendered, got: {prompt}"
    );
    assert!(
        prompt.contains('…'),
        "an oversized tool result must be truncated with an ellipsis, got: {prompt}"
    );
    assert!(
        !prompt.contains(&"z".repeat(2_001)),
        "the tool result must be capped at 2000 characters"
    );
    Ok(())
}
