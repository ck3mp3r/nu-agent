use std::path::PathBuf;

use crate::plugin::AgentPlugin;
use nu_agent_core::session::SessionStore;
use nu_agent_core::session::SessionStoreBackend;
use nu_agent_core::session::prefix::{
    dir_prefix, dir_prefix_legacy, filter_sessions_by_cwd, match_prefixs,
};
use nu_plugin::{EngineInterface, EvaluatedCall, PluginCommand, SimplePluginCommand};
use nu_protocol::{Category, Example, LabeledError, Record, Signature, Value};

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

/// The `agent session list` command lists all sessions with their statistics.
pub struct AgentSessionList;

impl Default for AgentSessionList {
    fn default() -> Self {
        Self
    }
}

impl AgentSessionList {
    pub(crate) async fn run_inner<C: CwdInterface>(
        &self,
        engine: &C,
        call: &EvaluatedCall,
        store: &SessionStoreBackend,
    ) -> Result<Value, LabeledError> {
        let cwd = PathBuf::from(engine.get_current_dir()?);
        let new_prefix = dir_prefix(&cwd);
        let legacy_prefix = dir_prefix_legacy(&cwd);

        // Call SessionStore::list() and post-filter by prefix
        let all_sessions = store
            .list()
            .await
            .map_err(|e| LabeledError::new(format!("Failed to list sessions: {e}")))?;
        let sessions = filter_sessions_by_cwd(all_sessions, &cwd);

        // Convert SessionInfo list to Nushell Value (list of records)
        let session_values: Vec<Value> = sessions
            .iter()
            .map(|info| {
                let mut record = Record::new();
                let display_id = match_prefixs(&info.id, &new_prefix, &legacy_prefix)
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
                "Session store backend: sqlite|jsonl|memory",
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
