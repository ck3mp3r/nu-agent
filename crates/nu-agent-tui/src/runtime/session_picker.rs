use ratatui::text::Line;

use crate::state::{AppState, PickerPayload};

pub(super) const SESSION_PICKER_EMPTY_STATE_MESSAGE: &str =
    "No sessions found. Start a new session with /new.";

pub(super) fn session_picker_table_model(
    state: &AppState,
    popup_height: u16,
) -> SessionPickerTableModel {
    let inner_height = popup_height.saturating_sub(2) as usize;
    let query_height: usize = 1;
    let header_height: usize = 1;
    let available_rows = inner_height
        .saturating_sub(query_height)
        .saturating_sub(header_height);

    let picker_state = state.picker.active_state().expect("session picker open");
    let options = picker_state.filtered();
    let total = options.len();
    let overflow_cue = if total > available_rows {
        Some(format!("{available_rows} of {total}"))
    } else {
        None
    };

    let now = chrono::Utc::now();
    let rows: Vec<Vec<String>> = options
        .iter()
        .map(|opt| {
            let (created_at, title) = match &opt.payload {
                PickerPayload::Session {
                    created_at, title, ..
                } => (created_at, title),
                _ => unreachable!(),
            };
            let relative = relative_timestamp(*created_at, now);
            let title = title.as_deref().unwrap_or("(untitled)").to_string();
            vec![relative, title]
        })
        .collect();

    let query_line = Line::from(format!("Query: {}", picker_state.query));

    SessionPickerTableModel {
        query_line,
        rows,
        selected: Some(picker_state.selection),
        overflow_cue,
    }
}

pub(super) struct SessionPickerTableModel {
    pub query_line: Line<'static>,
    pub rows: Vec<Vec<String>>,
    pub selected: Option<usize>,
    pub overflow_cue: Option<String>,
}

pub(super) fn relative_timestamp(
    timestamp: chrono::DateTime<chrono::Utc>,
    now: chrono::DateTime<chrono::Utc>,
) -> String {
    let duration = now.signed_duration_since(timestamp);
    let seconds = duration.num_seconds();

    if seconds < 60 {
        format!("{seconds}s ago")
    } else if seconds < 3600 {
        format!("{}m ago", seconds / 60)
    } else if seconds < 86400 {
        format!("{}h ago", seconds / 3600)
    } else if seconds < 604800 {
        format!("{}d ago", seconds / 86400)
    } else if seconds < 2592000 {
        format!("{}w ago", seconds / 604800)
    } else if seconds < 31536000 {
        format!("{}mo ago", seconds / 2592000)
    } else {
        format!("{}y ago", seconds / 31536000)
    }
}
