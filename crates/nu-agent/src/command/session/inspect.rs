use crate::plugin::AgentPlugin;
use nu_agent_core::compaction::CompactionParams;
use nu_agent_core::session::prefix::{dir_prefix, dir_prefix_legacy};
use nu_agent_core::session::{SessionStore, SessionStoreBackend, StoreEntry};
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
pub struct AgentSessionInspect;

impl Default for AgentSessionInspect {
    fn default() -> Self {
        Self
    }
}

impl AgentSessionInspect {
    pub(crate) async fn run_inner<C: CwdInterface>(
        &self,
        engine: &C,
        call: &EvaluatedCall,
        store: &SessionStoreBackend,
    ) -> Result<Value, LabeledError> {
        let session_id: String = call.req(0)?;

        let cwd = std::path::PathBuf::from(engine.get_current_dir()?);
        let new_prefix = dir_prefix(&cwd);
        let legacy_prefix = dir_prefix_legacy(&cwd);
        let new_id = format!("{new_prefix}-{session_id}");
        let legacy_id = format!("{legacy_prefix}-{session_id}");

        // Load session metadata and entries from the store, trying the new
        // prefix first and falling back to the legacy prefix for sessions
        // created before the prefix length increase.
        let loaded = store
            .load(&new_id)
            .await
            .map_err(|e| LabeledError::new(format!("Failed to load session: {e}")))?;
        let (metadata, entries) = match loaded {
            Some(v) => v,
            None => {
                let legacy = store
                    .load(&legacy_id)
                    .await
                    .map_err(|e| LabeledError::new(format!("Failed to load session: {e}")))?;
                legacy
                    .ok_or_else(|| LabeledError::new(format!("Session not found: {session_id}")))?
            }
        };

        // Filter messages from entries (skip compaction markers)
        let messages: Vec<&Message> = entries
            .iter()
            .filter_map(|e| match e {
                StoreEntry::Message(msg) => Some(msg),
                StoreEntry::Marker(_) => None,
            })
            .collect();

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

        // Convert config to Nushell Value (record).
        // Compaction params are not persisted in the new store format;
        // display defaults for the config block.
        let config = CompactionParams::default();
        let mut config_record = Record::new();
        config_record.push(
            "compaction_strategy",
            Value::string(config.compaction_strategy.as_str(), call.head),
        );

        // Build the final session record
        let mut session_record = Record::new();
        session_record.push("id", Value::string(metadata.session_id, call.head));
        session_record.push(
            "created_at",
            Value::string(metadata.created_at.to_rfc3339(), call.head),
        );
        session_record.push(
            "message_count",
            Value::int(messages.len() as i64, call.head),
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
        "Shows full session details including config and message history."
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
            .named(
                "store",
                SyntaxShape::String,
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
