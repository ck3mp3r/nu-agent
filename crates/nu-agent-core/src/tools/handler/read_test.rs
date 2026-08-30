use super::*;
use crate::bus::Bus;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[tokio::test]
async fn read_returns_file_content() -> Result<()> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    std::fs::write(&path, "hello world\n").unwrap();
    let args = serde_json::json!({ "path": path.to_str().unwrap() });
    let result = ReadTool::execute(&args, dir.path(), &Bus::default())
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert!(result["content"].as_str().unwrap().contains("hello world"));
    Ok(())
}

#[tokio::test]
async fn read_returns_total_lines() -> Result<()> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    std::fs::write(&path, "line1\nline2\nline3\n").unwrap();
    let args = serde_json::json!({ "path": path.to_str().unwrap() });
    let result = ReadTool::execute(&args, dir.path(), &Bus::default())
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(result["total_lines"], 3);
    Ok(())
}

#[tokio::test]
async fn read_respects_offset() -> Result<()> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    std::fs::write(&path, "line1\nline2\nline3\nline4\nline5\n").unwrap();
    let args = serde_json::json!({ "path": path.to_str().unwrap(), "offset": 2, "limit": 3 });
    let result = ReadTool::execute(&args, dir.path(), &Bus::default())
        .await
        .map_err(|e| format!("{e:?}"))?;
    let content = result["content"].as_str().unwrap();
    assert!(!content.contains("line1"));
    assert!(content.contains("line3"));
    Ok(())
}

#[tokio::test]
async fn read_respects_limit() -> Result<()> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    std::fs::write(&path, "line1\nline2\nline3\nline4\nline5\n").unwrap();
    let args = serde_json::json!({ "path": path.to_str().unwrap(), "offset": 0, "limit": 3 });
    let result = ReadTool::execute(&args, dir.path(), &Bus::default())
        .await
        .map_err(|e| format!("{e:?}"))?;
    let content = result["content"].as_str().unwrap();
    assert!(content.contains("line1"));
    assert!(content.contains("line3"));
    assert!(!content.contains("line4"));
    Ok(())
}

#[tokio::test]
async fn read_missing_file_returns_runtime_error() {
    let dir = tempfile::tempdir().unwrap();
    let args = serde_json::json!({ "path": "/nonexistent/path/file.txt" });
    let result = ReadTool::execute(&args, dir.path(), &Bus::default()).await;
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err().kind,
        super::super::ToolErrorKind::Runtime
    );
}

#[tokio::test]
async fn read_returns_version() -> Result<()> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    std::fs::write(&path, "content\n").unwrap();
    let args = serde_json::json!({ "path": path.to_str().unwrap() });
    let result = ReadTool::execute(&args, dir.path(), &Bus::default())
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert!(result["version"].as_str().is_some_and(|v| !v.is_empty()));
    Ok(())
}
