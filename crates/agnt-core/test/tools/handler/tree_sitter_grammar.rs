//! Grammar-backed AST tool tests.
//!
//! The production grammar cache is a process-global `OnceLock`, so these tests
//! install a private tree-sitter config, a private parser directory, and a
//! private OS grammar cache holding a copy of the host's compiled `rust`
//! grammar. The whole module is `#[serial]` because it mutates process
//! environment variables.
//!
//! When the host has no compiled `rust` grammar the tests return early: the
//! production code path cannot be reached without one.

use super::*;
use crate::bus::Bus;
use crate::tools::handler::ToolErrorKind;

use std::ffi::OsString;
use std::path::PathBuf;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// The shared-library extension the host platform uses for grammars.
fn library_extension() -> &'static str {
    if cfg!(target_os = "windows") {
        "dll"
    } else if cfg!(target_os = "macos") {
        "dylib"
    } else {
        "so"
    }
}

/// The host's compiled `rust` grammar, when the OS cache holds one.
fn host_rust_grammar() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let candidate = PathBuf::from(home)
        .join(".cache")
        .join("tree-sitter")
        .join("lib")
        .join(format!("rust.{}", library_extension()));
    candidate.is_file().then_some(candidate)
}

/// A private tree-sitter environment, restored on drop.
struct GrammarEnv {
    _root: tempfile::TempDir,
    previous_tree_sitter_dir: Option<OsString>,
    previous_xdg_cache_home: Option<OsString>,
}

impl GrammarEnv {
    /// Install the private environment. Returns `None` when the host has no
    /// compiled `rust` grammar to copy.
    fn install() -> Result<Option<Self>> {
        let Some(grammar) = host_rust_grammar() else {
            return Ok(None);
        };
        let root = tempfile::tempdir()?;

        // A parser directory holding a `tree-sitter-rust` grammar repo.
        let parsers = root.path().join("parsers");
        std::fs::create_dir_all(parsers.join("tree-sitter-rust"))?;

        // The tree-sitter config that names that parser directory.
        let config_dir = root.path().join("config");
        std::fs::create_dir_all(&config_dir)?;
        std::fs::write(
            config_dir.join("config.json"),
            serde_json::json!({ "parser-directories": [parsers] }).to_string(),
        )?;

        // The OS grammar cache holding the copied compiled grammar.
        let lib_dir = root.path().join("cache").join("tree-sitter").join("lib");
        std::fs::create_dir_all(&lib_dir)?;
        std::fs::copy(
            &grammar,
            lib_dir.join(format!("rust.{}", library_extension())),
        )?;

        let previous_tree_sitter_dir = std::env::var_os("TREE_SITTER_DIR");
        let previous_xdg_cache_home = std::env::var_os("XDG_CACHE_HOME");
        unsafe {
            std::env::set_var("TREE_SITTER_DIR", &config_dir);
            std::env::set_var("XDG_CACHE_HOME", root.path().join("cache"));
        }

        Ok(Some(Self {
            _root: root,
            previous_tree_sitter_dir,
            previous_xdg_cache_home,
        }))
    }
}

impl Drop for GrammarEnv {
    fn drop(&mut self) {
        match self.previous_tree_sitter_dir.take() {
            Some(value) => unsafe { std::env::set_var("TREE_SITTER_DIR", value) },
            None => unsafe { std::env::remove_var("TREE_SITTER_DIR") },
        }
        match self.previous_xdg_cache_home.take() {
            Some(value) => unsafe { std::env::set_var("XDG_CACHE_HOME", value) },
            None => unsafe { std::env::remove_var("XDG_CACHE_HOME") },
        }
    }
}

/// A small Rust source with two functions and three `Status` occurrences.
const SOURCE: &str = "struct Status { ready: bool }\n\nfn process(status: Status) -> bool {\n    status.ready\n}\n\nfn main() {\n    let s = Status { ready: true };\n    process(s);\n}\n";

/// Write `SOURCE` into a tempdir and return the directory and file path.
fn write_source() -> Result<(tempfile::TempDir, PathBuf)> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("sample.rs");
    std::fs::write(&path, SOURCE)?;
    Ok((dir, path))
}

