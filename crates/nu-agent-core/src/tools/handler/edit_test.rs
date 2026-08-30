use super::*;
use crate::bus::Bus;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[tokio::test]
async fn edit_apply_search_replace_modifies_file() -> Result<()> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    std::fs::write(&path, "hello world\n").unwrap();
    let version = crate::tools::fs::core::version_token("hello world\n");
    let args = serde_json::json!({
        "path": path.to_str().unwrap(),
        "expected_version": version,
        "operation": {
            "type": "search_replace",
            "search": "world",
            "replacement": "there"
        }
    });
    let result = EditTool::execute(&args, dir.path(), &Bus::default())
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(result["applied"], true);
    assert_eq!(result["changed"], true);
    assert_eq!(result["wrote"], true);
    let content = std::fs::read_to_string(&path).unwrap();
    assert_eq!(content, "hello there\n");
    Ok(())
}

#[tokio::test]
async fn edit_preview_returns_diff_without_writing() -> Result<()> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    std::fs::write(&path, "hello world\n").unwrap();
    let version = crate::tools::fs::core::version_token("hello world\n");
    let args = serde_json::json!({
        "path": path.to_str().unwrap(),
        "mode": "preview",
        "expected_version": version,
        "operation": {
            "type": "search_replace",
            "search": "world",
            "replacement": "there"
        }
    });
    let result = EditTool::execute(&args, dir.path(), &Bus::default())
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(result["applied"], false);
    assert_eq!(result["would_change"], true);
    assert!(result["diff"].as_str().unwrap().contains("world"));
    let content = std::fs::read_to_string(&path).unwrap();
    assert_eq!(content, "hello world\n");
    Ok(())
}

#[tokio::test]
async fn edit_create_creates_new_file() -> Result<()> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("new.txt");
    let args = serde_json::json!({
        "path": path.to_str().unwrap(),
        "operation": {
            "type": "create",
            "content": "new content\n"
        }
    });
    let result = EditTool::execute(&args, dir.path(), &Bus::default())
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(result["applied"], true);
    assert_eq!(result["wrote"], true);
    assert!(path.exists());
    let content = std::fs::read_to_string(&path).unwrap();
    assert_eq!(content, "new content\n");
    Ok(())
}

#[tokio::test]
async fn edit_returns_conflict_on_stale_version() -> Result<()> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    std::fs::write(&path, "hello world\n").unwrap();
    let args = serde_json::json!({
        "path": path.to_str().unwrap(),
        "expected_version": "wrong-version",
        "operation": {
            "type": "search_replace",
            "search": "world",
            "replacement": "there"
        }
    });
    let result = EditTool::execute(&args, dir.path(), &Bus::default())
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(result["conflict"], true);
    assert_eq!(result["applied"], false);
    let content = std::fs::read_to_string(&path).unwrap();
    assert_eq!(content, "hello world\n");
    Ok(())
}

#[tokio::test]
async fn edit_noop_when_no_change() -> Result<()> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    std::fs::write(&path, "hello world\n").unwrap();
    let version = crate::tools::fs::core::version_token("hello world\n");
    let args = serde_json::json!({
        "path": path.to_str().unwrap(),
        "expected_version": version,
        "operation": {
            "type": "search_replace",
            "search": "world",
            "replacement": "world"
        }
    });
    let result = EditTool::execute(&args, dir.path(), &Bus::default())
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(result["noop"], true);
    assert_eq!(result["applied"], false);
    assert_eq!(result["changed"], false);
    Ok(())
}

#[tokio::test]
async fn edit_missing_file_returns_error() -> Result<()> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nonexistent.txt");
    let args = serde_json::json!({
        "path": path.to_str().unwrap(),
        "expected_version": "some-version",
        "operation": {
            "type": "search_replace",
            "search": "foo",
            "replacement": "bar"
        }
    });
    let result = EditTool::execute(&args, dir.path(), &Bus::default())
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(result["applied"], false);
    assert_eq!(result["would_change"], false);
    assert!(result["diagnostics"].as_array().is_some());
    Ok(())
}

#[tokio::test]
async fn edit_json_shape_preserved() -> Result<()> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    std::fs::write(&path, "hello world\n").unwrap();
    let version = crate::tools::fs::core::version_token("hello world\n");
    let args = serde_json::json!({
        "path": path.to_str().unwrap(),
        "expected_version": version,
        "operation": {
            "type": "search_replace",
            "search": "world",
            "replacement": "there"
        }
    });
    let result = EditTool::execute(&args, dir.path(), &Bus::default())
        .await
        .map_err(|e| format!("{e:?}"))?;
    for field in [
        "path",
        "mode",
        "applied",
        "would_change",
        "diff",
        "stats",
        "diagnostics",
        "changed",
        "wrote",
        "noop",
        "conflict",
    ] {
        assert!(result.get(field).is_some(), "missing field: {field}");
    }
    Ok(())
}
