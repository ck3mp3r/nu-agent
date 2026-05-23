use crate::AgentPlugin;
use crate::session::SessionStore;
use nu_plugin::{EngineInterface, EvaluatedCall, PluginCommand, SimplePluginCommand};
use nu_protocol::{Category, Example, LabeledError, Signature, SyntaxShape, Value};

/// The `agent session clear` command deletes a session by removing its JSONL file.
pub struct AgentSessionClear {
    pub(crate) store: SessionStore,
}

impl AgentSessionClear {
    /// Creates a new AgentSessionClear command with the given SessionStore.
    pub fn new(store: SessionStore) -> Self {
        Self { store }
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
        vec![
            Example {
                description: "Delete a session",
                example: "agent session clear my-project",
                result: None,
            },
        ]
    }

    fn signature(&self) -> Signature {
        Signature::build(PluginCommand::name(self))
            .required("id", SyntaxShape::String, "Session ID to delete")
            .category(Category::Experimental)
    }

    fn run(
        &self,
        _plugin: &AgentPlugin,
        _engine: &EngineInterface,
        call: &EvaluatedCall,
        _input: &Value,
    ) -> Result<Value, LabeledError> {
        // Get session_id parameter
        let session_id: String = call.req(0)?;

        // Delete the session
        self.store
            .delete_session(&session_id)
            .map_err(|e| LabeledError::new(format!("Failed to delete session: {}", e)))?;

        // Return empty value (success)
        Ok(Value::nothing(call.head))
    }
}
