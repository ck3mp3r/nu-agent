use chrono::{Datelike, TimeZone};

use crate::runtime::render::section_head_count;
use crate::runtime::session_picker::{
    day_header_label, relative_timestamp, session_picker_table_model,
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

/// The day head names the current day and its predecessor, shortens a day of
/// the current year, and carries the year for any earlier one.
#[test]
fn test_session_picker_day_header_label_reads_relative_to_today() -> Result<()> {
    // -- Setup & Fixtures
    let today = chrono::NaiveDate::from_ymd_opt(2025, 10, 31).ok_or("should build a date")?;
    let yesterday = chrono::NaiveDate::from_ymd_opt(2025, 10, 30).ok_or("should build a date")?;
    let same_year = chrono::NaiveDate::from_ymd_opt(2025, 9, 30).ok_or("should build a date")?;
    let other_year = chrono::NaiveDate::from_ymd_opt(2024, 9, 30).ok_or("should build a date")?;

    // -- Exec & Check
    assert_eq!(day_header_label(today, today), "Today");
    assert_eq!(day_header_label(yesterday, today), "Yesterday");
    assert_eq!(day_header_label(same_year, today), "Sep 30");
    assert_eq!(day_header_label(other_year, today), "Sep 30, 2024");
    Ok(())
}

#[test]
fn test_session_picker_table_model_empty() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    state.picker.open(ActivePicker::Session);

    // -- Exec
    let model = session_picker_table_model(&state, 20, 80);

    // -- Check
    assert!(model.options.is_empty());
    assert_eq!(model.selected, None);
    assert_eq!(model.query_line.to_string(), "Query: ");
    Ok(())
}

#[test]
fn test_session_picker_table_model_single_day_single_head() -> Result<()> {
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
    let model = session_picker_table_model(&state, 20, 80);

    // -- Check
    assert_eq!(model.options.len(), 3);
    assert_eq!(
        section_head_count(&model.options),
        1,
        "one day group renders one head row"
    );
    Ok(())
}

#[test]
fn test_session_picker_table_model_heads_one_per_day() -> Result<()> {
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
    let model = session_picker_table_model(&state, 20, 80);

    // -- Check: one head per day, newest day first, and the options keep the
    // titles the rows render.
    assert_eq!(section_head_count(&model.options), 3);
    assert_eq!(
        section_keys(&model.options),
        vec![today.to_string(), yesterday.to_string(), older.to_string(),]
    );
    assert_eq!(
        session_titles(&model.options),
        vec![
            "Today Session".to_string(),
            "Yesterday Session".to_string(),
            "Older Session".to_string(),
        ]
    );
    Ok(())
}

/// The model reports the selected OPTION index; the head rows the table inserts
/// above it are the render walk's accounting.
#[test]
fn test_session_picker_table_model_selected_is_the_option_index() -> Result<()> {
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
    state
        .picker
        .active_state_mut()
        .ok_or("should have an active session picker")?
        .selection = 1;

    // -- Exec
    let model = session_picker_table_model(&state, 20, 80);

    // -- Check
    assert_eq!(model.options.len(), 2);
    assert_eq!(model.selected, Some(1));
    Ok(())
}

#[test]
fn test_session_picker_table_model_overflow_cue_accounts_for_footer_rows() -> Result<()> {
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
    // A 10-row popup leaves inner height 8, so the details ladder reserves two
    // footer rows. The render path passes popup.height - dh = 8 → inner 6 →
    // query=1, header=1 → available_rows=4, minus one day head → 3 rows.
    let model = session_picker_table_model(&state, 8, 80);

    // -- Check
    assert_eq!(model.overflow_cue, Some("3 of 5".to_string()));
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
    // One day group → 1 head row → 3 session rows fit.
    let model = session_picker_table_model(&state, 8, 80);

    // -- Check
    assert_eq!(section_head_count(&model.options), 1);
    assert_eq!(model.overflow_cue, Some("3 of 5".to_string()));
    Ok(())
}

#[test]
fn test_session_picker_table_model_overflow_cue_accounts_for_head_rows() -> Result<()> {
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
    // popup_height=8 → available_rows=4. Three day groups → 3 head rows, so
    // only 1 session row fits even though 3 sessions <= 4 available rows.
    let model = session_picker_table_model(&state, 8, 80);

    // -- Check
    assert_eq!(section_head_count(&model.options), 3);
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
    // popup_height=8 → available_rows=4. Two day groups → 2 head rows, so 2
    // session rows fit and both sessions are visible.
    let model = session_picker_table_model(&state, 8, 80);

    // -- Check
    assert_eq!(section_head_count(&model.options), 2);
    assert_eq!(model.overflow_cue, None);
    Ok(())
}

/// The model hands the render arm the title budget its rows ellipsize with: the
/// table width less the When and Msgs columns, their spacing, and the selection
/// gutter.
#[test]
fn test_session_picker_table_model_title_budget_leaves_the_columns() -> Result<()> {
    // -- Setup & Fixtures
    let today = today();
    let mut state = AppState::default();
    state.set_picker_options(
        ActivePicker::Session,
        vec![session_option("id0", Some("My Session"), today, 42)?],
    );
    state.picker.open(ActivePicker::Session);

    // -- Exec
    let model = session_picker_table_model(&state, 20, 80);

    // -- Check
    assert_eq!(model.title_budget, 55);
    let Some(option) = model.options.first() else {
        return Err("should carry the session option".into());
    };
    let PickerPayload::Session {
        title,
        message_count,
        created_at,
        ..
    } = &option.payload
    else {
        return Err("should carry a session payload".into());
    };
    assert_eq!(title.as_deref(), Some("My Session"));
    assert_eq!(*message_count, 42);
    assert!(!relative_timestamp(*created_at, model.now).is_empty());
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
        section: Some(crate::state::session_day(created_at).to_string()),
        sort_key: vec![PickerSortKeyPart::Recent(std::cmp::Reverse(created_at))],
        payload: PickerPayload::Session {
            session_id: id.to_string(),
            title: title.map(str::to_string),
            created_at,
            message_count,
        },
    })
}

fn section_keys(options: &[PickerOption]) -> Vec<String> {
    options
        .iter()
        .map(|option| option.section.clone().unwrap_or_default())
        .collect()
}

fn session_titles(options: &[PickerOption]) -> Vec<String> {
    options
        .iter()
        .filter_map(|option| match &option.payload {
            PickerPayload::Session { title, .. } => title.clone(),
            _ => None,
        })
        .collect()
}

// endregion: --- Test Support
