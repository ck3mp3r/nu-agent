//! Translation of nu-agent `UiEvent`s into ACP `session/update` notifications.
//!
//! The ACP prompt handler drains the session bus while `execute_turn()` runs and
//! forwards every translated event to the client as a `session/update`
//! notification. Events without an ACP equivalent are dropped.
//!
//! Translation is stateful: `ToolStarted` and `ToolCompleted` carry no call id,
//! so the translator allocates one per started call and matches the completion
//! back to it. One [`EventTranslator`] therefore serves one session.

// region:    --- Modules

use std::collections::VecDeque;
use std::future::Future;

use agent_client_protocol::schema::v1::{
    Content, ContentBlock, ContentChunk, Diff, SessionId, SessionInfoUpdate, SessionNotification,
    SessionUpdate, TextContent, ToolCall, ToolCallContent, ToolCallId, ToolCallStatus,
    ToolCallUpdate, ToolCallUpdateFields, ToolKind, UsageUpdate,
};
use agent_client_protocol::{Client, ConnectionTo};
use agnt_core::bus::{Bus, ChannelError, TryRecvError, UiEventRx};
use agnt_core::config::defaults;
use agnt_core::protocol::event::{ToolDisplay, ToolDisplaySection, UiEvent};
use agnt_core::transcript::ir::ContentKind;

use crate::error::AcpError;
use crate::permission::{AcpPermissionBridge, PermissionRequestSink};

// endregion: --- Modules

// region:    --- Types

/// Destination for translated `session/update` notifications.
///
/// Implemented by the ACP connection. The trait exists so the drain loop can be
/// driven by a test sink without a live transport.
pub trait SessionUpdateSink {
    /// Send one notification. Returns the transport error on failure.
    fn send_update(
        &self,
        session_id: &str,
        update: SessionUpdate,
    ) -> core::result::Result<(), AcpError>;
}

impl SessionUpdateSink for ConnectionTo<Client> {
    fn send_update(
        &self,
        session_id: &str,
        update: SessionUpdate,
    ) -> core::result::Result<(), AcpError> {
        self.send_notification(SessionNotification::new(SessionId::new(session_id), update))
            .map_err(|err| AcpError::Runtime(err.to_string()))
    }
}

/// Translates the `UiEvent` stream of one session into ACP `SessionUpdate`s.
///
/// Holds the per-session tool-call counter and the queue of started calls that
/// have not completed yet. Both are required: `ToolStarted` and `ToolCompleted`
/// carry no call id, so the id is allocated on start and matched on completion
/// by `(name, arguments)`. The counter keeps ids unique even when the same tool
/// is called twice with identical arguments in one session.
pub struct EventTranslator {
    session_id: String,
    max_context_tokens: u64,
    next_call_seq: u64,
    pending: VecDeque<PendingCall>,
}

/// A started tool call awaiting its completion event.
struct PendingCall {
    name: String,
    arguments: String,
    id: ToolCallId,
}

// endregion: --- Types

// region:    --- EventTranslator Impl

impl EventTranslator {
    /// Create a translator for `session_id`, reporting the built-in default
    /// context-window size on `usage_update`.
    pub fn new(session_id: impl Into<String>) -> Self {
        Self::with_context_size(session_id, defaults::MAX_CONTEXT_TOKENS as u64)
    }

    /// Create a translator for `session_id` that reports `max_context_tokens`
    /// as the context-window size on `usage_update`.
    pub fn with_context_size(session_id: impl Into<String>, max_context_tokens: u64) -> Self {
        Self {
            session_id: session_id.into(),
            max_context_tokens,
            next_call_seq: 0,
            pending: VecDeque::new(),
        }
    }

