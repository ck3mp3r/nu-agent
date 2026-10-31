use crate::gallery::{Gallery, GallerySnapshot};
use crate::planner::EntryFamily;
use agnt_core::transcript::entry::TranscriptEntry;

// ---------------------------------------------------------------------------
// Every family
// ---------------------------------------------------------------------------

/// The gallery carries one snapshot per family — message, tool (pending
/// preview), tool (completion result), notice, banner, tool (diff result),
/// multi-paragraph message.
#[test]
fn test_gallery_snapshots_cover_every_family() -> Result<()> {
    // -- Setup & Fixtures
    let expected = [
        ("message", EntryFamily::User),
        ("tool", EntryFamily::Tool),
        ("tool-result", EntryFamily::Tool),
        ("notice", EntryFamily::Notice),
        ("banner", EntryFamily::Banner),
        ("tool-diff", EntryFamily::Tool),
        ("message-blank-lines", EntryFamily::User),
    ];

    // -- Exec
    let snapshots = Gallery::snapshots();

    // -- Check
    assert_eq!(snapshots.len(), expected.len(), "one snapshot per sample");
    for (snapshot, (name, family)) in snapshots.iter().zip(expected) {
        assert_eq!(snapshot.name, name);
        assert_eq!(snapshot.family, family);
        assert!(!snapshot.text.is_empty(), "{name} snapshot has text");
    }
    Ok(())
}

/// Every snapshot is flat text: no lane marker and no leading pad on any row.
/// The one row shape that may open with a space is a unified-diff context row:
/// its first column is the diff's own marker and the line-number gutter
/// follows it, so the space is content, not a pad.
#[test]
fn test_gallery_snapshots_are_flat_text() -> Result<()> {
    // -- Setup & Fixtures
    let lane_glyphs = ['⚙', '╎', '~', '·'];
    let samples = Gallery::samples();

    // -- Exec
    let snapshots = Gallery::snapshots();

    // -- Check
    for (sample, snapshot) in samples.iter().zip(&snapshots) {
        let diff_rows = carries_diff(&sample.entry);
        for line in snapshot.text.lines() {
            assert!(
                !line.starts_with(lane_glyphs),
                "{} has no lane marker: {line:?}",
                snapshot.name
            );
            if line.starts_with(' ') {
                assert!(
                    diff_rows && is_diff_gutter_row(line),
                    "{} has no leading pad: {line:?}",
                    snapshot.name
                );
            }
        }
    }
    Ok(())
}

/// The snapshots are the golden fixtures: exact text per family.
#[test]
fn test_gallery_snapshots_golden() -> Result<()> {
    // -- Setup & Fixtures
    let expected = [
        ("message", "What changed in the render loop?"),
        ("tool", "read → src/render.rs\nseam 2 now plans the gaps"),
        ("tool-result", "edit → src/render.rs\nresult body"),
        ("notice", "Compaction skipped: transcript is short"),
        ("banner", "nu-agent"),
        (
            "tool-diff",
            "\
edit → crates/agnt-render/src/plain.rs
(diff)
files=1 +2 -1
--- a/crates/agnt-render/src/plain.rs
+++ b/crates/agnt-render/src/plain.rs
@@ -118,7 +118,8 @@
  118  118 │fn content_rows(entry:
&TranscriptEntry, width: usize) ->
Vec<String> {
  119  119 │    let width =
width.max(1);
- 120      │    content_lines(entry,
width)
+      120 │    let wrapping =
WrapMode::first_fit(width);
+      121 │    content_lines(entry,
wrapping)
  121  122 │        .iter()
  122  123 │        .flat_map(|line|
wrap_line(&rendered_line_to_plain_text(l
ine), width))
  123  124 │        .collect()
}",
        ),
        (
            "message-blank-lines",
            "\
Two seams moved in this wave.

The planner now plans the gap rows, so a
wrapped preview keeps its spacing.

The TTY strips the escapes back to the
plain rows.",
        ),
    ];

    // -- Exec
    let snapshots = Gallery::snapshots();

    // -- Check
    for (snapshot, (name, text)) in snapshots.iter().zip(expected) {
        assert_eq!(snapshot.name, name);
        assert_eq!(snapshot.text, text, "{name} snapshot");
    }
    assert_snapshot_kinds_are_distinct(&snapshots)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Catalog components
// ---------------------------------------------------------------------------

/// The catalog components join the family snapshots: one sample per
/// component, in catalog order.
#[test]
fn test_gallery_component_snapshots_cover_the_catalog() -> Result<()> {
    // -- Setup & Fixtures
    let expected = [
        "lane-line",
        "call-line-row",
        "stat-row",
        "diff-view",
        "code-surface",
    ];

    // -- Exec
    let samples = Gallery::component_samples();
    let snapshots = Gallery::component_snapshots();

    // -- Check
    assert_eq!(samples.len(), expected.len(), "one sample per component");
    assert_eq!(snapshots.len(), expected.len(), "one snapshot per sample");
    for ((sample, snapshot), name) in samples.iter().zip(&snapshots).zip(expected) {
        assert_eq!(sample.component.name(), name);
        assert_eq!(snapshot.name, name);
        assert!(!snapshot.text.is_empty(), "{name} snapshot has text");
    }
    assert_eq!(
        Gallery::snapshots().len(),
        Gallery::samples().len(),
        "the family snapshots stay alongside the components"
    );
    Ok(())
}

/// The component snapshots are the golden fixtures: exact plain text per
/// component.
#[test]
fn test_gallery_component_snapshots_golden() -> Result<()> {
    // -- Setup & Fixtures
    let expected = [
        ("lane-line", "  ⚙ "),
        ("call-line-row", "✓ read → src/render.rs"),
        ("stat-row", "files=1 +2 -0"),
        ("diff-view", "@@ -1 +1 @@\n-   1      │old\n+        1 │new"),
        ("code-surface", "\nlet a = 1;\n"),
    ];

    // -- Exec
    let snapshots = Gallery::component_snapshots();

    // -- Check
    for (snapshot, (name, text)) in snapshots.iter().zip(expected) {
        assert_eq!(snapshot.name, name);
        assert_eq!(snapshot.text, text, "{name} snapshot");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Test Support
// ---------------------------------------------------------------------------

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// The family snapshots are distinct fixtures, not copies of one rendering.
fn assert_snapshot_kinds_are_distinct(snapshots: &[GallerySnapshot]) -> Result<()> {
    for (index, left) in snapshots.iter().enumerate() {
        for right in snapshots.iter().skip(index + 1) {
            assert_ne!(
                left.text, right.text,
                "{} and {} differ",
                left.name, right.name
            );
        }
    }
    Ok(())
}

/// A tool entry whose display carries a diff body.
fn carries_diff(entry: &TranscriptEntry) -> bool {
    match entry {
        TranscriptEntry::Tool(tool) => tool
            .result
            .as_ref()
            .or(tool.preview.as_ref())
            .is_some_and(|display| display.has_diff()),
        _ => false,
    }
}

/// A unified-diff gutter row: the marker column, the right-aligned old and new
/// line numbers, then the pipe against the body.
fn is_diff_gutter_row(line: &str) -> bool {
    let head: Vec<char> = line.chars().take(12).collect();
    head.len() == 12 && head[0] == ' ' && head[11] == '│'
}
