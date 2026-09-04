use crate::runtime::session_picker::{relative_timestamp, session_picker_table_model};
use crate::state::{ActivePicker, AppState, PickerOption, PickerPayload};

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
    let mut state = AppState::default();
    state.picker.open(ActivePicker::Session);
    let model = session_picker_table_model(&state, 20);
    assert!(model.rows.is_empty());
    assert_eq!(model.query_line.to_string(), "Query: ");
}

#[test]
fn session_picker_table_model_with_options() {
    let mut state = AppState::default();
    let now = chrono::Utc::now();
    state.set_picker_options(
        ActivePicker::Session,
        vec![
            PickerOption {
                id: "abc123def456789".to_string(),
                display: "My Session (abc123def456789)".to_string(),
                search_text: "My Session (abc123def456789)".to_string(),
                payload: PickerPayload::Session {
                    session_id: "abc123def456789".to_string(),
                    title: Some("My Session".to_string()),
                    created_at: now - chrono::Duration::hours(2),
                },
            },
            PickerOption {
                id: "xyz789".to_string(),
                display: "(untitled) (xyz789)".to_string(),
                search_text: "(untitled) (xyz789)".to_string(),
                payload: PickerPayload::Session {
                    session_id: "xyz789".to_string(),
                    title: None,
                    created_at: now - chrono::Duration::days(1),
                },
            },
        ],
    );
    state.picker.open(ActivePicker::Session);

    let model = session_picker_table_model(&state, 20);
    assert_eq!(model.rows.len(), 2);
    assert_eq!(model.rows[0][0], "2h ago");
    assert_eq!(model.rows[0][1], "My Session");
    assert_eq!(model.rows[1][0], "1d ago");
    assert_eq!(model.rows[1][1], "(untitled)");
}

#[test]
fn session_picker_table_model_overflow_cue() {
    let mut state = AppState::default();
    let now = chrono::Utc::now();
    let options: Vec<PickerOption> = (0..5)
        .map(|i| PickerOption {
            id: format!("id{i}"),
            display: format!("Session {i} (id{i})"),
            search_text: format!("Session {i} (id{i})"),
            payload: PickerPayload::Session {
                session_id: format!("id{i}"),
                title: Some(format!("Session {i}")),
                created_at: now - chrono::Duration::hours(i),
            },
        })
        .collect();
    state.set_picker_options(ActivePicker::Session, options);
    state.picker.open(ActivePicker::Session);

    // popup_height=5 → inner_height=3 → query=1, header=1 → available_rows=1
    let model = session_picker_table_model(&state, 5);
    assert_eq!(model.rows.len(), 5);
    assert_eq!(model.overflow_cue, Some("1 of 5".to_string()));
}
