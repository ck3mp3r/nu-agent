use super::*;

/// An assistant message is appended to the transcript before the turn
/// completes; turn completion then unlocks input.
#[test]
fn assistant_message_is_appended_to_transcript_before_completed_unlock() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState {
        input: InputState::default().with_pending_submit_text("ping".to_string()),
        ..Default::default()
    };
    reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::Submit), None);
    let _ = state.activate_next_prompt();
    assert!(!state.input_locked);

    // -- Exec
    reduce_llm(&mut state, assistant_message("pong"));

    // -- Check: [User, Assistant] — content blocks only, no separators.
    assert!(!state.input_locked);
    assert_eq!(
        state
            .transcript
            .blocks()
            .iter()
            .map(|b| b.source.plain_text())
            .collect::<Vec<_>>(),
        vec!["ping", "pong"]
    );
    assert!(matches!(
        state.transcript.blocks()[0].source,
        BlockSource::Markdown {
            role: agnt_core::transcript::ir::MessageRole::User,
            ..
        }
    ));
    assert!(is_assistant_block(&state.transcript.blocks()[1]));

    // -- Exec: turn completion unlocks via the turn domain.
    use agnt_core::bus::TurnEvent;
    state.turn.reduce_turn_event(
        &mut state.transcript,
        &mut state.status,
        &mut state.input_locked,
        TurnEvent::Completed {
            tool_calls: 0,
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: None,
            cost: None,
            cost_complete: true,
            duration_ms: 0,
        },
    );
    state.finalize_cycle();

    // -- Check
    assert_eq!(state.phase, UiPhase::Idle);
    assert!(!state.input_locked);
    Ok(())
}

/// Assistant streaming appends one block, trims whitespace, ignores
/// whitespace-only text, replaces rather than appends on each delta, and
/// resets the stream cursor on a new LLM start.
#[test]
fn assistant_streaming_append_and_replace() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = busy_state_with_clean_transcript();

    // -- Exec & Check: whitespace-only text is a no-op.
    reduce_llm(&mut state, assistant_message(" \n\t\n"));
    assert!(state.transcript.blocks().is_empty());

    // -- Exec & Check: text is trimmed and appended as one block.
    reduce_llm(&mut state, assistant_message("\nline 1\nline 2\n"));
    let assistant_entries: Vec<_> = state
        .transcript
        .blocks()
        .iter()
        .filter(|b| is_assistant_block(b))
        .map(|b| b.source.plain_text())
        .collect();
    assert_eq!(assistant_entries.len(), 1, "one assistant block");
    assert!(assistant_entries[0].contains("line 1"));
    assert!(assistant_entries[0].contains("line 2"));

    // -- Exec & Check: a new delta replaces the block, not appends.
    let mut stream = busy_state_with_clean_transcript();
    reduce_llm(&mut stream, assistant_message("hello"));
    assert!(stream.transcript.assistant_stream_start.is_some());
    let first_start = stream
        .transcript
        .assistant_stream_start
        .ok_or("should have assistant_stream_start")?;
    reduce_llm(&mut stream, assistant_message("hello world"));
    assert_eq!(stream.transcript.assistant_stream_start, Some(first_start));
    let assistant_entries: Vec<_> = stream
        .transcript
        .blocks()
        .iter()
        .filter(|b| is_assistant_block(b))
        .collect();
    assert_eq!(assistant_entries.len(), 1);
    assert_eq!(assistant_entries[0].source.plain_text(), "hello world");
    assert!(
        !stream
            .transcript
            .blocks()
            .iter()
            .any(|b| b.source.plain_text() == "hello")
    );

    // -- Exec & Check: LlmStarted resets the stream cursor.
    reduce_llm(&mut stream, LlmEvent::Started);
    assert!(stream.transcript.assistant_stream_start.is_none());

    // -- Exec & Check: a fresh state pushes one assistant block, no spacer.
    let mut fresh = AppState::default();
    reduce_llm(&mut fresh, assistant_message("hello"));
    assert!(is_assistant_block(&fresh.transcript.blocks()[0]));
    Ok(())
}

/// A dry-run diff regurgitated by the assistant is still processed and
/// projected through markdown, whether or not a direct tool display exists.
#[test]
fn assistant_dry_run_diff_regurgitation_is_processed() -> Result<()> {
    // -- Setup & Fixtures: direct tool display present.
    let mut state = AppState::default();
    let mut evicted = 0usize;
    state.tool.reduce_tool_event(
        &mut state.transcript,
        agnt_core::bus::ToolEvent::Started {
            name: "edit".to_string(),
            source: "closure".to_string(),
            arguments: r#"{"path":"sample.txt"}"#.to_string(),
            call_line: CallLine::from_json_summary(r#"{"path":"sample.txt"}"#),
        },
        &mut evicted,
    );
    state.tool.reduce_tool_event(
        &mut state.transcript,
        agnt_core::bus::ToolEvent::Completed {
            name: "edit".to_string(),
            source: "closure".to_string(),
            arguments: r#"{"path":"sample.txt"}"#.to_string(),
            success: true,
            result: "{}".to_string(),
            display: Some(ToolDisplay {
                title: Some("edit sample.txt".to_string()),
                sections: vec![ToolDisplaySection {
                    label: Some("sample.txt".to_string()),
                    kind: agnt_core::transcript::ir::ContentKind::Diff {
                        language: "diff".to_string(),
                        path: "test.rs".to_string(),
                    },
                    content: "--- a/sample.txt\n+++ b/sample.txt\n@@ -1 +1 @@\n-old\n+new\n"
                        .to_string(),
                    stats: None,
                }],
            }),
            error_kind: None,
            message: None,
        },
        &mut evicted,
    );
    let before = state.transcript.len();

    // -- Exec
    reduce_llm(
        &mut state,
        assistant_message(
            "Dry-run diff:\n```diff\n--- a/sample.txt\n+++ b/sample.txt\n@@ -1 +1 @@\n-old\n+new\n```",
        ),
    );

    // -- Check: the assistant message is processed and projected.
    assert!(state.transcript.len() > before);
    assert!(state.transcript.blocks().iter().any(is_assistant_block));

    // -- Setup & Fixtures: no direct display present.
    let mut plain = AppState::default();

    // -- Exec
    reduce_llm(
        &mut plain,
        assistant_message(
            "Dry-run diff:\n```diff\n--- a/sample.txt\n+++ b/sample.txt\n@@ -1 +1 @@\n-old\n+new\n```",
        ),
    );

    // -- Check
    assert!(!plain.transcript.blocks().is_empty());
    assert!(plain.transcript.blocks().iter().any(is_assistant_block));
    Ok(())
}
