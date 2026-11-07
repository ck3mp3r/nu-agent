//! The fold: a session's entries written as the store's transcript rows.

// region:    --- Modules

use std::collections::HashMap;
use std::sync::Arc;

use nu_protocol::LabeledError;

use crate::session::settle::SessionFacts;
use crate::session::tool_status::ToolStatusMap;
use crate::types::{
    AssistantContent, CallId, Document, DocumentData, DocumentSourceKind, Image, ImageDetail,
    Message, MimeType, Text, ToolCall, UserContent,
};
use agnt_storage::{
    Entries, MediaKind, MediaOwner, MediaSource, Storage, TranscriptRecord, TranscriptRow,
    TranscriptStamp, TurnFacts,
};

use super::super::StoredEntry;
use super::super::hydration::{compaction_summary, entry_stamp};

// endregion: --- Modules

// region:    --- Fold

/// The entries `session_id`'s log folds to, each carrying the turn that
/// persisted it by rig's key.
///
/// The link is a key lookup against the attribution table, never a pairing by
/// position: an entry no settled turn holds reads `None`.
///
/// # Errors
///
/// When the session's log cannot be read.
pub(super) async fn fold_entries(
    store: &Arc<dyn rig_memory::Store>,
    session_id: &str,
    facts: &SessionFacts,
) -> Result<Vec<StoredEntry>, LabeledError> {
    let store = Arc::clone(store);
    let session = session_id.to_owned();
    let read = crate::session::blocking::on_pool(move || rig_memory::load(&*store, &session)).await;
    let conversation = match read {
        Ok(loaded) => loaded.map_err(|e| {
            LabeledError::new(format!("Failed to load session '{session_id}': {e}"))
        })?,
        Err(e) => {
            return Err(LabeledError::new(format!(
                "Failed to load session '{session_id}': {e}"
            )));
        }
    };
    let turns = facts.entry_turns();
    Ok(conversation
        .entries()
        .map(|entry| {
            let key = crate::session::settle::entry_keys([entry]);
            StoredEntry {
                message: entry.message.clone(),
                author: entry.author.cloned(),
                turn_seq: key.first().and_then(|key| turns.get(key)).copied(),
            }
        })
        .collect())
}

/// Writes the fold's entries as the session's transcript rows.
///
/// A refusal is logged and never propagated: the entries the fold produced are
/// already the transcript the caller hydrates.
pub(super) async fn record_transcript(
    storage: Arc<Storage>,
    session_id: &str,
    entries: &[StoredEntry],
    facts: &SessionFacts,
) {
    let records = transcript_records(session_id, entries, &facts.status_map(), &facts.turns);
    if let Err(error) = Entries::new(storage).record_transcript(records).await {
        log::warn!("failed to record the transcript of '{session_id}': {error}");
    }
}

/// The transcript rows `entries` fold to, in fold order.
///
/// One row per block: a user text, an assistant text, a system text, one per
/// call, and one per block that is not text. The row's position is that order,
/// so a folded log renumbers its rows from the first entry it keeps. Every row
/// carries the turn the entry that produced it was persisted under.
fn transcript_records(
    session_id: &str,
    entries: &[StoredEntry],
    statuses: &ToolStatusMap,
    turns: &[TurnFacts],
) -> Vec<TranscriptRecord> {
    let turns_by_seq: HashMap<i64, &TurnFacts> =
        turns.iter().map(|turn| (turn.turn_seq, turn)).collect();
    let outcomes = call_outcomes(entries);
    let mut records = Vec::new();
    for entry in entries {
        for row in entry_rows(entry, &outcomes, statuses, &turns_by_seq) {
            let row_seq = records.len() as i64;
            records.push(
                TranscriptRecord::new(session_id, row_seq, row).with_turn_seq(entry.turn_seq),
            );
        }
    }
    records
}

