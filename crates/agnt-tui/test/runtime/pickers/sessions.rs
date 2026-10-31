use super::*;

use crate::rendering::theme::TuiTheme;
use crate::state::{PickerOption, PickerPayload, PickerSortKeyPart};

/// At 56x20 the popup inner height is 18, so the details ladder caps the footer
/// at three rows. The one-row selected title occupies the bottom inner row and
/// the table budget grows by the two rows the footer does not need, so the
/// overflow cue counts them.
#[tokio::test]
async fn test_session_picker_footer_shows_full_title_and_reduced_cue() -> Result<()> {
    // -- Setup & Fixtures
    let title = "review the picker footer wrapping behaviour";
    let now = chrono::Utc::now();
    let options = (0..20)
        .map(|i| session_option(&format!("id{i}"), title, now, i + 1))
        .collect::<Vec<_>>();
    let mut driver = RenderLoopDriver::new(58, 24);
    driver
        .coordinator_mut()
        .state
        .set_picker_options(ActivePicker::Session, options);
    driver
        .coordinator_mut()
        .state
        .picker
        .open(ActivePicker::Session);

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check: popup 56x20 → inner 18 → ladder cap 3, and the title wraps to
    // one row at width 51, so the footer rect claims two rows (the gap plus the
    // text row) and the model is budgeted with popup height 18 → inner 16 → 14
    // row slots, one day header, and 13 visible session rows.
    let popup = super::render::frame::modal_rect_for_panel(
        driver.state().scroll.transcript_area,
        super::render::frame::ModalPanelKind::Sessions,
    );
    let popup_text = driver.buffer_text_in_rect(popup);
    let lines: Vec<&str> = popup_text.lines().collect();
    let title_line = lines.first().ok_or("should render the popup title")?;
    assert!(
        title_line.contains("13 of 20"),
        "the cue must count rows against the footer-sized budget; title: {title_line:?}\n{popup_text}"
    );
    let clipped = format!("{}…", title.chars().take(30).collect::<String>());
    let table_idx = lines
        .iter()
        .position(|line| line.contains(&clipped))
        .ok_or("should render the ellipsized title in the table")?;
    let footer_idx = lines
        .iter()
        .position(|line| line.contains(title))
        .ok_or("should render the full title in the footer")?;
    assert!(
        table_idx < footer_idx,
        "the table row must sit above the footer; table: {table_idx}, footer: {footer_idx}\n{popup_text}"
    );
    assert_eq!(
        footer_idx, 18,
        "the footer must sit on the bottom inner row (18); popup:\n{popup_text}"
    );
    Ok(())
}

/// At 56x20 a twenty-session list fills the table area, so the last data row
/// would sit flush against the footer text. The footer text takes the bottom
/// inner row and the row directly above it stays blank base background.
#[tokio::test]
async fn test_session_picker_footer_leaves_a_gap_row_above_the_text() -> Result<()> {
    // -- Setup & Fixtures
    let title = "review the picker footer wrapping behaviour";
    let now = chrono::Utc::now();
    let options = (0..20)
        .map(|i| session_option(&format!("id{i}"), title, now, i + 1))
        .collect::<Vec<_>>();
    let theme = TuiTheme::default();
    let mut driver = RenderLoopDriver::new(58, 24);
    driver
        .coordinator_mut()
        .state
        .set_picker_options(ActivePicker::Session, options);
    driver
        .coordinator_mut()
        .state
        .picker
        .open(ActivePicker::Session);

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check: popup 56x20 → inner 54x18 → the one-row title fills one text
    // row, so the footer claims rows 17 and 18: row 17 is the blank gap and row
    // 18 carries the title on the bottom inner row.
    let popup = super::render::frame::modal_rect_for_panel(
        driver.state().scroll.transcript_area,
        super::render::frame::ModalPanelKind::Sessions,
    );
    let popup_text = driver.buffer_text_in_rect(popup);
    let lines: Vec<&str> = popup_text.lines().collect();
    assert!(
        lines[17].trim_matches(['│', ' ']).is_empty(),
        "the gap row above the footer text must carry no text; row: {:?}\n{popup_text}",
        lines[17]
    );
    assert_eq!(
        driver.cell_style_at(popup.x + 5, 17).bg,
        Some(theme.base),
        "the gap row must keep the base background; popup:\n{popup_text}"
    );
    assert!(
        !lines[16].trim_matches(['│', ' ']).is_empty(),
        "the table must fill every row above the gap; row: {:?}\n{popup_text}",
        lines[16]
    );
    assert!(
        lines[18].contains(title),
        "the footer text must still sit on the bottom inner row (18); row: {:?}\n{popup_text}",
        lines[18]
    );
    Ok(())
}

