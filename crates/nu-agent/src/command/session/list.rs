use std::sync::Arc;

use crate::plugin::AgentPlugin;
use nu_agent_core::session::SessionStore;
use nu_agent_core::session::prefix::dir_prefix;
use nu_plugin::{EngineInterface, EvaluatedCall, PluginCommand, SimplePluginCommand};
use nu_protocol::{Category, Example, LabeledError, Record, Signature, Value};

/// The `agent session list` command lists all sessions with their statistics.
pub struct AgentSessionList;

impl AgentSessionList {
    /// Creates a new AgentSessionList command. The session store is obtained
    /// lazily from the plugin in `run()`.
    pub fn new() -> Self {
        Self
    }
}

impl Default for AgentSessionList {
    fn default() -> Self {
        Self::new()
    }
}

impl SimplePluginCommand for AgentSessionList {
    type Plugin = AgentPlugin;

    fn name(&self) -> &str {
        "agent session list"
    }

    fn description(&self) -> &str {
        "List all sessions with their statistics"
    }

    fn extra_description(&self) -> &str {
        "Lists all cached sessions with message counts and last activity timestamps."
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["session", "list", "history", "chat"]
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![Example {
            description: "List all sessions",
            example: "agent session list",
            result: None,
        }]
    }

    fn signature(&self) -> Signature {
        Signature::build(PluginCommand::name(self))
            .named(
                "store",
                nu_protocol::SyntaxShape::String,
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
        let store = Arc::new(
            plugin
                .create_store_with(store_type)
                .map_err(|e| LabeledError::new(format!("Failed to create session store: {e}")))?,
        );

        let cwd = std::path::PathBuf::from(
            engine
                .get_current_dir()
                .map_err(|e| LabeledError::new(format!("Failed to get working directory: {e}")))?,
        );
        let prefix = dir_prefix(&cwd);

        // Call SessionStore::list() and post-filter by prefix
        let all_sessions = crate::block_on!(plugin, store.list())
            .map_err(|e| LabeledError::new(format!("Failed to list sessions: {}", e)))?;
        let sessions: Vec<_> = all_sessions
            .into_iter()
            .filter(|info| info.id.starts_with(&prefix))
            .collect();

        // Convert SessionInfo list to Nushell Value (list of records)
        let session_values: Vec<Value> = sessions
            .iter()
            .map(|info| {
                let mut record = Record::new();
                let display_id = info
                    .id
                    .strip_prefix(&format!("{prefix}-"))
                    .unwrap_or(&info.id)
                    .to_string();
                record.push("id", Value::string(display_id, call.head));
                record.push(
                    "message_count",
                    Value::int(info.message_count as i64, call.head),
                );
                record.push(
                    "last_active",
                    Value::string(info.last_active.to_rfc3339(), call.head),
                );
                Value::record(record, call.head)
            })
            .collect();

        Ok(Value::list(session_values, call.head))
    }
}
