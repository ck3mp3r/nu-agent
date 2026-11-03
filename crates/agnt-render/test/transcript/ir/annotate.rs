//! Direct tests for the diff-line hint classifier that lives beside
//! [`StyleHint`].

use super::*;

/// Hunk headers, file headers, the no-newline marker, added lines, and removed
/// lines each take their own hint; any other line is content.
#[test]
fn annotate_diff_hint_classifies_every_diff_line_kind() {
    // -- Setup & Fixtures
    let table = [
        ("@@ -1,3 +1,4 @@", StyleHint::DiffHunk),
        ("--- a/src/lib.rs", StyleHint::Meta),
        ("+++ b/src/lib.rs", StyleHint::Meta),
        ("\\ No newline at end of file", StyleHint::Meta),
        ("+added line", StyleHint::DiffAdd),
        ("-removed line", StyleHint::DiffRemove),
        (" context line", StyleHint::Normal),
        ("", StyleHint::Normal),
    ];

    // -- Exec & Check
    for (text, expected) in table {
        assert_eq!(annotate_diff_hint(text), expected, "hint for {text:?}");
    }
}

/// The classifier trims leading whitespace before it reads the marker, so an
/// indented hunk header or add/remove line still takes the diff hint.
#[test]
fn annotate_diff_hint_trims_leading_whitespace() {
    // -- Setup & Fixtures
    let table = [
        ("    @@ -2 +2 @@", StyleHint::DiffHunk),
        ("\t+added", StyleHint::DiffAdd),
        ("  -removed", StyleHint::DiffRemove),
        ("   --- a/x", StyleHint::Meta),
    ];

    // -- Exec & Check
    for (text, expected) in table {
        assert_eq!(annotate_diff_hint(text), expected, "hint for {text:?}");
    }
}

/// A bare marker with no trailing space is not a file header: only the `--- `
/// and `+++ ` forms take the meta hint.
#[test]
fn annotate_diff_hint_requires_the_file_header_space() {
    // -- Setup & Fixtures
    let table = [
        ("---", StyleHint::DiffRemove),
        ("+++", StyleHint::DiffAdd),
        ("----", StyleHint::DiffRemove),
        ("@@@@", StyleHint::Normal),
    ];

    // -- Exec & Check
    for (text, expected) in table {
        assert_eq!(annotate_diff_hint(text), expected, "hint for {text:?}");
    }
}
