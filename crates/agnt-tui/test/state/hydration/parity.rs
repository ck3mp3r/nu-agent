//! Replayed preview parity: a replayed row draws the card its live fold drew.
//!
//! A card is a drawer derivation of the call's own data, never a
//! producer-rendered payload. The live gate attaches it from a gate record;
//! a replayed call has no gate record, so the replay lane derives it from the
//! call alone. These tests pin the lane whose card the call's own data carries
//! (a `nu` command) byte for byte, the lane whose card only the producer can
//! supply (an `edit` diff) as the documented asymmetry, and the replay window's
//! boundary.
use super::*;

// region:    --- Fixtures

use crate::interaction::reducer::{RenderInput, reduce};
use agnt_render::tool_view::ToolView;
use agnt_render::transcript::entry::ToolEntry;
use agnt_render::transcript::ir::{ContentKind, DisplaySection};
use agnt_runtime::protocol::events::{
    LifecycleEvent, PermissionRecord, RequestId, SessionId, SessionLifecycle, ToolCall, ToolCallId,
    ToolOutcome,
};

/// An `edit` call as the model writes it: the path and the search/replace
/// operation. The diff the card shows is planned against the file on disk, so
/// these arguments are not the card.
const EDIT_ARGS: &str = r#"{"path":"a.rs","operation":{"type":"search_replace","search":"old line","replacement":"new line"}}"#;
const EDIT_DIFF: &str = "--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-old line\n+new line\n";

/// A `nu` call: the command the card shows IS its arguments.
const NU_ARGS: &str = r#"{"command":"ls | select name"}"#;
const OUT_OF_WINDOW_ARGS: &str = r#"{"command":"ls | length"}"#;

fn call(name: &str, arguments: &str) -> ToolCall {
    ToolCall::new(
        name.to_string(),
        agnt_runtime::tools::handler::ToolSource::Builtin,
        arguments,
    )
}

fn started(name: &str, arguments: &str) -> SessionRecord {
    SessionRecord::Tool(ToolRecord::Started {
        call: call(name, arguments),
    })
}

fn completed(name: &str, arguments: &str, result: &str) -> SessionRecord {
    SessionRecord::Tool(ToolRecord::Completed {
        call: call(name, arguments),
        outcome: ToolOutcome {
            status: ToolStatus::Success,
            payload: None,
            text: result.to_string(),
            message: None,
            kind: None,
        },
    })
}

/// The gate record a prompted call narrates: the exact call key plus the
/// content only the producer can supply. A `nu` card needs no content.
fn gate(name: &str, arguments: &str, preview: Option<&str>) -> SessionRecord {
    SessionRecord::Permission(PermissionRecord::Requested {
        request_id: RequestId::mint(),
        call_id: ToolCallId::new(name, arguments),
        tool: name.to_string(),
        source: "builtin".to_string(),
        rule_identity: format!("tool:{name}"),
        scope: "tool".to_string(),
        pattern: name.to_string(),
        target_field: None,
        mode: Some("apply".to_string()),
        summary: format!("→ {name}"),
        shows_preview: true,
        preview: preview.map(str::to_string),
    })
}

/// The session switch that opens the replay window.
fn switched() -> SessionRecord {
    SessionRecord::Session(SessionLifecycle::Switched {
        from: None,
        to: SessionId::new("switched-to"),
    })
}

/// The first live turn after a replay batch: it ends the window.
fn first_live_turn() -> LifecycleEvent {
    LifecycleEvent::TurnStarted {
        prompt: "next question".to_string(),
        task_id: None,
    }
}

// endregion: --- Fixtures

// region:    --- Support

/// Fold one record through the reducer, discarding the effects it produces —
/// the live lane's drain, so the two folds differ in nothing but the records.
fn fold(state: &mut AppState, record: &SessionRecord) {
    let cancel_controller = CancelController::default();
    reduce(state, RenderInput::Record(record), None);
    state.discard_pending_events(&cancel_controller);
    while state.take_next_launch_request().is_some() {}
}

