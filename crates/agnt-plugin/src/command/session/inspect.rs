use crate::plugin::AgentPlugin;
use agnt_core::types::{AssistantContent, Message, UserContent};
use agnt_runtime::config::CompactionConfig;
use agnt_runtime::conversation::runtime_build::build_compaction_policy;
use agnt_runtime::session::RigStoreConfig;
use agnt_runtime::session::prefix::dir_prefix;
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
        rig: &RigStoreConfig,
        compaction: &CompactionConfig,
    ) -> Result<Value, LabeledError> {
        let session_id: String = call.req(0)?;

        let cwd = std::path::PathBuf::from(engine.get_current_dir()?);
        let prefix = dir_prefix(&cwd);
        let id = format!("{prefix}-{session_id}");

        // The session must exist in the rig-memory store.
        if !rig.holds(&id).await.unwrap_or(false) {
            return Err(LabeledError::new(format!(
                "Session not found: {session_id}"
            )));
        }

        let view = rig
            .view(&id)
            .await
            .map_err(|e| LabeledError::new(format!("Failed to read session: {e}")))?;

        // Collect the stored messages
        let messages: Vec<&Message> = view.messages.iter().collect();

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
                Message::Assistant(assistant) => {
                    let text = assistant
                        .content
                        .iter()
                        .map(|c| match c {
                            AssistantContent::Text(t) => t.text.clone(),
                            AssistantContent::ToolCall(tc) => format!(
                                "Tool call: {} - {}",
                                tc.function.name,
                                serde_json::to_string(&tc.function.arguments).unwrap_or_default()
                            ),
                            AssistantContent::Reasoning(r) => {
                                format!("[Reasoning: {r:?}]")
                            }
                            AssistantContent::Image(_) => "[Image]".to_string(),
                            AssistantContent::Opaque(_) => "[Opaque]".to_string(),
                            _ => "[Unknown]".to_string(),
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

        // Convert the resolved compaction policy to a Nushell Value (record).
        // The policy is not persisted in the store format: it is the resolved
        // policy the runtime compacts with, built from the loaded config.
        let policy = build_compaction_policy(compaction);
        let mut config_record = Record::new();
        config_record.push(
            "keep_recent",
            Value::int(policy.keep_recent as i64, call.head),
        );
        config_record.push(
            "keep_asked",
            Value::int(policy.keep_asked as i64, call.head),
        );
        config_record.push(
            "reserve",
            Value::int(policy.summarizer.limits.reserve as i64, call.head),
        );
        config_record.push(
            "summary_tokens",
            Value::int(policy.summarizer.limits.summary_tokens as i64, call.head),
        );
        config_record.push(
            "keep_tokens",
            Value::int(policy.clearing.keep_tokens as i64, call.head),
        );
        config_record.push(
            "keep_under",
            Value::int(policy.clearing.keep_under as i64, call.head),
        );

        // Build the final session record
        let mut session_record = Record::new();
        session_record.push("id", Value::string(id, call.head));
        session_record.push(
            "created_at",
            Value::string(
                view.started_ms
                    .and_then(|ms| chrono::DateTime::from_timestamp_millis(ms as i64))
                    .unwrap_or_else(chrono::Utc::now)
                    .to_rfc3339(),
                call.head,
            ),
        );
        session_record.push(
            "title",
            Value::string(view.title.unwrap_or_default(), call.head),
        );
        session_record.push(
            "model",
            Value::string(view.model.unwrap_or_default(), call.head),
        );
        session_record.push(
            "message_count",
            Value::int(messages.len() as i64, call.head),
        );
        session_record.push("config", Value::record(config_record, call.head));
        session_record.push(
            "repairs",
            Value::list(
                view.repairs
                    .iter()
                    .map(|repair| Value::string(repair.to_string(), call.head))
                    .collect(),
                call.head,
            ),
        );
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
        let compaction = agnt_runtime::config::toml_config::load()
            .unwrap_or_default()
            .compaction
            .unwrap_or_default();
        crate::block_on!(plugin, self.run_inner(engine, call, &rig, &compaction))
    }
}