/// `ast_query` finds every function name, honours the capture filter, and
/// reports truncation when `max_matches` is below the total.
#[tokio::test]
#[serial_test::serial]
async fn ast_query_end_to_end_against_a_compiled_grammar() -> Result<()> {
    // -- Setup & Fixtures
    let Some(_env) = GrammarEnv::install()? else {
        return Ok(());
    };
    let (dir, path) = write_source()?;
    let path = path.to_str().ok_or("path must be UTF-8")?;

    // -- Exec
    let result = AstQueryTool::execute(
        &serde_json::json!({
            "path": path,
            "language": "rust",
            "query": "(function_item name: (identifier) @fn-name)"
        }),
        dir.path(),
        &Bus::default(),
    )
    .await
    .map_err(|e| format!("query must succeed: {e:?}"))?;

    // -- Check
    let matches = result["matches"].as_array().ok_or("matches array")?;
    let names: Vec<&str> = matches
        .iter()
        .filter_map(|m| m["captures"]["fn-name"]["text"].as_str())
        .collect();
    assert_eq!(names, vec!["process", "main"], "captured function names");
    assert_eq!(result["total_matches"], 2);
    assert_eq!(result["truncated"], false);
    assert_eq!(result["language"], "rust");
    assert_eq!(result["file"], path);
    let first = &matches[0]["captures"]["fn-name"];
    assert_eq!(first["start_row"], 2, "`process` starts on row 2");
    assert_eq!(first["start_col"], 3, "`process` starts at column 3");
    assert!(
        first["end_byte"].as_u64().ok_or("end_byte")?
            > first["start_byte"].as_u64().ok_or("start_byte")?,
        "the capture must span at least one byte"
    );
    Ok(())
}

/// `ast_query` truncates at `max_matches`, drops text when `include_text` is
/// false, and filters captures by name.
#[tokio::test]
#[serial_test::serial]
async fn ast_query_honours_max_matches_include_text_and_capture_filter() -> Result<()> {
    // -- Setup & Fixtures
    let Some(_env) = GrammarEnv::install()? else {
        return Ok(());
    };
    let (dir, path) = write_source()?;
    let path = path.to_str().ok_or("path must be UTF-8")?;
    let query = "(function_item name: (identifier) @fn-name)";

    // -- Exec
    let truncated = AstQueryTool::execute(
        &serde_json::json!({
            "path": path,
            "language": "rust",
            "query": query,
            "max_matches": 1,
            "include_text": false
        }),
        dir.path(),
        &Bus::default(),
    )
    .await
    .map_err(|e| format!("query must succeed: {e:?}"))?;
    let filtered = AstQueryTool::execute(
        &serde_json::json!({
            "path": path,
            "language": "rust",
            "query": query,
            "captures": ["not-a-capture"]
        }),
        dir.path(),
        &Bus::default(),
    )
    .await
    .map_err(|e| format!("query must succeed: {e:?}"))?;

    // -- Check
    let matches = truncated["matches"].as_array().ok_or("matches array")?;
    assert_eq!(
        matches.len(),
        1,
        "max_matches must cap the returned matches"
    );
    assert_eq!(truncated["total_matches"], 2, "the total still counts all");
    assert_eq!(truncated["truncated"], true, "truncation must be reported");
    assert!(
        matches[0]["captures"]["fn-name"]["text"].is_null(),
        "include_text=false must drop the capture text"
    );
    let filtered_matches = filtered["matches"].as_array().ok_or("matches array")?;
    assert_eq!(filtered_matches.len(), 2, "the match count is unaffected");
    assert!(
        filtered_matches[0]["captures"]
            .as_object()
            .ok_or("captures object")?
            .is_empty(),
        "a capture filter that matches nothing must drop every capture"
    );
    Ok(())
}

