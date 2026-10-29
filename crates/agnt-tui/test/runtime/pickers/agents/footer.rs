//! Agent picker footer: the pin to the popup's bottom edge, the fg-only text,
//! the gap row, the wrap, and the alignment with the table column.

use super::*;

use crate::rendering::theme::TuiTheme;

/// At 120x30 the popup inner height is 18, so the details ladder caps the
/// footer at three rows. A one-row footer text sits on the bottom inner row,
/// directly above the bottom border, with no reserved blank rows.
#[tokio::test]
async fn test_agent_picker_footer_pins_to_bottom_edge() -> Result<()> {
    // -- Setup & Fixtures
    let footer_text = "reviewer — Runs reviews";
    let mut driver = RenderLoopDriver::new(120, 30);
    driver.coordinator_mut().state.set_picker_options(
        ActivePicker::Agent,
        vec![agnt_core::protocol::picker::AgentPickerOption {
            name: "reviewer".to_string(),
            description: Some("Runs reviews".to_string()),
            display: "reviewer".to_string(),
            builtin: false,
        }],
    );
    driver
        .coordinator_mut()
        .state
        .picker
        .open(ActivePicker::Agent);

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check: popup 90x20, inner 88x18. The footer text is one row at width
    // 88, so it renders on popup row 18 — the last row before the border at 19.
    let popup = super::render::frame::modal_rect_for_panel(
        driver.state().scroll.transcript_area,
        super::render::frame::ModalPanelKind::Agents,
    );
    let popup_text = driver.buffer_text_in_rect(popup);
    let lines: Vec<&str> = popup_text.lines().collect();
    assert!(
        lines[18].contains(footer_text),
        "the footer must sit on the bottom inner row (18); row: {:?}\n{popup_text}",
        lines[18]
    );
    assert!(
        !lines[16].contains(footer_text) && !lines[17].contains(footer_text),
        "no reserved blank rows may sit between the footer and the border; rows: {:?} {:?}\n{popup_text}",
        lines[16],
        lines[17]
    );
    Ok(())
}

/// The agent footer paints foreground-only text on the popup base, starting on
/// the column the agent rows use, so the footer never paints a band.
#[tokio::test]
async fn test_agent_picker_footer_renders_fg_only_text() -> Result<()> {
    // -- Setup & Fixtures
    let theme = TuiTheme::default();
    let mut driver = RenderLoopDriver::new(120, 30);
    driver.coordinator_mut().state.set_picker_options(
        ActivePicker::Agent,
        vec![agnt_core::protocol::picker::AgentPickerOption {
            name: "reviewer".to_string(),
            description: Some("Runs reviews".to_string()),
            display: "reviewer".to_string(),
            builtin: false,
        }],
    );
    driver
        .coordinator_mut()
        .state
        .picker
        .open(ActivePicker::Agent);

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check: popup 90x20 offset inside the 120x30 area, so the one-row footer
    // sits on the popup's last inner row. The agent rows start on popup column 3,
    // and the footer text starts there too.
    let popup = super::render::frame::modal_rect_for_panel(
        driver.state().scroll.transcript_area,
        super::render::frame::ModalPanelKind::Agents,
    );
    let footer_row = popup.y + popup.height - 2;
    let leading_style = driver.cell_style_at(popup.x + 2, footer_row);
    let text_style = driver.cell_style_at(popup.x + 3, footer_row);
    let row_bg = driver.cell_style_at(popup.x + 5, footer_row - 1).bg;
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
        leading_style.bg,
        Some(theme.base),
        "no band may start before the footer text; style: {leading_style:?}"
    );
    assert_eq!(
        row_bg,
        Some(theme.base),
        "the row above the footer must keep the popup base"
    );
    Ok(())
}

