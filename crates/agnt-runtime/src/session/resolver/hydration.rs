//! The hydration read: the entries a session's transcript is, and the snapshots
//! a surface replays from them.

// region:    --- Modules

use std::collections::HashMap;
use std::path::Path;

use rig::conversation::{Author, EntryRef};

use crate::protocol::contracts::UiMessageSnapshot;
use crate::session::resolver::record::result_text;
use crate::session::tool_status::{ToolStatus, ToolStatusMap};
use crate::types::{AssistantContent, CallId, Message, UserContent};
use agnt_storage::TurnFacts;

// endregion: --- Modules

// region:    --- Types

/// One persisted conversation entry as hydration reads it: the message, and
/// who wrote it when the store records an author.
///
/// The author is what tells a compaction summary apart from the user's own
/// text: rig-memory splices its summary in as a user message by
/// [`rig_memory::summary_author`].
#[derive(Debug, Clone)]
pub struct StoredEntry {
    /// The persisted message.
    pub message: Message,
    /// Who wrote it, when the store records an author.
    pub author: Option<Author>,
    /// The turn that persisted it, `None` when no settled turn holds its key.
    ///
    /// The link is by rig's entry key, never by position: a compaction folds
    /// the log (the kept entries keep their keys) and a cancelled turn settles
    /// no turn row, so its delta stays unattributed instead of being guessed.
    pub turn_seq: Option<i64>,
}

// endregion: --- Types

// region:    --- Froms

impl From<Message> for StoredEntry {
    fn from(message: Message) -> Self {
        Self {
            message,
            author: None,
            turn_seq: None,
        }
    }
}

impl From<EntryRef<'_>> for StoredEntry {
    fn from(entry: EntryRef<'_>) -> Self {
        Self {
            message: entry.message.clone(),
            author: entry.author.cloned(),
            turn_seq: None,
        }
    }
}

// endregion: --- Froms

// region:    --- Conversion