/// `ast_nodes` walks the tree, honours `include_text`, and reports truncation.
#[tokio::test]
#[serial_test::serial]
async fn ast_nodes_end_to_end_against_a_compiled_grammar() -> Result<()> {
    // -- Setup & Fixtures
    let Some(_env) = GrammarEnv::install()? else {
        return Ok(());
    };
    let (dir, path) = write_source()?;
    let path = path.to_str().ok_or("path must be UTF-8")?;

    // -- Exec
    let all = AstNodesTool::execute(
        &serde_json::json!({
            "path": path,
            "language": "rust",
            "node_type": "function_item",
            "include_text": true
        }),
        dir.path(),
        &Bus::default(),
    )
    .await
    .map_err(|e| format!("nodes must succeed: {e:?}"))?;
    let capped = AstNodesTool::execute(
        &serde_json::json!({
            "path": path,
            "language": "rust",
            "node_type": "function_item",
            "max_matches": 1
        }),
        dir.path(),
        &Bus::default(),
    )
    .await
    .map_err(|e| format!("nodes must succeed: {e:?}"))?;

    // -- Check
    let nodes = all["nodes"].as_array().ok_or("nodes array")?;
    assert_eq!(nodes.len(), 2, "both functions must be found");
    assert_eq!(all["total"], 2);
    assert_eq!(all["truncated"], false);
    assert_eq!(all["node_type"], "function_item");
    let text = nodes[0]["text"].as_str().ok_or("node text")?;
    assert!(
        text.starts_with("fn process"),
        "include_text=true must carry the node source: {text}"
    );
    let capped_nodes = capped["nodes"].as_array().ok_or("nodes array")?;
    assert_eq!(capped_nodes.len(), 1, "max_matches must cap the nodes");
    assert_eq!(capped["total"], 2);
    assert_eq!(capped["truncated"], true);
    assert!(
        capped_nodes[0]["text"].is_null(),
        "include_text defaults to false"
    );
    Ok(())
}

/// `ast_refs` finds every identifier occurrence of a name and reports
/// truncation.
#[tokio::test]
#[serial_test::serial]
async fn ast_refs_end_to_end_against_a_compiled_grammar() -> Result<()> {
    // -- Setup & Fixtures
    let Some(_env) = GrammarEnv::install()? else {
        return Ok(());
    };
    let (dir, path) = write_source()?;
    let path = path.to_str().ok_or("path must be UTF-8")?;

    // -- Exec
    let all = AstRefsTool::execute(
        &serde_json::json!({"path": path, "language": "rust", "name": "Status"}),
        dir.path(),
        &Bus::default(),
    )
    .await
    .map_err(|e| format!("refs must succeed: {e:?}"))?;
    let capped = AstRefsTool::execute(
        &serde_json::json!({
            "path": path,
            "language": "rust",
            "name": "Status",
            "max_matches": 1
        }),
        dir.path(),
        &Bus::default(),
    )
    .await
    .map_err(|e| format!("refs must succeed: {e:?}"))?;

    // -- Check
    let matches = all["matches"].as_array().ok_or("matches array")?;
    assert_eq!(matches.len(), 3, "three `Status` occurrences");
    assert_eq!(all["total_matches"], 3);
    assert_eq!(all["truncated"], false);
    assert!(
        matches
            .iter()
            .all(|m| m["captures"]["name"]["text"] == "Status"),
        "every capture must carry the searched name"
    );
    let capped_matches = capped["matches"].as_array().ok_or("matches array")?;
    assert_eq!(capped_matches.len(), 1, "max_matches must cap the matches");
    assert_eq!(capped["total_matches"], 3);
    assert_eq!(capped["truncated"], true);
    Ok(())
}

