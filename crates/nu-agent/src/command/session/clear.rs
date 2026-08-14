use std::path::PathBuf;

use crate::plugin::AgentPlugin;
use nu_agent_core::session::SessionStore;
use nu_agent_core::session::SessionStoreImpl;
use nu_agent_core::session::prefix::{dir_prefix, dir_prefix_legacy};
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

/// The `agent session clear` command deletes a session by removing its JSONL file.
pub struct AgentSessionClear;

impl AgentSessionClear {
    /// Creates a new AgentSessionClear command. The session store is obtained
    /// lazily from the plugin in `run()`.
    pub fn new() -> Self {
        Self
    }
}

impl Default for AgentSessionClear {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentSessionClear {
    pub(crate) async fn run_inner<C: CwdInterface>(
        &self,
        engine: &C,
        call: &EvaluatedCall,
        store: &SessionStoreImpl,
    ) -> Result<Value, LabeledError> {
        // Get session_id parameter
        let session_id: String = call.req(0)?;

        let cwd = PathBuf::from(engine.get_current_dir()?);
        let new_prefix = dir_prefix(&cwd);
        let legacy_prefix = dir_prefix_legacy(&cwd);
        let new_id = format!("{new_prefix}-{session_id}");
        let legacy_id = format!("{legacy_prefix}-{session_id}");

        // Determine which prefixed ID exists, trying the new prefix first and
        // falling back to the legacy prefix for sessions created before the
        // prefix length increase.
        let new_exists = store
            .load(&new_id)
            .await
            .map_err(|e| LabeledError::new(format!("Failed to load session: {}", e)))?
            .is_some();
        let target_id = if new_exists {
            new_id
        } else {
            let legacy_exists = store
                .load(&legacy_id)
                .await
                .map_err(|e| LabeledError::new(format!("Failed to load session: {}", e)))?
                .is_some();
            if legacy_exists {
                legacy_id
            } else {
                return Err(LabeledError::new(format!(
                    "Session not found: {session_id}"
                )));
            }
        };

        // Delete the session
        store
            .delete(&target_id)
            .await
            .map_err(|e| LabeledError::new(format!("Failed to delete session: {}", e)))?;

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
        "Delete a session by removing its JSONL file from cache"
    }

    fn extra_description(&self) -> &str {
        "Permanently deletes the session's JSONL file from the cache directory."
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
                "Session store backend: sqlite|jsonl",
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
        let store = plugin
            .create_store_with(store_type)
            .map_err(|e| LabeledError::new(format!("Failed to create session store: {e}")))?;
        crate::block_on!(plugin, self.run_inner(engine, call, &store))
    }
}