/// At a popup inner height of 18 the footer reserves three rows. The selected
/// agent's name and description render there in full even when the description
/// cell clips.
#[tokio::test]
async fn test_agent_picker_footer_shows_full_name_and_description() -> Result<()> {
    // -- Setup & Fixtures
    let name = "reviewer".to_string();
    let description = "d".repeat(50);
    let mut driver = RenderLoopDriver::new(120, 30);
    driver.coordinator_mut().state.set_picker_options(
        ActivePicker::Agent,
        vec![agnt_core::protocol::picker::AgentPickerOption {
            name: name.clone(),
            description: Some(description.clone()),
            display: name.clone(),
            builtin: false,
        }],
    );
    driver
        .coordinator_mut()
        .state
        .picker
        .open(ActivePicker::Agent);

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check: popup 90x20, inner 88x18, footer 3 rows. The column budget is
    // (88 - 5) / 2 = 41, so the 50-char description clips in the cell and the
    // footer carries it in full.
    let popup = super::render::frame::modal_rect_for_panel(
        driver.state().scroll.transcript_area,
        super::render::frame::ModalPanelKind::Agents,
    );
    let popup_text = driver.buffer_text_in_rect(popup);
    let lines: Vec<&str> = popup_text.lines().collect();
    let clipped = format!("{}…", "d".repeat(40));
    let table_idx = lines
        .iter()
        .position(|line| line.contains(&clipped))
        .ok_or("should render the ellipsized description in the table")?;
    let footer_idx = lines
        .iter()
        .position(|line| line.contains(&description))
        .ok_or("should render the full description in the footer")?;
    assert!(
        table_idx < footer_idx,
        "the table row must sit above the footer; table: {table_idx}, footer: {footer_idx}\n{popup_text}"
    );
    assert!(
        lines[footer_idx].contains(&name) && lines[footer_idx].contains(" — "),
        "the footer must carry the name and separator; row: {:?}\n{popup_text}",
        lines[footer_idx]
    );
    Ok(())
}

/// At terminal width 45 the popup is 45 wide, so the table is 43 wide and the
/// solver pins the Description column at its 20-column minimum, leaving the
/// Agent column 18. A 29-char name ellipsized against a stale 19-cell budget
/// clips at 18 and loses the marker; the derived budget keeps it.
#[tokio::test]
async fn test_agent_picker_name_cell_keeps_marker_at_width_45() -> Result<()> {
    // -- Setup & Fixtures
    let name = "code-reviewer-extended-name".to_string();
    let mut driver = RenderLoopDriver::new(47, 24);
    driver.coordinator_mut().state.set_picker_options(
        ActivePicker::Agent,
        vec![agnt_core::protocol::picker::AgentPickerOption {
            name: name.clone(),
            description: Some("d".repeat(40)),
            display: name.clone(),
            builtin: false,
        }],
    );
    driver
        .coordinator_mut()
        .state
        .picker
        .open(ActivePicker::Agent);

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check
    let popup = super::render::frame::modal_rect_for_panel(
        driver.state().scroll.transcript_area,
        super::render::frame::ModalPanelKind::Agents,
    );
    let popup_text = driver.buffer_text_in_rect(popup);
    let row = popup_text
        .lines()
        .find(|line| line.contains("code-reviewer"))
        .ok_or("should render the long-name agent row")?;
    assert!(
        row.contains("code-reviewer-ext…"),
        "the 18-wide name cell should hold the 18-cell ellipsized name; row: {row:?}\n{popup_text}"
    );
    assert!(
        row.matches('…').count() >= 2,
        "the row should carry a name marker and a description marker; row: {row:?}\n{popup_text}"
    );
    Ok(())
}

/// At 120x30 a twenty-agent list fills the table area, so the last data row
/// would sit flush against the footer text. The footer text takes the bottom
/// inner row and the row directly above it stays blank base background.
#[tokio::test]
async fn test_agent_picker_footer_leaves_a_gap_row_above_the_text() -> Result<()> {
    // -- Setup & Fixtures
    let theme = TuiTheme::default();
    let options = (0..20)
        .map(|idx| agnt_core::protocol::picker::AgentPickerOption {
            name: format!("agent-{idx}"),
            description: Some(format!("description {idx}")),
            display: format!("agent-{idx}"),
            builtin: false,
        })
        .collect::<Vec<_>>();
    let footer_text = "agent-0 — description 0";
    let mut driver = RenderLoopDriver::new(120, 30);
    driver
        .coordinator_mut()
        .state
        .set_picker_options(ActivePicker::Agent, options);
    driver
        .coordinator_mut()
        .state
        .picker
        .open(ActivePicker::Agent);

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check: popup 90x20 → inner 88x18 → the one-row footer fills one text
    // row, so the footer claims rows 17 and 18: row 17 is the blank gap and row
    // 18 carries the footer text on the bottom inner row.
    let popup = super::render::frame::modal_rect_for_panel(
        driver.state().scroll.transcript_area,
        super::render::frame::ModalPanelKind::Agents,
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
        lines[18].contains(footer_text),
        "the footer text must still sit on the bottom inner row (18); row: {:?}\n{popup_text}",
        lines[18]
    );
    Ok(())
}

