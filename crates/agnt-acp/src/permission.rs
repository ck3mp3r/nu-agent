//! Permission bridge between nu-agent's permission flow and ACP
//! `session/request_permission`.
//!
//! nu-agent publishes `PermissionRecord::Requested` on the session stream and
//! awaits the decision on a oneshot registered in `PendingPermissions`. In ACP
//! mode the editor is the permission UI: the bridge forwards the request to the
//! client as `session/request_permission`, maps the client's outcome to a
//! `PermissionDecision`, and resolves the pending oneshot.
//!
//! The bridge reads the request record directly. The record is the source: its
//! `call_id` is the call identity, its `shows_preview`/`preview` carry the
//! content only the producer could compute, and no intermediate view type
//! exists.

// region:    --- Modules

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::sync::{MutexGuard, PoisonError};

use agent_client_protocol::schema::v1::{
    PermissionOption, PermissionOptionKind, RequestPermissionOutcome, RequestPermissionRequest,
    SessionId, ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields,
};
use agent_client_protocol::{Client, ConnectionTo};
use agnt_kernel::{PermissionRecord, ToolCall};
use agnt_render::tool_view::ToolView;
use agnt_render::transcript::ir::ToolName;
use agnt_runtime::bus::{ControlCommand, ControlTx, OneshotTx};
use agnt_runtime::conversation::runtime::PendingPermissions;
use agnt_runtime::protocol::event::PermissionDecision;
use agnt_runtime::tools::handler::ToolSource;

use crate::error::{AcpError, Result};
use crate::translator::content_from_display;

// endregion: --- Modules

// region:    --- Types

/// Destination for ACP `session/request_permission` calls.
///
/// Implemented by the ACP connection. The trait exists so the bridge can be
/// driven by a test sink without a live transport.
pub trait PermissionRequestSink {
    /// Ask the client to authorize one tool call and return its outcome.
    fn send_request(
        &self,
        session_id: &str,
        tool_call: ToolCallUpdate,
        options: Vec<PermissionOption>,
    ) -> impl Future<Output = RequestPermissionOutcome> + Send;
}

impl PermissionRequestSink for ConnectionTo<Client> {
    fn send_request(
        &self,
        session_id: &str,
        tool_call: ToolCallUpdate,
        options: Vec<PermissionOption>,
    ) -> impl Future<Output = RequestPermissionOutcome> + Send {
        let request = RequestPermissionRequest::new(SessionId::new(session_id), tool_call, options);
        async move {
            match ConnectionTo::<Client>::send_request(self, request)
                .block_task()
                .await
            {
                Ok(response) => response.outcome,
                Err(err) => {
                    log::warn!("session/request_permission failed: {err}");
                    RequestPermissionOutcome::Cancelled
                }
            }
        }
    }
}

/// A shared reference to a sink is itself a sink.
///
/// Lets a caller hold one sink and hand it to both the update stream and the
/// permission bridge without cloning the transport handle.
impl<T: PermissionRequestSink + ?Sized> PermissionRequestSink for &T {
    fn send_request(
        &self,
        session_id: &str,
        tool_call: ToolCallUpdate,
        options: Vec<PermissionOption>,
    ) -> impl Future<Output = RequestPermissionOutcome> + Send {
        (**self).send_request(session_id, tool_call, options)
    }
}

/// Bridges nu-agent permission requests to ACP `session/request_permission`.
///
/// Generic over the sink, so the transport is chosen at compile time — no
/// dynamic dispatch.
pub struct AcpPermissionBridge<S: PermissionRequestSink> {
    sink: S,
    session_id: String,
    pending: PendingPermissions,
    denied: HashSet<String>,
    control: Option<ControlTx>,
}

// endregion: --- Types

// region:    --- AcpPermissionBridge Impl

impl<S: PermissionRequestSink> AcpPermissionBridge<S> {
    /// Create a bridge for `session_id` that resolves decisions through
    /// `pending`.
    pub fn new(sink: S, session_id: impl Into<String>, pending: PendingPermissions) -> Self {
        Self {
            sink,
            session_id: session_id.into(),
            pending,
            denied: HashSet::new(),
            control: None,
        }
    }

    /// Announce the surface's decision on `control`.
    ///
    /// Without this builder the bridge stays silent: the decision still
    /// resolves the pending wait, it is just not announced.
    pub fn with_control(mut self, control: ControlTx) -> Self {
        self.control = Some(control);
        self
    }

