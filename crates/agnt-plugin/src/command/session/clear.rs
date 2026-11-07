use std::path::PathBuf;

use crate::plugin::AgentPlugin;
use agnt_runtime::session::RigStoreConfig;
use agnt_runtime::session::prefix::dir_prefix;
use nu_plugin::{EngineInterface, EvaluatedCall, PluginCommand, SimplePluginCommand};
use nu_protocol::{Category, Example, LabeledError, Signature, SyntaxShape, Value};

/// Abstracts the engine's cwd query so `run_inner` can be tested without a live plugin context.
pub(crate) trait CwdInterface {
    fn get_current_dir(&self) -> Result<String, LabeledError>;
}

impl CwdInterface for EngineInterface {
    fn get_current_dir(&self) -> Result<String, LabeledError> {
        self.get_current_dir()
            .map_err(|e| LabeledError::new(format!("Failed to get working directory: {e}")))
    }
}

/// The `agent session clear` command deletes a session's record from the session store.
pub struct AgentSessionClear;

impl Default for AgentSessionClear {
    fn default() -> Self {
        Self
    }
}

impl AgentSessionClear {
    pub(crate) async fn run_inner<C: CwdInterface>(
        &self,
        engine: &C,
        call: &EvaluatedCall,
        rig: &RigStoreConfig,
    ) -> Result<Value, LabeledError> {
        // Get session_id parameter
        let session_id: String = call.req(0)?;

        let cwd = PathBuf::from(engine.get_current_dir()?);
        let prefix = dir_prefix(&cwd);
        let target_id = format!("{prefix}-{session_id}");

        // The session must exist in the rig-memory store.
        if !rig.holds(&target_id).await.unwrap_or(false) {
            return Err(LabeledError::new(format!(
                "Session not found: {session_id}"
            )));
        }

        // Delete the session's rig-memory store: its logs and its files.
        rig.destroy(&target_id)
            .await
            .map_err(|e| LabeledError::new(format!("Failed to delete session: {e}")))?;

        // Return empty value (success)
        Ok(Value::nothing(call.head))
    }
}

impl SimplePluginCommand for AgentSessionClear {
    type Plugin = AgentPlugin;

    fn name(&self) -> &str {
        "agent session clear"
    }

    fn description(&self) -> &str {
        "Delete a session and its stored conversation"
    }

    fn extra_description(&self) -> &str {
        "Permanently removes the session's logs and files from the session database (`sessions.db`), or from the in-memory database."
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["session", "clear", "delete", "remove"]
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![Example {
            description: "Delete a session",
            example: "agent session clear my-project",
            result: None,
        }]
    }

    fn signature(&self) -> Signature {
        Signature::build(PluginCommand::name(self))
            .required("id", SyntaxShape::String, "Session ID to delete")
            .named(
                "store",
                SyntaxShape::String,
                "Session store backend: sqlite|memory",
                None,
            )
            .category(Category::Experimental)
    }

    fn run(
        &self,
        plugin: &AgentPlugin,
        engine: &EngineInterface,
        call: &EvaluatedCall,
        _input: &Value,
    ) -> Result<Value, LabeledError> {
        let store_type = plugin.resolve_store_type(call)?;
        let rig = crate::plugin::rig_store_config(store_type)?;
        crate::block_on!(plugin, self.run_inner(engine, call, &rig))
    }
}
