use nu_protocol::LabeledError;

use agnt_runtime::config;
use agnt_runtime::conversation::builder::{
    AgentRuntimeBuilder, BuildArtifacts, BuildInput, merge_compaction_configs,
};

/// Thin binary entry-point: merges the loaded plugin config's compaction block
/// and delegates all registration work to `AgentRuntimeBuilder`.
///
/// `input.merged_compaction` is ignored on entry — this function computes the
/// merged value and injects it before build. No compaction key has a CLI flag:
/// the block is the one surface.
pub(crate) async fn register_tools(input: BuildInput<'_>) -> Result<BuildArtifacts, LabeledError> {
    let plugin_compaction = config::toml_config::load().ok().and_then(|c| c.compaction);
    let merged = merge_compaction_configs(
        plugin_compaction.as_ref(),
        &config::CompactionConfig::default(),
    );
    AgentRuntimeBuilder::new(BuildInput {
        merged_compaction: merged,
        ..input
    })
    .build()
    .await
}
