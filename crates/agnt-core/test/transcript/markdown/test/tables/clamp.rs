use super::*;

/// Table structure: rounded borders, a bold header row, a `├──┼──┤` separator,
/// and header `│` separators aligned with separator `┼` intersections.
#[test]
fn markdown_projection_table_structure_table() -> Result<()> {
    // -- Setup & Fixtures
    let markdown =
        "| Commit | Message |\n|--------|--------|\n| abc123 | fix bug |\n| def456 | add feature |";
    let lines = project_markdown_to_lines(markdown, None);
    let plain: Vec<String> = lines.iter().map(plain_line).collect();

    // -- Check: the header row carries the column names and the generic
    // heading hint (MdHeading(0)).
    let header_line = lines
        .iter()
        .find(|l| plain_line(l).contains("Commit") && plain_line(l).contains("Message"))
        .ok_or("header row should contain column names")?;
    assert!(
        header_line
            .spans
            .iter()
            .any(|s| s.hint == StyleHint::MdHeading(0)),
        "header cells should carry the generic heading hint"
    );

    // -- Check: the separator row carries horizontal lines with a junction and
    // uses the rounded `├` / `┤` edges.
    let separator = plain
        .iter()
        .find(|l| l.contains("─") && l.contains("┼"))
        .ok_or("separator row should contain horizontal lines with junction")?;
    assert!(
        separator.starts_with('├'),
        "separator left edge should be ├, got: {separator}"
    );
    assert!(
        separator.ends_with('┤'),
        "separator right edge should be ┤, got: {separator}"
    );

    // -- Check: data rows carry cell values and are bordered left and right.
    let data = plain
        .iter()
        .find(|l| l.contains("abc123") && l.contains("fix bug"))
        .ok_or("data row should contain cell values")?;
    assert!(
        data.starts_with('│') && data.ends_with('│'),
        "data row should be bordered, got: {data}"
    );

    // -- Check: rounded top and bottom borders.
    let top = plain.first().ok_or("table should have at least one line")?;
    assert!(
        top.contains('╭') && top.contains('╮'),
        "first output line should be the rounded top border with ╭ and ╮, got: {top}"
    );
    let bottom = plain.last().ok_or("table should have at least one line")?;
    assert!(
        bottom.contains('╰') && bottom.contains('╯'),
        "last output line should be the rounded bottom border with ╰ and ╯, got: {bottom}"
    );

    // -- Check: every content row is bordered left and right.
    let bordered = plain_lines("| A | B |\n|---|---|\n| x | y |\n| p | q |");
    let content_rows: Vec<&String> = bordered
        .iter()
        .filter(|l| l.contains('A') || l.contains('x') || l.contains('p'))
        .collect();
    assert!(
        !content_rows.is_empty(),
        "should have at least one content row"
    );
    for row in content_rows {
        assert!(
            row.starts_with('│'),
            "content row should start with │, got: {row}"
        );
        assert!(
            row.ends_with('│'),
            "content row should end with │, got: {row}"
        );
    }

    // -- Check: column widths follow the longest cell.
    let wide = plain_lines("| A | Long Header |\n|---|---|\n| x | y |\n| longer text | z |");
    let wide_sep = wide
        .iter()
        .find(|l| l.contains('┼'))
        .ok_or("separator line exists")?;
    let parts: Vec<&str> = wide_sep.split('┼').collect();
    assert_eq!(parts.len(), 2, "two columns = two separator parts");
    assert!(
        parts[0].len() >= " longer text ".len(),
        "first separator should be at least as wide as longest cell: {:?}",
        parts[0]
    );

    // -- Check: header `│` separators align with separator `┼` intersections.
    let aligned = "| # | Hash    | Date         | Author             | Message |\n\
                   |---|---------|--------------|--------------------|---------|\n\
                   | 0 | 66f1401 | 2026-06-18   | Christian Kemper   | feat(tui): branch icon |";
    let aligned_plain = plain_lines(aligned);
    let aligned_header = aligned_plain
        .iter()
        .find(|l| l.contains("Hash") && l.contains("Author"))
        .ok_or("header row exists")?;
    let aligned_sep = aligned_plain
        .iter()
        .find(|l| l.contains("┼"))
        .ok_or("separator row exists")?;
    let bar_positions: Vec<usize> = {
        let all: Vec<usize> = aligned_header
            .char_indices()
            .filter(|(_, c)| *c == '│')
            .map(|(i, _)| aligned_header[..i].chars().count())
            .collect();
        all[1..all.len().saturating_sub(1)].to_vec()
    };
    let intersection_positions: Vec<usize> = aligned_sep
        .char_indices()
        .filter(|(_, c)| *c == '┼')
        .map(|(i, _)| aligned_sep[..i].chars().count())
        .collect();
    assert_eq!(
        bar_positions, intersection_positions,
        "every inter-column header `│` must sit at the same column as the matching separator `┼` \
         (got bars at {bar_positions:?}, intersections at {intersection_positions:?})\n\
         header:    {aligned_header}\n\
         separator: {aligned_sep}"
    );
    Ok(())
}