/// The tool row one call produced.
fn tool_row<'a>(state: &'a AppState, name: &str) -> Result<&'a ToolEntry> {
    state
        .transcript
        .entries()
        .iter()
        .find_map(|entry| match entry {
            TranscriptEntry::Tool(tool) if tool.call.name.0 == name => Some(tool),
            _ => None,
        })
        .ok_or_else(|| format!("should have the {name} row").into())
}

/// The card a row carries, cloned so the comparison is not a borrow dance.
fn preview_of(state: &AppState, name: &str) -> Result<Option<Display>> {
    Ok(tool_row(state, name)?.preview.clone())
}

/// The one section a diff display carries.
fn diff_section(display: &Display) -> Result<&DisplaySection> {
    let [section] = display.sections.as_slice() else {
        return Err("a diff display carries exactly one section".into());
    };
    Ok(section)
}

/// The section shape a diff over `a.rs` must have, so a card and a completion
/// body are compared as data and not as rendered text.
fn diff_section_of(diff: &str) -> DisplaySection {
    DisplaySection {
        label: None,
        kind: ContentKind::Diff {
            language: "diff".to_string(),
            path: "a.rs".to_string(),
        },
        content: diff.to_string(),
        stats: None,
    }
}

/// The live lane: each call starts, its gate record attaches the card, and its
/// completion lands.
fn live_fold(edit_result_text: &str) -> AppState {
    let mut live = AppState::default();
    fold(&mut live, &started("edit", EDIT_ARGS));
    fold(&mut live, &gate("edit", EDIT_ARGS, Some(EDIT_DIFF)));
    fold(&mut live, &started("nu", NU_ARGS));
    fold(&mut live, &gate("nu", NU_ARGS, None));
    fold(&mut live, &completed("edit", EDIT_ARGS, edit_result_text));
    fold(&mut live, &completed("nu", NU_ARGS, "alpha\nbeta"));
    live
}

/// The replay lane: a switch opens the window, the replayed log folds its
/// started and completed records, and the first live turn closes the window.
fn replayed_fold(edit_result_text: &str) -> AppState {
    let mut replayed = AppState::default();
    fold(&mut replayed, &switched());
    fold(&mut replayed, &started("edit", EDIT_ARGS));
    fold(
        &mut replayed,
        &completed("edit", EDIT_ARGS, edit_result_text),
    );
    fold(&mut replayed, &started("nu", NU_ARGS));
    fold(&mut replayed, &completed("nu", NU_ARGS, "alpha\nbeta"));
    let turn = first_live_turn();
    reduce(&mut replayed, RenderInput::Lifecycle(&turn), None);
    replayed
}

// endregion: --- Support

// region:    --- Parity

/// WHEN a replayed `nu` call folds inside the replay window, THE row SHALL carry
/// the card its live fold carries, byte for byte: the command is the call's own
/// arguments, so the derivation loses nothing on the way through the store.
#[test]
fn test_state_hydration_replay_preview_parity_for_nu() -> Result<()> {
    // -- Setup & Fixtures
    let edit_result_text = edit_result("a.rs", EDIT_DIFF);

    // -- Exec
    let live = live_fold(&edit_result_text);
    let replayed = replayed_fold(&edit_result_text);

    // -- Check: the same card, byte for byte — not merely two non-empty slots.
    let live_nu = preview_of(&live, "nu")?.ok_or("should have the live nu card")?;
    let replayed_nu = preview_of(&replayed, "nu")?.ok_or("should have the replayed nu card")?;
    assert_eq!(replayed_nu, live_nu, "the nu card replays byte for byte");
    assert!(
        display_text(&replayed_nu).contains("ls | select name"),
        "the card carries the command, so the parity is of real content"
    );

    // -- Check: both rows settle at the persisted verdict. A `nu` call draws no
    // completion body, so the card is all the row shows.
    assert_eq!(tool_row(&replayed, "nu")?.status, ItemStatus::Done);
    assert_eq!(tool_row(&live, "nu")?.status, ItemStatus::Done);
    assert!(
        tool_row(&replayed, "nu")?.result.is_none(),
        "a nu call draws no completion body"
    );
    Ok(())
}

