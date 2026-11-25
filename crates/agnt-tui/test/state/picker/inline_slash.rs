use super::*;
use crate::interaction::input::Key;
use agnt_runtime::bus::{CompactionAvailability, FactRecord};

/// A reason the runtime could ship for a gated /compact.
const REASON: &str = "the conversation ends with its most recent compaction.";

/// Inline slash suggestions open on a leading slash with the canonical command
/// list, filter incrementally as the prefix grows, close when the prefix is
/// removed, and never open the command palette.
#[test]
fn inline_slash_suggestions_lifecycle() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();

    // -- Exec: a leading slash opens the inline slash picker.
    state.check_inline_slash("/");

    // -- Check: the canonical command list, selection at 0, not the palette.
    assert_eq!(
        state.picker.render_kind(),
        Some(PickerRenderKind::InlineSlash)
    );
    assert_ne!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );
    let s = state.picker.active_state().ok_or("picker should be open")?;
    assert_eq!(s.selection, 0);
    let ids: Vec<String> = s.options.iter().map(|o| o.id.clone()).collect();
    assert_eq!(
        ids,
        vec![
            "/compact",
            "/uncompact",
            "/mcp",
            "/help",
            "/stats",
            "/status",
            "/models",
            "/agent",
            "/new",
            "/session",
            "/theme",
            "/skills",
        ]
    );

    // -- Exec & Check: the suggestions filter incrementally.
    state.check_inline_slash("/c");
    let ids: Vec<String> = state
        .picker
        .active_state()
        .ok_or("picker should be open")?
        .options
        .iter()
        .map(|o| o.id.clone())
        .collect();
    assert_eq!(ids, vec!["/compact"]);

    state.check_inline_slash("/co");
    let ids: Vec<String> = state
        .picker
        .active_state()
        .ok_or("picker should be open")?
        .options
        .iter()
        .map(|o| o.id.clone())
        .collect();
    assert_eq!(ids, vec!["/compact"]);

    // -- Exec & Check: removing the prefix closes the picker.
    state.check_inline_slash("");
    assert_eq!(state.picker.render_kind(), None);
    Ok(())
}

/// WHEN `check_inline_slash` replaces the options, THE InlineSlash picker SHALL
/// reset its selection to 0: the highlight follows the narrowed list, never the
/// index the previous keystroke left behind.
#[test]
fn test_state_picker_inline_slash_selection_resets() -> Result<()> {
    // -- Setup & Fixtures: the full command list, both commands offered, selection
    // moved onto /help.
    let mut state = AppState::default();
    assert!(
        state
            .status
            .reduce_fact_record(FactRecord::CommandAvailability(
                crate::test_support::both_commands_available()
            ))
    );
    state.check_inline_slash("/");
    for _ in 0..3 {
        state
            .picker
            .active_state_mut()
            .ok_or("picker should be open")?
            .move_down();
    }
    assert_eq!(
        state
            .picker
            .active_state()
            .ok_or("picker should be open")?
            .selection,
        3,
        "three Down presses reach /help"
    );

    // -- Exec: a later keystroke replaces the options with the narrowed list.
    state.check_inline_slash("/s");

    // -- Check: the selection restarts at the first narrowed entry.
    let s = state.picker.active_state().ok_or("picker should be open")?;
    assert_eq!(s.selection, 0, "replacing the options resets the selection");
    assert_eq!(
        s.options.first().map(|o| o.id.as_str()),
        Some("/stats"),
        "the narrowed list starts at /stats"
    );

    // -- Exec: moving down again, then re-listing the full command set.
    state
        .picker
        .active_state_mut()
        .ok_or("picker should be open")?
        .move_down();
    state.check_inline_slash("/");

    // -- Check: a re-list resets the selection too.
    let s = state.picker.active_state().ok_or("picker should be open")?;
    assert_eq!(
        s.selection, 0,
        "re-listing the command set resets the selection"
    );
    assert_eq!(s.options.first().map(|o| o.id.as_str()), Some("/compact"));
    Ok(())
}

