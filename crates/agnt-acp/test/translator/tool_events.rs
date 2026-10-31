//! Tool-call translation: `ToolStarted` and `ToolCompleted`.

use agent_client_protocol::schema::v1::{
    ContentBlock, SessionUpdate, ToolCallContent, ToolCallStatus, ToolKind,
};
use agnt_core::protocol::event::ToolDisplay;
use agnt_core::protocol::events::{SystemEvent, ToolCall, ToolOutcome};
use agnt_core::session::tool_status::ToolStatus;
use agnt_core::transcript::ir::ToolName;

use super::support::*;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// ---------------------------------------------------------------------------
// ToolStarted
// ---------------------------------------------------------------------------

#[test]
fn test_translator_tool_started_produces_pending_tool_call() -> Result<()> {
    // -- Setup & Fixtures
    let event = tool_started("read", "{\"path\":\"src/lib.rs\"}");

    // -- Exec
    let update = translator()
        .translate(event)
        .ok_or("tool started must translate")?;

    // -- Check
    let SessionUpdate::ToolCall(call) = update else {
        return Err(format!("expected tool_call, got: {update:?}").into());
    };
    assert_eq!(call.status, ToolCallStatus::Pending);
    assert_eq!(call.kind, ToolKind::Read);
    assert_eq!(call.name.as_deref(), Some("read"));
    assert_eq!(call.title, "read src/lib.rs");
    assert!(!call.tool_call_id.0.is_empty());
    Ok(())
}

#[test]
fn test_translator_tool_started_falls_back_to_name_when_summary_empty() -> Result<()> {
    // -- Setup & Fixtures
    let event = SystemEvent::ToolStarted {
        call: ToolCall::new(ToolName("glob".to_string()), "builtin", "{}", call_line("")),
    };

    // -- Exec
    let update = translator()
        .translate(event)
        .ok_or("tool started must translate")?;

    // -- Check
    let SessionUpdate::ToolCall(call) = update else {
        return Err(format!("expected tool_call, got: {update:?}").into());
    };
    assert_eq!(call.title, "glob");
    Ok(())
}

