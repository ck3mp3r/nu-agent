//! The event vocabularies the bus channels carry.
//!
//! Four channel contracts, one vocabulary each:
//!
//! - [`ControlCommand`] — commands addressed to the loop owner. Never dropped;
//!   a command that is lost is a command that never ran.
//! - [`LifecycleEvent`] — the ordered, complete turn spine plus the session and
//!   compaction sequences. Never dropped: a missing spine step cannot be
//!   re-derived, and the attribution map consumes the whole sequence.
//! - [`NarrationEvent`] — text for a human to read. Order-critical with an
//!   oldest-droppable tail: a consumer that lags skips whole records and
//!   resumes in receipt order.
//! - [`FactRecord`] — latest-wins snapshots of kernel state. Every record is a
//!   whole value for its field, so a consumer that lags loses intermediate
//!   snapshots and never a state: folding the surviving suffix reaches the
//!   latest state.
//!
//! The process, turn, session, and external enums below predate the contract
//! and retire into [`LifecycleEvent`] as its publishers move.

use crate::records::{
    CompactionSource, InferenceId, NoticeKind, PermissionDecision, PermissionRecord, RequestId,
    SummaryStage, Usage,
};

// region:    --- Types

/// A process-wide agent lifecycle event.
///
/// Agent events are per-process, not per-session: one `Started`/`Shutdown` pair
/// per process lifetime. They ride a standalone channel created at mode entry,
/// never the per-runtime `Bus` — a bus is cloned per session, so a
/// process-scoped event on it would be a category error.
#[derive(Debug, Clone)]
pub enum AgentEvent {
    /// The agent process started. `mode` is "tui", "stderr", or "acp".
    Started { mode: String },
    /// The agent process is shutting down. `reason` is "completed", "error",
    /// or "shutdown".
    Shutdown { reason: String },
}

/// The cancellation vocabulary.
///
/// More variants would require a delivery-contract review; cancellation has
/// none today.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CancelEvent {
    /// The current task was cancelled.
    Requested,
}

/// A command addressed to the loop owner.
///
/// Control carries work to be done, never state to be shown. It is published
/// on a never-dropped channel: a command that is lost is a command that never
/// ran. The enum grows by adding variants — the first is
/// [`EnqueueExternalPrompt`](ControlCommand::EnqueueExternalPrompt).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlCommand {
    /// A prompt arrived from outside the surface. The loop queues it and
    /// submits it carrying the task id that attributes its turn.
    EnqueueExternalPrompt { task_id: String, prompt: String },
    /// A surface answered the open permission card.
    PermissionDecided {
        request_id: RequestId,
        decision: PermissionDecision,
    },
    /// A surface asked for a compaction sweep (`/compact`), never the automatic ladder.
    CompactionRequested { source: CompactionSource },
    /// A surface asked to undo the newest compaction (`/uncompact`).
    UncompactRequested { source: CompactionSource },
    /// A surface asked for one of its own panels to open.
    SharedUiAction { action: SharedUiAction },
}

/// The panels every surface shares.
///
/// The set is closed and enumerated kernel-side because the request is a
/// control command: the surface receiving it owns what a panel looks like and
/// how it opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SharedUiAction {
    Help,
    Status,
    Mcps,
    Models,
    Agents,
    Sessions,
    Themes,
    Skills,
    RenderStats,
}