/// The rows one entry folds to, in the order its content carries.
///
/// A call's result lands in the entry that answers it, so the call's row is
/// written where the call is and reads the result the entries after it carry.
/// An empty answer block is written too: the record keeps what the entries
/// carry, and the hydration drops the block the live fold drops.
fn entry_rows(
    entry: &StoredEntry,
    outcomes: &HashMap<CallId, CallOutcome>,
    statuses: &ToolStatusMap,
    turns: &HashMap<i64, &TurnFacts>,
) -> Vec<TranscriptRow> {
    match &entry.message {
        Message::User { content } => content
            .iter()
            .flat_map(|item| match item {
                UserContent::Text(text) => vec![user_row(entry, text, turns)],
                UserContent::ToolResult(result) => {
                    result_media_rows(MediaOwner::ToolResult, result)
                }
                UserContent::Image(image) => vec![image_row(MediaOwner::User, image)],
                UserContent::Audio(audio) => vec![TranscriptRow::Media {
                    owner: MediaOwner::User,
                    kind: MediaKind::Audio,
                    source: media_source(&audio.data),
                    text: None,
                    media_type: audio
                        .media_type
                        .as_ref()
                        .map(|kind| kind.to_mime_type().to_owned()),
                    detail: None,
                }],
                UserContent::Video(video) => vec![TranscriptRow::Media {
                    owner: MediaOwner::User,
                    kind: MediaKind::Video,
                    source: media_source(&video.data),
                    text: None,
                    media_type: video
                        .media_type
                        .as_ref()
                        .map(|kind| kind.to_mime_type().to_owned()),
                    detail: None,
                }],
                UserContent::Document(document) => vec![document_row(MediaOwner::User, document)],
            })
            .collect(),
        Message::Assistant(assistant) => assistant
            .content
            .iter()
            .flat_map(|item| match item {
                AssistantContent::Text(text) => vec![TranscriptRow::Assistant {
                    content: text.text.clone(),
                }],
                AssistantContent::ToolCall(call) => vec![tool_row(call, outcomes, statuses)],
                AssistantContent::Reasoning(reasoning) => vec![TranscriptRow::Reasoning {
                    text: reasoning.text.clone(),
                    redacted: reasoning.redacted,
                }],
                AssistantContent::Image(image) => vec![image_row(MediaOwner::Assistant, image)],
                AssistantContent::Opaque(opaque) => vec![TranscriptRow::Opaque {
                    item: opaque.item.to_string(),
                    replay: opaque.replay,
                }],
                _ => Vec::new(),
            })
            .collect(),
        Message::System { content } => vec![TranscriptRow::System {
            content: content.clone(),
        }],
    }
}

/// The row `text` folds to: the summary a compaction left, or the user's own
/// text with the footer facts its settled turn recorded.
fn user_row(entry: &StoredEntry, text: &Text, turns: &HashMap<i64, &TurnFacts>) -> TranscriptRow {
    if let Some(summary) = compaction_summary(&text.text, entry.author.as_ref()) {
        return TranscriptRow::Compaction { content: summary };
    }
    let (millis, model, agent) = entry_stamp(entry, turns);
    let stamp = millis.map(|millis| TranscriptStamp {
        millis,
        model,
        agent,
    });
    TranscriptRow::User {
        content: text.text.clone(),
        stamp,
    }
}

/// The row one call folds to: the call as the model asked it, with the outcome
/// the entries recorded.
///
/// A status exists only for a call the settle path attributed; a call no turn
/// settled carries none rather than a derived guess.
fn tool_row(
    call: &ToolCall,
    outcomes: &HashMap<CallId, CallOutcome>,
    statuses: &ToolStatusMap,
) -> TranscriptRow {
    let outcome = outcomes.get(&call.id);
    TranscriptRow::Tool {
        call_id: call.id.to_string(),
        name: call.function.name.to_string(),
        executed_name: outcome.map(|outcome| outcome.name.clone()),
        arguments: Some(
            serde_json::to_string(&call.function.arguments).unwrap_or_else(|_| "{}".to_string()),
        ),
        result: outcome.and_then(|outcome| outcome.result.clone()),
        success: outcome.and_then(|outcome| outcome.success),
        status: statuses
            .get(&call.id.to_string())
            .copied()
            .map(crate::session::settle::call_status),
    }
}

