//! Edit-tool argument-contract tests.
//!
//! The edit tool answers a malformed request with a contract error response
//! (`applied: false` plus a diagnostic) rather than a hard tool error, so the
//! model sees the same envelope on every path. These tests pin that envelope
//! for each rejected argument shape.

use super::*;

// region:    --- Test Support

/// The first diagnostic of a contract response.
fn first_diagnostic(
    response: &serde_json::Value,
) -> core::result::Result<&serde_json::Value, String> {
    response["diagnostics"]
        .as_array()
        .ok_or_else(|| "diagnostics must be an array".to_string())?
        .first()
        .ok_or_else(|| "a rejected request must produce a diagnostic".to_string())
}

/// The diagnostic message of a contract response.
fn diagnostic_message(response: &serde_json::Value) -> core::result::Result<String, String> {
    Ok(first_diagnostic(response)?["message"]
        .as_str()
        .ok_or_else(|| "diagnostic message must be a string".to_string())?
        .to_string())
}

/// Run the edit tool against a seeded file and return the contract response.
async fn execute(dir: &std::path::Path, args: serde_json::Value) -> Result<serde_json::Value> {
    Ok(EditTool::execute(&args, dir, &Bus::default())
        .await
        .map_err(|e| format!("the edit tool must answer with a contract response: {e:?}"))?)
}

// endregion: --- Test Support

#[tokio::test]
async fn edit_invalid_mode_returns_validation_contract_error() -> Result<()> {
    // -- Setup & Fixtures
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("target.txt");
    std::fs::write(&path, "hello\n")?;
    let args = serde_json::json!({
        "path": path.to_str().ok_or("temp path must be UTF-8")?,
        "mode": "sideways",
        "operation": {"type": "search_replace", "search": "hello", "replacement": "bye"}
    });

    // -- Exec
    let response = execute(dir.path(), args).await?;

    // -- Check
    assert_eq!(response["applied"], false);
    assert_eq!(
        response["mode"], "apply",
        "an invalid mode must be reported as apply"
    );
    assert_eq!(first_diagnostic(&response)?["class"], "validation");
    assert!(
        diagnostic_message(&response)?.contains("Invalid edit.mode 'sideways'"),
        "got: {}",
        diagnostic_message(&response)?
    );
    assert_eq!(
        std::fs::read_to_string(&path)?,
        "hello\n",
        "a rejected request must leave the file untouched"
    );
    Ok(())
}

#[tokio::test]
async fn edit_invalid_operation_type_returns_validation_contract_error() -> Result<()> {
    // -- Setup & Fixtures
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("target.txt");
    std::fs::write(&path, "hello\n")?;
    let args = serde_json::json!({
        "path": path.to_str().ok_or("temp path must be UTF-8")?,
        "operation": {"type": "rename"}
    });

    // -- Exec
    let response = execute(dir.path(), args).await?;

    // -- Check
    assert_eq!(response["applied"], false);
    assert_eq!(first_diagnostic(&response)?["class"], "validation");
    assert!(
        diagnostic_message(&response)?.contains("Invalid edit.operation.type 'rename'"),
        "got: {}",
        diagnostic_message(&response)?
    );
    Ok(())
}

#[tokio::test]
async fn edit_missing_operation_fields_return_validation_contract_errors() -> Result<()> {
    // -- Setup & Fixtures
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("target.txt");
    std::fs::write(&path, "hello\n")?;
    let path_str = path.to_str().ok_or("temp path must be UTF-8")?.to_string();
    let cases: Vec<(&str, serde_json::Value, &str)> = vec![
        (
            "create without content",
            serde_json::json!({"type": "create"}),
            "missing field `operation.content`",
        ),
        (
            "search_replace without search",
            serde_json::json!({"type": "search_replace", "replacement": "bye"}),
            "missing field `operation.search`",
        ),
        (
            "search_replace without replacement",
            serde_json::json!({"type": "search_replace", "search": "hello"}),
            "missing field `operation.replacement`",
        ),
    ];

    // -- Exec & Check
    for (name, operation, expected) in cases {
        let args = serde_json::json!({"path": path_str, "operation": operation});
        let response = execute(dir.path(), args).await?;
        assert_eq!(response["applied"], false, "case '{name}': applied");
        assert_eq!(
            first_diagnostic(&response)?["class"],
            "validation",
            "case '{name}': class"
        );
        let message = diagnostic_message(&response)?;
        assert!(
            message.contains(expected),
            "case '{name}': message must contain {expected:?}, got: {message}"
        );
    }
    assert_eq!(
        std::fs::read_to_string(&path)?,
        "hello\n",
        "rejected requests must leave the file untouched"
    );
    Ok(())
}

