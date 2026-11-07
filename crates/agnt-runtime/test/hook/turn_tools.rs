//! `TurnToolLog` — the per-turn accumulator of dispatched tool calls and their
//! settled results.

use super::*;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// The call id a provider-issued id yields.
fn call_id(id: &str) -> CallId {
    CallId::from_wire(id)
}

/// A tool name, or the test's own failure when the name is empty.
fn tool_name(name: &str) -> Result<ToolName> {
    Ok(ToolName::new(name)?)
}

/// A call as the stream's `ToolCall` projection carries it: the id and name
/// the model emitted, with empty arguments.
fn streamed_call(id: &str, name: &str) -> Result<ToolCall> {
    Ok(ToolCall::from_wire(
        id,
        crate::types::ToolFunction::new(tool_name(name)?, serde_json::json!({})),
    ))
}

#[test]
fn test_turn_tools_records_empty_by_default() -> Result<()> {
    // -- Setup & Fixtures
    let log = TurnToolLog::default();

    // -- Exec
    let records = log.records();

    // -- Check
    assert!(records.is_empty(), "a fresh log must hold no records");
    Ok(())
}

#[test]
fn test_turn_tools_record_call_appends_in_dispatch_order() -> Result<()> {
    // -- Setup & Fixtures
    let log = TurnToolLog::default();

    // -- Exec
    log.record_call(call_id("call_1"), tool_name("read")?, r#"{"path":"/a"}"#);
    log.record_call(call_id("call_2"), tool_name("write")?, r#"{"path":"/b"}"#);
    let records = log.records();

    // -- Check
    assert_eq!(records.len(), 2, "each dispatch must add one record");
    assert_eq!(records[0].call_id, call_id("call_1"));
    assert_eq!(records[0].name.as_str(), "read");
    assert_eq!(records[0].arguments, r#"{"path":"/a"}"#);
    assert_eq!(records[1].call_id, call_id("call_2"));
    assert_eq!(records[1].name.as_str(), "write");
    assert_eq!(records[1].arguments, r#"{"path":"/b"}"#);
    assert_eq!(
        (
            records[0].result.as_deref(),
            records[0].success,
            records[0].error_kind.as_deref()
        ),
        (None, None, None),
        "a dispatched call carries no result until its outcome arrives"
    );
    Ok(())
}

#[test]
fn test_turn_tools_record_result_fills_the_matching_record() -> Result<()> {
    // -- Setup & Fixtures
    let log = TurnToolLog::default();
    log.record_call(call_id("call_1"), tool_name("read")?, "{}");
    log.record_call(call_id("call_2"), tool_name("read")?, "{}");

    // -- Exec
    log.record_result(
        call_id("call_2"),
        tool_name("read")?,
        "{}",
        SettledToolCall::new("contents", true, ToolStatus::Success),
    );
    let records = log.records();

    // -- Check
    assert_eq!(
        records.len(),
        2,
        "a result must land on its call's record, not add one"
    );
    assert_eq!(
        records[0].result, None,
        "the other call must stay unsettled"
    );
    assert_eq!(records[1].result.as_deref(), Some("contents"));
    assert_eq!(records[1].success, Some(true));
    assert_eq!(records[1].error_kind, None);
    assert_eq!(
        records[1].status,
        Some(ToolStatus::Success),
        "the settled status rides the record as its variant"
    );
    Ok(())
}

#[test]
fn test_turn_tools_record_result_appends_when_no_call_recorded() -> Result<()> {
    // -- Setup & Fixtures
    let log = TurnToolLog::default();

    // -- Exec
    log.record_result(
        call_id("call_9"),
        tool_name("nu")?,
        r#"{"cmd":"exit 1"}"#,
        SettledToolCall::new("boom", false, ToolStatus::Failed)
            .with_error_kind(Some("other".to_string())),
    );
    let records = log.records();

    // -- Check
    assert_eq!(
        records.len(),
        1,
        "a result with no dispatch must still be recorded"
    );
    assert_eq!(records[0].call_id, call_id("call_9"));
    assert_eq!(records[0].name.as_str(), "nu");
    assert_eq!(records[0].arguments, r#"{"cmd":"exit 1"}"#);
    assert_eq!(records[0].result.as_deref(), Some("boom"));
    assert_eq!(records[0].success, Some(false));
    assert_eq!(records[0].error_kind.as_deref(), Some("other"));
    Ok(())
}

#[test]
fn test_turn_tools_record_call_twice_keeps_one_record() -> Result<()> {
    // -- Setup & Fixtures
    let log = TurnToolLog::default();

    // -- Exec
    log.record_call(call_id("call_1"), tool_name("read")?, "{}");
    log.record_call(call_id("call_1"), tool_name("read")?, r#"{"path":"/a"}"#);
    let records = log.records();

    // -- Check
    assert_eq!(records.len(), 1, "one call id is one record");
    assert_eq!(
        records[0].arguments, "{}",
        "the first dispatch's arguments stand"
    );
    Ok(())
}

#[test]
fn test_turn_tools_records_returns_a_snapshot() -> Result<()> {
    // -- Setup & Fixtures
    let log = TurnToolLog::default();
    log.record_call(call_id("call_1"), tool_name("read")?, "{}");

    // -- Exec
    let snapshot = log.records();
    log.record_call(call_id("call_2"), tool_name("read")?, "{}");

    // -- Check
    assert_eq!(
        snapshot.len(),
        1,
        "a read must not see dispatches that land after it"
    );
    assert_eq!(log.records().len(), 2);
    Ok(())
}

#[test]
fn test_turn_tools_undispatched_streamed_calls_keeps_the_unrecorded_in_order() -> Result<()> {
    // -- Setup & Fixtures
    let log = TurnToolLog::default();
    log.record_call(call_id("call_2"), tool_name("read")?, "{}");
    let streamed = [
        streamed_call("call_1", "read")?,
        streamed_call("call_2", "read")?,
        streamed_call("call_3", "read")?,
    ];

    // -- Exec
    let undispatched = undispatched_streamed_calls(&streamed, &log.records());

    // -- Check
    assert_eq!(
        undispatched
            .iter()
            .map(|call| call.id.clone())
            .collect::<Vec<_>>(),
        vec![call_id("call_1"), call_id("call_3")],
        "only the calls with no record survive, in streamed order"
    );
    Ok(())
}

#[test]
fn test_turn_tools_undispatched_streamed_calls_empty_when_all_dispatched() -> Result<()> {
    // -- Setup & Fixtures
    let log = TurnToolLog::default();
    log.record_call(call_id("call_1"), tool_name("read")?, "{}");
    let streamed = [streamed_call("call_1", "read")?];

    // -- Exec
    let undispatched = undispatched_streamed_calls(&streamed, &log.records());

    // -- Check
    assert!(
        undispatched.is_empty(),
        "a call the turn dispatched is not undispatched; got {undispatched:?}"
    );
    Ok(())
}
