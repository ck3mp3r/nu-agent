//! Banner widget tests.

use super::support::*;

/// WHEN the banner widget renders, THE row SHALL center the gutter and the text
/// together and paint the banner foreground with no surface.
#[test]
fn test_widgets_banner_centers_and_paints_text() -> Result<()> {
    // -- Setup & Fixtures
    let entry = banner_entry("nu-agent");
    let theme = test_theme();
    // 40 columns, 4 gutter columns, 8 text columns: 28 spare, 14 left.
    let expected = format!("{}nu-agent", " ".repeat(18));

    // -- Exec
    let (backend, rows) = render_at(&entry, 0)?;

    // -- Check
    assert_eq!(rows, 1);
    assert_eq!(row_text(&backend, 0).trim_end(), expected);
    assert!(is_untinted(cell_bg(&backend, 0, 0)));
    assert_eq!(cell_fg(&backend, 18, 0), Some(terminal(theme.banner)));
    Ok(())
}
