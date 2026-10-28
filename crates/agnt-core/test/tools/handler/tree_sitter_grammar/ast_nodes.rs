//! `ast_nodes` grammar-backed test.

use super::*;

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
