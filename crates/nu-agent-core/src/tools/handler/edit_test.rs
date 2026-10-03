use super::*;
use crate::bus::Bus;
use crate::protocol::tool_args::CallLine;
use crate::tools::handler::builtin_tool::{BuiltinTool, Previewable};
use crate::transcript::ir::ContentKind;

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
    let error = EditTool::execute(&args, dir.path(), &Bus::default())
        .await
        .err()
        .ok_or("stale expected_version must fail the tool call")?;
    assert!(
        error.message.contains("conflict"),
        "conflict message must name the conflict, got: {}",
        error.message
    );
    let details = error
        .details
        .ok_or("conflict error must carry the contract response")?;
    assert_eq!(details["conflict"], true);
    assert_eq!(details["applied"], false);
    assert_eq!(details["diff"], "");
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
    let error = EditTool::execute(&args, dir.path(), &Bus::default())
        .await
        .err()
        .ok_or("a replacement identical to the search must fail the tool call")?;
    assert!(
        error.message.contains("no change"),
        "noop message must name the no-op, got: {}",
        error.message
    );
    let details = error
        .details
        .ok_or("noop error must carry the contract response")?;
    assert_eq!(details["noop"], true);
    assert_eq!(details["applied"], false);
    assert_eq!(details["changed"], false);
    Ok(())
}

#[tokio::test]
async fn edit_search_not_found_returns_no_change_error() -> Result<()> {
    // -- Setup & Fixtures
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("test.txt");
    std::fs::write(&path, "hello world\n")?;
    let version = crate::tools::fs::core::version_token("hello world\n");
    let args = serde_json::json!({
        "path": path.to_str().unwrap(),
        "expected_version": version,
        "operation": {
            "type": "search_replace",
            "search": "absent-token",
            "replacement": "there"
        }
    });

    // -- Exec
    let error = EditTool::execute(&args, dir.path(), &Bus::default())
        .await
        .err()
        .ok_or("a search string that is not found must fail the tool call")?;

    // -- Check
    assert!(
        error.message.contains("no change"),
        "not-found message must name the no-op, got: {}",
        error.message
    );
    let details = error
        .details
        .ok_or("not-found error must carry the contract response")?;
    assert_eq!(details["noop"], true);
    assert_eq!(details["replacements"], 0);
    assert_eq!(details["diff"], "");
    Ok(())
}