/// Converts rig store entries into UiMessageSnapshots for TUI transcript
/// hydration.
///
/// Each entry is delegated to `hydrate_single_message`. A compaction summary
/// hydrates as a `"compaction"` snapshot carrying its raw text, so the replayed
/// log renders the same block the live compaction emits.
///
/// # Arguments
/// * `entries` - The conversation's entries, as rig reads them: message plus
///   the author the store recorded
/// * `cwd` - Unused by the projection: hydration reconstructs no gate preview, so
///   a replayed row carries no producer-only content and nothing reads the
///   filesystem root. The card and the completion body a row shows are derived
///   from the call and the result the store recorded.
/// * `statuses` - The structural status of every call the session's store
///   carries, by call
///
/// # Returns
/// The `UiMessageSnapshot`s the transcript hydrates from, in store order
pub fn hydrate_transcript_messages(
    entries: &[StoredEntry],
    _cwd: &Path,
    statuses: &ToolStatusMap,
    turns: &[TurnFacts],
) -> Vec<UiMessageSnapshot> {
    // Pass 1: collect call_id → tool_name and call_id → arguments from all
    //         ToolCalls, call_id → result text from all ToolResults, and
    //         call_id → success from all ToolResults. Success is the inverse of
    //         the persisted `ToolResult::is_error` flag.
    let mut tool_names: HashMap<CallId, String> = HashMap::new();
    let mut tool_arguments: HashMap<CallId, String> = HashMap::new();
    let mut tool_results: HashMap<CallId, String> = HashMap::new();
    let mut tool_success_map: HashMap<CallId, bool> = HashMap::new();
    for entry in entries {
        match &entry.message {
            Message::Assistant(assistant) => {
                let content = &assistant.content;
                for item in content.iter() {
                    if let AssistantContent::ToolCall(tc) = item {
                        tool_names.insert(tc.id.clone(), tc.function.name.to_string());
                        tool_arguments.insert(
                            tc.id.clone(),
                            serde_json::to_string(&tc.function.arguments)
                                .unwrap_or_else(|_| "{}".to_string()),
                        );
                    }
                }
            }
            Message::User { content } => {
                for item in content.iter() {
                    if let UserContent::ToolResult(tr) = item {
                        tool_success_map.insert(tr.call.clone(), !tr.is_error);
                        if let Some(text) = result_text(tr) {
                            tool_results.insert(tr.call.clone(), text);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    // Pass 1b: resolve each result's status into the map. A result whose call id
    //          has no matching ToolCall gets no entry — the same guard the
    //          ToolResult branch applies. The status is the one the store
    //          carries; a row with no recorded status keeps the stored
    //          `is_error` verdict. No display is derived here: the surface
    //          derives every row from the snapshot it is handed, exactly as the
    //          live turn does.
    let mut call_statuses: ToolStatusMap = HashMap::new();
    for entry in entries {
        let Message::User { content } = &entry.message else {
            continue;
        };
        for item in content.iter() {
            let UserContent::ToolResult(tr) = item else {
                continue;
            };
            let Some(_tool_name) = tool_names.get(&tr.call) else {
                continue;
            };
            let status = match statuses.get(&tr.call.to_string()).copied() {
                Some(status) => status,
                None if tr.is_error => ToolStatus::Failed,
                None => ToolStatus::Success,
            };
            call_statuses.insert(tr.call.to_string(), status);
        }
    }
    // Pass 2: generate snapshots. A snapshot carries the call and its result as
    // data; the surface derives the display, exactly as it does for a live turn.
    let turns_by_seq: HashMap<i64, &TurnFacts> =
        turns.iter().map(|turn| (turn.turn_seq, turn)).collect();
    entries
        .iter()
        .flat_map(|entry| {
            hydrate_single_message(
                entry,
                &tool_results,
                &tool_success_map,
                &call_statuses,
                &turns_by_seq,
            )
        })
        .collect()
}

/// Converts a single rig store entry into `UiMessageSnapshot`s.
///
/// The function maps rig messages into snapshots — the call and its result as
/// data, the status typed by the map — and derives no display:
/// - `Message::User { content }` with `UserContent::Text` → a user snapshot, or a compaction one for a summary
/// - `Message::User { content }` with `UserContent::ToolResult` → skipped; the
///   result rides its call's snapshot
/// - `Message::Assistant(AssistantMessage { content, .. })` with `AssistantContent::Text` → assistant text
/// - `Message::Assistant(AssistantMessage { content, .. })` with `AssistantContent::ToolCall` → the call as data
/// - `Message::System { content }` → a system snapshot
pub(crate) fn hydrate_single_message(
    entry: &StoredEntry,
    tool_results: &HashMap<CallId, String>,
    tool_success_map: &HashMap<CallId, bool>,
    call_statuses: &ToolStatusMap,
    turns: &HashMap<i64, &TurnFacts>,
) -> Vec<UiMessageSnapshot> {
    let mut snapshots = Vec::new();

    match &entry.message {
        Message::User { content } => {
            for item in content.iter() {
                // ToolResult, Image, etc. — skip
                if let UserContent::Text(text) = item {
                    match compaction_summary(&text.text, entry.author.as_ref()) {
                        Some(summary) => {
                            snapshots.push(UiMessageSnapshot::new("compaction", summary))
                        }
                        None => {
                            // The user row carries the facts its turn recorded:
                            // the true submit instant and the identities the
                            // turn ran under, verbatim. No turn, or a stamp the
                            // record cannot stand behind, carries nothing.
                            let (millis, model, agent) = entry_stamp(entry, turns);
                            snapshots.push(
                                UiMessageSnapshot::new("user", text.text.clone())
                                    .with_stamp(millis, model, agent),
                            );
                        }
                    }
                }
            }
        }
        Message::Assistant(assistant) => {
            let content = &assistant.content;
            for item in content.iter() {
                match item {
                    AssistantContent::Text(text) if !text.text.is_empty() => {
                        snapshots.push(UiMessageSnapshot::new("assistant", text.text.clone()));
                    }
                    AssistantContent::ToolCall(tool_call) => {
                        let args_json = serde_json::to_string(&tool_call.function.arguments)
                            .unwrap_or_else(|_| "{}".to_string());

                        // The snapshot carries the call, its arguments and the
                        // result the store recorded as data. The surface derives
                        // its call line, its card and its completion body from
                        // them, exactly as it does live.

                        // A call with no result at all carries no recorded
                        // status; the replay rule makes it a failed completion.
                        let status = call_statuses
                            .get(&tool_call.id.to_string())
                            .copied()
                            .unwrap_or(ToolStatus::Failed);
                        let snapshot = UiMessageSnapshot::new("tool", String::new())
                            .with_tool_name(tool_call.function.name.to_string())
                            .with_tool_details(
                                Some(args_json),
                                tool_results.get(&tool_call.id).cloned(),
                                tool_success_map.get(&tool_call.id).copied(),
                            )
                            .with_tool_status(status);
                        snapshots.push(snapshot);
                    }
                    _ => {} // Reasoning, Image, etc.
                }
            }
        }
        Message::System { content } => {
            snapshots.push(UiMessageSnapshot::new("system", content.clone()));
        }
    }

    snapshots
}

// endregion: --- Conversion

// region:    --- Support

/// The footer facts one entry carries: the submit instant and the identities
/// of the turn that persisted it, verbatim.
///
/// The link is the entry's recorded turn, resolved through the join. An entry no
/// turn holds, or a turn whose recorded instant cannot stand behind a footer
/// (`<= 0`), carries nothing at all — never a partial row, never a re-derived
/// instant. Each identity passes through [`identity`]: a real value verbatim, an
/// empty or whitespace-only record as absent.
pub(super) fn entry_stamp(
    entry: &StoredEntry,
    turns: &HashMap<i64, &TurnFacts>,
) -> (Option<i64>, Option<String>, Option<String>) {
    let Some(turn) = entry.turn_seq.and_then(|seq| turns.get(&seq)).copied() else {
        return (None, None, None);
    };
    if turn.submitted_millis <= 0 {
        return (None, None, None);
    }
    (
        Some(turn.submitted_millis),
        identity(&turn.model),
        identity(&turn.agent),
    )
}

/// An identity fact as the entry carries it.
///
/// The record writes the empty string where a turn had no identity (a
/// persona-less turn records `agent = ""`), so emptiness is unknown, not a
/// segment: an empty or whitespace-only value becomes `None`. A real value
/// passes verbatim — the gate tests emptiness and never trims the value.
fn identity(value: &str) -> Option<String> {
    (!value.trim().is_empty()).then(|| value.to_string())
}

/// The headline rig-memory puts above the summary it splices in.
const SUMMARY_HEADLINE: &str =
    "The conversation before this point was compacted into this summary:";

/// The raw summary `text` carries, when the entry is a compaction summary.
///
/// The entry author is the primary signal. A store that lost the author (an
/// older entry, written before it was recorded) is still recognised by the
/// wrapper shape rig-memory writes. Any other text is the user's own and
/// yields `None`.
pub(super) fn compaction_summary(text: &str, author: Option<&Author>) -> Option<String> {
    let wrapped = summary_body(text);
    if author == Some(&rig_memory::summary_author()) {
        return Some(wrapped.unwrap_or_else(|| text.to_string()));
    }
    wrapped
}

/// The summary rig-memory's wrapper holds: the text between its `<summary>`
/// line and the first `</summary>` line. The tracked sections that follow the
/// closing tag are the model's context, not the summary, so they are dropped.
///
/// `None` when `text` is not the wrapper: the headline must be the first line,
/// and the closing tag must follow the opening one.
fn summary_body(text: &str) -> Option<String> {
    let mut lines = text.split('\n');
    if lines.next() != Some(SUMMARY_HEADLINE) {
        return None;
    }
    let rest: Vec<&str> = lines.collect();
    let open = rest.iter().position(|line| *line == "<summary>")?;
    let close = rest.iter().position(|line| *line == "</summary>")?;
    if open >= close {
        return None;
    }
    Some(rest[open + 1..close].join("\n"))
}

// endregion: --- Support
