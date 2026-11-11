use super::*;
use unicode_width::UnicodeWidthStr;

/// The defect fixture: `abcd` makes the first column 4 display columns wide and
/// the emoji cell makes its widest single char 2 wide. At a budget of 9 the
/// pre-floor algorithm shrank the column to 1 and let the emoji overflow.
const EMOJI_TABLE: &str = "| abcd | e |\n| --- | --- |\n| 😀 | x |";

/// WHEN a column would shrink below its cells' widest single char, THE
/// render_table function SHALL floor it there, so every physical row stays at
/// the border width instead of overflowing it.
#[test]
fn markdown_projection_table_shrink_floors_at_max_char_width_table() -> Result<()> {
    // -- Setup & Fixtures: natural widths [4, 1] (table width 12) shrunk towards
    // a budget of 9. The first column floors at 2 (the emoji's display width).
    let lines = project_markdown_to_lines(EMOJI_TABLE, Some(9));
    let plain: Vec<String> = lines.iter().map(plain_line).collect();

    // -- Exec
    let top = plain.first().ok_or("table should have a top border")?;
    let border_width = UnicodeWidthStr::width(top.as_str());

    // -- Check: the floors [2, 1] leave the table at its structural minimum,
    // 1 + 3*2 + 3 = 10, one display column over the budget.
    assert_eq!(
        border_width, 10,
        "floors [2, 1] put the table at its structural minimum 10: {plain:?}"
    );

    // -- Check: every physical row matches the border width, emoji row included.
    for line in &plain {
        assert_eq!(
            UnicodeWidthStr::width(line.as_str()),
            border_width,
            "physical row must match its border width: {line:?}"
        );
    }

    // -- Check: the emoji keeps its glyph inside its own cell.
    assert!(
        plain.iter().any(|line| line.contains('😀')),
        "the emoji cell must keep its glyph: {plain:?}"
    );
    Ok(())
}

/// WHEN `max_width` is at least the structural minimum of the floors, THE
/// render_table function SHALL emit every line within `max_width`.
#[test]
fn markdown_projection_table_floored_table_fits_budget_table() -> Result<()> {
    // -- Setup & Fixtures: the same table at exactly its floored minimum 10.
    let lines = project_markdown_to_lines(EMOJI_TABLE, Some(10));
    let plain: Vec<String> = lines.iter().map(plain_line).collect();

    // -- Exec
    let top = plain.first().ok_or("table should have a top border")?;
    let border_width = UnicodeWidthStr::width(top.as_str());

    // -- Check: the table settles exactly on the budget.
    assert_eq!(
        border_width, 10,
        "a budget of 10 holds the floored minimum exactly: {plain:?}"
    );

    // -- Check: no line exceeds the budget.
    for line in &plain {
        assert!(
            UnicodeWidthStr::width(line.as_str()) <= 10,
            "line exceeds max_width=10: {line:?} (width={})",
            UnicodeWidthStr::width(line.as_str())
        );
    }
    Ok(())
}

/// WHEN `max_width` is `None`, THE render_table function SHALL CONTINUE TO
/// allocate the natural unshrunk widths, floors excluded.
#[test]
fn markdown_projection_table_without_budget_keeps_natural_widths_table() -> Result<()> {
    // -- Setup & Fixtures: no budget, so the natural widths [4, 1] apply.
    let lines = project_markdown_to_lines(EMOJI_TABLE, None);
    let plain: Vec<String> = lines.iter().map(plain_line).collect();

    // -- Exec
    let top = plain.first().ok_or("table should have a top border")?;
    let border_width = UnicodeWidthStr::width(top.as_str());

    // -- Check: natural widths [4, 1] give 1 + 3*2 + 5 = 12.
    assert_eq!(
        border_width, 12,
        "an unbudgeted table keeps its natural widths: {plain:?}"
    );
    for line in &plain {
        assert_eq!(
            UnicodeWidthStr::width(line.as_str()),
            border_width,
            "physical row must match its border width: {line:?}"
        );
    }
    Ok(())
}

/// WHEN every char in every cell is at most one display column wide, THE
/// render_table function SHALL CONTINUE TO shrink every column down to 1.
#[test]
fn markdown_projection_table_ascii_columns_still_shrink_to_one_table() -> Result<()> {
    // -- Setup & Fixtures: natural widths [5, 4, 5] and every floor 1, so a
    // budget of 3 below the structural minimum 13 drives all columns to 1.
    let markdown = "| alpha | beta | gamma |\n| --- | --- | --- |\n| a | b | c |";
    let lines = project_markdown_to_lines(markdown, Some(3));
    let plain: Vec<String> = lines.iter().map(plain_line).collect();

    // -- Exec
    let separator = plain
        .iter()
        .find(|line| line.contains('┼'))
        .ok_or("table should have a separator row")?;
    let segments: Vec<&str> = separator.split(['├', '┼', '┤']).collect();

    // -- Check: three 1-wide columns, each separator segment 3 cells wide.
    assert_eq!(
        segments.len(),
        5,
        "3 columns give 2 inner segments plus edges: {separator:?}"
    );
    for segment in &segments[1..4] {
        assert_eq!(
            UnicodeWidthStr::width(*segment),
            3,
            "every ASCII column shrinks to 1, so its segment is 3 wide: {separator:?}"
        );
    }

    // -- Check: the residual table renders at its structural minimum, every
    // physical row matching the border width.
    let top = plain.first().ok_or("table should have a top border")?;
    let border_width = UnicodeWidthStr::width(top.as_str());
    assert_eq!(
        border_width, 13,
        "structural minimum is 1 + 3*3 + 3: {plain:?}"
    );
    for line in &plain {
        assert_eq!(
            UnicodeWidthStr::width(line.as_str()),
            border_width,
            "physical row must match its border width: {line:?}"
        );
    }
    Ok(())
}

/// WHEN a column's cells are all empty, THE render_table function SHALL floor
/// that column at 1 instead of allocating it zero width.
#[test]
fn markdown_projection_table_empty_column_floors_at_one_table() -> Result<()> {
    // -- Setup & Fixtures: the second column holds no text, so its natural
    // width is 0.
    let markdown = "| A |  |\n| --- | --- |\n| 1 |  |";
    let lines = project_markdown_to_lines(markdown, Some(20));
    let plain: Vec<String> = lines.iter().map(plain_line).collect();

    // -- Exec
    let top = plain.first().ok_or("table should have a top border")?;
    let border_width = UnicodeWidthStr::width(top.as_str());

    // -- Check: both columns are at least 1 wide — 1 + 3*2 + 2 = 9.
    assert_eq!(
        border_width, 9,
        "an empty column renders 1 wide, not 0: {plain:?}"
    );
    assert_eq!(
        top.chars().filter(|c| *c == '┬').count(),
        1,
        "the empty column is still a column: {top:?}"
    );

    // -- Check: every physical row matches the border width.
    for line in &plain {
        assert_eq!(
            UnicodeWidthStr::width(line.as_str()),
            border_width,
            "physical row must match its border width: {line:?}"
        );
    }
    Ok(())
}
