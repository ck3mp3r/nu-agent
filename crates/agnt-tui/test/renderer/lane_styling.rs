//! Lane styling: which semantic surface each lane's text and marker paint.
//!
//! The lane decides the styling, and the entry family decides the lane, so every
//! assertion here names one lane and one semantic color.

use super::support::*;
use agnt_core::transcript::ir::{MessageRole, NoticeKind};
use agnt_core::transcript::renderer::ItemStatus;

// region:    --- Text surfaces

/// WHEN a banner renders, THE text SHALL paint the banner color, not the body
/// color.
#[test]
fn test_renderer_lane_styling_banner_text_paints_the_banner_color() -> Result<()> {
    // -- Setup & Fixtures
    let theme = test_theme();
    let entry = banner_entry("NUAGENT");

    // -- Exec
    let (backend, _) = render_at(&entry, 0)?;
    let column = u16::try_from(
        row_text(&backend, 0)
            .find("NUAGENT")
            .ok_or("the banner must render")?,
    )?;

    // -- Check
    assert_eq!(
        cell_fg(&backend, column, 0),
        Some(terminal(theme.banner)),
        "the banner paints the banner color"
    );
    assert_ne!(
        cell_fg(&backend, column, 0),
        Some(terminal(theme.text)),
        "the banner must not paint the body color"
    );
    Ok(())
}

/// WHEN a system notice renders, THE text SHALL paint the notice color.
#[test]
fn test_renderer_lane_styling_system_notice_paints_the_notice_color() -> Result<()> {
    // -- Setup & Fixtures
    let theme = test_theme();
    let entry = notice_entry(NoticeKind::System, "system notice text");

    // -- Exec
    let (backend, _) = render_at(&entry, 0)?;

    // -- Check
    assert_eq!(
        cell_fg(&backend, 4, 0),
        Some(terminal(theme.notice)),
        "a system notice paints the notice color"
    );
    assert_ne!(
        cell_fg(&backend, 4, 0),
        Some(terminal(theme.muted)),
        "a system notice must not paint the muted color"
    );
    Ok(())
}

/// WHEN a compaction notice renders, THE text SHALL paint the muted color.
#[test]
fn test_renderer_lane_styling_compaction_notice_paints_the_muted_color() -> Result<()> {
    // -- Setup & Fixtures
    let theme = test_theme();
    let entry = notice_entry(NoticeKind::Compaction, "compaction notice text");

    // -- Exec
    let (backend, _) = render_at(&entry, 0)?;

    // -- Check
    assert_eq!(
        cell_fg(&backend, 4, 0),
        Some(terminal(theme.muted)),
        "a compaction notice paints the muted color"
    );
    assert_ne!(
        cell_fg(&backend, 4, 0),
        Some(terminal(theme.notice)),
        "a compaction notice must not paint the notice color"
    );
    Ok(())
}

/// WHEN assistant prose renders, THE text SHALL paint the body color and no
/// surface.
#[test]
fn test_renderer_lane_styling_assistant_prose_paints_the_body_color() -> Result<()> {
    // -- Setup & Fixtures
    let theme = test_theme();
    let entry = message_entry(MessageRole::Assistant, "hello world");

    // -- Exec
    let (backend, _) = render_at(&entry, 0)?;

    // -- Check
    assert_eq!(
        cell_fg(&backend, 2, 0),
        Some(terminal(theme.text)),
        "assistant prose paints the body color"
    );
    assert!(
        is_untinted(cell_bg(&backend, 2, 0)),
        "assistant prose carries no surface"
    );
    Ok(())
}

/// WHEN user prose renders, THE text SHALL paint the body color over the user
/// surface.
#[test]
fn test_renderer_lane_styling_user_prose_paints_body_on_the_user_surface() -> Result<()> {
    // -- Setup & Fixtures
    let theme = test_theme();
    let entry = message_entry(MessageRole::User, "hi");

    // -- Exec
    let (backend, _) = render_at(&entry, 0)?;

    // -- Check
    assert_eq!(
        cell_fg(&backend, 2, 0),
        Some(terminal(theme.text)),
        "user prose paints the body color"
    );
    assert_eq!(
        cell_bg(&backend, 2, 0),
        Some(terminal(theme.user_surface)),
        "user prose paints the user surface"
    );
    Ok(())
}

// endregion: --- Text surfaces

// region:    --- Marker surfaces

/// WHEN the marker lanes render, EACH marker SHALL paint its lane accent: the
/// tool marker the tool accent, the user marker the user lane accent, the
/// compaction and assistant markers the muted color, the notice markers the
/// notice color.
#[test]
fn test_renderer_lane_styling_markers_paint_their_lane_accent() -> Result<()> {
    // -- Setup & Fixtures
    let theme = test_theme();
    let cases = [
        (bare_tool(ItemStatus::Done), 2u16, theme.accent),
        (message_entry(MessageRole::User, "hi"), 0, theme.user_accent),
        (notice_entry(NoticeKind::Compaction, "c"), 2, theme.muted),
        (notice_entry(NoticeKind::System, "s"), 2, theme.notice),
    ];

    // -- Exec & Check
    for (entry, column, expected) in cases {
        let (backend, _) = render_at(&entry, 0)?;
        assert_eq!(
            cell_fg(&backend, column, 0),
            Some(terminal(expected)),
            "the marker at column {column} must paint its lane accent"
        );
    }
    Ok(())
}

// endregion: --- Marker surfaces

// region:    --- Centering

/// WHEN a banner renders next to a prose entry, THE banner SHALL center while the
/// prose SHALL start at its gutter — centering follows the lane, not the payload.
#[test]
fn test_renderer_lane_styling_centering_follows_the_lane() -> Result<()> {
    // -- Setup & Fixtures
    let centered = render(&banner_entry("CENTERME"))?;
    let flush = render(&message_entry(MessageRole::Assistant, "CENTERME"))?;

    // -- Exec
    let centered_column = row_text(&centered, 0)
        .find("CENTERME")
        .ok_or("the banner must render")?;
    let flush_column = row_text(&flush, 0)
        .find("CENTERME")
        .ok_or("the prose must render")?;

    // -- Check
    assert!(
        centered_column > flush_column,
        "the banner centers ({centered_column}) while the prose stays flush ({flush_column})"
    );
    assert_eq!(flush_column, 2, "the prose starts at its two-column gutter");
    Ok(())
}

// endregion: --- Centering
