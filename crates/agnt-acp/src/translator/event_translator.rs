//! The record-to-ACP translation methods of [`EventTranslator`].

use std::collections::VecDeque;

use agent_client_protocol::schema::v1::{
    Content, ContentBlock, ContentChunk, SessionUpdate, TextContent, ToolCall, ToolCallContent,
    ToolCallId, ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields, UsageUpdate,
};
use agnt_kernel::{
    CompactionRecord, InferenceRecord, NoticeKind, SessionRecord, ToolRecord, ToolStatus,
};
use agnt_render::tool_args::plain_call_text;
use agnt_render::tool_view::ToolView;
use agnt_runtime::bus::NarrationEvent;
use agnt_runtime::config::defaults;

use super::types::{EventTranslator, PendingCall, TranslateInput};
use super::wire::{compaction_update, content_from_display, summary_preview, tool_kind};

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
            })) => Some(compaction_update("started", Some(*source), None)),
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
            })) => Some(compaction_update(
                "failed",
                Some(*source),
                Some(message.clone()),
            )),
            TranslateInput::Record(SessionRecord::Compaction(CompactionRecord::Uncompacted {
                source,
            })) => Some(compaction_update("undone", Some(*source), None)),
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
