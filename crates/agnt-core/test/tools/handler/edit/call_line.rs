use super::*;

// === call_line_render ===

#[test]
fn edit_call_line_render_table() -> Result<()> {
    // -- Setup & Fixtures
    let cases: Vec<(&str, &str, Option<&str>)> = vec![
        (
            "shows path and diff marker",
            r#"{"path":"/tmp/f.txt"}"#,
            Some("→ /tmp/f.txt (diff)"),
        ),
        ("falls back to generic on invalid json", "not-json", None),
    ];

    // -- Exec & Check
    for (name, args, expected) in cases {
        let render = EditTool::call_line_render(args);
        match expected {
            Some(summary) => assert_eq!(
                render,
                CallLine {
                    summary: summary.to_string(),
                },
                "case '{name}'"
            ),
            None => assert_eq!(render, CallLine::from_json_summary(args), "case '{name}'"),
        }
    }
    Ok(())
}

// === diff path headers ===

#[test]
fn edit_diff_headers_carry_real_path() -> Result<()> {
    // -- Setup & Fixtures
    let plan = diff_plan();

    // -- Exec
    let display = edit_preview_display("src/main.rs", &plan);
    let response =
        build_edit_contract_response("src/main.rs", EditToolMode::Preview, plan, false, None);

    // -- Check
    let display_content = display.sections[0].content.as_str();
    let diff = response["diff"].as_str().ok_or("diff must be a string")?;
    for (name, content) in [
        ("preview display", display_content),
        ("contract response", diff),
    ] {
        assert!(
            content.contains("--- a/src/main.rs"),
            "case '{name}': diff must carry the real path in the old header, got: {content:?}"
        );
        assert!(
            content.contains("+++ b/src/main.rs"),
            "case '{name}': diff must carry the real path in the new header, got: {content:?}"
        );
    }
    Ok(())
}

#[test]
fn edit_diff_headers_select_syntax_language_from_extension() -> Result<()> {
    // -- Setup & Fixtures
    let plan = diff_plan();
    let display = edit_preview_display("src/main.rs", &plan);

    // -- Exec
    let lines =
        crate::transcript::markdown::project_diff_lines(&display.sections[0].content, "diff");

    // -- Check: the `.rs` header extension selects the Rust grammar, so the
    // added body line carries syntax hints instead of a single plain span.
    let added = lines
        .iter()
        .find(|line| line.spans.iter().any(|span| span.text.contains("42")))
        .ok_or("added body line should be projected")?;
    assert!(
        added
            .spans
            .iter()
            .any(|span| span.hint == crate::transcript::ir::StyleHint::MdCodeKeyword),
        "`let` should carry MdCodeKeyword when the header extension selects Rust; got {:?}",
        added.spans
    );
    Ok(())
}