/// One ordered, complete lifecycle sequence.
///
/// The turn spine is `PromptSubmitted` → `TurnStarted` → `TurnCompleted`,
/// every step carrying the task id it belongs to; that sequence is what the
/// attribution map consumes. Session and compaction share the channel because
/// they are the same kind of record: ordered, complete, and never re-derivable
/// from anything else.
#[derive(Debug, Clone, PartialEq)]
pub enum LifecycleEvent {
    /// A queued prompt was submitted to the turn executor.
    PromptSubmitted {
        text: String,
        task_id: Option<String>,
        stamps: PromptStamps,
    },
    /// The turn for the submitted prompt started running.
    TurnStarted {
        prompt: String,
        task_id: Option<String>,
    },
    /// The turn for the submitted prompt ended.
    TurnCompleted {
        task_id: Option<String>,
        duration_ms: u64,
    },
    /// An external task ended with its output.
    TaskCompleted { task_id: String, output: String },
    /// An external task failed or was cancelled.
    TaskFailed { task_id: String, error: String },
    /// The session opened.
    SessionStarted { session_id: String, hydrated: bool },
    /// The session closed. Totals are those of the session's turns.
    SessionEnded {
        session_id: String,
        input_tokens: u64,
        output_tokens: u64,
        total_tokens: Option<u64>,
        cost: Option<f64>,
        cost_complete: bool,
    },
    /// The active session switched.
    SessionSwitched { from: Option<String>, to: String },
    /// The active agent switched. `from` is absent when none was loaded.
    AgentSwitched { from: Option<String>, to: String },
    /// The active model switched.
    ModelSwitched { from: String, to: String },
    /// A compaction sweep started.
    CompactionStarted { source: CompactionSource },
    /// A compaction sweep finished; `summary` replaced the swept region.
    CompactionCompleted {
        source: CompactionSource,
        summary: String,
    },
    /// A compaction sweep failed; the region is untouched.
    CompactionFailed {
        source: CompactionSource,
        message: String,
    },
    /// The newest compaction was undone; the entries it replaced are back.
    Uncompacted { source: CompactionSource },
    /// The uncompaction finished with nothing restored; the state is untouched.
    UncompactFailed {
        source: CompactionSource,
        message: String,
    },
}

/// The footer facts a prompt submission carries.
///
/// The submit instant and the identities active when the prompt was accepted.
/// An unknown fact stays absent — the record is what the boundary knew. It
/// lives with the records because the stream's prompt row carries it; the
/// spine's `PromptSubmitted` carries the same value.
pub use crate::records::PromptStamps;

/// Text for a human to read.
///
/// Narration is order-critical with an oldest-droppable tail: a consumer that
/// falls behind skips whole records and resumes in receipt order. Chunk records
/// carry the aggregated text, so a skipped chunk costs no content.
#[derive(Debug, Clone, PartialEq)]
pub enum NarrationEvent {
    /// An inference call started.
    InferenceStarted,
    /// One streamed delta. `aggregated` accumulates from the call start.
    InferenceChunk {
        /// The call this delta belongs to; the record twin carries the same id.
        inference: InferenceId,
        delta: String,
        aggregated: String,
    },
    /// The inference call finished; `usage` carries every reported total.
    InferenceCompleted { usage: Usage },
    /// A hook or the user stopped the run.
    InferenceStopped { reason: String },
    /// One streamed delta of a compaction summary, of the pass `stage` names.
    CompactionSummaryChunk {
        source: CompactionSource,
        stage: SummaryStage,
        delta: String,
        aggregated: String,
    },
    /// A message for the user: a warning or a turn error.
    SystemMessage { kind: NoticeKind, text: String },
}

/// A latest-wins snapshot of kernel state.
///
/// Every record is a whole value for its field family, never a delta: a
/// consumer that lags loses intermediate snapshots and never a state, so
/// folding the surviving suffix reaches the latest state. Facts carry state
/// only — no commands, no prompt content, and nothing a caller waits on.
///
/// `PartialEq` only: usage totals carry floats. No variant needs a total order.
#[derive(Debug, Clone, PartialEq)]
pub enum FactRecord {
    /// What the worker is doing now.
    WorkerPhase(WorkerPhase),
    /// The permission gate's current state.
    PermissionGate(PermissionGate),
    /// The session's cumulative usage — the sum of every call the session ran.
    /// A reset publishes a zeroed snapshot.
    SessionUsage(Usage),
    /// The model identity now active.
    ActiveModelIdentity(String),
    /// The agent identity now active.
    ActiveAgentIdentity(String),
    /// The persona icon now active.
    ActivePersonaIcon(Option<String>),
    /// The active model's context-window ceiling.
    ContextWindowMaxTokens(Option<u64>),
    /// One MCP server's usability, with the reason it failed when it did.
    McpServerState {
        server: String,
        state: McpServerUsability,
        /// Why the server is unusable, when the producer knows. It belongs to
        /// the state it explains, so a later snapshot replaces both together.
        error: Option<String>,
    },
    /// How many tools the MCP servers expose to the model, all servers
    /// counted together.
    McpVisibleToolTotal(usize),
    /// How many tools one MCP server exposes to the model.
    McpVisibleToolCount { server: String, count: usize },
    /// Which tools one MCP server exposes to the model.
    McpVisibleToolNames { server: String, names: Vec<String> },
    /// Whether the compaction commands can run now: `/compact` once a sweep has
    /// something to cut, `/uncompact` while a summary waits to be put back. One
    /// read answers both, so no surface can see the two disagree.
    CommandAvailability(CommandFacts),
}

