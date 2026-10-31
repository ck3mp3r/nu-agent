//! Diff line tints: the add/remove surfaces that override the code surface, and
//! the foregrounds they keep on top.

use super::support::*;
use agnt_core::transcript::renderer::ItemStatus;

// region:    --- Fixtures

/// A tool entry whose preview is the one-hunk diff.
fn diff_entry() -> agnt_core::transcript::entry::TranscriptEntry {
    tool_entry(
        "edit",
        "→ a.rs",
        Some(diff_display(SIMPLE_DIFF)),
        None,
        ItemStatus::Done,
    )
}

// endregion: --- Fixtures

/// WHEN a diff line carries `DiffTint::Add`, THE row SHALL paint the add surface
/// across the pane, gutter included.
#[test]
fn test_renderer_diff_tint_add_line_paints_the_add_surface() -> Result<()> {
    // -- Setup & Fixtures
    let theme = test_theme();
    let entry = diff_entry();

    // -- Exec
    let backend = render(&entry)?;
    let added = row_index_containing(&backend, "new").ok_or("the added row must render")?;

    // -- Check
    for x in [0u16, 1, WIDTH / 2, WIDTH - 1] {
        assert_eq!(
            cell_bg(&backend, x, added),
            Some(terminal(theme.diff_add_bg)),
            "the added row paints its tint at column {x}"
        );
    }
    assert!(
        row_text(&backend, added).contains("\u{2502}new"),
        "the added row keeps its line-number gutter; got {:?}",
        row_trimmed(&backend, added)
    );
    Ok(())
}

/// WHEN a diff line carries `DiffTint::Remove`, THE row SHALL paint the remove
/// surface across the pane.
#[test]
fn test_renderer_diff_tint_remove_line_paints_the_remove_surface() -> Result<()> {
    // -- Setup & Fixtures
    let theme = test_theme();
    let entry = diff_entry();

    // -- Exec
    let backend = render(&entry)?;
    let removed = row_index_containing(&backend, "old").ok_or("the removed row must render")?;

    // -- Check
    for x in [0u16, 1, WIDTH / 2, WIDTH - 1] {
        assert_eq!(
            cell_bg(&backend, x, removed),
            Some(terminal(theme.diff_remove_bg)),
            "the removed row paints its tint at column {x}"
        );
    }
    Ok(())
}

/// WHEN a diff line carries `DiffTint::Context`, THE row SHALL paint the code
/// surface it sits on, not a tint of its own.
#[test]
fn test_renderer_diff_tint_context_line_keeps_the_surface() -> Result<()> {
    // -- Setup & Fixtures: a hunk with a context line between a removal and an
    // addition.
    let theme = test_theme();
    let entry = tool_entry(
        "edit",
        "→ a.rs",
        Some(diff_display(
            "--- a/a.rs\n+++ b/a.rs\n@@ -1,2 +1,2 @@\n alpha\n-beta\n+omega\n",
        )),
        None,
        ItemStatus::Done,
    );

    // -- Exec
    let backend = render(&entry)?;
    let context = row_index_containing(&backend, "alpha").ok_or("the context row must render")?;

    // -- Check
    assert_eq!(
        cell_bg(&backend, 0, context),
        Some(terminal(theme.code_surface)),
        "a context row keeps the surrounding surface"
    );
    Ok(())
}

/// WHEN a diff's headers render, THE rows SHALL carry neither the add nor the
/// remove tint.
#[test]
fn test_renderer_diff_tint_headers_carry_no_line_tint() -> Result<()> {
    // -- Setup & Fixtures
    let theme = test_theme();
    let entry = diff_entry();

    // -- Exec
    let backend = render(&entry)?;

    // -- Check
    for needle in ["@@", "--- a/a.rs", "+++ b/a.rs"] {
        let row = row_index_containing(&backend, needle).ok_or("the header row must render")?;
        for x in [0u16, 1, WIDTH - 1] {
            assert_ne!(
                cell_bg(&backend, x, row),
                Some(terminal(theme.diff_add_bg)),
                "row {needle:?} must not carry the add tint"
            );
            assert_ne!(
                cell_bg(&backend, x, row),
                Some(terminal(theme.diff_remove_bg)),
                "row {needle:?} must not carry the remove tint"
            );
        }
    }
    Ok(())
}

/// WHEN a diff line carries syntax-highlighted spans, THE span foreground SHALL
/// survive the row's tint underneath it.
#[test]
fn test_renderer_diff_tint_keeps_the_syntax_foreground() -> Result<()> {
    // -- Setup & Fixtures: an added Rust line whose `let` keyword highlights.
    let theme = test_theme();
    let entry = tool_entry(
        "edit",
        "→ a.rs",
        Some(diff_display(
            "--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-old\n+let x = 1;\n",
        )),
        None,
        ItemStatus::Done,
    );

    // -- Exec
    let backend = render(&entry)?;
    let added = row_index_containing(&backend, "let").ok_or("the added row must render")?;
    let row = row_text(&backend, added);
    let column = row.find("let").ok_or("the row must carry the keyword")?;
    let column = u16::try_from(column)?;

    // -- Check
    assert_eq!(
        cell_fg(&backend, column, added),
        Some(terminal(theme.markdown.code_keyword)),
        "the keyword keeps its syntax foreground"
    );
    assert_eq!(
        cell_bg(&backend, column, added),
        Some(terminal(theme.diff_add_bg)),
        "the keyword sits on the add tint"
    );
    Ok(())
}

/// WHEN a diff row's gutter renders, THE gutter glyph SHALL keep the muted
/// foreground over the row's tint.
#[test]
fn test_renderer_diff_tint_gutter_keeps_the_muted_foreground() -> Result<()> {
    // -- Setup & Fixtures
    let theme = test_theme();
    let entry = diff_entry();

    // -- Exec
    let backend = render(&entry)?;
    let added = row_index_containing(&backend, "new").ok_or("the added row must render")?;
    let gutter = (0..WIDTH)
        .find(|x| backend.buffer()[(*x, added)].symbol() == "\u{2502}")
        .ok_or("the added row must carry its line-number gutter")?;

    // -- Check
    assert_eq!(
        cell_fg(&backend, gutter, added),
        Some(terminal(theme.muted)),
        "the gutter glyph keeps the muted foreground"
    );
    assert_eq!(
        cell_bg(&backend, gutter, added),
        Some(terminal(theme.diff_add_bg)),
        "the gutter glyph sits on the add tint"
    );
    Ok(())
}

/// WHEN a tinted line sits inside a code surface, THE tint SHALL win over the
/// surface — the line-level tint is more specific.
#[test]
fn test_renderer_diff_tint_wins_over_the_code_surface() -> Result<()> {
    // -- Setup & Fixtures
    let theme = test_theme();
    let entry = diff_entry();

    // -- Exec
    let backend = render(&entry)?;
    let added = row_index_containing(&backend, "new").ok_or("the added row must render")?;

    // -- Check
    assert_ne!(
        cell_bg(&backend, 0, added),
        Some(terminal(theme.code_surface)),
        "the surface must not clobber the tint"
    );
    assert_eq!(
        cell_bg(&backend, 0, added),
        Some(terminal(theme.diff_add_bg)),
        "the add tint wins over the surface"
    );
    Ok(())
}
