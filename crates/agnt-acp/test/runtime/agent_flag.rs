//! Agent flag override tests for `build_acp_runtime()`.

use agnt_core::tools::authz::PermissionAction;

use super::runtime_support::*;
use super::*;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// ---------------------------------------------------------------------------
// Agent flag override
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_runtime_build_acp_runtime_agent_flag_overrides_default_agent() -> Result<()> {
    // -- Setup & Fixtures
    force_env_only_vault();
    let temp = tempfile::tempdir()?;
    isolate_xdg_config(temp.path());
    let cwd = temp.path().join("session-cwd");
    std::fs::create_dir_all(&cwd)?;
    // The config default agent denies `http`; the flag names an agent that
    // allows it. Only the flag's overlay can produce an Allow decision.
    let config =
        format!("{BASE_CONFIG}\n[agents]\ndefault = \"strict\"\n\n[permissions]\nhttp = \"ask\"\n");
    let config_path = write_config(temp.path(), &config)?;
    write_persona(
        &cwd,
        "strict",
        "---\nname: strict\npermissions:\n  http: deny\n---\n\nStrict agent.\n",
    )?;
    write_persona(
        &cwd,
        "dev",
        "---\nname: dev\npermissions:\n  http: allow\n---\n\nDev agent.\n",
    )?;
    let store = memory_store(temp.path()).await?;
    let mut params = make_params(cwd.clone(), config_path, Vec::new(), store);
    params.agent_name = Some("dev".to_string());

    // -- Exec
    let runtime = build_acp_runtime(params)
        .await
        .map_err(|e| format!("should build runtime: {e:?}"))?;
    let decision = runtime
        .permission_state
        .permissions()
        .evaluate("http", &serde_json::Map::new());

    // -- Check
    assert_eq!(
        decision.action,
        PermissionAction::Allow,
        "the flag's agent overlay must win over the config default agent; got: {decision:?}"
    );
    Ok(())
}

#[tokio::test]
async fn test_runtime_build_acp_runtime_agent_flag_unknown_agent_returns_error() -> Result<()> {
    // -- Setup & Fixtures
    force_env_only_vault();
    let temp = tempfile::tempdir()?;
    isolate_xdg_config(temp.path());
    let cwd = temp.path().join("session-cwd");
    std::fs::create_dir_all(&cwd)?;
    let config = format!("{BASE_CONFIG}\n[agents]\ndefault = \"strict\"\n");
    let config_path = write_config(temp.path(), &config)?;
    write_persona(&cwd, "strict", "---\nname: strict\n---\n\nStrict agent.\n")?;
    let store = memory_store(temp.path()).await?;
    let mut params = make_params(cwd.clone(), config_path, Vec::new(), store);
    params.agent_name = Some("ghost".to_string());

    // -- Exec
    let result = build_acp_runtime(params).await;

    // -- Check
    let err = result.err().ok_or("an unknown agent name must fail")?;
    assert!(
        matches!(err, AcpError::Config(_)),
        "an unknown agent name must produce AcpError::Config, got: {err:?}"
    );
    assert!(
        err.to_string().contains("Persona file not found"),
        "error must mention the missing persona, got: {err}"
    );
    Ok(())
}