/// Table width budget and cell content: every column survives a narrow budget,
/// the box borders stay intact, inline code stays in its column, and emoji cells
/// keep the right border aligned.
#[test]
fn markdown_projection_table_clamping_and_content_table() -> Result<()> {
    // -- Setup & Fixtures: 5 single-char columns at max_width=16. The box
    // borders alone need 1 + 4*5 = 21 display columns, so 16 cannot hold them:
    // the no-drop invariant wins and every column still renders.
    let wide = "| A | B | C | D | E |\n|---|---|---|---|---|\n| 1 | 2 | 3 | 4 | 5 |";
    let clamped = project_markdown_to_lines(wide, Some(16));
    let clamped_plain: Vec<String> = clamped.iter().map(plain_line).collect();

    // -- Check: no column is dropped — all five header cells survive and no
    // marker announces hidden columns.
    let top = clamped_plain.first().ok_or("non-empty output")?;
    assert_eq!(
        top.chars().filter(|c| *c == '┬').count(),
        4,
        "5-column table top border should have 4 ┬ joiners: {top}"
    );
    for label in [" A ", " B ", " C ", " D ", " E "] {
        assert!(
            clamped_plain.iter().any(|l| l.contains(label)),
            "header cell {label:?} must not be dropped: {clamped_plain:?}"
        );
    }
    assert!(
        !clamped_plain.iter().any(|l| l.contains("hidden")),
        "nothing is dropped, so no hidden-columns marker: {clamped_plain:?}"
    );

    // -- Check: the clamped right border uses ╮ / ┤ / ╯.
    assert!(
        top.ends_with('╮'),
        "top border should end with ╮ after clamping, got: {top}"
    );
    let separator = clamped_plain
        .iter()
        .find(|l| l.contains('┼') || l.contains('┤'))
        .ok_or("separator row exists")?;
    assert!(
        separator.ends_with('┤'),
        "separator should end with ┤ after clamping, got: {separator}"
    );
    let bottom = clamped_plain
        .iter()
        .rev()
        .find(|l| l.starts_with('╰'))
        .ok_or("has bottom border")?;
    assert!(
        bottom.ends_with('╯'),
        "bottom border should end with ╯ after clamping, got: {bottom}"
    );

    // -- Check: when the budget holds the borders, every emitted line stays
    // within it: three columns shrink to one display column each, their text
    // hard-breaks at character boundaries, and all four borders stay put.
    use unicode_width::UnicodeWidthStr;
    let fitted = project_markdown_to_lines(
        "| Alpha | Beta | Gamma |\n|-------|------|-------|\n| long word here | b | c |",
        Some(16),
    );
    let fitted_plain: Vec<String> = fitted.iter().map(plain_line).collect();
    for line in &fitted_plain {
        assert!(
            UnicodeWidthStr::width(line.as_str()) <= 16,
            "line exceeds max_width=16: {line:?} (width={})",
            UnicodeWidthStr::width(line.as_str())
        );
    }
    let fitted_top = fitted_plain.first().ok_or("non-empty output")?;
    assert_eq!(
        fitted_top.chars().filter(|c| *c == '┬').count(),
        2,
        "all 3 columns survive at max_width=16: {fitted_top}"
    );
    for line in fitted_plain.iter().filter(|l| l.starts_with('│')) {
        assert_eq!(
            line.chars().filter(|c| *c == '│').count(),
            4,
            "every physical row carries all 4 borders: {line:?}"
        );
    }

    // -- Check: inline code inside cells stays in its own column.
    let code_md = r#"This is the **nu-agent** project directory. Here's a summary of its contents:

| Name | Type | Description |
|------|------|-------------|
| `AGENTS.md` | file | Agent documentation |
| `Cargo.lock` / `Cargo.toml` | files | Rust project manifest and lockfile |
| `Formula/` | dir | Likely Homebrew formula for distribution |
| `README.md` | file | Project readme |

It's a **Rust project** using **Nix flakes** for development/build environment management."#;
    let code_plain = plain_lines(code_md);
    let first_col_values = ["AGENTS.md", "Cargo.lock", "Formula/", "README.md"];
    for line in &code_plain {
        let matches: Vec<_> = first_col_values
            .iter()
            .filter(|val| line.contains(*val))
            .collect();
        assert!(
            matches.len() <= 1,
            "Line should not contain multiple first-column values concatenated: {:?}\nLine: {}",
            matches,
            line
        );
    }
    let data_rows = [
        ("AGENTS.md", "file", "Agent documentation"),
        ("Cargo.lock", "files", "Rust project manifest"),
        ("Formula/", "dir", "Homebrew formula"),
        ("README.md", "file", "Project readme"),
    ];
    for (name, typ, desc_part) in &data_rows {
        let line = code_plain
            .iter()
            .find(|line| line.contains(name) && line.contains("│"))
            .ok_or_else(|| {
                format!("Should find a table row containing '{name}' with cell separator │")
            })?;
        assert!(
            line.contains(typ),
            "Row with '{name}' should also contain type '{typ}'\nGot: {line}"
        );
        assert!(
            line.contains(desc_part),
            "Row with '{name}' should also contain description part '{desc_part}'\nGot: {line}"
        );
    }
    assert!(
        code_plain.iter().any(|l| l.contains("Name")
            && l.contains("│")
            && l.contains("Type")
            && l.contains("Description")),
        "Should have header row with Name, Type, Description"
    );
    assert!(
        code_plain
            .iter()
            .any(|l| l.contains("─") && l.contains("┼")),
        "Should have separator row"
    );

    // -- Check: emoji cells keep the right border aligned by display width.
    let emoji = plain_lines("| Name | Status |\n| --- | --- |\n| server | 🟢 |\n| other | ⚪ |\n");
    let top_border = emoji
        .iter()
        .find(|l| l.starts_with('╭'))
        .ok_or("no top border")?;
    let border_width = UnicodeWidthStr::width(top_border.as_str());
    for line in &emoji {
        if line.contains('│') && !line.contains('─') {
            assert_eq!(
                UnicodeWidthStr::width(line.as_str()),
                border_width,
                "row width != border width for line: {line}"
            );
        }
    }

    // -- Check: a table that fits the budget announces nothing either.
    let fitting = plain_lines("| A | B |\n| --- | --- |\n| 1 | 2 |");
    assert!(
        !fitting.iter().any(|l| l.contains("hidden")),
        "a table within the budget must emit no hidden-columns marker"
    );
    Ok(())
}

