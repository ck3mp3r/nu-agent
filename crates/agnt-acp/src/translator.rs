//! Translation of nu-agent session records into ACP `session/update` notifications.
//!
//! The ACP prompt handler drains the session bus while `execute_turn()` runs and
//! forwards every translated event to the client as a `session/update`
//! notification. Events without an ACP equivalent are dropped.
//!
//! Translation is stateful: the translator allocates its own ACP tool-call id
//! per started call and matches the completion back to it, so the ids a client
//! sees keep their `{name}-{seq}` shape regardless of the call key the event
//! carries. One [`EventTranslator`] therefore serves one session.

// region:    --- Modules

use std::collections::VecDeque;
use std::future::Future;

use agent_client_protocol::schema::v1::{
    Content, ContentBlock, ContentChunk, Diff, SessionId, SessionInfoUpdate, SessionNotification,
    SessionUpdate, TextContent, ToolCall, ToolCallContent, ToolCallId, ToolCallStatus,
    ToolCallUpdate, ToolCallUpdateFields, ToolKind, UsageUpdate,
};
use agent_client_protocol::{Client, ConnectionTo};
use agnt_kernel::{
    BuiltinKind, CompactionRecord, CompactionSource, InferenceRecord, NoticeKind, PermissionRecord,
    SessionRecord, ToolKind as KernelToolKind, ToolRecord, ToolStatus,
};
use agnt_render::tool_args::plain_call_text;
use agnt_render::tool_view::ToolView;
use agnt_render::transcript::ir::{ContentKind, Display, DisplaySection};
use agnt_runtime::bus::{
    Control, ControlCommand, ControlRx, Narration, NarrationEvent, ReceiveError, Stream,
    Subscription, TryReceiveError,
};
use agnt_runtime::config::defaults;

use crate::error::AcpError;
use crate::permission::{AcpPermissionBridge, PermissionRequestSink};

// endregion: --- Modules

// region:    --- Types

/// The drawer's intake: one datum from the concerns ACP draws from.
///
/// The producer ships records and events; this enum only names which concern
/// arrived. How anything is translated is this crate's business.
pub enum TranslateInput<'a> {
    Record(&'a SessionRecord),
    Narration(&'a NarrationEvent),
}

/// The concerns one ACP turn drains.
///
/// Named fields, one per concern the turn reads: the ordered record stream
/// (which carries compaction now), the narration text, and the control intake
/// the compaction request rides.
pub struct TurnSources<'a> {
    pub stream: &'a Stream,
    pub narration: &'a Narration,
    pub control: &'a Control,
}

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

