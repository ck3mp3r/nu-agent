//! Hydration replay: the snapshot projection and the replay determinism.
//!
//! The projection tests pin the snapshot to event mapping. The determinism test
//! is the hydration contract: the state a replay produces equals the state the
//! same event log produces when the reducer folds it live.

use super::*;

use crate::interaction::cancel::CancelController;
use crate::interaction::reducer::reduce;
use agnt_core::protocol::contracts::{UiMessageSnapshot, UiMessageUsageSnapshot};
use agnt_core::protocol::event::{ToolDisplay, ToolDisplaySection};
use agnt_core::protocol::events::SystemEvent;
use agnt_core::session::tool_status::ToolStatus;
use agnt_core::transcript::entry::TranscriptEntry;
use agnt_core::transcript::ir::{ContentKind, MessageRole};
use agnt_core::transcript::renderer::ItemStatus;

pub(super) type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

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

fn tool(name: &str, arguments: &str, success: Option<bool>) -> UiMessageSnapshot {
    UiMessageSnapshot::new("tool", format!("-> {arguments}"))
        .with_tool_name(name.to_string())
        .with_tool_details(Some(arguments.to_string()), None, success)
}

fn diff_display(title: &str, content: &str) -> ToolDisplay {
    ToolDisplay {
        title: Some(title.to_string()),
        sections: vec![ToolDisplaySection {
            label: Some(title.to_string()),
            kind: ContentKind::Diff {
                language: "diff".to_string(),
                path: "a.rs".to_string(),
            },
            content: content.to_string(),
            stats: None,
        }],
    }
}

fn tool_display(display: ToolDisplay) -> UiMessageSnapshot {
    UiMessageSnapshot::new("tool_display", String::new()).with_tool_display(display)
}