/// WHEN a command in the inline-slash list cannot run, THE line moves SHALL step
/// over its row: the walk lands on the next option that can act, wraps in both
/// directions, and a list with nothing selectable keeps the selection still.
#[test]
fn test_state_picker_inline_slash_moves_skip_unavailable_rows() -> Result<()> {
    // -- Setup & Fixtures: /compact is gated and the list starts on it, while the
    // undo still has work — only the sweep's row is skipped.
    let mut state = AppState::default();
    state.status.command_availability.compact =
        CompactionAvailability::Unavailable(REASON.to_string());
    state.status.command_availability.uncompact = CompactionAvailability::Available;
    state.check_inline_slash("/");
    state
        .picker
        .active_state_mut()
        .ok_or("picker should be open")?
        .selection = 1;

    // -- Exec: one step up from /uncompact, whose neighbour is the gated row.
    state.picker.handle_key(Key::Up);

    // -- Check: the walk skips /compact and wraps onto the last option.
    assert_eq!(
        inline_slash_selection(&state)?,
        11,
        "the walk skips the gated row and wraps"
    );

    // -- Exec & Check: one step down from the last option wraps to the first
    // option that can act.
    state.picker.handle_key(Key::Down);
    assert_eq!(
        inline_slash_selection(&state)?,
        1,
        "the walk skips the gated row forward"
    );

    // -- Setup & Fixtures: the narrowed list holds the gated row alone.
    state.check_inline_slash("/c");
    assert_eq!(inline_slash_selection(&state)?, 0);

    // -- Exec & Check: nothing is selectable, so both moves keep the selection.
    state.picker.handle_key(Key::Down);
    assert_eq!(
        inline_slash_selection(&state)?,
        0,
        "an all-unavailable list keeps its selection"
    );
    state.picker.handle_key(Key::Up);
    assert_eq!(
        inline_slash_selection(&state)?,
        0,
        "an all-unavailable list keeps its selection"
    );
    Ok(())
}

/// WHEN a query narrows the list onto a row that cannot run, THE accept SHALL do
/// nothing: the picker keeps its place, the narrowed rows stay as they were, and
/// nothing submits. The machinery reads a row's own answer, never a command's
/// name.
///
/// Delta from the shipped shape: the accept on an all-dim view used to close the
/// popup. The ruling "does nothing" applies uniformly, so the close is gone and
/// the picker stays open.
#[test]
fn test_state_picker_inline_slash_accept_on_a_narrowed_dim_row_does_nothing() -> Result<()> {
    // -- Setup & Fixtures: `/uncompact` is gated, and `/u` narrows the list to it.
    let mut state = AppState::default();
    state.status.command_availability.uncompact =
        CompactionAvailability::Unavailable(REASON.to_string());
    state.check_inline_slash("/u");
    assert_eq!(inline_slash_selection(&state)?, 0);

    // -- Exec: the accept lands on the one row, which cannot act.
    let outcome = state.picker.handle_key(Key::Enter);

    // -- Check: the key is consumed, the popup keeps its place, the row stays.
    assert!(
        matches!(outcome, PickerKeyOutcome::Consumed(true)),
        "the accept consumes the key instead of submitting"
    );
    assert_eq!(
        state.picker.render_kind(),
        Some(PickerRenderKind::InlineSlash),
        "the popup stays open"
    );
    let rows: Vec<String> = state
        .picker
        .active_state()
        .ok_or("picker should be open")?
        .filtered()
        .iter()
        .map(|o| o.id.clone())
        .collect();
    assert_eq!(rows, vec!["/uncompact"], "the narrowed rows stay");
    assert_eq!(
        state.picker.selected_slash_label().as_deref(),
        Some("/uncompact"),
        "the selection keeps the dim row"
    );

    // -- Exec: the same key through the reducer.
    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);

    // -- Check: nothing is submitted and nothing is queued.
    assert!(changed, "the accept is consumed");
    assert_eq!(
        state.take_next_prompt_for_execution(),
        None,
        "nothing is submitted"
    );
    assert_eq!(state.pending_prompt_count(), 0, "nothing is queued");
    Ok(())
}

