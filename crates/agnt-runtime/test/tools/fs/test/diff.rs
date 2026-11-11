use super::*;

// ═══════════════════════════════════════════════════════════════════════════════
// Tests: diff
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn deterministic_diff_core_table() {
    // -- Setup & Fixtures
    let path = Path::new("src/file.txt");

    // -- Exec
    let replaced = compute_edit_unified_diff_bounded(
        path,
        "alpha\nbeta\ngamma\n",
        "alpha\nomega\ngamma\n",
        DiffBounds::default(),
    );
    let replaced_again = compute_edit_unified_diff_bounded(
        path,
        "alpha\nbeta\ngamma\n",
        "alpha\nomega\ngamma\n",
        DiffBounds::default(),
    );
    let inserted =
        compute_edit_unified_diff_bounded(path, "alpha\n", "alpha\nbeta\n", DiffBounds::default());
    let deleted =
        compute_edit_unified_diff_bounded(path, "alpha\nbeta\n", "alpha\n", DiffBounds::default());
    let noop = compute_edit_unified_diff_bounded(path, "alpha\n", "alpha\n", DiffBounds::default());

    // -- Check
    let expected =
        "--- a/src/file.txt\n+++ b/src/file.txt\n@@ -1,3 +1,3 @@\n alpha\n-beta\n+omega\n gamma\n";
    assert_eq!(replaced.text, expected);
    assert_eq!(
        replaced.text, replaced_again.text,
        "the diff must be deterministic"
    );
    assert_eq!(
        replaced.stats,
        super::super::diff::DiffStats {
            files_changed: 1,
            insertions: 1,
            deletions: 1
        }
    );

    assert!(inserted.text.contains("+beta\n"));
    assert_eq!(inserted.stats.insertions, 1);
    assert_eq!(inserted.stats.deletions, 0);

    assert!(deleted.text.contains("-beta\n"));
    assert_eq!(deleted.stats.insertions, 0);
    assert_eq!(deleted.stats.deletions, 1);

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
fn bounded_diff_table() {
    // -- Setup & Fixtures
    let path = Path::new("src/huge.txt");
    let previous = (0..200).map(|i| format!("old-{i}\n")).collect::<String>();
    let next = (0..200).map(|i| format!("new-{i}\n")).collect::<String>();

    // -- Exec
    let bounded = compute_edit_unified_diff_bounded(
        path,
        &previous,
        &next,
        DiffBounds {
            max_bytes: 400,
            max_lines: 30,
        },
    );

    // -- Check: truncation keeps the markers and the summary stats
    assert!(bounded.truncated);
    assert!(bounded.text.contains("... diff truncated ..."));
    assert!(bounded.text.contains("omitted_lines="));
    assert!(bounded.text.contains("omitted_hunks="));
    assert_eq!(bounded.omitted_files, 0);
    assert!(bounded.text.lines().count() <= 31);
    assert!(bounded.text.len() <= 520);
    assert_eq!(bounded.stats.files_changed, 1);
    assert_eq!(bounded.stats.insertions, 200);
    assert_eq!(bounded.stats.deletions, 200);

    // -- Exec & Check: tiny budgets never exceed max_bytes
    let tiny_previous = (0..64).map(|i| format!("old-{i}\n")).collect::<String>();
    let tiny_next = (0..64).map(|i| format!("new-{i}\n")).collect::<String>();
    for max_bytes in [0usize, 1, 2, 3, 4, 5, 8, 13, 21] {
        let diff = compute_edit_unified_diff_bounded(
            path,
            &tiny_previous,
            &tiny_next,
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

    // -- Exec & Check: a deterministic compact marker when the marker exceeds the budget
    let compact_previous = (0..32).map(|i| format!("old-{i}\n")).collect::<String>();
    let compact_next = (0..32).map(|i| format!("new-{i}\n")).collect::<String>();
    let first = compute_edit_unified_diff_bounded(
        path,
        &compact_previous,
        &compact_next,
        DiffBounds {
            max_bytes: 4,
            max_lines: 4,
        },
    );
    let second = compute_edit_unified_diff_bounded(
        path,
        &compact_previous,
        &compact_next,
        DiffBounds {
            max_bytes: 4,
            max_lines: 4,
        },
    );

    assert_eq!(first.text, "...\n");
    assert_eq!(first.text, second.text);
    assert!(first.truncated);
    assert!(first.omitted_hunks >= 1);
    assert!(first.text.len() <= 4);
}
