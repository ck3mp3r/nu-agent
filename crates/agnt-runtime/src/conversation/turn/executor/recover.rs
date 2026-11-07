//! Recovery persistence for a turn that did not complete.
//!
//! rig saves the conversation only when a run completes; every other exit saves
//! nothing. `persist_turn_delta` writes the interrupted turn's delta to the
//! session's rig log, so a cancelled or failed turn keeps the prompt, the
//! committed tool exchanges and the partial assistant text it produced.
//!
//! Recovery never fails the turn: every failure is a `log::warn!` and a return.

use std::sync::Arc;

use crate::conversation::managers::SessionManager;
use crate::hook::turn_tools::TurnToolRecord;
use crate::protocol::events::{SystemEvent, SystemKind};
use crate::session::CachedMemory;
use crate::types::{
    AssistantContent, AssistantMessage, CallId, Message, ToolCall, ToolFunction, UserContent,
};

use super::core::TurnExecutor;

// region:    --- Types

/// What a stopped turn recovered beyond the delta its reported history carries.
#[derive(Debug, Default)]
pub(crate) struct TurnRecovery<'a> {
    /// Every call the turn dispatched, with the result it settled with.
    pub tool_records: &'a [TurnToolRecord],
    /// The text the turn streamed, when the caller carries it.
    pub streamed_text: Option<&'a str>,
}

// endregion: --- Types

