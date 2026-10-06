use super::*;

#[test]
fn picker_state_new_has_defaults() {
    let state = PickerState::<PickerOption>::default();
    assert!(!state.open);
    assert_eq!(state.query, "");
    assert_eq!(state.selection, 0);
    assert!(state.options.is_empty());
}

#[test]
fn picker_state_open_resets_query_and_selection() {
    let mut state = PickerState::<PickerOption>::default();
    state.open();
    assert!(state.open);
    assert_eq!(state.query, "");
    assert_eq!(state.selection, 0);
}

#[test]
fn picker_state_close_resets_query_and_selection() {
    let mut state = PickerState::<PickerOption>::default();
    state.open();
    state.close();
    assert!(!state.open);
    assert_eq!(state.query, "");
    assert_eq!(state.selection, 0);
}

#[test]
fn picker_state_move_up_wraps() {
    let mut state = PickerState {
        open: true,
        query: String::new(),
        selection: 0,
        options: vec![picker_option("a"), picker_option("b"), picker_option("c")],
    };
    state.move_up();
    assert_eq!(state.selection, 2);
    state.move_up();
    assert_eq!(state.selection, 1);
}

#[test]
fn picker_state_move_down_wraps() {
    let mut state = PickerState {
        open: true,
        query: String::new(),
        selection: 0,
        options: vec![picker_option("a"), picker_option("b"), picker_option("c")],
    };
    state.move_down();
    assert_eq!(state.selection, 1);
    state.move_down();
    assert_eq!(state.selection, 2);
    state.move_down();
    assert_eq!(state.selection, 0);
}

#[test]
fn picker_state_move_up_empty_resets_selection() {
    let mut state = PickerState::<PickerOption>::default();
    state.open();
    state.move_up();
    assert_eq!(state.selection, 0);
}

#[test]
fn picker_state_append_query_char_resets_selection() {
    let mut state = PickerState {
        open: true,
        query: String::new(),
        selection: 2,
        options: vec![picker_option("a"), picker_option("b"), picker_option("c")],
    };
    state.append_query_char('a');
    assert_eq!(state.query, "a");
    assert_eq!(state.selection, 0);
}

#[test]
fn picker_state_backspace_query_char() {
    let mut state = PickerState::<PickerOption>::default();
    state.open();
    state.append_query_char('a');
    state.append_query_char('b');
    state.backspace_query_char();
    assert_eq!(state.query, "a");
}

#[test]
fn picker_state_clamp_selection() {
    let mut state = PickerState {
        open: true,
        query: String::new(),
        selection: 5,
        options: vec![picker_option("a"), picker_option("b"), picker_option("c")],
    };
    state.clamp_selection(3);
    assert_eq!(state.selection, 2);
    state.selection = 0;
    state.clamp_selection(0);
    assert_eq!(state.selection, 0);
}
