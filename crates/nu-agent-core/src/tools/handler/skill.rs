use serde_json::Value as JsonValue;
use std::path::Path;

use super::{ToolHandlerError, builtin_tool::BuiltinTool};
use crate::bus::Bus;

#[derive(Debug, serde::Deserialize)]
struct SkillArgs {
    name: String,
}

pub struct SkillTool;

impl BuiltinTool for SkillTool {
    const NAME: &'static str = "skill";

    async fn execute(
        args: &JsonValue,
        cwd: &Path,
        _bus: &Bus,
    ) -> Result<JsonValue, ToolHandlerError> {
        let args: SkillArgs = serde_json::from_value(args.clone())
            .map_err(|e| ToolHandlerError::validation(format!("Invalid skill arguments: {e}")))?;

        let resolved =
            crate::protocol::skills::resolve_explicit_skill_request_for_cwd(cwd, &args.name)
                .map_err(|e| {
                    ToolHandlerError::validation(format!("skill resolution failed: {e}"))
                })?;

        let payload = match resolved {
            Some(resolved) => serde_json::json!({
                "name": resolved.name,
                "source": resolved.source.label(),
                "path": resolved.path,
                "content": resolved.content,
            }),
            None => serde_json::json!({
                "name": args.name,
                "found": false,
            }),
        };

        Ok(payload)
    }
}

#[cfg(test)]
#[path = "skill_test.rs"]
mod tests;
