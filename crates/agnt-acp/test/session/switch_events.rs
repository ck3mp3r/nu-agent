//! The switch events the runtime publishes on the session channel.
//!
//! Both switches funnel through `ModelSwitching`, so these tests drive the ACP
//! config-option handler — one caller of the shared site — and read the session
//! bus. The TUI picker reaches the same site through the orchestrator.

use super::*;

use super::agent_options::{isolate_xdg_config, write_persona};
use super::config_options::{manager_with_cache, models_cache};

/// One switch event, flattened for comparison: `(event, from, to)`.
type Switch = (String, Option<String>, String);

/// The switch events currently buffered on the session channel.
fn take_switches(rx: &mut agnt_core::bus::SessionRx) -> Vec<Switch> {
    let mut switches = Vec::new();
    loop {
        match rx.try_recv() {
            Ok(SessionEvent::AgentSwitched { from, to }) => {
                switches.push(("agent_switched".to_string(), from, to));
            }
            Ok(SessionEvent::ModelSwitched { from, to }) => {
                switches.push(("model_switched".to_string(), Some(from), to));
            }
            Ok(_) => {}
            Err(agnt_core::bus::TryRecvError::Lagged(_)) => continue,
            Err(_) => break,
        }
    }
    switches
}

/// Subscribe the session channel before the switch runs: a broadcast channel
/// does not buffer for later subscribers.
fn subscribe(
    manager: &AcpSessionManager<MockRuntimeBuilder>,
    session_id: &str,
) -> Result<agnt_core::bus::SessionRx> {
    Ok(manager
        .get_session(session_id)
        .ok_or("session must exist")?
        .bus()
        .session()
        .subscribe())
}

#[tokio::test]
async fn test_session_manager_agent_switch_publishes_agent_switched() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    isolate_xdg_config(temp.path());
    let manager = manager(temp.path()).await?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;
    write_persona(&cwd, "dev", "---\nname: dev\n---\n\nDev agent.\n")?;
    let session_id = manager.new_session(cwd, Vec::new()).await?;
    let mut rx = subscribe(&manager, &session_id)?;
    let request =
        SetSessionConfigOptionRequest::new(session_id.clone(), crate::AGENT_CONFIG_ID, "dev");

    // -- Exec
    let _ = crate::handle_set_config_option(&manager, request).await?;

    // -- Check: a persona without a model override changes one fact only.
    assert_eq!(
        take_switches(&mut rx),
        vec![("agent_switched".to_string(), None, "dev".to_string())],
        "an agent switch publishes exactly one agent_switched"
    );
    Ok(())
}

#[tokio::test]
async fn test_session_manager_model_switch_publishes_model_switched() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let cache = models_cache(
        "ollama-cloud",
        &[("llama3", "Llama 3"), ("llama4", "Llama 4")],
    )?;
    let manager = manager_with_cache(temp.path(), cache).await?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;
    let session_id = manager.new_session(cwd, Vec::new()).await?;
    let mut rx = subscribe(&manager, &session_id)?;
    let request = SetSessionConfigOptionRequest::new(
        session_id.clone(),
        crate::MODEL_CONFIG_ID,
        "ollama/llama4",
    );

    // -- Exec
    let _ = crate::handle_set_config_option(&manager, request).await?;

    // -- Check
    assert_eq!(
        take_switches(&mut rx),
        vec![(
            "model_switched".to_string(),
            Some("ollama/llama3".to_string()),
            "ollama/llama4".to_string()
        )],
        "a model switch publishes from the old identity to the new one"
    );
    Ok(())
}

#[tokio::test]
async fn test_session_manager_model_switch_to_the_current_model_publishes_from_equal_to()
-> Result<()> {
    // -- Setup & Fixtures: a re-select of the model already in use.
    let temp = tempfile::tempdir()?;
    let cache = models_cache("ollama-cloud", &[("llama3", "Llama 3")])?;
    let manager = manager_with_cache(temp.path(), cache).await?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;
    let session_id = manager.new_session(cwd, Vec::new()).await?;
    let mut rx = subscribe(&manager, &session_id)?;
    let request = SetSessionConfigOptionRequest::new(
        session_id.clone(),
        crate::MODEL_CONFIG_ID,
        "ollama/llama3",
    );

    // -- Exec
    let _ = crate::handle_set_config_option(&manager, request).await?;

    // -- Check: every successful switch publishes, even a no-op one.
    assert_eq!(
        take_switches(&mut rx),
        vec![(
            "model_switched".to_string(),
            Some("ollama/llama3".to_string()),
            "ollama/llama3".to_string()
        )],
        "re-selecting the current model publishes from == to"
    );
    Ok(())
}

#[tokio::test]
async fn test_session_manager_failed_agent_switch_publishes_nothing() -> Result<()> {
    // -- Setup & Fixtures: an agent that does not exist.
    let temp = tempfile::tempdir()?;
    isolate_xdg_config(temp.path());
    let manager = manager(temp.path()).await?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;
    let session_id = manager.new_session(cwd, Vec::new()).await?;
    let mut rx = subscribe(&manager, &session_id)?;
    let request = SetSessionConfigOptionRequest::new(
        session_id.clone(),
        crate::AGENT_CONFIG_ID,
        "does-not-exist",
    );

    // -- Exec
    let result = crate::handle_set_config_option(&manager, request).await;

    // -- Check
    assert!(result.is_err(), "an unknown agent must fail the switch");
    assert!(
        take_switches(&mut rx).is_empty(),
        "a failed switch changes nothing, so it publishes no switch event"
    );
    Ok(())
}
