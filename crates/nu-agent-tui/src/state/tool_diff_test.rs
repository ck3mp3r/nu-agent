//! Unit tests for `add_diff_line_number_readability` (task 6910acb8).
//!
//! The function restores the pre-refactor diff gutter format: context lines
//! carry both old and new line numbers, all numbers are right-aligned to 4
//! columns, and the pipe separator sits directly against the body (no space
//! after the pipe). Unified-diff file headers (`--- ` / `+++ `) and the
//! no-newline marker (`\\ `) pass through verbatim.

use super::add_diff_line_number_readability;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn context_line_shows_both_old_and_new_numbers() -> Result<()> {
    // -- Setup & Fixtures
    let diff = "@@ -10,3 +10,3 @@\n hello\n";

    // -- Exec
    let out = add_diff_line_number_readability(diff);

    // -- Check: space + old (4-col) + space + new (4-col) + pipe + body.
    assert_eq!(out, "@@ -10,3 +10,3 @@\n   10   10 │hello\n");
    Ok(())
}

#[test]
fn removed_line_shows_old_number_in_old_column() -> Result<()> {
    // -- Setup & Fixtures
    let diff = "@@ -5,2 +5,2 @@\n-world\n";

    // -- Exec
    let out = add_diff_line_number_readability(diff);

    // -- Check: minus + old (4-col) + 6 spaces + pipe + body.
    assert_eq!(out, "@@ -5,2 +5,2 @@\n-   5      │world\n");
    Ok(())
}

#[test]
fn added_line_shows_new_number_in_new_column() -> Result<()> {
    // -- Setup & Fixtures
    let diff = "@@ -1,2 +8,2 @@\n+foo\n";

    // -- Exec
    let out = add_diff_line_number_readability(diff);

    // -- Check: plus + 5 spaces + new (4-col) + space + pipe + body.
    assert_eq!(out, "@@ -1,2 +8,2 @@\n+        8 │foo\n");
    Ok(())
}

#[test]
fn file_headers_pass_through_without_line_numbers() -> Result<()> {
    // -- Setup & Fixtures: a multi-file diff, so the SECOND file's headers
    // follow a hunk whose counters are already set. This is where the defect
    // bites — headers after a hunk would otherwise take a `-`/`+` gutter.
    let diff = "--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-old\n+new\n--- a/b.rs\n+++ b/b.rs\n@@ -1 +1 @@\n-old\n+new\n";

    // -- Exec
    let out = add_diff_line_number_readability(diff);

    // -- Check: every header stays verbatim — none is hunk content.
    assert_eq!(
        out,
        "--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-   1      │old\n+        1 │new\n--- a/b.rs\n+++ b/b.rs\n@@ -1 +1 @@\n-   1      │old\n+        1 │new\n"
    );
    Ok(())
}

#[test]
fn hunk_header_sets_counters_for_following_lines() -> Result<()> {
    // -- Setup & Fixtures
    let diff = "@@ -3,2 +3,2 @@\n alpha\n-beta\n+omega\n";

    // -- Exec
    let out = add_diff_line_number_readability(diff);

    // -- Check: hunk verbatim, then counters advance per line role.
    // alpha: context, old=3 new=3. beta: removed, old=4. omega: added, new=4.
    assert_eq!(
        out,
        "@@ -3,2 +3,2 @@\n    3    3 │alpha\n-   4      │beta\n+        4 │omega\n"
    );
    Ok(())
}

#[test]
fn no_newline_marker_passes_through_verbatim() -> Result<()> {
    // -- Setup & Fixtures
    let diff = "@@ -1 +1 @@\n-old\n\\ No newline at end of file\n";

    // -- Exec
    let out = add_diff_line_number_readability(diff);

    // -- Check: the marker is not diff content and gets no gutter.
    assert_eq!(
        out,
        "@@ -1 +1 @@\n-   1      │old\n\\ No newline at end of file\n"
    );
    Ok(())
}
