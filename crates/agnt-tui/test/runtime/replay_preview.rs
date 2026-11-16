//! The mid-loop switch replay: the rows a rehydrated session shows when the
//! surface switches to it mid-run.
//!
//! The switch publishes one record log — the switch marker, then the projected
//! session — and the surface folds it exactly as it folds a live turn. Every
//! replayed call derives the card and the completion body the live fold derives,
//! across every turn the log carries, until the first live turn starts.

use super::*;

use crate::interaction::reducer::RenderInput;
use crate::runtime::status::content::model_activity_label;
use crate::test_support::{inference_completed, session_switched};
use agnt_render::transcript::entry::{ToolEntry, TranscriptEntry};
use agnt_render::transcript::ir::Display;
use agnt_runtime::bus::LifecycleEvent;
use agnt_runtime::protocol::contracts::{
    UiMessageSnapshot, UiMessageUsageSnapshot, project_snapshots,
};
use agnt_runtime::protocol::events::InferenceRecord;

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

/// WHEN the surface switches into the same history repeatedly, THE footer
/// figures SHALL hold: the token figure re-places to that history's own total
/// and the dollars keep the value the live calls left — each repeat adds nothing
/// to and removes nothing from either figure.
#[test]
fn test_runtime_replay_preview_repeated_switch_holds_the_figures() -> Result<()> {
    // -- Setup & Fixtures: one persisted answered prompt, with the usage the
    // store recorded for it, and one live priced call on the surface.
    let messages = vec![
        UiMessageSnapshot::new("user", "first question"),
        assistant(
            "first answer",
            Some(UiMessageUsageSnapshot {
                input_tokens: Some(40),
                output_tokens: Some(20),
                total_tokens: Some(60),
            }),
        ),
    ];
    let (records, _total) = project_snapshots(messages);
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.fold_record(&inference_completed(40, 20, Some(60), Some(0.002), true));
    let mut displayed = vec![coordinator.state.status.tokens.latest_total_tokens];
    let mut labels = vec![coordinator.state.status.tokens.session_cost.label()];

    // -- Exec: the switch marker and the whole projected session, twice — the
    // shape a user switching back and forth produces.
    for _ in 0..2 {
        coordinator.fold_record(&session_switched());
        for record in &records {
            coordinator.fold_record(record);
        }
        displayed.push(coordinator.state.status.tokens.latest_total_tokens);
        labels.push(coordinator.state.status.tokens.session_cost.label());
    }

    // -- Check: the token figure re-places on every switch and accumulates none.
    assert_eq!(
        displayed,
        vec![Some(60), Some(60), Some(60)],
        "each switch places the history's figure, and none accumulates it"
    );

    // -- Check: the dollars hold their accumulated value. A replayed call is
    // unpriced, so the label gains the lower-bound marker; it adds no money, and
    // the repeat adds nothing more.
    assert_eq!(
        labels[0].as_deref(),
        Some("$0.002"),
        "the live priced call is the only money the figure sees"
    );
    assert_eq!(
        labels[1].as_deref(),
        Some("$0.002+"),
        "the replay adds the unpriced marker and no money"
    );
    assert_eq!(
        labels[2].as_deref(),
        Some("$0.002+"),
        "a repeat switch adds nothing to the dollars"
    );

    // -- Exec: a live priced call in the session the surface switched into.
    let live_start = LifecycleEvent::TurnStarted {
        prompt: "second question".to_string(),
        task_id: None,
    };
    coordinator.reduce_render_input(RenderInput::Lifecycle(&live_start));
    coordinator.fold_record(&inference_completed(40, 20, Some(60), Some(0.002), true));

    // -- Check: the boundary reset nothing — the live call summed onto the
    // accumulated figure.
    assert_eq!(
        coordinator
            .state
            .status
            .tokens
            .session_cost
            .label()
            .as_deref(),
        Some("$0.004+"),
        "a live priced call adds to the figure the boundary carried over"
    );
    Ok(())
}

/// One persisted assistant row carrying the usage the store recorded for it.
fn assistant(text: &str, usage: Option<UiMessageUsageSnapshot>) -> UiMessageSnapshot {
    let mut snapshot = UiMessageSnapshot::new("assistant", text);
    snapshot.usage = usage;
    snapshot
}

/// WHEN a switch replays a session whose history holds an answered prompt, THE
/// surface SHALL land at rest: the replayed call takes the lock exactly as a
/// live one does, and the log's own turn boundary releases it, so the spinner
/// stops with no live turn to stop it.
#[test]
fn test_runtime_replay_preview_switch_into_history_lands_at_rest() -> Result<()> {
    // -- Setup & Fixtures: one answered prompt in the store.
    let messages = vec![
        UiMessageSnapshot::new("user", "first question"),
        UiMessageSnapshot::new("assistant", "first answer"),
    ];
    let (records, _total) = project_snapshots(messages);
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));

    // -- Exec: the switch marker, then the whole projected session.
    coordinator.fold_record(&session_switched());
    let mut locked_by_the_replayed_call = false;
    for record in &records {
        coordinator.fold_record(record);
        if matches!(
            record,
            SessionRecord::Inference(InferenceRecord::Started { .. })
        ) {
            locked_by_the_replayed_call = coordinator.state.input_locked;
        }
    }

    // -- Check: the replayed call did take the lock, so the rest is the log's
    // own boundary releasing it — not a replay that never locked.
    assert!(
        locked_by_the_replayed_call,
        "a replayed completion call takes the lock exactly as a live one does"
    );
    assert_eq!(
        coordinator.state.phase,
        UiPhase::Idle,
        "the replayed session ends at rest"
    );
    assert!(
        !coordinator.state.input_locked,
        "no release-less replay may strand the lock"
    );
    assert!(
        !coordinator.state.is_active_cycle(),
        "no turn cycle survives the replay"
    );
    assert_eq!(
        model_activity_label(&coordinator.state),
        "idle",
        "the busy spinner the switch used to strand stays off"
    );
    Ok(())
}
