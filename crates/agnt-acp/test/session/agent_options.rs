use super::*;

use super::config_options::{current_value, select_values};
use agnt_core::tools::authz::PermissionAction;

// ---------------------------------------------------------------------------
// Agent config options
// ---------------------------------------------------------------------------

/// Point the XDG config lookup at `dir` so no host persona file leaks into the
/// advertised agent list.
fn isolate_xdg_config(dir: &Path) {
    // SAFETY: nextest gives each test its own process, so no other thread
    // reads the environment concurrently.
    unsafe { std::env::set_var("XDG_CONFIG_HOME", dir) };
}

/// Write a persona file at `{cwd}/.agents/{name}.md`.
fn write_persona(cwd: &Path, name: &str, contents: &str) -> Result<()> {
    let dir = cwd.join(".agents");
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join(format!("{name}.md")), contents)?;
    Ok(())
}

/// The ids of the advertised config options.
fn option_ids(options: &[SessionConfigOption]) -> Vec<String> {
    options.iter().map(|option| option.id.to_string()).collect()
}

#[tokio::test]
async fn test_session_manager_agent_config_options_lists_builtins_and_filesystem_agents()
-> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    isolate_xdg_config(temp.path());
    let manager = manager(temp.path()).await?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;
    write_persona(&cwd, "dev", "---\nname: dev\n---\n\nDev agent.\n")?;
    let session_id = manager.new_session(cwd, Vec::new()).await?;
    manager.set_agent(&session_id, "dev").await?;

    // -- Exec
    let options = manager.agent_config_options(&session_id).await?;

    // -- Check
    assert_eq!(
        option_ids(&options),
        vec![crate::AGENT_CONFIG_ID.to_string()],
        "the agent selector must be the only option"
    );
    let values = select_values(&options);
    for expected in ["planner", "maker", "dev"] {
        assert!(
            values.contains(&expected.to_string()),
            "agent '{expected}' must be advertised; got: {values:?}"
        );
    }
    assert_eq!(
        current_value(&options).as_deref(),
        Some("dev"),
        "the current value must be the session's agent identity"
    );
    Ok(())
}

#[tokio::test]
async fn test_session_manager_config_options_lists_agent_before_model() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    isolate_xdg_config(temp.path());
    let manager = manager(temp.path()).await?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;
    let session_id = manager.new_session(cwd, Vec::new()).await?;

    // -- Exec
    let options = manager.config_options(&session_id).await?;

    // -- Check
    assert_eq!(
        option_ids(&options),
        vec![
            crate::AGENT_CONFIG_ID.to_string(),
            crate::MODEL_CONFIG_ID.to_string()
        ],
        "the agent option must come first, then the model option"
    );
    Ok(())
}

#[tokio::test]
async fn test_session_manager_agent_config_options_unknown_session_returns_error() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let manager = manager(temp.path()).await?;

    // -- Exec
    let result = manager.agent_config_options("no-such-session").await;

    // -- Check
    let err = result.err().ok_or("an unknown session must fail")?;
    assert!(
        matches!(err, AcpError::SessionNotFound(_)),
        "unknown id must produce AcpError::SessionNotFound, got: {err:?}"
    );
    Ok(())
}

#[tokio::test]
async fn test_session_manager_set_agent_switches_identity() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    isolate_xdg_config(temp.path());
    let manager = manager(temp.path()).await?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;
    write_persona(&cwd, "dev", "---\nname: dev\n---\n\nDev agent.\n")?;
    let session_id = manager.new_session(cwd, Vec::new()).await?;

    // -- Exec
    manager.set_agent(&session_id, "dev").await?;

    // -- Check
    let session = manager
        .get_session(&session_id)
        .ok_or("session must exist")?;
    let runtime = session.runtime().lock().await;
    assert_eq!(
        runtime.persona.agent_identity(),
        Some("dev"),
        "the switch must set the agent identity"
    );
    Ok(())
}

