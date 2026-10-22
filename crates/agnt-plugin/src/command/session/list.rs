use std::path::PathBuf;

use crate::plugin::AgentPlugin;
use agnt_core::session::RigStoreConfig;
use agnt_core::session::prefix::{dir_prefix, dir_prefix_legacy, match_prefixs};
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
        rig: &RigStoreConfig,
    ) -> Result<Value, LabeledError> {
        let cwd = PathBuf::from(engine.get_current_dir()?);
        let new_prefix = dir_prefix(&cwd);
        let legacy_prefix = dir_prefix_legacy(&cwd);

        let ids = rig
            .session_ids()
            .map_err(|e| LabeledError::new(format!("Failed to list sessions: {e}")))?;

        // Convert each session's `meta.json` listing to a Nushell record. A
        // session whose store cannot be opened is skipped: one unreadable log
        // must not hide the others.
        let mut session_values: Vec<Value> = Vec::new();
        for id in ids {
            let Some(display_id) = match_prefixs(&id, &new_prefix, &legacy_prefix) else {
                continue;
            };
            let Ok(meta) = rig.listing(&id) else {
                log::warn!("failed to read the session listing of '{id}'");
                continue;
            };
            let mut record = Record::new();
            record.push("id", Value::string(display_id, call.head));
            record.push("title", Value::string(meta.title, call.head));
            record.push("model", Value::string(meta.model, call.head));
            record.push("messages", Value::int(meta.messages as i64, call.head));
            record.push("updated", Value::int(meta.updated as i64, call.head));
            record.push(
                "labels",
                Value::record(
                    meta.labels
                        .into_iter()
                        .map(|(tag, label)| (tag, Value::string(label, call.head)))
                        .collect(),
                    call.head,
                ),
            );
            // Plugins' listed values are arbitrary JSON; the listing shows them
            // as JSON text rather than inventing a value mapping.
            record.push(
                "listed",
                Value::string(
                    serde_json::to_string(&meta.listed).unwrap_or_else(|_| "{}".to_string()),
                    call.head,
                ),
            );
            session_values.push(Value::record(record, call.head));
        }

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
        let rig = RigStoreConfig::new(store_type);
        crate::block_on!(plugin, self.run_inner(engine, call, &rig))
    }
}