#[tokio::test]
async fn edit_create_existing_file_returns_already_exists_error() -> Result<()> {
    // -- Setup & Fixtures
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("existing.txt");
    std::fs::write(&path, "already here\n")?;
    let args = serde_json::json!({
        "path": path.to_str().unwrap(),
        "operation": {"type": "create", "content": "new content\n"}
    });

    // -- Exec
    let error = EditTool::execute(&args, dir.path(), &Bus::default())
        .await
        .err()
        .ok_or("creating an existing file must fail the tool call")?;

    // -- Check
    assert!(
        error.message.contains("already exists"),
        "create-conflict message must name the existing file, got: {}",
        error.message
    );
    let details = error
        .details
        .ok_or("create-conflict error must carry the contract response")?;
    assert_eq!(details["conflict"], true);
    assert_eq!(details["applied"], false);
    assert_eq!(std::fs::read_to_string(&path)?, "already here\n");
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

// === Previewable ===

#[test]
fn edit_preview_create_operation_returns_diff_display() -> Result<()> {
    // -- Setup & Fixtures
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("new-file.txt");
    let args = serde_json::json!({
        "path": path.to_string_lossy(),
        "operation": {"type": "create", "content": "hello\n"}
    });

    // -- Exec
    let display = EditTool::preview(&args, dir.path());

    // -- Check
    let display = display.ok_or("preview must produce a display for a valid create")?;
    assert_eq!(display.title, format!("edit {}", path.to_string_lossy()));
    assert_eq!(display.sections.len(), 1);
    let section = &display.sections[0];
    assert!(
        matches!(section.kind, ContentKind::Diff { .. }),
        "section kind must be Diff, got {:?}",
        section.kind
    );
    assert!(
        section.content.contains("+hello"),
        "diff must contain the created content, got: {:?}",
        section.content
    );
    let stats = section
        .stats
        .clone()
        .ok_or("diff section must carry stats")?;
    assert_eq!(stats.files_changed, Some(1));
    Ok(())
}

#[test]
fn edit_preview_search_replace_returns_diff_display() -> Result<()> {
    // -- Setup & Fixtures
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("existing.txt");
    std::fs::write(&path, "hello\nworld\n")?;
    let args = serde_json::json!({
        "path": path.to_string_lossy(),
        "mode": "apply",
        "operation": {
            "type": "search_replace",
            "search": "world",
            "replacement": "there"
        }
    });

    // -- Exec
    let display = EditTool::preview(&args, dir.path());

    // -- Check
    let display = display.ok_or("preview must produce a display")?;
    let section = &display.sections[0];
    assert!(
        section.content.contains("-world") && section.content.contains("+there"),
        "diff must contain the replacement, got: {:?}",
        section.content
    );
    Ok(())
}

#[test]
fn edit_preview_invalid_args_returns_none() -> Result<()> {
    // -- Setup & Fixtures
    let dir = tempfile::tempdir()?;
    let args = serde_json::json!({"path": "nowhere.txt"});

    // -- Exec & Check
    assert!(
        EditTool::preview(&args, dir.path()).is_none(),
        "invalid edit arguments must produce no preview"
    );
    Ok(())
}

#[test]
fn edit_preview_noop_returns_none() -> Result<()> {
    // -- Setup & Fixtures
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("existing.txt");
    std::fs::write(&path, "hello\nworld\n")?;
    let args = serde_json::json!({
        "path": path.to_string_lossy(),
        "mode": "apply",
        "operation": {
            "type": "search_replace",
            "search": "world",
            "replacement": "world"
        }
    });

    // -- Exec & Check
    assert!(
        EditTool::preview(&args, dir.path()).is_none(),
        "a no-op plan has no diff to approve; preview must return None"
    );
    Ok(())
}

#[test]
fn edit_preview_search_not_found_returns_none() -> Result<()> {
    // -- Setup & Fixtures
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("existing.txt");
    std::fs::write(&path, "hello\nworld\n")?;
    let args = serde_json::json!({
        "path": path.to_string_lossy(),
        "mode": "apply",
        "operation": {
            "type": "search_replace",
            "search": "absent-token",
            "replacement": "there"
        }
    });

    // -- Exec & Check
    assert!(
        EditTool::preview(&args, dir.path()).is_none(),
        "an unmatched search has no diff to approve; preview must return None"
    );
    Ok(())
}

#[test]
fn edit_preview_stale_version_returns_none() -> Result<()> {
    // -- Setup & Fixtures
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("existing.txt");
    std::fs::write(&path, "hello\nworld\n")?;
    let args = serde_json::json!({
        "path": path.to_string_lossy(),
        "mode": "apply",
        "expected_version": "deadbeef",
        "operation": {
            "type": "search_replace",
            "search": "world",
            "replacement": "there"
        }
    });

    // -- Exec & Check
    assert!(
        EditTool::preview(&args, dir.path()).is_none(),
        "a version conflict has no diff to approve; preview must return None"
    );
    Ok(())
}

#[test]
fn edit_preview_create_existing_file_returns_none() -> Result<()> {
    // -- Setup & Fixtures
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("existing.txt");
    std::fs::write(&path, "already here\n")?;
    let args = serde_json::json!({
        "path": path.to_string_lossy(),
        "operation": {"type": "create", "content": "new content\n"}
    });

    // -- Exec & Check
    assert!(
        EditTool::preview(&args, dir.path()).is_none(),
        "creating an existing file has no diff to approve; preview must return None"
    );
    Ok(())
}

// === call_line_render ===

#[test]
fn edit_call_line_render_shows_path_and_diff_marker() -> Result<()> {
    // -- Setup & Fixtures
    let args = r#"{"path":"/tmp/f.txt"}"#;

    // -- Exec
    let render = EditTool::call_line_render(args);

    // -- Check
    assert_eq!(
        render,
        CallLine {
            summary: "→ /tmp/f.txt (diff)".to_string(),
        }
    );
    Ok(())
}

#[test]
fn edit_call_line_render_falls_back_to_generic_on_invalid_json() -> Result<()> {
    // -- Setup & Fixtures
    let args = "not-json";

    // -- Exec
    let render = EditTool::call_line_render(args);

    // -- Check
    assert_eq!(render, CallLine::from_json_summary(args));
    Ok(())
}
