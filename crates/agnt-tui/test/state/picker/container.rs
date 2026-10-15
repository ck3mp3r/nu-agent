use super::*;

/// The picker container tracks the active picker: none by default, set on open,
/// cleared on close, and opening one picker closes any other.
#[test]
fn picker_container_active_lifecycle() -> Result<()> {
    // -- Setup & Fixtures
    let container = PickerContainer::default();

    // -- Exec & Check: none by default.
    assert_eq!(container.active(), None);

    // -- Exec & Check: open sets the active picker.
    let mut container = PickerContainer::default();
    container.open(ActivePicker::CommandPalette);
    assert_eq!(container.active(), Some(ActivePicker::CommandPalette));

    // -- Exec & Check: close clears the active picker.
    container.close();
    assert_eq!(container.active(), None);

    // -- Exec & Check: opening the model picker closes the command palette.
    let mut state = AppState::default();
    open_command_palette_for_test(&mut state);
    state.picker.open(ActivePicker::Model);
    assert_eq!(state.picker.render_kind(), Some(PickerRenderKind::Model));
    assert_eq!(state.picker.active(), Some(ActivePicker::Model));

    // -- Exec & Check: opening the command palette closes the inline slash.
    let mut state = AppState::default();
    state.check_inline_slash("/");
    open_command_palette_for_test(&mut state);
    assert_eq!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );
    assert_eq!(state.picker.active(), Some(ActivePicker::CommandPalette));
    Ok(())
}
