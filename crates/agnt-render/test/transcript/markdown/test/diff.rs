use super::*;

// ---------------------------------------------------------------------------
// project_diff_lines — syntax-highlighted diff bodies (task 7f931bef)
// ---------------------------------------------------------------------------

/// Each diff line role carries its own line-level tint.
#[test]
fn project_diff_lines_tint_table() -> Result<()> {
    // -- Setup & Fixtures
    let lines = project_diff_lines(RUST_DIFF, "diff");
    let cases = [
        ("let x = 42;", DiffTint::Add),
        ("let x = 1;", DiffTint::Remove),
        ("fn main() {", DiffTint::Context),
    ];

    // -- Exec & Check
    for (needle, expected) in cases {
        let line = diff_line(&lines, needle)?;
        assert_eq!(
            line.diff_tint,
            Some(expected),
            "{needle:?} should carry the expected tint"
        );
    }
    Ok(())
}

/// Meta rows — hunk headers, file headers, and the no-newline marker — carry
/// no tint and keep their verbatim text.
#[test]
fn project_diff_lines_meta_rows_table() -> Result<()> {
    // -- Setup & Fixtures
    let lines = project_diff_lines(RUST_DIFF, "diff");

    // -- Exec & Check: the hunk header is one DiffHunk span with no tint.
    let hunk = diff_line(&lines, "@@ -1,2 +1,2 @@")?;
    assert_eq!(hunk.diff_tint, None, "hunk header carries no tint");
    assert_eq!(hunk.spans.len(), 1, "hunk header is one span");
    assert_eq!(hunk.spans[0].hint, StyleHint::DiffHunk);

    // -- Check: file headers are Meta spans with no tint.
    for needle in ["--- a/a.rs", "+++ b/a.rs"] {
        let header = diff_line(&lines, needle)?;
        assert_eq!(header.diff_tint, None, "{needle:?} carries no tint");
        assert_eq!(header.spans[0].hint, StyleHint::Meta, "{needle:?} is Meta");
    }

    // -- Check: the no-newline marker passes through verbatim with no gutter.
    let marker_diff = "@@ -1 +1 @@\n-old\n\\ No newline at end of file\n";
    let marker_lines = project_diff_lines(marker_diff, "");
    let marker = diff_line(&marker_lines, "No newline at end of file")?;
    assert_eq!(marker.diff_tint, None);
    assert_eq!(marker.spans.len(), 1);
    assert_eq!(marker.spans[0].hint, StyleHint::Meta);
    assert_eq!(plain_line(marker), "\\ No newline at end of file");

    // -- Check: a multi-file diff keeps every header verbatim.
    let multi = "--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-old\n+new\n--- a/b.rs\n+++ b/b.rs\n@@ -1 +1 @@\n-old\n+new\n";
    let multi_lines = project_diff_lines(multi, "");
    let headers: Vec<String> = multi_lines
        .iter()
        .map(plain_line)
        .filter(|text| text.starts_with("--- ") || text.starts_with("+++ "))
        .collect();
    assert_eq!(
        headers,
        vec!["--- a/a.rs", "+++ b/a.rs", "--- a/b.rs", "+++ b/b.rs"]
    );
    Ok(())
}

