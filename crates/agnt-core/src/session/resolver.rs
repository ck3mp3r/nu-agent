use nu_protocol::LabeledError;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rig::conversation::{Author, EntryRef};

use crate::protocol::contracts::UiMessageSnapshot;
use crate::types::{AssistantContent, CallId, Message, UserContent};
use agnt_runtime::session::settle::{FactStore, SessionFacts};
use agnt_runtime::session::tool_status::{ToolStatus, ToolStatusMap};
use agnt_runtime::session::{RigStoreConfig, Session};
use agnt_storage::TurnFacts;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionRequest {
    None,
    Attach(String),
    Create(String),
}

#[derive(Debug, Clone)]
pub struct SessionResolutionInput {
    pub use_tui: bool,
    pub session_id: Option<String>,
    pub cwd: PathBuf,
}

#[derive(Debug, Clone)]
pub struct SessionResolution {
    pub final_session_id: Option<String>,
    pub session: Option<Session>,
    pub should_hydrate_transcript: bool,
    pub initial_messages: Vec<UiMessageSnapshot>,
    pub last_total_tokens: Option<u64>,
}

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

pub trait SessionResolver {
    fn resolve(
        &self,
        input: SessionResolutionInput,
    ) -> impl std::future::Future<Output = Result<SessionResolution, LabeledError>> + Send;
}

pub struct DefaultSessionResolver {
    rig: RigStoreConfig,
}

impl DefaultSessionResolver {
    pub fn new(rig: RigStoreConfig) -> Self {
        Self { rig }
    }
}

impl SessionResolver for DefaultSessionResolver {
    async fn resolve(
        &self,
        input: SessionResolutionInput,
    ) -> Result<SessionResolution, LabeledError> {
        let prefix = agnt_runtime::session::prefix::dir_prefix(&input.cwd);
        let request = resolve_session_request(input.use_tui, input.session_id);
        let request = match request {
            SessionRequest::Attach(id) => SessionRequest::Attach(format!("{prefix}-{id}")),
            SessionRequest::Create(id) => SessionRequest::Create(format!("{prefix}-{id}")),
            SessionRequest::None => SessionRequest::None,
        };
        match request {
            SessionRequest::None => Ok(SessionResolution {
                final_session_id: None,
                session: None,
                should_hydrate_transcript: false,
                initial_messages: Vec::new(),
                last_total_tokens: None,
            }),
            SessionRequest::Attach(id) => {
                let store = self.open(&id).await?;
                let existed_before_attach = holds(&store, &id).await?;
                // Token count is not estimated here — rig's CompactingMemory
                // tracks the context and last_total_tokens stays None on attach.
                let initial_messages = if existed_before_attach {
                    hydrate_transcript(&self.rig, &store, &id, &input.cwd).await?
                } else {
                    Vec::new()
                };

                Ok(SessionResolution {
                    final_session_id: Some(id.clone()),
                    session: Some(Session::new(id)),
                    should_hydrate_transcript: !initial_messages.is_empty(),
                    initial_messages,
                    last_total_tokens: None,
                })
            }
            SessionRequest::Create(id) => Ok(SessionResolution {
                final_session_id: Some(id.clone()),
                session: Some(Session::new(id)),
                should_hydrate_transcript: false,
                initial_messages: Vec::new(),
                last_total_tokens: None,
            }),
        }
    }
}

impl DefaultSessionResolver {
    /// The rig-memory store of `session_id`.
    async fn open(&self, session_id: &str) -> Result<Arc<dyn rig_memory::Store>, LabeledError> {
        self.rig
            .open(session_id)
            .await
            .map_err(|e| LabeledError::new(format!("Failed to open session '{session_id}': {e}")))
    }
}

/// Whether `store` holds a log for `session_id`.
async fn holds(store: &Arc<dyn rig_memory::Store>, session_id: &str) -> Result<bool, LabeledError> {
    let store = Arc::clone(store);
    let session = session_id.to_owned();
    let logs = agnt_runtime::session::blocking::on_pool(move || store.logs())
        .await
        .map_err(|e| LabeledError::new(format!("Failed to list session logs '{session}': {e}")))?
        .map_err(|e| LabeledError::new(format!("Failed to list session logs '{session}': {e}")))?;
    Ok(logs.iter().any(|log| log == session_id))
}

/// The transcript of `session_id`, read from the rig store, with the facts the
/// settle path recorded: each entry's turn, and each call's status.
async fn hydrate_transcript(
    rig: &RigStoreConfig,
    store: &Arc<dyn rig_memory::Store>,
    session_id: &str,
    cwd: &Path,
) -> Result<Vec<UiMessageSnapshot>, LabeledError> {
    let (entries, statuses, turns) = read_entries(rig, store, session_id).await?;
    Ok(hydrate_transcript_messages(
        &entries, cwd, &statuses, &turns,
    ))
}