    /// The session this translator serves.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Translate one bus event into an ACP `SessionUpdate`.
    ///
    /// Returns `None` for events that have no ACP equivalent.
    pub fn translate(&mut self, event: UiEvent) -> Option<SessionUpdate> {
        match event {
            UiEvent::AssistantMessage { delta, .. } => Some(SessionUpdate::AgentMessageChunk(
                ContentChunk::new(ContentBlock::Text(TextContent::new(delta))),
            )),
            UiEvent::ToolStarted {
                name,
                arguments,
                call_line,
                ..
            } => {
                let id = self.allocate_call_id(&name, &arguments);
                let summary = call_line.to_plain_text();
                let title = if summary.is_empty() {
                    name.clone()
                } else {
                    summary
                };
                Some(SessionUpdate::ToolCall(
                    ToolCall::new(id, title)
                        .name(name.clone())
                        .kind(tool_kind(&name))
                        .status(ToolCallStatus::Pending),
                ))
            }
            UiEvent::ToolCompleted {
                name,
                arguments,
                success,
                display,
                message,
                ..
            } => {
                let id = self.take_call_id(&name, &arguments);
                let status = if success {
                    ToolCallStatus::Completed
                } else {
                    ToolCallStatus::Failed
                };
                let mut content = display
                    .as_ref()
                    .map(content_from_display)
                    .unwrap_or_default();
                if content.is_empty()
                    && let Some(message) = message
                {
                    content.push(ToolCallContent::Content(Content::new(message)));
                }
                let mut fields = ToolCallUpdateFields::new().status(status);
                if !content.is_empty() {
                    fields = fields.content(content);
                }
                Some(SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
                    id, fields,
                )))
            }
            UiEvent::LlmCompleted { total_tokens, .. } => Some(SessionUpdate::UsageUpdate(
                UsageUpdate::new(total_tokens, self.max_context_tokens),
            )),
            UiEvent::CompactionStarted { source } => {
                Some(compaction_update("started", &source, None))
            }
            UiEvent::CompactionCompleted {
                source,
                summary_preview,
                ..
            } => Some(compaction_update(
                "completed",
                &source,
                Some(summary_preview),
            )),
            UiEvent::CompactionFailed { source, message } => {
                Some(compaction_update("failed", &source, Some(message)))
            }
            // No ACP equivalent: streaming compaction deltas, turn lifecycle
            // markers, warnings, and permission events (the permission bridge
            // owns `PermissionRequested`).
            _ => None,
        }
    }

    /// Allocate the id for a started call and queue it for completion matching.
    fn allocate_call_id(&mut self, name: &str, arguments: &str) -> ToolCallId {
        let id = self.next_call_id(name);
        self.pending.push_back(PendingCall {
            name: name.to_string(),
            arguments: arguments.to_string(),
            id: id.clone(),
        });
        id
    }

    /// Take the id of the oldest started call matching `(name, arguments)`.
    ///
    /// Falls back to a fresh id when no start was observed — the drain loop can
    /// begin mid-turn, and a completion without a start must still reach the
    /// client. The fallback does not queue the call: nothing will complete it,
    /// and a queued entry would mis-match a later call with the same arguments.
    fn take_call_id(&mut self, name: &str, arguments: &str) -> ToolCallId {
        let position = self
            .pending
            .iter()
            .position(|call| call.name == name && call.arguments == arguments);
        match position.and_then(|index| self.pending.remove(index)) {
            Some(call) => call.id,
            None => self.next_call_id(name),
        }
    }

    /// Allocate the next unique id for `name` without queueing it.
    fn next_call_id(&mut self, name: &str) -> ToolCallId {
        let seq = self.next_call_seq;
        self.next_call_seq += 1;
        ToolCallId::new(format!("{name}-{seq}"))
    }
}

// endregion: --- EventTranslator Impl

// region:    --- Public Functions

/// Drain the session bus into `session/update` notifications until the bus
/// closes.
///
/// Subscribes a fresh receiver, so events published before the call are lost.
/// For a prompt turn, prefer [`run_turn_with_notifications`]: it owns the
/// receiver, so it can flush the events published just before the turn
/// completes.
pub async fn drain_bus_to_notifications<S, P>(
    bus: &Bus,
    sink: &S,
    translator: &mut EventTranslator,
    bridge: &mut AcpPermissionBridge<P>,
) where
    S: SessionUpdateSink,
    P: PermissionRequestSink,
{
    let mut rx = bus.ui_event().subscribe();
    drain_receiver_to_notifications(&mut rx, sink, translator, bridge).await;
}

