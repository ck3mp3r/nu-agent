//! Construction and tool-registration tests for `build_acp_runtime()`.

use agnt_core::tools::handler::is_builtin_tool_name;

use super::runtime_support::*;
use super::*;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// ---------------------------------------------------------------------------
// Construction
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_runtime_build_acp_runtime_valid_config_returns_ok() -> Result<()> {
    // -- Setup & Fixtures
    force_env_only_vault();
    let temp = tempfile::tempdir()?;
    let cwd = temp.path().join("session-cwd");
    std::fs::create_dir_all(&cwd)?;
    let config_path = write_config(temp.path(), BASE_CONFIG)?;
    let store = memory_store(temp.path()).await?;

    // -- Exec
    let runtime = build_acp_runtime(make_params(cwd.clone(), config_path, Vec::new(), store))
        .await
        .map_err(|e| format!("should build runtime: {e:?}"))?;

    // -- Check
    assert_eq!(runtime.cwd, cwd, "runtime cwd must come from params");
    assert!(
        runtime.final_session_id.is_none(),
        "a fresh session has no persisted session id"
    );
    Ok(())
}

#[tokio::test]
async fn test_runtime_build_acp_runtime_installs_interactive_pending() -> Result<()> {
    // -- Setup & Fixtures
    force_env_only_vault();
    let temp = tempfile::tempdir()?;
    let config_path = write_config(temp.path(), BASE_CONFIG)?;
    let store = memory_store(temp.path()).await?;

    // -- Exec
    let runtime = build_acp_runtime(make_params(
        temp.path().to_path_buf(),
        config_path,
        Vec::new(),
        store,
    ))
    .await
    .map_err(|e| format!("should build runtime: {e:?}"))?;
    let pending = runtime.interactive_pending_arc();

    // -- Check
    assert!(
        pending.is_some(),
        "the ACP runtime must install the interactive pending map so execute_turn takes the interactive branch"
    );
    Ok(())
}

#[tokio::test]
async fn test_runtime_build_acp_runtime_missing_config_returns_config_error() -> Result<()> {
    // -- Setup & Fixtures
    force_env_only_vault();
    let temp = tempfile::tempdir()?;
    let missing = temp.path().join("does-not-exist.toml");
    let store = memory_store(temp.path()).await?;

    // -- Exec
    let result = build_acp_runtime(make_params(
        temp.path().to_path_buf(),
        missing,
        Vec::new(),
        store,
    ))
    .await;

    // -- Check
    let err = result.err().ok_or("missing config must fail")?;
    assert!(
        matches!(err, AcpError::Config(_)),
        "missing config must produce AcpError::Config, got: {err:?}"
    );
    assert!(
        err.to_string().contains("config"),
        "error message must mention config, got: {err}"
    );
    Ok(())
}

#[tokio::test]
async fn test_runtime_build_acp_runtime_invalid_config_returns_config_error() -> Result<()> {
    // -- Setup & Fixtures
    force_env_only_vault();
    // The model id is synthetic so no models cache entry can clamp
    // max_output_tokens down and mask the invalid combination.
    let temp = tempfile::tempdir()?;
    let config = r#"
[models.default]
model = "ollama/acp-validation-probe"
max_output_tokens = 8192
max_context_tokens = 4096

[providers.ollama]
base_url = "http://localhost:11434"
"#;
    let config_path = write_config(temp.path(), config)?;
    let store = memory_store(temp.path()).await?;

    // -- Exec
    let result = build_acp_runtime(make_params(
        temp.path().to_path_buf(),
        config_path,
        Vec::new(),
        store,
    ))
    .await;

    // -- Check
    let err = result.err().ok_or("invalid config must fail")?;
    assert!(
        matches!(err, AcpError::Config(_)),
        "invalid config must produce AcpError::Config, got: {err:?}"
    );
    assert!(
        err.to_string().contains("config validation failed"),
        "error message must mention config validation, got: {err}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Tool registration
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_runtime_build_acp_runtime_registers_builtin_tools() -> Result<()> {
    // -- Setup & Fixtures
    force_env_only_vault();
    let temp = tempfile::tempdir()?;
    let config_path = write_config(temp.path(), BASE_CONFIG)?;
    let store = memory_store(temp.path()).await?;

    // -- Exec
    let runtime = build_acp_runtime(make_params(
        temp.path().to_path_buf(),
        config_path,
        Vec::new(),
        store,
    ))
    .await
    .map_err(|e| format!("should build runtime: {e:?}"))?;
    let names = visible_tool_names(&runtime);

    // -- Check
    for expected in ["read", "edit", "grep", "glob"] {
        assert!(
            names.iter().any(|name| name == expected),
            "built-in tool '{expected}' must be registered; got: {names:?}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn test_runtime_build_acp_runtime_has_no_closure_tools() -> Result<()> {
    // -- Setup & Fixtures
    force_env_only_vault();
    let temp = tempfile::tempdir()?;
    let config_path = write_config(temp.path(), BASE_CONFIG)?;
    let store = memory_store(temp.path()).await?;

    // -- Exec
    let runtime = build_acp_runtime(make_params(
        temp.path().to_path_buf(),
        config_path,
        Vec::new(),
        store,
    ))
    .await
    .map_err(|e| format!("should build runtime: {e:?}"))?;
    let closure_count = runtime.tools.closure_registry().names().count();
    let names = visible_tool_names(&runtime);

    // -- Check
    assert_eq!(
        closure_count, 0,
        "ACP mode has no Nushell engine, so the closure registry must be empty"
    );
    let non_builtin: Vec<&str> = names
        .iter()
        .map(String::as_str)
        .filter(|name| !is_builtin_tool_name(name))
        .collect();
    assert!(
        non_builtin.is_empty(),
        "every visible tool must be a built-in; found non-builtins: {non_builtin:?}"
    );
    Ok(())
}
