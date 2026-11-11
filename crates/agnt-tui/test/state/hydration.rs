//! Hydration replay: the snapshot projection and the replay determinism.
//!
//! The projection tests pin the snapshot to event mapping. The determinism test
//! is the hydration contract: the state a replay produces equals the state the
//! same event log produces when the reducer folds it live.

use super::*;

use crate::interaction::cancel::CancelController;
use crate::interaction::reducer::{RenderInput, reduce};
use crate::test_support::entry_text;
use agnt_render::transcript::entry::{MessageEntry, TranscriptEntry};
use agnt_render::transcript::ir::{Display, MessageRole};
use agnt_render::transcript::renderer::ItemStatus;
use agnt_runtime::protocol::contracts::{
    UiMessageSnapshot, UiMessageUsageSnapshot, project_snapshots,
};
use agnt_runtime::protocol::events::{InferenceRecord, SessionRecord, ToolRecord};
use agnt_runtime::session::tool_status::ToolStatus;

pub(super) type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// One display flattened to the text a backend draws, line by line.
fn display_text(display: &Display) -> String {
    display
        .project_lines()
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.text.as_str())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// region:    --- Fixtures

fn usage(input_tokens: u64, output_tokens: u64, total_tokens: u64) -> UiMessageUsageSnapshot {
    UiMessageUsageSnapshot {
        input_tokens: Some(input_tokens),
        output_tokens: Some(output_tokens),
        total_tokens: Some(total_tokens),
    }
}

fn assistant(text: &str, usage: Option<UiMessageUsageSnapshot>) -> UiMessageSnapshot {
    let mut snapshot = UiMessageSnapshot::new("assistant", text);
    snapshot.usage = usage;
    snapshot
}

/// One persisted tool row: the call it names, the result the model received,
/// and the verdict the store recorded. The content IS the tool's data — the
/// surface derives the body from it, live and on replay alike.
fn tool(name: &str, arguments: &str, result: &str, success: Option<bool>) -> UiMessageSnapshot {
    UiMessageSnapshot::new("tool", result.to_string())
        .with_tool_name(name.to_string())
        .with_tool_details(
            Some(arguments.to_string()),
            Some(result.to_string()),
            success,
        )
}

/// The result text an `edit` call leaves in the transcript: the path it changed
/// and the diff it made, as the handler returned them.
fn edit_result(path: &str, diff: &str) -> String {
    // `serde_json` is not a test dependency here: build the handler's own shape.
    let escaped = diff
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n");
    format!("{{\"path\":\"{path}\",\"diff\":\"{escaped}\"}}")
}

/// A two-turn persisted session: prose, a finished tool call with its result, a
/// compaction summary, and prose after it.
fn persisted_session() -> Vec<UiMessageSnapshot> {
    let diff = "--- a\n+++ b\n";
    vec![
        UiMessageSnapshot::new("user", "first question"),
        assistant("first answer", Some(usage(3, 2, 5))),
        tool(
            "edit",
            r#"{"path":"a.rs"}"#,
            &edit_result("a.rs", diff),
            Some(true),
        ),
        UiMessageSnapshot::new("user", "second question"),
        UiMessageSnapshot::new("compaction", "## Summary\n- alpha"),
        assistant("third answer", None),
    ]
}

/// Fold the projected log the way the live session folds it: one reduce per
/// record, the effect queue drained after each, exactly as the coordinator does.
fn live_reduction(messages: &[UiMessageSnapshot]) -> AppState {
    let cancel_controller = CancelController::default();
    let mut state = AppState::default();
    let (records, _total) = project_snapshots(messages.to_vec());
    for record in records {
        reduce(&mut state, RenderInput::Record(&record), None);
        let _ = state.take_pending_events(&cancel_controller);
        while state.take_next_launch_request().is_some() {}
    }
    state
}

/// The projection's record log.
fn projected_records(messages: Vec<UiMessageSnapshot>) -> Vec<SessionRecord> {
    project_snapshots(messages).0
}

/// The displayed token counters, in the order the status line reads them.
fn token_state(state: &AppState) -> (Option<u64>, Option<u64>, Option<u64>, u64) {
    let tokens = &state.status.tokens;
    (
        tokens.latest_input_tokens,
        tokens.latest_output_tokens,
        tokens.latest_total_tokens,
        tokens.session_total_tokens,
    )
}

fn entry_roles(state: &AppState) -> Vec<&'static str> {
    state
        .transcript
        .entries()
        .iter()
        .map(|entry| match entry {
            TranscriptEntry::Message(message) if message.role == MessageRole::User => "user",
            TranscriptEntry::Message(_) => "assistant",
            TranscriptEntry::Tool(_) => "tool",
            TranscriptEntry::Task(_) => "task",
            TranscriptEntry::Notice(_) => "notice",
            TranscriptEntry::Banner(_) => "banner",
        })
        .collect()
}

/// Every user message row in the transcript, in order.
fn user_rows(state: &AppState) -> Vec<&MessageEntry> {
    state
        .transcript
        .entries()
        .iter()
        .filter_map(|entry| match entry {
            TranscriptEntry::Message(message) if message.role == MessageRole::User => Some(message),
            _ => None,
        })
        .collect()
}

// endregion: --- Fixtures

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
        Some(SessionRecord::Inference(InferenceRecord::Started))
    ));
    let Some(SessionRecord::Inference(InferenceRecord::Chunk { delta, aggregated })) = log.get(1)
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
    let (input, output, total, session_total) = token_state(&replayed);
    let live_tokens = token_state(&live);
    assert_eq!((input, output), (live_tokens.0, live_tokens.1));
    assert_eq!(total, Some(14000), "the persisted total is displayed");
    // The persisted session total is authoritative over the per-message sums.
    assert_eq!(session_total, 14000);
    assert_eq!(live.status.tokens.session_total_tokens, 5);

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
