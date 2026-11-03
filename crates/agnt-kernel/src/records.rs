//! The session record vocabulary — the typed entries of one ordered stream.
//!
//! A stream record is the log of what happened in a session plus
//! current-truth publications. Records are never commands and never render
//! instructions: surfaces project whichever records they care about.

// region:    --- Types

/// The typed envelope of the session stream. The stream ring delivers these
/// in publish order; the reducer of every consumer folds this vocabulary.
#[derive(Debug, Clone, PartialEq)]
pub enum SessionRecord {
    Inference(InferenceRecord),
    Tool(ToolRecord),
    Permission(PermissionRecord),
    Compaction(CompactionRecord),
    Notice(NoticeRecord),
    Session(SessionLifecycle),
    Turn(TurnRecord),
}

/// The inference pipeline of one completion call.
#[derive(Debug, Clone, PartialEq)]
pub enum InferenceRecord {
    /// The completion call started.
    Started,
    /// One streamed delta. `aggregated` accumulates from the call start.
    Chunk {
        /// The newly received text.
        delta: String,
        /// The accumulated response text so far.
        aggregated: String,
    },
    /// The completion finished and the totals attached to it arrived.
    Completed { usage: Usage },
    /// The completion was stopped before it finished: a hook, a guard, or the
    /// user cancelled the run. The call narrates [`InferenceRecord::Stopped`]
    /// instead of [`InferenceRecord::Completed`]; the turn itself still
    /// completes.
    Stopped {
        /// Why the run stopped, as a closed enum — never read from text.
        cause: StopCause,
        /// The stop's content text, for the surfaces that show it.
        reason: String,
    },
}

/// Why a completion call stopped early. Producers know the cause and emit the
/// variant; no consumer ever infers a cause by inspecting text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopCause {
    /// The user cancelled the run.
    UserCancelled,
    /// The doom-loop guard stopped the stream.
    DoomLoop,
    /// The output-repetition guard stopped the stream.
    OutputRepetition,
    /// Any other stop that produced no completion result.
    Other,
}

/// The totals of one inference call.
///
/// Carries the priced cost so consumers showing usage do not have to join the
/// stream with runtime accounting. `PartialEq` only: prices are floats.
#[derive(Debug, Clone, PartialEq)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// The total the wire reported, when it reported one.
    pub total_tokens: Option<u64>,
    /// What the call cost in USD, when anything priced it.
    pub cost: Option<f64>,
    /// Whether `cost` is the whole charge rather than a lower bound.
    pub cost_complete: bool,
}

/// The turn pipeline of one submitted prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnRecord {
    /// The turn finished; the usage totals arrive on
    /// [`InferenceRecord::Completed`] before this record. Call counts are not
    /// carried: they fold from the turn's [`ToolRecord`]s (dispatched = the
    /// started records; executed = the completions that are not
    /// [`ToolStatus::Cancelled`]).
    Completed {
        /// Wall-clock time of the turn — the one fact records cannot fold.
        duration_ms: u64,
    },
}

/// The tool-call pipeline. Every dispatched call narrates a started record and
/// exactly one completed record — including calls the permission gate denies:
/// they dispatch, are decided against, and settle as [`ToolStatus::Cancelled`]
/// without running. The started record is published before the permission
/// decision is requested, so the pair brackets the whole call lifecycle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolRecord {
    /// The call was dispatched.
    Started { call: ToolCall },
    /// The call settled; `outcome` carries the disposition and the content.
    Completed {
        call: ToolCall,
        outcome: ToolOutcome,
    },
}

/// One tool call, as the stream identifies it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCall {
    /// Derived by [`ToolCallId::new`] from `name` and `arguments`, and carried
    /// so consumers never recompute the anchor: the id cannot drift between a
    /// live call and a replayed record.
    pub id: ToolCallId,
    pub name: String,
    /// Where the tool comes from (builtin, closure, or an MCP server name).
    pub source: String,
    /// The raw arguments JSON — the other half of the call key.
    pub arguments: String,
}

impl ToolCall {
    /// A call with its id derived by the one derivation.
    pub fn new(
        name: impl Into<String>,
        source: impl Into<String>,
        arguments: impl Into<String>,
    ) -> Self {
        let name = name.into();
        let source = source.into();
        let arguments = arguments.into();
        let id = ToolCallId::new(&name, &arguments);
        Self {
            id,
            name,
            source,
            arguments,
        }
    }
}

/// The exact key of a tool call: `{name}\n{arguments}`.
///
/// One constructor derives it, so the key cannot drift between producers, the
/// permission gate, and a replayed transcript.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCallId(String);

impl ToolCallId {
    /// The key of the call with `name` and `arguments`.
    pub fn new(name: &str, arguments: &str) -> Self {
        Self(format!("{name}\n{arguments}"))
    }

    /// Adopt a canonical key a producer already holds (the permission gate
    /// carries the call key, not its parts). Prefer [`ToolCall::new`] whenever
    /// the parts are at hand; this constructor exists only for that boundary.
    pub fn from_key(key: impl Into<String>) -> Self {
        Self(key.into())
    }

    /// The key as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The structural disposition of a settled call.
///
/// The variant decides; the text fields are content and are never read as
/// status. Every call that produced no result — a gate denial, a turn cancel,
/// a wall clock — is [`ToolStatus::Cancelled`]; what differs between them is
/// content, and content is not a disposition.
///
/// Serde is present because the session store persists this status per call;
/// the stored shape is the same three dispositions, never a re-derived one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolStatus {
    /// The call ran and succeeded.
    Success,
    /// The call ran and failed.
    Failed,
    /// The call produced no result.
    Cancelled,
}

