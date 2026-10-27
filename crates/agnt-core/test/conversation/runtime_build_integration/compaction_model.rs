//! The compaction (summarizer) model: which role it resolves from and what
//! the planning spec and wire report.

use super::*;

/// With `[models.light]` configured, the compactor is built from the light
/// role: the planning spec describes the light model and the wire carries its
/// price.
#[tokio::test]
async fn test_runtime_build_compaction_model_uses_the_light_role() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let store_type = StoreType::Memory;

    // -- Exec
    let runtime = build_runtime(make_params(
        temp.path().to_path_buf(),
        offline_config(),
        Some(light_plugin_config()),
        store_type,
    ))
    .await
    .map_err(|e| format!("should build runtime: {e:?}"))?;

    // -- Check
    let spec = runtime
        .compaction
        .spec()
        .ok_or("the planning spec must resolve")?;
    assert_eq!(
        spec.id, "light-model",
        "the planning spec must describe the light model"
    );
    assert_eq!(
        spec.context_window,
        Some(262_000),
        "the planning spec must carry the light row's context limit"
    );
    let wire = runtime.compaction.model();
    assert_eq!(
        wire.id(),
        Some("light-model"),
        "the compactor's wire must be the light model"
    );
    let pricing = wire
        .spec()
        .and_then(|spec| spec.pricing)
        .ok_or("the light row's price must reach the wire")?;
    assert!(
        (pricing.input - 0.1).abs() < 1e-12 && (pricing.output - 0.2).abs() < 1e-12,
        "the wire must carry the light row's rates, got {} and {}",
        pricing.input,
        pricing.output
    );
    Ok(())
}

/// Without `[models.light]`, the compactor keeps the startup active model.
#[tokio::test]
async fn test_runtime_build_compaction_model_falls_back_to_the_startup_model() -> Result<()> {
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
        .compaction
        .spec()
        .ok_or("the planning spec must resolve")?;
    assert_eq!(
        spec.id, "llama3",
        "without a light role the planning spec must describe the startup model"
    );
    assert_eq!(
        spec.context_window,
        Some(defaults::MAX_CONTEXT_TOKENS),
        "the startup model's default context window must reach the spec"
    );
    assert_eq!(
        runtime.compaction.model().id(),
        Some("llama3"),
        "the compactor's wire must be the startup model"
    );
    Ok(())
}

/// A custom-named provider block that declares the `ollama` rig impl must
/// still resolve the planning spec: the spec names the vendor the wire
/// reports, not the config key.
#[tokio::test]
async fn test_runtime_build_compaction_model_resolves_a_custom_named_light_provider() -> Result<()>
{
    // -- Setup & Fixtures: the light role points at `ollama-cloud`, a config
    // key that is not a rig provider id.
    let temp = tempfile::tempdir()?;
    let store_type = StoreType::Memory;

    // -- Exec
    let runtime = build_runtime(make_params(
        temp.path().to_path_buf(),
        offline_config(),
        Some(custom_named_light_plugin_config()),
        store_type,
    ))
    .await
    .map_err(|e| format!("should build runtime: {e:?}"))?;

    // -- Check
    let spec = runtime
        .compaction
        .spec()
        .ok_or("the planning spec must resolve for a custom-named provider")?;
    assert_eq!(
        spec.id, "light-model",
        "the planning spec must describe the light model"
    );
    assert_eq!(
        spec.context_window,
        Some(262_000),
        "the planning spec must carry the light row's context limit"
    );
    let wire = runtime.compaction.model();
    assert_eq!(
        wire.id(),
        Some("light-model"),
        "the compactor's wire must be the light model"
    );
    let pricing = wire
        .spec()
        .and_then(|spec| spec.pricing)
        .ok_or("the light row's price must reach the wire")?;
    assert!(
        (pricing.input - 0.1).abs() < 1e-12 && (pricing.output - 0.2).abs() < 1e-12,
        "the wire must carry the light row's rates, got {} and {}",
        pricing.input,
        pricing.output
    );
    Ok(())
}

/// The startup-fallback path resolves the same way: a custom-named startup
/// provider that declares the `ollama` rig impl still yields a planning spec.
#[tokio::test]
async fn test_runtime_build_compaction_model_resolves_a_custom_named_startup_provider() -> Result<()>
{
    // -- Setup & Fixtures: no light role, and the startup provider is the
    // custom config key `ollama-cloud`.
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
        .compaction
        .spec()
        .ok_or("the planning spec must resolve for a custom-named provider")?;
    assert_eq!(
        spec.id, "llama3",
        "the planning spec must describe the startup model"
    );
    assert_eq!(
        spec.context_window,
        Some(defaults::MAX_CONTEXT_TOKENS),
        "the startup model's default context window must reach the spec"
    );
    assert_eq!(
        runtime.compaction.model().id(),
        Some("llama3"),
        "the compactor's wire must be the startup model"
    );
    Ok(())
}
