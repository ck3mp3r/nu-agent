use super::*;

use rig::conversation::pairing::NO_RESULT_PROVIDED;

/// A non-empty delta is appended with its unanswered tool call closed by a
/// synthetic `[interrupted]` result.
#[tokio::test]
async fn persist_turn_delta_appends_the_closed_delta() -> Result<()> {
    // -- Setup & Fixtures
    let config = Config::default();
    let mut memory_state = MemoryState::new(RigStoreConfig::in_memory());
    let session_id = "test-recover-closed-delta";
    seed(
        &memory_state,
        session_id,
        vec![Message::user("q1"), Message::assistant("a1")],
    )?;
    let full_history = vec![
        Message::user("q1"),
        Message::assistant("a1"),
        Message::user("q2"),
        assistant_tool_calls(&["tc1"])?,
    ];
    let bus = crate::bus::create_bus();
    let mut ui_events = bus.ui_event().subscribe();

    // -- Exec
    executor_with_bus(&config, &mut memory_state, bus)
        .persist_turn_delta(
            Some(session_id),
            "q2",
            &full_history,
            2,
            INTERRUPTED_REPLY,
            recovery(&[], None),
        )
        .await;

    // -- Check
    let messages = stored(&memory_state, session_id)?;
    assert_eq!(
        messages.len(),
        5,
        "the prior conversation plus the closed delta; got {messages:?}"
    );
    assert!(
        warnings(&mut ui_events).is_empty(),
        "a store that did not move must not warn"
    );
    assert_eq!(messages[2], Message::user("q2"));
    assert!(
        matches!(&messages[3], Message::Assistant(_)),
        "the committed tool call is persisted; got {messages:?}"
    );
    let Message::User { content } = &messages[4] else {
        return Err("the unanswered call must be closed by a user message".into());
    };
    let UserContent::ToolResult(result) = &content[0] else {
        return Err("the close must be a tool result".into());
    };
    assert_eq!(result.call, CallId::from_wire("tc1"));
    assert!(
        matches!(&result.content[0], ToolResultContent::Text(text) if text.text == "[interrupted]"),
        "the synthetic result carries the interrupted marker; got {result:?}"
    );
    Ok(())
}

/// A call the delta does not carry is reconstructed from the turn's records:
/// one assistant message holding the calls, closed by their results.
#[tokio::test]
async fn persist_turn_delta_reconstructs_the_missing_tool_exchange() -> Result<()> {
    // -- Setup & Fixtures
    let config = Config::default();
    let mut memory_state = MemoryState::new(RigStoreConfig::in_memory());
    let session_id = "test-recover-records";
    let full_history = vec![Message::user("q2")];
    let records = vec![record("tc1", Some("file contents"))?, record("tc2", None)?];

    // -- Exec
    executor(&config, &mut memory_state)
        .persist_turn_delta(
            Some(session_id),
            "q2",
            &full_history,
            0,
            INTERRUPTED_REPLY,
            recovery(&records, None),
        )
        .await;

    // -- Check
    let messages = stored(&memory_state, session_id)?;
    assert_eq!(
        messages.len(),
        3,
        "the prompt and the reconstructed exchange; got {messages:?}"
    );
    assert_eq!(messages[0], Message::user("q2"));
    let Message::Assistant(assistant) = &messages[1] else {
        return Err("the reconstructed calls must be an assistant message".into());
    };
    let calls: Vec<&CallId> = assistant.tool_calls().map(|call| &call.id).collect();
    assert_eq!(
        calls,
        vec![&CallId::from_wire("tc1"), &CallId::from_wire("tc2")],
        "one call per record, in dispatch order; got {messages:?}"
    );
    assert_eq!(
        result_texts(&messages[2]),
        vec!["file contents".to_string(), "[interrupted]".to_string()],
        "a settled call keeps its result, an unsettled one is interrupted; got {messages:?}"
    );
    Ok(())
}

/// A record whose call the delta already carries is not duplicated.
#[tokio::test]
async fn persist_turn_delta_does_not_duplicate_a_recorded_call() -> Result<()> {
    // -- Setup & Fixtures
    let config = Config::default();
    let mut memory_state = MemoryState::new(RigStoreConfig::in_memory());
    let session_id = "test-recover-no-duplicate";
    let full_history = vec![Message::user("q2"), assistant_tool_calls(&["tc1"])?];
    let records = vec![record("tc1", Some("file contents"))?];

    // -- Exec
    executor(&config, &mut memory_state)
        .persist_turn_delta(
            Some(session_id),
            "q2",
            &full_history,
            0,
            INTERRUPTED_REPLY,
            recovery(&records, None),
        )
        .await;

    // -- Check
    let messages = stored(&memory_state, session_id)?;
    assert_eq!(
        messages.len(),
        3,
        "the committed call is not repeated; got {messages:?}"
    );
    assert_eq!(
        result_texts(&messages[2]),
        vec!["file contents".to_string()],
        "the committed call closes with its settled result; got {messages:?}"
    );
    Ok(())
}

/// A placeholder result the turn settled is replaced by the record's text; a
/// placeholder no record matches stays as rig wrote it.
#[tokio::test]
async fn persist_turn_delta_replaces_a_placeholder_result_with_the_settled_record() -> Result<()> {
    // -- Setup & Fixtures
    let config = Config::default();
    let mut memory_state = MemoryState::new(RigStoreConfig::in_memory());
    let session_id = "test-recover-placeholder-replace";
    let name = ToolName::new("read_file")?;
    let full_history = vec![
        Message::user("q2"),
        assistant_tool_calls(&["tc1", "tc2"])?,
        Message::User {
            content: vec![
                UserContent::ToolResult(ToolResult {
                    call: CallId::from_wire("tc1"),
                    name: name.clone(),
                    content: vec![ToolResultContent::text(NO_RESULT_PROVIDED)],
                    is_error: true,
                }),
                UserContent::ToolResult(ToolResult {
                    call: CallId::from_wire("tc2"),
                    name: name.clone(),
                    content: vec![ToolResultContent::text(NO_RESULT_PROVIDED)],
                    is_error: true,
                }),
            ],
        },
    ];
    let records = vec![record("tc1", Some("real output"))?, record("tc2", None)?];

    // -- Exec
    executor(&config, &mut memory_state)
        .persist_turn_delta(
            Some(session_id),
            "q2",
            &full_history,
            0,
            INTERRUPTED_REPLY,
            recovery(&records, None),
        )
        .await;

    // -- Check
    let messages = stored(&memory_state, session_id)?;
    assert_eq!(
        messages.len(),
        3,
        "the prompt and the committed exchange; got {messages:?}"
    );
    assert_eq!(
        result_texts(&messages[2]),
        vec!["real output".to_string(), NO_RESULT_PROVIDED.to_string()],
        "the settled call takes the record's text, the unsettled one keeps the placeholder; \
         got {messages:?}"
    );
    let Message::User { content } = &messages[2] else {
        return Err("the results must be a user message".into());
    };
    let results: Vec<&ToolResult> = content
        .iter()
        .filter_map(|item| match item {
            UserContent::ToolResult(result) => Some(result),
            _ => None,
        })
        .collect();
    assert!(
        !results[0].is_error,
        "the settled call takes the record's error flag; got {results:?}"
    );
    assert!(
        results[1].is_error,
        "the unmatched placeholder keeps rig's error flag; got {results:?}"
    );
    Ok(())
}