/// Why a call has no result — the classification a surface prints beside it.
///
/// The variants mirror the labels surfaces already render, one per stable name,
/// so a record carries what the UI shows and a surface never parses a message to
/// learn it. [`Cancelled`](ToolErrorKind::Cancelled) is every call that produced
/// no result at all (a turn stop, a wall clock, a gate refusal of the run); [`Denied`](ToolErrorKind::Denied)
/// is a gate refusing this call specifically.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolErrorKind {
    /// The call's arguments could not be decoded or validated.
    InvalidArgs,
    /// The call exceeded its deadline.
    Timeout,
    /// The call produced no result: something stopped it first.
    Cancelled,
    /// The requested tool or resource was not found.
    NotFound,
    /// An authorization check refused the call.
    PermissionDenied,
    /// A rate limit was reached.
    RateLimited,
    /// An upstream provider failed.
    Provider,
    /// A network operation failed.
    Network,
    /// Any other failure.
    Other,
    /// The permission gate refused this call.
    Denied,
}

/// One settled call: its disposition, its content, and why it has no result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolOutcome {
    pub status: ToolStatus,
    /// The call's result content.
    pub result: String,
    /// The handler's or gate's own message (a denial reason, a failure detail).
    pub message: Option<String>,
    /// Why the call has no result. `None` for a call that produced one.
    pub kind: Option<ToolErrorKind>,
}

/// Identity of one permission request. Assigned by the producing gate; opaque
/// to the kernel and to consumers alike.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestId(String);

impl RequestId {
    /// The request id the gate assigned.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// The id as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The authorization pipeline. A prompted call narrates `Requested` and then
/// exactly one resolution (`Decided`, `TimedOut`, `Ignored`, or `Cancelled`); a
/// call a policy resolved without prompting narrates `AutoResolved` and no
/// `Requested`. Authorization records are the never-evicted class of the
/// stream: losing one would strand a pause point.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionRecord {
    /// A permission card was opened for `call_id`.
    Requested {
        request_id: RequestId,
        call_id: ToolCallId,
        /// The tool's display identity as the gate spells it (a decorated name —
        /// matching on call identity uses `call_id`, never this).
        tool: String,
        source: String,
        /// The rule that matched, as data.
        rule_identity: String,
        scope: String,
        pattern: String,
        target_field: Option<String>,
        mode: Option<String>,
        /// The question the card asks, as content text.
        summary: String,
    },
    /// A decision was submitted for the request.
    Decided {
        request_id: RequestId,
        decision: PermissionDecision,
        rule_identity: String,
    },
    /// The request expired without a decision (`ask` means deny on timeout).
    TimedOut { request_id: RequestId },
    /// A submitted decision was discarded.
    Ignored {
        request_id: RequestId,
        reason: String,
    },
    /// The request was abandoned because the turn was cancelled.
    Cancelled {
        request_id: RequestId,
        rule_identity: String,
    },
    /// The policy resolved the call without prompting.
    AutoResolved {
        rule_identity: String,
        action: AutoResolution,
        reason: Option<String>,
    },
}

/// What a user (or surface) answered to a permission card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionDecision {
    AllowOnce,
    AllowAlways,
    Deny,
}

/// The action a policy resolved a call to without prompting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoResolution {
    Allow,
    Deny,
}

/// A user-facing notice: enumerated kind plus content text. The kind decides
/// how it is treated; the text is content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoticeRecord {
    pub kind: NoticeKind,
    pub text: String,
}

/// What kind of notice this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeKind {
    Warning,
    TurnError,
}

/// Where a compaction came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactionSource {
    /// The conversation crossed the compaction threshold on its own.
    Auto,
    /// Something asked for the compaction explicitly.
    Manual,
}

/// The compaction pipeline: the transcript-mutating sweep. A compaction that
/// runs brackets its summary stream (`Started` … chunks … `Completed`); one
/// that fails narrates `Failed` instead of `Completed`. The summary text is the
/// transcript content that replaced the swept region.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompactionRecord {
    /// A compaction was decided for the session.
    Requested { source: CompactionSource },
    /// The compaction sweep started.
    Started { source: CompactionSource },
    /// One streamed delta of the compaction summary; `aggregated` accumulates
    /// from the sweep start.
    SummaryChunk {
        source: CompactionSource,
        delta: String,
        aggregated: String,
    },
    /// The sweep finished; `summary` is the text that replaced the region.
    Completed {
        source: CompactionSource,
        summary: String,
    },
    /// The sweep failed; the region is untouched and `message` says why.
    Failed {
        source: CompactionSource,
        message: String,
    },
}

/// Identity of one session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionId(String);

impl SessionId {
    /// The session identity as the store keys it.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// The id as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The session lifecycle: identity, identity switches, and the close. Session
/// usage totals are not carried — they fold from the session's inference
/// records, the same way turn-level usage does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionLifecycle {
    /// The session opened; `hydrated` says whether prior history loaded.
    Started {
        session_id: SessionId,
        hydrated: bool,
    },
    /// The session closed. Totals fold from inference records.
    Ended { session_id: SessionId },
    /// The active session switched.
    Switched {
        from: Option<SessionId>,
        to: SessionId,
    },
    /// The active agent (persona) switched.
    AgentSwitched { from: Option<String>, to: String },
    /// The active model switched.
    ModelSwitched { from: String, to: String },
}

// endregion: --- Types
