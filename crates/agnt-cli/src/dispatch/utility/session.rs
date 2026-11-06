//! `agent session` subcommand dispatch.

use std::path::PathBuf;

use agnt_core::conversation::runtime_build::build_compaction_policy;
use agnt_core::rig_memory::CompactionPolicy;
use agnt_core::session::RigStoreConfig;
use agnt_core::session::prefix::dir_prefix;
use agnt_core::types::{AssistantContent, Message, UserContent};
use agnt_runtime::config::CompactionConfig;

use crate::AgentArgs;
use crate::SessionCommand;
use crate::SessionIdArgs;
use crate::error::{Error, Result};

use super::super::config::{resolve_session_store_type_from_config, resolve_store_type_from_clap};
use super::table::print_table;

// region:    --- Functions

/// Dispatch an `agent session` subcommand.
pub async fn run_session_command(args: &AgentArgs, command: &SessionCommand) -> Result<()> {
    match command {
        SessionCommand::List => run_session_list(args).await,
        SessionCommand::Clear(id) => run_session_clear(args, id).await,
        SessionCommand::Inspect(id) => run_session_inspect(args, id).await,
    }
}

/// List all sessions for the current working directory.
async fn run_session_list(args: &AgentArgs) -> Result<()> {
    let cwd = current_dir()?;
    let rig = open_rig(args)?;

    let prefixed = format!("{}-", dir_prefix(&cwd));

    let ids = rig
        .session_ids()
        .await
        .map_err(|e| Error::Custom(format!("Failed to list sessions: {e}")))?;

    // A session whose store cannot be opened is skipped: one unreadable log
    // must not hide the others.
    let mut rows: Vec<Vec<String>> = Vec::new();
    for id in ids {
        let Some(display_id) = id.strip_prefix(&prefixed) else {
            continue;
        };
        let Ok(meta) = rig.listing(&id).await else {
            log::warn!("failed to read the session listing of '{id}'");
            continue;
        };
        rows.push(vec![
            display_id.to_string(),
            meta.title,
            meta.model,
            meta.messages.to_string(),
            meta.updated.to_string(),
        ]);
    }

    if rows.is_empty() {
        println!("No sessions found for {}.", cwd.display());
        return Ok(());
    }

    print_table(&["ID", "TITLE", "MODEL", "MESSAGES", "UPDATED"], &rows);
    Ok(())
}

/// Delete a session by ID.
async fn run_session_clear(args: &AgentArgs, id: &SessionIdArgs) -> Result<()> {
    let cwd = current_dir()?;
    let rig = open_rig(args)?;

    let prefix = dir_prefix(&cwd);
    let target_id = format!("{prefix}-{}", id.id);

    // The session must exist in the rig-memory store.
    if !rig.holds(&target_id).await.unwrap_or(false) {
        return Err(Error::Custom(format!("Session not found: {}", id.id)));
    }

    // Delete the session's rig-memory store: its logs and its files.
    rig.destroy(&target_id)
        .await
        .map_err(|e| Error::Custom(format!("Failed to delete session: {e}")))?;

    println!("Deleted session '{}'", id.id);
    Ok(())
}

/// Display full details of a specific session.
async fn run_session_inspect(args: &AgentArgs, id: &SessionIdArgs) -> Result<()> {
    let cwd = current_dir()?;
    let rig = open_rig(args)?;

    let prefix = dir_prefix(&cwd);
    let session_id = format!("{prefix}-{}", id.id);

    if !rig.holds(&session_id).await.unwrap_or(false) {
        return Err(Error::Custom(format!("Session not found: {}", id.id)));
    }
    let view = rig
        .view(&session_id)
        .await
        .map_err(|e| Error::Custom(format!("Failed to read session: {e}")))?;

    // The compaction policy is not persisted in the store format: display the
    // policy the runtime resolves from the loaded config.
    println!("Session: {session_id}");
    if let Some(ms) = view.started_ms
        && let Some(created) = chrono::DateTime::from_timestamp_millis(ms as i64)
    {
        println!("Created: {}", created.to_rfc3339());
    }
    println!("Title: {}", view.title.as_deref().unwrap_or(""));
    println!("Model: {}", view.model.as_deref().unwrap_or(""));
    println!("Messages: {}", view.messages.len());
    let compaction = load_compaction_config();
    for line in compaction_display(&build_compaction_policy(&compaction)) {
        println!("{line}");
    }
    for repair in &view.repairs {
        println!("Repair: {repair}");
    }

    for msg in &view.messages {
        let (role, content) = extract_message_info(msg);
        println!();
        println!("[{role}]");
        println!("{content}");
    }

    Ok(())
}

// endregion: --- Functions

// region:    --- Support

/// The compaction record lines for a resolved policy: the values the runtime
/// compacts with, not a hardcoded default.
fn compaction_display(policy: &CompactionPolicy) -> Vec<String> {
    vec![
        format!("Compaction keep_recent: {}", policy.keep_recent),
        format!("Compaction keep_asked: {}", policy.keep_asked),
        format!("Compaction reserve: {}", policy.summarizer.limits.reserve),
        format!(
            "Compaction summary_tokens: {}",
            policy.summarizer.limits.summary_tokens
        ),
        format!("Compaction keep_tokens: {}", policy.clearing.keep_tokens),
        format!("Compaction keep_under: {}", policy.clearing.keep_under),
    ]
}

/// The `[compaction]` block of the user's config, empty when it cannot be read.
fn load_compaction_config() -> CompactionConfig {
    agnt_runtime::config::toml_config::load()
        .unwrap_or_default()
        .compaction
        .unwrap_or_default()
}

fn current_dir() -> Result<PathBuf> {
    std::env::current_dir()
        .map_err(|e| Error::Custom(format!("Failed to resolve working directory: {e}")))
}

fn open_rig(args: &AgentArgs) -> Result<RigStoreConfig> {
    let store_type = resolve_store_type_from_clap(args, resolve_session_store_type_from_config())?;
    let config = agnt_runtime::config::toml_config::load().unwrap_or_default();
    let paths = agnt_runtime::config::Paths::resolve_for_process(config.paths.as_ref())
        .map_err(|e| Error::Custom(format!("Failed to resolve application paths: {e}")))?;
    Ok(RigStoreConfig::new(store_type).with_store_path(paths.store_path))
}

/// Extract the role and text content from a stored message.
fn extract_message_info(msg: &Message) -> (String, String) {
    match msg {
        Message::System { content } => ("system".to_string(), content.clone()),
        Message::User { content } => {
            let text = content
                .iter()
                .map(|c| match c {
                    UserContent::Text(t) => t.text.clone(),
                    UserContent::ToolResult(t) => format!("Tool result: {t:?}"),
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
                    AssistantContent::ToolCall(tc) => {
                        format!(
                            "Tool call: {} - {}",
                            tc.function.name,
                            serde_json::to_string(&tc.function.arguments).unwrap_or_default()
                        )
                    }
                    AssistantContent::Reasoning(r) => format!("[Reasoning: {r:?}]"),
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

// endregion: --- Support

// region:    --- Tests

#[cfg(test)]
#[path = "../../../test/dispatch/utility/session.rs"]
mod session_test;

// endregion: --- Tests
