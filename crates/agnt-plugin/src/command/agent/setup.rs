use nu_protocol::LabeledError;

use agnt_runtime::conversation::builder::{AgentRuntimeBuilder, BuildInput};

/// Thin binary entry-point: delegates all registration work to
/// `AgentRuntimeBuilder`.
///
/// No compaction key has a CLI flag: rig's `[compaction]` block is the one
/// surface, and it is rig's own serde table, so a key rig does not define was
/// refused when the config was loaded.
pub(crate) async fn register_tools(input: BuildInput<'_>) -> Result<(), LabeledError> {
    AgentRuntimeBuilder::new(input).build().await
}
