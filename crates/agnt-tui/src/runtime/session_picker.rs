use chrono::Datelike;
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

    let Some(picker_state) = state.picker.active_state() else {
        return SessionPickerTableModel {
            query_line: Line::from(String::new()),
            rows: Vec::new(),
            selected: None,
            overflow_cue: None,
        };
    };
    let options = picker_state.filtered();
    let total = options.len();

    let now = chrono::Utc::now();
    let today = chrono::Local::now().date_naive();
    let mut rows: Vec<SessionPickerRow> = Vec::new();
    let mut current_day: Option<chrono::NaiveDate> = None;
    let mut selected = None;
    for (option_idx, opt) in options.iter().enumerate() {
        let (created_at, title, message_count) = match &opt.payload {
            PickerPayload::Session {
                created_at,
                title,
                message_count,
                ..
            } => (created_at, title, *message_count),
            _ => unreachable!(),
        };
        let day = created_at.with_timezone(&chrono::Local).date_naive();
        if current_day != Some(day) {
            rows.push(SessionPickerRow::DayHeader(day_header_label(day, today)));
            current_day = Some(day);
        }
        rows.push(SessionPickerRow::Session {
            relative: relative_timestamp(*created_at, now),
            title: title.as_deref().unwrap_or("(untitled)").to_string(),
            message_count,
        });
        if option_idx == picker_state.selection {
            selected = Some(rows.len() - 1);
        }
    }

    let query_line = Line::from(format!("Query: {}", picker_state.query));

    // Header rows consume table rows, so the number of session rows that fit
    // is the available height minus one row per day group.
    let header_rows = rows.len().saturating_sub(total);
    let visible_session_rows = available_rows.saturating_sub(header_rows);
    let overflow_cue = if total > visible_session_rows {
        Some(format!("{visible_session_rows} of {total}"))
    } else {
        None
    };

    SessionPickerTableModel {
        query_line,
        rows,
        selected,
        overflow_cue,
    }
}

pub(super) enum SessionPickerRow {
    DayHeader(String),
    Session {
        relative: String,
        title: String,
        message_count: usize,
    },
}

fn day_header_label(day: chrono::NaiveDate, today: chrono::NaiveDate) -> String {
    if day == today {
        "Today".to_string()
    } else if today.pred_opt() == Some(day) {
        "Yesterday".to_string()
    } else if day.year() == today.year() {
        day.format("%b %d").to_string()
    } else {
        day.format("%b %d, %Y").to_string()
    }
}

pub(super) struct SessionPickerTableModel {
    pub query_line: Line<'static>,
    pub rows: Vec<SessionPickerRow>,
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
