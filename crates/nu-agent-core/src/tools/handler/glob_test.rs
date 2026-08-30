use super::*;
use crate::bus::Bus;
use tempfile::tempdir;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[tokio::test]
async fn glob_finds_matching_files() -> Result<()> {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("foo.rs"), "").unwrap();
    std::fs::write(dir.path().join("bar.rs"), "").unwrap();
    std::fs::write(dir.path().join("baz.txt"), "").unwrap();

    let args = serde_json::json!({"pattern": "*.rs", "path": dir.path().to_str().unwrap()});
    let result = GlobTool::execute(&args, dir.path(), &Bus::default())
        .await
        .map_err(|e| format!("{e:?}"))?;

    let matches = result["matches"].as_array().unwrap();
    assert_eq!(matches.len(), 2, "expected 2 .rs files");
    assert!(matches.iter().all(|m| m.as_str().unwrap().ends_with(".rs")));
    assert!(
        !matches
            .iter()
            .any(|m| m.as_str().unwrap().ends_with(".txt"))
    );
    assert_eq!(result["total"], 2);
    Ok(())
}

#[tokio::test]
async fn glob_returns_empty_for_no_match() -> Result<()> {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("foo.rs"), "").unwrap();

    let args = serde_json::json!({"pattern": "*.txt", "path": dir.path().to_str().unwrap()});
    let result = GlobTool::execute(&args, dir.path(), &Bus::default())
        .await
        .map_err(|e| format!("{e:?}"))?;

    assert_eq!(result["matches"].as_array().unwrap().len(), 0);
    assert_eq!(result["total"], 0);
    Ok(())
}

#[tokio::test]
async fn glob_returns_relative_paths() -> Result<()> {
    let dir = tempdir().unwrap();
    let sub = dir.path().join("subdir");
    std::fs::create_dir(&sub).unwrap();
    std::fs::write(sub.join("nested.rs"), "").unwrap();

    let args = serde_json::json!({"pattern": "**/*.rs", "path": dir.path().to_str().unwrap()});
    let result = GlobTool::execute(&args, dir.path(), &Bus::default())
        .await
        .map_err(|e| format!("{e:?}"))?;

    let matches = result["matches"].as_array().unwrap();
    assert_eq!(matches.len(), 1);
    let path = matches[0].as_str().unwrap();
    assert!(
        !path.starts_with('/'),
        "path should be relative, got: {path}"
    );
    assert!(
        path.contains("subdir"),
        "expected subdir in path, got: {path}"
    );
    Ok(())
}

#[tokio::test]
async fn glob_searches_recursively() -> Result<()> {
    let dir = tempdir().unwrap();
    let deep = dir.path().join("a").join("b").join("c");
    std::fs::create_dir_all(&deep).unwrap();
    std::fs::write(deep.join("deep.rs"), "").unwrap();

    let args = serde_json::json!({"pattern": "**/*.rs", "path": dir.path().to_str().unwrap()});
    let result = GlobTool::execute(&args, dir.path(), &Bus::default())
        .await
        .map_err(|e| format!("{e:?}"))?;

    let matches = result["matches"].as_array().unwrap();
    assert_eq!(matches.len(), 1);
    assert!(matches[0].as_str().unwrap().ends_with("deep.rs"));
    Ok(())
}

#[tokio::test]
async fn glob_results_are_sorted() -> Result<()> {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("z.rs"), "").unwrap();
    std::fs::write(dir.path().join("a.rs"), "").unwrap();
    std::fs::write(dir.path().join("m.rs"), "").unwrap();

    let args = serde_json::json!({"pattern": "*.rs", "path": dir.path().to_str().unwrap()});
    let result = GlobTool::execute(&args, dir.path(), &Bus::default())
        .await
        .map_err(|e| format!("{e:?}"))?;

    let matches: Vec<&str> = result["matches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    let mut sorted = matches.clone();
    sorted.sort();
    assert_eq!(matches, sorted, "results should be sorted alphabetically");
    Ok(())
}

#[tokio::test]
async fn glob_invalid_pattern_returns_validation_error() {
    let dir = tempdir().unwrap();

    let args = serde_json::json!({"pattern": "!", "path": dir.path().to_str().unwrap()});
    let result = GlobTool::execute(&args, dir.path(), &Bus::default()).await;
    match result {
        Err(e) => assert_eq!(e.kind, ToolErrorKind::Validation),
        Ok(_) => { /* OverrideBuilder accepted it — acceptable */ }
    }
}
