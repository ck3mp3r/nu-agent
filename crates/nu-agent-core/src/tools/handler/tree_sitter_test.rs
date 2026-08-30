use super::*;
use crate::bus::Bus;
use crate::tools::handler::ToolErrorKind;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn bounded_tree_cache_evicts_oldest_when_over_capacity() {
    let mut cache = BoundedTreeCache::<u32>::new();
    for i in 0..TREE_CACHE_CAPACITY + 1 {
        cache.insert(PathBuf::from(format!("file-{i}.rs")), i as u32);
    }
    // The first inserted entry (file-0.rs) must have been evicted.
    assert!(cache.get(Path::new("file-0.rs")).is_none());
    // The most recently inserted entry is still present.
    assert_eq!(
        cache.get(Path::new(&format!("file-{}.rs", TREE_CACHE_CAPACITY))),
        Some(&(TREE_CACHE_CAPACITY as u32))
    );
    assert_eq!(cache.map.len(), TREE_CACHE_CAPACITY);
}

#[test]
fn bounded_tree_cache_get_refreshes_lru_order() {
    let mut cache = BoundedTreeCache::<u32>::new();
    for i in 0..TREE_CACHE_CAPACITY {
        cache.insert(PathBuf::from(format!("file-{i}.rs")), i as u32);
    }
    // Access the oldest entry, making it most recently used.
    cache.get(Path::new("file-0.rs"));
    // Insert one more entry; the least recently used (file-1.rs) is evicted.
    cache.insert(PathBuf::from("file-new.rs"), 999);
    assert!(cache.get(Path::new("file-1.rs")).is_none());
    assert_eq!(cache.get(Path::new("file-0.rs")), Some(&0));
    assert_eq!(cache.get(Path::new("file-new.rs")), Some(&999));
}

#[tokio::test]
async fn ast_query_tool_missing_query_returns_validation_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.rs");
    std::fs::write(&path, "fn main() {}").unwrap();
    let args = serde_json::json!({ "path": path.to_str().unwrap(), "language": "rust" });
    let err = AstQueryTool::execute(&args, dir.path(), &Bus::default())
        .await
        .unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Validation);
}

#[tokio::test]
async fn ast_nodes_tool_missing_node_type_returns_validation_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.rs");
    std::fs::write(&path, "fn main() {}").unwrap();
    let args = serde_json::json!({ "path": path.to_str().unwrap(), "language": "rust" });
    let err = AstNodesTool::execute(&args, dir.path(), &Bus::default())
        .await
        .unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Validation);
}

#[tokio::test]
async fn ast_refs_tool_missing_name_returns_validation_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.rs");
    std::fs::write(&path, "fn main() {}").unwrap();
    let args = serde_json::json!({ "path": path.to_str().unwrap(), "language": "rust" });
    let err = AstRefsTool::execute(&args, dir.path(), &Bus::default())
        .await
        .unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Validation);
}

#[tokio::test]
async fn ast_tree_tool_missing_language_returns_validation_error() {
    let dir = tempfile::tempdir().unwrap();
    let args = serde_json::json!({ "path": "main.rs" });
    let err = AstTreeTool::execute(&args, dir.path(), &Bus::default())
        .await
        .unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Validation);
}

