//! Compaction policy wiring tests for `build_acp_runtime()`.

use super::runtime_support::*;
use super::*;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// A config whose `[compaction]` block sets every wired policy field.
const WIRED_COMPACTION_CONFIG: &str = r#"
[models.default]
model = "ollama/llama3"

[providers.ollama]
base_url = "http://localhost:11434"

[compaction]
proactive_threshold_pct = 0.75
reserve = 8000
summary_tokens = 6000
snippet_chars = 1500
unknown_window_chars = 200000
keep_recent = 5000
keep_asked = 1000

[compaction.clearing]
keep_tokens = 9000
keep_under = 250

[compaction.prompts]
system = "system prompt"
"#;

/// The `[compaction]` block must reach the runtime compactor's policy.
#[tokio::test]
async fn test_runtime_build_acp_runtime_wires_the_compaction_policy() -> Result<()> {
    // -- Setup & Fixtures
    force_env_only_vault();
    let temp = tempfile::tempdir()?;
    let config_path = write_config(temp.path(), WIRED_COMPACTION_CONFIG)?;
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
    let policy = runtime.compaction.policy();
    assert_eq!(policy.keep_recent, 5_000);
    assert_eq!(policy.keep_asked, 1_000);
    assert_eq!(policy.summarizer.limits.reserve, 8_000);
    assert_eq!(policy.summarizer.limits.summary_tokens, 6_000);
    assert_eq!(policy.summarizer.limits.snippet_chars, 1_500);
    assert_eq!(policy.summarizer.limits.unknown_window_chars, 200_000);
    assert_eq!(policy.clearing.keep_tokens, 9_000);
    assert_eq!(policy.clearing.keep_under, 250);
    assert_eq!(policy.summarizer.prompts.system, "system prompt");
    assert_eq!(
        policy.near_window,
        rig_memory::CompactionPolicy::default().near_window,
        "`near_window` is not a config key: it stays rig's default"
    );
    Ok(())
}

/// Without a `[compaction]` block the compactor runs rig's default policy.
#[tokio::test]
async fn test_runtime_build_acp_runtime_defaults_the_compaction_policy() -> Result<()> {
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
    assert_eq!(
        runtime.compaction.policy(),
        &rig_memory::CompactionPolicy::default()
    );
    Ok(())
}
