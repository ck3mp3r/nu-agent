//! AST tool validation-error grammar-backed test.

use super::*;

/// An unknown language and a malformed query are reported as validation
/// errors, not as runtime failures.
#[tokio::test]
#[serial_test::serial]
async fn ast_tools_report_validation_errors_for_unknown_language_and_bad_query() -> Result<()> {
    // -- Setup & Fixtures
    let Some(_env) = GrammarEnv::install()? else {
        return Ok(());
    };
    let (dir, path) = write_source()?;
    let path = path.to_str().ok_or("path must be UTF-8")?;

    // -- Exec
    let unknown_language = AstTreeTool::execute(
        &serde_json::json!({"path": path, "language": "cobol"}),
        dir.path(),
        &Bus::default(),
    )
    .await
    .err()
    .ok_or("an unknown language must fail")?;
    let bad_query = AstQueryTool::execute(
        &serde_json::json!({"path": path, "language": "rust", "query": "(not_a_node"}),
        dir.path(),
        &Bus::default(),
    )
    .await
    .err()
    .ok_or("a malformed query must fail")?;

    // -- Check
    assert_eq!(unknown_language.kind, ToolErrorKind::Validation);
    assert!(
        unknown_language
            .message
            .contains("No tree-sitter grammar found"),
        "got: {}",
        unknown_language.message
    );
    assert_eq!(bad_query.kind, ToolErrorKind::Validation);
    assert!(
        bad_query.message.contains("Invalid tree-sitter query"),
        "got: {}",
        bad_query.message
    );
    Ok(())
}
