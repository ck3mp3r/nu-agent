use super::*;
use crate::bus::Bus;
use crate::protocol::tool_args::CallLineRender;
use crate::tools::fs::core::version_token;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[tokio::test]
async fn patch_applies_single_range_replacement() -> Result<()> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    let content = "line1\nline2\nline3\nline4\nline5\n";
    std::fs::write(&path, content).unwrap();
    let args = serde_json::json!({
        "path": path.to_str().unwrap(),
        "expected_version": version_token(content),
        "operations": [
            {"range": {"start": 2, "end": 3}, "replacement": "replaced\n"}
        ]
    });
    let result = PatchTool::execute(&args, dir.path(), &Bus::default())
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(result["changed"], true);
    assert_eq!(result["operation_count"], 1);
    Ok(())
}

#[tokio::test]
async fn patch_applies_multiple_operations() -> Result<()> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    let content = "line1\nline2\nline3\nline4\nline5\n";
    std::fs::write(&path, content).unwrap();
    let args = serde_json::json!({
        "path": path.to_str().unwrap(),
        "expected_version": version_token(content),
        "operations": [
            {"range": {"start": 1, "end": 1}, "replacement": "first\n"},
            {"range": {"start": 5, "end": 5}, "replacement": "last\n"}
        ]
    });
    let result = PatchTool::execute(&args, dir.path(), &Bus::default())
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(result["operation_count"], 2);
    assert_eq!(result["changed"], true);
    Ok(())
}

#[tokio::test]
async fn patch_returns_conflict_on_stale_version() -> Result<()> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    let content = "line1\nline2\nline3\n";
    std::fs::write(&path, content).unwrap();
    let args = serde_json::json!({
        "path": path.to_str().unwrap(),
        "expected_version": "wrong-version",
        "operations": [
            {"range": {"start": 1, "end": 1}, "replacement": "x\n"}
        ]
    });
    let result = PatchTool::execute(&args, dir.path(), &Bus::default())
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(result["conflict"], true);
    assert_eq!(result["changed"], false);
    Ok(())
}

#[tokio::test]
async fn patch_noop_when_replacement_matches_existing() -> Result<()> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    let content = "line1\nline2\nline3\n";
    std::fs::write(&path, content).unwrap();
    let args = serde_json::json!({
        "path": path.to_str().unwrap(),
        "expected_version": version_token(content),
        "operations": [
            {"range": {"start": 2, "end": 2}, "replacement": "line2\n"}
        ]
    });
    let result = PatchTool::execute(&args, dir.path(), &Bus::default())
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(result["noop"], true);
    assert_eq!(result["changed"], false);
    Ok(())
}

#[tokio::test]
async fn patch_missing_file_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let args = serde_json::json!({
        "path": "/nonexistent/path/file.txt",
        "expected_version": "any",
        "operations": [
            {"range": {"start": 1, "end": 1}, "replacement": "x\n"}
        ]
    });
    let result = PatchTool::execute(&args, dir.path(), &Bus::default()).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn patch_returns_version_info() -> Result<()> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    let content = "line1\nline2\nline3\n";
    std::fs::write(&path, content).unwrap();
    let args = serde_json::json!({
        "path": path.to_str().unwrap(),
        "expected_version": version_token(content),
        "operations": [
            {"range": {"start": 1, "end": 1}, "replacement": "changed\n"}
        ]
    });
    let result = PatchTool::execute(&args, dir.path(), &Bus::default())
        .await
        .map_err(|e| format!("{e:?}"))?;
    let previous = result["previous_version"].as_str().unwrap();
    let new = result["new_version"].as_str().unwrap();
    assert!(!previous.is_empty());
    assert!(!new.is_empty());
    assert_ne!(previous, new);
    Ok(())
}

// === call_line_render ===

#[test]
fn patch_call_line_render_shows_path_and_operation_count() -> Result<()> {
    // -- Setup & Fixtures
    let args = r#"{"path":"/tmp/f.txt","operations":[{"range":{"start":1,"end":1},"replacement":"a\n"},{"range":{"start":2,"end":2},"replacement":"b\n"}]}"#;

    // -- Exec
    let render = PatchTool::call_line_render(args);

    // -- Check
    assert_eq!(
        render,
        CallLineRender::Inline {
            summary: "→ /tmp/f.txt (2 ops)".to_string(),
        }
    );
    Ok(())
}

#[test]
fn patch_call_line_render_falls_back_to_generic_on_invalid_json() -> Result<()> {
    // -- Setup & Fixtures
    let args = "not-json";

    // -- Exec
    let render = PatchTool::call_line_render(args);

    // -- Check
    assert_eq!(render, CallLineRender::generic_json_summary(args));
    Ok(())
}