/// Translates the record and narration concerns of one session into ACP
/// `SessionUpdate`s.
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
    pub fn translate(&mut self, input: TranslateInput<'_>) -> Option<SessionUpdate> {
        match input {
            TranslateInput::Narration(NarrationEvent::InferenceChunk { delta, .. }) => {
                Some(SessionUpdate::AgentMessageChunk(ContentChunk::new(
                    ContentBlock::Text(TextContent::new(delta)),
                )))
            }
            TranslateInput::Record(SessionRecord::Tool(ToolRecord::Started { call })) => {
                let name = call.name.0.clone();
                let id = self.allocate_call_id(&name, &call.arguments);
                // The call line is a derivation, not data: derive it here, once
                // per record, exactly as the transcript entries do at build time.
                let call_line = call.call_line();
                let summary = plain_call_text(&call_line);
                let title = if summary.is_empty() {
                    name.clone()
                } else {
                    summary.to_string()
                };
                Some(SessionUpdate::ToolCall(
                    ToolCall::new(id, title)
                        .name(name.clone())
                        .kind(tool_kind(call.kind()))
                        .status(ToolCallStatus::Pending),
                ))
            }
            TranslateInput::Record(SessionRecord::Tool(ToolRecord::Completed {
                call,
                outcome,
            })) => {
                let id = self.take_call_id(&call.name.0, &call.arguments);
                let status = if outcome.status == ToolStatus::Success {
                    ToolCallStatus::Completed
                } else {
                    ToolCallStatus::Failed
                };
                // The drawer derives the body from the result data; ACP maps that
                // body onto its own content blocks.
                let mut content = call
                    .result_body(Some(&outcome.text))
                    .map(|display| content_from_display(&display))
                    .unwrap_or_default();
                if content.is_empty()
                    && let Some(message) = &outcome.message
                {
                    content.push(ToolCallContent::Content(Content::new(message.clone())));
                }
                let mut fields = ToolCallUpdateFields::new().status(status);
                if !content.is_empty() {
                    fields = fields.content(content);
                }
                Some(SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
                    id, fields,
                )))
            }
            TranslateInput::Record(SessionRecord::Inference(InferenceRecord::Completed {
                usage,
            })) => Some(SessionUpdate::UsageUpdate(
                // `UsageUpdate` takes a plain count, so a wire that omits the
                // total still reports 0 to ACP clients — unchanged behavior.
                UsageUpdate::new(usage.total_tokens.unwrap_or(0), self.max_context_tokens),
            )),
            TranslateInput::Record(SessionRecord::Compaction(CompactionRecord::Started {
                source,
            })) => Some(compaction_update("started", *source, None)),
            TranslateInput::Record(SessionRecord::Compaction(CompactionRecord::Completed {
                source,
                summary,
            })) => Some(compaction_update(
                "completed",
                *source,
                Some(summary_preview(summary)),
            )),
            TranslateInput::Record(SessionRecord::Compaction(CompactionRecord::Failed {
                source,
                message,
            })) => Some(compaction_update("failed", *source, Some(message.clone()))),
            TranslateInput::Record(SessionRecord::Compaction(CompactionRecord::Uncompacted {
                source,
            })) => Some(compaction_update("undone", *source, None)),
            // Mid-turn notices (context-window pressure, repetition steering)
            // reach the client as assistant text. The texts are self-describing,
            // so no prefix is added. A turn error is not a client-facing fact in
            // ACP mode: the turn's failure reaches the client as the prompt
            // response.
            TranslateInput::Narration(NarrationEvent::SystemMessage {
                kind: NoticeKind::Warning,
                text,
            }) => Some(SessionUpdate::AgentMessageChunk(ContentChunk::new(
                ContentBlock::Text(TextContent::new(text)),
            ))),
            // No ACP equivalent: streaming compaction deltas, turn lifecycle
            // markers, and permission events (the permission bridge owns
            // `PermissionRecord::Requested`).
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
    sources: TurnSources<'_>,
    sink: &S,
    translator: &mut EventTranslator,
    bridge: &mut AcpPermissionBridge<P>,
) where
    S: SessionUpdateSink,
    P: PermissionRequestSink,
{
    let mut stream_rx = sources.stream.subscribe();
    let mut narration_rx = sources.narration.subscribe();
    drain_receiver_to_notifications(&mut stream_rx, &mut narration_rx, sink, translator, bridge)
        .await;
}

