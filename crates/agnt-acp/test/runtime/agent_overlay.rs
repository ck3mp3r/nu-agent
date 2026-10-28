//! Agent permissions overlay tests for `build_acp_runtime()`.

use agnt_core::conversation::managers::PersonaManager;
use agnt_core::tools::authz::PermissionAction;

use super::runtime_support::*;
use super::*;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// ---------------------------------------------------------------------------
// Agent permissions overlay
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_runtime_build_acp_runtime_applies_agent_permissions_overlay() -> Result<()> {
    // -- Setup & Fixtures
    force_env_only_vault();
    let temp = tempfile::tempdir()?;
    isolate_xdg_config(temp.path());
    let cwd = temp.path().join("session-cwd");
    std::fs::create_dir_all(&cwd)?;
    // Base config asks for `http`; the agent persona allows it.
    let config =
        format!("{BASE_CONFIG}\n[agents]\ndefault = \"dev\"\n\n[permissions]\nhttp = \"ask\"\n");
    let config_path = write_config(temp.path(), &config)?;
    write_persona(
        &cwd,
        "dev",
        "---\nname: dev\npermissions:\n  http: allow\n---\n\nDev agent.\n",
    )?;
    let store = memory_store_type();

    // -- Exec
    let runtime = build_acp_runtime(make_params(cwd.clone(), config_path, Vec::new(), store))
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
        "the agent overlay must allow `http`; got: {decision:?}"
    );
    Ok(())
}

#[tokio::test]
async fn test_runtime_build_acp_runtime_agent_without_permissions_keeps_base() -> Result<()> {
    // -- Setup & Fixtures
    force_env_only_vault();
    let temp = tempfile::tempdir()?;
    isolate_xdg_config(temp.path());
    let cwd = temp.path().join("session-cwd");
    std::fs::create_dir_all(&cwd)?;
    let config =
        format!("{BASE_CONFIG}\n[agents]\ndefault = \"dev\"\n\n[permissions]\nhttp = \"ask\"\n");
    let config_path = write_config(temp.path(), &config)?;
    // No `permissions` front matter: the base config must survive unchanged.
    write_persona(&cwd, "dev", "---\nname: dev\n---\n\nDev agent.\n")?;
    let store = memory_store_type();

    // -- Exec
    let runtime = build_acp_runtime(make_params(cwd.clone(), config_path, Vec::new(), store))
        .await
        .map_err(|e| format!("should build runtime: {e:?}"))?;
    let decision = runtime
        .permission_state
        .permissions()
        .evaluate("http", &serde_json::Map::new());

    // -- Check
    assert_eq!(
        decision.action,
        PermissionAction::Ask,
        "without a permissions front matter the base `http = ask` rule must stand; got: {decision:?}"
    );
    Ok(())
}

#[tokio::test]
async fn test_runtime_build_acp_runtime_invalid_agent_permissions_returns_error() -> Result<()> {
    // -- Setup & Fixtures
    force_env_only_vault();
    let temp = tempfile::tempdir()?;
    isolate_xdg_config(temp.path());
    let cwd = temp.path().join("session-cwd");
    std::fs::create_dir_all(&cwd)?;
    let config = format!("{BASE_CONFIG}\n[agents]\ndefault = \"dev\"\n");
    let config_path = write_config(temp.path(), &config)?;
    write_persona(
        &cwd,
        "dev",
        "---\nname: dev\npermissions:\n  http: yolo\n---\n\nDev agent.\n",
    )?;
    let store = memory_store_type();

    // -- Exec
    let result = build_acp_runtime(make_params(cwd.clone(), config_path, Vec::new(), store)).await;

    // -- Check
    let err = result.err().ok_or("invalid agent permissions must fail")?;
    assert!(
        matches!(err, AcpError::Config(_)),
        "invalid agent permissions must produce AcpError::Config, got: {err:?}"
    );
    assert!(
        err.to_string().contains("invalid permissions"),
        "error must mention the invalid permissions, got: {err}"
    );
    Ok(())
}

#[tokio::test]
async fn test_runtime_build_acp_runtime_missing_agent_persona_returns_error() -> Result<()> {
    // -- Setup & Fixtures
    force_env_only_vault();
    let temp = tempfile::tempdir()?;
    isolate_xdg_config(temp.path());
    let cwd = temp.path().join("session-cwd");
    std::fs::create_dir_all(&cwd)?;
    // No persona file exists for the resolved default agent name.
    let config = format!("{BASE_CONFIG}\n[agents]\ndefault = \"ghost\"\n");
    let config_path = write_config(temp.path(), &config)?;
    let store = memory_store_type();

    // -- Exec
    let result = build_acp_runtime(make_params(cwd.clone(), config_path, Vec::new(), store)).await;

    // -- Check
    let err = result.err().ok_or("a missing agent persona must fail")?;
    assert!(
        matches!(err, AcpError::Config(_)),
        "a missing agent persona must produce AcpError::Config, got: {err:?}"
    );
    assert!(
        err.to_string().contains("Persona file not found"),
        "error must mention the missing persona, got: {err}"
    );
    Ok(())
}

#[tokio::test]
async fn test_runtime_build_acp_runtime_overlay_active_in_startup_summary() -> Result<()> {
    // -- Setup & Fixtures
    force_env_only_vault();
    let temp = tempfile::tempdir()?;
    isolate_xdg_config(temp.path());
    let cwd = temp.path().join("session-cwd");
    std::fs::create_dir_all(&cwd)?;
    let config = format!("{BASE_CONFIG}\n[agents]\ndefault = \"dev\"\n");
    let config_path = write_config(temp.path(), &config)?;
    write_persona(
        &cwd,
        "dev",
        "---\nname: dev\npermissions:\n  http: allow\n---\n\nDev agent.\n",
    )?;
    let store = memory_store_type();
    let _ = log::set_boxed_logger(Box::new(CaptureLogger));
    log::set_max_level(log::LevelFilter::Info);
    CAPTURED_LOGS.lock().map_err(|e| format!("{e:?}"))?.clear();

    // -- Exec
    let mut runtime = build_acp_runtime(make_params(cwd.clone(), config_path, Vec::new(), store))
        .await
        .map_err(|e| format!("should build runtime: {e:?}"))?;
    runtime.permission_state.emit_startup_summary_once();

    // -- Check
    let logs = CAPTURED_LOGS
        .lock()
        .map_err(|e| format!("{e:?}"))?
        .join("\n");
    assert!(
        logs.contains("overlay_active=true"),
        "the startup summary must report the applied overlay; got: {logs}"
    );
    Ok(())
}

#[tokio::test]
async fn test_runtime_build_acp_runtime_populates_effective_agent_identity() -> Result<()> {
    // -- Setup & Fixtures
    force_env_only_vault();
    let temp = tempfile::tempdir()?;
    isolate_xdg_config(temp.path());
    let cwd = temp.path().join("session-cwd");
    std::fs::create_dir_all(&cwd)?;
    let config = format!("{BASE_CONFIG}\n[agents]\ndefault = \"dev\"\n");
    let config_path = write_config(temp.path(), &config)?;
    write_persona(&cwd, "dev", "---\nname: dev\n---\n\nDev agent.\n")?;
    let store = memory_store_type();

    // -- Exec
    let runtime = build_acp_runtime(make_params(cwd.clone(), config_path, Vec::new(), store))
        .await
        .map_err(|e| format!("should build runtime: {e:?}"))?;

    // -- Check
    assert_eq!(
        runtime.persona.agent_identity(),
        Some("dev"),
        "the startup switch must populate the effective agent identity"
    );
    Ok(())
}
