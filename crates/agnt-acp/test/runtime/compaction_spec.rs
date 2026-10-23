//! Compaction planning-spec tests for `build_acp_runtime()`.

use super::runtime_support::*;
use super::*;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// A config whose provider is a custom config key declaring the `ollama` rig
/// impl, with a 4096-token context limit.
const CUSTOM_NAMED_CONFIG: &str = r#"
[models.default]
model = "ollama-cloud/llama3"
max_context_tokens = 4096

[providers.ollama-cloud]
provider = "ollama"
base_url = "http://localhost:11434"
"#;

/// A custom-named provider block that declares the `ollama` rig impl must
/// still resolve the planning spec: the spec names the vendor the wire
/// reports, not the config key.
#[tokio::test]
async fn test_runtime_build_acp_runtime_resolves_a_custom_named_compaction_spec() -> Result<()> {
    // -- Setup & Fixtures
    force_env_only_vault();
    let temp = tempfile::tempdir()?;
    let config_path = write_config(temp.path(), CUSTOM_NAMED_CONFIG)?;
    let store = memory_store_type();

    // -- Exec
    let runtime = build_acp_runtime(make_params(
        temp.path().to_path_buf(),
        config_path,
        Vec::new(),
        store,
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
        "the planning spec must describe the config's model"
    );
    assert_eq!(
        spec.context_window,
        Some(4096),
        "the planning spec must carry the config's context limit"
    );
    Ok(())
}

/// A provider name that is itself a rig provider id keeps resolving.
#[tokio::test]
async fn test_runtime_build_acp_runtime_resolves_a_rig_id_compaction_spec() -> Result<()> {
    // -- Setup & Fixtures
    force_env_only_vault();
    let temp = tempfile::tempdir()?;
    let config_path = write_config(temp.path(), BASE_CONFIG)?;
    let store = memory_store_type();

    // -- Exec
    let runtime = build_acp_runtime(make_params(
        temp.path().to_path_buf(),
        config_path,
        Vec::new(),
        store,
    ))
    .await
    .map_err(|e| format!("should build runtime: {e:?}"))?;

    // -- Check
    let spec = runtime
        .compaction
        .spec()
        .ok_or("the planning spec must resolve for a rig provider id")?;
    assert_eq!(
        spec.id, "llama3",
        "the planning spec must describe the config's model"
    );
    Ok(())
}