/// WHEN a replayed `edit` call folds, THE live row SHALL show the diff the
/// producer planned and THE replayed row SHALL derive no card: the planned diff
/// is not in the persisted arguments, and the drawer never fabricates one. Both
/// rows SHALL settle with the completion body the persisted result carries.
#[test]
fn test_state_hydration_replay_preview_edit_card_is_producer_content() -> Result<()> {
    // -- Setup & Fixtures
    let edit_result_text = edit_result("a.rs", EDIT_DIFF);

    // -- Exec
    let live = live_fold(&edit_result_text);
    let replayed = replayed_fold(&edit_result_text);

    // -- Check: the live card is the planned diff, drawn over the call's path.
    let live_edit = preview_of(&live, "edit")?.ok_or("should have the live edit card")?;
    assert_eq!(
        diff_section(&live_edit)?,
        &diff_section_of(EDIT_DIFF),
        "the live card carries the planned diff"
    );

    // -- Check: the replayed row derives none — the data is not in the store,
    // and a derivation from the call alone has nothing to draw.
    assert_eq!(
        call("edit", EDIT_ARGS).preview_body(None),
        None,
        "an edit card needs the producer's content"
    );
    assert_eq!(
        preview_of(&replayed, "edit")?,
        None,
        "the replayed edit row carries no card"
    );

    // -- Check: both rows settle with the same completion body, derived from the
    // persisted result the store kept.
    for (label, state) in [("live", &live), ("replayed", &replayed)] {
        let row = tool_row(state, "edit")?;
        assert_eq!(
            row.status,
            ItemStatus::Done,
            "{label}: the persisted verdict"
        );
        let body = row
            .result
            .as_ref()
            .ok_or("should have the completion body")?;
        assert_eq!(
            diff_section(body)?,
            &diff_section_of(EDIT_DIFF),
            "{label}: the completion body carries the applied diff"
        );
    }
    Ok(())
}

/// WHEN a call folds after the replay window has closed, THE row SHALL carry no
/// derived card: outside the window the gate record is a card's only source.
#[test]
fn test_state_hydration_replay_preview_stops_at_the_window_boundary() -> Result<()> {
    // -- Setup & Fixtures: a replay window, opened and then closed.
    let mut state = AppState::default();
    fold(&mut state, &switched());
    fold(&mut state, &started("nu", NU_ARGS));
    let turn = first_live_turn();
    reduce(&mut state, RenderInput::Lifecycle(&turn), None);

    // -- Exec: a call folds after the first live turn started.
    fold(&mut state, &started("nu", OUT_OF_WINDOW_ARGS));

    // -- Check: the in-window call derived its card, and the out-of-window call
    // derived none — the same call shape, differing only by the window.
    assert!(
        preview_of(&state, "nu")?.is_some(),
        "the in-window call derives its card"
    );
    let outside = state
        .transcript
        .entries()
        .iter()
        .find_map(|entry| match entry {
            TranscriptEntry::Tool(tool) if tool.call.arguments == OUT_OF_WINDOW_ARGS => Some(tool),
            _ => None,
        })
        .ok_or("should have the out-of-window row")?;
    assert!(
        outside.preview.is_none(),
        "a call outside the window derives no card"
    );

    // -- Check: the fixture draws a card when the window is open, so the
    // boundary is what withheld it.
    assert!(
        call("nu", OUT_OF_WINDOW_ARGS).preview_body(None).is_some(),
        "the call draws a card when the window is open"
    );
    Ok(())
}

// endregion: --- Parity