impl<S> TurnExecutor<'_, S>
where
    S: SessionManager<
            Memory = crate::conversation::state::memory::MemoryOf,
            InnerMemory = CachedMemory,
        >,
{
    /// Append the delta of an interrupted turn to the session's rig log.
    ///
    /// `full_history` is the history the turn reported (rig's `chat_history` or
    /// the hook's snapshot). The delta is anchored by content: it starts at the
    /// LAST user message whose text is `prompt` or ends with `"\n\n{prompt}"`
    /// (the steering format), because the turn's prompt is always present in
    /// the reported history while the pre-turn count can be one ahead of the
    /// run's own load.
    ///
    /// rig's cancellation repair merges adjacent user messages, so the anchored
    /// message can carry the store's last result row. A result the store
    /// already answers is dropped from it: saving it again would hold one
    /// call's result twice.
    ///
    /// `recovery` holds what the turn recovered beyond its reported history:
    /// every call it dispatched with the result it settled with, and the text
    /// it streamed. A call the delta does not carry is reconstructed as one
    /// assistant message holding the recorded calls, closed by the recorded
    /// results (`[interrupted]` for a call that never settled). Only the part
    /// of the streamed text the delta's committed assistant text does not
    /// already hold is appended, so a cancelled turn keeps its partial answer
    /// without duplicating it.
    ///
    /// When the prompt is absent from `full_history` and nothing else is
    /// recovered, `[user(prompt), assistant(empty_delta_reply)]` is persisted: a
    /// turn stopped before any LLM call would otherwise lose the request
    /// entirely.
    ///
    /// The store is loaded as the save base, because a save extends a loaded
    /// conversation. A log whose length moved since the turn started is left
    /// alone: slicing it would mis-attribute another writer's messages. That
    /// skip is user-visible: the turn's work is lost, so the skip publishes a
    /// `SystemEvent::SystemMessage` alongside the log line.
    pub(crate) async fn persist_turn_delta(
        &self,
        session_id: Option<&str>,
        prompt: &str,
        full_history: &[Message],
        pre_turn_message_count: usize,
        empty_delta_reply: &str,
        recovery: TurnRecovery<'_>,
    ) {
        let Some(session_id) = session_id else {
            return;
        };

        let store = match self.memory_state.rig_store(session_id).await {
            Ok(store) => store,
            Err(error) => {
                log::warn!("Failed to open the session store for recovery: {error}");
                return;
            }
        };

        let loaded = crate::session::blocking::on_pool({
            let store = Arc::clone(&store);
            let session_id = session_id.to_owned();
            move || rig_memory::load(&*store, &session_id)
        })
        .await;
        let mut conversation = match loaded {
            Ok(Ok(conversation)) => conversation,
            Ok(Err(error)) => {
                log::warn!("Failed to load the session for recovery: {error}");
                return;
            }
            Err(error) => {
                log::warn!("Failed to load the session for recovery: {error}");
                return;
            }
        };

        let loaded_len = conversation.messages().len();
        if loaded_len != pre_turn_message_count {
            log::warn!(
                "Skipping recovery: the session moved mid-turn (log holds {loaded_len} messages, \
                 the turn started from {pre_turn_message_count})"
            );
            let _ = self
                .tool_infra
                .bus
                .ui_event()
                .send(SystemEvent::SystemMessage {
                    kind: SystemKind::Warning,
                    text: "Turn stopped: the session log moved mid-turn; this turn's work was \
                              not saved"
                        .to_string(),
                })
                .await;
            return;
        }

        let stored = answered_call_ids(conversation.messages().iter().map(|message| &**message));
        let anchor = prompt_anchor(full_history, prompt);
        let mut delta: Vec<Message> = match anchor {
            Some(index) => full_history[index..].to_vec(),
            None => {
                log::warn!("persist: the turn's prompt was not found in the reported history");
                Vec::new()
            }
        };
        delta = drop_stored_results(delta, &stored);

        let missing_calls = missing_tool_calls(&delta, recovery.tool_records);
        if !missing_calls.is_empty() {
            if anchor.is_none() {
                delta.insert(0, Message::user(prompt));
            }
            delta.push(Message::Assistant(AssistantMessage::new(missing_calls)));
        }

        if anchor.is_none() && delta.is_empty() {
            delta = vec![Message::user(prompt), Message::assistant(empty_delta_reply)];
        }

        let settled = settled_results(recovery.tool_records);
        let delta = crate::session::repair::close_unanswered_tool_calls_with(delta, &settled);
        let mut delta = crate::session::repair::replace_placeholder_results_with(delta, &settled);

        if let Some(text) = recovery.streamed_text {
            let committed: String = delta
                .iter()
                .filter_map(|message| match message {
                    Message::Assistant(assistant) => Some(assistant.text()),
                    _ => None,
                })
                .collect();
            match text.strip_prefix(&committed) {
                Some(remainder) if !remainder.is_empty() => {
                    delta.push(Message::assistant(remainder));
                }
                _ => log::debug!(
                    "persist: the streamed text does not extend the committed assistant text"
                ),
            }
        }

        // The keys the log already held: the delta's keys are what recovery adds.
        let loaded_keys = crate::session::settle::entry_keys(conversation.entries());
        conversation.extend(delta);

        let saved = crate::session::blocking::on_pool({
            let store = Arc::clone(&store);
            let session_id = session_id.to_owned();
            let mut conversation = conversation;
            move || {
                let saved = rig_memory::save(&*store, &session_id, &mut conversation);
                // The post-save read-back names the entries the recovery persisted.
                let keys = if saved.is_ok() {
                    rig_memory::load(&*store, &session_id)
                        .map(|saved| crate::session::settle::entry_keys(saved.entries()))
                        .unwrap_or_default()
                } else {
                    Vec::new()
                };
                (saved, keys)
            }
        })
        .await;
        let keys = match saved {
            Ok((Ok(()), keys)) => keys,
            Ok((Err(error), _)) => {
                log::warn!("Failed to save the interrupted turn: {error}");
                Vec::new()
            }
            Err(error) => {
                log::warn!("Failed to save the interrupted turn: {error}");
                Vec::new()
            }
        };

        self.persist_tool_facts(session_id, &keys, &loaded_keys, recovery.tool_records)
            .await;
    }

    /// Record the facts of the turn `persist_turn_delta` just closed.
    ///
    /// The closed turn is a settled turn like any other: its row, the keys of
    /// the entries recovery persisted and the status every call settled with
    /// land in one transaction. The submission instant is the persisted
    /// prompt's own made-time; nothing here is derived from order or invented.
    pub(crate) async fn persist_tool_facts(
        &self,
        session_id: &str,
        saved: &[agnt_storage::MessageKey],
        loaded: &[agnt_storage::MessageKey],
        records: &[TurnToolRecord],
    ) {
        if records.is_empty() {
            return;
        }
        let facts = match self.memory_state.inner_memory().facts().await {
            Ok(facts) => facts,
            Err(error) => {
                log::warn!("Failed to open the session's fact tables: {error}");
                return;
            }
        };
        // Both sides of the difference are known here: the log as recovery
        // loaded it, and the log as the read-back found it.
        let keys = crate::session::settle::new_keys(saved, Some(loaded));
        let submitted_millis = keys
            .as_deref()
            .and_then(|keys| keys.first())
            .map_or_else(crate::session::settle::now_millis, |key| key.made);
        let input = crate::session::settle::SettleInput {
            session_id,
            model: &format!("{}/{}", self.config.provider, self.config.model),
            agent: &self.agent,
            submitted_millis,
            keys,
            records,
        };
        if let Err(error) = facts.settle(input).await {
            log::warn!("Failed to record the interrupted turn's facts: {error}");
        }
    }
}