/// The media row an image block folds to.
fn image_row(owner: MediaOwner, image: &Image) -> TranscriptRow {
    TranscriptRow::Media {
        owner,
        kind: MediaKind::Image,
        source: media_source(&image.data),
        text: None,
        media_type: image
            .media_type
            .as_ref()
            .map(|kind| kind.to_mime_type().to_owned()),
        detail: image
            .detail
            .as_ref()
            .map(|detail| detail_token(detail).to_owned()),
    }
}

/// The media row a document block folds to: its own text when it carries text,
/// and where its file's bytes are when it carries a file.
fn document_row(owner: MediaOwner, document: &Document) -> TranscriptRow {
    let (source, text) = match &document.data {
        DocumentData::Text(text) => (MediaSource::Unknown, Some(text.clone())),
        DocumentData::File(source) => (media_source(source), None),
    };
    TranscriptRow::Media {
        owner,
        kind: MediaKind::Document,
        source,
        text,
        media_type: document
            .media_type
            .as_ref()
            .map(|kind| kind.to_mime_type().to_owned()),
        detail: None,
    }
}

/// The media rows a tool result's parts fold to.
fn result_media_rows(owner: MediaOwner, result: &crate::types::ToolResult) -> Vec<TranscriptRow> {
    result
        .content
        .iter()
        .filter_map(|part| match part {
            crate::types::ToolResultContent::Image(image) => Some(image_row(owner, image)),
            crate::types::ToolResultContent::Text(_)
            | crate::types::ToolResultContent::Json { .. } => None,
        })
        .collect()
}

/// Where a block's bytes are, as the record keeps them.
fn media_source(data: &DocumentSourceKind) -> MediaSource {
    match data {
        DocumentSourceKind::Url(url) => MediaSource::Url(url.clone()),
        DocumentSourceKind::Base64(encoded) => MediaSource::Base64(encoded.clone()),
        DocumentSourceKind::FileId(id) => MediaSource::FileId(id.clone()),
        DocumentSourceKind::Raw(bytes) => MediaSource::Raw(bytes.clone()),
        DocumentSourceKind::Unknown => MediaSource::Unknown,
    }
}

/// The preference an image block named, verbatim.
fn detail_token(detail: &ImageDetail) -> &'static str {
    match detail {
        ImageDetail::Low => "low",
        ImageDetail::High => "high",
        ImageDetail::Auto => "auto",
    }
}

/// The result half of each call the entries carry, by call id.
fn call_outcomes(entries: &[StoredEntry]) -> HashMap<CallId, CallOutcome> {
    let mut outcomes = HashMap::new();
    for entry in entries {
        let Message::User { content } = &entry.message else {
            continue;
        };
        for item in content.iter() {
            let UserContent::ToolResult(result) = item else {
                continue;
            };
            outcomes.insert(
                result.call.clone(),
                CallOutcome {
                    name: result.name.to_string(),
                    result: result_text(result),
                    success: Some(!result.is_error),
                },
            );
        }
    }
    outcomes
}

/// The text a tool result carries: its parts joined in order, a structured part
/// as its JSON. An image part carries no text, so it contributes none.
fn result_text(result: &crate::types::ToolResult) -> Option<String> {
    let parts: Vec<String> = result
        .content
        .iter()
        .filter_map(|part| match part {
            crate::types::ToolResultContent::Text(text) => Some(text.text.clone()),
            crate::types::ToolResultContent::Json { value } => Some(value.to_string()),
            _ => None,
        })
        .collect();
    (!parts.is_empty()).then(|| parts.join("\n"))
}

// endregion: --- Fold

// region:    --- Support

/// The result half one call's entry carries: the name it answered with, the
/// text it produced, and the verdict it carried.
struct CallOutcome {
    name: String,
    result: Option<String>,
    success: Option<bool>,
}

// endregion: --- Support
