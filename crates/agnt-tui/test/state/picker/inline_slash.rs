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
