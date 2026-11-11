use super::*;

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
            "/compact", "/mcp", "/help", "/status", "/models", "/agent", "/new", "/session",
            "/theme", "/skills",
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
    // -- Setup & Fixtures: the full command list, selection moved onto /status.
    let mut state = AppState::default();
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
        "three Down presses reach /status"
    );

    // -- Exec: a later keystroke replaces the options with the narrowed list.
    state.check_inline_slash("/s");

    // -- Check: the selection restarts at the first narrowed entry.
    let s = state.picker.active_state().ok_or("picker should be open")?;
    assert_eq!(s.selection, 0, "replacing the options resets the selection");
    assert_eq!(
        s.options.first().map(|o| o.id.as_str()),
        Some("/status"),
        "the narrowed list starts at /status"
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
