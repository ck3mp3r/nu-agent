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
    assert_eq!(
        runtime.cwd(),
        cwd.as_path(),
        "runtime cwd must come from params"
    );
    assert!(
        runtime.current_session_id().is_none(),
        "a fresh session has no persisted session id"
    );
    assert_eq!(runtime.provider_config().provider, "ollama");
    assert_eq!(runtime.provider_config().model, "llama3");
    assert_eq!(runtime.agent_identity(), Some("test-agent"));
    assert_eq!(runtime.agent_persona_body(), Some("persona body"));
    assert_eq!(runtime.cached_agents_chain(), Some("agents chain"));
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
        runtime.hook_context().model(),
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
async fn test_runtime_build_build_runtime_keeps_the_default_compaction_policy() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let store_type = StoreType::Memory;

    // -- Exec
    let runtime = build_runtime(make_params(
        temp.path().to_path_buf(),
        offline_config(),
        None,
        store_type,
    ))
    .await
    .map_err(|e| format!("should build runtime: {e:?}"))?;

    // -- Check: no `[compaction]` block, so the knob keeps our default and the
    // policy keeps the pinned summary budget.
    let policy = runtime.compaction().policy();
    assert_eq!(
        runtime.compaction().reserve_pct,
        crate::config::defaults::DEFAULT_RESERVE_PCT,
        "an unset reserve_pct keeps our 20 percent"
    );
    assert_eq!(
        policy.summarizer.limits.summary_tokens,
        crate::config::defaults::COMPACTION_SUMMARY_TOKENS,
        "an unset summary budget keeps the pinned default"
    );
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
