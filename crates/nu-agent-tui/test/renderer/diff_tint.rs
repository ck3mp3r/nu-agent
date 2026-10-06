use super::support::{content_spans, diff_tint_block, frame_ctx, rows_containing};
use crate::rendering::theme::TuiTheme;
use crate::tui_renderer::layout;
use nu_agent_core::transcript::ir::*;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// ========== diff tint background (task 669b4dac) ==========

/// WHEN a `ContentLine` has `diff_tint: Some(DiffTint::Add)`, THE renderer
/// SHALL apply `theme.diff_add_bg` as the background of every span in the
/// line.
#[test]
fn layout_diff_add_line_carries_add_background() -> Result<()> {
    // -- Setup & Fixtures
    let theme = TuiTheme::default();
    let block = diff_tint_block("--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-old\n+new\n");

    // -- Exec
    let lines = layout(&block, &frame_ctx(80), &TuiTheme::default());

    // -- Check: the added line's content row carries the add tint on every
    // span, including the muted gutter span.
    let added = rows_containing(&lines, "new");
    assert_eq!(added.len(), 1, "exactly one row must carry the added body");
    let added = added[0];
    assert!(
        content_spans(added).len() >= 2,
        "a diff body row must carry a gutter span plus body spans; got {added:?}"
    );
    for span in content_spans(added) {
        assert_eq!(
            span.style.bg,
            Some(theme.diff_add_bg),
            "every span of an added line must carry the add tint; span '{}' has {:?}",
            span.content,
            span.style.bg
        );
    }
    Ok(())
}

/// WHEN a `ContentLine` has `diff_tint: Some(DiffTint::Remove)`, THE renderer
/// SHALL apply `theme.diff_remove_bg` as the background of every span in the
/// line.
#[test]
fn layout_diff_remove_line_carries_remove_background() -> Result<()> {
    // -- Setup & Fixtures
    let theme = TuiTheme::default();
    let block = diff_tint_block("--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-old\n+new\n");

    // -- Exec
    let lines = layout(&block, &frame_ctx(80), &TuiTheme::default());

    // -- Check
    let removed = rows_containing(&lines, "old");
    assert_eq!(
        removed.len(),
        1,
        "exactly one row must carry the removed body"
    );
    for span in content_spans(removed[0]) {
        assert_eq!(
            span.style.bg,
            Some(theme.diff_remove_bg),
            "every span of a removed line must carry the remove tint; span '{}' has {:?}",
            span.content,
            span.style.bg
        );
    }
    Ok(())
}

/// WHEN a `ContentLine` has `diff_tint: Some(DiffTint::Context)` and the theme
/// leaves `diff_context_bg` as `None`, THE renderer SHALL apply no diff
/// background to the line.
#[test]
fn layout_diff_context_line_carries_no_background_by_default() -> Result<()> {
    // -- Setup & Fixtures
    let theme = TuiTheme::default();
    assert_eq!(
        theme.diff_context_bg, None,
        "the default theme must leave context lines untinted"
    );
    let block = diff_tint_block("--- a/a.rs\n+++ b/a.rs\n@@ -1,2 +1,2 @@\n alpha\n-beta\n+omega\n");

    // -- Exec
    let lines = layout(&block, &frame_ctx(80), &TuiTheme::default());

    // -- Check: the context row ('alpha') carries no background.
    let context = rows_containing(&lines, "alpha");
    assert_eq!(
        context.len(),
        1,
        "exactly one row must carry the context body"
    );
    for span in content_spans(context[0]) {
        assert_eq!(
            span.style.bg, None,
            "a context line must carry no diff background; span '{}' has {:?}",
            span.content, span.style.bg
        );
    }
    Ok(())
}

/// WHEN a `ContentLine` has `diff_tint: None`, THE renderer SHALL NOT apply
/// any diff background (regression check: non-diff lines are unchanged).
#[test]
fn layout_non_diff_line_carries_no_diff_background() -> Result<()> {
    // -- Setup & Fixtures
    let theme = TuiTheme::default();
    let block = diff_tint_block("--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-old\n+new\n");

    // -- Exec
    let lines = layout(&block, &frame_ctx(80), &TuiTheme::default());

    // -- Check: the hunk header and the file headers carry no tint.
    for needle in ["@@", "--- a/a.rs", "+++ b/a.rs"] {
        let rows = rows_containing(&lines, needle);
        assert_eq!(rows.len(), 1, "exactly one row must carry {needle:?}");
        for span in content_spans(rows[0]) {
            assert_ne!(
                span.style.bg,
                Some(theme.diff_add_bg),
                "non-diff row {needle:?} must not carry the add tint"
            );
            assert_ne!(
                span.style.bg,
                Some(theme.diff_remove_bg),
                "non-diff row {needle:?} must not carry the remove tint"
            );
        }
    }
    Ok(())
}