/// WHEN a cell's text exceeds its allocated column width, THE projector SHALL
/// wrap it at word boundaries across physical rows, each one bordered │ … │.
#[test]
fn markdown_projection_table_cell_wraps_at_word_boundaries_table() -> Result<()> {
    // -- Setup & Fixtures: natural width 26 (3 + 16 + 7) clamped to 24, so the
    // description column gets 14 display columns and "alpha beta gamma" wraps.
    let markdown = "| Key | Description |\n| --- | --- |\n| a | alpha beta gamma |";
    let lines = project_markdown_to_lines(markdown, Some(24));
    let plain: Vec<String> = lines.iter().map(plain_line).collect();

    // -- Exec
    let data_rows: Vec<&String> = plain.iter().filter(|l| l.starts_with('│')).collect();

    // -- Check: the header is one physical row, the data row wrapped into two.
    assert_eq!(
        data_rows.len(),
        3,
        "header + 2 wrapped data rows: {plain:?}"
    );

    // -- Check: the first data row carries the first words, not the last word.
    let first = data_rows[1];
    assert!(
        first.contains("alpha beta"),
        "first physical row should hold the first words: {first:?}"
    );
    assert!(
        !first.contains("gamma"),
        "gamma must move to the next physical row: {first:?}"
    );

    // -- Check: the continuation row carries the remaining word behind borders.
    let second = data_rows[2];
    assert!(
        second.contains("gamma"),
        "second physical row should hold the last word: {second:?}"
    );
    assert!(
        !second.contains("alpha"),
        "the wrapped word must not repeat: {second:?}"
    );

    // -- Check: every physical row keeps all three borders and the budget.
    for row in &data_rows {
        assert_eq!(
            row.chars().filter(|c| *c == '│').count(),
            3,
            "2 columns need 3 borders on every physical row: {row:?}"
        );
        assert!(
            unicode_width::UnicodeWidthStr::width(row.as_str()) == 24,
            "each physical row is padded to the table width 24: {row:?}"
        );
    }
    Ok(())
}

