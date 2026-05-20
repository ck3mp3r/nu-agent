use crate::AgentPlugin;
use crate::session::{ConversationStore, JsonlConversationStore, SessionStore};
use nu_plugin::{EngineInterface, EvaluatedCall, PluginCommand, SimplePluginCommand};
use nu_protocol::{Category, LabeledError, Record, Signature, SyntaxShape, Value};
use rig::completion::message::{AssistantContent, UserContent};

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

        // Load the session metadata
        let session = self
            .store
            .load_session(&session_id)
            .map_err(|e| LabeledError::new(format!("Failed to load session: {}", e)))?;

        // Load messages from ConversationStore (rig::completion::Message)
        let conversation_store = JsonlConversationStore::new(self.store.cache_dir().to_path_buf());
        let messages = conversation_store
            .load(&session_id)
            .map_err(|e| LabeledError::new(format!("Failed to load messages: {}", e)))?;

        // Helper function to extract role and text from rig Message
        fn extract_message_info(msg: &rig::completion::Message) -> (String, String) {
            match msg {
                rig::completion::Message::System { content } => {
                    // System content is just a string, not an enum
                    ("system".to_string(), content.clone())
                }
                rig::completion::Message::User { content } => {
                    let text = content.iter().map(|c| match c {
                        UserContent::Text(t) => t.text.clone(),
                        UserContent::ToolResult(t) => format!("Tool result: {:?}", t),
                        UserContent::Image(_) | UserContent::Audio(_) | UserContent::Video(_) | UserContent::Document(_) => {
                            format!("[Media content]")
                        }
                    }).collect::<Vec<_>>().join("\n");
                    ("user".to_string(), text)
                }
                rig::completion::Message::Assistant { content, .. } => {
                    let text = content.iter().map(|c| match c {
                        AssistantContent::Text(t) => t.text.clone(),
                        AssistantContent::ToolCall(tc) => format!("Tool call: {} - {}", tc.function.name, tc.function.arguments),
                        AssistantContent::Reasoning(r) => format!("[Reasoning: {:?}]", r.content),
                        AssistantContent::Image(_) => "[Image]".to_string(),
                    }).collect::<Vec<_>>().join("\n");
                    ("assistant".to_string(), text)
                }
            }
        }

        // Convert messages to Nushell Value (list of records)
        let message_values: Vec<Value> = messages
            .iter()
            .map(|msg| {
                let (role, content) = extract_message_info(msg);
                let mut record = Record::new();
                record.push("role", Value::string(role, call.head));
                record.push("content", Value::string(content, call.head));
                // Note: rig Messages don't have timestamps, so we omit that field
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
            Value::string(session.config().compaction_strategy.as_str(), call.head),
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
            Value::int(messages.len() as i64, call.head),
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
