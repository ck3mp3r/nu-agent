use super::*;

// ========================================================================
// DynModel<Completion> construction regression tests (task 47715fca)
// ========================================================================

/// Build a `Config` for an OpenAI-compatible provider pointed at a mock
/// server. `base_url` selects the `OpenAiCompletions` variant (targets
/// `/chat/completions`), matching the mock setup.
fn openai_wiremock_config(server_uri: &str, model: &str) -> crate::config::Config {
    crate::config::Config {
        provider: "openai".to_string(),
        provider_impl: None,
        catalog: None,
        model: model.to_string(),
        api_key: Some("fake-key".to_string()),
        base_url: Some(server_uri.to_string()),
        temperature: None,
        max_tokens: None,
        max_context_tokens: None,
        max_output_tokens: None,
        max_tool_turns: None,
        preamble: None,
        read_timeout_secs: None,
        max_tool_result_bytes: None,
        model_context_tokens: None,
        context_warning_threshold: None,
        max_retries: None,
        retry_base_delay_ms: None,
        output_budget_empty_remedy: None,
        output_budget_remedy_mode: None,
        output_budget_raise_enabled: None,
        output_budget_raise_multiplier: None,
        output_budget_raise_cap: None,
        repetition_guard: None,
        max_tool_calls_per_subturn: None,
        additional_params: None,
        a2a_enabled: None,
        a2a_port: None,
        session_store_type: None,
        pricing: None,
    }
}

/// Build a `PluginConfig` whose `openai` provider block points at the mock
/// server, so `switch_model("openai/<model>")` resolves to a config that uses it.
fn openai_wiremock_plugin_config(server_uri: &str) -> crate::config::PluginConfig {
    let mut plugin_config = crate::config::PluginConfig::default();
    plugin_config.providers.insert(
        "openai".to_string(),
        crate::config::ProviderConfig {
            api_key: Some("fake-key".to_string()),
            base_url: Some(server_uri.to_string()),
            ..Default::default()
        },
    );
    plugin_config
}

/// Drive a single completion through a `DynModel<Completion>` against the
/// mock server, consuming the stream so the request is actually sent.
async fn drive_completion(model: &rig::DynModel<rig::operation::Completion>) -> Result<()> {
    use futures::StreamExt;
    let stream = model
        .stream(rig::completion::CompletionRequest::new("hello"))
        .map_err(|e| format!("completion stream: {e}"))?;
    let mut stream = std::pin::pin!(stream);
    while let Some(item) = stream.next().await {
        let _ = item.map_err(|e| format!("stream item: {e}"))?;
    }
    Ok(())
}

/// Extract the `"model"` field from the first captured request body.
async fn captured_model_field(server: &agnt_testkit::MockServer) -> Result<String> {
    let requests = agnt_testkit::received_requests(server)
        .await
        .ok_or("no requests captured")?;
    let req = requests.first().ok_or("no request captured")?;
    let body: serde_json::Value = serde_json::from_slice(&req.body)?;
    Ok(body["model"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| format!("request body has no string 'model' field: {body}"))?)
}

/// RED/GREEN: after a model switch, the handle rebuilt via the canonical
/// constructor must send the bare model name (not "provider/model") in the
/// completion request. Before the fix this fails because the handle was built
/// from the `"provider/model"` identity string.
#[tokio::test]
async fn switch_model_builds_handle_from_bare_model_name() -> Result<()> {
    // -- Setup & Fixtures
    let server = agnt_testkit::start_server().await;
    agnt_testkit::mount_sse_response(
        &server,
        "POST",
        None,
        agnt_testkit::sse_text_body("hello from mock"),
    )
    .await;

    let config = openai_wiremock_config(&server.uri(), "glm-5.3-flash");
    let plugin_config = openai_wiremock_plugin_config(&server.uri());
    let mut state =
        crate::conversation::state::provider::ProviderState::new(config, Some(plugin_config));

    // -- Exec
    // switch_model resolves the spec and invalidates the cache; the canonical
    // constructor then rebuilds the handle from the bare model name.
    let identity = state.switch_model("openai/glm-5.3-flash")?;
    let handle = state.build_dyn_model().await?;
    drive_completion(&handle).await?;

    // -- Check
    let model = captured_model_field(&server).await?;
    assert_eq!(
        model, "glm-5.3-flash",
        "request model must be the bare name, not the provider-prefixed identity"
    );
    assert_eq!(identity, "openai/glm-5.3-flash");
    Ok(())
}

