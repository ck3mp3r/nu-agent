use super::*;

/// `PickerState` defaults, open/close reset, move up/down wrapping, query
/// editing, and selection clamping.
#[test]
fn picker_state_lifecycle() -> Result<()> {
    // -- Setup & Fixtures
    let state = PickerState::<PickerOption>::default();

    // -- Exec & Check: defaults.
    assert!(!state.open);
    assert_eq!(state.query, "");
    assert_eq!(state.selection, 0);
    assert!(state.options.is_empty());

    // -- Exec & Check: open resets query and selection.
    let mut state = PickerState::<PickerOption>::default();
    state.open();
    assert!(state.open);
    assert_eq!(state.query, "");
    assert_eq!(state.selection, 0);

    // -- Exec & Check: close resets query and selection.
    state.close();
    assert!(!state.open);
    assert_eq!(state.query, "");
    assert_eq!(state.selection, 0);

    // -- Setup & Fixtures: three options.
    let options = || vec![picker_option("a"), picker_option("b"), picker_option("c")];

    // -- Exec & Check: move up wraps.
    let mut up = PickerState {
        open: true,
        query: String::new(),
        selection: 0,
        options: options(),
    };
    up.move_up();
    assert_eq!(up.selection, 2);
    up.move_up();
    assert_eq!(up.selection, 1);

    // -- Exec & Check: move down wraps.
    let mut down = PickerState {
        open: true,
        query: String::new(),
        selection: 0,
        options: options(),
    };
    down.move_down();
    assert_eq!(down.selection, 1);
    down.move_down();
    assert_eq!(down.selection, 2);
    down.move_down();
    assert_eq!(down.selection, 0);

    // -- Exec & Check: move up on an empty list resets the selection.
    let mut empty = PickerState::<PickerOption>::default();
    empty.open();
    empty.move_up();
    assert_eq!(empty.selection, 0);

    // -- Exec & Check: appending a query char resets the selection.
    let mut query = PickerState {
        open: true,
        query: String::new(),
        selection: 2,
        options: options(),
    };
    query.append_query_char('a');
    assert_eq!(query.query, "a");
    assert_eq!(query.selection, 0);

    // -- Exec & Check: backspace removes the last query char.
    let mut backspace = PickerState::<PickerOption>::default();
    backspace.open();
    backspace.append_query_char('a');
    backspace.append_query_char('b');
    backspace.backspace_query_char();
    assert_eq!(backspace.query, "a");

    // -- Exec & Check: clamp_selection clamps to the option count.
    let mut clamp = PickerState {
        open: true,
        query: String::new(),
        selection: 5,
        options: options(),
    };
    clamp.clamp_selection(3);
    assert_eq!(clamp.selection, 2);
    clamp.selection = 0;
    clamp.clamp_selection(0);
    assert_eq!(clamp.selection, 0);
    Ok(())
}
