//! `agent session` subcommand dispatch.

use std::path::PathBuf;

use agnt_core::compaction::CompactionParams;
use agnt_core::session::RigStoreConfig;
use agnt_core::session::prefix::{dir_prefix, dir_prefix_legacy, match_prefixs};
use agnt_core::types::{AssistantContent, Message, UserContent};

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

    let new_prefix = dir_prefix(&cwd);
    let legacy_prefix = dir_prefix_legacy(&cwd);

    let ids = rig
        .session_ids()
        .map_err(|e| Error::Custom(format!("Failed to list sessions: {e}")))?;

    // A session whose store cannot be opened is skipped: one unreadable log
    // must not hide the others.
    let mut rows: Vec<Vec<String>> = Vec::new();
    for id in ids {
        let Some(display_id) = match_prefixs(&id, &new_prefix, &legacy_prefix) else {
            continue;
        };
        let Ok(meta) = rig.listing(&id) else {
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

    let new_prefix = dir_prefix(&cwd);
    let legacy_prefix = dir_prefix_legacy(&cwd);
    let new_id = format!("{new_prefix}-{}", id.id);
    let legacy_id = format!("{legacy_prefix}-{}", id.id);

    // Determine which prefixed ID exists, trying the new prefix first and
    // falling back to the legacy prefix for sessions created before the
    // prefix length increase.
    let target_id = [new_id, legacy_id]
        .into_iter()
        .find(|id| rig.holds(id).unwrap_or(false))
        .ok_or_else(|| Error::Custom(format!("Session not found: {}", id.id)))?;

    // Delete the session's rig-memory store: its logs and its files.
    rig.destroy(&target_id)
        .map_err(|e| Error::Custom(format!("Failed to delete session: {e}")))?;

    println!("Deleted session '{}'", id.id);
    Ok(())
}

/// Display full details of a specific session.
async fn run_session_inspect(args: &AgentArgs, id: &SessionIdArgs) -> Result<()> {
    let cwd = current_dir()?;
    let rig = open_rig(args)?;

    let new_prefix = dir_prefix(&cwd);
    let legacy_prefix = dir_prefix_legacy(&cwd);
    let new_id = format!("{new_prefix}-{}", id.id);
    let legacy_id = format!("{legacy_prefix}-{}", id.id);

    let (session_id, view) = [new_id, legacy_id]
        .into_iter()
        .find(|id| rig.holds(id).unwrap_or(false))
        .map(|id| {
            let view = rig.view(&id);
            (id, view)
        })
        .ok_or_else(|| Error::Custom(format!("Session not found: {}", id.id)))?;
    let view = view.map_err(|e| Error::Custom(format!("Failed to read session: {e}")))?;

    // Compaction params are not persisted in the store format; display defaults.
    let config = CompactionParams::default();

    println!("Session: {session_id}");
    if let Some(ms) = view.started_ms
        && let Some(created) = chrono::DateTime::from_timestamp_millis(ms as i64)
    {
        println!("Created: {}", created.to_rfc3339());
    }
    println!("Title: {}", view.title.as_deref().unwrap_or(""));
    println!("Model: {}", view.model.as_deref().unwrap_or(""));
    println!("Messages: {}", view.messages.len());
    println!(
        "Compaction strategy: {}",
        config.compaction_strategy.as_str()
    );
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

fn current_dir() -> Result<PathBuf> {
    std::env::current_dir()
        .map_err(|e| Error::Custom(format!("Failed to resolve working directory: {e}")))
}

fn open_rig(args: &AgentArgs) -> Result<RigStoreConfig> {
    let store_type = resolve_store_type_from_clap(args, resolve_session_store_type_from_config())?;
    Ok(RigStoreConfig::new(store_type))
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
