//! `close_unanswered_tool_calls_with` tests: every call a turn never answered
//! is closed, preferring the result the turn observed.

use super::*;

use rig::conversation::pairing::NO_RESULT_PROVIDED;

/// Every unanswered call gets one synthetic result, in call order, in a single
/// user message right after its assistant message.
#[test]
fn close_unanswered_tool_calls_with_closes_each_unanswered_call() -> Result<()> {
    // -- Setup & Fixtures
    let messages = vec![Message::user("q"), assistant_tool_calls(&["tc1", "tc2"])?];

    // -- Exec
    let closed = crate::session::repair::close_unanswered_tool_calls_with(messages, &[]);

    // -- Check
    assert_eq!(
        closed.len(),
        3,
        "one close message for the assistant turn; got {closed:?}"
    );
    let Message::User { content } = &closed[2] else {
        return Err("the close must be a user message".into());
    };
    assert_eq!(
        content.len(),
        2,
        "one result per unanswered call; got {closed:?}"
    );
    let calls: Vec<&CallId> = content
        .iter()
        .filter_map(|item| match item {
            UserContent::ToolResult(result) => Some(&result.call),
            _ => None,
        })
        .collect();
    assert_eq!(
        calls,
        vec![&CallId::from_wire("tc1"), &CallId::from_wire("tc2")],
        "the results follow the call order; got {closed:?}"
    );
    Ok(())
}

/// A call a result already answers is left alone: no synthetic close is added.
#[test]
fn close_unanswered_tool_calls_with_leaves_answered_calls_alone() -> Result<()> {
    // -- Setup & Fixtures
    let messages = vec![
        assistant_tool_calls(&["tc1"])?,
        Message::User {
            content: vec![UserContent::ToolResult(ToolResult {
                call: CallId::from_wire("tc1"),
                name: ToolName::new("read_file").map_err(|_| "non-empty tool name")?,
                content: vec![ToolResultContent::text("done")],
                is_error: false,
            })],
        },
    ];

    // -- Exec
    let closed = crate::session::repair::close_unanswered_tool_calls_with(messages.clone(), &[]);

    // -- Check
    assert_eq!(
        closed, messages,
        "an answered call needs no synthetic result; got {closed:?}"
    );
    Ok(())
}

/// A placeholder result the turn settled is replaced by the record's text.
#[test]
fn replace_placeholder_results_with_replaces_a_placeholder_with_the_settled_result() -> Result<()> {
    // -- Setup & Fixtures
    let messages = vec![
        assistant_tool_calls(&["tc1"])?,
        Message::User {
            content: vec![UserContent::ToolResult(ToolResult {
                call: CallId::from_wire("tc1"),
                name: ToolName::new("read_file").map_err(|_| "non-empty tool name")?,
                content: vec![ToolResultContent::text(NO_RESULT_PROVIDED)],
                is_error: true,
            })],
        },
    ];
    let results = vec![(CallId::from_wire("tc1"), "real output".to_string(), false)];

    // -- Exec
    let replaced = crate::session::repair::replace_placeholder_results_with(messages, &results);

    // -- Check
    assert_eq!(
        replaced.len(),
        2,
        "the message count is unchanged; got {replaced:?}"
    );
    let Message::User { content } = &replaced[1] else {
        return Err("the result must stay a user message".into());
    };
    let UserContent::ToolResult(result) = &content[0] else {
        return Err("the result must stay a tool result".into());
    };
    assert_eq!(
        result.content,
        vec![ToolResultContent::text("real output")],
        "the record's text replaces the placeholder; got {result:?}"
    );
    assert!(
        !result.is_error,
        "the record's error flag replaces the placeholder's; got {result:?}"
    );
    Ok(())
}

