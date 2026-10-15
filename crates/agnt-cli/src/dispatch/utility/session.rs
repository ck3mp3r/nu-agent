//! `agent session` subcommand dispatch.

use std::path::PathBuf;

use agnt_core::compaction::CompactionParams;
use agnt_core::session::prefix::{
    dir_prefix, dir_prefix_legacy, filter_sessions_by_cwd, match_prefixs,
};
use agnt_core::session::{SessionStore, SessionStoreBackend, StoreEntry, create_store};
use agnt_core::types::{AssistantContent, Message, UserContent};

use crate::AgentArgs;
use crate::SessionCommand;
use crate::SessionIdArgs;
use crate::error::{Error, Result};

use super::super::config::resolve_store_type_from_clap;
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
    let store = open_store(args).await?;

    let all_sessions = store
        .list()
        .await
        .map_err(|e| Error::Custom(format!("Failed to list sessions: {e}")))?;
    let sessions = filter_sessions_by_cwd(all_sessions, &cwd);

    if sessions.is_empty() {
        println!("No sessions found for {}.", cwd.display());
        return Ok(());
    }

    let new_prefix = dir_prefix(&cwd);
    let legacy_prefix = dir_prefix_legacy(&cwd);

    let rows: Vec<Vec<String>> = sessions
        .iter()
        .map(|info| {
            let display_id = match_prefixs(&info.id, &new_prefix, &legacy_prefix)
                .unwrap_or(&info.id)
                .to_string();
            vec![
                display_id,
                info.message_count.to_string(),
                info.last_active.to_rfc3339(),
            ]
        })
        .collect();

    print_table(&["ID", "MESSAGES", "LAST ACTIVE"], &rows);
    Ok(())
}

/// Delete a session by ID.
async fn run_session_clear(args: &AgentArgs, id: &SessionIdArgs) -> Result<()> {
    let cwd = current_dir()?;
    let store = open_store(args).await?;

    let new_prefix = dir_prefix(&cwd);
    let legacy_prefix = dir_prefix_legacy(&cwd);
    let new_id = format!("{new_prefix}-{}", id.id);
    let legacy_id = format!("{legacy_prefix}-{}", id.id);

    // Determine which prefixed ID exists, trying the new prefix first and
    // falling back to the legacy prefix for sessions created before the
    // prefix length increase.
    let new_exists = store
        .load(&new_id)
        .await
        .map_err(|e| Error::Custom(format!("Failed to load session: {e}")))?
        .is_some();
    let target_id = if new_exists {
        new_id
    } else {
        let legacy_exists = store
            .load(&legacy_id)
            .await
            .map_err(|e| Error::Custom(format!("Failed to load session: {e}")))?
            .is_some();
        if legacy_exists {
            legacy_id
        } else {
            return Err(Error::Custom(format!("Session not found: {}", id.id)));
        }
    };

    store
        .delete(&target_id)
        .await
        .map_err(|e| Error::Custom(format!("Failed to delete session: {e}")))?;

    println!("Deleted session '{}'", id.id);
    Ok(())
}

/// Display full details of a specific session.
async fn run_session_inspect(args: &AgentArgs, id: &SessionIdArgs) -> Result<()> {
    let cwd = current_dir()?;
    let store = open_store(args).await?;

    let new_prefix = dir_prefix(&cwd);
    let legacy_prefix = dir_prefix_legacy(&cwd);
    let new_id = format!("{new_prefix}-{}", id.id);
    let legacy_id = format!("{legacy_prefix}-{}", id.id);

    let loaded = store
        .load(&new_id)
        .await
        .map_err(|e| Error::Custom(format!("Failed to load session: {e}")))?;
    let (metadata, entries) = match loaded {
        Some(loaded) => loaded,
        None => store
            .load(&legacy_id)
            .await
            .map_err(|e| Error::Custom(format!("Failed to load session: {e}")))?
            .ok_or_else(|| Error::Custom(format!("Session not found: {}", id.id)))?,
    };

    let messages: Vec<&Message> = entries
        .iter()
        .filter_map(|entry| match entry {
            StoreEntry::Message(msg) => Some(msg),
            StoreEntry::Marker(_) => None,
        })
        .collect();

    // Compaction params are not persisted in the store format; display defaults.
    let config = CompactionParams::default();

    println!("Session: {}", metadata.session_id);
    println!("Created: {}", metadata.created_at.to_rfc3339());
    println!("Messages: {}", messages.len());
    println!(
        "Compaction strategy: {}",
        config.compaction_strategy.as_str()
    );

    for msg in &messages {
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

async fn open_store(args: &AgentArgs) -> Result<SessionStoreBackend> {
    let store_type = resolve_store_type_from_clap(args)?;
    create_store(store_type)
        .await
        .map_err(|e| Error::Custom(format!("Failed to create session store: {e}")))
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
