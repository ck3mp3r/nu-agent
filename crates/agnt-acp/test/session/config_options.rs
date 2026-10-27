use super::*;

// ---------------------------------------------------------------------------
// Model config options
// ---------------------------------------------------------------------------

/// A models cache holding `models` under `provider`.
pub(super) fn models_cache(provider: &str, models: &[(&str, &str)]) -> Result<ModelsCache> {
    let models: serde_json::Map<String, serde_json::Value> = models
        .iter()
        .map(|(id, name)| {
            (
                id.to_string(),
                serde_json::json!({
                    "id": id,
                    "name": name,
                    "tool_call": true,
                    "limit": { "context": 8192, "output": 4096 },
                }),
            )
        })
        .collect();
    let json = serde_json::json!({
        (provider): {
            "id": provider,
            "name": provider,
            "models": models,
        }
    });
    Ok(serde_json::from_value(json)?)
}

/// A manager that advertises `cache` as its models cache.
pub(super) async fn manager_with_cache(
    dir: &Path,
    cache: ModelsCache,
) -> Result<AcpSessionManager<MockRuntimeBuilder>> {
    Ok(AcpSessionManager::with_runtime_builder(
        rig_config(dir),
        dir.join("config.toml"),
        tokio::runtime::Handle::current(),
        MockRuntimeBuilder::default(),
    )
    .with_models_cache(Some(Arc::new(cache))))
}

/// The current value of the first option.
pub(super) fn current_value(options: &[SessionConfigOption]) -> Option<String> {
    let option = options.first()?;
    let SessionConfigKind::Select(select) = &option.kind else {
        return None;
    };
    Some(select.current_value.to_string())
}

/// The selectable values of the first option.
pub(super) fn select_values(options: &[SessionConfigOption]) -> Vec<String> {
    let Some(option) = options.first() else {
        return Vec::new();
    };
    let SessionConfigKind::Select(select) = &option.kind else {
        return Vec::new();
    };
    match &select.options {
        SessionConfigSelectOptions::Ungrouped(values) => {
            values.iter().map(|value| value.value.to_string()).collect()
        }
        SessionConfigSelectOptions::Grouped(groups) => groups
            .iter()
            .flat_map(|group| group.options.iter())
            .map(|value| value.value.to_string())
            .collect(),
        _ => Vec::new(),
    }
}

#[tokio::test]
async fn test_session_manager_model_config_options_without_cache_lists_current() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let manager = manager(temp.path()).await?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;
    let session_id = manager.new_session(cwd, Vec::new()).await?;

    // -- Exec
    let options = manager.model_config_options(&session_id).await?;

    // -- Check
    assert_eq!(current_value(&options).as_deref(), Some("ollama/llama3"));
    assert_eq!(select_values(&options), vec!["ollama/llama3".to_string()]);
    Ok(())
}

#[tokio::test]
async fn test_session_manager_model_config_options_unknown_session_returns_error() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let manager = manager(temp.path()).await?;

    // -- Exec
    let result = manager.model_config_options("no-such-session").await;

    // -- Check
    let err = result.err().ok_or("an unknown session must fail")?;
    assert!(
        matches!(err, AcpError::SessionNotFound(_)),
        "unknown id must produce AcpError::SessionNotFound, got: {err:?}"
    );
    Ok(())
}

#[tokio::test]
async fn test_session_manager_set_model_valid_updates_current_value() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let cache = models_cache(
        "ollama-cloud",
        &[("llama3", "Llama 3"), ("llama4", "Llama 4")],
    )?;
    let manager = manager_with_cache(temp.path(), cache).await?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;
    let session_id = manager.new_session(cwd, Vec::new()).await?;

    // -- Exec
    let options = manager.set_model(&session_id, "ollama/llama4").await?;

    // -- Check
    assert_eq!(current_value(&options).as_deref(), Some("ollama/llama4"));
    let refreshed = manager.model_config_options(&session_id).await?;
    assert_eq!(
        current_value(&refreshed).as_deref(),
        Some("ollama/llama4"),
        "the session must keep the switched model"
    );
    Ok(())
}