// region:    --- Support

/// The index of the LAST user message in `full_history` that carries `prompt`.
///
/// The turn's prompt is always present in the reported history: rig's
/// `CommittedLog[0]` holds it, and the hook's snapshot appends it. A steering
/// retry runs with `"{feedback}\n\n{prompt}"`, so a message ending with
/// `"\n\n{prompt}"` counts as the prompt too.
///
/// A message's text is its text blocks concatenated, as rig's repair merges
/// adjacent user messages by extending their content. A merged message that
/// only holds the prompt among other blocks is not the prompt message.
fn prompt_anchor(full_history: &[Message], prompt: &str) -> Option<usize> {
    let steering_suffix = format!("\n\n{prompt}");
    full_history.iter().rposition(|message| {
        let Message::User { content } = message else {
            return false;
        };
        let text: String = content
            .iter()
            .filter_map(|item| match item {
                UserContent::Text(text) => Some(text.text.as_str()),
                _ => None,
            })
            .collect();
        text == prompt || text.ends_with(&steering_suffix)
    })
}

/// Drops from the delta's leading user message every tool result the store
/// already answers.
///
/// rig's cancellation repair merges adjacent user messages, so the message the
/// delta is anchored on carries the store's last result row. Saving that row
/// again would hold one call's result twice, and a provider that rejects a
/// repeated tool-result id fails the first turn after a reload.
fn drop_stored_results(
    mut delta: Vec<Message>,
    stored: &std::collections::HashSet<CallId>,
) -> Vec<Message> {
    if stored.is_empty() {
        return delta;
    }
    let Some(Message::User { content }) = delta.first_mut() else {
        return delta;
    };
    let before = content.len();
    content.retain(|item| match item {
        UserContent::ToolResult(result) => !stored.contains(&result.call),
        _ => true,
    });
    let dropped = before - content.len();
    let empty = content.is_empty();
    if dropped == 0 {
        return delta;
    }
    log::debug!("persist: dropped {dropped} stored result(s) from the anchored message");
    if empty {
        delta.remove(0);
    }
    delta
}

/// Every call id a tool result in `messages` answers.
fn answered_call_ids<'a>(
    messages: impl IntoIterator<Item = &'a Message>,
) -> std::collections::HashSet<CallId> {
    messages
        .into_iter()
        .filter_map(|message| match message {
            Message::User { content } => Some(content),
            _ => None,
        })
        .flatten()
        .filter_map(|item| match item {
            UserContent::ToolResult(result) => Some(result.call.clone()),
            _ => None,
        })
        .collect()
}

/// The assistant tool-call blocks for every record whose call id `delta` does
/// not carry.
fn missing_tool_calls(delta: &[Message], tool_records: &[TurnToolRecord]) -> Vec<AssistantContent> {
    let present = delta_call_ids(delta);
    tool_records
        .iter()
        .filter(|record| !present.contains(&record.call_id))
        .map(|record| {
            AssistantContent::ToolCall(ToolCall::new(
                record.call_id.clone(),
                ToolFunction::parse(record.name.clone(), &record.arguments),
            ))
        })
        .collect()
}

/// Every call id `delta` already carries, as a call or as a result.
fn delta_call_ids(delta: &[Message]) -> std::collections::HashSet<CallId> {
    let mut present = std::collections::HashSet::new();
    for message in delta {
        match message {
            Message::Assistant(assistant) => {
                present.extend(assistant.tool_calls().map(|call| call.id.clone()));
            }
            Message::User { content } => {
                present.extend(content.iter().filter_map(|item| match item {
                    UserContent::ToolResult(result) => Some(result.call.clone()),
                    _ => None,
                }));
            }
            Message::System { .. } => {}
        }
    }
    present
}

/// The `(call id, result text, is_error)` of every record that settled.
fn settled_results(tool_records: &[TurnToolRecord]) -> Vec<(CallId, String, bool)> {
    tool_records
        .iter()
        .filter_map(|record| {
            let result = record.result.clone()?;
            Some((
                record.call_id.clone(),
                result,
                record.success == Some(false),
            ))
        })
        .collect()
}

// endregion: --- Support
