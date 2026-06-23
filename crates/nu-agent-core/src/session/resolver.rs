use nu_protocol::LabeledError;

use crate::hook::agent_hook::is_tool_failure;
use crate::protocol::contracts::UiMessageSnapshot;
use crate::session::{
    ConversationStore, JsonlConversationStore, Session, SessionStore, StoreEntry,
};
use crate::types::{AssistantContent, Message, ToolResultContent, UserContent};
use std::collections::HashMap;

use crate::tools::handler::build_direct_tool_display;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionRequest {
    None,
    Attach(String),
    Create(String),
}

#[derive(Debug, Clone)]
pub struct SessionResolutionInput {
    pub use_tui: bool,
    pub input_is_nothing: bool,
    pub session_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SessionResolution {
    pub final_session_id: Option<String>,
    pub session: Option<Session>,
    pub tui_should_hydrate_transcript: bool,
    pub tui_initial_messages: Vec<UiMessageSnapshot>,
    pub last_total_tokens: Option<u64>,
}

pub trait SessionResolver {
    fn resolve(&self, input: SessionResolutionInput) -> Result<SessionResolution, LabeledError>;
}

pub struct DefaultSessionResolver<'a> {
    store: &'a SessionStore,
}

impl<'a> DefaultSessionResolver<'a> {
    pub fn new(store: &'a SessionStore) -> Self {
        Self { store }
    }
}

impl SessionResolver for DefaultSessionResolver<'_> {
    fn resolve(&self, input: SessionResolutionInput) -> Result<SessionResolution, LabeledError> {
        let request = resolve_session_request(input.use_tui, input.session_id);
        match request {
            SessionRequest::None => Ok(SessionResolution {
                final_session_id: None,
                session: None,
                tui_should_hydrate_transcript: false,
                tui_initial_messages: Vec::new(),
                last_total_tokens: None,
            }),
            SessionRequest::Attach(id) => {
                let (session, tui_hydration_messages, last_total_tokens) = if input.use_tui {
                    let (session, existed_before_attach) =
                        load_or_create_tui_session(self.store, &id)?;
                    let (messages, last_total_tokens) =
                        if input.input_is_nothing && existed_before_attach {
                            // Load store entries (messages + markers) from JSONL
                            let conversation_store =
                                JsonlConversationStore::new(self.store.cache_dir().to_path_buf());
                            let (entries, last_total_tokens) =
                                conversation_store.load_all(&id).map_err(|e| {
                                    LabeledError::new(format!("Failed to load messages: {e}"))
                                })?;

                            // Convert to UiMessageSnapshots for transcript display
                            (
                                hydrate_transcript_from_store_entries(&entries),
                                last_total_tokens,
                            )
                        } else {
                            (Vec::new(), None)
                        };

                    (session, messages, last_total_tokens)
                } else {
                    let session = self.store.get_or_create(Some(id.clone())).map_err(|e| {
                        LabeledError::new(format!("Failed to load/create session: {e}"))
                    })?;

                    (session, Vec::new(), None)
                };

                Ok(SessionResolution {
                    final_session_id: Some(id),
                    session: Some(session),
                    tui_should_hydrate_transcript: !tui_hydration_messages.is_empty(),
                    tui_initial_messages: tui_hydration_messages,
                    last_total_tokens,
                })
            }
            SessionRequest::Create(id) => {
                let _ = (input.use_tui, input.input_is_nothing);
                let session = self.store.get_or_create(Some(id.clone())).map_err(|e| {
                    LabeledError::new(format!("Failed to load/create session: {e}"))
                })?;
                Ok(SessionResolution {
                    final_session_id: Some(id),
                    session: Some(session),
                    tui_should_hydrate_transcript: false,
                    tui_initial_messages: Vec::new(),
                    last_total_tokens: None,
                })
            }
        }
    }
}

