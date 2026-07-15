use nu_plugin::EvaluatedCall;
use nu_protocol::LabeledError;

use nu_agent_core::config::PluginConfig;
use nu_agent_core::conversation::builder::{
    AgentRuntimeBuilder, BuildArtifacts, BuildInput, merge_compaction_configs,
};

/// Thin binary entry-point: extracts and merges compaction config from CLI and
/// plugin config, then delegates all registration work to `AgentRuntimeBuilder`.
///
/// `input.merged_compaction` is ignored on entry — this function computes the
/// merged value from `call` and `plugin_config_value` and injects it before build.
pub(crate) fn register_tools(
    call: &EvaluatedCall,
    plugin_config_value: Option<&nu_protocol::Value>,
    input: BuildInput<'_>,
) -> Result<BuildArtifacts, LabeledError> {
    let plugin_compaction =
        plugin_config_value.and_then(|v| match PluginConfig::from_plugin_config(v) {
            Ok(pc) => pc.compaction,
            Err(e) => {
                log::warn!("failed to parse plugin config: {e}");
                None
            }
        });
    let cli_compaction = super::args::extract_compaction_flags(call)?;
    let merged = merge_compaction_configs(plugin_compaction.as_ref(), &cli_compaction);
    AgentRuntimeBuilder::new(BuildInput {
        merged_compaction: merged,
        ..input
    })
    .build()
}
