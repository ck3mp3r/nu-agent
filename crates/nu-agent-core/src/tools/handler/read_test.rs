use super::*;
use crate::bus::Bus;

#[tokio::test]
async fn read_returns_file_content() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    std::fs::write(&path, "hello world\n").unwrap();
    let args = serde_json::json!({ "path": path.to_str().unwrap() });
    let result = ReadTool::execute(&args, dir.path(), &Bus::new())
        .await
        .unwrap();
    assert!(result["content"].as_str().unwrap().contains("hello world"));
}

#[tokio::test]
async fn read_returns_total_lines() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    std::fs::write(&path, "line1\nline2\nline3\n").unwrap();
    let args = serde_json::json!({ "path": path.to_str().unwrap() });
    let result = ReadTool::execute(&args, dir.path(), &Bus::new())
        .await
        .unwrap();
    assert_eq!(result["total_lines"], 3);
}

#[tokio::test]
async fn read_respects_offset() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    std::fs::write(&path, "line1\nline2\nline3\nline4\nline5\n").unwrap();
    let args = serde_json::json!({ "path": path.to_str().unwrap(), "offset": 2, "limit": 3 });
    let result = ReadTool::execute(&args, dir.path(), &Bus::new())
        .await
        .unwrap();
    let content = result["content"].as_str().unwrap();
    assert!(!content.contains("line1"));
    assert!(content.contains("line3"));
}

#[tokio::test]
async fn read_respects_limit() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    std::fs::write(&path, "line1\nline2\nline3\nline4\nline5\n").unwrap();
    let args = serde_json::json!({ "path": path.to_str().unwrap(), "offset": 0, "limit": 3 });
    let result = ReadTool::execute(&args, dir.path(), &Bus::new())
        .await
        .unwrap();
    let content = result["content"].as_str().unwrap();
    assert!(content.contains("line1"));
    assert!(content.contains("line3"));
    assert!(!content.contains("line4"));
}

#[tokio::test]
async fn read_missing_file_returns_runtime_error() {
    let dir = tempfile::tempdir().unwrap();
    let args = serde_json::json!({ "path": "/nonexistent/path/file.txt" });
    let result = ReadTool::execute(&args, dir.path(), &Bus::new()).await;
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err().kind,
        super::super::ToolErrorKind::Runtime
    );
}

#[tokio::test]
async fn read_returns_version() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    std::fs::write(&path, "content\n").unwrap();
    let args = serde_json::json!({ "path": path.to_str().unwrap() });
    let result = ReadTool::execute(&args, dir.path(), &Bus::new())
        .await
        .unwrap();
    assert!(result["version"].as_str().is_some_and(|v| !v.is_empty()));
}
