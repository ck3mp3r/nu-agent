use nu_protocol::LabeledError;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::protocol::contracts::UiMessageSnapshot;
use crate::protocol::event::ToolDisplay;
use crate::session::tool_status::{ToolStatus, ToolStatusMap, read_statuses};
use crate::session::{RigStoreConfig, Session};
use crate::types::{AssistantContent, CallId, Message, ToolResultContent, UserContent};
use std::collections::HashMap;

use crate::tools::handler::tool_display_from_arguments;
use crate::tools::handler::tool_display_from_result;

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
        let new_prefix = crate::session::prefix::dir_prefix(&input.cwd);
        let legacy_prefix = crate::session::prefix::dir_prefix_legacy(&input.cwd);
        let request = resolve_session_request(input.use_tui, input.session_id);
        let request = match request {
            SessionRequest::Attach(id) => {
                let new_id = format!("{new_prefix}-{id}");
                let legacy_id = format!("{legacy_prefix}-{id}");
                // Try the new prefix first, then fall back to the legacy prefix
                // for sessions created before the prefix length increase.
                let resolved = if self.holds(&new_id)? {
                    new_id
                } else if self.holds(&legacy_id)? {
                    legacy_id
                } else {
                    new_id
                };
                SessionRequest::Attach(resolved)
            }
            SessionRequest::Create(id) => SessionRequest::Create(format!("{new_prefix}-{id}")),
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
                let store = self.open(&id)?;
                let existed_before_attach = holds(&store, &id)?;
                // Token count is not estimated here — rig's CompactingMemory
                // tracks the context and last_total_tokens stays None on attach.
                let initial_messages = if existed_before_attach {
                    hydrate_transcript(&store, &id, &input.cwd)?
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
    fn open(&self, session_id: &str) -> Result<Arc<dyn rig_memory::Store>, LabeledError> {
        self.rig
            .open(session_id)
            .map_err(|e| LabeledError::new(format!("Failed to open session '{session_id}': {e}")))
    }

    /// Whether the rig store holds `session_id`.
    fn holds(&self, session_id: &str) -> Result<bool, LabeledError> {
        holds(&self.open(session_id)?, session_id)
    }
}

/// Whether `store` holds a log for `session_id`.
fn holds(store: &Arc<dyn rig_memory::Store>, session_id: &str) -> Result<bool, LabeledError> {
    let logs = store.logs().map_err(|e| {
        LabeledError::new(format!("Failed to list session logs '{session_id}': {e}"))
    })?;
    Ok(logs.iter().any(|log| log == session_id))
}

/// The transcript of `session_id`, read from the rig store.
fn hydrate_transcript(
    store: &Arc<dyn rig_memory::Store>,
    session_id: &str,
    cwd: &Path,
) -> Result<Vec<UiMessageSnapshot>, LabeledError> {
    let conversation = rig_memory::load(&**store, session_id)
        .map_err(|e| LabeledError::new(format!("Failed to load session '{session_id}': {e}")))?;
    let messages: Vec<Message> = conversation
        .entries()
        .map(|entry| entry.message.clone())
        .collect();
    let statuses = read_statuses(&**store);
    Ok(hydrate_transcript_messages(&messages, cwd, &statuses))
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

/// Converts rig messages into UiMessageSnapshots for TUI transcript hydration.
///
/// Each message is delegated to `hydrate_single_message`. Compaction summaries
/// live in the rig-memory log, not in the session store.
///
/// # Arguments
/// * `messages` - The conversation's messages, as rig reads them
/// * `cwd` - Working directory the session was executed in; used to
///   reconstruct tool previews that depend on the filesystem root
/// * `statuses` - The structural status of every call the session's store
///   carries, by call
///
/// # Returns
/// Iterator of UiMessageSnapshot ready for transcript hydration
pub fn hydrate_transcript_messages(
    messages: &[Message],
    cwd: &Path,
    statuses: &ToolStatusMap,
) -> Vec<UiMessageSnapshot> {
    // Pass 1: collect call_id → tool_name and call_id → arguments from all
    //         ToolCalls, and call_id → success from all ToolResults. Success
    //         is the inverse of the persisted `ToolResult::is_error` flag.
    let mut tool_names: HashMap<CallId, String> = HashMap::new();
    let mut tool_arguments: HashMap<CallId, String> = HashMap::new();
    let mut tool_success_map: HashMap<CallId, bool> = HashMap::new();
    for message in messages {
        match message {
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
    //          stored `is_error` verdict (the legacy disposition). A cancelled
    //          call carries the ⊘ mark and the args preview of what the turn
    //          would have run, the same preview the live permission gate shows.
    let mut tool_displays: HashMap<CallId, ToolDisplay> = HashMap::new();
    let mut call_statuses: ToolStatusMap = HashMap::new();
    for message in messages {
        let Message::User { content } = message else {
            continue;
        };
        for item in content.iter() {
            let UserContent::ToolResult(tr) = item else {
                continue;
            };
            let Some(tool_name) = tool_names.get(&tr.call) else {
                continue;
            };
            let status = match statuses.get(&tr.call).copied() {
                Some(status) => status,
                None if tr.is_error => ToolStatus::Failed,
                None => ToolStatus::Success,
            };
            call_statuses.insert(tr.call.clone(), status);
            match status {
                ToolStatus::Success | ToolStatus::Failed => {
                    let result_text: String = tr
                        .content
                        .iter()
                        .filter_map(|c| match c {
                            ToolResultContent::Text(t) => Some(t.text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    let Ok(json) = serde_json::from_str::<serde_json::Value>(&result_text) else {
                        continue;
                    };
                    // Result-sourced display wins (an embedded `display` object, or
                    // the edit diff). When the result carries none, fall back to the
                    // argument-sourced preview the live path shows at the permission
                    // gate — this is what makes a `nu` call render its command code
                    // block after rehydration.
                    let display = tool_display_from_result(tool_name, &json).or_else(|| {
                        tool_arguments.get(&tr.call).and_then(|args_json| {
                            tool_display_from_arguments(tool_name, args_json, cwd)
                        })
                    });
                    if let Some(display) = display {
                        tool_displays.insert(tr.call.clone(), display);
                    }
                }
                ToolStatus::Cancelled => {
                    if let Some(display) = tool_arguments.get(&tr.call).and_then(|args_json| {
                        tool_display_from_arguments(tool_name, args_json, cwd)
                    }) {
                        tool_displays.insert(tr.call.clone(), display);
                    }
                }
            }
        }
    }
    // Pass 2: generate snapshots with tool display reconstruction
    messages
        .iter()
        .flat_map(|message| {
            hydrate_single_message(message, &tool_success_map, &tool_displays, &call_statuses)
        })
        .collect()
}

/// Converts a single rig Message into UiMessageSnapshots.
///
/// This function maps rig message types to transcript display items:
/// - `Message::User { content }` with `UserContent::Text` → user transcript item
/// - `Message::User { content }` with `UserContent::ToolResult` → skipped (not shown in TUI)
/// - `Message::Assistant(AssistantMessage { content, .. })` with `AssistantContent::Text` → assistant text
/// - `Message::Assistant(AssistantMessage { content, .. })` with `AssistantContent::ToolCall` → tool call display,
///   followed by the call's own `tool_display` snapshot when its result yielded one
/// - `Message::System { content }` → system/compaction summary display
pub(crate) fn hydrate_single_message(
    msg: &Message,
    tool_success_map: &HashMap<CallId, bool>,
    tool_displays: &HashMap<CallId, ToolDisplay>,
    call_statuses: &ToolStatusMap,
) -> Vec<UiMessageSnapshot> {
    let mut snapshots = Vec::new();

    match msg {
        Message::User { content } => {
            for item in content.iter() {
                // ToolResult, Image, etc. — skip
                if let UserContent::Text(text) = item {
                    snapshots.push(UiMessageSnapshot::new("user", text.text.clone()));
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

                        let args_summary =
                            crate::protocol::tool_args::summarize_tool_arguments(&args_json);

                        let display_content = format!("→ {args_summary}");

                        // A call with no result at all carries no recorded
                        // status; the replay rule makes it a failed completion.
                        let status = call_statuses
                            .get(&tool_call.id)
                            .copied()
                            .unwrap_or(ToolStatus::Failed);
                        let snapshot = UiMessageSnapshot::new("tool", display_content)
                            .with_tool_name(tool_call.function.name.to_string())
                            .with_tool_details(
                                Some(args_json),
                                None,
                                tool_success_map.get(&tool_call.id).copied(),
                            )
                            .with_tool_status(status);
                        snapshots.push(snapshot);
                        // The display snapshot follows its own call's snapshot,
                        // so the TUI docks it under this header rather than
                        // under the last Tool block of the turn.
                        if let Some(display) = tool_displays.get(&tool_call.id) {
                            snapshots.push(
                                UiMessageSnapshot::new("tool_display", String::new())
                                    .with_tool_display(display.clone()),
                            );
                        }
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
