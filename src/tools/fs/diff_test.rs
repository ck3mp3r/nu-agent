use super::diff::{DiffBounds, compute_edit_unified_diff_bounded};
use std::path::Path;

#[test]
fn deterministic_diff_core_replace_has_stable_headers_and_hunks() {
    let path = Path::new("src/file.txt");
    let previous = "alpha\nbeta\ngamma\n";
    let next = "alpha\nomega\ngamma\n";

    let diff = compute_edit_unified_diff_bounded(path, previous, next, DiffBounds::default());

    let expected =
        "--- a/src/file.txt\n+++ b/src/file.txt\n@@ -1,3 +1,3 @@\n alpha\n-beta\n+omega\n gamma\n";
    assert_eq!(diff.text, expected);

    let second = compute_edit_unified_diff_bounded(path, previous, next, DiffBounds::default());
    assert_eq!(diff.text, second.text);
    assert_eq!(
        diff.stats,
        super::diff::DiffStats {
            files_changed: 1,
            insertions: 1,
            deletions: 1
        }
    );
}

#[test]
fn deterministic_diff_core_insert_delete_and_noop_behavior() {
    let path = Path::new("src/file.txt");

    let inserted =
        compute_edit_unified_diff_bounded(path, "alpha\n", "alpha\nbeta\n", DiffBounds::default());
    assert!(inserted.text.contains("+beta\n"));
    assert_eq!(inserted.stats.insertions, 1);
    assert_eq!(inserted.stats.deletions, 0);

    let deleted =
        compute_edit_unified_diff_bounded(path, "alpha\nbeta\n", "alpha\n", DiffBounds::default());
    assert!(deleted.text.contains("-beta\n"));
    assert_eq!(deleted.stats.insertions, 0);
    assert_eq!(deleted.stats.deletions, 1);

    let noop = compute_edit_unified_diff_bounded(path, "alpha\n", "alpha\n", DiffBounds::default());
    assert_eq!(noop.text, "");
    assert_eq!(noop.stats.files_changed, 0);
    assert_eq!(noop.stats.insertions, 0);
    assert_eq!(noop.stats.deletions, 0);
}

#[test]
fn newline_and_eof_semantics_include_no_newline_markers_and_keep_line_endings() {
    let path = Path::new("src/newlines.txt");

    let eof_added =
        compute_edit_unified_diff_bounded(path, "alpha", "alpha\n", DiffBounds::default());
    assert!(eof_added.text.contains("\\ No newline at end of file\n"));

    let eof_removed =
        compute_edit_unified_diff_bounded(path, "alpha\n", "alpha", DiffBounds::default());
    assert!(eof_removed.text.contains("\\ No newline at end of file\n"));

    let crlf_change =
        compute_edit_unified_diff_bounded(path, "a\r\nb\r\n", "a\r\nc\r\n", DiffBounds::default());
    assert!(crlf_change.text.contains("-b\r\n"));
    assert!(crlf_change.text.contains("+c\r\n"));

    let mixed_eol =
        compute_edit_unified_diff_bounded(path, "a\r\nb\n", "a\r\nc\n", DiffBounds::default());
    assert!(mixed_eol.text.contains("-b\n"));
    assert!(mixed_eol.text.contains("+c\n"));
}

#[test]
fn large_diff_bounding_truncates_text_with_markers_but_preserves_summary_stats() {
    let path = Path::new("src/huge.txt");
    let previous = (0..200).map(|i| format!("old-{i}\n")).collect::<String>();
    let next = (0..200).map(|i| format!("new-{i}\n")).collect::<String>();

    let diff = compute_edit_unified_diff_bounded(
        path,
        &previous,
        &next,
        DiffBounds {
            max_bytes: 400,
            max_lines: 30,
        },
    );

    assert!(diff.truncated);
    assert!(diff.text.contains("... diff truncated ..."));
    assert!(diff.text.contains("omitted_lines="));
    assert!(diff.text.contains("omitted_hunks="));
    assert_eq!(diff.omitted_files, 0);
    assert!(diff.text.lines().count() <= 31);
    assert!(diff.text.len() <= 520);
    assert_eq!(diff.stats.files_changed, 1);
    assert_eq!(diff.stats.insertions, 200);
    assert_eq!(diff.stats.deletions, 200);
}

#[test]
fn bounded_diff_never_exceeds_max_bytes_even_for_tiny_budgets() {
    let path = Path::new("src/tiny-budget.txt");
    let previous = (0..64).map(|i| format!("old-{i}\n")).collect::<String>();
    let next = (0..64).map(|i| format!("new-{i}\n")).collect::<String>();

    for max_bytes in [0usize, 1, 2, 3, 4, 5, 8, 13, 21] {
        let diff = compute_edit_unified_diff_bounded(
            path,
            &previous,
            &next,
            DiffBounds {
                max_bytes,
                max_lines: 8,
            },
        );

        assert!(
            diff.text.len() <= max_bytes,
            "max_bytes={max_bytes} produced {} bytes",
            diff.text.len()
        );
        assert!(diff.truncated);
        assert_eq!(diff.stats.files_changed, 1);
        assert_eq!(diff.stats.insertions, 64);
        assert_eq!(diff.stats.deletions, 64);
    }
}

#[test]
fn bounded_diff_uses_deterministic_compact_marker_when_marker_exceeds_budget() {
    let path = Path::new("src/compact-marker.txt");
    let previous = (0..32).map(|i| format!("old-{i}\n")).collect::<String>();
    let next = (0..32).map(|i| format!("new-{i}\n")).collect::<String>();

    let bounds = DiffBounds {
        max_bytes: 4,
        max_lines: 4,
    };
    let first = compute_edit_unified_diff_bounded(path, &previous, &next, bounds);
    let second = compute_edit_unified_diff_bounded(path, &previous, &next, bounds);

    assert_eq!(first.text, "...\n");
    assert_eq!(first.text, second.text);
    assert!(first.truncated);
    assert!(first.omitted_hunks >= 1);
    assert!(first.text.len() <= bounds.max_bytes);
}
