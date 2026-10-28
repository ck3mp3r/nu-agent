//! `ast_refs` grammar-backed test.

use super::*;

/// `ast_refs` finds every identifier occurrence of a name and reports
/// truncation.
#[tokio::test]
#[serial_test::serial]
async fn ast_refs_end_to_end_against_a_compiled_grammar() -> Result<()> {
    // -- Setup & Fixtures
    let Some(_env) = GrammarEnv::install()? else {
        return Ok(());
    };
    let (dir, path) = write_source()?;
    let path = path.to_str().ok_or("path must be UTF-8")?;

    // -- Exec
    let all = AstRefsTool::execute(
        &serde_json::json!({"path": path, "language": "rust", "name": "Status"}),
        dir.path(),
        &Bus::default(),
    )
    .await
    .map_err(|e| format!("refs must succeed: {e:?}"))?;
    let capped = AstRefsTool::execute(
        &serde_json::json!({
            "path": path,
            "language": "rust",
            "name": "Status",
            "max_matches": 1
        }),
        dir.path(),
        &Bus::default(),
    )
    .await
    .map_err(|e| format!("refs must succeed: {e:?}"))?;

    // -- Check
    let matches = all["matches"].as_array().ok_or("matches array")?;
    assert_eq!(matches.len(), 3, "three `Status` occurrences");
    assert_eq!(all["total_matches"], 3);
    assert_eq!(all["truncated"], false);
    assert!(
        matches
            .iter()
            .all(|m| m["captures"]["name"]["text"] == "Status"),
        "every capture must carry the searched name"
    );
    let capped_matches = capped["matches"].as_array().ok_or("matches array")?;
    assert_eq!(capped_matches.len(), 1, "max_matches must cap the matches");
    assert_eq!(capped["total_matches"], 3);
    assert_eq!(capped["truncated"], true);
    Ok(())
}