/// Drain an existing bus receiver into `session/update` notifications until the
/// bus closes.
///
/// Use this variant when the caller owns the receiver, so the same receiver can
/// be flushed after the turn completes. Permission requests are routed to
/// `bridge`; every request still awaiting a decision when the loop ends is
/// denied.
pub async fn drain_receiver_to_notifications<S, P>(
    stream_rx: &mut Subscription<SessionRecord>,
    narration_rx: &mut Subscription<NarrationEvent>,
    sink: &S,
    translator: &mut EventTranslator,
    bridge: &mut AcpPermissionBridge<P>,
) where
    S: SessionUpdateSink,
    P: PermissionRequestSink,
{
    let mut stream_open = true;
    let mut narration_open = true;
    while stream_open || narration_open {
        tokio::select! {
            received = stream_rx.recv(), if stream_open => match received {
                Ok(record) => {
                    if !handle_record(sink, translator, bridge, &record).await {
                        break;
                    }
                }
                // One concern closed. The other may still hold records the
                // bus published before it dropped, so drain that one first.
                Err(ReceiveError::Closed) => stream_open = false,
                Err(ReceiveError::Lagged(dropped)) => {
                    log::warn!(
                        "session {}: stream receiver lagged, dropped {dropped} records",
                        translator.session_id()
                    );
                }
            },
            received = narration_rx.recv(), if narration_open => match received {
                Ok(event) => {
                    if !send_input(sink, translator, TranslateInput::Narration(&event)) {
                        break;
                    }
                }
                Err(ReceiveError::Closed) => narration_open = false,
                Err(ReceiveError::Lagged(dropped)) => {
                    log::warn!(
                        "session {}: narration receiver lagged, dropped {dropped} events",
                        translator.session_id()
                    );
                }
            },
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
    stream_rx: &mut Subscription<SessionRecord>,
    narration_rx: &mut Subscription<NarrationEvent>,
    sink: &S,
    translator: &mut EventTranslator,
) {
    loop {
        let more = match stream_rx.try_recv() {
            Ok(record) => send_input(sink, translator, TranslateInput::Record(&record)),
            Err(TryReceiveError::Lagged(dropped)) => {
                log::warn!(
                    "session {}: stream receiver lagged, dropped {dropped} records",
                    translator.session_id()
                );
                true
            }
            Err(TryReceiveError::Empty | TryReceiveError::Closed) => false,
        };
        if !more {
            break;
        }
    }
    loop {
        match narration_rx.try_recv() {
            Ok(event) => {
                send_input(sink, translator, TranslateInput::Narration(&event));
            }
            Err(TryReceiveError::Lagged(dropped)) => {
                log::warn!(
                    "session {}: narration receiver lagged, dropped {dropped} events",
                    translator.session_id()
                );
            }
            Err(TryReceiveError::Empty | TryReceiveError::Closed) => break,
        }
    }
}

/// Drive one prompt turn while streaming its bus events to the client.
///
/// `turn` is the `execute_turn()` future. The loop polls the turn and the bus
/// receivers together, so events reach the client as they are published. When
/// the turn completes, the receivers are flushed and the turn output is
/// returned.
///
/// `control_rx` carries the session's control intake. A compaction request is a
/// `ControlCommand`, not a record: it is the chain's auto-compaction decision,
/// addressed to the loop owner rather than to a client.
///
/// The returned flag reports whether a `ControlCommand::CompactionRequested`
/// arrived. The request is not a client-facing event: the caller dispatches it.
/// The flag is a `bool` because one
/// compaction runs per turn — a turn can trip the threshold on several
/// completions, and the extras are dropped: after the compaction the context
/// shrank, so the threshold re-evaluates on the next turn's completions.
///
/// The receivers must be subscribed before the turn starts, otherwise events
/// published before the subscription are lost. Permission requests are routed
/// to `bridge`; every request still awaiting a decision when the turn ends is
/// denied.
///
/// The drain is turn-scoped: an event published between turns reaches no
/// client, exactly like every ui event published between turns. That limit is
/// pre-existing and applies to compaction events unchanged.
pub async fn run_turn_with_notifications<S, P, F>(
    stream_rx: &mut Subscription<SessionRecord>,
    narration_rx: &mut Subscription<NarrationEvent>,
    control_rx: &mut ControlRx,
    sink: &S,
    translator: &mut EventTranslator,
    bridge: &mut AcpPermissionBridge<P>,
    turn: F,
) -> (F::Output, bool)
where
    S: SessionUpdateSink,
    P: PermissionRequestSink,
    F: Future,
{
    tokio::pin!(turn);
    let mut draining = true;
    let mut compaction_requested = false;
    let output = loop {
        tokio::select! {
            biased;
            output = &mut turn => break output,
            received = stream_rx.recv(), if draining => match received {
                Ok(record) => {
                    if !handle_record(sink, translator, bridge, &record).await {
                        draining = false;
                    }
                }
                Err(ReceiveError::Closed) => draining = false,
                Err(ReceiveError::Lagged(dropped)) => {
                    log::warn!(
                        "session {}: stream receiver lagged, dropped {dropped} records",
                        translator.session_id()
                    );
                }
            },
            received = narration_rx.recv(), if draining => match received {
                Ok(event) => {
                    if !send_input(sink, translator, TranslateInput::Narration(&event)) {
                        draining = false;
                    }
                }
                Err(ReceiveError::Closed) => draining = false,
                Err(ReceiveError::Lagged(dropped)) => {
                    log::warn!(
                        "session {}: narration receiver lagged, dropped {dropped} events",
                        translator.session_id()
                    );
                }
            },
            command = control_rx.recv(), if draining => match command {
                // The chain's auto-compaction decision: not a client-facing
                // event, so it is recorded rather than converted.
                Ok(ControlCommand::CompactionRequested { .. }) => compaction_requested = true,
                Ok(_) => {}
                Err(_receive_error) => draining = false,
            },
        }
    };
    if draining {
        flush_receiver_to_notifications(stream_rx, narration_rx, sink, translator);
        // A command the turn published before it completed is still queued:
        // the biased select breaks on the finished turn without reading it.
        while let Ok(command) = control_rx.try_recv() {
            if matches!(command, ControlCommand::CompactionRequested { .. }) {
                compaction_requested = true;
            }
        }
    }
    bridge.deny_all_pending();
    (output, compaction_requested)
}

// endregion: --- Public Functions

// region:    --- Support

/// Handle one stream record during a drain.
///
/// A permission request goes to the bridge — the translator has no ACP
/// equivalent for it. Everything else is translated and sent. Returns `false`
/// when the transport is gone.
async fn handle_record<S, P>(
    sink: &S,
    translator: &mut EventTranslator,
    bridge: &mut AcpPermissionBridge<P>,
    record: &SessionRecord,
) -> bool
where
    S: SessionUpdateSink,
    P: PermissionRequestSink,
{
    if let SessionRecord::Permission(requested @ PermissionRecord::Requested { .. }) = record {
        if let Err(err) = bridge.handle_permission_request(requested).await {
            log::warn!(
                "session {}: failed to handle permission request: {err}",
                translator.session_id()
            );
        }
        return true;
    }
    send_input(sink, translator, TranslateInput::Record(record))
}

/// Send one translated datum. Returns `false` when the transport is gone.
fn send_input<S: SessionUpdateSink>(
    sink: &S,
    translator: &mut EventTranslator,
    input: TranslateInput<'_>,
) -> bool {
    let Some(update) = translator.translate(input) else {
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

/// Map the kernel's tool kind to an ACP tool kind.
///
/// The kernel already classified the call: its name became a variant once, at
/// the boundary that produced the record. This is a total map over that enum,
/// so no name — and no name prefix — is read here.
fn tool_kind(kind: KernelToolKind) -> ToolKind {
    match kind {
        KernelToolKind::Builtin(
            BuiltinKind::Read
            | BuiltinKind::AstQuery
            | BuiltinKind::AstNodes
            | BuiltinKind::AstRefs
            | BuiltinKind::AstTree,
        ) => ToolKind::Read,
        KernelToolKind::Builtin(BuiltinKind::Edit) => ToolKind::Edit,
        KernelToolKind::Builtin(BuiltinKind::Grep | BuiltinKind::Glob) => ToolKind::Search,
        KernelToolKind::Builtin(
            BuiltinKind::Nu
            | BuiltinKind::TmuxSession
            | BuiltinKind::TmuxWindow
            | BuiltinKind::TmuxPane
            | BuiltinKind::TmuxLayout,
        ) => ToolKind::Execute,
        KernelToolKind::Builtin(BuiltinKind::Http) => ToolKind::Fetch,
        KernelToolKind::Builtin(
            BuiltinKind::Skill
            | BuiltinKind::SpawnAgent
            | BuiltinKind::TerminateAgent
            | BuiltinKind::SendMessage
            | BuiltinKind::ListAgents,
        )
        | KernelToolKind::Other => ToolKind::Other,
    }
}

/// The label this translator puts on a compaction's source.
fn source_label(source: CompactionSource) -> &'static str {
    match source {
        CompactionSource::Auto => "auto",
        CompactionSource::Manual => "slash",
    }
}

/// The head of a compaction summary: the client previews it, never the body.
fn summary_preview(summary: &str) -> String {
    summary.chars().take(200).collect()
}

/// Build a `session_info_update` carrying compaction state in `_meta`.
fn compaction_update(
    state: &str,
    source: CompactionSource,
    detail: Option<String>,
) -> SessionUpdate {
    let mut compaction = serde_json::Map::new();
    compaction.insert(
        "state".to_string(),
        serde_json::Value::String(state.to_string()),
    );
    compaction.insert(
        "source".to_string(),
        serde_json::Value::String(source_label(source).to_string()),
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
pub(crate) fn content_from_display(display: &Display) -> Vec<ToolCallContent> {
    display.sections.iter().map(section_content).collect()
}

/// Map one display section to ACP tool-call content.
///
/// A diff section becomes an ACP `Diff` carrying the file path from the
/// content kind. The section content is a unified diff, so the old and new
/// sides are reconstructed from the hunk lines — the reconstruction covers the
/// hunks only, not the untouched remainder of the file.
fn section_content(section: &DisplaySection) -> ToolCallContent {
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
