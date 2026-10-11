use super::*;

#[test]
fn picker_container_active_none_by_default() {
    let container = PickerContainer::default();
    assert_eq!(container.active(), None);
}

#[test]
fn picker_container_open_command_palette_sets_active() {
    let mut container = PickerContainer::default();
    container.open(ActivePicker::CommandPalette);
    assert_eq!(container.active(), Some(ActivePicker::CommandPalette));
}

#[test]
fn picker_container_close_command_palette_clears_active() {
    let mut container = PickerContainer::default();
    container.open(ActivePicker::CommandPalette);
    container.close();
    assert_eq!(container.active(), None);
}

#[test]
fn open_model_picker_closes_command_palette() {
    let mut state = AppState::default();
    open_command_palette_for_test(&mut state);
    state.picker.open(ActivePicker::Model);

    assert_eq!(state.picker.render_kind(), Some(PickerRenderKind::Model));
    assert_eq!(state.picker.active(), Some(ActivePicker::Model));
}

#[test]
fn open_command_palette_closes_inline_slash() {
    let mut state = AppState::default();
    state.check_inline_slash("/");
    open_command_palette_for_test(&mut state);

    assert_eq!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );
    assert_eq!(state.picker.active(), Some(ActivePicker::CommandPalette));
}