pub fn generate_session_id() -> String {
    use chrono::Utc;
    let now = Utc::now();
    format!(
        "session-{}-{}",
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

/// Converts store entries (messages + compaction markers) into UiMessageSnapshots
/// for TUI transcript hydration.
///
/// This function maps store entry types to transcript display items:
/// - `StoreEntry::Message` → delegated to `hydrate_single_message`
/// - `StoreEntry::Marker` → compaction snapshot with strategy, counts, and summary text
///
/// # Arguments
/// * `entries` - Slice of StoreEntry from ConversationStore::load_all()
///
/// # Returns
/// Iterator of UiMessageSnapshot ready for transcript hydration
fn hydrate_transcript_from_store_entries(entries: &[StoreEntry]) -> Vec<UiMessageSnapshot> {
    // Pass 1: collect call_id → tool_name from all ToolCalls and
    //         call_id → success from all ToolResults (failure = is_tool_failure())
    let mut tool_names: HashMap<String, String> = HashMap::new();
    let mut tool_success_map: HashMap<String, bool> = HashMap::new();
    for entry in entries {
        match entry {
            StoreEntry::Message(Message::Assistant { content, .. }) => {
                for item in content.iter() {
                    if let AssistantContent::ToolCall(tc) = item {
                        tool_names.insert(tc.id.clone(), tc.function.name.clone());
                    }
                }
            }
            StoreEntry::Message(Message::User { content }) => {
                for item in content.iter() {
                    if let UserContent::ToolResult(tr) = item {
                        for c in tr.content.iter() {
                            if let ToolResultContent::Text(t) = c {
                                let success = !is_tool_failure(&t.text);
                                tool_success_map.insert(tr.id.clone(), success);
                                break;
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    // Pass 2: generate snapshots with tool display reconstruction
    entries
        .iter()
        .flat_map(|entry| match entry {
            StoreEntry::Message(msg) => hydrate_single_message(msg, &tool_names, &tool_success_map),
            StoreEntry::Marker(marker) => {
                vec![UiMessageSnapshot::new(
                    "compaction",
                    format_compaction_content(marker),
                )]
            }
        })
        .collect()
}

/// Maximum character length for the summary body in a hydrated compaction entry.
const COMPACTION_SUMMARY_MAX_CHARS: usize = 500;

/// Format a `CompactionMarker` into display content for the TUI transcript.
///
/// Produces a stats header line followed by the (optionally truncated) summary body:
/// ```text
/// 10 summarized · 3 kept · strategy: sliding_summary
///
/// Summary text here...
/// ```
///
/// If the summary is empty, only the stats header is emitted.
fn format_compaction_content(marker: &crate::session::CompactionMarker) -> String {
    let stats = format!(
        "{} summarized · {} kept · strategy: {}",
        marker.summarized_count, marker.kept_recent_count, marker.strategy
    );

    let body = marker.summary.trim();
    if body.is_empty() {
        stats
    } else {
        let truncated: String = body.chars().take(COMPACTION_SUMMARY_MAX_CHARS).collect();
        let ellipsis = if body.chars().count() > COMPACTION_SUMMARY_MAX_CHARS {
            "…"
        } else {
            ""
        };
        format!("{stats}\n\n{truncated}{ellipsis}")
    }
}

/// Converts a single rig Message into UiMessageSnapshots.
///
/// This function maps rig message types to transcript display items:
/// - `Message::User { content }` with `UserContent::Text` → user transcript item
/// - `Message::User { content }` with `UserContent::ToolResult` → skipped (not shown in TUI)
/// - `Message::Assistant { content }` with `AssistantContent::Text` → assistant text
/// - `Message::Assistant { content }` with `AssistantContent::ToolCall` → tool call display
/// - `Message::System { content }` → system/compaction summary display
fn hydrate_single_message(
    msg: &Message,
    tool_names: &HashMap<String, String>,
    tool_success_map: &HashMap<String, bool>,
) -> Vec<UiMessageSnapshot> {
    let mut snapshots = Vec::new();

    match msg {
        Message::User { content } => {
            for item in content.iter() {
                match item {
                    UserContent::Text(text) => {
                        snapshots.push(UiMessageSnapshot::new("user", text.text.clone()));
                    }
                    UserContent::ToolResult(tr) => {
                        if let Some(tool_name) = tool_names.get(&tr.id) {
                            let result_text: String = tr
                                .content
                                .iter()
                                .filter_map(|c| match c {
                                    ToolResultContent::Text(t) => Some(t.text.as_str()),
                                    _ => None,
                                })
                                .collect::<Vec<_>>()
                                .join("\n");
                            if let Ok(json) =
                                serde_json::from_str::<serde_json::Value>(&result_text)
                                && let Some(display) = build_direct_tool_display(tool_name, &json)
                            {
                                snapshots.push(
                                    UiMessageSnapshot::new("tool_display", String::new())
                                        .with_tool_display(display),
                                );
                            }
                        }
                    }
                    _ => {} // Image, etc. — skip
                }
            }
        }
        Message::Assistant { content, .. } => {
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

                        let display_content =
                            format!("tool[{}] args={}", tool_call.function.name, args_summary);

                        snapshots.push(
                            UiMessageSnapshot::new("tool", display_content).with_tool_details(
                                Some(args_json),
                                None,
                                Some(*tool_success_map.get(&tool_call.id).unwrap_or(&true)),
                            ),
                        );
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

fn load_or_create_tui_session(
    store: &SessionStore,
    session_id: &str,
) -> Result<(Session, bool), LabeledError> {
    match store.load_session(session_id) {
        Ok(session) => Ok((session, true)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => store
            .get_or_create(Some(session_id.to_string()))
            .map_err(|create_err| {
                LabeledError::new(format!(
                    "Failed to create missing session '{session_id}': {create_err}"
                ))
            })
            .map(|session| (session, false)),
        Err(err) => Err(LabeledError::new(format!(
            "Failed to attach session '{session_id}': {err}"
        ))),
    }
}

#[cfg(test)]
#[path = "resolver_test.rs"]
mod resolver_test;
