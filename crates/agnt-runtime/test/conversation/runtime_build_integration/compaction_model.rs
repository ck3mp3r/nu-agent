//! The compaction (summarizer) model: what the planning spec and the wire
//! report. The summarizer is the active model everywhere, so no role selects it.

use super::*;

/// THE runtime build SHALL summarize with the active model: the planning spec
/// describes it and the compactor's wire is it.
#[tokio::test]
async fn test_runtime_build_compaction_model_uses_the_active_model() -> Result<()> {
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

    // -- Check
    let spec = runtime
        .compaction()
        .spec()
        .ok_or("the planning spec must resolve")?;
    assert_eq!(
        spec.id, "llama3",
        "the planning spec must describe the active model"
    );
    assert_eq!(
        spec.context_window,
        Some(defaults::MAX_CONTEXT_TOKENS),
        "the active model's default context window must reach the spec"
    );
    assert_eq!(
        runtime.compaction().model().id(),
        Some("llama3"),
        "the compactor's wire must be the active model"
    );
    Ok(())
}

/// The active-model resolution names the vendor the wire reports, not the config
/// key: a custom-named provider block that declares the `ollama` rig impl still
/// yields a planning spec.
#[tokio::test]
async fn test_runtime_build_compaction_model_resolves_a_custom_named_provider() -> Result<()> {
    // -- Setup & Fixtures: the active provider is the custom config key
    // `ollama-cloud`, which declares the `ollama` rig impl.
    let temp = tempfile::tempdir()?;
    let store_type = StoreType::Memory;

    // -- Exec
    let runtime = build_runtime(make_params(
        temp.path().to_path_buf(),
        custom_named_startup_config(),
        Some(custom_named_startup_plugin_config()),
        store_type,
    ))
    .await
    .map_err(|e| format!("should build runtime: {e:?}"))?;

    // -- Check
    let spec = runtime
        .compaction()
        .spec()
        .ok_or("the planning spec must resolve for a custom-named provider")?;
    assert_eq!(
        spec.id, "llama3",
        "the planning spec must describe the active model"
    );
    assert_eq!(
        spec.context_window,
        Some(defaults::MAX_CONTEXT_TOKENS),
        "the active model's default context window must reach the spec"
    );
    assert_eq!(
        runtime.compaction().model().id(),
        Some("llama3"),
        "the compactor's wire must be the active model"
    );
    Ok(())
}