    /// Ask the client to authorize the call described by `requested` and
    /// resolve the pending decision registered under its request id.
    ///
    /// A call key already in the deny set is denied without asking the client:
    /// the user chose "reject always" for it earlier in the session.
    pub async fn handle_permission_request(
        &mut self,
        requested: &PermissionRecord,
    ) -> Result<PermissionDecision> {
        let PermissionRecord::Requested {
            request_id,
            call_id,
            summary,
            shows_preview,
            preview,
            ..
        } = requested
        else {
            return Err(AcpError::Runtime(
                "permission bridge received a record that opens no card".to_string(),
            ));
        };
        let call_key = call_id.as_str();
        let decision = if self.denied.contains(call_key) {
            PermissionDecision::Deny
        } else {
            let options = build_permission_options();
            let tool_call = build_tool_call(summary, call_key, *shows_preview, preview.as_deref());
            let outcome = self
                .sink
                .send_request(&self.session_id, tool_call, options)
                .await;
            let decision = map_outcome_to_decision(&outcome);
            if is_reject_always(&outcome) {
                self.denied.insert(call_key.to_string());
            }
            decision
        };
        if let Some(tx) = self.lock_pending().remove(request_id.as_str()) {
            let _ = tx.send(decision);
        } else if let Some(control) = &self.control {
            // The wait is gone: the request timed out, was cancelled, or was
            // already resolved. The surface still answered, so announce the
            // decision — the record carries the request id it belongs to.
            let _ = control.publish(ControlCommand::PermissionDecided {
                request_id: request_id.clone(),
                decision,
            });
        }
        Ok(decision)
    }

    /// Whether `tool_key` is denied for the rest of the session.
    pub fn is_denied(&self, tool_key: &str) -> bool {
        self.denied.contains(tool_key)
    }

    /// Deny every permission request still awaiting a decision.
    ///
    /// Called when the drain loop ends: a tool call must not hang waiting for
    /// a client that will never answer. Requests already resolved by
    /// [`Self::handle_permission_request`] are gone from the map and are left
    /// untouched.
    pub fn deny_all_pending(&mut self) {
        let mut pending = self.lock_pending();
        for (_, tx) in pending.drain() {
            let _ = tx.send(PermissionDecision::Deny);
        }
    }

    /// The pending-request map, recovered if a submitter panicked mid-insert.
    fn lock_pending(&self) -> MutexGuard<'_, HashMap<String, OneshotTx<PermissionDecision>>> {
        self.pending.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

// endregion: --- AcpPermissionBridge Impl

// region:    --- Support

/// Option id for "allow this call once".
const OPTION_ALLOW_ONCE: &str = "allow-once";
/// Option id for "allow this call and remember".
const OPTION_ALLOW_ALWAYS: &str = "allow-always";
/// Option id for "reject this call once".
const OPTION_REJECT_ONCE: &str = "reject-once";
/// Option id for "reject this call and remember".
const OPTION_REJECT_ALWAYS: &str = "reject-always";

/// The four options an ACP client renders for a permission prompt.
fn build_permission_options() -> Vec<PermissionOption> {
    vec![
        PermissionOption::new(
            OPTION_ALLOW_ONCE,
            "Allow once",
            PermissionOptionKind::AllowOnce,
        ),
        PermissionOption::new(
            OPTION_ALLOW_ALWAYS,
            "Allow always",
            PermissionOptionKind::AllowAlways,
        ),
        PermissionOption::new(
            OPTION_REJECT_ONCE,
            "Reject once",
            PermissionOptionKind::RejectOnce,
        ),
        PermissionOption::new(
            OPTION_REJECT_ALWAYS,
            "Reject always",
            PermissionOptionKind::RejectAlways,
        ),
    ]
}

/// Build the tool-call update the client renders next to the prompt.
///
/// The record carries the exact call key; the client renders it as the ACP
/// tool-call id, which is unique per call within a session. The card's content
/// is drawn only when the record says the call has one, and only from the
/// content the producer supplied: the drawer never fabricates it.
fn build_tool_call(
    summary: &str,
    call_key: &str,
    shows_preview: bool,
    preview: Option<&str>,
) -> ToolCallUpdate {
    let mut fields = ToolCallUpdateFields::new()
        .title(summary.to_string())
        .status(ToolCallStatus::Pending);
    let (name, arguments) = call_key.split_once('\n').unwrap_or((call_key, ""));
    let call = ToolCall::new(ToolName(name.to_string()), ToolSource::Unknown, arguments);
    if shows_preview && let Some(display) = call.preview_body(preview) {
        let content = content_from_display(&display);
        if !content.is_empty() {
            fields = fields.content(content);
        }
    }
    ToolCallUpdate::new(call_key.to_string(), fields)
}

/// Map the client's outcome to a nu-agent decision.
///
/// `reject-always` maps to `Deny`; the session-scoped deny set is applied by
/// the caller of this mapping. A cancelled request is a denial.
fn map_outcome_to_decision(outcome: &RequestPermissionOutcome) -> PermissionDecision {
    match selected_option_id(outcome) {
        Some(OPTION_ALLOW_ONCE) => PermissionDecision::AllowOnce,
        Some(OPTION_ALLOW_ALWAYS) => PermissionDecision::AllowAlways,
        _ => PermissionDecision::Deny,
    }
}

/// Whether the client chose "reject always" for this request.
fn is_reject_always(outcome: &RequestPermissionOutcome) -> bool {
    selected_option_id(outcome) == Some(OPTION_REJECT_ALWAYS)
}

/// The option id the client selected, or `None` when the request was
/// cancelled.
fn selected_option_id(outcome: &RequestPermissionOutcome) -> Option<&str> {
    match outcome {
        RequestPermissionOutcome::Selected(selected) => Some(selected.option_id.0.as_ref()),
        _ => None,
    }
}

// endregion: --- Support
