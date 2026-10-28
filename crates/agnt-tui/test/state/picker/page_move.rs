use super::*;
use crate::interaction::reducer::UserAction;

/// PgUp/PgDn page the selection 8 rows in every picker kind, clamped to the
/// first and last row instead of wrapping; single-row moves stay one row.
#[test]
fn picker_page_moves_clamp_in_every_kind() -> Result<()> {
    // -- Setup & Fixtures
    let kinds = [
        ActivePicker::CommandPalette,
        ActivePicker::Model,
        ActivePicker::Agent,
        ActivePicker::Session,
        ActivePicker::Theme,
        ActivePicker::InlineSlash,
    ];
    let many = || {
        (0..20)
            .map(|idx| picker_option(&format!("opt-{idx:02}")))
            .collect::<Vec<_>>()
    };

    for kind in kinds {
        // -- Setup & Fixtures: a fresh picker with twenty rows.
        let mut state = AppState::default();
        state.set_picker_options(kind, many());
        state.picker.open(kind);

        // -- Exec & Check: pages step eight rows down from the top.
        let (rewritten, changed) = state.picker.handle_action(UserAction::ScrollPageDown);
        assert_eq!(rewritten, UserAction::Noop, "{kind:?}");
        assert!(changed, "{kind:?}");
        assert_eq!(picker_selection(&state)?, 8, "{kind:?}");

        state.picker.handle_action(UserAction::ScrollPageDown);
        assert_eq!(picker_selection(&state)?, 16, "{kind:?}");

        // -- Exec & Check: the last page clamps to the last row, never past it.
        state.picker.handle_action(UserAction::ScrollPageDown);
        assert_eq!(picker_selection(&state)?, 19, "{kind:?}");
        state.picker.handle_action(UserAction::ScrollPageDown);
        assert_eq!(picker_selection(&state)?, 19, "{kind:?}");

        // -- Exec & Check: pages walk back up and clamp at the first row.
        state.picker.handle_action(UserAction::ScrollPageUp);
        assert_eq!(picker_selection(&state)?, 11, "{kind:?}");
        state.picker.handle_action(UserAction::ScrollPageUp);
        assert_eq!(picker_selection(&state)?, 3, "{kind:?}");
        state.picker.handle_action(UserAction::ScrollPageUp);
        assert_eq!(picker_selection(&state)?, 0, "{kind:?}");
        state.picker.handle_action(UserAction::ScrollPageUp);
        assert_eq!(picker_selection(&state)?, 0, "{kind:?}");

        // -- Exec & Check: a line move still moves exactly one row. The inline
        // slash popup routes arrows through its own History* arms, so the check
        // stays on the query-picker kinds.
        if kind != ActivePicker::InlineSlash {
            state.picker.handle_action(UserAction::ScrollLineDown);
            assert_eq!(picker_selection(&state)?, 1, "{kind:?}");
        }
    }
    Ok(())
}

/// A page never overshoots a short list, an empty list, or the filtered view's
/// bounds.
#[test]
fn picker_page_moves_clamp_on_short_and_filtered_lists() -> Result<()> {
    // -- Setup & Fixtures: three rows.
    let mut state = AppState::default();
    state.set_picker_options(
        ActivePicker::Model,
        (0..3)
            .map(|idx| picker_option(&format!("opt-{idx}")))
            .collect::<Vec<_>>(),
    );
    state.picker.open(ActivePicker::Model);

    // -- Exec & Check: fewer than eight rows clamps to the last row.
    state.picker.handle_action(UserAction::ScrollPageDown);
    assert_eq!(picker_selection(&state)?, 2);
    state.picker.handle_action(UserAction::ScrollPageDown);
    assert_eq!(picker_selection(&state)?, 2);
    state.picker.handle_action(UserAction::ScrollPageUp);
    assert_eq!(picker_selection(&state)?, 0);
    state.picker.handle_action(UserAction::ScrollPageUp);
    assert_eq!(picker_selection(&state)?, 0);

    // -- Setup & Fixtures: an empty list.
    let mut empty = AppState::default();
    empty.set_picker_options(ActivePicker::Agent, Vec::<PickerOption>::new());
    empty.picker.open(ActivePicker::Agent);

    // -- Exec & Check: pages stay at the top of an empty list.
    empty.picker.handle_action(UserAction::ScrollPageDown);
    assert_eq!(picker_selection(&empty)?, 0);

    // -- Setup & Fixtures: twenty rows narrowed to one by a query.
    let mut filtered = AppState::default();
    filtered.set_picker_options(
        ActivePicker::Theme,
        (0..20)
            .map(|idx| picker_option(&format!("opt-{idx:02}")))
            .collect::<Vec<_>>(),
    );
    filtered.picker.open(ActivePicker::Theme);
    type_query(&mut filtered, "opt-19")?;

    // -- Exec & Check: the page clamps to the single filtered row.
    filtered.picker.handle_action(UserAction::ScrollPageDown);
    assert_eq!(picker_selection(&filtered)?, 0);

    // -- Exec & Check: widening the query restores the full page step.
    for _ in 0..6 {
        filtered
            .picker
            .active_state_mut()
            .ok_or("picker should be open")?
            .backspace_query_char();
    }
    filtered.picker.handle_action(UserAction::ScrollPageDown);
    assert_eq!(picker_selection(&filtered)?, 8);
    Ok(())
}

/// The inline-slash popup moves its selection on the same line-move action set as
/// every other picker: `ScrollLineUp`/`ToggleCommandPalette` up,
/// `ScrollLineDown`/`QueryNext` down. None of them leak to the transcript.
#[test]
fn inline_slash_line_moves_share_the_picker_action_set() -> Result<()> {
    // -- Setup & Fixtures: the canonical slash list (`/compact` … `/skills`).
    let mut state = AppState::default();
    state.check_inline_slash("/");
    assert_eq!(
        state
            .picker
            .active_state()
            .ok_or("picker should be open")?
            .options
            .len(),
        10,
        "the canonical slash list"
    );

    // -- Exec & Check: each line-move action moves one row and is consumed.
    for (action, expected) in [
        (UserAction::ScrollLineDown, 1),
        (UserAction::QueryNext, 2),
        (UserAction::ScrollLineUp, 1),
        (UserAction::ToggleCommandPalette, 0),
        (UserAction::HistoryDown, 1),
        (UserAction::HistoryUp, 0),
    ] {
        let (rewritten, changed) = state.picker.handle_action(action.clone());
        assert_eq!(rewritten, UserAction::Noop, "{action:?} must not leak");
        assert!(changed, "{action:?} must repaint");
        assert_eq!(picker_selection(&state)?, expected, "{action:?}");
    }

    // -- Check: the page moves the page-scroll task pinned still work.
    let (rewritten, _) = state.picker.handle_action(UserAction::ScrollPageDown);
    assert_eq!(rewritten, UserAction::Noop);
    assert_eq!(picker_selection(&state)?, 8, "eight rows down");
    state.picker.handle_action(UserAction::ScrollPageUp);
    assert_eq!(picker_selection(&state)?, 0, "eight rows back up");
    Ok(())
}

// region:    --- Test Support

fn picker_selection(state: &AppState) -> Result<usize> {
    Ok(state
        .picker
        .active_state()
        .ok_or("picker should be open")?
        .selection)
}

// endregion: --- Test Support
