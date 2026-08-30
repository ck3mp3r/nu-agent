use super::*;
use crate::bus::Bus;
use std::io::Write;
use tempfile::tempdir;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[tokio::test]
async fn grep_finds_literal_match() -> Result<()> {
    let dir = tempdir().unwrap();
    let file = dir.path().join("foo.rs");
    std::fs::write(&file, "fn hello_world() {}\n").unwrap();

    let args = serde_json::json!({"pattern": "hello_world", "path": dir.path().to_str().unwrap()});
    let result = GrepTool::execute(&args, dir.path(), &Bus::default())
        .await
        .map_err(|e| format!("{e:?}"))?;

    let matches = result["matches"].as_array().unwrap();
    assert_eq!(matches.len(), 1);
    assert!(matches[0]["file"].as_str().unwrap().contains("foo.rs"));
    assert_eq!(matches[0]["line"], 1);
    assert!(
        matches[0]["content"]
            .as_str()
            .unwrap()
            .contains("hello_world")
    );
    assert_eq!(result["truncated"], false);
    Ok(())
}

#[tokio::test]
async fn grep_returns_empty_when_no_match() -> Result<()> {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("foo.rs"), "fn unrelated() {}\n").unwrap();

    let args =
        serde_json::json!({"pattern": "xyzzy_no_match", "path": dir.path().to_str().unwrap()});
    let result = GrepTool::execute(&args, dir.path(), &Bus::default())
        .await
        .map_err(|e| format!("{e:?}"))?;

    assert_eq!(result["matches"].as_array().unwrap().len(), 0);
    assert_eq!(result["total"], 0);
    assert_eq!(result["truncated"], false);
    Ok(())
}

#[tokio::test]
async fn grep_case_insensitive_flag() -> Result<()> {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("foo.rs"), "Hello World\n").unwrap();

    let sensitive = serde_json::json!({"pattern": "hello world", "path": dir.path().to_str().unwrap(), "case_insensitive": false});
    let sensitive_res = GrepTool::execute(&sensitive, dir.path(), &Bus::default())
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(sensitive_res["matches"].as_array().unwrap().len(), 0);

    let insensitive = serde_json::json!({"pattern": "hello world", "path": dir.path().to_str().unwrap(), "case_insensitive": true});
    let insensitive_res = GrepTool::execute(&insensitive, dir.path(), &Bus::default())
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(insensitive_res["matches"].as_array().unwrap().len(), 1);
    Ok(())
}

#[tokio::test]
async fn grep_glob_filter_limits_files() -> Result<()> {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("foo.rs"), "needle\n").unwrap();
    std::fs::write(dir.path().join("foo.txt"), "needle\n").unwrap();

    let rs_only = serde_json::json!({"pattern": "needle", "path": dir.path().to_str().unwrap(), "glob": "*.rs"});
    let rs_res = GrepTool::execute(&rs_only, dir.path(), &Bus::default())
        .await
        .map_err(|e| format!("{e:?}"))?;

    let txt_only = serde_json::json!({"pattern": "needle", "path": dir.path().to_str().unwrap(), "glob": "*.txt"});
    let txt_res = GrepTool::execute(&txt_only, dir.path(), &Bus::default())
        .await
        .map_err(|e| format!("{e:?}"))?;

    assert_eq!(rs_res["matches"].as_array().unwrap().len(), 1);
    assert!(
        rs_res["matches"][0]["file"]
            .as_str()
            .unwrap()
            .ends_with(".rs")
    );

    assert_eq!(txt_res["matches"].as_array().unwrap().len(), 1);
    assert!(
        txt_res["matches"][0]["file"]
            .as_str()
            .unwrap()
            .ends_with(".txt")
    );
    Ok(())
}

#[tokio::test]
async fn grep_respects_max_results_and_sets_truncated() -> Result<()> {
    let dir = tempdir().unwrap();
    let content = (1..=10)
        .map(|i| format!("match line {i}\n"))
        .collect::<String>();
    std::fs::write(dir.path().join("foo.rs"), content).unwrap();

    let capped = serde_json::json!({"pattern": "match line", "path": dir.path().to_str().unwrap(), "max_results": 3});
    let capped_res = GrepTool::execute(&capped, dir.path(), &Bus::default())
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(capped_res["matches"].as_array().unwrap().len(), 3);
    assert_eq!(capped_res["truncated"], true);

    let uncapped = serde_json::json!({"pattern": "match line", "path": dir.path().to_str().unwrap(), "max_results": 20});
    let uncapped_res = GrepTool::execute(&uncapped, dir.path(), &Bus::default())
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(uncapped_res["matches"].as_array().unwrap().len(), 10);
    assert_eq!(uncapped_res["truncated"], false);
    Ok(())
}

#[tokio::test]
async fn grep_invalid_regex_returns_validation_error() {
    let dir = tempdir().unwrap();
    let args = serde_json::json!({"pattern": "[invalid", "path": dir.path().to_str().unwrap()});
    let err = GrepTool::execute(&args, dir.path(), &Bus::default())
        .await
        .unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Validation);
    assert!(err.message.contains("Invalid regex pattern"));
}

#[tokio::test]
async fn grep_searches_subdirectories_recursively() -> Result<()> {
    let dir = tempdir().unwrap();
    let sub = dir.path().join("subdir");
    std::fs::create_dir(&sub).unwrap();
    std::fs::write(sub.join("nested.rs"), "deep_match\n").unwrap();

    let args = serde_json::json!({"pattern": "deep_match", "path": dir.path().to_str().unwrap()});
    let result = GrepTool::execute(&args, dir.path(), &Bus::default())
        .await
        .map_err(|e| format!("{e:?}"))?;

    let matches = result["matches"].as_array().unwrap();
    assert_eq!(matches.len(), 1);
    let file = matches[0]["file"].as_str().unwrap();
    assert!(
        file.contains("subdir"),
        "expected subdir in path, got: {file}"
    );
    assert!(file.ends_with("nested.rs"));
    Ok(())
}

#[tokio::test]
async fn grep_skips_binary_files_without_error() {
    let dir = tempdir().unwrap();
    let mut f = std::fs::File::create(dir.path().join("binary.bin")).unwrap();
    f.write_all(&[0u8, 1u8, 2u8, 0u8]).unwrap();
    std::fs::write(dir.path().join("normal.rs"), "no match here\n").unwrap();

    let args = serde_json::json!({"pattern": "anything", "path": dir.path().to_str().unwrap()});
    let result = GrepTool::execute(&args, dir.path(), &Bus::default()).await;
    assert!(
        result.is_ok(),
        "should not error on binary files: {result:?}"
    );
}