/// What the worker is doing now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerPhase {
    /// No turn is running.
    Idle,
    /// A turn is running.
    Busy,
    /// A turn is running and a cancellation is armed.
    AbortPending,
}

/// The permission gate's state, as one snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionGate {
    /// No card is open.
    Idle,
    /// A card is open; the resolver waits for the answer. The payload is the
    /// request record the gate published, so the card is drawn from exactly
    /// the data the stream recorded — a [`PermissionRecord::Requested`].
    ///
    /// Boxed because one card is 280 bytes while every other fact is a scalar:
    /// the snapshot family stays one size, and the card is read through the box.
    Prompting(Box<PermissionRecord>),
    /// A policy resolved the call without prompting. The payload is the record
    /// the stream carries for the same resolution, so the without-prompting
    /// card is drawn from exactly that data — a
    /// [`PermissionRecord::AutoResolved`]. Boxed for the same reason as
    /// [`PermissionGate::Prompting`].
    AutoResolved(Box<PermissionRecord>),
}

/// How usable one MCP server is right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpServerUsability {
    Enabled,
    Disabled,
    Failed,
}

/// Whether one gated command can run now, and why not when it cannot.
///
/// The variant is the whole disposition: a consumer that acts on the difference
/// matches it, and the reason is content — the producer knows why, and the layer
/// that prints it owns the sentence it shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompactionAvailability {
    /// The command would do its work now.
    Available,
    /// The command has nothing to do, for the reason carried.
    Unavailable(String),
}

/// The compaction commands' availability, answered by one read of the session's
/// conversation: one field per gated command.
///
/// `/compact` needs something a sweep would summarize; `/uncompact` needs a
/// leading summary to put back. The two answers are near-complements over the
/// same state, and they travel as one fact, so every surface folds the same read.
/// A third gated command adds one field here and one arm on each surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandFacts {
    /// Whether a sweep would summarize anything.
    pub compact: CompactionAvailability,
    /// Whether a summary waits to be put back.
    pub uncompact: CompactionAvailability,
}

// endregion: --- Types

// region:    --- Defaults

/// What a surface displays for `/compact` before the runtime has read the
/// session: nothing waits to be summarized yet, so a sweep has nothing to cut.
///
/// The sentence is the kernel's own. The runtime's predicate carries the sweep's
/// text in the runtime crate, which the kernel must not see; the pre-fact window
/// is the producer of this text.
const NOTHING_TO_COMPACT_YET: &str = "Nothing to compact: the session holds no conversation yet.";

/// What a surface displays for `/uncompact` before the runtime has read the
/// session: nothing was compacted yet, so there is nothing to put back.
const NOTHING_TO_UNDO_YET: &str = "Nothing to undo: the session holds no compaction yet.";

impl Default for CommandFacts {
    /// The pre-fact display truth: before the runtime answers, neither command is
    /// offered, because a session nothing has been read from has nothing to
    /// compact and nothing to undo.
    fn default() -> Self {
        Self {
            compact: CompactionAvailability::Unavailable(NOTHING_TO_COMPACT_YET.to_string()),
            uncompact: CompactionAvailability::Unavailable(NOTHING_TO_UNDO_YET.to_string()),
        }
    }
}

// endregion: --- Defaults