/// Regression prevention: the startup path (canonical constructor on a fresh
/// ProviderState) must also build the handle from the bare model name.
#[tokio::test]
async fn startup_builds_handle_from_bare_model_name() -> Result<()> {
    // -- Setup & Fixtures
    let server = agnt_testkit::start_server().await;
    agnt_testkit::mount_sse_response(
        &server,
        "POST",
        None,
        agnt_testkit::sse_text_body("hello from mock"),
    )
    .await;

    let config = openai_wiremock_config(&server.uri(), "glm-5.3-flash");
    let mut state = crate::conversation::state::provider::ProviderState::new(config, None);

    // -- Exec
    let handle = state.build_dyn_model().await?;
    drive_completion(&handle).await?;

    // -- Check
    let model = captured_model_field(&server).await?;
    assert_eq!(
        model, "glm-5.3-flash",
        "startup handle must be built from the bare model name"
    );
    Ok(())
}

// ========================================================================
// switch_agent shared_model rewrite regression tests (task 74835100)
// ========================================================================

/// Characterise the switch_agent model rewrite: when the persona specifies a
/// model, the shared handle bound in `self.shared_model` must be rebuilt via
/// the canonical constructor so the persona's model serves the next turn
/// (instead of the previous model silently remaining active).
#[tokio::test]
async fn switch_agent_rebuilds_shared_handle_when_model_present() -> Result<()> {
    // -- Setup & Fixtures
    let server = agnt_testkit::start_server().await;
    agnt_testkit::mount_sse_response(
        &server,
        "POST",
        None,
        agnt_testkit::sse_text_body("hello from mock"),
    )
    .await;

    // Shared model, mirroring runtime.rs dyn_model: Arc<Mutex<DynModel<Completion>>>.
    // Start bound to a previous model.
    let mut state = crate::conversation::state::provider::ProviderState::new(
        openai_wiremock_config(&server.uri(), "old-model"),
        Some(openai_wiremock_plugin_config(&server.uri())),
    );
    let shared_model: std::sync::Arc<std::sync::Mutex<rig::DynModel<rig::operation::Completion>>> =
        std::sync::Arc::new(std::sync::Mutex::new(state.build_dyn_model().await?));

    // -- Exec
    // Replicate switch_agent's model-rewrite block (runtime.rs:374-377).
    // The persona model field is a "provider/model" spec (resolve_model splits
    // on '/'), so switch_model resolves it and the canonical constructor then
    // rebuilds the model from the bare name.
    let persona_model = Some("openai/glm-5.3-flash".to_string());
    if let Some(model) = persona_model {
        let _ = state.switch_model(&model);
        if let Ok(new_model) = state.build_dyn_model().await {
            *shared_model
                .lock()
                .map_err(|_| "model mutex poisoned".to_string())? = new_model;
        }
    }

    // -- Check
    let handle = shared_model
        .lock()
        .map_err(|_| "model mutex poisoned".to_string())?
        .clone();
    drive_completion(&handle).await?;
    let model = captured_model_field(&server).await?;
    assert_eq!(
        model, "glm-5.3-flash",
        "shared handle must be rebuilt from the persona's bare model name"
    );
    Ok(())
}

/// Characterise the no-model branch: when the persona has no model field, the
/// shared handle must remain unchanged (not rebound to anything else).
#[tokio::test]
async fn switch_agent_keeps_shared_handle_when_no_model() -> Result<()> {
    // -- Setup & Fixtures
    let server = agnt_testkit::start_server().await;
    agnt_testkit::mount_sse_response(
        &server,
        "POST",
        None,
        agnt_testkit::sse_text_body("hello from mock"),
    )
    .await;

    let mut state = crate::conversation::state::provider::ProviderState::new(
        openai_wiremock_config(&server.uri(), "glm-5.3-flash"),
        Some(openai_wiremock_plugin_config(&server.uri())),
    );
    let shared_model: std::sync::Arc<std::sync::Mutex<rig::DynModel<rig::operation::Completion>>> =
        std::sync::Arc::new(std::sync::Mutex::new(state.build_dyn_model().await?));

    // -- Exec
    // Replicate switch_agent's model-rewrite block with NO persona model.
    let persona_model: Option<String> = None;
    if let Some(model) = persona_model {
        let _ = state.switch_model(&model);
        if let Ok(new_model) = state.build_dyn_model().await {
            *shared_model
                .lock()
                .map_err(|_| "model mutex poisoned".to_string())? = new_model;
        }
    }

    // -- Check
    let handle = shared_model
        .lock()
        .map_err(|_| "model mutex poisoned".to_string())?
        .clone();
    drive_completion(&handle).await?;
    let model = captured_model_field(&server).await?;
    assert_eq!(
        model, "glm-5.3-flash",
        "shared handle must remain unchanged when the persona has no model"
    );
    Ok(())
}
