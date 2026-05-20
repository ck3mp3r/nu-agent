use nu_protocol::LabeledError;

use crate::agent::protocol::contracts::UiMessageSnapshot;
use crate::session::{ConversationStore, JsonlConversationStore, Session, SessionStore};
use rig::completion::message::{AssistantContent, UserContent};
use rig::completion::Message;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SessionRequest {
    None,
    Attach(String),
    Create(String),
}

#[derive(Debug, Clone)]
pub(crate) struct SessionResolutionInput {
    pub use_tui: bool,
    pub input_is_nothing: bool,
    pub session_id: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct SessionResolution {
    pub final_session_id: Option<String>,
    pub session: Option<Session>,
    pub tui_should_hydrate_transcript: bool,
    pub tui_initial_messages: Vec<UiMessageSnapshot>,
}

pub(crate) trait SessionResolver {
    fn resolve(&self, input: SessionResolutionInput) -> Result<SessionResolution, LabeledError>;
}

pub(crate) struct DefaultSessionResolver<'a> {
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
            }),
            SessionRequest::Attach(id) => {
                let (session, tui_hydration_messages) = if input.use_tui {
                    let (session, existed_before_attach) =
                        load_or_create_tui_session(self.store, &id)?;
                    let messages = if input.input_is_nothing && existed_before_attach {
                        // Load rig messages from JSONL via ConversationStore
                        let conversation_store =
                            JsonlConversationStore::new(self.store.cache_dir().to_path_buf());
                        let rig_messages = conversation_store
                            .load(&id)
                            .map_err(|e| LabeledError::new(format!("Failed to load messages: {e}")))?;

                        // Convert to UiMessageSnapshots for transcript display
                        hydrate_transcript_from_rig_messages(&rig_messages).collect()
                    } else {
                        Vec::new()
                    };

                    (session, messages)
                } else {
                    let session = self.store.get_or_create(Some(id.clone())).map_err(|e| {
                        LabeledError::new(format!("Failed to load/create session: {e}"))
                    })?;

                    (session, Vec::new())
                };

                Ok(SessionResolution {
                    final_session_id: Some(id),
                    session: Some(session),
                    tui_should_hydrate_transcript: !tui_hydration_messages.is_empty(),
                    tui_initial_messages: tui_hydration_messages,
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
                })
            }
        }
    }
}

pub(crate) fn generate_session_id() -> String {
    use chrono::Utc;
    let now = Utc::now();
    format!(
        "session-{}-{}",
        now.format("%Y%m%d-%H%M%S"),
        now.timestamp_subsec_micros()
    )
}

pub(crate) fn resolve_session_request(
    use_tui: bool,
    session_id: Option<String>,
) -> SessionRequest {
    match (use_tui, session_id) {
        // TUI explicit session policy is centralized here:
        // always attempt to attach first (resolver handles load-then-create fallback).
        (true, Some(id)) => SessionRequest::Attach(id),
        (_, Some(id)) => SessionRequest::Create(id),
        (true, None) => SessionRequest::Create(generate_session_id()),
        (false, None) => SessionRequest::None,
    }
}

/// Converts rig Messages loaded from JSONL into UiMessageSnapshots for TUI transcript hydration.
///
/// This function maps rig message types to transcript display items:
/// - `Message::User { content }` with `UserContent::Text` → user transcript item
/// - `Message::User { content }` with `UserContent::ToolResult` → tool result display
/// - `Message::Assistant { content }` with `AssistantContent::Text` → assistant text
/// - `Message::Assistant { content }` with `AssistantContent::ToolCall` → tool call display
/// - `Message::System { content }` → system/compaction summary display
///
/// # Arguments
/// * `messages` - Slice of rig::completion::Message from ConversationStore::load()
///
/// # Returns
/// Iterator of UiMessageSnapshot ready for transcript hydration
fn hydrate_transcript_from_rig_messages(
    messages: &[Message],
) -> impl Iterator<Item = UiMessageSnapshot> + '_ {
    messages.iter().flat_map(|msg| {
        let mut snapshots = Vec::new();

        match msg {
            Message::User { content } => {
                for item in content.iter() {
                    match item {
                        UserContent::Text(text) => {
                            snapshots.push(UiMessageSnapshot::new("user", text.text.clone()));
                        }
                        UserContent::ToolResult(_) => {
                            // Tool results are kept in memory/JSONL for the LLM,
                            // but not shown in the hydrated TUI transcript.
                            // Only the tool invocation line (⚙) is displayed.
                        }
                        _ => {} // Image, etc. — skip
                    }
                }
            }
            Message::Assistant { content, .. } => {
                // Process text and tool calls separately
                for item in content.iter() {
                    match item {
                        AssistantContent::Text(text) => {
                            if !text.text.is_empty() {
                                snapshots.push(UiMessageSnapshot::new("assistant", text.text.clone()));
                            }
                        }
                        AssistantContent::ToolCall(tool_call) => {
                            // Tool calls: hydrate as tool invocation with proper format
                            let args_json = serde_json::to_string(&tool_call.function.arguments)
                                .unwrap_or_else(|_| "{}".to_string());

                            // Summarize arguments to match live rendering
                            let args_summary =
                                crate::agent::protocol::tool_args::summarize_tool_arguments(&args_json);

                            // Format content to match what start_tool_call produces
                            let display_content =
                                format!("tool[{}] args={}", tool_call.function.name, args_summary);

                            snapshots.push(
                                UiMessageSnapshot::new("tool", display_content)
                                    .with_tool_details(Some(args_json), None, Some(true)),
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
    })
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