/// WHEN Enter/Tab lands on a DIM (unavailable) slash row, THE accept SHALL do
/// nothing: the picker stays open, the selection stays on that row, nothing is
/// submitted or queued, and the nearest selectable neighbour never fires.
#[test]
fn test_state_picker_inline_slash_accept_on_a_dim_row_does_nothing() -> Result<()> {
    // -- Setup & Fixtures: the full list open on the gated /compact row, with the
    // undo offered as the next row that can act — the row a neighbour walk would
    // have submitted.
    let mut state = AppState::default();
    state.status.command_availability.compact =
        CompactionAvailability::Unavailable(REASON.to_string());
    state.status.command_availability.uncompact = CompactionAvailability::Available;
    state.check_inline_slash("/");
    assert_eq!(inline_slash_selection(&state)?, 0);
    assert_eq!(
        state.picker.selected_slash_label().as_deref(),
        Some("/compact"),
        "the cursor sits on the dim row"
    );

    // -- Exec: the picker's own accept.
    let outcome = state.picker.handle_key(Key::Enter);

    // -- Check: the key is consumed, nothing is submitted, and the cursor stays.
    assert!(
        matches!(outcome, PickerKeyOutcome::Consumed(true)),
        "the accept on a dim row consumes the key instead of submitting"
    );
    assert_eq!(
        state.picker.render_kind(),
        Some(PickerRenderKind::InlineSlash),
        "the picker stays open"
    );
    assert_eq!(
        inline_slash_selection(&state)?,
        0,
        "the selection stays on the dim row"
    );
    assert_eq!(
        state.picker.selected_slash_label().as_deref(),
        Some("/compact"),
        "the neighbour command never fires"
    );

    // -- Exec: the same key through the reducer.
    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);

    // -- Check: nothing is queued, no notice appears, and the row stays the dim one.
    assert!(changed, "the accept is consumed");
    assert_eq!(
        state.picker.render_kind(),
        Some(PickerRenderKind::InlineSlash),
        "the picker stays open"
    );
    assert_eq!(
        state.picker.selected_slash_label().as_deref(),
        Some("/compact"),
        "the neighbour command never fires"
    );
    assert_eq!(
        state.take_next_prompt_for_execution(),
        None,
        "nothing is submitted"
    );
    assert_eq!(state.pending_prompt_count(), 0, "nothing is queued");
    assert!(
        state.status.message.status_line().is_empty(),
        "no notice appears"
    );
    Ok(())
}

/// WHEN every offered command cannot run, THE accept SHALL submit nothing and
/// SHALL keep the popup open: the typed text stays in the input.
///
/// Delta from the shipped shape: this view used to close the popup on Enter. The
/// ruling "does nothing" applies uniformly, so the close is gone — the picker
/// stays open and the user's text is untouched.
#[test]
fn test_state_picker_inline_slash_accept_on_an_all_dim_list_does_nothing() -> Result<()> {
    // -- Setup & Fixtures: `/c` narrows the list onto the gated /compact row, so
    // every offered row is dim.
    let mut state = AppState::default();
    state.status.command_availability.compact =
        CompactionAvailability::Unavailable(REASON.to_string());
    state.check_inline_slash("/c");
    assert!(
        state
            .picker
            .active_state()
            .ok_or("picker should be open")?
            .filtered()
            .iter()
            .all(|o| !o.selectable()),
        "every offered row cannot act"
    );

    // -- Exec
    let outcome = state.picker.handle_key(Key::Tab);

    // -- Check: the key is consumed, nothing is submitted, and the popup stays.
    assert!(
        matches!(outcome, PickerKeyOutcome::Consumed(true)),
        "the accept consumes the key instead of submitting"
    );
    assert_eq!(
        state.picker.render_kind(),
        Some(PickerRenderKind::InlineSlash),
        "the popup stays open"
    );
    assert_eq!(
        state.picker.selected_slash_label().as_deref(),
        Some("/compact"),
        "the selection keeps the dim row"
    );

    // -- Exec: the same key through the reducer.
    let changed = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Tab), None);

    // -- Check: nothing is submitted and nothing is queued.
    assert!(changed, "the accept is consumed");
    assert_eq!(
        state.take_next_prompt_for_execution(),
        None,
        "nothing is submitted"
    );
    assert_eq!(state.pending_prompt_count(), 0, "nothing is queued");
    Ok(())
}

/// WHEN Enter/Tab lands on an AVAILABLE slash row, THE accept SHALL submit it
/// exactly as before: a dim row elsewhere in the list changes nothing.
#[test]
fn test_state_picker_inline_slash_accept_on_an_available_row_submits() -> Result<()> {
    // -- Setup & Fixtures: the full list with the gated /compact row first and the
    // undo offered next, walked one step down onto the row that can act.
    let mut state = AppState::default();
    state.status.command_availability.compact =
        CompactionAvailability::Unavailable(REASON.to_string());
    state.status.command_availability.uncompact = CompactionAvailability::Available;
    state.check_inline_slash("/");
    state.picker.handle_key(Key::Down);
    assert_eq!(inline_slash_selection(&state)?, 1);

    // -- Exec
    let outcome = state.picker.handle_key(Key::Enter);

    // -- Check: the row under the cursor is the one accepted, and it is submitted.
    assert!(
        matches!(outcome, PickerKeyOutcome::Submit(SubmitAction::SlashAccept)),
        "an available row still submits"
    );
    assert_eq!(
        state.picker.selected_slash_label().as_deref(),
        Some("/uncompact"),
        "the accepted label is the row under the cursor"
    );
    Ok(())
}

// region:    --- Test Support

fn inline_slash_selection(state: &AppState) -> Result<usize> {
    Ok(state
        .picker
        .active_state()
        .ok_or("picker should be open")?
        .selection)
}

// endregion: --- Test Support
