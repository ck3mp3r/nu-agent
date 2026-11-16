use super::*;

// region:    --- Projection

/// A user snapshot submits its text, and the turn it opens is closed by the
/// at-rest marker so the session folds back to idle.
#[test]
fn test_state_hydration_projection_maps_user_prompt() {
    // -- Setup & Fixtures
    let messages = vec![UiMessageSnapshot::new("user", "hi")];

    // -- Exec
    let log = projected_records(messages);

    // -- Check
    assert_eq!(log.len(), 2, "the prompt plus the turn boundary");
    assert!(matches!(
        log.first(),
        Some(SessionRecord::Prompt(record)) if record.text == "hi"
    ));
    assert!(matches!(log.get(1), Some(SessionRecord::Turn(_))));
}

/// An assistant snapshot is one non-streamed chunk: both chunk fields carry
/// the whole text, and the completion carries the persisted usage.
#[test]
fn test_state_hydration_projection_maps_assistant_chunk_and_usage() {
    // -- Setup & Fixtures
    let messages = vec![assistant("hello", Some(usage(3, 2, 5)))];

    // -- Exec
    let log = projected_records(messages);

    // -- Check
    assert!(matches!(
        log.first(),
        Some(SessionRecord::Inference(InferenceRecord::Started { .. }))
    ));
    let Some(SessionRecord::Inference(InferenceRecord::Chunk {
        delta, aggregated, ..
    })) = log.get(1)
    else {
        panic!("expected the chunk");
    };
    assert_eq!(delta, "hello", "delta carries the whole text");
    assert_eq!(aggregated, "hello", "aggregated carries the whole text");
    let Some(SessionRecord::Inference(InferenceRecord::Completed { usage })) = log.get(2) else {
        panic!("expected the completion");
    };
    assert_eq!(usage.input_tokens, 3);
    assert_eq!(usage.output_tokens, 2);
    assert_eq!(usage.total_tokens, Some(5));
}

/// A tool snapshot becomes the started call and its settled outcome; a later
/// call's data never lands on the earlier call.
#[test]
fn test_state_hydration_projection_maps_tool_row_to_its_records() {
    // -- Setup & Fixtures
    let result = r#"{"path":"a.rs","diff":"--- a\\n+++ b\\n"}"#;
    let messages = vec![
        tool("edit", r#"{"path":"a.rs"}"#, result, Some(true)),
        tool("read_file", r#"{"path":"b.txt"}"#, "b", Some(true)),
    ];

    // -- Exec
    let log = projected_records(messages);

    // -- Check: a start and a completion per call, then the turn boundary.
    let Some(SessionRecord::Tool(ToolRecord::Started { call: first_start })) = log.first() else {
        panic!("expected the first start");
    };
    let Some(SessionRecord::Tool(ToolRecord::Completed {
        call: first_end,
        outcome,
    })) = log.get(1)
    else {
        panic!("expected the first completion");
    };
    assert_eq!(first_start.id(), first_end.id(), "the pair names one call");
    assert_eq!(
        outcome.text, result,
        "the completion carries the persisted result text"
    );
    let Some(SessionRecord::Tool(ToolRecord::Started { call: second_start })) = log.get(2) else {
        panic!("expected the second start");
    };
    let Some(SessionRecord::Tool(ToolRecord::Completed { outcome, .. })) = log.get(3) else {
        panic!("expected the second completion");
    };
    assert_eq!(second_start.name.0, "read_file");
    assert_eq!(outcome.text, "b", "the second row's own result");
    assert_eq!(log.len(), 5, "two pairs, and the turn boundary");
}

/// Every persisted tool row completes with the status the store recorded.
#[test]
fn test_state_hydration_projection_routes_every_tool_row_to_a_completed_outcome() -> Result<()> {
    // -- Setup & Fixtures: one persisted row per outcome.
    let cases: [(&str, ToolStatus); 3] = [
        ("cancelled", ToolStatus::Cancelled),
        ("failed", ToolStatus::Failed),
        ("success", ToolStatus::Success),
    ];

    for (label, status) in cases {
        let success = Some(matches!(status, ToolStatus::Success));
        let snapshot = tool("nu", r#"{"command":"ls | select name"}"#, "out", success)
            .with_tool_status(status);
        let messages = vec![snapshot];

        // -- Exec
        let log = projected_records(messages);

        // -- Check: start, completion, turn boundary.
        assert_eq!(log.len(), 3, "{label}: one pair plus the boundary");
        let Some(SessionRecord::Tool(ToolRecord::Started { call: started })) = log.first() else {
            return Err("expected the tool start".into());
        };
        let Some(SessionRecord::Tool(ToolRecord::Completed {
            call: ended,
            outcome,
        })) = log.get(1)
        else {
            return Err("expected the completion".into());
        };
        assert_eq!(
            ended.id(),
            started.id(),
            "{label}: the completion names the call"
        );
        assert_eq!(
            outcome.status, status,
            "{label}: the outcome carries the status the row records"
        );
    }
    Ok(())
}

/// A role outside the resolver's closed set is a persisted system line: it
/// replays as a system notice, the row the pre-replay hydration pushed. Empty
/// lines carry no fact and are dropped.
#[test]
fn test_state_hydration_projection_maps_system_lines_to_notices() {
    // -- Setup & Fixtures
    let messages = vec![
        UiMessageSnapshot::new("mystery", "system fallback"),
        UiMessageSnapshot::new("system", "  a system prompt  "),
        UiMessageSnapshot::new("system", "   "),
    ];

    // -- Exec
    let log = projected_records(messages);

    // -- Check
    let notices: Vec<&str> = log
        .iter()
        .filter_map(|record| match record {
            SessionRecord::Notice(notice) => Some(notice.text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        notices,
        vec!["system fallback", "a system prompt"],
        "two non-empty system lines, one trimmed notice each"
    );
    assert!(projected_records(Vec::<UiMessageSnapshot>::new()).is_empty());
}

// endregion: --- Projection