#[test]
fn test_translator_tool_kind_mapping_covers_every_family() -> Result<()> {
    // -- Setup & Fixtures
    let cases = [
        ("read", ToolKind::Read),
        ("edit", ToolKind::Edit),
        ("grep", ToolKind::Search),
        ("glob", ToolKind::Search),
        ("nu", ToolKind::Execute),
        ("http", ToolKind::Fetch),
        ("skill", ToolKind::Other),
        ("ast_query", ToolKind::Read),
        ("ast_nodes", ToolKind::Read),
        ("ast_refs", ToolKind::Read),
        ("ast_tree", ToolKind::Read),
        ("tmux_session", ToolKind::Execute),
        ("tmux_window", ToolKind::Execute),
        ("tmux_pane", ToolKind::Execute),
        ("tmux_layout", ToolKind::Execute),
        ("spawn_agent", ToolKind::Other),
    ];
    let mut translator = translator();

    // -- Exec & Check
    for (name, expected) in cases {
        let update = translator
            .translate(tool_started(name, "{}"))
            .ok_or_else(|| format!("tool '{name}' must translate"))?;
        let SessionUpdate::ToolCall(call) = update else {
            return Err(format!("expected tool_call for '{name}', got: {update:?}").into());
        };
        assert_eq!(call.kind, expected, "wrong kind for tool '{name}'");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// ToolCompleted
// ---------------------------------------------------------------------------

#[test]
fn test_translator_tool_completed_success_produces_completed_update() -> Result<()> {
    // -- Setup & Fixtures
    let event = tool_completed("read", "{\"path\":\"src/lib.rs\"}", true);

    // -- Exec
    let update = translator()
        .translate(event)
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
        .translate(event)
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
    let event = SystemEvent::ToolCompleted {
        call: ToolCall::new(
            ToolName("edit".to_string()),
            "builtin",
            "{}",
            call_line("→ edit"),
        ),
        outcome: ToolOutcome {
            success: false,
            status: ToolStatus::Cancelled,
            result: String::new(),
            display: None,
            error_kind: None,
            message: Some("Permission denied by user".to_string()),
        },
    };

    // -- Exec
    let update = translator()
        .translate(event)
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
fn test_translator_tool_completed_text_display_becomes_content() -> Result<()> {
    // -- Setup & Fixtures
    let event = SystemEvent::ToolCompleted {
        call: ToolCall::new(
            ToolName("grep".to_string()),
            "builtin",
            "{}",
            call_line("→ grep foo"),
        ),
        outcome: ToolOutcome {
            success: true,
            status: ToolStatus::Success,
            result: "{}".to_string(),
            display: Some(ToolDisplay {
                title: Some("grep foo".to_string()),
                sections: vec![text_section("matches", "src/lib.rs:1: foo")],
            }),
            error_kind: None,
            message: None,
        },
    };

    // -- Exec
    let update = translator()
        .translate(event)
        .ok_or("tool completed must translate")?;

    // -- Check
    let SessionUpdate::ToolCallUpdate(update) = update else {
        return Err(format!("expected tool_call_update, got: {update:?}").into());
    };
    let content = update.fields.content.ok_or("display must become content")?;
    assert_eq!(content.len(), 1);
    let ToolCallContent::Content(block) = &content[0] else {
        return Err(format!("expected content block, got: {:?}", content[0]).into());
    };
    let ContentBlock::Text(text) = &block.content else {
        return Err(format!("expected text block, got: {:?}", block.content).into());
    };
    assert_eq!(text.text, "src/lib.rs:1: foo");
    Ok(())
}

#[test]
fn test_translator_tool_completed_diff_display_becomes_diff_content() -> Result<()> {
    // -- Setup & Fixtures
    let diff =
        "--- a/src/file.txt\n+++ b/src/file.txt\n@@ -1,3 +1,3 @@\n alpha\n-beta\n+omega\n gamma\n";
    let event = SystemEvent::ToolCompleted {
        call: ToolCall::new(
            ToolName("edit".to_string()),
            "builtin",
            "{}",
            call_line("→ edit src/file.txt"),
        ),
        outcome: ToolOutcome {
            success: true,
            status: ToolStatus::Success,
            result: "{}".to_string(),
            display: Some(ToolDisplay {
                title: Some("edit src/file.txt".to_string()),
                sections: vec![diff_section("src/file.txt", diff)],
            }),
            error_kind: None,
            message: None,
        },
    };

    // -- Exec
    let update = translator()
        .translate(event)
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
    let event = SystemEvent::ToolCompleted {
        call: ToolCall::new(
            ToolName("edit".to_string()),
            "builtin",
            "{}",
            call_line("→ edit src/file.txt"),
        ),
        outcome: ToolOutcome {
            success: true,
            status: ToolStatus::Success,
            result: "{}".to_string(),
            display: Some(ToolDisplay {
                title: Some("edit src/new.txt".to_string()),
                sections: vec![diff_section("src/new.txt", diff)],
            }),
            error_kind: None,
            message: None,
        },
    };

    // -- Exec
    let update = translator()
        .translate(event)
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

#[test]
fn test_translator_tool_call_id_matches_between_started_and_completed() -> Result<()> {
    // -- Setup & Fixtures
    let arguments = "{\"path\":\"src/lib.rs\"}";
    let mut translator = translator();

    // -- Exec
    let started = translator
        .translate(tool_started("read", arguments))
        .ok_or("tool started must translate")?;
    let completed = translator
        .translate(tool_completed("read", arguments, true))
        .ok_or("tool completed must translate")?;

    // -- Check
    let SessionUpdate::ToolCall(call) = started else {
        return Err(format!("expected tool_call, got: {started:?}").into());
    };
    let SessionUpdate::ToolCallUpdate(update) = completed else {
        return Err(format!("expected tool_call_update, got: {completed:?}").into());
    };
    assert_eq!(call.tool_call_id, update.tool_call_id);
    Ok(())
}

#[test]
fn test_translator_tool_call_id_differs_for_different_arguments() -> Result<()> {
    // -- Setup & Fixtures
    let mut translator = translator();

    // -- Exec
    let first = translator
        .translate(tool_started("read", "{\"path\":\"a.rs\"}"))
        .ok_or("tool started must translate")?;
    let second = translator
        .translate(tool_started("read", "{\"path\":\"b.rs\"}"))
        .ok_or("tool started must translate")?;

    // -- Check
    let SessionUpdate::ToolCall(first) = first else {
        return Err(format!("expected tool_call, got: {first:?}").into());
    };
    let SessionUpdate::ToolCall(second) = second else {
        return Err(format!("expected tool_call, got: {second:?}").into());
    };
    assert_ne!(first.tool_call_id, second.tool_call_id);
    Ok(())
}

#[test]
fn test_translator_tool_call_id_unique_for_repeated_identical_calls() -> Result<()> {
    // -- Setup & Fixtures
    let arguments = "{\"path\":\"src/lib.rs\"}";
    let mut translator = translator();

    // -- Exec
    let first = translator
        .translate(tool_started("read", arguments))
        .ok_or("tool started must translate")?;
    let second = translator
        .translate(tool_started("read", arguments))
        .ok_or("tool started must translate")?;

    // -- Check
    let SessionUpdate::ToolCall(first) = first else {
        return Err(format!("expected tool_call, got: {first:?}").into());
    };
    let SessionUpdate::ToolCall(second) = second else {
        return Err(format!("expected tool_call, got: {second:?}").into());
    };
    assert_ne!(
        first.tool_call_id, second.tool_call_id,
        "ids must be unique within a session even for identical calls"
    );
    Ok(())
}

#[test]
fn test_translator_repeated_identical_calls_match_completions_in_order() -> Result<()> {
    // -- Setup & Fixtures
    let arguments = "{\"path\":\"src/lib.rs\"}";
    let mut translator = translator();
    let first_start = translator
        .translate(tool_started("read", arguments))
        .ok_or("tool started must translate")?;
    let second_start = translator
        .translate(tool_started("read", arguments))
        .ok_or("tool started must translate")?;

    // -- Exec
    let first_done = translator
        .translate(tool_completed("read", arguments, true))
        .ok_or("tool completed must translate")?;
    let second_done = translator
        .translate(tool_completed("read", arguments, true))
        .ok_or("tool completed must translate")?;

    // -- Check
    let SessionUpdate::ToolCall(first_start) = first_start else {
        return Err(format!("expected tool_call, got: {first_start:?}").into());
    };
    let SessionUpdate::ToolCall(second_start) = second_start else {
        return Err(format!("expected tool_call, got: {second_start:?}").into());
    };
    let SessionUpdate::ToolCallUpdate(first_done) = first_done else {
        return Err(format!("expected tool_call_update, got: {first_done:?}").into());
    };
    let SessionUpdate::ToolCallUpdate(second_done) = second_done else {
        return Err(format!("expected tool_call_update, got: {second_done:?}").into());
    };
    assert_eq!(first_start.tool_call_id, first_done.tool_call_id);
    assert_eq!(second_start.tool_call_id, second_done.tool_call_id);
    Ok(())
}

#[test]
fn test_translator_completion_without_start_still_produces_update() -> Result<()> {
    // -- Setup & Fixtures
    let mut translator = translator();

    // -- Exec
    let update = translator
        .translate(tool_completed("read", "{\"path\":\"src/lib.rs\"}", true))
        .ok_or("a completion without a start must still translate")?;

    // -- Check
    let SessionUpdate::ToolCallUpdate(update) = update else {
        return Err(format!("expected tool_call_update, got: {update:?}").into());
    };
    assert!(!update.tool_call_id.0.is_empty());
    Ok(())
}

#[test]
fn test_translator_completion_without_start_does_not_consume_later_start_id() -> Result<()> {
    // -- Setup & Fixtures
    let arguments = "{\"path\":\"src/lib.rs\"}";
    let mut translator = translator();
    let orphan = translator
        .translate(tool_completed("read", arguments, true))
        .ok_or("a completion without a start must still translate")?;

    // -- Exec
    let started = translator
        .translate(tool_started("read", arguments))
        .ok_or("tool started must translate")?;
    let completed = translator
        .translate(tool_completed("read", arguments, true))
        .ok_or("tool completed must translate")?;

    // -- Check
    let SessionUpdate::ToolCallUpdate(orphan) = orphan else {
        return Err(format!("expected tool_call_update, got: {orphan:?}").into());
    };
    let SessionUpdate::ToolCall(started) = started else {
        return Err(format!("expected tool_call, got: {started:?}").into());
    };
    let SessionUpdate::ToolCallUpdate(completed) = completed else {
        return Err(format!("expected tool_call_update, got: {completed:?}").into());
    };
    assert_ne!(
        orphan.tool_call_id, started.tool_call_id,
        "the orphan completion must not consume the later call's id"
    );
    assert_eq!(
        started.tool_call_id, completed.tool_call_id,
        "the later call must still match its own completion"
    );
    Ok(())
}