/// Drain an existing bus receiver into `session/update` notifications until the
/// bus closes.
///
/// Use this variant when the caller owns the receiver, so the same receiver can
/// be flushed after the turn completes. Permission requests are routed to
/// `bridge`; every request still awaiting a decision when the loop ends is
/// denied.
pub async fn drain_receiver_to_notifications<S, P>(
    rx: &mut UiEventRx,
    sink: &S,
    translator: &mut EventTranslator,
    bridge: &mut AcpPermissionBridge<P>,
) where
    S: SessionUpdateSink,
    P: PermissionRequestSink,
{
    loop {
        match rx.recv().await {
            Ok(event) => {
                if !handle_event(sink, translator, bridge, event).await {
                    break;
                }
            }
            Err(ChannelError::Lagged { dropped }) => {
                log::warn!(
                    "session {}: ui_event receiver lagged, dropped {dropped} events",
                    translator.session_id()
                );
            }
            Err(_) => break,
        }
    }
    bridge.deny_all_pending();
}

/// Forward every event already buffered on `rx` as a `session/update`
/// notification, without awaiting new events.
///
/// Call this after `execute_turn()` returns: the turn publishes its final
/// events before it completes, so a drain future dropped by `tokio::select!`
/// would otherwise leave them unread.
///
/// Permission requests are dropped here. The turn cannot complete while a
/// permission decision is still awaited, so a buffered request at this point
/// has no resolver left to unblock.
pub fn flush_receiver_to_notifications<S: SessionUpdateSink>(
    rx: &mut UiEventRx,
    sink: &S,
    translator: &mut EventTranslator,
) {
    loop {
        match rx.try_recv() {
            Ok(event) => {
                send_event(sink, translator, event);
            }
            Err(TryRecvError::Lagged(dropped)) => {
                log::warn!(
                    "session {}: ui_event receiver lagged, dropped {dropped} events",
                    translator.session_id()
                );
            }
            Err(TryRecvError::Empty | TryRecvError::Closed) => break,
        }
    }
}

/// Drive one prompt turn while streaming its bus events to the client.
///
/// `turn` is the `execute_turn()` future. The loop polls the turn and the bus
/// receiver together, so events reach the client as they are published. When
/// the turn completes, the receiver is flushed and the turn output is returned.
///
/// The receiver must be subscribed before the turn starts, otherwise events
/// published before the subscription are lost. Permission requests are routed
/// to `bridge`; every request still awaiting a decision when the turn ends is
/// denied.
pub async fn run_turn_with_notifications<S, P, F>(
    rx: &mut UiEventRx,
    sink: &S,
    translator: &mut EventTranslator,
    bridge: &mut AcpPermissionBridge<P>,
    turn: F,
) -> F::Output
where
    S: SessionUpdateSink,
    P: PermissionRequestSink,
    F: Future,
{
    tokio::pin!(turn);
    let mut draining = true;
    let output = loop {
        tokio::select! {
            biased;
            output = &mut turn => break output,
            received = rx.recv(), if draining => match received {
                Ok(event) => {
                    if !handle_event(sink, translator, bridge, event).await {
                        draining = false;
                    }
                }
                Err(ChannelError::Lagged { dropped }) => {
                    log::warn!(
                        "session {}: ui_event receiver lagged, dropped {dropped} events",
                        translator.session_id()
                    );
                }
                Err(_) => draining = false,
            },
        }
    };
    if draining {
        flush_receiver_to_notifications(rx, sink, translator);
    }
    bridge.deny_all_pending();
    output
}

// endregion: --- Public Functions

// region:    --- Support

/// Handle one bus event during a drain.
///
/// Permission requests go to the bridge — the translator has no ACP equivalent
/// for them. Everything else is translated and sent. Returns `false` when the
/// transport is gone.
async fn handle_event<S, P>(
    sink: &S,
    translator: &mut EventTranslator,
    bridge: &mut AcpPermissionBridge<P>,
    event: UiEvent,
) -> bool
where
    S: SessionUpdateSink,
    P: PermissionRequestSink,
{
    if let UiEvent::PermissionRequested {
        request_id,
        context,
    } = event
    {
        if let Err(err) = bridge.handle_permission_request(&request_id, context).await {
            log::warn!(
                "session {}: failed to handle permission request: {err}",
                translator.session_id()
            );
        }
        return true;
    }
    send_event(sink, translator, event)
}