#[tokio::test]
async fn test_session_manager_set_agent_applies_permissions_overlay() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    isolate_xdg_config(temp.path());
    let manager = manager(temp.path()).await?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;
    write_persona(
        &cwd,
        "dev",
        "---\nname: dev\npermissions:\n  http: allow\n---\n\nDev agent.\n",
    )?;
    let session_id = manager.new_session(cwd, Vec::new()).await?;

    // -- Exec
    manager.set_agent(&session_id, "dev").await?;

    // -- Check
    let session = manager
        .get_session(&session_id)
        .ok_or("session must exist")?;
    let runtime = session.runtime().lock().await;
    let decision = runtime
        .permission_state
        .permissions()
        .evaluate("http", &serde_json::Map::new());
    assert_eq!(
        decision.action,
        PermissionAction::Allow,
        "the persona overlay must allow `http`; got: {decision:?}"
    );
    Ok(())
}

#[tokio::test]
async fn test_session_manager_set_agent_clears_session_grants() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    isolate_xdg_config(temp.path());
    let manager = manager(temp.path()).await?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;
    write_persona(&cwd, "dev", "---\nname: dev\n---\n\nDev agent.\n")?;
    let session_id = manager.new_session(cwd, Vec::new()).await?;
    let session = manager
        .get_session(&session_id)
        .ok_or("session must exist")?;
    {
        let runtime = session.runtime().lock().await;
        let args = serde_json::Map::new();
        let decision = runtime
            .permission_state
            .permissions()
            .evaluate("http", &args);
        runtime
            .permission_state
            .session_grants_arc()
            .lock()
            .unwrap()
            .insert_allow_always(&decision, "http", "closure", &args);
    }

    // -- Exec
    manager.set_agent(&session_id, "dev").await?;

    // -- Check
    let runtime = session.runtime().lock().await;
    let args = serde_json::Map::new();
    let decision = runtime
        .permission_state
        .permissions()
        .evaluate("http", &args);
    let held = runtime
        .permission_state
        .session_grants_arc()
        .lock()
        .unwrap()
        .get(&decision, "http", "closure", &args);
    assert!(
        held.is_none(),
        "the switch must clear the session grant cache; got: {held:?}"
    );
    Ok(())
}

#[tokio::test]
async fn test_session_manager_set_agent_unknown_agent_returns_error_and_keeps_agent() -> Result<()>
{
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    isolate_xdg_config(temp.path());
    let manager = manager(temp.path()).await?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;
    write_persona(&cwd, "dev", "---\nname: dev\n---\n\nDev agent.\n")?;
    let session_id = manager.new_session(cwd, Vec::new()).await?;
    manager.set_agent(&session_id, "dev").await?;

    // -- Exec
    let result = manager.set_agent(&session_id, "ghost").await;

    // -- Check
    let err = result.err().ok_or("an unknown agent must be rejected")?;
    assert!(
        matches!(err, AcpError::Config(_)),
        "an unknown agent must produce AcpError::Config, got: {err:?}"
    );
    let session = manager
        .get_session(&session_id)
        .ok_or("session must exist")?;
    let runtime = session.runtime().lock().await;
    assert_eq!(
        runtime.persona.agent_identity(),
        Some("dev"),
        "a rejected switch must leave the agent unchanged"
    );
    Ok(())
}

#[tokio::test]
async fn test_session_manager_set_model_returns_agent_and_model_options() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    isolate_xdg_config(temp.path());
    let manager = manager(temp.path()).await?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;
    let session_id = manager.new_session(cwd, Vec::new()).await?;

    // -- Exec
    let options = manager.set_model(&session_id, "ollama/llama3").await?;

    // -- Check
    let ids = option_ids(&options);
    assert!(
        ids.contains(&crate::MODEL_CONFIG_ID.to_string()),
        "the response must carry the model option; got: {ids:?}"
    );
    assert!(
        ids.contains(&crate::AGENT_CONFIG_ID.to_string()),
        "the response must carry the agent option; got: {ids:?}"
    );
    Ok(())
}

#[tokio::test]
async fn test_handle_set_config_option_agent_id_switches_agent() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    isolate_xdg_config(temp.path());
    let manager = manager(temp.path()).await?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;
    write_persona(&cwd, "dev", "---\nname: dev\n---\n\nDev agent.\n")?;
    let session_id = manager.new_session(cwd, Vec::new()).await?;
    let request = SetSessionConfigOptionRequest::new(session_id, crate::AGENT_CONFIG_ID, "dev");

    // -- Exec
    let response = crate::handle_set_config_option(&manager, request).await?;

    // -- Check
    assert_eq!(
        current_value(&response.config_options).as_deref(),
        Some("dev"),
        "the response must carry the new agent as the current value"
    );
    Ok(())
}
