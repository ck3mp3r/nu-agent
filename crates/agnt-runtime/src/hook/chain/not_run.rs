//! The live sequence for a tool call that never ran.
//!
//! A cancel lands before the dispatch sends `ToolStarted`, and a permission
//! denial stops the call before it executes. Both publish the same pair, so the
//! live transcript renders the call and the reason it was not run.

use std::path::Path;

use agnt_kernel::bus::Stream;
use agnt_kernel::{
    SessionRecord, ToolCall as StreamToolCall, ToolErrorKind as StreamToolErrorKind,
    ToolOutcome as StreamToolOutcome, ToolRecord,
};

use crate::bus::UiEventTx;
use crate::protocol::events::{SystemEvent, ToolCall, ToolOutcome};
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

/// The error kind the cancel arm's emissions carry.
///
/// The kind is the event-level discriminator: a renderer classifies the pair as
/// "did not run" without text-matching the message.
const CANCELLED_ERROR_KIND: &str = "cancelled";

/// The error kind the permission-deny branch's emission carries.
///
/// A denial is the same "did not run" class as a cancel: the kind is the
/// discriminator, the message carries the reason.
const DENIED_ERROR_KIND: &str = "denied";

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
/// the caller already published it), then the call's argument-sourced preview
/// when `preview` is set and the tool has one, then
/// `ToolCompleted(success=false)` carrying the outcome's kind and message.
///
/// The cancel arm returns `stop` before the `ToolStarted` send, and rig fails
/// the dispatch before `on_outcome` fires — so without this sequence the live
/// transcript renders nothing for the denied call. The permission-deny branch
/// publishes the same sequence; only the outcome differs.
///
/// The executor's cancel arm shares this shape for the batch's siblings the
/// stop left undispatched, so both paths emit one identical sequence.
///
/// The TUI anchors a preview to a call in `InProgress`, so the preview must
/// land after `ToolStarted` and before `ToolCompleted` — the same order the
/// deny path and rehydration produce.
pub async fn emit_not_run_dispatch(
    ui_event: &UiEventTx,
    stream: &Stream,
    cwd: &Path,
    call: NotRunCall<'_>,
    outcome: NotRunOutcome<'_>,
    preview: bool,
) {
    let NotRunCall {
        tool_name,
        source,
        arguments,
        start,
    } = call;
    let publish_start = matches!(start, NotRunStart::Publish);
    let call = ToolCall::new(tool_name.to_string(), source, arguments);
    if publish_start {
        let _ = ui_event
            .send(SystemEvent::ToolStarted { call: call.clone() })
            .await;
        let _ = stream.publish(SessionRecord::Tool(ToolRecord::Started {
            call: StreamToolCall::new(tool_name, source, arguments),
        }));
    }
    if preview && let Some(card) = gate_card_for(tool_name, arguments, cwd) {
        // The gate has a card for this call. The event is the signal; `card`
        // carries only the content a surface cannot derive (an edit's proposed
        // diff). A surface derives the rest from the call's arguments.
        let _ = ui_event
            .send(SystemEvent::PreviewRequested {
                call: call.clone(),
                preview: card,
            })
            .await;
    }
    let (error_kind, message, kind) = match outcome {
        NotRunOutcome::Cancelled => (
            CANCELLED_ERROR_KIND,
            CANCELLED_DISPATCH_MESSAGE,
            StreamToolErrorKind::Cancelled,
        ),
        NotRunOutcome::Denied { reason } => {
            (DENIED_ERROR_KIND, reason, StreamToolErrorKind::Denied)
        }
    };
    let _ = ui_event
        .send(SystemEvent::ToolCompleted {
            call,
            outcome: ToolOutcome {
                success: false,
                // Both arms are the one non-completed class: the call never
                // reached a result. The kind and the message stay content.
                status: ToolStatus::Cancelled,
                result: String::new(),
                error_kind: Some(error_kind.to_string()),
                message: Some(message.to_string()),
            },
        })
        .await;
    // The same pair on the record stream: a call the turn never dispatched is
    // still a dispatched call to every consumer folding records.
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

// region:    --- Support

/// The gate card a call has, as its preview content: `Some(Some(diff))` for an
/// edit (the producer plans the diff against the file on disk), `Some(None)` for
/// a tool whose card the drawer derives from the arguments (a `nu` command), and
/// `None` when the tool has no card at all.
fn gate_card_for(
    tool_name: &str,
    arguments: &str,
    cwd: &std::path::Path,
) -> Option<Option<String>> {
    let arguments: serde_json::Value = serde_json::from_str(arguments).ok()?;
    let kind = tool_name.parse::<agnt_kernel::BuiltinKind>().ok();
    crate::tools::handler::pre_authorize_fs_tool(kind, arguments.as_object()?, cwd)
        .map(|output| output.preview)
}

// endregion: --- Support
