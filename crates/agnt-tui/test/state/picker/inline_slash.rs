use super::*;

#[test]
fn inline_slash_suggestions_open_on_leading_slash() {
    let mut state = AppState::default();

    state.check_inline_slash("/");

    assert_eq!(
        state.picker.render_kind(),
        Some(PickerRenderKind::InlineSlash)
    );
    let s = state.picker.active_state().unwrap();
    assert_eq!(s.selection, 0);
    let ids: Vec<String> = s.options.iter().map(|o| o.id.clone()).collect();
    assert_eq!(
        ids,
        vec![
            "/compact", "/mcp", "/help", "/status", "/models", "/agent", "/new", "/session",
            "/theme", "/skills",
        ]
    );
}

#[test]
fn inline_slash_suggestions_filter_incrementally_as_input_grows() {
    let mut state = AppState::default();

    state.check_inline_slash("/");
    assert_eq!(state.picker.active_state().unwrap().options.len(), 10);

    state.check_inline_slash("/c");
    let ids: Vec<String> = state
        .picker
        .active_state()
        .unwrap()
        .options
        .iter()
        .map(|o| o.id.clone())
        .collect();
    assert_eq!(ids, vec!["/compact"]);

    state.check_inline_slash("/co");
    let ids: Vec<String> = state
        .picker
        .active_state()
        .unwrap()
        .options
        .iter()
        .map(|o| o.id.clone())
        .collect();
    assert_eq!(ids, vec!["/compact"]);
}

#[test]
fn inline_slash_suggestions_close_when_prefix_removed() {
    let mut state = AppState::default();

    state.check_inline_slash("/c");
    assert_eq!(
        state.picker.render_kind(),
        Some(PickerRenderKind::InlineSlash)
    );

    state.check_inline_slash("");

    assert_eq!(state.picker.render_kind(), None);
}

#[test]
fn inline_slash_suggestions_do_not_open_command_palette() {
    let mut state = AppState::default();

    state.check_inline_slash("/");

    assert_eq!(
        state.picker.render_kind(),
        Some(PickerRenderKind::InlineSlash)
    );
    assert_ne!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );
}
