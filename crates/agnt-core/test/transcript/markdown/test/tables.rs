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

    // -- Check: the header row carries the column names and is bold.
    let header_line = lines
        .iter()
        .find(|l| plain_line(l).contains("Commit") && plain_line(l).contains("Message"))
        .ok_or("header row should contain column names")?;
    assert!(
        header_line
            .spans
            .iter()
            .any(|s| s.hint == StyleHint::MdBold),
        "header cells should be bold"
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

/// Table clamping and cell content: width budget, minimum one column, right
/// border after clamping, inline code in cells, and emoji width alignment.
#[test]
fn markdown_projection_table_clamping_and_content_table() -> Result<()> {
    // -- Setup & Fixtures: 5 single-char columns; max_width=16 fits 3.
    let wide = "| A | B | C | D | E |\n|---|---|---|---|---|\n| 1 | 2 | 3 | 4 | 5 |";
    let clamped = project_markdown_to_lines(wide, Some(16));
    let clamped_plain: Vec<String> = clamped.iter().map(plain_line).collect();

    // -- Check: every line fits the width budget.
    for line in &clamped_plain {
        assert!(
            line.chars().count() <= 16,
            "line exceeds max_width=16: {line:?} (len={})",
            line.chars().count()
        );
    }

    // -- Check: exactly 3 columns survive; D and E are clamped away.
    let top = clamped_plain.first().ok_or("non-empty output")?;
    assert_eq!(
        top.chars().filter(|c| *c == '┬').count(),
        2,
        "3-column table top border should have 2 ┬ joiners: {top}"
    );
    assert!(
        !clamped_plain.iter().any(|l| l.contains('D')),
        "column D should have been clamped away"
    );
    assert!(
        !clamped_plain.iter().any(|l| l.contains('E')),
        "column E should have been clamped away"
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
    let bottom = clamped_plain.last().ok_or("has bottom border")?;
    assert!(
        bottom.ends_with('╯'),
        "bottom border should end with ╯ after clamping, got: {bottom}"
    );

    // -- Check: an impossibly small width still renders at least one column.
    let tiny = project_markdown_to_lines(
        "| Alpha | Beta | Gamma |\n|-------|------|-------|\n| a | b | c |",
        Some(1),
    );
    let tiny_plain: Vec<String> = tiny.iter().map(plain_line).collect();
    assert!(
        tiny_plain
            .iter()
            .any(|l| l.contains('╭') && l.contains('╮')),
        "even with max_width=1, should still render top border"
    );
    assert!(
        tiny_plain.iter().any(|l| l.contains("Alpha")),
        "at least the first column (Alpha) must be rendered"
    );

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
    use unicode_width::UnicodeWidthStr;
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
    Ok(())
}
