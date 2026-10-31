//! Theme threading: every surface the widget paints resolves from the theme the
//! caller passes, never from a default.

use super::support::*;
use agnt_core::transcript::ir::MessageRole;
use agnt_core::transcript::renderer::ItemStatus;

/// WHEN the renderer receives a caller-provided theme, THE diff tint SHALL
/// resolve from that theme.
#[test]
fn test_renderer_theme_threading_diff_tint_uses_the_caller_theme() -> Result<()> {
    // -- Setup & Fixtures
    let caller = alt_theme();
    let entry = tool_entry(
        "edit",
        "→ a.rs",
        Some(diff_display(SIMPLE_DIFF)),
        None,
        ItemStatus::Done,
    );

    // -- Exec
    let (backend, _) = render_at_with(&entry, 0, &caller)?;
    let added = row_index_containing(&backend, "new").ok_or("the added row must render")?;

    // -- Check
    assert_eq!(
        cell_bg(&backend, 0, added),
        Some(terminal(caller.diff_add_bg)),
        "the added row must carry the caller's tint"
    );
    assert_ne!(
        cell_bg(&backend, 0, added),
        Some(terminal(test_theme().diff_add_bg)),
        "the fixture theme must not leak through"
    );
    Ok(())
}

/// WHEN the renderer receives a caller-provided theme, THE lane surfaces SHALL
/// resolve from that theme.
#[test]
fn test_renderer_theme_threading_lane_surface_uses_the_caller_theme() -> Result<()> {
    // -- Setup & Fixtures
    let caller = alt_theme();
    let entry = message_entry(MessageRole::User, "hi");

    // -- Exec
    let (backend, _) = render_at_with(&entry, 0, &caller)?;

    // -- Check
    assert_eq!(
        cell_bg(&backend, 0, 0),
        Some(terminal(caller.user_surface)),
        "the user row must carry the caller's surface"
    );
    assert_eq!(
        cell_fg(&backend, 2, 0),
        Some(terminal(caller.text)),
        "the user text must carry the caller's body color"
    );
    Ok(())
}

/// WHEN one entry is painted under two themes, THE painted row count SHALL stay
/// the same — the row budget is the theme's business no more than the layout's.
#[test]
fn test_renderer_theme_threading_painted_rows_are_theme_independent() -> Result<()> {
    // -- Setup & Fixtures
    let entry = tool_entry(
        "nu",
        "→ ls",
        Some(code_display("nu", "alpha\nbeta\ngamma")),
        None,
        ItemStatus::Done,
    );

    // -- Exec
    let (first, rows_first) = render_at_with(&entry, 0, &test_theme())?;
    let (second, rows_second) = render_at_with(&entry, 0, &alt_theme())?;

    // -- Check
    assert_eq!(rows_first, rows_second, "the measured rows must not move");
    assert_eq!(
        painted_rows(&first),
        painted_rows(&second),
        "the painted rows must not move with the theme"
    );
    Ok(())
}