#[tokio::test]
#[serial_test::serial]
async fn missing_config_returns_runtime_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.rs");
    std::fs::write(&path, "fn main() {}").unwrap();
    let args = serde_json::json!({ "path": path.to_str().unwrap(), "language": "rust" });
    // Point TREE_SITTER_DIR at a nonexistent path so tree_sitter_config finds no
    // config regardless of the host's actual tree-sitter setup.
    let missing_dir = dir.path().join("nonexistent-tree-sitter-dir");
    let previous = std::env::var_os("TREE_SITTER_DIR");
    unsafe { std::env::set_var("TREE_SITTER_DIR", &missing_dir) };
    let result = AstTreeTool::execute(&args, dir.path(), &Bus::default()).await;
    match previous {
        Some(v) => unsafe { std::env::set_var("TREE_SITTER_DIR", v) },
        None => unsafe { std::env::remove_var("TREE_SITTER_DIR") },
    }
    let err = result.unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Runtime);
    assert!(err.message.contains("No tree-sitter config found"));
}
#[tokio::test]
#[ignore]
async fn ast_query_tool_finds_function_names() -> Result<()> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.rs");
    std::fs::write(&path, "fn foo() {}\nfn bar() {}\n").unwrap();
    let args = serde_json::json!({
        "path": path.to_str().unwrap(),
        "language": "rust",
        "query": "(function_item name: (identifier) @fn-name)"
    });
    let result = AstQueryTool::execute(&args, dir.path(), &Bus::default())
        .await
        .map_err(|e| format!("{e:?}"))?;
    let matches = result["matches"].as_array().unwrap();
    assert!(!matches.is_empty());
    let names: Vec<String> = matches
        .iter()
        .filter_map(|m| m["captures"]["fn-name"]["text"].as_str().map(String::from))
        .collect();
    assert!(names.contains(&"foo".to_string()));
    assert!(names.contains(&"bar".to_string()));
    Ok(())
}
#[tokio::test]
#[ignore]
async fn ast_nodes_tool_finds_match_arms() -> Result<()> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.rs");
    std::fs::write(
        &path,
        "fn main() {\n    let x = 1;\n    match x {\n        1 => println!(\"one\"),\n        _ => println!(\"other\"),\n    }\n}\n",
    )
    .unwrap();
    let args = serde_json::json!({
        "path": path.to_str().unwrap(),
        "language": "rust",
        "node_type": "match_arm"
    });
    let result = AstNodesTool::execute(&args, dir.path(), &Bus::default())
        .await
        .map_err(|e| format!("{e:?}"))?;
    let nodes = result["nodes"].as_array().unwrap();
    assert!(!nodes.is_empty());
    assert_eq!(result["node_type"], "match_arm");
    Ok(())
}

#[tokio::test]
#[ignore]
async fn ast_refs_tool_finds_symbol() -> Result<()> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.rs");
    std::fs::write(
        &path,
        "struct Foo {}\nfn main() {\n    let x: Foo = Foo;\n}\n",
    )
    .unwrap();
    let args = serde_json::json!({
        "path": path.to_str().unwrap(),
        "language": "rust",
        "name": "Foo"
    });
    let result = AstRefsTool::execute(&args, dir.path(), &Bus::default())
        .await
        .map_err(|e| format!("{e:?}"))?;
    let matches = result["matches"].as_array().unwrap();
    assert!(!matches.is_empty());
    let texts: Vec<String> = matches
        .iter()
        .filter_map(|m| m["captures"]["name"]["text"].as_str().map(String::from))
        .collect();
    assert!(texts.iter().all(|t| t == "Foo"));
    Ok(())
}

#[tokio::test]
#[ignore]
async fn ast_tree_tool_returns_sexp() -> Result<()> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.rs");
    std::fs::write(&path, "fn main() {}").unwrap();
    let args = serde_json::json!({ "path": path.to_str().unwrap(), "language": "rust" });
    let result = AstTreeTool::execute(&args, dir.path(), &Bus::default())
        .await
        .map_err(|e| format!("{e:?}"))?;
    let tree = result["tree"].as_str().unwrap();
    assert!(tree.contains("source_file"));
    assert!(tree.contains("function_item"));
    Ok(())
}

#[tokio::test]
#[ignore]
async fn ast_tree_tool_with_max_depth_1_returns_truncated() -> Result<()> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.rs");
    std::fs::write(&path, "fn main() {}").unwrap();
    let args =
        serde_json::json!({ "path": path.to_str().unwrap(), "language": "rust", "max_depth": 1 });
    let result = AstTreeTool::execute(&args, dir.path(), &Bus::default())
        .await
        .map_err(|e| format!("{e:?}"))?;
    let tree = result["tree"].as_str().unwrap();
    assert!(tree.contains("..."));
    Ok(())
}
// ── E2E tests with fixtures ────────────────────────────────────────────────

