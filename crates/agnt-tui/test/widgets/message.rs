use super::support::*;
use agnt_core::transcript::ir::MessageRole;

// ---------------------------------------------------------------------------
// User prose
// ---------------------------------------------------------------------------

/// WHEN the message widget renders a user turn, THE row SHALL carry the user
/// surface across the whole pane and the rail SHALL sit in the gutter.
#[test]
fn test_widgets_message_user_paints_surface_and_rail() -> Result<()> {
    // -- Setup & Fixtures
    let entry = message_entry(MessageRole::User, "Hello there");
    let theme = test_theme();

    // -- Exec
    let backend = render(&entry)?;

    // -- Check
    assert_eq!(row_text(&backend, 0).trim_end(), "╎ Hello there");
    for x in [0u16, 1, 10, WIDTH - 1] {
        assert_eq!(
            cell_bg(&backend, x, 0),
            Some(terminal(theme.user_surface)),
            "the user surface covers column {x}"
        );
    }
    assert_eq!(cell_fg(&backend, 2, 0), Some(terminal(theme.text)));
    Ok(())
}

/// WHEN a user turn wraps, THE rail SHALL stay in the gutter on every row and
/// the user surface SHALL cover every row.
#[test]
fn test_widgets_message_user_rail_survives_wrapped_rows() -> Result<()> {
    // -- Setup & Fixtures
    let markdown = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu";
    let entry = message_entry(MessageRole::User, markdown);
    let theme = test_theme();

    // -- Exec
    let (backend, rows) = render_at(&entry, 0)?;

    // -- Check
    assert!(rows > 1, "the fixture must wrap, got {rows} rows");
    for y in 0..u16::try_from(rows)? {
        assert!(
            row_text(&backend, y).starts_with("╎ "),
            "row {y} keeps the rail: {:?}",
            row_text(&backend, y)
        );
        assert_eq!(
            cell_bg(&backend, WIDTH - 1, y),
            Some(terminal(theme.user_surface)),
            "row {y} keeps the user surface to the pane edge"
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Assistant prose
// ---------------------------------------------------------------------------

/// WHEN the message widget renders an assistant turn, THE row SHALL carry a
/// blank gutter and plain text with no surface background.
#[test]
fn test_widgets_message_assistant_is_untinted_prose() -> Result<()> {
    // -- Setup & Fixtures
    let entry = message_entry(MessageRole::Assistant, "Hello there");
    let theme = test_theme();

    // -- Exec
    let backend = render(&entry)?;

    // -- Check
    assert_eq!(row_text(&backend, 0).trim_end(), "  Hello there");
    assert!(
        is_untinted(cell_bg(&backend, 0, 0)),
        "assistant prose is untinted"
    );
    assert!(is_untinted(cell_bg(&backend, WIDTH - 1, 0)));
    assert_eq!(cell_fg(&backend, 2, 0), Some(terminal(theme.text)));
    Ok(())
}
