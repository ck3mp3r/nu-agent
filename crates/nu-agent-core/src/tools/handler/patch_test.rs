use super::*;
use crate::bus::Bus;
use crate::tools::fs::core::version_token;

#[tokio::test]
async fn patch_applies_single_range_replacement() {
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
    let result = PatchTool::execute(&args, dir.path(), &Bus::new())
        .await
        .unwrap();
    assert_eq!(result["changed"], true);
    assert_eq!(result["operation_count"], 1);
}

#[tokio::test]
async fn patch_applies_multiple_operations() {
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
    let result = PatchTool::execute(&args, dir.path(), &Bus::new())
        .await
        .unwrap();
    assert_eq!(result["operation_count"], 2);
    assert_eq!(result["changed"], true);
}

#[tokio::test]
async fn patch_returns_conflict_on_stale_version() {
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
    let result = PatchTool::execute(&args, dir.path(), &Bus::new())
        .await
        .unwrap();
    assert_eq!(result["conflict"], true);
    assert_eq!(result["changed"], false);
}

#[tokio::test]
async fn patch_noop_when_replacement_matches_existing() {
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
    let result = PatchTool::execute(&args, dir.path(), &Bus::new())
        .await
        .unwrap();
    assert_eq!(result["noop"], true);
    assert_eq!(result["changed"], false);
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
    let result = PatchTool::execute(&args, dir.path(), &Bus::new()).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn patch_returns_version_info() {
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
    let result = PatchTool::execute(&args, dir.path(), &Bus::new())
        .await
        .unwrap();
    let previous = result["previous_version"].as_str().unwrap();
    let new = result["new_version"].as_str().unwrap();
    assert!(!previous.is_empty());
    assert!(!new.is_empty());
    assert_ne!(previous, new);
}
