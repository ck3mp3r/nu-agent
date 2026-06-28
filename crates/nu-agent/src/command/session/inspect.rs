use crate::plugin::AgentPlugin;
use nu_agent_core::session::prefix::dir_prefix;
use nu_agent_core::session::{ConversationStore, JsonlConversationStore, SessionStore};
use nu_agent_core::types::{AssistantContent, Message, UserContent};
use nu_plugin::{EngineInterface, EvaluatedCall, PluginCommand, SimplePluginCommand};
use nu_protocol::{Category, Example, LabeledError, Record, Signature, SyntaxShape, Value};

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

/// The `agent session inspect` command displays full details of a specific session.
pub struct AgentSessionInspect {
    pub(crate) store: SessionStore,
}

impl AgentSessionInspect {
    /// Creates a new AgentSessionInspect command with the given SessionStore.
    pub fn new(store: SessionStore) -> Self {
        Self { store }
    }

    pub(crate) fn run_inner<C: CwdInterface>(
        &self,
        engine: &C,
        call: &EvaluatedCall,
    ) -> Result<Value, LabeledError> {
        let session_id: String = call.req(0)?;

        let cwd = std::path::PathBuf::from(engine.get_current_dir()?);
        let prefix = dir_prefix(&cwd);
        let session_id = format!("{prefix}-{session_id}");

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
        fn extract_message_info(msg: &Message) -> (String, String) {
            match msg {
                Message::System { content } => {
                    // System content is just a string, not an enum
                    ("system".to_string(), content.clone())
                }
                Message::User { content } => {
                    let text = content
                        .iter()
                        .map(|c| match c {
                            UserContent::Text(t) => t.text.clone(),
                            UserContent::ToolResult(t) => format!("Tool result: {:?}", t),
                            UserContent::Image(_)
                            | UserContent::Audio(_)
                            | UserContent::Video(_)
                            | UserContent::Document(_) => "[Media content]".to_string(),
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    ("user".to_string(), text)
                }
                Message::Assistant { content, .. } => {
                    let text = content
                        .iter()
                        .map(|c| match c {
                            AssistantContent::Text(t) => t.text.clone(),
                            AssistantContent::ToolCall(tc) => format!(
                                "Tool call: {} - {}",
                                tc.function.name, tc.function.arguments
                            ),
                            AssistantContent::Reasoning(r) => {
                                format!("[Reasoning: {:?}]", r.content)
                            }
                            AssistantContent::Image(_) => "[Image]".to_string(),
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
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
            "compaction_strategy",
            Value::string(
                session.compaction_config().compaction_strategy.as_str(),
                call.head,
            ),
        );
        config_record.push(
            "keep_recent",
            Value::int(session.compaction_config().keep_recent as i64, call.head),
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

impl SimplePluginCommand for AgentSessionInspect {
    type Plugin = AgentPlugin;

    fn name(&self) -> &str {
        "agent session inspect"
    }

    fn description(&self) -> &str {
        "Display full details of a specific session"
    }

    fn extra_description(&self) -> &str {
        "Shows full session details including config, message history, and compaction count."
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["session", "inspect", "view", "history", "messages"]
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![Example {
            description: "Inspect a session by ID",
            example: "agent session inspect my-project",
            result: None,
        }]
    }

    fn signature(&self) -> Signature {
        Signature::build(PluginCommand::name(self))
            .required("id", SyntaxShape::String, "Session ID to inspect")
            .category(Category::Experimental)
    }

    fn run(
        &self,
        _plugin: &AgentPlugin,
        engine: &EngineInterface,
        call: &EvaluatedCall,
        _input: &Value,
    ) -> Result<Value, LabeledError> {
        self.run_inner(engine, call)
    }
}
