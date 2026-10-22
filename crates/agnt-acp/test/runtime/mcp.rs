//! MCP merge and MCP tool-definition mapping tests for `build_acp_runtime()`.

use agnt_core::tools::mcp::client::McpToolDefinition;

use super::runtime_support::*;
use super::*;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// ---------------------------------------------------------------------------
// MCP merge
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_runtime_build_acp_runtime_merges_distinct_mcp_servers() -> Result<()> {
    // -- Setup & Fixtures
    force_env_only_vault();
    let temp = tempfile::tempdir()?;
    let config = format!(
        "{BASE_CONFIG}\n[mcp.config_only]\ntransport = \"http\"\nurl = \"http://127.0.0.1:9/mcp\"\nenabled = false\n"
    );
    let config_path = write_config(temp.path(), &config)?;
    let store = memory_store_type();
    let client_servers = vec![disabled_http_server("client_only")];

    // -- Exec
    let runtime = build_acp_runtime(make_params(
        temp.path().to_path_buf(),
        config_path,
        client_servers,
        store,
    ))
    .await
    .map_err(|e| format!("should build runtime: {e:?}"))?;
    let names = lifecycle_names(&runtime);

    // -- Check
    assert_eq!(
        names,
        vec!["client_only".to_string(), "config_only".to_string()],
        "servers unique to either source must all be present"
    );
    Ok(())
}

#[tokio::test]
async fn test_runtime_build_acp_runtime_client_mcp_server_wins_on_collision() -> Result<()> {
    // -- Setup & Fixtures
    force_env_only_vault();
    // The config.toml server is enabled; the client server of the same name is
    // disabled. Only a client-wins merge leaves the merged entry disabled — and
    // therefore leaves the runtime constructible without a reachable endpoint.
    let temp = tempfile::tempdir()?;
    let config = format!(
        "{BASE_CONFIG}\n[mcp.shared]\ntransport = \"http\"\nurl = \"http://127.0.0.1:9/mcp\"\nenabled = true\n"
    );
    let config_path = write_config(temp.path(), &config)?;
    let store = memory_store_type();
    let client_servers = vec![disabled_http_server("shared")];

    // -- Exec
    let runtime = build_acp_runtime(make_params(
        temp.path().to_path_buf(),
        config_path,
        client_servers,
        store,
    ))
    .await
    .map_err(|e| format!("should build runtime: {e:?}"))?;
    let projection = runtime.mcp_state.mcp_lifecycle_projection();

    // -- Check
    assert_eq!(
        projection.len(),
        1,
        "a name collision must collapse to one merged server, got: {projection:?}"
    );
    let shared = projection
        .first()
        .ok_or("merged projection must hold the shared server")?;
    assert_eq!(shared.name, "shared");
    assert!(
        !shared.enabled,
        "the client server must replace the config.toml server of the same name"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// MCP tool definition mapping
// ---------------------------------------------------------------------------

#[test]
fn test_runtime_mcp_tool_definition_uses_tool_metadata() -> Result<()> {
    // -- Setup & Fixtures
    let tool = McpToolDefinition {
        server: "srv".to_string(),
        name: "mcp__srv__search".to_string(),
        raw_name: "search".to_string(),
        description: Some("Search things".to_string()),
        parameters: Some(serde_json::json!({"type": "object"})),
    };

    // -- Exec
    let def = mcp_tool_definition(&tool).map_err(|e| format!("{e:?}"))?;

    // -- Check
    assert_eq!(def.name.to_string(), "mcp__srv__search");
    assert_eq!(def.description, "Search things");
    assert_eq!(def.parameters, serde_json::json!({"type": "object"}));
    Ok(())
}

#[test]
fn test_runtime_mcp_tool_definition_falls_back_to_defaults() -> Result<()> {
    // -- Setup & Fixtures
    let tool = McpToolDefinition {
        server: "srv".to_string(),
        name: "mcp__srv__ping".to_string(),
        raw_name: "ping".to_string(),
        description: None,
        parameters: None,
    };

    // -- Exec
    let def = mcp_tool_definition(&tool).map_err(|e| format!("{e:?}"))?;

    // -- Check
    assert_eq!(def.description, "MCP tool from server 'srv'");
    assert_eq!(def.parameters["type"], "object");
    assert_eq!(def.parameters["required"][0], "args");
    Ok(())
}

#[test]
fn test_runtime_mcp_tool_definition_rejects_empty_tool_name() {
    // -- Setup & Fixtures
    let tool = McpToolDefinition {
        server: "srv".to_string(),
        name: String::new(),
        raw_name: String::new(),
        description: None,
        parameters: None,
    };

    // -- Exec
    let result = mcp_tool_definition(&tool);

    // -- Check
    assert!(result.is_err(), "an empty MCP tool name must be rejected");
}
