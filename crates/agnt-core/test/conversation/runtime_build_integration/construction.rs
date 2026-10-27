//! Construction: what `build_runtime` puts on the runtime from its params.

use super::*;

#[tokio::test]
async fn test_runtime_build_build_runtime_constructs_runtime_from_params() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let cwd = temp.path().join("session-cwd");
    std::fs::create_dir_all(&cwd)?;
    let store_type = StoreType::Memory;

    // -- Exec
    let runtime = build_runtime(make_params(cwd.clone(), offline_config(), None, store_type))
        .await
        .map_err(|e| format!("should build runtime: {e:?}"))?;

    // -- Check
    assert_eq!(runtime.cwd, cwd, "runtime cwd must come from params");
    assert!(
        runtime.final_session_id.is_none(),
        "a fresh session has no persisted session id"
    );
    assert_eq!(runtime.provider.config().provider, "ollama");
    assert_eq!(runtime.provider.config().model, "llama3");
    assert_eq!(runtime.persona.agent_identity(), Some("test-agent"));
    assert_eq!(runtime.persona.agent_persona_body(), Some("persona body"));
    assert_eq!(runtime.persona.cached_agents_chain(), Some("agents chain"));
    Ok(())
}

#[tokio::test]
async fn test_runtime_build_build_runtime_stamps_the_hook_context_model() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let store_type = StoreType::Memory;
    let hook_context = crate::hooks::HookAgentContext::default();
    let mut params = make_params(
        temp.path().to_path_buf(),
        offline_config(),
        None,
        store_type,
    );
    params.hook_context = hook_context.clone();

    // -- Exec
    let runtime = build_runtime(params)
        .await
        .map_err(|e| format!("should build runtime: {e:?}"))?;

    // -- Check
    assert_eq!(
        runtime.hook_context.model(),
        "ollama/llama3",
        "the runtime stamps the model identity it starts on"
    );
    assert_eq!(
        hook_context.model(),
        "ollama/llama3",
        "the caller's handle is the one the runtime writes"
    );
    assert!(
        hook_context.persona().is_none(),
        "build_runtime does not invent a persona slug"
    );
    Ok(())
}

#[tokio::test]
async fn test_runtime_build_build_runtime_uses_default_compaction_threshold() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let store_type = StoreType::Memory;
    let expected =
        (defaults::MAX_CONTEXT_TOKENS as f64 * defaults::PROACTIVE_THRESHOLD_PCT) as usize;

    // -- Exec
    let runtime = build_runtime(make_params(
        temp.path().to_path_buf(),
        offline_config(),
        None,
        store_type,
    ))
    .await
    .map_err(|e| format!("should build runtime: {e:?}"))?;

    // -- Check
    assert_eq!(runtime.compaction.threshold_tokens, Some(expected));
    Ok(())
}

#[tokio::test]
async fn test_runtime_build_build_runtime_unsupported_provider_reports_model_error() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let store_type = StoreType::Memory;
    let config = Config {
        provider: "not-a-provider".to_string(),
        model: "whatever".to_string(),
        ..Config::default()
    };

    // -- Exec
    let error = build_runtime(make_params(
        temp.path().to_path_buf(),
        config,
        None,
        store_type,
    ))
    .await
    .err()
    .ok_or("an unsupported provider must fail")?;

    // -- Check
    assert!(
        error.to_string().contains("Failed to build model"),
        "unexpected error: {error}"
    );
    Ok(())
}
