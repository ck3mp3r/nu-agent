use super::*;

/// A spec whose input limit is far below the seeded conversation, so the
/// trigger judges the turn's request over it.
pub(super) fn turn_model_spec() -> Result<std::sync::Arc<rig::catalog::ModelSpec>> {
    let spec = crate::conversation::compaction::compactor::resolve_model_spec(
        "openai",
        "gpt-4o",
        Some(8_000),
        Some(1_024),
        None,
    )
    .ok_or("openai must resolve to a registered provider id")?;
    Ok(std::sync::Arc::new(spec))
}
