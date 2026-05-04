pub const SIDE_PANE_COLLAPSE_COLUMNS: u16 = 120;

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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayoutOutput {
    pub transcript: PaneGeometry,
    pub status_event: PaneGeometry,
    pub input: PaneGeometry,
    pub side_pane: Option<PaneGeometry>,
}

pub fn recompute_layout(input: LayoutInput) -> LayoutOutput {
    let (main_width, side_pane) = compute_columns(input.columns, input.rows, input.side_pane_visible);
    let (transcript_height, status_height, input_height) = compute_rows(input.rows);

    let transcript = clip(
        PaneGeometry {
            x: 0,
            y: 0,
            width: main_width,
            height: transcript_height,
        },
        input.columns,
        input.rows,
    );

    let status_event = clip(
        PaneGeometry {
            x: 0,
            y: transcript.height,
            width: main_width,
            height: status_height,
        },
        input.columns,
        input.rows,
    );

    let input_pane = clip(
        PaneGeometry {
            x: 0,
            y: transcript.height.saturating_add(status_event.height),
            width: main_width,
            height: input_height,
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

fn compute_rows(rows: u16) -> (u16, u16, u16) {
    let status_height = if rows >= 2 { STATUS_HEIGHT } else { 0 };

    let rows_after_status = rows.saturating_sub(status_height);
    let input_height = rows_after_status.min(INPUT_HEIGHT);
    let transcript_height = rows_after_status.saturating_sub(input_height);

    (transcript_height, status_height, input_height)
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