/// The entries of `session_id`, each carrying the turn that persisted it, the
/// statuses its calls settled with, and the session's settled turns.
///
/// The link is a key lookup against the attribution table, never a pairing by
/// position: an entry no settled turn holds reads `None`.
///
/// # Errors
///
/// When the session's log cannot be read.
pub(crate) async fn read_entries(
    rig: &RigStoreConfig,
    store: &Arc<dyn rig_memory::Store>,
    session_id: &str,
) -> Result<(Vec<StoredEntry>, ToolStatusMap, Vec<TurnFacts>), LabeledError> {
    let store = Arc::clone(store);
    let session = session_id.to_owned();
    let read =
        agnt_runtime::session::blocking::on_pool(move || rig_memory::load(&*store, &session)).await;
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
    let facts = read_facts(rig, session_id).await?;
    let turns = facts.entry_turns();
    let entries: Vec<StoredEntry> = conversation
        .entries()
        .map(|entry| {
            let key = agnt_runtime::session::settle::entry_keys([entry]);
            StoredEntry {
                message: entry.message.clone(),
                author: entry.author.cloned(),
                turn_seq: key.first().and_then(|key| turns.get(key)).copied(),
            }
        })
        .collect();
    Ok((entries, facts.status_map(), facts.turns))
}

/// The facts of `session_id`, or none when the database cannot be read.
///
/// A reload that cannot read the facts still hydrates its transcript: every
/// row then reads its stored verdict and no entry carries a turn.
async fn read_facts(rig: &RigStoreConfig, session_id: &str) -> Result<SessionFacts, LabeledError> {
    let facts = match FactStore::open(rig).await {
        Ok(facts) => facts,
        Err(error) => {
            log::warn!("failed to open the session's fact tables: {error}");
            return Ok(SessionFacts::default());
        }
    };
    match facts.read(session_id).await {
        Ok(facts) => Ok(facts),
        Err(error) => {
            log::warn!("failed to read the session's facts: {error}");
            Ok(SessionFacts::default())
        }
    }
}

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

pub fn generate_session_id() -> String {
    use chrono::Utc;
    let now = Utc::now();
    format!(
        "{}-{}",
        now.format("%Y%m%d-%H%M%S"),
        now.timestamp_subsec_micros()
    )
}

pub fn resolve_session_request(use_tui: bool, session_id: Option<String>) -> SessionRequest {
    match (use_tui, session_id) {
        // TUI explicit session policy is centralized here:
        // always attempt to attach first (resolver handles load-then-create fallback).
        (true, Some(id)) => SessionRequest::Attach(id),
        (_, Some(id)) => SessionRequest::Create(id),
        (true, None) => SessionRequest::Create(generate_session_id()),
        (false, None) => SessionRequest::None,
    }
}

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
/// * `cwd` - Working directory the session was executed in; used to
///   reconstruct tool previews that depend on the filesystem root
/// * `statuses` - The structural status of every call the session's store
///   carries, by call
///
/// # Returns
/// Iterator of UiMessageSnapshot ready for transcript hydration
pub fn hydrate_transcript_messages(
    entries: &[StoredEntry],
    _cwd: &Path,
    statuses: &ToolStatusMap,
    turns: &[TurnFacts],
) -> Vec<UiMessageSnapshot> {
    // Pass 1: collect call_id → tool_name and call_id → arguments from all
    //         ToolCalls, and call_id → success from all ToolResults. Success
    //         is the inverse of the persisted `ToolResult::is_error` flag.
    let mut tool_names: HashMap<CallId, String> = HashMap::new();
    let mut tool_arguments: HashMap<CallId, String> = HashMap::new();
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
                    }
                }
            }
            _ => {}
        }
    }
    // Pass 1b: resolve each result's status and derive its display. A result
    //          whose call id has no matching ToolCall yields no display — the
    //          same guard the ToolResult branch used to apply. The status is the
    //          one the store carries; a row with no recorded status keeps the
    //          stored `is_error` verdict. A cancelled
    //          call carries the ⊘ mark and the args preview of what the turn
    //          would have run, the same preview the live permission gate shows.
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
            hydrate_single_message(entry, &tool_success_map, &call_statuses, &turns_by_seq)
        })
        .collect()
}

/// Converts a single rig store entry into UiMessageSnapshots.
///
/// This function maps rig message types to transcript display items:
/// - `Message::User { content }` with `UserContent::Text` → user transcript item,
///   or a compaction item when the entry is a compaction summary
/// - `Message::User { content }` with `UserContent::ToolResult` → skipped (not shown in TUI)
/// - `Message::Assistant(AssistantMessage { content, .. })` with `AssistantContent::Text` → assistant text
/// - `Message::Assistant(AssistantMessage { content, .. })` with `AssistantContent::ToolCall` → tool call display,
///   followed by the call's own `tool_display` snapshot when its result yielded one
/// - `Message::System { content }` → system/compaction summary display
pub(crate) fn hydrate_single_message(
    entry: &StoredEntry,
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

                        // The snapshot carries the call as data. The surface derives
                        // its call line and its sections, exactly as it does live.

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
                                None,
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

// region:    --- Support

/// The footer facts one entry carries: the submit instant and the identities
/// of the turn that persisted it, verbatim.
///
/// The link is the entry's recorded turn, resolved through the join. An entry no
/// turn holds, or a turn whose recorded instant cannot stand behind a footer
/// (`<= 0`), carries nothing at all — never a partial row, never a re-derived
/// instant. Each identity passes through [`identity`]: a real value verbatim, an
/// empty or whitespace-only record as absent.
fn entry_stamp(
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
fn compaction_summary(text: &str, author: Option<&Author>) -> Option<String> {
    let wrapped = summary_body(text);
    if author == Some(&rig_memory::summary_author()) {
        return Some(wrapped.unwrap_or_else(|| text.to_string()));
    }
    wrapped
}

/// The summary rig-memory's wrapper holds: the text between its `<summary>
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