/// A two-turn persisted session: prose, a finished tool call with its display,
/// a compaction summary, and prose after it.
fn persisted_session() -> Vec<UiMessageSnapshot> {
    vec![
        UiMessageSnapshot::new("user", "first question"),
        assistant("first answer", Some(usage(3, 2, 5))),
        tool("nu", r#"{"command":"ls"}"#, Some(true)),
        tool_display(diff_display("ls", "--- a\n+++ b\n")),
        UiMessageSnapshot::new("user", "second question"),
        UiMessageSnapshot::new("compaction", "## Summary\n- alpha"),
        assistant("third answer", None),
    ]
}

/// Fold the projected log the way the live session folds it: one reduce per
/// event, the effect queue drained after each, exactly as the coordinator does.
fn live_reduction(messages: &[UiMessageSnapshot]) -> AppState {
    let cancel_controller = CancelController::default();
    let mut state = AppState::default();
    for step in project_snapshots(messages.to_vec()) {
        match step {
            ReplayStep::Event(event) => {
                reduce(&mut state, *event, None);
                let _ = state.take_pending_events(&cancel_controller);
                let _ = state.take_pending_ui_state_events();
            }
            ReplayStep::Notice(text) => {
                crate::interaction::reducer::push_system_notice(&mut state, text)
            }
        }
    }
    state
}

/// The projection's event half: the steps the reducer folds, without the
/// persisted system lines that bypass it.
fn projected_events(messages: Vec<UiMessageSnapshot>) -> Vec<SystemEvent> {
    project_snapshots(messages)
        .into_iter()
        .filter_map(|step| match step {
            ReplayStep::Event(event) => Some(*event),
            ReplayStep::Notice(_) => None,
        })
        .collect()
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
            TranscriptEntry::Notice(_) => "notice",
            TranscriptEntry::Banner(_) => "banner",
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
    let log = projected_events(messages);

    // -- Check
    assert_eq!(log.len(), 2, "the prompt plus the turn boundary");
    assert!(matches!(
        log.first(),
        Some(SystemEvent::PromptSubmitted { text }) if text == "hi"
    ));
    assert!(matches!(
        log.get(1),
        Some(SystemEvent::TurnCompleted { .. })
    ));
}

/// An assistant snapshot is one non-streamed chunk: both chunk fields carry
/// the whole text, and the completion carries the persisted usage.
#[test]
fn test_state_hydration_projection_maps_assistant_chunk_and_usage() {
    // -- Setup & Fixtures
    let messages = vec![assistant("hello", Some(usage(3, 2, 5)))];

    // -- Exec
    let log = projected_events(messages);

    // -- Check
    assert!(matches!(log.first(), Some(SystemEvent::InferenceStarted)));
    let Some(SystemEvent::InferenceChunk { delta, aggregated }) = log.get(1) else {
        panic!("expected the chunk");
    };
    assert_eq!(delta, "hello", "delta carries the whole text");
    assert_eq!(aggregated, "hello", "aggregated carries the whole text");
    let Some(SystemEvent::InferenceCompleted { usage }) = log.get(2) else {
        panic!("expected the completion");
    };
    assert_eq!(usage.input_tokens, 3);
    assert_eq!(usage.output_tokens, 2);
    assert_eq!(usage.total_tokens, Some(5));
}

/// A tool snapshot and the display snapshot after it become the started call,
/// its gate preview, and its displayless completion; a later call's display
/// never lands on the earlier call.
#[test]
fn test_state_hydration_projection_maps_tool_pair_with_display() {
    // -- Setup & Fixtures
    let messages = vec![
        tool("edit", r#"{"path":"a.rs"}"#, Some(true)),
        tool_display(diff_display("edit a.rs", "--- a\n+++ b\n")),
        tool("read_file", r#"{"path":"b.txt"}"#, Some(true)),
    ];

    // -- Exec
    let log = projected_events(messages);

    // -- Check: start, gate preview, displayless completion per call, and the
    // display rides its OWN call.
    let Some(SystemEvent::ToolStarted { call: first_start }) = log.first() else {
        panic!("expected the first start");
    };
    let Some(SystemEvent::PreviewRequested {
        call: first_preview,
        ..
    }) = log.get(1)
    else {
        panic!("expected the first gate preview");
    };
    assert_eq!(
        first_start.id, first_preview.id,
        "the preview names the call"
    );
    let Some(SystemEvent::ToolCompleted {
        call: first_end,
        outcome,
    }) = log.get(2)
    else {
        panic!("expected the first completion");
    };
    assert_eq!(first_start.id, first_end.id, "the pair names one call");
    assert!(
        outcome.display.is_none(),
        "the display is the gate preview, not the completion"
    );
    let Some(SystemEvent::ToolStarted { call: second_start }) = log.get(3) else {
        panic!("expected the second start");
    };
    let Some(SystemEvent::ToolCompleted { outcome, .. }) = log.get(4) else {
        panic!("expected the second completion");
    };
    assert_eq!(second_start.name.0, "read_file");
    assert!(outcome.display.is_none(), "no display for the second call");
    assert_eq!(log.len(), 6, "two pairs, one preview, the turn boundary");
}

/// A hydrated tool display is a GATE PREVIEW, not a completion — for every
/// outcome. The replayed log carries it through `PreviewRequested` (the event
/// the live permission gate emits) and completes displayless with the status
/// semantics only, so the entry keeps the preview slot main's hydration used and
/// the widget keeps banding the code surface.
#[test]
fn test_state_hydration_projection_routes_every_tool_display_to_the_gate_preview() -> Result<()> {
    // -- Setup & Fixtures: one persisted pair per outcome, as a cancelled run,
    // a failed run, and a successful run leave them behind.
    let display = ToolDisplay {
        title: None,
        sections: vec![ToolDisplaySection {
            label: None,
            kind: ContentKind::Code {
                language: "nu".to_string(),
            },
            content: "ls | select name".to_string(),
            stats: None,
        }],
    };
    let cases: [(&str, ToolStatus); 3] = [
        ("cancelled", ToolStatus::Cancelled),
        ("failed", ToolStatus::Failed),
        ("success", ToolStatus::Success),
    ];

    for (label, status) in cases {
        let success = Some(matches!(status, ToolStatus::Success));
        let snapshot =
            tool("nu", r#"{"command":"ls | select name"}"#, success).with_tool_status(status);
        let messages = vec![snapshot, tool_display(display.clone())];

        // -- Exec
        let log = projected_events(messages);

        // -- Check: start, gate preview, displayless completion, turn boundary.
        assert_eq!(
            log.len(),
            4,
            "{label}: one pair plus its preview and the boundary"
        );
        let Some(SystemEvent::ToolStarted { call: started }) = log.first() else {
            return Err("expected the tool start".into());
        };
        let Some(SystemEvent::PreviewRequested {
            call: previewed,
            display: preview,
        }) = log.get(1)
        else {
            return Err("expected the gate preview".into());
        };
        assert_eq!(
            previewed.id, started.id,
            "{label}: the preview names the call"
        );
        assert_eq!(
            preview, &display,
            "{label}: the persisted display rides the preview channel"
        );
        let Some(SystemEvent::ToolCompleted {
            call: ended,
            outcome,
        }) = log.get(2)
        else {
            return Err("expected the completion".into());
        };
        assert_eq!(
            ended.id, started.id,
            "{label}: the completion names the call"
        );
        assert!(
            outcome.display.is_none(),
            "{label}: the completion carries no display"
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
    let steps = project_snapshots(messages);

    // -- Check
    assert_eq!(
        steps.len(),
        2,
        "two non-empty system lines, one notice each"
    );
    assert_eq!(
        steps.first(),
        Some(&ReplayStep::Notice("system fallback".to_string()))
    );
    assert_eq!(
        steps.get(1),
        Some(&ReplayStep::Notice("a system prompt".to_string())),
        "the notice carries the trimmed line"
    );
    assert!(project_snapshots(Vec::<UiMessageSnapshot>::new()).is_empty());
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

    // -- Check: the same entries, in the same order.
    assert_eq!(
        replayed.transcript.entries(),
        live.transcript.entries(),
        "the replayed entries must equal the live entries"
    );
    assert_eq!(
        entry_roles(&replayed),
        vec![
            "user",
            "assistant",
            "tool",
            "user",
            "notice",
            "assistant",
            "assistant"
        ],
        "one entry per persisted fact: the compaction header plus its summary"
    );

    // -- Check: main's hydrated shape. The hand-builder docked every persisted
    // display in the tool entry's PREVIEW slot and never set `result`, so a
    // hydrated row renders banded whatever its verdict.
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
    assert!(
        tool.preview.is_some(),
        "main docked the display in the preview slot"
    );
    assert!(
        tool.result.is_none(),
        "main's hydration never set a completion display"
    );

    // -- Check: the compaction bookkeeping is the live bookkeeping.
    assert_eq!(replayed.compaction.blocks, live.compaction.blocks);
    assert_eq!(replayed.compaction.blocks.len(), 1);
    assert_eq!(replayed.compaction.blocks[0].source, "history");

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

/// A replayed tool call holds ONE terminal entry carrying the persisted display
/// in its gate-preview slot — main's hydrated arrangement, which is also the
/// live path's shape.
#[test]
fn test_state_hydration_replay_lands_one_terminal_tool_entry() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();

    // -- Exec
    hydrate_by_replay(
        &mut state,
        vec![
            tool("edit", r#"{"path":"a.rs"}"#, Some(true)),
            tool_display(diff_display("edit a.rs", "--- a\n+++ b\n")),
        ],
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
    assert!(
        tool.preview.is_some(),
        "the persisted display docks in the gate-preview slot"
    );
    assert!(
        tool.result.is_none(),
        "a hydrated completion carries no display of its own"
    );
    let preview = tool.preview.as_ref().ok_or("should have the preview")?;
    let text: String = preview
        .project_lines()
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.text.as_str())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("+++ b"), "the persisted diff is the preview");
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
    assert!(state.take_pending_ui_state_events().is_empty());

    // -- Check: the drain still placed the user row.
    assert_eq!(
        entry_roles(&state),
        vec!["user", "assistant"],
        "the prompt row lands through the drain"
    );
    Ok(())
}

// endregion: --- Replay
