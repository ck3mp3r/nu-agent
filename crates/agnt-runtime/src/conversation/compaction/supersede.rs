//! The between-runs splice: the patched call's reply becomes the conversation's
//! summary, written after pass one's save and before pass two's load.
//!
//! rig's runner loads the conversation before each run and saves it once the run
//! succeeds, so a splice made during a run is wiped by that run's own save. The
//! only window in which a summary can land is between the two runs: the store
//! then holds pass one's exchange, and pass two loads what this module wrote.

use std::sync::Arc;

use rig::conversation::Conversation;
use rig::message::{AssistantContent, CallId, Message, UserContent};
use rig_memory::summary_author;

use agnt_kernel::bus::LifecycleEvent;
use agnt_kernel::{CompactionRecord, SessionRecord};

use crate::bus::{LifecycleTx, Stream};
use crate::hook::chain::Supersede;

// region:    --- Types

/// What the splice did with the reply.
#[derive(Debug)]
pub(crate) enum Landed {
    /// The summary is in the conversation and the store holds it.
    Summary(String),
    /// The reply could not be a summary; its entry is removed and nothing was
    /// inserted.
    Rejected(String),
}

// endregion: --- Types

// region:    --- Functions

/// Land the patched call's reply in `store` under `log`: splice the summary in,
/// remove the reply entry, and save.
///
/// Every outcome is published on the record stream and the lifecycle spine, so a
/// surface sees the compaction finish even though it ran in the turn executor:
/// `Completed` when the summary landed, `Failed` when it could not.
///
/// A cancelled turn calls nothing here: the reply it never received cannot be
/// its summary.
pub(crate) async fn land(
    supersede: Supersede,
    store: Arc<dyn rig_memory::Store>,
    log: String,
    stream: &Stream,
    lifecycle: &LifecycleTx,
) -> Landed {
    let source = supersede.source();
    let spliced = crate::session::blocking::on_pool(move || splice(&*store, &log, supersede)).await;
    let landed = match spliced {
        Ok(Ok(landed)) => landed,
        Ok(Err(message)) => Landed::Rejected(message),
        Err(e) => Landed::Rejected(format!("Compaction splice error: {e}")),
    };
    match &landed {
        Landed::Summary(summary) => {
            let _ = stream.publish(SessionRecord::Compaction(CompactionRecord::Completed {
                source,
                summary: summary.clone(),
            }));
            let _ = lifecycle.publish(LifecycleEvent::CompactionCompleted {
                source,
                summary: summary.clone(),
            });
        }
        Landed::Rejected(message) => {
            let _ = stream.publish(SessionRecord::Compaction(CompactionRecord::Failed {
                source,
                message: message.clone(),
            }));
            let _ = lifecycle.publish(LifecycleEvent::CompactionFailed {
                source,
                message: message.clone(),
            });
        }
    }
    landed
}

// endregion: --- Functions

// region:    --- Support

/// Load the conversation, put the summary in place of the messages it replaced,
/// remove the reply entry, and save.
///
/// The reply entry is found by its own text at its newest position: the patched
/// call is the model's internal one, so its entry must not stay in the history
/// as an assistant turn. No summary is inserted when the reply was rejected.
fn splice(
    store: &dyn rig_memory::Store,
    log: &str,
    supersede: Supersede,
) -> Result<Landed, String> {
    let mut conversation =
        rig_memory::load(store, log).map_err(|e| format!("Compaction load error: {e}"))?;
    let reply = supersede.reply_text().to_owned();
    let landed = match supersede.summary() {
        Some(text) => {
            let mut state = supersede.state().clone();
            state.summary = text.to_owned();
            rig_memory::apply_summary(&mut conversation, supersede.keep_from(), &state)
                .ok_or_else(|| "Compaction summary could not be spliced in".to_string())?;
            Landed::Summary(text.to_owned())
        }
        None => {
            let why = supersede
                .rejection()
                .unwrap_or("the reply cannot be used as a summary")
                .to_owned();
            Landed::Rejected(format!("Compaction produced an unusable summary: {why}"))
        }
    };
    remove_reply(&mut conversation, &reply);
    rig_memory::save(store, log, &mut conversation)
        .map_err(|e| format!("Compaction save error: {e}"))?;
    Ok(landed)
}

