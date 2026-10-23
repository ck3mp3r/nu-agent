use chrono::{Datelike, TimeZone};

use crate::runtime::session_picker::{
    SessionPickerRow, relative_timestamp, session_picker_table_model,
};
use crate::state::{ActivePicker, AppState, PickerOption, PickerPayload, PickerSortKeyPart};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Tests

#[test]
fn test_session_picker_relative_timestamp_seconds() {
    let now = chrono::Utc::now();
    let ts = now - chrono::Duration::seconds(30);
    assert_eq!(relative_timestamp(ts, now), "30s ago");
}

#[test]
fn test_session_picker_relative_timestamp_minutes() {
    let now = chrono::Utc::now();
    let ts = now - chrono::Duration::minutes(5);
    assert_eq!(relative_timestamp(ts, now), "5m ago");
}

#[test]
fn test_session_picker_relative_timestamp_hours() {
    let now = chrono::Utc::now();
    let ts = now - chrono::Duration::hours(3);
    assert_eq!(relative_timestamp(ts, now), "3h ago");
}

#[test]
fn test_session_picker_relative_timestamp_days() {
    let now = chrono::Utc::now();
    let ts = now - chrono::Duration::days(2);
    assert_eq!(relative_timestamp(ts, now), "2d ago");
}

#[test]
fn test_session_picker_relative_timestamp_weeks() {
    let now = chrono::Utc::now();
    let ts = now - chrono::Duration::weeks(3);
    assert_eq!(relative_timestamp(ts, now), "3w ago");
}

#[test]
fn test_session_picker_relative_timestamp_months() {
    let now = chrono::Utc::now();
    let ts = now - chrono::Duration::days(60);
    assert_eq!(relative_timestamp(ts, now), "2mo ago");
}

#[test]
fn test_session_picker_relative_timestamp_years() {
    let now = chrono::Utc::now();
    let ts = now - chrono::Duration::days(400);
    assert_eq!(relative_timestamp(ts, now), "1y ago");
}

#[test]
fn test_session_picker_table_model_empty() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    state.picker.open(ActivePicker::Session);

    // -- Exec
    let model = session_picker_table_model(&state, 20);

    // -- Check
    assert!(model.rows.is_empty());
    assert_eq!(model.selected, None);
    assert_eq!(model.query_line.to_string(), "Query: ");
    Ok(())
}

#[test]
fn test_session_picker_table_model_single_day_single_header() -> Result<()> {
    // -- Setup & Fixtures
    let today = today();
    let mut state = AppState::default();
    state.set_picker_options(
        ActivePicker::Session,
        vec![
            session_option("id0", Some("Session 0"), today, 1)?,
            session_option("id1", Some("Session 1"), today, 2)?,
            session_option("id2", Some("Session 2"), today, 3)?,
        ],
    );
    state.picker.open(ActivePicker::Session);

    // -- Exec
    let model = session_picker_table_model(&state, 20);

    // -- Check
    assert_eq!(model.rows.len(), 4);
    assert_eq!(header_labels(&model.rows), vec!["Today".to_string()]);
    Ok(())
}

#[test]
fn test_session_picker_table_model_groups_by_day() -> Result<()> {
    // -- Setup & Fixtures
    let today = today();
    let yesterday = today - chrono::Duration::days(1);
    let older = today - chrono::Duration::days(3);
    let mut state = AppState::default();
    state.set_picker_options(
        ActivePicker::Session,
        vec![
            session_option("id0", Some("Today Session"), today, 1)?,
            session_option("id1", Some("Yesterday Session"), yesterday, 2)?,
            session_option("id2", Some("Older Session"), older, 3)?,
        ],
    );
    state.picker.open(ActivePicker::Session);

    // -- Exec
    let model = session_picker_table_model(&state, 20);

    // -- Check
    assert_eq!(model.rows.len(), 6);
    assert_eq!(
        header_labels(&model.rows),
        vec![
            "Today".to_string(),
            "Yesterday".to_string(),
            expected_day_label(older, today),
        ]
    );
    assert_eq!(
        session_titles(&model.rows),
        vec![
            "Today Session".to_string(),
            "Yesterday Session".to_string(),
            "Older Session".to_string(),
        ]
    );
    Ok(())
}

#[test]
fn test_session_picker_table_model_header_includes_year_for_other_year() -> Result<()> {
    // -- Setup & Fixtures
    let today = today();
    let previous_year = today - chrono::Duration::days(400);
    let mut state = AppState::default();
    state.set_picker_options(
        ActivePicker::Session,
        vec![session_option(
            "id0",
            Some("Old Session"),
            previous_year,
            1,
        )?],
    );
    state.picker.open(ActivePicker::Session);

    // -- Exec
    let model = session_picker_table_model(&state, 20);

    // -- Check
    assert_eq!(
        header_labels(&model.rows),
        vec![previous_year.format("%b %d, %Y").to_string()]
    );
    Ok(())
}

#[test]
fn test_session_picker_table_model_selected_maps_to_table_row() -> Result<()> {
    // -- Setup & Fixtures
    let today = today();
    let yesterday = today - chrono::Duration::days(1);
    let mut state = AppState::default();
    state.set_picker_options(
        ActivePicker::Session,
        vec![
            session_option("id0", Some("Today Session"), today, 1)?,
            session_option("id1", Some("Yesterday Session"), yesterday, 2)?,
        ],
    );
    state.picker.open(ActivePicker::Session);
    let picker_state = state
        .picker
        .active_state_mut()
        .ok_or("should have an active session picker")?;
    picker_state.selection = 1;

    // -- Exec
    let model = session_picker_table_model(&state, 20);

    // -- Check
    // Table rows: [header, today, header, yesterday] — the second option is row 3.
    assert_eq!(model.rows.len(), 4);
    assert_eq!(model.selected, Some(3));
    Ok(())
}

