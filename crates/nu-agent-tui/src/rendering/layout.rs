pub const SIDE_PANE_COLLAPSE_COLUMNS: u16 = 120;
pub const INPUT_PROMPT_WIDTH: u16 = 2;
pub const INPUT_MIN_HEIGHT: u16 = 1;
pub const INPUT_MAX_HEIGHT: u16 = 6;
pub const MAIN_SIDE_MARGIN: u16 = 2;

const MIN_MAIN_COLUMNS: u16 = 72;
const MIN_SIDE_COLUMNS: u16 = 24;

pub fn compute_columns(
    columns: u16,
    rows: u16,
    side_pane_visible: Option<bool>,
) -> (u16, Option<u16>) {
    if rows == 0 {
        return (0, None);
    }

    let prefers_side = side_pane_visible.unwrap_or(false);
    if !prefers_side || columns < SIDE_PANE_COLLAPSE_COLUMNS {
        return (columns, None);
    }

    if columns < MIN_MAIN_COLUMNS.saturating_add(MIN_SIDE_COLUMNS) {
        return (columns, None);
    }

    let max_side = columns.saturating_sub(MIN_MAIN_COLUMNS);
    let side_width = (columns / 3).clamp(MIN_SIDE_COLUMNS, max_side);
    let main_width = columns.saturating_sub(side_width);
    (main_width, Some(side_width))
}

pub fn wrapped_input_rows(input: &str, content_width: usize) -> Vec<String> {
    let width = content_width.max(1);
    let mut rows = Vec::new();

    for logical_line in input.split('\n') {
        if logical_line.is_empty() {
            rows.push(String::new());
            continue;
        }

        let mut current = String::new();
        let mut col = 0usize;
        for ch in logical_line.chars() {
            current.push(ch);
            col += 1;
            if col >= width {
                rows.push(current);
                current = String::new();
                col = 0;
            }
        }

        if !current.is_empty() {
            rows.push(current);
        }
    }

    if rows.is_empty() {
        rows.push(String::new());
    }

    rows
}

pub fn input_content_row_count(input: &str, content_width: usize) -> u16 {
    wrapped_input_rows(input, content_width)
        .len()
        .min(u16::MAX as usize) as u16
}

pub fn input_pane_height_for_content(input: &str, pane_width: u16) -> u16 {
    let content_width = pane_width.saturating_sub(4).max(1) as usize;
    let content_rows = input_content_row_count(input, content_width);
    content_rows.clamp(INPUT_MIN_HEIGHT, INPUT_MAX_HEIGHT)
}

pub fn input_cursor_row_col(input: &str, cursor: usize, content_width: usize) -> (u16, u16) {
    let width = content_width.max(1);
    let cursor = cursor.min(input.len());
    let mut row = 0usize;
    let mut col = 0usize;

    for ch in input[..cursor].chars() {
        if ch == '\n' {
            row += 1;
            col = 0;
            continue;
        }

        col += 1;
        if col >= width {
            row += 1;
            col = 0;
        }
    }

    (
        row.min(u16::MAX as usize) as u16,
        col.min(u16::MAX as usize) as u16,
    )
}
