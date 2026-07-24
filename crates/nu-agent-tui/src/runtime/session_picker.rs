use super::*;

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
        Some(format!("{} of {}", available_rows, total))
    } else {
        None
    };

    let now = chrono::Utc::now();
    let rows: Vec<Vec<String>> = options
        .iter()
        .take(available_rows)
        .map(|option| {
            let relative = relative_timestamp(option.created_at, now);
            let title = option.title.as_deref().unwrap_or("(untitled)").to_string();
            let truncated_title = if title.chars().count() > 20 {
                format!("{}…", title.chars().take(19).collect::<String>())
            } else {
                title
            };
            let id = if option.id.len() > 12 {
                format!("{}…", &option.id[..12])
            } else {
                option.id.clone()
            };
            vec![relative, truncated_title, id]
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

fn relative_timestamp(
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

#[cfg(test)]
mod test {
    use super::*;
    use crate::state::SessionPickerOption;

    #[test]
    fn relative_timestamp_seconds() {
        let now = chrono::Utc::now();
        let ts = now - chrono::Duration::seconds(30);
        assert_eq!(relative_timestamp(ts, now), "30s ago");
    }

    #[test]
    fn relative_timestamp_minutes() {
        let now = chrono::Utc::now();
        let ts = now - chrono::Duration::minutes(5);
        assert_eq!(relative_timestamp(ts, now), "5m ago");
    }

    #[test]
    fn relative_timestamp_hours() {
        let now = chrono::Utc::now();
        let ts = now - chrono::Duration::hours(3);
        assert_eq!(relative_timestamp(ts, now), "3h ago");
    }

    #[test]
    fn relative_timestamp_days() {
        let now = chrono::Utc::now();
        let ts = now - chrono::Duration::days(2);
        assert_eq!(relative_timestamp(ts, now), "2d ago");
    }

    #[test]
    fn relative_timestamp_weeks() {
        let now = chrono::Utc::now();
        let ts = now - chrono::Duration::weeks(3);
        assert_eq!(relative_timestamp(ts, now), "3w ago");
    }

    #[test]
    fn relative_timestamp_months() {
        let now = chrono::Utc::now();
        let ts = now - chrono::Duration::days(60);
        assert_eq!(relative_timestamp(ts, now), "2mo ago");
    }

    #[test]
    fn relative_timestamp_years() {
        let now = chrono::Utc::now();
        let ts = now - chrono::Duration::days(400);
        assert_eq!(relative_timestamp(ts, now), "1y ago");
    }

    #[test]
    fn session_picker_table_model_empty() {
        let state = AppState::new();
        let model = session_picker_table_model(&state, 20);
        assert!(model.rows.is_empty());
        assert_eq!(model.query_line.to_string(), "Query: ");
    }

    #[test]
    fn session_picker_table_model_with_options() {
        let mut state = AppState::new();
        let now = chrono::Utc::now();
        state.set_session_picker_options(vec![
            SessionPickerOption {
                id: "abc123def456789".to_string(),
                title: Some("My Session".to_string()),
                created_at: now - chrono::Duration::hours(2),
                display: "My Session (abc123def456789)".to_string(),
            },
            SessionPickerOption {
                id: "xyz789".to_string(),
                title: None,
                created_at: now - chrono::Duration::days(1),
                display: "(untitled) (xyz789)".to_string(),
            },
        ]);
        state.open_session_picker();

        let model = session_picker_table_model(&state, 20);
        assert_eq!(model.rows.len(), 2);
        assert_eq!(model.rows[0][0], "2h ago");
        assert_eq!(model.rows[0][1], "My Session");
        assert_eq!(model.rows[0][2], "abc123def456…");
        assert_eq!(model.rows[1][0], "1d ago");
        assert_eq!(model.rows[1][1], "(untitled)");
        assert_eq!(model.rows[1][2], "xyz789");
    }

    #[test]
    fn session_picker_table_model_overflow_cue() {
        let mut state = AppState::new();
        let now = chrono::Utc::now();
        let options: Vec<SessionPickerOption> = (0..5)
            .map(|i| SessionPickerOption {
                id: format!("id{i}"),
                title: Some(format!("Session {i}")),
                created_at: now - chrono::Duration::hours(i),
                display: format!("Session {i} (id{i})"),
            })
            .collect();
        state.set_session_picker_options(options);
        state.open_session_picker();

        // popup_height=5 → inner_height=3 → query=1, header=1 → available_rows=1
        let model = session_picker_table_model(&state, 5);
        assert_eq!(model.rows.len(), 1);
        assert_eq!(model.overflow_cue, Some("1 of 5".to_string()));
    }
}