fn fixture_path(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/tools/handler/testdata")
        .join(name)
}

#[tokio::test]
async fn e2e_nonexistent_file_returns_runtime_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nonexistent.rs");
    let args = serde_json::json!({ "path": path.to_str().unwrap(), "language": "rust", "query": "(function_item)" });
    let err = AstQueryTool::execute(&args, dir.path(), &Bus::default())
        .await
        .unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Runtime);
    assert!(err.message.contains("File not found"));
}

#[tokio::test]
#[ignore]
async fn e2e_query_on_sample_rs_returns_function_matches() -> Result<()> {
    let path = fixture_path("sample.rs");
    let cwd = path.parent().unwrap();
    let args = serde_json::json!({
        "path": path.to_str().unwrap(),
        "language": "rust",
        "query": "(function_item name: (identifier) @fn-name)"
    });
    let result = AstQueryTool::execute(&args, cwd, &Bus::default())
        .await
        .map_err(|e| format!("{e:?}"))?;
    let matches = result["matches"].as_array().unwrap();
    assert!(!matches.is_empty());
    let names: Vec<String> = matches
        .iter()
        .filter_map(|m| m["captures"]["fn-name"]["text"].as_str().map(String::from))
        .collect();
    assert!(names.contains(&"process".to_string()));
    assert!(names.contains(&"main".to_string()));
    Ok(())
}

#[tokio::test]
#[ignore]
async fn e2e_nodes_on_sample_rs_returns_match_arms() -> Result<()> {
    let path = fixture_path("sample.rs");
    let cwd = path.parent().unwrap();
    let args = serde_json::json!({
        "path": path.to_str().unwrap(),
        "language": "rust",
        "node_type": "match_arm"
    });
    let result = AstNodesTool::execute(&args, cwd, &Bus::default())
        .await
        .map_err(|e| format!("{e:?}"))?;
    let nodes = result["nodes"].as_array().unwrap();
    assert!(!nodes.is_empty());
    assert_eq!(result["node_type"], "match_arm");
    Ok(())
}

#[tokio::test]
#[ignore]
async fn e2e_refs_on_sample_rs_finds_status_occurrences() -> Result<()> {
    let path = fixture_path("sample.rs");
    let cwd = path.parent().unwrap();
    let args = serde_json::json!({
        "path": path.to_str().unwrap(),
        "language": "rust",
        "name": "Status"
    });
    let result = AstRefsTool::execute(&args, cwd, &Bus::default())
        .await
        .map_err(|e| format!("{e:?}"))?;
    let matches = result["matches"].as_array().unwrap();
    assert!(!matches.is_empty());
    let texts: Vec<String> = matches
        .iter()
        .filter_map(|m| m["captures"]["name"]["text"].as_str().map(String::from))
        .collect();
    assert!(texts.iter().all(|t| t == "Status"));
    Ok(())
}

#[tokio::test]
#[ignore]
async fn e2e_tree_on_sample_rs_returns_valid_sexp() -> Result<()> {
    let path = fixture_path("sample.rs");
    let cwd = path.parent().unwrap();
    let args = serde_json::json!({ "path": path.to_str().unwrap(), "language": "rust" });
    let result = AstTreeTool::execute(&args, cwd, &Bus::default())
        .await
        .map_err(|e| format!("{e:?}"))?;
    let tree = result["tree"].as_str().unwrap();
    assert!(tree.contains("source_file"));
    assert!(tree.contains("function_item"));
    assert!(tree.contains("struct_item"));
    Ok(())
}

#[tokio::test]
#[ignore]
async fn e2e_tree_with_max_depth_1_returns_truncated() -> Result<()> {
    let path = fixture_path("sample.rs");
    let cwd = path.parent().unwrap();
    let args =
        serde_json::json!({ "path": path.to_str().unwrap(), "language": "rust", "max_depth": 1 });
    let result = AstTreeTool::execute(&args, cwd, &Bus::default())
        .await
        .map_err(|e| format!("{e:?}"))?;
    let tree = result["tree"].as_str().unwrap();
    assert!(tree.contains("..."));
    Ok(())
}
