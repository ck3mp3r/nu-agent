use chrono::Datelike;
use ratatui::text::Line;

use crate::runtime::panels::picker_query_line;
use crate::runtime::render::section_head_count;
use crate::state::{AppState, PickerOption};

pub(super) const SESSION_PICKER_EMPTY_STATE_MESSAGE: &str =
    "No sessions found. Start a new session with /new.";

/// The session table's data: the rows the picker table walks, the budgets its
/// cells render with, and the clock its relative stamps and day heads read.
pub(super) struct SessionPickerTableModel {
    pub query_line: Line<'static>,
    pub options: Vec<PickerOption>,
    pub title_budget: usize,
    pub now: chrono::DateTime<chrono::Utc>,
    pub today: chrono::NaiveDate,
    /// The index of the selected option in `options`.
    pub selected: Option<usize>,
    pub overflow_cue: Option<String>,
}

pub(super) fn session_picker_table_model(
    state: &AppState,
    popup_height: u16,
    table_width: u16,
) -> SessionPickerTableModel {
    let inner_height = popup_height.saturating_sub(2) as usize;
    let query_height: usize = 1;
    let header_height: usize = 1;
    let available_rows = inner_height
        .saturating_sub(query_height)
        .saturating_sub(header_height);
    // When + Msgs columns, their spacing, and the selection column.
    let title_budget = table_width.saturating_sub(25) as usize;
    let now = chrono::Utc::now();
    let today = chrono::Local::now().date_naive();

    let Some(picker_state) = state.picker.active_state() else {
        return SessionPickerTableModel {
            query_line: Line::from(String::new()),
            options: Vec::new(),
            title_budget,
            now,
            today,
            selected: None,
            overflow_cue: None,
        };
    };
    let options = picker_state.filtered();
    let total = options.len();

    // Head rows consume table rows, so the number of session rows that fit is
    // the available height minus one row per day group.
    let head_rows = section_head_count(&options);
    let visible_session_rows = available_rows.saturating_sub(head_rows);
    let overflow_cue = if total > visible_session_rows {
        Some(format!("{visible_session_rows} of {total}"))
    } else {
        None
    };

    SessionPickerTableModel {
        query_line: Line::from(picker_query_line(&picker_state.query)),
        selected: (!options.is_empty()).then_some(picker_state.selection),
        options,
        title_budget,
        now,
        today,
        overflow_cue,
    }
}

/// The day head's label, relative to the current day: today and yesterday by
/// name, the current year without it, an earlier year with it.
pub(super) fn day_header_label(day: chrono::NaiveDate, today: chrono::NaiveDate) -> String {
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
