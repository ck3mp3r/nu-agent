//! `ast_query` grammar-backed tests.

use super::*;

/// `ast_query` finds every function name, honours the capture filter, and
/// reports truncation when `max_matches` is below the total.
#[tokio::test]
#[serial_test::serial]
async fn ast_query_end_to_end_against_a_compiled_grammar() -> Result<()> {
    // -- Setup & Fixtures
    let Some(_env) = GrammarEnv::install()? else {
        return Ok(());
    };
    let (dir, path) = write_source()?;
    let path = path.to_str().ok_or("path must be UTF-8")?;

    // -- Exec
    let result = AstQueryTool::execute(
        &serde_json::json!({
            "path": path,
            "language": "rust",
            "query": "(function_item name: (identifier) @fn-name)"
        }),
        dir.path(),
        &Bus::default(),
    )
    .await
    .map_err(|e| format!("query must succeed: {e:?}"))?;

    // -- Check
    let matches = result["matches"].as_array().ok_or("matches array")?;
    let names: Vec<&str> = matches
        .iter()
        .filter_map(|m| m["captures"]["fn-name"]["text"].as_str())
        .collect();
    assert_eq!(names, vec!["process", "main"], "captured function names");
    assert_eq!(result["total_matches"], 2);
    assert_eq!(result["truncated"], false);
    assert_eq!(result["language"], "rust");
    assert_eq!(result["file"], path);
    let first = &matches[0]["captures"]["fn-name"];
    assert_eq!(first["start_row"], 2, "`process` starts on row 2");
    assert_eq!(first["start_col"], 3, "`process` starts at column 3");
    assert!(
        first["end_byte"].as_u64().ok_or("end_byte")?
            > first["start_byte"].as_u64().ok_or("start_byte")?,
        "the capture must span at least one byte"
    );
    Ok(())
}

/// `ast_query` truncates at `max_matches`, drops text when `include_text` is
/// false, and filters captures by name.
#[tokio::test]
#[serial_test::serial]
async fn ast_query_honours_max_matches_include_text_and_capture_filter() -> Result<()> {
    // -- Setup & Fixtures
    let Some(_env) = GrammarEnv::install()? else {
        return Ok(());
    };
    let (dir, path) = write_source()?;
    let path = path.to_str().ok_or("path must be UTF-8")?;
    let query = "(function_item name: (identifier) @fn-name)";

    // -- Exec
    let truncated = AstQueryTool::execute(
        &serde_json::json!({
            "path": path,
            "language": "rust",
            "query": query,
            "max_matches": 1,
            "include_text": false
        }),
        dir.path(),
        &Bus::default(),
    )
    .await
    .map_err(|e| format!("query must succeed: {e:?}"))?;
    let filtered = AstQueryTool::execute(
        &serde_json::json!({
            "path": path,
            "language": "rust",
            "query": query,
            "captures": ["not-a-capture"]
        }),
        dir.path(),
        &Bus::default(),
    )
    .await
    .map_err(|e| format!("query must succeed: {e:?}"))?;

    // -- Check
    let matches = truncated["matches"].as_array().ok_or("matches array")?;
    assert_eq!(
        matches.len(),
        1,
        "max_matches must cap the returned matches"
    );
    assert_eq!(truncated["total_matches"], 2, "the total still counts all");
    assert_eq!(truncated["truncated"], true, "truncation must be reported");
    assert!(
        matches[0]["captures"]["fn-name"]["text"].is_null(),
        "include_text=false must drop the capture text"
    );
    let filtered_matches = filtered["matches"].as_array().ok_or("matches array")?;
    assert_eq!(filtered_matches.len(), 2, "the match count is unaffected");
    assert!(
        filtered_matches[0]["captures"]
            .as_object()
            .ok_or("captures object")?
            .is_empty(),
        "a capture filter that matches nothing must drop every capture"
    );
    Ok(())
}
