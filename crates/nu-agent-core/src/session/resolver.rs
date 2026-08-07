use nu_protocol::LabeledError;
use std::path::PathBuf;
use std::sync::Arc;

use crate::hook::agent_hook::is_tool_failure;
use crate::protocol::contracts::UiMessageSnapshot;
use crate::session::{CompactionMarker, Session, SessionStore, StoreEntry, extract_llm_context};
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

pub struct DefaultSessionResolver<S: SessionStore + Clone + Send + Sync> {
    store: Arc<S>,
}

impl<S: SessionStore + Clone + Send + Sync> DefaultSessionResolver<S> {
    pub fn new(store: Arc<S>) -> Self {
        Self { store }
    }
}

impl<S: SessionStore + Clone + Send + Sync> SessionResolver for DefaultSessionResolver<S> {
    async fn resolve(
        &self,
        input: SessionResolutionInput,
    ) -> Result<SessionResolution, LabeledError> {
        let prefix = crate::session::prefix::dir_prefix(&input.cwd);
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
                let (session, existed_before_attach) =
                    load_or_create_session(&self.store, &id).await?;
                let (initial_messages, last_total_tokens) = if existed_before_attach {
                    // Load store entries (messages + markers) from the store
                    let (_metadata, entries) = self
                        .store
                        .load(&id)
                        .await
                        .map_err(|e| LabeledError::new(format!("Failed to load messages: {e}")))?
                        .unwrap_or_else(|| {
                            // Session exists but has no entries yet — shouldn't happen
                            // after load_or_create_session returned existed=true,
                            // but handle gracefully.
                            (
                                crate::session::SessionMetadata {
                                    metadata_type: "session".to_string(),
                                    session_id: id.clone(),
                                    created_at: chrono::Utc::now(),
                                    title: None,
                                },
                                Vec::new(),
                            )
                        });

                    // Convert to UiMessageSnapshots for transcript display.
                    // Estimate token count from extracted LLM context so that
                    // compaction evaluation works correctly on re-attach.
                    let llm_context = extract_llm_context(&entries);
                    let estimated_tokens: u64 = llm_context
                        .iter()
                        .map(|msg| crate::compaction::helpers::estimate_tokens(msg) as u64)
                        .sum();
                    (
                        hydrate_transcript_from_store_entries(&entries),
                        Some(estimated_tokens),
                    )
                } else {
                    (Vec::new(), None)
                };

                Ok(SessionResolution {
                    final_session_id: Some(id),
                    session: Some(session),
                    should_hydrate_transcript: !initial_messages.is_empty(),
                    initial_messages,
                    last_total_tokens,
                })
            }
            SessionRequest::Create(id) => {
                let session = create_session(&id);
                Ok(SessionResolution {
                    final_session_id: Some(id),
                    session: Some(session),
                    should_hydrate_transcript: false,
                    initial_messages: Vec::new(),
                    last_total_tokens: None,
                })
            }
        }
    }
}

/// Create a new Session with the given ID.
fn create_session(id: &str) -> Session {
    Session::new(id.to_string())
}

/// Load an existing session or create a new one.
/// Returns `(Session, existed_before_attach)`.
async fn load_or_create_session<S: SessionStore + Clone + Send + Sync>(
    store: &Arc<S>,
    session_id: &str,
) -> Result<(Session, bool), LabeledError> {
    match store.load(session_id).await {
        Ok(Some((metadata, _entries))) => {
            let session = Session::from_metadata(metadata);
            Ok((session, true))
        }
        Ok(None) => {
            // Session doesn't exist yet — create it
            let session = create_session(session_id);
            Ok((session, false))
        }
        Err(e) => Err(LabeledError::new(format!(
            "Failed to attach session '{session_id}': {e}"
        ))),
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

/// Converts store entries (messages + compaction markers) into UiMessageSnapshots
/// for TUI transcript hydration.
///
/// This function maps store entry types to transcript display items:
/// - `StoreEntry::Message` → delegated to `hydrate_single_message`
/// - `StoreEntry::Marker` → compaction snapshot with strategy, counts, and summary text
///
/// # Arguments
/// * `entries` - Slice of StoreEntry from SessionStore::load()
///
/// # Returns
/// Iterator of UiMessageSnapshot ready for transcript hydration
pub(crate) fn hydrate_transcript_from_store_entries(
    entries: &[StoreEntry],
) -> Vec<UiMessageSnapshot> {
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
/// 10 summarized · strategy: sliding_summary
///
/// Summary text here...
/// ```
///
/// If the summary is empty, only the stats header is emitted.
fn format_compaction_content(marker: &CompactionMarker) -> String {
    let stats = format!(
        "{} summarized · strategy: {}",
        marker.summarized_count, marker.strategy
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
pub(crate) fn hydrate_single_message(
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
                            format!("tool[{}] → {}", tool_call.function.name, args_summary);

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
