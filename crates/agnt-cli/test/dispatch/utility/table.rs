use super::*;

#[test]
fn test_utility_table_format_row_pads_columns_and_trims_trailing() {
    // -- Setup & Fixtures
    let cells = vec!["a".to_string(), "bb".to_string()];
    let widths = [3usize, 4usize];

    // -- Exec
    let row = format_row(&cells, &widths);

    // -- Check
    assert_eq!(row, "a    bb");
}

#[test]
fn test_utility_table_format_row_without_widths_keeps_cell_text() {
    // -- Setup & Fixtures
    let cells = vec!["x".to_string()];

    // -- Exec
    let row = format_row(&cells, &[]);

    // -- Check
    assert_eq!(row, "x");
}