/// WHEN a diff line has syntax-highlighted spans, THE renderer SHALL apply the
/// syntax foreground colour on top of the diff background colour.
#[test]
fn layout_diff_line_keeps_syntax_foreground_over_tint() -> Result<()> {
    // -- Setup & Fixtures: a Rust diff body whose `let` keyword highlights.
    let theme = TuiTheme::default();
    let block = diff_tint_block("--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-old\n+let x = 1;\n");

    // -- Exec
    let lines = layout(&block, &frame_ctx(80), &TuiTheme::default());

    // -- Check: the added body carries a keyword span with the syntax keyword
    // foreground AND the add tint background.
    let added = rows_containing(&lines, "let");
    assert_eq!(added.len(), 1, "exactly one row must carry the added body");
    let keyword = added[0]
        .spans
        .iter()
        .find(|span| span.content.as_ref() == "let")
        .ok_or("the added body must carry a `let` keyword span")?;
    assert_eq!(
        keyword.style.fg,
        Some(theme.syntax_keyword.fg.ok_or("keyword fg must be set")?),
        "the keyword span must keep its syntax foreground"
    );
    assert_eq!(
        keyword.style.bg,
        Some(theme.diff_add_bg),
        "the keyword span must carry the add tint background"
    );
    Ok(())
}

/// WHEN a diff line has a gutter span with `StyleHint::Muted`, THE renderer
/// SHALL apply the muted foreground colour plus the diff background colour.
#[test]
fn layout_diff_gutter_span_keeps_muted_foreground_over_tint() -> Result<()> {
    // -- Setup & Fixtures
    let theme = TuiTheme::default();
    let block = diff_tint_block("--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-old\n+new\n");

    // -- Exec
    let lines = layout(&block, &frame_ctx(80), &TuiTheme::default());

    // -- Check: the added row's first span is the muted gutter, carrying the
    // muted foreground plus the add tint background.
    let added = rows_containing(&lines, "new");
    assert_eq!(added.len(), 1, "exactly one row must carry the added body");
    let gutter = content_spans(added[0])
        .first()
        .ok_or("a diff body row must start with a gutter span")?;
    assert!(
        gutter.content.contains('│'),
        "the first content span must be the line-number gutter; got {gutter:?}"
    );
    assert_eq!(
        gutter.style.fg, theme.tool_meta.fg,
        "the gutter span must keep the muted foreground"
    );
    assert_eq!(
        gutter.style.bg,
        Some(theme.diff_add_bg),
        "the gutter span must carry the add tint background"
    );
    Ok(())
}

/// WHEN a single-span `ContentLine` carries a `DiffTint`, THE renderer SHALL
/// apply the tint background on the single-span fast path too.
#[test]
fn layout_single_span_diff_tint_carries_background() -> Result<()> {
    // -- Setup & Fixtures: a ToolDisplay block whose one line is a single
    // tinted span (the fast path in `wrapped_row_spans`).
    let theme = TuiTheme::default();
    let lines = vec![ContentLine::single_with_tint(
        "+added".to_string(),
        StyleHint::MdCodePlain,
        DiffTint::Add,
    )];
    let block = Block {
        source: BlockSource::ToolDisplay { lines },
        lane: Lane::Blank,
        fill: Fill::None,
        status: None,
    };

    // -- Exec
    let rendered = layout(&block, &frame_ctx(80), &TuiTheme::default());

    // -- Check
    let added = rows_containing(&rendered, "+added");
    assert_eq!(added.len(), 1, "exactly one row must carry the tinted body");
    for span in content_spans(added[0]) {
        assert_eq!(
            span.style.bg,
            Some(theme.diff_add_bg),
            "a single-span tinted line must carry the add tint; span '{}' has {:?}",
            span.content,
            span.style.bg
        );
    }
    Ok(())
}

/// WHEN a diff line sits inside a `Fill::Code` preview block, THE renderer
/// SHALL keep the diff tint background rather than the surface0 fill — the
/// tint is more specific than the row fill.
#[test]
fn layout_diff_tint_wins_over_code_surface_fill() -> Result<()> {
    // -- Setup & Fixtures: the production preview shape — a ToolDisplay block
    // with Fill::Code whose lines are the projected diff.
    let theme = TuiTheme::default();
    let lines = nu_agent_core::transcript::markdown::project_diff_lines(
        "--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-old\n+new\n",
        "diff",
    );
    let block = Block {
        source: BlockSource::ToolDisplay { lines },
        lane: Lane::Blank,
        fill: Fill::Code,
        status: None,
    };

    // -- Exec
    let rendered = layout(&block, &frame_ctx(80), &TuiTheme::default());

    // -- Check: the added row keeps the add tint, not surface0.
    let added = rows_containing(&rendered, "new");
    assert_eq!(added.len(), 1, "exactly one row must carry the added body");
    for span in content_spans(added[0]) {
        assert_eq!(
            span.style.bg,
            Some(theme.diff_add_bg),
            "the diff tint must win over the code surface fill; span '{}' has {:?}",
            span.content,
            span.style.bg
        );
    }
    Ok(())
}
