//! Tool-call translation: `ToolCompleted` status, message fallback, and diff display.

use agent_client_protocol::schema::v1::{
    ContentBlock, SessionUpdate, ToolCallContent, ToolCallStatus,
};
use agnt_kernel::{SessionRecord, ToolCall, ToolOutcome, ToolRecord};
use agnt_render::transcript::ir::ToolName;
use agnt_runtime::session::tool_status::ToolStatus;

use super::support::*;
use crate::translator::TranslateInput;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// ---------------------------------------------------------------------------
// ToolCompleted
// ---------------------------------------------------------------------------

#[test]
fn test_translator_tool_completed_success_produces_completed_update() -> Result<()> {
    // -- Setup & Fixtures
    let event = tool_completed("read", "{\"path\":\"src/lib.rs\"}", true);

    // -- Exec
    let update = translator()
        .translate(TranslateInput::Record(&event))
        .ok_or("tool completed must translate")?;

    // -- Check
    let SessionUpdate::ToolCallUpdate(update) = update else {
        return Err(format!("expected tool_call_update, got: {update:?}").into());
    };
    assert_eq!(update.fields.status, Some(ToolCallStatus::Completed));
    Ok(())
}

#[test]
fn test_translator_tool_completed_failure_produces_failed_update() -> Result<()> {
    // -- Setup & Fixtures
    let event = tool_completed("read", "{\"path\":\"src/lib.rs\"}", false);

    // -- Exec
    let update = translator()
        .translate(TranslateInput::Record(&event))
        .ok_or("tool completed must translate")?;

    // -- Check
    let SessionUpdate::ToolCallUpdate(update) = update else {
        return Err(format!("expected tool_call_update, got: {update:?}").into());
    };
    assert_eq!(update.fields.status, Some(ToolCallStatus::Failed));
    Ok(())
}

#[test]
fn test_translator_tool_completed_message_becomes_content_when_no_display() -> Result<()> {
    // -- Setup & Fixtures
    let event = SessionRecord::Tool(ToolRecord::Completed {
        call: ToolCall::new(
            ToolName("edit".to_string()),
            agnt_runtime::tools::handler::ToolSource::Builtin,
            "{}",
        ),
        outcome: ToolOutcome {
            status: ToolStatus::Cancelled,
            payload: None,
            text: String::new(),
            message: Some("Permission denied by user".to_string()),
            kind: None,
        },
    });

    // -- Exec
    let update = translator()
        .translate(TranslateInput::Record(&event))
        .ok_or("tool completed must translate")?;

    // -- Check
    let SessionUpdate::ToolCallUpdate(update) = update else {
        return Err(format!("expected tool_call_update, got: {update:?}").into());
    };
    let content = update.fields.content.ok_or("message must become content")?;
    assert_eq!(content.len(), 1);
    let ToolCallContent::Content(block) = &content[0] else {
        return Err(format!("expected content block, got: {:?}", content[0]).into());
    };
    let ContentBlock::Text(text) = &block.content else {
        return Err(format!("expected text block, got: {:?}", block.content).into());
    };
    assert_eq!(text.text, "Permission denied by user");
    Ok(())
}

#[test]
fn test_translator_tool_completed_diff_display_becomes_diff_content() -> Result<()> {
    // -- Setup & Fixtures
    let diff =
        "--- a/src/file.txt\n+++ b/src/file.txt\n@@ -1,3 +1,3 @@\n alpha\n-beta\n+omega\n gamma\n";
    let event = SessionRecord::Tool(ToolRecord::Completed {
        call: ToolCall::new(
            ToolName("edit".to_string()),
            agnt_runtime::tools::handler::ToolSource::Builtin,
            "{}",
        ),
        outcome: ToolOutcome {
            status: ToolStatus::Success,
            payload: None,
            text: format!(
                "{{\"path\":\"src/file.txt\",\"diff\":\"{}\"}}",
                diff.replace('\n', "\\n")
            ),
            message: None,
            kind: None,
        },
    });

    // -- Exec
    let update = translator()
        .translate(TranslateInput::Record(&event))
        .ok_or("tool completed must translate")?;

    // -- Check
    let SessionUpdate::ToolCallUpdate(update) = update else {
        return Err(format!("expected tool_call_update, got: {update:?}").into());
    };
    let content = update.fields.content.ok_or("display must become content")?;
    assert_eq!(content.len(), 1);
    let ToolCallContent::Diff(diff) = &content[0] else {
        return Err(format!("expected diff content, got: {:?}", content[0]).into());
    };
    assert_eq!(diff.path.to_string_lossy(), "src/file.txt");
    assert_eq!(diff.old_text.as_deref(), Some("alpha\nbeta\ngamma\n"));
    assert_eq!(diff.new_text, "alpha\nomega\ngamma\n");
    Ok(())
}

#[test]
fn test_translator_tool_completed_diff_without_old_side_has_no_old_text() -> Result<()> {
    // -- Setup & Fixtures
    let diff = "--- a/src/new.txt\n+++ b/src/new.txt\n@@ -0,0 +1,1 @@\n+first line\n";
    let event = SessionRecord::Tool(ToolRecord::Completed {
        call: ToolCall::new(
            ToolName("edit".to_string()),
            agnt_runtime::tools::handler::ToolSource::Builtin,
            "{}",
        ),
        outcome: ToolOutcome {
            status: ToolStatus::Success,
            payload: None,
            text: format!(
                "{{\"path\":\"src/new.txt\",\"diff\":\"{}\"}}",
                diff.replace('\n', "\\n")
            ),
            message: None,
            kind: None,
        },
    });

    // -- Exec
    let update = translator()
        .translate(TranslateInput::Record(&event))
        .ok_or("tool completed must translate")?;

    // -- Check
    let SessionUpdate::ToolCallUpdate(update) = update else {
        return Err(format!("expected tool_call_update, got: {update:?}").into());
    };
    let content = update.fields.content.ok_or("display must become content")?;
    let ToolCallContent::Diff(diff) = &content[0] else {
        return Err(format!("expected diff content, got: {:?}", content[0]).into());
    };
    assert_eq!(diff.old_text, None);
    assert_eq!(diff.new_text, "first line\n");
    Ok(())
}
