pub const SIDE_PANE_COLLAPSE_COLUMNS: u16 = 120;
pub const INPUT_PROMPT_WIDTH: u16 = 2;
pub const INPUT_MIN_HEIGHT: u16 = 2;
pub const INPUT_MAX_HEIGHT: u16 = 8;
pub const MAIN_SIDE_MARGIN: u16 = 2;

const MIN_MAIN_COLUMNS: u16 = 72;
const MIN_SIDE_COLUMNS: u16 = 24;
const INPUT_HEIGHT: u16 = 3;
const STATUS_HEIGHT: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PaneGeometry {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayoutInput {
    pub columns: u16,
    pub rows: u16,
    pub side_pane_visible: Option<bool>,
    pub input_height: Option<u16>,
    pub queue_height: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayoutOutput {
    pub transcript: PaneGeometry,
    pub queue: PaneGeometry,
    pub status_event: PaneGeometry,
    pub input: PaneGeometry,
    pub side_pane: Option<PaneGeometry>,
}

pub fn recompute_layout(input: LayoutInput) -> LayoutOutput {
    let (main_width, side_pane) =
        compute_columns(input.columns, input.rows, input.side_pane_visible);
    let (transcript_height, queue_height, input_height, status_height) =
        compute_rows(input.rows, input.input_height, input.queue_height);
    let margin = side_margin_for_main_width(main_width);
    let inner_width = main_width.saturating_sub(margin.saturating_mul(2));

    let transcript = clip(
        PaneGeometry {
            x: margin,
            y: 0,
            width: inner_width,
            height: transcript_height,
        },
        input.columns,
        input.rows,
    );

    let queue_pane = clip(
        PaneGeometry {
            x: margin,
            y: transcript.height,
            width: inner_width,
            height: queue_height,
        },
        input.columns,
        input.rows,
    );

    let input_pane = clip(
        PaneGeometry {
            x: margin,
            y: transcript.height.saturating_add(queue_pane.height),
            width: inner_width,
            height: input_height,
        },
        input.columns,
        input.rows,
    );

    let status_event = clip(
        PaneGeometry {
            x: margin,
            y: transcript
                .height
                .saturating_add(queue_pane.height)
                .saturating_add(input_pane.height),
            width: inner_width,
            height: status_height,
        },
        input.columns,
        input.rows,
    );

    let side_pane = side_pane.map(|side_width| {
        clip(
            PaneGeometry {
                x: main_width,
                y: 0,
                width: side_width,
                height: input.rows,
            },
            input.columns,
            input.rows,
        )
    });

    LayoutOutput {
        transcript,
        queue: queue_pane,
        status_event,
        input: input_pane,
        side_pane,
    }
}

fn compute_columns(columns: u16, rows: u16, side_pane_visible: Option<bool>) -> (u16, Option<u16>) {
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

fn compute_rows(
    rows: u16,
    preferred_input_height: Option<u16>,
    queue_height: u16,
) -> (u16, u16, u16, u16) {
    let status_height = if rows >= 2 { STATUS_HEIGHT } else { 0 };

    let rows_after_status = rows.saturating_sub(status_height);
    let requested_input_height = preferred_input_height.unwrap_or(INPUT_HEIGHT).max(1);
    let input_height = rows_after_status.min(requested_input_height);
    let rows_after_input = rows_after_status.saturating_sub(input_height);
    let actual_queue_h = rows_after_input.min(queue_height);
    let transcript_height = rows_after_input.saturating_sub(actual_queue_h);

    (
        transcript_height,
        actual_queue_h,
        input_height,
        status_height,
    )
}

fn clip(geometry: PaneGeometry, columns: u16, rows: u16) -> PaneGeometry {
    let x = geometry.x.min(columns);
    let y = geometry.y.min(rows);
    let width = geometry.width.min(columns.saturating_sub(x));
    let height = geometry.height.min(rows.saturating_sub(y));

    PaneGeometry {
        x,
        y,
        width,
        height,
    }
}

fn side_margin_for_main_width(main_width: u16) -> u16 {
    if main_width < 8 { 0 } else { MAIN_SIDE_MARGIN }
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
    let content_width = pane_width.saturating_sub(INPUT_PROMPT_WIDTH).max(1) as usize;
    let content_rows = input_content_row_count(input, content_width);
    let desired = content_rows.saturating_add(1);
    desired.clamp(INPUT_MIN_HEIGHT, INPUT_MAX_HEIGHT)
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
