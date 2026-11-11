//! The live sequence for a tool call that never ran.
//!
//! A cancel lands before the dispatch sends `ToolStarted`, and a permission
//! denial stops the call before it executes. Both publish the same pair, so the
//! live transcript renders the call and the reason it was not run.

use agnt_kernel::bus::Stream;
use agnt_kernel::{
    SessionRecord, ToolCall as StreamToolCall, ToolErrorKind as StreamToolErrorKind,
    ToolOutcome as StreamToolOutcome, ToolRecord,
};

use crate::session::tool_status::ToolStatus;

/// The completion message for a call the cancel arm denied before it ran.
///
/// The call never executes, so there is no result to report — the message says
/// why the block is empty.
pub const CANCELLED_DISPATCH_MESSAGE: &str = "Interrupted: the turn stopped before this call ran";

/// The reason a user cancel carries.
///
/// One text, one place: the stop record, rig's stop reason, and the executor's
/// notice all name the same thing.
pub const CANCELLED_STOP_REASON: &str = "Cancelled by user";

/// Why a call never ran.
///
/// The kind is the event-level discriminator: a renderer classifies the
/// sequence as "did not run" without text-matching the message.
pub enum NotRunOutcome<'a> {
    /// The turn stopped before the call ran.
    Cancelled,
    /// The permission policy denied the call; the reason is the message.
    Denied { reason: &'a str },
}

/// How the not-run emitter publishes `ToolStarted`.
pub enum NotRunStart {
    /// Publish `ToolStarted` for this call.
    Publish,
    /// The caller already published `ToolStarted` — the deny arm sends it
    /// before permission resolution so the permission card anchors to the
    /// call.
    AlreadyPublished,
}

/// The call a not-run emission publishes.
pub struct NotRunCall<'a> {
    /// The tool's name.
    pub tool_name: &'a str,
    /// The tool's source.
    pub source: crate::tools::handler::ToolSource,
    /// The raw JSON arguments.
    pub arguments: &'a str,
    /// How `ToolStarted` reaches the bus.
    pub start: NotRunStart,
}

/// Publish the live sequence for a call that never ran: `ToolStarted` (unless
/// the caller already published it), then `ToolCompleted` carrying the
/// outcome's kind and message.
///
/// The cancel arm returns `stop` before the `ToolStarted` send, and rig fails
/// the dispatch before `on_outcome` fires — so without this sequence the live
/// transcript renders nothing for the denied call. The permission-deny branch
/// publishes the same sequence; only the outcome differs.
///
/// The executor's cancel arm shares this shape for the batch's siblings the
/// stop left undispatched, so both paths emit one identical sequence.
///
/// The gate card content rides the gate's own record, never this pair: a
/// `PermissionRecord` is the call's card, and the drawer joins it by the call
/// id it names.
pub async fn emit_not_run_dispatch(
    stream: &Stream,
    call: NotRunCall<'_>,
    outcome: NotRunOutcome<'_>,
) {
    let NotRunCall {
        tool_name,
        source,
        arguments,
        start,
    } = call;
    let publish_start = matches!(start, NotRunStart::Publish);
    if publish_start {
        let _ = stream.publish(SessionRecord::Tool(ToolRecord::Started {
            call: StreamToolCall::new(tool_name, source, arguments),
        }));
    }
    let (message, kind) = match outcome {
        NotRunOutcome::Cancelled => (CANCELLED_DISPATCH_MESSAGE, StreamToolErrorKind::Cancelled),
        NotRunOutcome::Denied { reason } => (reason, StreamToolErrorKind::Denied),
    };
    let _ = stream.publish(SessionRecord::Tool(ToolRecord::Completed {
        call: StreamToolCall::new(tool_name, source, arguments),
        outcome: StreamToolOutcome {
            status: ToolStatus::Cancelled,
            payload: None,
            text: String::new(),
            message: Some(message.to_string()),
            kind: Some(kind),
        },
    }));
}
