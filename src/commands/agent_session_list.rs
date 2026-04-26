use crate::AgentPlugin;
use crate::session::SessionStore;
use nu_plugin::{EngineInterface, EvaluatedCall, PluginCommand, SimplePluginCommand};
use nu_protocol::{Category, LabeledError, Record, Signature, Value};

/// The `agent session list` command lists all sessions with their statistics.
pub struct AgentSessionList {
    pub(crate) store: SessionStore,
}

impl AgentSessionList {
    /// Creates a new AgentSessionList command with the given SessionStore.
    pub fn new(store: SessionStore) -> Self {
        Self { store }
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

    fn signature(&self) -> Signature {
        Signature::build(PluginCommand::name(self)).category(Category::Experimental)
    }

    fn run(
        &self,
        _plugin: &AgentPlugin,
        _engine: &EngineInterface,
        call: &EvaluatedCall,
        _input: &Value,
    ) -> Result<Value, LabeledError> {
        // Call SessionStore::list_sessions()
        let sessions = self
            .store
            .list_sessions()
            .map_err(|e| LabeledError::new(format!("Failed to list sessions: {}", e)))?;

        // Convert SessionInfo list to Nushell Value (list of records)
        let session_values: Vec<Value> = sessions
            .iter()
            .map(|info| {
                let mut record = Record::new();
                record.push("id", Value::string(&info.id, call.head));
                record.push(
                    "message_count",
                    Value::int(info.message_count as i64, call.head),
                );
                record.push(
                    "compaction_count",
                    Value::int(info.compaction_count as i64, call.head),
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

#[cfg(test)]
#[path = "agent_session_list_test.rs"]
mod agent_session_list_test;