/// The day head carries no background band: it renders bold subtext0 text on
/// the popup base, with its label on the When column.
#[tokio::test]
async fn test_session_picker_day_header_is_fg_only_on_the_when_column() -> Result<()> {
    // -- Setup & Fixtures
    let title = "review the picker footer wrapping behaviour";
    let now = chrono::Utc::now();
    let options = (0..3)
        .map(|i| session_option(&format!("id{i}"), title, now, i + 1))
        .collect::<Vec<_>>();
    let palette = crate::rendering::theme::CatppuccinPalette::mocha();
    let theme = TuiTheme::default();
    let mut driver = RenderLoopDriver::new(58, 24);
    driver
        .coordinator_mut()
        .state
        .set_picker_options(ActivePicker::Session, options);
    driver
        .coordinator_mut()
        .state
        .picker
        .open(ActivePicker::Session);

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check: popup 56x20 at the origin. The day header renders on the table's
    // first data row, and its label starts on the When column (popup column 3).
    let popup = super::render::frame::modal_rect_for_panel(
        driver.state().scroll.transcript_area,
        super::render::frame::ModalPanelKind::Sessions,
    );
    let popup_text = driver.buffer_text_in_rect(popup);
    let lines: Vec<&str> = popup_text.lines().collect();
    let header_row = lines
        .iter()
        .position(|line| line.contains("Today"))
        .ok_or("should render the day header")?;
    let session_row = lines
        .iter()
        .position(|line| line.contains(" ago"))
        .ok_or("should render a session row")?;
    let y = popup.y + u16::try_from(header_row)?;
    for x in popup.x + 1..=popup.x + popup.width - 2 {
        assert_eq!(
            driver.cell_style_at(x, y).bg,
            Some(theme.base),
            "the day header must not paint a band at x={x}\n{popup_text}"
        );
    }
    let header_col = lines[header_row]
        .chars()
        .position(|c| !matches!(c, '│' | ' '))
        .ok_or("the day header row should carry text")?;
    let session_col = lines[session_row]
        .chars()
        .position(|c| !matches!(c, '│' | ' ' | '❯'))
        .ok_or("the session row should carry text")?;
    assert_eq!(
        header_col, 3,
        "the day head must sit flush-left on the When column; column: {header_col}\n{popup_text}"
    );
    assert_eq!(
        session_col, 5,
        "the session rows must indent two cells under their head; column: {session_col}\n{popup_text}"
    );
    assert_eq!(
        session_row,
        header_row + 1,
        "no blank separator row may sit between a head and its rows; head: {header_row}, session: {session_row}\n{popup_text}"
    );
    let header_x = popup.x + u16::try_from(header_col)?;
    let header_style = driver.cell_style_at(header_x, y);
    assert_eq!(
        header_style.fg,
        Some(palette.subtext0),
        "the day header text must render in subtext0\n{popup_text}"
    );
    assert!(
        header_style
            .add_modifier
            .contains(ratatui::style::Modifier::BOLD),
        "the day header text must render bold\n{popup_text}"
    );
    Ok(())
}

