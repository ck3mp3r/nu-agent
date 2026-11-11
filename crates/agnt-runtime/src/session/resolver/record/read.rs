//! The read: the store's rows as the entries the hydration consumes.

// region:    --- Modules

use std::sync::Arc;

use nu_protocol::LabeledError;

use crate::session::RigStoreConfig;
use crate::session::settle::SessionFacts;
use crate::session::tool_status::ToolStatusMap;
use crate::types::{
    AssistantContent, AssistantMessage, CallId, Message, Text, ToolCall, ToolFunction, ToolName,
    ToolResult, ToolResultContent, UserContent,
};
use agnt_storage::{TranscriptRecord, TranscriptRow, TurnFacts};

use super::fold::{fold_entries, record_transcript};
use super::store::{open_store, read_facts, read_transcript};
use crate::session::resolver::StoredEntry;

// endregion: --- Modules

// region:    --- Functions

/// The entries of `session_id`, each carrying the turn that persisted it, the
/// statuses its calls settled with, and the session's settled turns.
///
/// The read is the store's transcript read: the session's log is folded and
/// written as the store's rows, and the entries the hydration consumes are built
/// from those rows. A store that refuses the write leaves no rows, and the fold's
/// own entries answer instead: a refused record never costs a transcript.
///
/// # Errors
///
/// When the session's log cannot be read.
pub(crate) async fn read_entries(
    rig: &RigStoreConfig,
    store: &Arc<dyn rig_memory::Store>,
    session_id: &str,
) -> Result<(Vec<StoredEntry>, ToolStatusMap, Vec<TurnFacts>), LabeledError> {
    let facts = read_facts(rig, session_id).await?;
    let entries = fold_entries(store, session_id, &facts).await?;
    let storage = match open_store(rig).await {
        Ok(storage) => Some(storage),
        Err(error) => {
            log::warn!("failed to open the session's fact tables: {error}");
            None
        }
    };
    let records = match &storage {
        None => Vec::new(),
        Some(storage) => {
            record_transcript(Arc::clone(storage), session_id, &entries, &facts).await;
            match read_transcript(Arc::clone(storage), session_id).await {
                Ok(records) => records,
                Err(error) => {
                    log::warn!("failed to read the transcript of '{session_id}': {error}");
                    Vec::new()
                }
            }
        }
    };
    if records.is_empty() {
        return Ok((entries, facts.status_map(), facts.turns));
    }
    entries_from_records(&records, facts)
}

/// The entries the store's rows describe, with the statuses and turns the
/// hydration reads.
///
/// A carrier row — a media block, a reasoning block, a provider item — produces
/// no entry: the live fold draws nothing from it, so neither does the read. A
/// call's row produces the call entry and, when the call settled with a result,
/// the entry that answers it, so the pair reconstructs exactly what the fold
/// paired.
fn entries_from_records(
    records: &[TranscriptRecord],
    facts: SessionFacts,
) -> Result<(Vec<StoredEntry>, ToolStatusMap, Vec<TurnFacts>), LabeledError> {
    // The row's recorded status is what the fold wrote; the fact table answers
    // for a call no row carries. A row therefore wins over a fact that was lost.
    let mut statuses = facts.status_map();
    let mut entries = Vec::with_capacity(records.len());
    for record in records {
        let turn_seq = record.turn_seq();
        match record.row() {
            TranscriptRow::User { content, .. } => entries.push(StoredEntry {
                message: Message::User {
                    content: vec![UserContent::Text(Text::new(content))],
                },
                author: None,
                turn_seq,
            }),
            TranscriptRow::Compaction { content } => entries.push(StoredEntry {
                message: Message::User {
                    content: vec![UserContent::Text(Text::new(content))],
                },
                author: Some(rig_memory::summary_author()),
                turn_seq,
            }),
            TranscriptRow::Assistant { content } => entries.push(StoredEntry {
                message: Message::Assistant(AssistantMessage::new(vec![AssistantContent::Text(
                    Text::new(content),
                )])),
                author: None,
                turn_seq,
            }),
            TranscriptRow::System { content } => entries.push(StoredEntry {
                message: Message::System {
                    content: content.clone(),
                },
                author: None,
                turn_seq,
            }),
            TranscriptRow::Tool {
                call_id,
                name,
                executed_name,
                arguments,
                result,
                success,
                status,
            } => {
                let call = CallId::from_wire(call_id);
                entries.push(StoredEntry {
                    message: Message::Assistant(AssistantMessage::new(vec![
                        AssistantContent::ToolCall(ToolCall::new(
                            call.clone(),
                            ToolFunction::new(
                                tool_name(name)?,
                                arguments
                                    .as_deref()
                                    .and_then(|stored| serde_json::from_str(stored).ok())
                                    .unwrap_or_default(),
                            ),
                        )),
                    ])),
                    author: None,
                    turn_seq,
                });
                if let Some(status) = status {
                    statuses.insert(
                        call_id.clone(),
                        crate::session::settle::tool_status(*status),
                    );
                }
                let Some(answered) = success else {
                    continue;
                };
                let answered_name = executed_name.as_deref().unwrap_or(name);
                entries.push(StoredEntry {
                    message: Message::User {
                        content: vec![UserContent::ToolResult(ToolResult {
                            call,
                            name: tool_name(answered_name)?,
                            content: match result {
                                Some(text) => vec![ToolResultContent::Text(Text::new(text))],
                                None => Vec::new(),
                            },
                            is_error: !answered,
                        })],
                    },
                    author: None,
                    turn_seq,
                });
            }
            TranscriptRow::Media { .. }
            | TranscriptRow::Reasoning { .. }
            | TranscriptRow::Opaque { .. } => {}
        }
    }
    Ok((entries, statuses, turns_of(records, facts.turns)))
}

/// The turns the hydration reads: the settled turns, plus one derived from a
/// row's own stamp wherever the fact table no longer holds the turn it names.
///
/// The row is the record; a lost turn row must not blank a footer the record
/// kept. The derived turn carries the stamp's submit instant and no settle
/// instant — nothing the hydration reads asks for one.
fn turns_of(records: &[TranscriptRecord], mut turns: Vec<TurnFacts>) -> Vec<TurnFacts> {
    for record in records {
        let TranscriptRow::User {
            stamp: Some(stamp), ..
        } = record.row()
        else {
            continue;
        };
        let Some(turn_seq) = record.turn_seq() else {
            continue;
        };
        if turns.iter().any(|turn| turn.turn_seq == turn_seq) {
            continue;
        }
        turns.push(TurnFacts {
            session_id: record.session().to_owned(),
            turn_seq,
            submitted_millis: stamp.millis,
            settled_millis: 0,
            model: stamp.model.clone().unwrap_or_default(),
            agent: stamp.agent.clone().unwrap_or_default(),
        });
    }
    turns.sort_by_key(|turn| turn.turn_seq);
    turns
}

/// The tool name `name` is, or a refusal when it names no tool.
fn tool_name(name: &str) -> Result<ToolName, LabeledError> {
    ToolName::new(name)
        .map_err(|_| LabeledError::new(format!("the transcript names no tool: '{name}' is empty")))
}

// endregion: --- Functions
