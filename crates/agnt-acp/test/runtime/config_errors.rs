//! AGENTS.md warning and config-load error-branch tests for `build_acp_runtime()`.

use super::runtime_support::*;
use super::*;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// ---------------------------------------------------------------------------
// AGENTS.md load warnings
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_runtime_build_acp_runtime_unreadable_agents_md_is_non_fatal() -> Result<()> {
    // -- Setup & Fixtures
    force_env_only_vault();
    // Enable the info level so the warning log arguments are evaluated.
    log::set_max_level(log::LevelFilter::Info);
    let temp = tempfile::tempdir()?;
    let cwd = temp.path().join("session-cwd");
    std::fs::create_dir_all(&cwd)?;
    let agents_md = cwd.join("AGENTS.md");
    std::fs::write(&agents_md, "UNREADABLE_MARKER")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&agents_md)?.permissions();
        perms.set_mode(0o000);
        std::fs::set_permissions(&agents_md, perms)?;
    }
    let config_path = write_config(temp.path(), BASE_CONFIG)?;
    let store = memory_store_type();

    // -- Exec
    let runtime = build_acp_runtime(make_params(cwd.clone(), config_path, Vec::new(), store))
        .await
        .map_err(|e| {
            format!("an unreadable AGENTS.md must not abort runtime construction: {e:?}")
        })?;

    // -- Check
    let chain = runtime.persona.cached_agents_chain().unwrap_or("");
    assert!(
        !chain.contains("UNREADABLE_MARKER"),
        "an unreadable AGENTS.md must not contribute to the chain, got: {chain}"
    );

    // Cleanup: restore permissions so the temp dir can be removed.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&agents_md)?.permissions();
        perms.set_mode(0o644);
        std::fs::set_permissions(&agents_md, perms)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// config load error branches
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_runtime_build_acp_runtime_malformed_config_returns_config_error() -> Result<()> {
    // -- Setup & Fixtures
    force_env_only_vault();
    let temp = tempfile::tempdir()?;
    let config_path = write_config(temp.path(), "this is not = valid toml [[[")?;
    let store = memory_store_type();

    // -- Exec
    let result = build_acp_runtime(make_params(
        temp.path().to_path_buf(),
        config_path,
        Vec::new(),
        store,
    ))
    .await;

    // -- Check
    let err = result.err().ok_or("a malformed config.toml must fail")?;
    assert!(
        matches!(err, AcpError::Config(_)),
        "a malformed config.toml must produce AcpError::Config, got: {err:?}"
    );
    assert!(
        err.to_string().contains("failed to load config"),
        "error must mention the config load failure, got: {err}"
    );
    Ok(())
}

#[tokio::test]
async fn test_runtime_build_acp_runtime_missing_default_model_returns_config_error() -> Result<()> {
    // -- Setup & Fixtures
    force_env_only_vault();
    let temp = tempfile::tempdir()?;
    let config = "[providers.ollama]\nbase_url = \"http://localhost:11434\"\n";
    let config_path = write_config(temp.path(), config)?;
    let store = memory_store_type();

    // -- Exec
    let result = build_acp_runtime(make_params(
        temp.path().to_path_buf(),
        config_path,
        Vec::new(),
        store,
    ))
    .await;

    // -- Check
    let err = result
        .err()
        .ok_or("a config without models.default must fail")?;
    assert!(
        matches!(err, AcpError::Config(_)),
        "a config without models.default must produce AcpError::Config, got: {err:?}"
    );
    assert!(
        err.to_string().contains("no model configured"),
        "error must mention the missing model, got: {err}"
    );
    Ok(())
}

#[tokio::test]
async fn test_runtime_build_acp_runtime_unresolvable_model_returns_config_error() -> Result<()> {
    // -- Setup & Fixtures
    force_env_only_vault();
    let temp = tempfile::tempdir()?;
    // An empty model name cannot be resolved into a runtime config.
    let config = "[models.default]\nmodel = \"ollama/\"\n";
    let config_path = write_config(temp.path(), config)?;
    let store = memory_store_type();

    // -- Exec
    let result = build_acp_runtime(make_params(
        temp.path().to_path_buf(),
        config_path,
        Vec::new(),
        store,
    ))
    .await;

    // -- Check
    let err = result.err().ok_or("an unresolvable model must fail")?;
    assert!(
        matches!(err, AcpError::Config(_)),
        "an unresolvable model must produce AcpError::Config, got: {err:?}"
    );
    assert!(
        err.to_string().contains("failed to resolve model"),
        "error must mention the resolve failure, got: {err}"
    );
    Ok(())
}
