use super::*;

// region:    --- Replay

/// The determinism invariant: replaying the persisted snapshots equals folding
/// the same event log live AND reproduces main's hydrated shape — entries,
/// status tokens, compaction bookkeeping, and every display in the preview slot
/// (`result` stays empty, exactly as the hand-builder left it).
#[test]
fn test_state_hydration_replay_matches_live_reduction() -> Result<()> {
    // -- Setup & Fixtures
    let messages = persisted_session();
    let live = live_reduction(&messages);
    let mut replayed = AppState::default();

    // -- Exec
    hydrate_by_replay(&mut replayed, messages, Some(14000));

    // -- Check: the same entries, in the same order. The one intended
    // difference is the compaction row's label: a replayed one reads
    // `history`, a live fold of the same record reads its own source.
    assert_eq!(
        replayed.transcript.entries().len(),
        live.transcript.entries().len(),
        "the replayed entries must match the live entries one for one"
    );
    for (replayed_entry, live_entry) in replayed
        .transcript
        .entries()
        .iter()
        .zip(live.transcript.entries())
    {
        if matches!(replayed_entry, TranscriptEntry::Task(_)) {
            continue;
        }
        assert_eq!(entry_text(replayed_entry), entry_text(live_entry));
    }
    assert_eq!(
        entry_roles(&replayed),
        vec![
            "user",
            "assistant",
            "tool",
            "user",
            "task",
            "assistant",
            "assistant"
        ],
        "one entry per persisted fact: the compaction call line plus its summary"
    );

    // -- Check: the completion body is derived from the persisted result data,
    // the same derivation the live path runs — an `edit` row shows its diff —
    // and the gate-preview slot stays empty, because the gate's diff was never
    // in the transcript.
    let tool = replayed
        .transcript
        .entries()
        .iter()
        .find_map(|entry| match entry {
            TranscriptEntry::Tool(tool) => Some(tool),
            _ => None,
        })
        .ok_or("should have the replayed tool entry")?;
    assert_eq!(tool.status, ItemStatus::Done, "the persisted verdict");
    assert!(tool.preview.is_none(), "the replay carries no gate content");
    let result = tool.result.as_ref().ok_or("should have the edit body")?;
    assert!(
        display_text(result).contains("+++ b"),
        "the persisted diff is the completion body"
    );

    // -- Check: the compaction bookkeeping is the live bookkeeping, except the
    // label the replay owns: a replayed row reads `history`, a live fold of the
    // same record reads its own source.
    assert_eq!(
        replayed.compaction.blocks.len(),
        live.compaction.blocks.len()
    );
    assert_eq!(replayed.compaction.blocks.len(), 1);
    assert_eq!(replayed.compaction.blocks[0].source, "history");
    assert_eq!(live.compaction.blocks[0].source, "auto");

    // -- Check: the displayed tokens. Replay applies the persisted total after
    // the fold, exactly where the live status line leaves it.
    let (input, output, total) = token_state(&replayed);
    let live_tokens = token_state(&live);
    assert_eq!((input, output), (live_tokens.0, live_tokens.1));
    assert_eq!(total, Some(14000), "the persisted total is displayed");

    // -- Check: the replay ends at rest, not mid-turn.
    assert_eq!(replayed.phase, UiPhase::Idle);
    assert!(!replayed.input_locked);
    Ok(())
}

/// A replayed tool call holds ONE terminal entry whose completion body is
/// derived from the persisted result data — the same shape the live path leaves.
#[test]
fn test_state_hydration_replay_lands_one_terminal_tool_entry() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    let diff = "--- a\n+++ b\n";

    // -- Exec
    hydrate_by_replay(
        &mut state,
        vec![tool(
            "edit",
            r#"{"path":"a.rs"}"#,
            &edit_result("a.rs", diff),
            Some(true),
        )],
        None,
    );

    // -- Check
    assert_eq!(state.transcript.len(), 1, "one call, one entry");
    let entry = state
        .transcript
        .entries()
        .first()
        .ok_or("should have the tool entry")?;
    let TranscriptEntry::Tool(tool) = entry else {
        return Err("expected a Tool entry".into());
    };
    assert_eq!(tool.status, ItemStatus::Done);
    assert!(tool.preview.is_none(), "the replay carries no gate content");
    let result = tool
        .result
        .as_ref()
        .ok_or("should have the completion body")?;
    assert!(
        display_text(result).contains("+++ b"),
        "the persisted diff is the completion body"
    );
    Ok(())
}

/// The replayed effects are discarded: the queues are empty, and the prompt
/// rows still landed in the transcript through the drain.
#[test]
fn test_state_hydration_replay_discards_pending_effects() -> Result<()> {
    // -- Setup & Fixtures
    let cancel_controller = CancelController::default();
    let mut state = AppState::default();

    // -- Exec
    hydrate_by_replay(
        &mut state,
        vec![
            UiMessageSnapshot::new("user", "restored question"),
            assistant("restored answer", None),
        ],
        None,
    );

    // -- Check: nothing is left to execute.
    assert_eq!(
        state.prompt_items().len(),
        1,
        "the prompt is history, not work"
    );
    assert_eq!(state.pending_prompt_count(), 0, "nothing waits to run");
    assert!(state.take_pending_events(&cancel_controller).is_empty());

    // -- Check: the drain still placed the user row.
    assert_eq!(
        entry_roles(&state),
        vec!["user", "assistant"],
        "the prompt row lands through the drain"
    );
    Ok(())
}