/// `ast_tree` returns the full s-expression, and a bounded depth truncates it.
#[tokio::test]
#[serial_test::serial]
async fn ast_tree_end_to_end_against_a_compiled_grammar() -> Result<()> {
    // -- Setup & Fixtures
    let Some(_env) = GrammarEnv::install()? else {
        return Ok(());
    };
    let (dir, path) = write_source()?;
    let path = path.to_str().ok_or("path must be UTF-8")?;

    // -- Exec
    let full = AstTreeTool::execute(
        &serde_json::json!({"path": path, "language": "rust"}),
        dir.path(),
        &Bus::default(),
    )
    .await
    .map_err(|e| format!("tree must succeed: {e:?}"))?;
    let shallow = AstTreeTool::execute(
        &serde_json::json!({"path": path, "language": "rust", "max_depth": 1}),
        dir.path(),
        &Bus::default(),
    )
    .await
    .map_err(|e| format!("tree must succeed: {e:?}"))?;

    // -- Check
    let tree = full["tree"].as_str().ok_or("tree string")?;
    assert!(tree.starts_with("(source_file"), "got: {tree}");
    assert!(tree.contains("function_item"), "got: {tree}");
    assert!(tree.contains("struct_item"), "got: {tree}");
    let shallow_tree = shallow["tree"].as_str().ok_or("tree string")?;
    assert!(
        shallow_tree.starts_with("(source_file ") && shallow_tree.ends_with(')'),
        "max_depth=1 must keep the root kind: {shallow_tree}"
    );
    let truncated_children: Vec<&str> = shallow_tree
        .trim_start_matches("(source_file ")
        .trim_end_matches(')')
        .split(' ')
        .collect();
    assert!(
        !truncated_children.is_empty() && truncated_children.iter().all(|child| *child == "..."),
        "max_depth=1 must truncate every child: {shallow_tree}"
    );
    Ok(())
}

/// `ast_tree` with a depth beyond the tree height renders every leaf: named
/// leaves as `(kind)` and anonymous leaves as their quoted source text.
#[tokio::test]
#[serial_test::serial]
async fn ast_tree_with_a_deep_max_depth_renders_leaf_nodes() -> Result<()> {
    // -- Setup & Fixtures
    let Some(_env) = GrammarEnv::install()? else {
        return Ok(());
    };
    let (dir, path) = write_source()?;
    let path = path.to_str().ok_or("path must be UTF-8")?;

    // -- Exec
    let deep = AstTreeTool::execute(
        &serde_json::json!({"path": path, "language": "rust", "max_depth": 64}),
        dir.path(),
        &Bus::default(),
    )
    .await
    .map_err(|e| format!("tree must succeed: {e:?}"))?;

    // -- Check
    let tree = deep["tree"].as_str().ok_or("tree string")?;
    assert!(
        tree.contains("(identifier)"),
        "a named leaf must render as (kind): {tree}"
    );
    assert!(
        tree.contains("\"fn\""),
        "an anonymous leaf must render as its quoted source text: {tree}"
    );
    assert!(
        !tree.contains("..."),
        "a depth beyond the tree height must not truncate: {tree}"
    );
    Ok(())
}

/// An unknown language and a malformed query are reported as validation
/// errors, not as runtime failures.
#[tokio::test]
#[serial_test::serial]
async fn ast_tools_report_validation_errors_for_unknown_language_and_bad_query() -> Result<()> {
    // -- Setup & Fixtures
    let Some(_env) = GrammarEnv::install()? else {
        return Ok(());
    };
    let (dir, path) = write_source()?;
    let path = path.to_str().ok_or("path must be UTF-8")?;

    // -- Exec
    let unknown_language = AstTreeTool::execute(
        &serde_json::json!({"path": path, "language": "cobol"}),
        dir.path(),
        &Bus::default(),
    )
    .await
    .err()
    .ok_or("an unknown language must fail")?;
    let bad_query = AstQueryTool::execute(
        &serde_json::json!({"path": path, "language": "rust", "query": "(not_a_node"}),
        dir.path(),
        &Bus::default(),
    )
    .await
    .err()
    .ok_or("a malformed query must fail")?;

    // -- Check
    assert_eq!(unknown_language.kind, ToolErrorKind::Validation);
    assert!(
        unknown_language
            .message
            .contains("No tree-sitter grammar found"),
        "got: {}",
        unknown_language.message
    );
    assert_eq!(bad_query.kind, ToolErrorKind::Validation);
    assert!(
        bad_query.message.contains("Invalid tree-sitter query"),
        "got: {}",
        bad_query.message
    );
    Ok(())
}
