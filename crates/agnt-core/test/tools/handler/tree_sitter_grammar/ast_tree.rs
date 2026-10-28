//! `ast_tree` grammar-backed tests.

use super::*;

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
