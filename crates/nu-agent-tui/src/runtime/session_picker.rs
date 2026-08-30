use crate::state::AppState;
use ratatui::text::Line;

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

    let options = state.session_picker_filtered_options();
    let total = options.len();
    let overflow_cue = if total > available_rows {
        Some(format!("{available_rows} of {total}"))
    } else {
        None
    };

    let now = chrono::Utc::now();
    let rows: Vec<Vec<String>> = options
        .iter()
        .map(|option| {
            let relative = relative_timestamp(option.created_at, now);
            let title = option.title.as_deref().unwrap_or("(untitled)").to_string();
            vec![relative, title]
        })
        .collect();

    let query_line = Line::from(format!("Query: {}", state.session_picker_query));

    SessionPickerTableModel {
        query_line,
        rows,
        selected: Some(state.session_picker_selection),
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