/// Diff body lines: syntax-highlighted spans, a muted gutter span, no
/// embedded trailing newline, and per-role gutter counters.
#[test]
fn project_diff_lines_body_and_gutter_table() -> Result<()> {
    // -- Setup & Fixtures
    let lines = project_diff_lines(RUST_DIFF, "diff");
    let added = diff_line(&lines, "let x = 42;")?;

    // -- Check: the body is syntax highlighted, not flattened to diff hints.
    assert!(
        added
            .spans
            .iter()
            .any(|span| span.text == "let" && span.hint == StyleHint::MdCodeKeyword),
        "`let` should carry MdCodeKeyword; got {:?}",
        added.spans
    );
    assert!(
        added
            .spans
            .iter()
            .any(|span| span.text == "42" && span.hint == StyleHint::MdCodeNumber),
        "`42` should carry MdCodeNumber; got {:?}",
        added.spans
    );
    assert!(
        !added
            .spans
            .iter()
            .any(|span| matches!(span.hint, StyleHint::DiffAdd | StyleHint::DiffRemove)),
        "diff body spans must not carry DiffAdd/DiffRemove hints; got {:?}",
        added.spans
    );

    // -- Check: the first span is the muted gutter with the new line number.
    // The hunk starts at new line 1; the context line advances it to 2, so the
    // added line shows new line 2 in the new-number column.
    let gutter = added.spans.first().ok_or("body line should have spans")?;
    assert_eq!(gutter.hint, StyleHint::Muted);
    assert_eq!(gutter.text, "+        2 │");

    // -- Check: no body span embeds a trailing newline.
    assert!(
        added.spans.iter().all(|span| !span.text.ends_with('\n')),
        "diff body spans must not embed a trailing newline; got {:?}",
        added.spans
    );

    // -- Check: gutter formatting per line role.
    let gutter_cases = [
        ("@@ -10,3 +10,3 @@\n hello\n", "hello", "   10   10 │hello"),
        ("@@ -5,2 +5,2 @@\n-world\n", "world", "-   5      │world"),
        ("@@ -1,2 +8,2 @@\n+foo\n", "foo", "+        8 │foo"),
    ];
    for (diff, needle, expected) in gutter_cases {
        let case_lines = project_diff_lines(diff, "");
        let line = diff_line(&case_lines, needle)?;
        assert_eq!(plain_line(line), expected, "{needle:?} gutter mismatch");
    }

    // -- Check: counters advance per line role within one hunk.
    let counters = project_diff_lines("@@ -3,2 +3,2 @@\n alpha\n-beta\n+omega\n", "");
    assert_eq!(
        plain_line(diff_line(&counters, "alpha")?),
        "    3    3 │alpha"
    );
    assert_eq!(
        plain_line(diff_line(&counters, "beta")?),
        "-   4      │beta"
    );
    assert_eq!(
        plain_line(diff_line(&counters, "omega")?),
        "+        4 │omega"
    );

    // -- Check: a blank context body keeps its gutter and tint.
    let blank_lines = project_diff_lines("@@ -1,2 +1,2 @@\n \n+added\n", "");
    let blank = blank_lines
        .iter()
        .find(|line| line.diff_tint == Some(DiffTint::Context))
        .ok_or("blank context line should carry Context tint")?;
    assert_eq!(blank.spans[0].hint, StyleHint::Muted);
    assert_eq!(blank.spans[0].text, "    1    1 │");
    assert_eq!(blank.spans.len(), 2);
    assert_eq!(blank.spans[1].text, "");
    assert_eq!(blank.spans[1].hint, StyleHint::Normal);
    Ok(())
}

/// Without a usable language hint the diff body falls back to plain Normal
/// spans while the gutter and tint stay intact.
#[test]
fn project_diff_lines_language_fallback_table() -> Result<()> {
    // -- Setup & Fixtures: a headerless diff with no language hint.
    let diff = "@@ -1 +1 @@\n-old\n+new\n";

    // -- Exec & Check: no language hint → plain Normal body spans.
    let no_lang = project_diff_lines(diff, "");
    let added = diff_line(&no_lang, "new")?;
    assert_eq!(added.diff_tint, Some(DiffTint::Add));
    assert_eq!(added.spans[0].hint, StyleHint::Muted);
    assert!(
        added.spans[1..]
            .iter()
            .all(|span| span.hint == StyleHint::Normal),
        "unhighlighted body must use Normal hints; got {:?}",
        added.spans
    );
    assert_eq!(plain_line(added), "+        1 │new");

    // -- Check: the coarse `"diff"` marker selects no grammar either.
    let coarse = project_diff_lines(diff, "diff");
    let coarse_added = diff_line(&coarse, "new")?;
    assert!(
        coarse_added.spans[1..]
            .iter()
            .all(|span| span.hint == StyleHint::Normal),
        "coarse diff marker must not select a grammar; got {:?}",
        coarse_added.spans
    );
    Ok(())
}