/// Send one translated event. Returns `false` when the transport is gone.
fn send_event<S: SessionUpdateSink>(
    sink: &S,
    translator: &mut EventTranslator,
    event: UiEvent,
) -> bool {
    let Some(update) = translator.translate(event) else {
        return true;
    };
    match sink.send_update(translator.session_id(), update) {
        Ok(()) => true,
        Err(err) => {
            log::warn!(
                "session {}: failed to send session/update: {err}",
                translator.session_id()
            );
            false
        }
    }
}

/// Map a nu-agent tool name to an ACP tool kind.
fn tool_kind(name: &str) -> ToolKind {
    match name {
        "read" => ToolKind::Read,
        "edit" => ToolKind::Edit,
        "grep" | "glob" => ToolKind::Search,
        "nu" => ToolKind::Execute,
        "http" => ToolKind::Fetch,
        "skill" => ToolKind::Other,
        _ if name.starts_with("ast_") => ToolKind::Read,
        _ if name.starts_with("tmux_") => ToolKind::Execute,
        _ => ToolKind::Other,
    }
}

/// Build a `session_info_update` carrying compaction state in `_meta`.
fn compaction_update(state: &str, source: &str, detail: Option<String>) -> SessionUpdate {
    let mut compaction = serde_json::Map::new();
    compaction.insert(
        "state".to_string(),
        serde_json::Value::String(state.to_string()),
    );
    compaction.insert(
        "source".to_string(),
        serde_json::Value::String(source.to_string()),
    );
    if let Some(detail) = detail {
        compaction.insert("detail".to_string(), serde_json::Value::String(detail));
    }
    let mut meta = serde_json::Map::new();
    meta.insert(
        "nu-agent".to_string(),
        serde_json::Value::Object(compaction),
    );
    SessionUpdate::SessionInfoUpdate(SessionInfoUpdate::new().meta(meta))
}

/// Map every display section to ACP tool-call content.
pub(crate) fn content_from_display(display: &ToolDisplay) -> Vec<ToolCallContent> {
    display.sections.iter().map(section_content).collect()
}

/// Map one display section to ACP tool-call content.
///
/// A diff section becomes an ACP `Diff` carrying the file path from the
/// content kind. The section content is a unified diff, so the old and new
/// sides are reconstructed from the hunk lines — the reconstruction covers the
/// hunks only, not the untouched remainder of the file.
fn section_content(section: &ToolDisplaySection) -> ToolCallContent {
    match &section.kind {
        ContentKind::Diff { path, .. } => {
            let (old_text, new_text) = split_unified_diff(&section.content);
            ToolCallContent::Diff(Diff::new(path.clone(), new_text).old_text(old_text))
        }
        _ => ToolCallContent::Content(Content::new(section.content.clone())),
    }
}

/// Split a unified diff into its old and new sides.
///
/// File headers, hunk headers, and `\ No newline` markers are dropped. Context
/// lines feed both sides; `-` lines feed the old side, `+` lines the new side.
/// The old side is `None` when the diff contains no old-side line (a new file).
fn split_unified_diff(text: &str) -> (Option<String>, String) {
    let mut old_text = String::new();
    let mut new_text = String::new();
    let mut has_old = false;
    for line in text.lines() {
        if line.starts_with("--- ") || line.starts_with("+++ ") || line.starts_with("@@") {
            continue;
        }
        if line.starts_with('\\') {
            continue;
        }
        match line.chars().next() {
            Some('-') => {
                old_text.push_str(&line[1..]);
                old_text.push('\n');
                has_old = true;
            }
            Some('+') => {
                new_text.push_str(&line[1..]);
                new_text.push('\n');
            }
            Some(' ') => {
                old_text.push_str(&line[1..]);
                old_text.push('\n');
                new_text.push_str(&line[1..]);
                new_text.push('\n');
                has_old = true;
            }
            _ => {
                old_text.push_str(line);
                old_text.push('\n');
                new_text.push_str(line);
                new_text.push('\n');
                has_old = true;
            }
        }
    }
    (has_old.then_some(old_text), new_text)
}

// endregion: --- Support

// region:    --- Tests

#[cfg(test)]
#[path = "../test/translator.rs"]
mod translator_test;

// endregion: --- Tests
