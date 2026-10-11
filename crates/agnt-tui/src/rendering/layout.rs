pub const SIDE_PANE_COLLAPSE_COLUMNS: u16 = 120;
pub const INPUT_PROMPT_WIDTH: u16 = 2;
pub const INPUT_MIN_HEIGHT: u16 = 1;
pub const INPUT_MAX_HEIGHT: u16 = 6;
pub const MAIN_SIDE_MARGIN: u16 = 1;

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
        let wrapped = textwrap::wrap(
            logical_line,
            textwrap::Options::new(width).word_splitter(textwrap::WordSplitter::NoHyphenation),
        );
        if wrapped.is_empty() {
            rows.push(String::new());
        } else {
            rows.extend(wrapped.into_iter().map(|row| row.into_owned()));
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