/// The session rows ellipsize their title to the table's Title column, and the
/// day head above them keeps its full label.
#[tokio::test]
async fn test_session_picker_ellipsizes_a_long_title_under_its_head() -> Result<()> {
    // -- Setup & Fixtures
    let title = "t".repeat(60);
    let now = chrono::Utc::now();
    let options = vec![session_option("id0", &title, now, 1)];
    let mut driver = RenderLoopDriver::new(58, 24);
    driver
        .coordinator_mut()
        .state
        .set_picker_options(ActivePicker::Session, options);
    driver
        .coordinator_mut()
        .state
        .picker
        .open(ActivePicker::Session);

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check: the table width is 56, so the Title column takes 31 cells and the
    // row keeps 30 of them plus the ellipsis marker.
    let popup = super::render::frame::modal_rect_for_panel(
        driver.state().scroll.transcript_area,
        super::render::frame::ModalPanelKind::Sessions,
    );
    let popup_text = driver.buffer_text_in_rect(popup);
    let clipped = format!("{}…", "t".repeat(30));
    assert!(
        popup_text.contains(&clipped),
        "the table row must carry the ellipsized title; expected {clipped:?}\n{popup_text}"
    );
    let head_row = popup_text
        .lines()
        .position(|line| line.contains("Today"))
        .ok_or("should render the day head")?;
    let row = popup_text
        .lines()
        .position(|line| line.contains(&clipped))
        .ok_or("should render the ellipsized session row")?;
    assert_eq!(
        row,
        head_row + 1,
        "the session row must sit directly under its head; head: {head_row}, row: {row}\n{popup_text}"
    );
    Ok(())
}

/// At 56x20 the footer text starts on the When column, so the popup has one left
/// edge for its rows and its footer, and it paints no background band.
#[tokio::test]
async fn test_session_picker_footer_renders_fg_only_text_on_the_when_column() -> Result<()> {
    // -- Setup & Fixtures
    let title = "review the picker footer wrapping behaviour";
    let now = chrono::Utc::now();
    let options = (0..3)
        .map(|i| session_option(&format!("id{i}"), title, now, i + 1))
        .collect::<Vec<_>>();
    let theme = TuiTheme::default();
    let mut driver = RenderLoopDriver::new(58, 24);
    driver
        .coordinator_mut()
        .state
        .set_picker_options(ActivePicker::Session, options);
    driver
        .coordinator_mut()
        .state
        .picker
        .open(ActivePicker::Session);

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check: popup 56x20 at the origin, so the one-row title footer sits on
    // popup row 18. The When column is popup column 3, and the footer text starts
    // there with no band around it.
    let popup = super::render::frame::modal_rect_for_panel(
        driver.state().scroll.transcript_area,
        super::render::frame::ModalPanelKind::Sessions,
    );
    let popup_text = driver.buffer_text_in_rect(popup);
    let lines: Vec<&str> = popup_text.lines().collect();
    let footer_row = popup.y + popup.height - 2;
    let leading = driver.cell_style_at(popup.x + 2, footer_row);
    let text_style = driver.cell_style_at(popup.x + 3, footer_row);
    assert_eq!(
        lines[18].chars().nth(3),
        Some('r'),
        "the first text glyph must sit on the When column; row: {:?}\n{popup_text}",
        lines[18]
    );
    assert_eq!(
        text_style.fg, theme.subtle_meta.fg,
        "the footer text must carry the meta foreground; style: {text_style:?}"
    );
    assert!(
        !text_style
            .add_modifier
            .contains(ratatui::style::Modifier::BOLD),
        "the footer text must not be bold; style: {text_style:?}"
    );
    assert_eq!(
        text_style.bg,
        Some(theme.base),
        "the footer text must sit on the popup base, not a band; style: {text_style:?}"
    );
    assert_eq!(
        leading.bg,
        Some(theme.base),
        "no band may start before the footer text; style: {leading:?}"
    );
    assert!(
        lines[18].contains(title),
        "the footer must still carry the whole title; row: {:?}\n{popup_text}",
        lines[18]
    );
    Ok(())
}

// region:    --- Test Support

fn session_option(
    id: &str,
    title: &str,
    created_at: chrono::DateTime<chrono::Utc>,
    message_count: usize,
) -> PickerOption {
    let display = format!("{title} ({id})");
    PickerOption {
        id: id.to_string(),
        display: display.clone(),
        search_text: display,
        section: Some(crate::state::session_day(created_at).to_string()),
        sort_key: vec![PickerSortKeyPart::Recent(std::cmp::Reverse(created_at))],
        payload: PickerPayload::Session {
            session_id: id.to_string(),
            title: Some(title.to_string()),
            created_at,
            message_count,
        },
    }
}

// endregion: --- Test Support
