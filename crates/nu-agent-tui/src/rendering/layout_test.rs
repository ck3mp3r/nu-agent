use crate::rendering::layout::{
    input_content_row_count, input_cursor_row_col, input_pane_height_for_content,
    wrapped_input_rows,
};

#[test]
fn input_height_grows_with_newlines_and_wrap_and_is_clamped() {
    let h_short = input_pane_height_for_content("x", 80);
    assert_eq!(h_short, 1);

    let h_multiline = input_pane_height_for_content("a\nb\nc", 80);
    assert!(h_multiline > h_short);

    let h_wrapped = input_pane_height_for_content("abcdefghij", 4);
    assert!(h_wrapped > h_short);

    let h_clamped = input_pane_height_for_content(&"x".repeat(300), 4);
    assert_eq!(h_clamped, 6);
}

#[test]
fn wrapped_rows_and_cursor_mapping_handle_mixed_newline_and_wrap() {
    let rows = wrapped_input_rows("ab\n12345", 3);
    assert_eq!(
        rows,
        vec!["ab".to_string(), "123".to_string(), "45".to_string()]
    );
    assert_eq!(input_content_row_count("ab\n12345", 3), 3);

    assert_eq!(input_cursor_row_col("ab\n12345", 0, 3), (0, 0));
    assert_eq!(input_cursor_row_col("ab\n12345", 2, 3), (0, 2));
    assert_eq!(input_cursor_row_col("ab\n12345", 3, 3), (1, 0));
    assert_eq!(input_cursor_row_col("ab\n12345", 6, 3), (2, 0));
    assert_eq!(input_cursor_row_col("ab\n12345", 8, 3), (2, 2));
}

#[test]
fn input_pane_height_is_content_rows_clamped_to_min() {
    let h = input_pane_height_for_content("hello", 80);
    assert_eq!(h, 1);
}

#[test]
fn input_pane_min_height_is_one() {
    let h = input_pane_height_for_content("", 80);
    assert_eq!(h, 1);
}
