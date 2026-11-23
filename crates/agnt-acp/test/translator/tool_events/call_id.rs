//! Tool-call translation: call-id correlation and orphan completions.

use agent_client_protocol::schema::v1::SessionUpdate;

use super::support::*;
use crate::translator::TranslateInput;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// ---------------------------------------------------------------------------
// ToolCompleted
// ---------------------------------------------------------------------------

#[test]
fn test_translator_tool_call_id_matches_between_started_and_completed() -> Result<()> {
    // -- Setup & Fixtures
    let arguments = "{\"path\":\"src/lib.rs\"}";
    let mut translator = translator();

    // -- Exec
    let started = translator
        .translate(TranslateInput::Record(&tool_started("read", arguments)))
        .ok_or("tool started must translate")?;
    let completed = translator
        .translate(TranslateInput::Record(&tool_completed(
            "read", arguments, true,
        )))
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
        .translate(TranslateInput::Record(&tool_started(
            "read",
            "{\"path\":\"a.rs\"}",
        )))
        .ok_or("tool started must translate")?;
    let second = translator
        .translate(TranslateInput::Record(&tool_started(
            "read",
            "{\"path\":\"b.rs\"}",
        )))
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
        .translate(TranslateInput::Record(&tool_started("read", arguments)))
        .ok_or("tool started must translate")?;
    let second = translator
        .translate(TranslateInput::Record(&tool_started("read", arguments)))
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
        .translate(TranslateInput::Record(&tool_started("read", arguments)))
        .ok_or("tool started must translate")?;
    let second_start = translator
        .translate(TranslateInput::Record(&tool_started("read", arguments)))
        .ok_or("tool started must translate")?;

    // -- Exec
    let first_done = translator
        .translate(TranslateInput::Record(&tool_completed(
            "read", arguments, true,
        )))
        .ok_or("tool completed must translate")?;
    let second_done = translator
        .translate(TranslateInput::Record(&tool_completed(
            "read", arguments, true,
        )))
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
        .translate(TranslateInput::Record(&tool_completed(
            "read",
            "{\"path\":\"src/lib.rs\"}",
            true,
        )))
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
        .translate(TranslateInput::Record(&tool_completed(
            "read", arguments, true,
        )))
        .ok_or("a completion without a start must still translate")?;

    // -- Exec
    let started = translator
        .translate(TranslateInput::Record(&tool_started("read", arguments)))
        .ok_or("tool started must translate")?;
    let completed = translator
        .translate(TranslateInput::Record(&tool_completed(
            "read", arguments, true,
        )))
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
