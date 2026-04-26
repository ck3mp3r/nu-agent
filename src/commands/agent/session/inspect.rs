use crate::AgentPlugin;
use crate::session::SessionStore;
use nu_plugin::{EngineInterface, EvaluatedCall, PluginCommand, SimplePluginCommand};
use nu_protocol::{Category, LabeledError, Record, Signature, SyntaxShape, Value};

/// The `agent session inspect` command displays full details of a specific session.
pub struct AgentSessionInspect {
    pub(crate) store: SessionStore,
}

impl AgentSessionInspect {
    /// Creates a new AgentSessionInspect command with the given SessionStore.
    pub fn new(store: SessionStore) -> Self {
        Self { store }
    }
}

impl SimplePluginCommand for AgentSessionInspect {
    type Plugin = AgentPlugin;

    fn name(&self) -> &str {
        "agent session inspect"
    }

    fn description(&self) -> &str {
        "Display full details of a specific session"
    }

    fn signature(&self) -> Signature {
        Signature::build(PluginCommand::name(self))
            .required("id", SyntaxShape::String, "Session ID to inspect")
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

        // Load the session
        let session = self
            .store
            .load_session(&session_id)
            .map_err(|e| LabeledError::new(format!("Failed to load session: {}", e)))?;

        // Convert messages to Nushell Value (list of records)
        let message_values: Vec<Value> = session
            .messages()
            .iter()
            .map(|msg| {
                let mut record = Record::new();
                record.push("role", Value::string(msg.role(), call.head));
                record.push("content", Value::string(msg.content(), call.head));
                record.push(
                    "timestamp",
                    Value::string(msg.timestamp().to_rfc3339(), call.head),
                );
                Value::record(record, call.head)
            })
            .collect();

        // Convert config to Nushell Value (record)
        let mut config_record = Record::new();
        config_record.push(
            "compaction_threshold",
            Value::int(session.config().compaction_threshold as i64, call.head),
        );
        config_record.push(
            "compaction_strategy",
            Value::string(
                format!("{:?}", session.config().compaction_strategy),
                call.head,
            ),
        );
        config_record.push(
            "keep_recent",
            Value::int(session.config().keep_recent as i64, call.head),
        );

        // Build the final session record
        let mut session_record = Record::new();
        session_record.push("id", Value::string(session.id(), call.head));
        session_record.push(
            "created_at",
            Value::string(session.created_at().to_rfc3339(), call.head),
        );
        session_record.push(
            "message_count",
            Value::int(session.messages().len() as i64, call.head),
        );
        session_record.push(
            "compaction_count",
            Value::int(session.compaction_count() as i64, call.head),
        );
        session_record.push("config", Value::record(config_record, call.head));
        session_record.push("messages", Value::list(message_values, call.head));

        Ok(Value::record(session_record, call.head))
    }
}