#[test]
fn test_session_picker_table_model_overflow_cue_counts_session_rows() -> Result<()> {
    // -- Setup & Fixtures
    let today = today();
    let options: Vec<PickerOption> = (0..5)
        .map(|i| {
            session_option(
                &format!("id{i}"),
                Some(&format!("Session {i}")),
                today,
                i + 1,
            )
        })
        .collect::<Result<Vec<_>>>()?;
    let mut state = AppState::default();
    state.set_picker_options(ActivePicker::Session, options);
    state.picker.open(ActivePicker::Session);

    // -- Exec
    // popup_height=8 → inner_height=6 → query=1, header=1 → available_rows=4
    // One day group → 1 header row → 3 session rows fit.
    let model = session_picker_table_model(&state, 8);

    // -- Check
    assert_eq!(model.rows.len(), 6);
    assert_eq!(model.overflow_cue, Some("3 of 5".to_string()));
    Ok(())
}

#[test]
fn test_session_picker_table_model_overflow_cue_accounts_for_header_rows() -> Result<()> {
    // -- Setup & Fixtures
    let today = today();
    let mut state = AppState::default();
    state.set_picker_options(
        ActivePicker::Session,
        vec![
            session_option("id0", Some("Session 0"), today, 1)?,
            session_option(
                "id1",
                Some("Session 1"),
                today - chrono::Duration::days(1),
                2,
            )?,
            session_option(
                "id2",
                Some("Session 2"),
                today - chrono::Duration::days(2),
                3,
            )?,
        ],
    );
    state.picker.open(ActivePicker::Session);

    // -- Exec
    // popup_height=8 → available_rows=4. Three day groups → 3 header rows, so
    // only 1 session row fits even though 3 sessions <= 4 available rows.
    let model = session_picker_table_model(&state, 8);

    // -- Check
    assert_eq!(model.rows.len(), 6);
    assert_eq!(model.overflow_cue, Some("1 of 3".to_string()));
    Ok(())
}

#[test]
fn test_session_picker_table_model_no_overflow_cue_when_all_rows_fit() -> Result<()> {
    // -- Setup & Fixtures
    let today = today();
    let mut state = AppState::default();
    state.set_picker_options(
        ActivePicker::Session,
        vec![
            session_option("id0", Some("Session 0"), today, 1)?,
            session_option(
                "id1",
                Some("Session 1"),
                today - chrono::Duration::days(1),
                2,
            )?,
        ],
    );
    state.picker.open(ActivePicker::Session);

    // -- Exec
    // popup_height=8 → available_rows=4. Two day groups → 2 header rows, so 2
    // session rows fit and both sessions are visible.
    let model = session_picker_table_model(&state, 8);

    // -- Check
    assert_eq!(model.rows.len(), 4);
    assert_eq!(model.overflow_cue, None);
    Ok(())
}

#[test]
fn test_session_picker_table_model_renders_message_count_cell() -> Result<()> {
    // -- Setup & Fixtures
    let today = today();
    let mut state = AppState::default();
    state.set_picker_options(
        ActivePicker::Session,
        vec![session_option("abc123", Some("My Session"), today, 42)?],
    );
    state.picker.open(ActivePicker::Session);

    // -- Exec
    let model = session_picker_table_model(&state, 20);

    // -- Check
    assert_eq!(model.rows.len(), 2);
    let Some(SessionPickerRow::Session {
        relative,
        title,
        message_count,
    }) = model.rows.get(1)
    else {
        return Err("should have a session row at index 1".into());
    };
    assert!(!relative.is_empty());
    assert_eq!(title, "My Session");
    assert_eq!(*message_count, 42);
    Ok(())
}

// endregion: --- Tests

// region:    --- Test Support

fn today() -> chrono::NaiveDate {
    chrono::Local::now().date_naive()
}

fn session_option(
    id: &str,
    title: Option<&str>,
    date: chrono::NaiveDate,
    message_count: usize,
) -> Result<PickerOption> {
    let local = chrono::Local
        .with_ymd_and_hms(date.year(), date.month(), date.day(), 12, 0, 0)
        .single()
        .ok_or("should build a local datetime")?;
    let created_at = local.with_timezone(&chrono::Utc);
    let display = format!("{} ({id})", title.unwrap_or("(untitled)"));
    Ok(PickerOption {
        id: id.to_string(),
        display: display.clone(),
        search_text: display,
        sort_key: vec![PickerSortKeyPart::Recent(std::cmp::Reverse(created_at))],
        payload: PickerPayload::Session {
            session_id: id.to_string(),
            title: title.map(str::to_string),
            created_at,
            message_count,
        },
    })
}

fn expected_day_label(day: chrono::NaiveDate, today: chrono::NaiveDate) -> String {
    if day.year() == today.year() {
        day.format("%b %d").to_string()
    } else {
        day.format("%b %d, %Y").to_string()
    }
}

fn header_labels(rows: &[SessionPickerRow]) -> Vec<String> {
    rows.iter()
        .filter_map(|row| match row {
            SessionPickerRow::DayHeader(label) => Some(label.clone()),
            SessionPickerRow::Session { .. } => None,
        })
        .collect()
}

fn session_titles(rows: &[SessionPickerRow]) -> Vec<String> {
    rows.iter()
        .filter_map(|row| match row {
            SessionPickerRow::Session { title, .. } => Some(title.clone()),
            SessionPickerRow::DayHeader(_) => None,
        })
        .collect()
}

// endregion: --- Test Support