/// WHEN the cells of one logical row wrap to different line counts, THE
/// projector SHALL pad the shorter cells with spaces so every physical row
/// carries the complete border structure.
#[test]
fn markdown_projection_table_uneven_wrap_pads_shorter_cells_table() -> Result<()> {
    // -- Setup & Fixtures: natural width 40 clamped to 30, so only the middle
    // column narrows (to 14) and wraps while its neighbours stay one line.
    let markdown =
        "| # | Title | Owner |\n| --- | --- | --- |\n| 1 | a much longer title here | me |";
    let lines = project_markdown_to_lines(markdown, Some(30));
    let plain: Vec<String> = lines.iter().map(plain_line).collect();

    // -- Exec
    let data_rows: Vec<&String> = plain.iter().filter(|l| l.starts_with('│')).collect();

    // -- Check: header + the two physical rows of the single data row.
    assert_eq!(
        data_rows.len(),
        3,
        "header + 2 wrapped data rows: {plain:?}"
    );

    // -- Check: the continuation row carries the tail of the middle cell behind
    // all four borders.
    let continuation = data_rows[2];
    assert!(
        continuation.contains("title here"),
        "continuation row should hold the wrapped tail: {continuation:?}"
    );
    let segments: Vec<&str> = continuation.split('│').collect();
    assert_eq!(
        segments.len(),
        5,
        "3 columns need 4 borders: {continuation:?}"
    );

    // -- Check: the neighbouring cells are blank, space-padded to their width.
    for blank in [segments[1], segments[3]] {
        assert!(
            blank.chars().all(|c| c == ' '),
            "a shorter cell pads with spaces on the continuation row: {blank:?}"
        );
    }
    assert!(
        !segments[1].is_empty() && !segments[3].is_empty(),
        "padded cells keep their column width: {continuation:?}"
    );
    Ok(())
}

/// WHEN a header cell wraps across physical rows, THE projector SHALL keep the
/// MdHeading(0) hint on every one of those rows.
#[test]
fn markdown_projection_table_wrapped_header_keeps_heading_hint_table() -> Result<()> {
    // -- Setup & Fixtures: natural width 25 clamped to 14, so both header cells
    // hard-break and the header spans several physical rows.
    let markdown = "| HeaderOne | HeaderTwo |\n| --- | --- |\n| a | b |";
    let lines = project_markdown_to_lines(markdown, Some(14));

    // -- Exec
    let header_rows: Vec<&ContentLine> = lines
        .iter()
        .take_while(|l| !plain_line(l).starts_with('├'))
        .filter(|l| plain_line(l).starts_with('│'))
        .collect();

    // -- Check: the header wrapped into more than one physical row.
    assert!(
        header_rows.len() > 1,
        "the fixture header should wrap: {header_rows:?}"
    );

    // -- Check: every physical header row carries the heading hint.
    for row in header_rows {
        assert!(
            row.spans.iter().any(|s| s.hint == StyleHint::MdHeading(0)),
            "every wrapped header row keeps MdHeading(0): {:?}",
            plain_line(row)
        );
    }
    Ok(())
}
