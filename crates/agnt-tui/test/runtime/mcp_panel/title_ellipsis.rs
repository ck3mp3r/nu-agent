use unicode_width::UnicodeWidthStr;

use super::*;

/// A cue of the shape the overflow path builds, wide enough to pass the title
/// row at the popup's narrower widths.
const LONG_CUE: &str = "↑/↓ select · Enter/Space toggle · Esc close · 1-9 / 30";

/// At the popup's minimum width the overflow cue pushes the title past the
/// title row, so the title ellipsizes to that row with a trailing marker
/// instead of hard-clipping at the frame border.
#[tokio::test]
async fn mcp_panel_title_ellipsizes_overflow_cue_at_minimum_popup_width() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(58, 20);
    driver
        .coordinator_mut()
        .state
        .status
        .mcp
        .set_mcp_servers(overflowing_servers(30));
    driver
        .coordinator_mut()
        .state
        .open_info_panel(crate::state::InfoPanel::Mcps);

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check: the title row spans the popup width minus the two border corners.
    let popup = super::render::frame::modal_rect_for_panel(
        driver.state().scroll.transcript_area,
        super::render::frame::ModalPanelKind::Mcps,
    );
    assert_eq!(
        popup.width, 56,
        "the MCP popup must render at the 56-column minimum"
    );
    let title = title_row_text(&driver.buffer_text_in_rect(popup))?;
    assert_eq!(
        UnicodeWidthStr::width(title.as_str()),
        54,
        "the title must fill the 54-cell title row; title: {title:?}"
    );
    assert!(
        title.ends_with('…'),
        "the title must ellipsize with a marker; title: {title:?}"
    );
    assert!(
        title.starts_with("MCPs ("),
        "the ellipsized title must keep the MCPs label; title: {title:?}"
    );
    Ok(())
}

/// At the popup's maximum width the same overflow cue title fits the title row,
/// so the title renders whole with no marker.
#[tokio::test]
async fn mcp_panel_title_renders_whole_at_maximum_popup_width() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(74, 20);
    driver
        .coordinator_mut()
        .state
        .status
        .mcp
        .set_mcp_servers(overflowing_servers(30));
    driver
        .coordinator_mut()
        .state
        .open_info_panel(crate::state::InfoPanel::Mcps);

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check: the title carries no clipping marker and closes its cue.
    let popup = super::render::frame::modal_rect_for_panel(
        driver.state().scroll.transcript_area,
        super::render::frame::ModalPanelKind::Mcps,
    );
    assert_eq!(
        popup.width, 72,
        "the MCP popup must widen with a 72-column terminal"
    );
    let title = title_row_text(&driver.buffer_text_in_rect(popup))?;
    assert!(
        !title.contains('…'),
        "a fitting title must carry no marker; title: {title:?}"
    );
    assert!(
        title.starts_with("MCPs (") && title.ends_with(')'),
        "a fitting title must render whole inside its parentheses; title: {title:?}"
    );
    Ok(())
}

/// A cue that fits the title row renders verbatim with no marker.
#[test]
fn mcp_panel_title_renders_fitting_cue_verbatim() -> Result<()> {
    // -- Exec
    let title = mcp_panel_title(Some("1-2 / 3"), 72);

    // -- Check
    assert_eq!(title, "MCPs (1-2 / 3)");
    Ok(())
}

/// Without an overflow cue the title is the bare panel label at any width.
#[test]
fn mcp_panel_title_without_cue_is_bare_label() -> Result<()> {
    // -- Exec
    let title = mcp_panel_title(None, 56);

    // -- Check
    assert_eq!(title, "MCPs");
    Ok(())
}

/// The title never exceeds the title row across the popup's whole width range:
/// it truncates to the row while it overflows and renders whole once it fits.
#[test]
fn mcp_panel_title_fits_title_row_at_every_popup_width() -> Result<()> {
    // -- Setup & Fixtures: the unbounded title the renderer would clip.
    let unbounded = format!("MCPs ({LONG_CUE})");
    let unbounded_width = UnicodeWidthStr::width(unbounded.as_str());

    // -- Exec & Check: popup widths 56..=72 span the popup's whole range.
    for popup_width in 56u16..=72 {
        let title = mcp_panel_title(Some(LONG_CUE), popup_width);
        let row_width = usize::from(popup_width.saturating_sub(2));
        let width = UnicodeWidthStr::width(title.as_str());
        assert_eq!(
            width,
            unbounded_width.min(row_width),
            "the title must fill the {row_width}-cell title row at popup width {popup_width}; \
             title: {title:?}"
        );
        if unbounded_width > row_width {
            assert!(
                title.ends_with('…'),
                "the overflowing title must truncate at popup width {popup_width}; \
                 title: {title:?}"
            );
        } else {
            assert_eq!(
                title, unbounded,
                "the fitting title must render verbatim at popup width {popup_width}"
            );
        }
    }
    Ok(())
}

// region:    --- Support

/// More MCP servers than the popup table window holds, so the overflow cue is
/// present.
fn overflowing_servers(count: usize) -> Vec<crate::state::McpServerState> {
    (0..count)
        .map(|idx| crate::state::McpServerState {
            name: format!("srv-{idx}"),
            state: McpServerUsabilityState::Enabled,
        })
        .collect()
}

/// The popup's title row as plain text, stripped of the frame border corners
/// and of the border fill that trails a short title.
fn title_row_text(popup_text: &str) -> Result<String> {
    let row = popup_text
        .lines()
        .next()
        .ok_or("should render the title row")?;
    let inner = row
        .chars()
        .skip(1)
        .take(row.chars().count().saturating_sub(2))
        .collect::<String>();
    Ok(inner.trim_end_matches(['─', ' ']).to_string())
}

// endregion: --- Support
