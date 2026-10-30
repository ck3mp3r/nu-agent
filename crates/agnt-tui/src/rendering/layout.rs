use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;

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

/// The number of visual rows the input textarea renders at `content_width`,
/// counting soft-wrapped rows.
///
/// The count is read from the textarea's own wrap state, never recomputed here.
/// Rendering the widget into a scratch buffer at the content width installs the
/// crate's screen map, and [`TextArea::screen_to_data`] then reports the text
/// position each visual row starts at. That position strictly increases while a
/// row exists and stops advancing past the last one, so the row index at which it
/// repeats is the visual row count.
///
/// The scratch render leaves the widget's viewport pointing at the scratch rect,
/// so the caller's next render repaints the input and restores it.
pub fn input_content_row_count(
    textarea: &ratatui_textarea::TextArea<'_>,
    content_width: u16,
) -> u16 {
    let scratch = Rect {
        x: 0,
        y: 0,
        width: content_width.max(1),
        height: INPUT_MAX_HEIGHT,
    };
    let mut buffer = Buffer::empty(scratch);
    textarea.render(scratch, &mut buffer);

    let mut previous = textarea.screen_to_data(0, 0);
    for row in 1..=usize::from(u16::MAX) {
        let current = textarea.screen_to_data(row, 0);
        if current == previous {
            return u16::try_from(row).unwrap_or(u16::MAX);
        }
        previous = current;
    }
    u16::MAX
}