/// At terminal 90x10 the popup inner height is 8, so the details ladder
/// reserves two footer rows. The selected agent's footer text is wider than
/// the inner width, and both wrapped rows render with no characters dropped.
#[tokio::test]
async fn test_agent_picker_footer_wraps_to_two_rows_at_inner_height_eight() -> Result<()> {
    // -- Setup & Fixtures
    let name = "reviewer".to_string();
    // A single-spaced 99-cell description: 15 words fit beside the name on the
    // first footer row and the remaining 5 flow onto the second.
    let description = vec!["dddd"; 20].join(" ");
    let mut driver = RenderLoopDriver::new(92, 14);
    driver.coordinator_mut().state.set_picker_options(
        ActivePicker::Agent,
        vec![agnt_core::protocol::picker::AgentPickerOption {
            name: name.clone(),
            description: Some(description.clone()),
            display: name.clone(),
            builtin: false,
        }],
    );
    driver
        .coordinator_mut()
        .state
        .picker
        .open(ActivePicker::Agent);

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check: popup 90x10, inner 88x8. The details ladder grants two rows at
    // popup rows 7 and 8, so the footer text wraps at width 88 across both.
    let popup = super::render::frame::modal_rect_for_panel(
        driver.state().scroll.transcript_area,
        super::render::frame::ModalPanelKind::Agents,
    );
    let popup_text = driver.buffer_text_in_rect(popup);
    let lines: Vec<&str> = popup_text.lines().collect();
    let footer_rows: Vec<String> = lines
        .iter()
        .skip(7)
        .take(2)
        .map(|line| line.replace('│', ""))
        .collect();
    assert_eq!(footer_rows.len(), 2, "popup must expose both footer rows");
    let rendered = footer_rows
        .iter()
        .flat_map(|row| row.split_whitespace())
        .collect::<Vec<_>>()
        .join(" ");
    assert_eq!(
        rendered,
        format!("{name} — {description}"),
        "the two footer rows must carry the whole footer text; rows: {footer_rows:?}\n{popup_text}"
    );
    for row in &footer_rows {
        assert!(
            !row.trim().is_empty(),
            "both footer rows must render text; row: {row:?}\n{popup_text}"
        );
        assert!(
            !row.contains('…'),
            "the footer must not truncate with a marker; row: {row:?}\n{popup_text}"
        );
    }
    Ok(())
}

/// At 120x30 the footer text starts on the same column as the agent rows, so the
/// popup has one left edge for its data and its footer.
#[tokio::test]
async fn test_agent_picker_footer_aligns_the_text_with_the_table_column() -> Result<()> {
    // -- Setup & Fixtures
    let footer_text = "reviewer — Runs reviews";
    let theme = TuiTheme::default();
    let mut driver = RenderLoopDriver::new(120, 30);
    driver.coordinator_mut().state.set_picker_options(
        ActivePicker::Agent,
        vec![agnt_core::protocol::picker::AgentPickerOption {
            name: "reviewer".to_string(),
            description: Some("Runs reviews".to_string()),
            display: "reviewer".to_string(),
            builtin: false,
        }],
    );
    driver
        .coordinator_mut()
        .state
        .picker
        .open(ActivePicker::Agent);

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check: popup 90x20 offset inside the 120x30 area, so the one-row footer
    // sits on the popup's last inner row. The agent rows start on popup column 3,
    // and the footer text starts there too.
    let popup = super::render::frame::modal_rect_for_panel(
        driver.state().scroll.transcript_area,
        super::render::frame::ModalPanelKind::Agents,
    );
    let popup_text = driver.buffer_text_in_rect(popup);
    let lines: Vec<&str> = popup_text.lines().collect();
    let table_row = lines
        .iter()
        .position(|line| line.contains("reviewer"))
        .ok_or("should render the agent row")?;
    let table_at = lines[table_row]
        .find("reviewer")
        .ok_or("the agent row should contain the name")?;
    let table_col = lines[table_row][..table_at].chars().count();
    let footer_row = lines
        .iter()
        .rposition(|line| line.contains(footer_text))
        .ok_or("should render the footer text")?;
    let footer_at = lines[footer_row]
        .find("reviewer")
        .ok_or("the footer row should contain the name")?;
    let footer_col = lines[footer_row][..footer_at].chars().count();
    assert_eq!(
        footer_col, table_col,
        "the footer must start on the agent column; table: {table_col}, footer: {footer_col}\n{popup_text}"
    );
    assert_eq!(
        footer_col, 3,
        "the agent column is the border, the inner edge, and the two-cell gutter; column: {footer_col}\n{popup_text}"
    );
    let leading = driver.cell_style_at(popup.x + 2, popup.y + popup.height - 2);
    assert_eq!(
        leading.bg,
        Some(theme.base),
        "no band may start before the footer text; style: {leading:?}"
    );
    Ok(())
}