#[tokio::test]
async fn test_session_manager_set_model_unknown_value_returns_error() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let cache = models_cache("ollama-cloud", &[("llama3", "Llama 3")])?;
    let manager = manager_with_cache(temp.path(), cache).await?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;
    let session_id = manager.new_session(cwd, Vec::new()).await?;

    // -- Exec
    let result = manager.set_model(&session_id, "ollama/not-a-model").await;

    // -- Check
    let err = result
        .err()
        .ok_or("an unadvertised model must be rejected")?;
    assert!(
        matches!(err, AcpError::Config(_)),
        "an unadvertised model must produce AcpError::Config, got: {err:?}"
    );
    let options = manager.model_config_options(&session_id).await?;
    assert_eq!(
        current_value(&options).as_deref(),
        Some("ollama/llama3"),
        "a rejected switch must leave the model unchanged"
    );
    Ok(())
}

#[tokio::test]
async fn test_session_manager_set_model_unknown_session_returns_error() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let manager = manager(temp.path()).await?;

    // -- Exec
    let result = manager.set_model("no-such-session", "ollama/llama3").await;

    // -- Check
    let err = result.err().ok_or("an unknown session must fail")?;
    assert!(
        matches!(err, AcpError::SessionNotFound(_)),
        "unknown id must produce AcpError::SessionNotFound, got: {err:?}"
    );
    Ok(())
}

#[tokio::test]
async fn test_session_manager_set_model_build_failure_restores_config() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let cache = models_cache("bad-provider", &[("target-model", "Target Model")])?;
    let manager = manager_with_cache(temp.path(), cache).await?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;
    let session_id = manager.new_session(cwd, Vec::new()).await?;

    // Sabotage the provider: switch to an unsupported name and drop the cached
    // client, so the next build_dyn_model() rebuilds and fails.
    {
        let session = manager
            .get_session(&session_id)
            .ok_or("session must exist")?;
        let mut runtime = session.runtime().lock().await;
        runtime.provider.invalidate_cache();
        let config = runtime.provider.provider_config_mut();
        config.provider = "bad-provider".to_string();
        // The sabotaged block declares no catalog, so its name is its section.
        config.catalog = None;
        config.model = "initial".to_string();
    }

    // -- Exec
    let result = manager
        .set_model(&session_id, "bad-provider/target-model")
        .await;

    // -- Check
    let err = result
        .err()
        .ok_or("set_model must fail when build_dyn_model fails")?;
    assert!(
        matches!(err, AcpError::Runtime(_)),
        "build_dyn_model failure must produce AcpError::Runtime, got: {err:?}"
    );
    let options = manager.model_config_options(&session_id).await?;
    assert_eq!(
        current_value(&options).as_deref(),
        Some("bad-provider/initial"),
        "config.model must be restored to 'initial' after build_dyn_model failure"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// session/set_config_option handler
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_handle_set_config_option_valid_value_returns_updated_options() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let cache = models_cache(
        "ollama-cloud",
        &[("llama3", "Llama 3"), ("llama4", "Llama 4")],
    )?;
    let manager = manager_with_cache(temp.path(), cache).await?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;
    let session_id = manager.new_session(cwd, Vec::new()).await?;
    let request =
        SetSessionConfigOptionRequest::new(session_id, crate::MODEL_CONFIG_ID, "ollama/llama4");

    // -- Exec
    let response = crate::handle_set_config_option(&manager, request).await?;

    // -- Check
    assert_eq!(
        current_value(&response.config_options).as_deref(),
        Some("ollama/llama4"),
        "the response must carry the updated current value"
    );
    Ok(())
}

#[tokio::test]
async fn test_handle_set_config_option_unknown_config_id_returns_error() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let manager = manager(temp.path()).await?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;
    let session_id = manager.new_session(cwd, Vec::new()).await?;
    let request = SetSessionConfigOptionRequest::new(session_id, "mode", "ollama/llama3");

    // -- Exec
    let result = crate::handle_set_config_option(&manager, request).await;

    // -- Check
    let err = result
        .err()
        .ok_or("an unknown config id must be rejected")?;
    assert!(
        matches!(err, AcpError::Config(_)),
        "an unknown config id must produce AcpError::Config, got: {err:?}"
    );
    Ok(())
}

#[tokio::test]
async fn test_handle_set_config_option_non_id_value_returns_error() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let manager = manager(temp.path()).await?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;
    let session_id = manager.new_session(cwd, Vec::new()).await?;
    let request = SetSessionConfigOptionRequest::new(
        session_id,
        crate::MODEL_CONFIG_ID,
        SessionConfigOptionValue::boolean(true),
    );

    // -- Exec
    let result = crate::handle_set_config_option(&manager, request).await;

    // -- Check
    let err = result.err().ok_or("a non-id value must be rejected")?;
    assert!(
        matches!(err, AcpError::Config(_)),
        "a non-id value must produce AcpError::Config, got: {err:?}"
    );
    Ok(())
}
