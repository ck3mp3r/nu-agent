//! The mid-loop switch replay: the rows a rehydrated session shows when the
//! surface switches to it mid-run.
//!
//! The switch publishes one record log — the switch marker, then the projected
//! session — and the surface folds it exactly as it folds a live turn. Every
//! replayed call derives the card and the completion body the live fold derives,
//! across every turn the log carries, until the first live turn starts.

use super::*;

use crate::interaction::reducer::RenderInput;
use crate::test_support::session_switched;
use agnt_render::transcript::entry::{ToolEntry, TranscriptEntry};
use agnt_render::transcript::ir::Display;
use agnt_runtime::bus::LifecycleEvent;
use agnt_runtime::protocol::contracts::{UiMessageSnapshot, project_snapshots};

/// The text one display projects to, line by line.
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

/// One persisted tool row: the call as data and the result the store recorded.
fn tool_row(name: &str, arguments: &str, result: &str) -> UiMessageSnapshot {
    UiMessageSnapshot::new("tool", String::new())
        .with_tool_name(name.to_string())
        .with_tool_details(
            Some(arguments.to_string()),
            Some(result.to_string()),
            Some(true),
        )
}

/// The replayed tool entries, in transcript order.
fn tool_entries(coordinator: &RuntimeCoordinator) -> Vec<&ToolEntry> {
    coordinator
        .state
        .transcript
        .entries()
        .iter()
        .filter_map(|entry| match entry {
            TranscriptEntry::Tool(tool) => Some(tool),
            _ => None,
        })
        .collect()
}

/// WHEN a mid-loop switch replay covers multiple turns, THE replayed fold SHALL
/// derive the nu card on every replayed nu entry and the edit's diff body from
/// the stored result text; the inter-turn `TurnCompleted` boundaries SHALL NOT
/// close the replay latch; WHEN the first live `TurnStarted` folds, THE latch
/// SHALL close and a live ungated call SHALL derive nothing.
#[test]
fn test_runtime_replay_preview_derives_across_every_replayed_turn() -> Result<()> {
    // -- Setup & Fixtures: a two-turn session, projected as the switch publishes
    // it: a nu call in the first turn, an edit and a second nu call in the
    // second.
    let messages = vec![
        UiMessageSnapshot::new("user", "first question"),
        tool_row("nu", r#"{"command":"ls"}"#, "a.txt"),
        UiMessageSnapshot::new("user", "second question"),
        tool_row(
            "edit",
            r#"{"path":"a.rs"}"#,
            r#"{"path":"a.rs","diff":"+++ b"}"#,
        ),
        tool_row("nu", r#"{"command":"pwd"}"#, "/tmp"),
    ];
    let (records, _total) = project_snapshots(messages);
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));

    // -- Exec: the switch marker, then the whole projected session.
    coordinator.fold_record(&session_switched());
    for record in &records {
        coordinator.fold_record(record);
    }

    // -- Check: one entry per replayed call.
    let tools = tool_entries(&coordinator);
    assert_eq!(tools.len(), 3, "one entry per replayed call");

    // -- Check: the first turn's nu call derives its card, after the switch
    // marker alone opened the replay.
    let first = tools.first().ok_or("should have the first nu entry")?;
    let card = first
        .preview
        .as_ref()
        .ok_or("the first turn's nu card must derive on replay")?;
    assert!(
        display_text(card).contains("ls"),
        "the card carries the command"
    );

    // -- Check: the second turn's edit carries its diff as the completion body,
    // derived from the stored result text the outcome payload carries.
    let second = tools.get(1).ok_or("should have the edit entry")?;
    let body = second
        .result
        .as_ref()
        .ok_or("the replayed edit must carry its diff body")?;
    assert!(
        display_text(body).contains("+++ b"),
        "the stored result is the completion body"
    );

    // -- Check: the inter-turn boundary did not close the latch — the second
    // turn's nu call derives its card too.
    let third = tools.get(2).ok_or("should have the second nu entry")?;
    let late_card = third
        .preview
        .as_ref()
        .ok_or("the second turn's nu card must derive on replay")?;
    assert!(
        display_text(late_card).contains("pwd"),
        "the card carries the command"
    );

    // -- Exec: the first live turn after the batch starts.
    let live_start = LifecycleEvent::TurnStarted {
        prompt: "third question".to_string(),
        task_id: None,
    };
    coordinator.reduce_render_input(RenderInput::Lifecycle(&live_start));
    let live_call = ToolCall::new(
        ToolName("nu".to_string()),
        agnt_runtime::tools::handler::ToolSource::Unknown,
        r#"{"command":"tree"}"#,
    );
    coordinator.fold_record(&tool_started(live_call));

    // -- Check: the latch is closed — a live call derives no card; the gate
    // record is the only live card source.
    let live = tool_entries(&coordinator)
        .into_iter()
        .next_back()
        .ok_or("should have the live entry")?;
    assert!(
        live.preview.is_none(),
        "a live call after the replay derives no card outside the gate"
    );
    assert_eq!(
        live.status,
        ItemStatus::InProgress,
        "the live call is still running"
    );
    Ok(())
}