/// The replacement takes the record's error flag, not the placeholder's.
#[test]
fn replace_placeholder_results_with_sets_is_error_from_the_record() -> Result<()> {
    // -- Setup & Fixtures
    let messages = vec![
        assistant_tool_calls(&["tc1"])?,
        Message::User {
            content: vec![UserContent::ToolResult(ToolResult {
                call: CallId::from_wire("tc1"),
                name: ToolName::new("read_file").map_err(|_| "non-empty tool name")?,
                content: vec![ToolResultContent::text(NO_RESULT_PROVIDED)],
                is_error: true,
            })],
        },
    ];
    let results = vec![(CallId::from_wire("tc1"), "tool failed".to_string(), true)];

    // -- Exec
    let replaced = crate::session::repair::replace_placeholder_results_with(messages, &results);

    // -- Check
    let Message::User { content } = &replaced[1] else {
        return Err("the result must stay a user message".into());
    };
    let UserContent::ToolResult(result) = &content[0] else {
        return Err("the result must stay a tool result".into());
    };
    assert_eq!(
        result.content,
        vec![ToolResultContent::text("tool failed")],
        "the record's text replaces the placeholder; got {result:?}"
    );
    assert!(
        result.is_error,
        "the record's error flag replaces the placeholder's; got {result:?}"
    );
    Ok(())
}

/// A result that is not rig's placeholder is left alone, even when a record
/// matches its call id.
#[test]
fn replace_placeholder_results_with_leaves_a_non_placeholder_result_alone() -> Result<()> {
    // -- Setup & Fixtures
    let messages = vec![
        assistant_tool_calls(&["tc1"])?,
        Message::User {
            content: vec![UserContent::ToolResult(ToolResult {
                call: CallId::from_wire("tc1"),
                name: ToolName::new("read_file").map_err(|_| "non-empty tool name")?,
                content: vec![ToolResultContent::text("host wrote this")],
                is_error: false,
            })],
        },
    ];
    let results = vec![(CallId::from_wire("tc1"), "real output".to_string(), false)];

    // -- Exec
    let replaced =
        crate::session::repair::replace_placeholder_results_with(messages.clone(), &results);

    // -- Check
    assert_eq!(
        replaced, messages,
        "a non-placeholder result is untouched; got {replaced:?}"
    );
    Ok(())
}

/// A placeholder no record matches stays as rig wrote it.
#[test]
fn replace_placeholder_results_with_leaves_an_unmatched_placeholder_alone() -> Result<()> {
    // -- Setup & Fixtures
    let messages = vec![
        assistant_tool_calls(&["tc1"])?,
        Message::User {
            content: vec![UserContent::ToolResult(ToolResult {
                call: CallId::from_wire("tc1"),
                name: ToolName::new("read_file").map_err(|_| "non-empty tool name")?,
                content: vec![ToolResultContent::text(NO_RESULT_PROVIDED)],
                is_error: true,
            })],
        },
    ];

    // -- Exec
    let replaced = crate::session::repair::replace_placeholder_results_with(messages.clone(), &[]);

    // -- Check
    assert_eq!(
        replaced, messages,
        "an unmatched placeholder is untouched; got {replaced:?}"
    );
    Ok(())
}

/// The placeholder text alone does not mark a result: rig writes it as an
/// error, so a non-error result holding the same text is left alone.
#[test]
fn replace_placeholder_results_with_leaves_a_non_error_placeholder_text_alone() -> Result<()> {
    // -- Setup & Fixtures
    let messages = vec![
        assistant_tool_calls(&["tc1"])?,
        Message::User {
            content: vec![UserContent::ToolResult(ToolResult {
                call: CallId::from_wire("tc1"),
                name: ToolName::new("read_file").map_err(|_| "non-empty tool name")?,
                content: vec![ToolResultContent::text(NO_RESULT_PROVIDED)],
                is_error: false,
            })],
        },
    ];
    let results = vec![(CallId::from_wire("tc1"), "real output".to_string(), false)];

    // -- Exec
    let replaced =
        crate::session::repair::replace_placeholder_results_with(messages.clone(), &results);

    // -- Check
    assert_eq!(
        replaced, messages,
        "a non-error result holding the placeholder text is untouched; got {replaced:?}"
    );
    Ok(())
}