/// WHEN the replay folds a stamped prompt row, THE replay lane SHALL place the
/// row, SHALL drain the submission the placement rode, and SHALL leave no
/// `OrchestratorEvent` and no launch request for a later drain.
#[test]
fn test_state_hydration_replay_places_the_row_and_drains_its_submission() -> Result<()> {
    // -- Setup & Fixtures: one stamped persisted row.
    let cancel_controller = CancelController::default();
    let mut state = AppState::default();
    let stamped = UiMessageSnapshot::new("user", "replayed question").with_stamp(
        Some(1_765_000_000_123),
        Some("openai/gpt-5".to_string()),
        Some("coder".to_string()),
    );

    // -- Exec
    hydrate_by_replay(&mut state, vec![stamped], None);

    // -- Check: the row landed, carrying the record's stored facts.
    let rows = user_rows(&state);
    assert_eq!(rows.len(), 1, "one persisted row, one entry");
    assert_eq!(rows[0].markdown, "replayed question");
    assert_eq!(rows[0].timestamp_millis, Some(1_765_000_000_123));
    assert_eq!(rows[0].model.as_deref(), Some("openai/gpt-5"));
    assert_eq!(rows[0].agent.as_deref(), Some("coder"));

    // -- Check: the submission the placement rode is drained, so the replay
    // leaves nothing for a later drain to re-execute, and opens no panel.
    assert_eq!(state.pending_prompt_count(), 0, "nothing waits to run");
    assert!(
        state.take_pending_events(&cancel_controller).is_empty(),
        "the replay's outbound effects are not re-executed"
    );
    assert!(
        state.take_next_launch_request().is_none(),
        "a persisted row opens no panel"
    );
    Ok(())
}

/// WHEN a run of adjacent replayed prompts folds, THE transcript SHALL carry one
/// entry per persisted row and the coalesced block shape the live gate pins: a
/// block whose text is the newline join replays verbatim, so it paints one row
/// per message (`test_renderer_layout_coalesced_user_turn_paints_one_row_per_prompt`).
#[test]
fn test_state_hydration_replay_keeps_the_coalesced_block_shape() -> Result<()> {
    // -- Setup & Fixtures: a coalesced block, then a later prompt.
    let mut state = AppState::default();

    // -- Exec
    hydrate_by_replay(
        &mut state,
        vec![
            UiMessageSnapshot::new("user", "first message\nsecond message"),
            assistant("answer", None),
            UiMessageSnapshot::new("user", "third message"),
        ],
        None,
    );

    // -- Check: two entries, and the block stays ONE entry carrying the join.
    let rows = user_rows(&state);
    assert_eq!(rows.len(), 2, "one entry per persisted row");
    assert_eq!(
        rows[0].markdown, "first message\nsecond message",
        "the coalesced block replays verbatim"
    );
    assert_eq!(rows[1].markdown, "third message");
    Ok(())
}

/// WHEN the replay folds a stamped prompt, THE pushed entry SHALL hold exactly
/// the event's facts; an unstamped prompt SHALL stay field-less.
#[test]
fn test_state_hydration_stamped_replay_carries_the_facts() -> Result<()> {
    // -- Setup & Fixtures: one stamped prompt, one without facts.
    let stamped = UiMessageSnapshot::new("user", "stamped prompt").with_stamp(
        Some(1_765_000_000_000),
        Some("openai/gpt-5".to_string()),
        Some("coder".to_string()),
    );
    let plain = UiMessageSnapshot::new("user", "plain prompt");
    let mut state = AppState::default();

    // -- Exec
    hydrate_by_replay(&mut state, vec![stamped, plain], None);

    // -- Check: the stamped row holds the snapshot's values verbatim.
    let rows: Vec<&MessageEntry> = state
        .transcript
        .entries()
        .iter()
        .filter_map(|entry| match entry {
            TranscriptEntry::Message(message) => Some(message),
            _ => None,
        })
        .collect();
    assert_eq!(rows.len(), 2, "both prompts replay a row");
    assert_eq!(rows[0].timestamp_millis, Some(1_765_000_000_000));
    assert_eq!(rows[0].model.as_deref(), Some("openai/gpt-5"));
    assert_eq!(rows[0].agent.as_deref(), Some("coder"));

    // -- Check: the fact-less row invents nothing.
    assert_eq!(rows[1].timestamp_millis, None);
    assert_eq!(rows[1].model, None);
    assert_eq!(rows[1].agent, None);
    Ok(())
}

// endregion: --- Replay