/// Remove the reply entry: the model's answer to the patched call, which must
/// not stay in the history as an assistant turn, and the tool records its calls
/// own.
///
/// The entry is named by its own text at its newest position. A reply that
/// carried no text names its entry by the text it lacks: the newest entry whose
/// own text is empty. It never names a later entry that carries text, because
/// the model's answer ending the run is a genuine turn the next run reads. No
/// such entry means nothing is removed.
fn remove_reply(conversation: &mut Conversation, reply: &str) {
    let reply = reply.trim();
    let found = conversation
        .entries()
        .rev()
        .find(|entry| entry.author.is_none() && is_reply_entry(entry.message, reply))
        .map(|entry| (entry.index, reply_calls(entry.message)));
    let Some((at, calls)) = found else {
        return;
    };
    conversation.splice(at..at + 1, Vec::new(), summary_author());
    remove_paired_tool_records(conversation, at, &calls);
}

/// Remove the tool records the removed reply's calls own: the entries between
/// `from` and the next assistant entry that hold only results answering one of
/// `calls`.
///
/// The run's real answer ends the sweep: past it the next run's own exchange
/// begins, and a call answered there is not the removed reply's to take. An
/// entry holding text beside a result is never removed, because the shape does
/// not occur in the saved store: a tool result is its own user message.
pub(super) fn remove_paired_tool_records(
    conversation: &mut Conversation,
    from: usize,
    calls: &[CallId],
) {
    let removed = paired_tool_records(conversation, from, calls);
    for at in removed.into_iter().rev() {
        conversation.splice(at..at + 1, Vec::new(), summary_author());
    }
}

/// Where the tool records the removed reply's calls own are: the entries from
/// `from` up to the next assistant entry that hold only results answering one of
/// `calls`, in conversation order.
fn paired_tool_records(conversation: &Conversation, from: usize, calls: &[CallId]) -> Vec<usize> {
    if calls.is_empty() {
        return Vec::new();
    }
    conversation
        .entries()
        .skip(from)
        .take_while(|entry| !matches!(entry.message, Message::Assistant(_)))
        .filter(|entry| answers_only(entry.message, calls))
        .map(|entry| entry.index)
        .collect()
}

/// The call ids the reply entry `message` carries, in order.
pub(super) fn reply_calls(message: &Message) -> Vec<CallId> {
    let Message::Assistant(turn) = message else {
        return Vec::new();
    };
    turn.content
        .iter()
        .filter_map(|content| match content {
            AssistantContent::ToolCall(call) => Some(call.id.clone()),
            _ => None,
        })
        .collect()
}

/// Whether `message` is a user message holding only tool results that answer a
/// call in `calls`.
fn answers_only(message: &Message, calls: &[CallId]) -> bool {
    let Message::User { content } = message else {
        return false;
    };
    !content.is_empty()
        && content.iter().all(|item| match item {
            UserContent::ToolResult(result) => calls.contains(&result.call),
            _ => false,
        })
}

/// Whether `message` is the reply `reply` names: its own assistant text is the
/// reply's text when it carried one, and empty when it carried none.
pub(super) fn is_reply_entry(message: &Message, reply: &str) -> bool {
    let text = reply_text(message);
    let text = text.trim();
    match (reply.is_empty(), text.is_empty()) {
        (true, empty) => empty && matches!(message, Message::Assistant(_)),
        (false, empty) => !empty && text == reply,
    }
}

/// The text an assistant message carries, empty for a message that carries none.
fn reply_text(message: &Message) -> String {
    match message {
        Message::Assistant(turn) => turn
            .content
            .iter()
            .filter_map(|content| match content {
                AssistantContent::Text(text) => Some(text.text.as_str()),
                _ => None,
            })
            .collect(),
        _ => String::new(),
    }
}

// endregion: --- Support

// region:    --- Tests

#[cfg(test)]
#[path = "../../../test/conversation/compaction/supersede.rs"]
mod supersede_test;

// endregion: --- Tests
