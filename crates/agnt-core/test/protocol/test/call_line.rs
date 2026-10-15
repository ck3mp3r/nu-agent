#[test]
fn call_line_stores_summary_string() {
    // -- Setup & Fixtures
    let line = super::tool_args::CallLine {
        summary: "→ {\"path\":\"a.rs\"}".to_string(),
    };

    // -- Exec & Check
    assert_eq!(line.summary, "→ {\"path\":\"a.rs\"}");
}
