//! Unit tests for the crate's pure builders.

use crate::{sse_response_template, sse_text_body, sse_tool_call_body};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- sse_text_body

#[test]
fn test_sse_text_body_short_text_emits_single_content_frame() -> Result<()> {
    // -- Setup & Fixtures
    let text = "hello";

    // -- Exec
    let body = sse_text_body(text);

    // -- Check
    let frames = frames(&body);
    assert_eq!(frames.len(), 3, "got frames: {frames:?}");
    assert_eq!(
        frames[0],
        "data: {\"id\":\"chatcmpl-test\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hello\"},\"finish_reason\":null}]}"
    );
    assert_eq!(frames[1], STOP_FRAME);
    assert_eq!(frames[2], DONE_FRAME);
    Ok(())
}

#[test]
fn test_sse_text_body_empty_text_emits_only_terminal_frames() -> Result<()> {
    // -- Setup & Fixtures
    let text = "";

    // -- Exec
    let body = sse_text_body(text);

    // -- Check
    let frames = frames(&body);
    assert_eq!(frames.len(), 2, "got frames: {frames:?}");
    assert_eq!(frames[0], STOP_FRAME);
    assert_eq!(frames[1], DONE_FRAME);
    Ok(())
}

#[test]
fn test_sse_text_body_long_text_splits_into_twenty_char_chunks() -> Result<()> {
    // -- Setup & Fixtures
    let text = "abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJ";

    // -- Exec
    let body = sse_text_body(text);

    // -- Check
    let frames = frames(&body);
    assert_eq!(frames.len(), 5, "got frames: {frames:?}");
    assert_eq!(frames[0], content_frame("abcdefghijklmnopqrst"));
    assert_eq!(frames[1], content_frame("uvwxyz0123456789ABCD"));
    assert_eq!(frames[2], content_frame("EFGHIJ"));
    assert_eq!(frames[3], STOP_FRAME);
    assert_eq!(frames[4], DONE_FRAME);
    Ok(())
}

#[test]
fn test_sse_text_body_chunks_by_char_not_byte() -> Result<()> {
    // -- Setup & Fixtures
    // 21 two-byte characters: 42 bytes, 21 chars, so two chunks of 20 and 1.
    let text = "é".repeat(21);

    // -- Exec
    let body = sse_text_body(&text);

    // -- Check
    let frames = frames(&body);
    assert_eq!(frames.len(), 4, "got frames: {frames:?}");
    assert_eq!(frames[0], content_frame(&"é".repeat(20)));
    assert_eq!(frames[1], content_frame("é"));
    assert_eq!(frames[2], STOP_FRAME);
    assert_eq!(frames[3], DONE_FRAME);
    Ok(())
}

#[test]
fn test_sse_text_body_escapes_content_as_json_string() -> Result<()> {
    // -- Setup & Fixtures
    let text = "a\"b\\c";

    // -- Exec
    let body = sse_text_body(text);

    // -- Check
    let frames = frames(&body);
    assert_eq!(frames.len(), 3, "got frames: {frames:?}");
    assert_eq!(
        frames[0],
        "data: {\"id\":\"chatcmpl-test\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"a\\\"b\\\\c\"},\"finish_reason\":null}]}"
    );
    Ok(())
}

// endregion: --- sse_text_body

// region:    --- sse_tool_call_body

#[test]
fn test_sse_tool_call_body_emits_three_frames_then_done() -> Result<()> {
    // -- Setup & Fixtures
    let id = "tc1";
    let name = "nu__shell";
    let args = "{\"command\":\"git pull\"}";

    // -- Exec
    let body = sse_tool_call_body(id, name, args);

    // -- Check
    let frames = frames(&body);
    assert_eq!(frames.len(), 4, "got frames: {frames:?}");
    assert_eq!(
        frames[0],
        "data: {\"id\":\"chatcmpl-test\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"tc1\",\"type\":\"function\",\"function\":{\"name\":\"nu__shell\",\"arguments\":\"\"}}]},\"finish_reason\":null}]}"
    );
    assert_eq!(
        frames[1],
        "data: {\"id\":\"chatcmpl-test\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"command\\\":\\\"git pull\\\"}\"}}]},\"finish_reason\":null}]}"
    );
    assert_eq!(frames[2], TOOL_CALLS_FRAME);
    assert_eq!(frames[3], DONE_FRAME);
    Ok(())
}

#[test]
fn test_sse_tool_call_body_encodes_arguments_as_json_string() -> Result<()> {
    // -- Setup & Fixtures
    let args = "{\"pattern\":\"needle\"}";

    // -- Exec
    let body = sse_tool_call_body("call_0", "grep", args);

    // -- Check
    let frames = frames(&body);
    assert_eq!(frames.len(), 4, "got frames: {frames:?}");
    assert!(
        frames[1].contains("\"arguments\":\"{\\\"pattern\\\":\\\"needle\\\"}\""),
        "arguments must be a JSON string literal; got: {}",
        frames[1]
    );
    Ok(())
}

// endregion: --- sse_tool_call_body

// region:    --- sse_response_template

#[test]
fn test_sse_response_template_carries_status_header_and_body() -> Result<()> {
    // -- Setup & Fixtures
    let body = "data: [DONE]\n\n".to_string();

    // -- Exec
    let template = sse_response_template(body);

    // -- Check
    // `ResponseTemplate` exposes no accessors, so the derived `Debug` view is
    // the only way to assert on the constructed template without a runtime.
    let debug = format!("{template:?}");
    assert_eq!(
        debug,
        "ResponseTemplate { mime: \"\", status_code: 200, headers: {\"content-type\": \"text/event-stream\"}, body: Some([100, 97, 116, 97, 58, 32, 91, 68, 79, 78, 69, 93, 10, 10]), delay: None }"
    );
    Ok(())
}

// endregion: --- sse_response_template

// region:    --- Test Support

/// The terminal `stop` frame of a text completion body.
const STOP_FRAME: &str = "data: {\"id\":\"chatcmpl-test\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":5,\"total_tokens\":15}}";

/// The terminal `tool_calls` frame of a tool-call body.
const TOOL_CALLS_FRAME: &str = "data: {\"id\":\"chatcmpl-test\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}],\"usage\":{\"prompt_tokens\":50,\"completion_tokens\":15,\"total_tokens\":65}}";

/// The stream terminator frame.
const DONE_FRAME: &str = "data: [DONE]";

/// Split an SSE body into its frames, dropping the blank separators.
fn frames(body: &str) -> Vec<&str> {
    body.trim_end_matches("\n\n").split("\n\n").collect()
}

/// The `content` delta frame carrying `chunk`.
fn content_frame(chunk: &str) -> String {
    format!(
        "data: {{\"id\":\"chatcmpl-test\",\"choices\":[{{\"index\":0,\"delta\":{{\"content\":{}}},\"finish_reason\":null}}]}}",
        serde_json::Value::String(chunk.to_string())
    )
}

// endregion: --- Test Support