#[tokio::test]
async fn edit_invalid_match_mode_and_occurrence_return_validation_contract_errors() -> Result<()> {
    // -- Setup & Fixtures
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("target.txt");
    std::fs::write(&path, "hello\n")?;
    let path_str = path.to_str().ok_or("temp path must be UTF-8")?.to_string();
    let cases: Vec<(&str, serde_json::Value, &str)> = vec![
        (
            "invalid match_mode",
            serde_json::json!({
                "type": "search_replace",
                "search": "hello",
                "replacement": "bye",
                "match_mode": "fuzzy"
            }),
            "Invalid edit.match_mode 'fuzzy'",
        ),
        (
            "invalid occurrence",
            serde_json::json!({
                "type": "search_replace",
                "search": "hello",
                "replacement": "bye",
                "occurrence": "second"
            }),
            "Invalid edit.occurrence 'second'",
        ),
    ];

    // -- Exec & Check
    for (name, operation, expected) in cases {
        let args = serde_json::json!({"path": path_str, "operation": operation});
        let response = execute(dir.path(), args).await?;
        assert_eq!(response["applied"], false, "case '{name}': applied");
        assert_eq!(
            first_diagnostic(&response)?["class"],
            "validation",
            "case '{name}': class"
        );
        let message = diagnostic_message(&response)?;
        assert!(
            message.contains(expected),
            "case '{name}': message must contain {expected:?}, got: {message}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn edit_missing_required_arguments_fail_the_tool_call() -> Result<()> {
    // -- Setup & Fixtures
    let dir = tempfile::tempdir()?;
    let cases: Vec<(&str, serde_json::Value)> = vec![
        (
            "missing path",
            serde_json::json!({"operation": {"type": "create", "content": "x"}}),
        ),
        (
            "missing operation",
            serde_json::json!({"path": "target.txt"}),
        ),
    ];

    // -- Exec & Check
    for (name, args) in cases {
        let error = EditTool::execute(&args, dir.path(), &Bus::default())
            .await
            .err()
            .ok_or("a request without the required fields must fail the tool call")?;
        assert_eq!(
            error.kind,
            crate::tools::handler::ToolErrorKind::Validation,
            "case '{name}': kind"
        );
        assert!(
            error.message.contains("Invalid edit arguments"),
            "case '{name}': message, got: {}",
            error.message
        );
    }
    Ok(())
}

#[tokio::test]
async fn edit_create_with_missing_parent_directory_returns_contract_error() -> Result<()> {
    // -- Setup & Fixtures
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("missing-dir").join("target.txt");
    let args = serde_json::json!({
        "path": path.to_str().ok_or("temp path must be UTF-8")?,
        "operation": {"type": "create", "content": "new content\n"}
    });

    // -- Exec
    let response = execute(dir.path(), args).await?;

    // -- Check
    assert_eq!(response["applied"], false);
    assert_eq!(first_diagnostic(&response)?["class"], "internal");
    assert!(
        diagnostic_message(&response)?.contains("Parent directory does not exist"),
        "got: {}",
        diagnostic_message(&response)?
    );
    assert!(!path.exists(), "a rejected create must not write the file");
    Ok(())
}

#[tokio::test]
async fn edit_regex_all_occurrences_replaces_every_match() -> Result<()> {
    // -- Setup & Fixtures
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("target.txt");
    let seed = "item-1\nitem-2\nitem-3\n";
    std::fs::write(&path, seed)?;
    let args = serde_json::json!({
        "path": path.to_str().ok_or("temp path must be UTF-8")?,
        "expected_version": crate::tools::fs::core::version_token(seed),
        "operation": {
            "type": "search_replace",
            "search": "item-[0-9]",
            "replacement": "entry",
            "match_mode": "regex",
            "occurrence": "all"
        }
    });

    // -- Exec
    let response = execute(dir.path(), args).await?;

    // -- Check
    assert_eq!(response["applied"], true);
    assert_eq!(response["replacements"], 3);
    assert_eq!(
        std::fs::read_to_string(&path)?,
        "entry\nentry\nentry\n",
        "every regex match must be replaced"
    );
    Ok(())
}
