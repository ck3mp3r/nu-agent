use nu_protocol::Span;

use crate::conversation::turn::TurnEnd;
use crate::orchestrator::events::OrchestratorEvent;
use crate::session::tool_status::ToolStatus;
use agnt_kernel::{
    CompactionRecord, CompactionSource, InferenceRecord, NoticeKind, NoticeRecord, PromptRecord,
    PromptStamps, SessionRecord, ToolCall, ToolOutcome, ToolRecord, ToolSource, TurnRecord, Usage,
};

/// The panels every surface shares — the kernel's own closed set, re-exported
/// so the surfaces keep one import path for it.
pub use agnt_kernel::bus::SharedUiAction;

/// The usability of one MCP server — the runtime's own vocabulary, re-exported
/// here so the contract module hands out every ui-facing type it names.
pub use crate::protocol::McpUsabilityState;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpToggleRequest {
    pub server_name: String,
    pub enable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiMessageSnapshot {
    role: String,
    content: String,
    tool_name: Option<String>,
    tool_arguments: Option<String>,
    tool_result: Option<String>,
    tool_success: Option<bool>,
    /// The call's structural status. A row the resolver reads carries the
    /// variant the store wrote; a row built without one carries the verdict its
    /// `tool_success` states.
    tool_status: Option<ToolStatus>,
    /// The submit instant the entry's turn recorded, when the entry is a user
    /// row whose key joined a settled turn. The recorded value, never a
    /// re-derivation.
    stamp_millis: Option<i64>,
    /// The model identity the entry's turn ran under.
    stamp_model: Option<String>,
    /// The persona identity that answered the entry's turn.
    stamp_agent: Option<String>,
    pub usage: Option<UiMessageUsageSnapshot>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UiMessageUsageSnapshot {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
}

impl UiMessageSnapshot {
    pub fn new(role: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: content.into(),
            tool_name: None,
            tool_arguments: None,
            tool_result: None,
            tool_success: None,
            tool_status: None,
            stamp_millis: None,
            stamp_model: None,
            stamp_agent: None,
            usage: None,
        }
    }

    /// Mark the snapshot with the footer facts its entry carries: the submit
    /// instant and the identities the turn ran under, verbatim.
    pub fn with_stamp(
        mut self,
        millis: Option<i64>,
        model: Option<String>,
        agent: Option<String>,
    ) -> Self {
        self.stamp_millis = millis;
        self.stamp_model = model;
        self.stamp_agent = agent;
        self
    }

    pub fn with_tool_details(
        mut self,
        arguments: Option<String>,
        result: Option<String>,
        success: Option<bool>,
    ) -> Self {
        self.tool_arguments = arguments;
        self.tool_result = result;
        self.tool_success = success;
        // The verdict the row states is the boolean fallback: a row the
        // store records a status for overrides it with `with_tool_status`.
        self.tool_status = success.map(|success| {
            if success {
                ToolStatus::Success
            } else {
                ToolStatus::Failed
            }
        });
        self
    }

    pub fn with_tool_name(mut self, name: String) -> Self {
        self.tool_name = Some(name);
        self
    }

    /// Mark the snapshot with the call's structural status: the variant the
    /// store carries for it.
    pub fn with_tool_status(mut self, status: ToolStatus) -> Self {
        self.tool_status = Some(status);
        self
    }

    pub fn role(&self) -> &str {
        &self.role
    }

    pub fn content(&self) -> &str {
        &self.content
    }

    pub fn tool_name(&self) -> Option<&str> {
        self.tool_name.as_deref()
    }

    pub fn tool_arguments(&self) -> Option<&str> {
        self.tool_arguments.as_deref()
    }

    pub fn tool_success(&self) -> Option<bool> {
        self.tool_success
    }

    /// The call's structural status, when the row carries one.
    pub fn tool_status(&self) -> Option<ToolStatus> {
        self.tool_status
    }

    pub fn usage(&self) -> Option<UiMessageUsageSnapshot> {
        self.usage
    }

    /// The recorded submit instant the row carries, when it carries one.
    pub fn stamp_millis(&self) -> Option<i64> {
        self.stamp_millis
    }

    /// The model identity the row carries, when it carries one.
    pub fn stamp_model(&self) -> Option<&str> {
        self.stamp_model.as_deref()
    }

    /// The persona identity the row carries, when it carries one.
    pub fn stamp_agent(&self) -> Option<&str> {
        self.stamp_agent.as_deref()
    }
}

impl UiMessageUsageSnapshot {
    pub fn input_tokens(&self) -> Option<u64> {
        self.input_tokens
    }

    pub fn output_tokens(&self) -> Option<u64> {
        self.output_tokens
    }

    pub fn total_tokens(&self) -> Option<u64> {
        self.total_tokens
    }
}

pub trait UserInputUi {
    fn event_sender(&self) -> &tokio::sync::mpsc::Sender<OrchestratorEvent>;
}

// region:    --- Projection

/// Project a persisted snapshot list onto the record log it represents.
///
/// The projection is the one bridge between the store's transport shape and the
/// stream's vocabulary, and it lives here — beside the snapshot type — so no
/// surface owns a second copy of these rules. One assistant snapshot becomes a
/// chunk carrying the whole text in both fields (a non-streaming producer sends
/// the full text) plus its completion; a tool snapshot becomes the started call
/// and its settled outcome; a compaction snapshot becomes one completion; a role
/// outside the resolver's closed set is a system line, replayed as a notice.
///
/// The second half of the return value is the session's displayed total, which
/// the persisted log does not itself carry.
pub fn project_snapshots(
    messages: impl IntoIterator<Item = UiMessageSnapshot>,
) -> (Vec<SessionRecord>, Option<u64>) {
    let mut records = Vec::new();
    let mut last_total_tokens = None;
    let mut turn_open = false;

    for message in messages {
        match message.role() {
            "user" => {
                if message.content().trim().is_empty() {
                    continue;
                }
                if turn_open {
                    push_turn_boundary(&mut records);
                }
                // The snapshot carries the facts the join recorded; the replay
                // passes them on verbatim, never filling one in.
                records.push(SessionRecord::Prompt(PromptRecord {
                    text: message.content().to_string(),
                    stamps: PromptStamps {
                        millis: message.stamp_millis(),
                        model: message.stamp_model().map(str::to_string),
                        agent: message.stamp_agent().map(str::to_string),
                    },
                    // A replayed prompt belongs to no live dispatch: the task
                    // that ran it finished before the log was written.
                    task_id: None,
                }));
                turn_open = true;
            }
            "assistant" => {
                let text = message.content();
                if text.trim().is_empty() {
                    continue;
                }
                let usage = usage_of(message.usage());
                last_total_tokens = usage.total_tokens.or(last_total_tokens);
                records.push(SessionRecord::Inference(InferenceRecord::Started));
                records.push(SessionRecord::Inference(InferenceRecord::Chunk {
                    delta: text.to_string(),
                    aggregated: text.to_string(),
                }));
                records.push(SessionRecord::Inference(InferenceRecord::Completed {
                    usage,
                }));
                turn_open = true;
            }
            "tool" => {
                let Some((call, outcome)) = tool_pair(&message) else {
                    continue;
                };
                records.push(SessionRecord::Tool(ToolRecord::Started {
                    call: call.clone(),
                }));
                records.push(SessionRecord::Tool(ToolRecord::Completed { call, outcome }));
                turn_open = true;
            }
            // A persisted display row names no call and no transcript fact: the
            // surface derives every display from the call it belongs to.
            "tool_display" => {}
            "compaction" => {
                records.push(SessionRecord::Compaction(CompactionRecord::Completed {
                    source: CompactionSource::Auto,
                    summary: message.content().to_string(),
                }));
                turn_open = true;
            }
            // A system line outside the resolver's closed set: the surface shows
            // it as a notice. An empty line carries no fact.
            _ => {
                let text = message.content().trim();
                if !text.is_empty() {
                    records.push(SessionRecord::Notice(NoticeRecord {
                        kind: NoticeKind::Warning,
                        text: text.to_string(),
                    }));
                }
            }
        }
    }

    if turn_open {
        push_turn_boundary(&mut records);
    }
    (records, last_total_tokens)
}

// endregion: --- Projection

// region:    --- Support

/// The turn-at-rest marker, pushed at every turn boundary.
///
/// The store persists messages, not turn boundaries: a user snapshot starts a
/// new turn, so every turn before it is complete. The record carries the one
/// figure a replay cannot reconstruct — the wall clock, which a replay reports
/// as zero.
fn push_turn_boundary(records: &mut Vec<SessionRecord>) {
    records.push(SessionRecord::Turn(TurnRecord::Completed {
        duration_ms: 0,
    }));
}

/// The usage one assistant snapshot reports. A snapshot that carries no usage
/// reports none of its token counters — the replay records what the wire would
/// have reported.
fn usage_of(persisted: Option<UiMessageUsageSnapshot>) -> Usage {
    Usage {
        input_tokens: persisted
            .and_then(|usage| usage.input_tokens())
            .unwrap_or(0),
        output_tokens: persisted
            .and_then(|usage| usage.output_tokens())
            .unwrap_or(0),
        total_tokens: persisted.and_then(|usage| usage.total_tokens()),
        // A persisted row carries no price: the log recorded the totals, not
        // what the call cost.
        cost: None,
        cost_complete: false,
    }
}

/// The replayed call one tool snapshot describes, and its settled outcome.
///
/// A snapshot without a name or without arguments names no call: the persisted
/// status line is no longer parsed, so such a row is dropped. The result the
/// store recorded is the call's data, so it rides the outcome as the payload a
/// drawer derives its body from and as the text the model received; a call with
/// no recorded result carries neither.
fn tool_pair(message: &UiMessageSnapshot) -> Option<(ToolCall, ToolOutcome)> {
    let name = message.tool_name()?;
    let arguments = message.tool_arguments()?;
    let call = ToolCall::new(name, ToolSource::Unknown, arguments);
    let result = message.tool_result.clone();
    let outcome = ToolOutcome {
        // The status the resolver recorded for the row. A snapshot carrying
        // none names no status; the replay rule makes such a row a failed
        // completion, the same rule the resolver applies.
        status: message.tool_status().unwrap_or(ToolStatus::Failed),
        payload: result.clone(),
        text: result.unwrap_or_default(),
        message: None,
        kind: None,
    };
    Some((call, outcome))
}

// endregion: --- Support

/// Minimal runtime required for a single turn. Used by run_single_turn.
pub trait CoreRuntime {
    fn execute_turn(
        &mut self,
        prompt: String,
        context: Option<String>,
        span: Span,
        // The external task that queued this prompt, or `None` for a user
        // submit. It is the turn's own spine attribution.
        task_id: Option<String>,
    ) -> impl std::future::Future<Output = TurnEnd> + Send;
}
