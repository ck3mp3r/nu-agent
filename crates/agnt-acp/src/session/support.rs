//! Free-standing session helpers: config options, history replay, and the
//! stop-category mapping.

use agent_client_protocol::schema::v1::{
    ContentBlock, ContentChunk, SessionConfigKind, SessionConfigOption, SessionConfigSelectOptions,
    SessionUpdate, TextContent,
};
use agnt_runtime::config::ModelsCache;
use agnt_runtime::conversation::runtime::AgentConversationRuntime;
use agnt_runtime::conversation::turn::{TurnBudget, TurnEnd};
use agnt_runtime::session::management::SessionPersistence;
use agnt_runtime::types::{AssistantContent, Message, UserContent};

use crate::error::{AcpError, Result};
use crate::runtime::{build_agent_config_options, build_model_config_options};
use crate::translator::SessionUpdateSink;

use super::types::StopCategory;

/// The agent config options of a locked session runtime.
pub(super) fn agent_options(runtime: &AgentConversationRuntime) -> Vec<SessionConfigOption> {
    build_agent_config_options(
        runtime.agents_config(),
        runtime.cwd(),
        runtime.agents_dir(),
        runtime.agent_identity(),
    )
}

/// The model config options of a locked session runtime.
pub(super) fn model_options(
    runtime: &AgentConversationRuntime,
    models_cache: Option<&ModelsCache>,
) -> Vec<SessionConfigOption> {
    build_model_config_options(runtime.provider_config(), models_cache)
}

/// Replay the stored conversation of `session_id` to `sink`.
///
/// Only the conversational text is replayed: user turns become
/// `user_message_chunk` and assistant turns become `agent_message_chunk`.
/// Tool calls, tool results, and compaction summaries have no replay form and
/// are skipped. Returns the transport error when the sink is gone.
pub(super) fn replay_history<S: SessionUpdateSink>(
    sink: &S,
    session_id: &str,
    messages: &[Message],
) -> Result<()> {
    for message in messages {
        match message {
            Message::User { content } => {
                for item in content.iter() {
                    if let UserContent::Text(text) = item {
                        send_chunk(sink, session_id, text.text.clone(), true)?;
                    }
                }
            }
            Message::Assistant(assistant) => {
                for item in assistant.content.iter() {
                    if let AssistantContent::Text(text) = item {
                        send_chunk(sink, session_id, text.text.clone(), false)?;
                    }
                }
            }
            Message::System { .. } => {}
        }
    }
    Ok(())
}

/// Send one replayed message as a `session/update` notification.
fn send_chunk<S: SessionUpdateSink>(
    sink: &S,
    session_id: &str,
    text: String,
    from_user: bool,
) -> Result<()> {
    let chunk = ContentChunk::new(ContentBlock::Text(TextContent::new(text)));
    let update = if from_user {
        SessionUpdate::UserMessageChunk(chunk)
    } else {
        SessionUpdate::AgentMessageChunk(chunk)
    };
    sink.send_update(session_id, update)
        .map_err(|e| AcpError::Runtime(format!("failed to replay session '{session_id}': {e}")))
}

/// Every selectable value of the select options in `options`.
///
/// Non-select options and unknown option kinds contribute no values, so a
/// caller can validate a requested value against the advertised set.
pub(super) fn select_values(options: &[SessionConfigOption]) -> Vec<String> {
    options
        .iter()
        .flat_map(|option| match &option.kind {
            SessionConfigKind::Select(select) => match &select.options {
                SessionConfigSelectOptions::Ungrouped(values) => {
                    values.iter().map(|value| value.value.to_string()).collect()
                }
                SessionConfigSelectOptions::Grouped(groups) => groups
                    .iter()
                    .flat_map(|group| group.options.iter())
                    .map(|value| value.value.to_string())
                    .collect(),
                _ => Vec::new(),
            },
            _ => Vec::new(),
        })
        .collect()
}

/// The stop category a finished turn reports to the client.
///
/// The turn's end carries the executor's classification: a stop is cancelled, a
/// failure names the budget that ended it, and everything else completed. A
/// failed turn is logged before it is mapped, so a swallowed error is never
/// silent.
pub(super) fn stop_category(session_id: &str, end: &TurnEnd) -> StopCategory {
    match end {
        TurnEnd::Completed { .. } => StopCategory::Completed,
        TurnEnd::Cancelled { .. } => StopCategory::Cancelled,
        TurnEnd::Failed(failure) => {
            log::warn!(
                "session {session_id}: prompt turn failed: {}",
                failure.error.msg
            );
            match failure.budget {
                Some(TurnBudget::MaxTurns { .. }) => StopCategory::MaxTurns,
                Some(TurnBudget::MaxTokens) => StopCategory::MaxTokens,
                None => StopCategory::Completed,
            }
        }
    }
}
