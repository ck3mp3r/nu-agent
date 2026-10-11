//! Permission bridge between nu-agent's permission flow and ACP
//! `session/request_permission`.
//!
//! nu-agent publishes `UiEvent::PermissionRequested` on the session bus and
//! awaits the decision on a oneshot registered in `PendingPermissions`. In ACP
//! mode the editor is the permission UI: the bridge forwards the request to the
//! client as `session/request_permission`, maps the client's outcome to a
//! `PermissionDecision`, and resolves the pending oneshot.

// region:    --- Modules

use std::collections::HashSet;
use std::future::Future;

use agent_client_protocol::schema::v1::{
    PermissionOption, PermissionOptionKind, RequestPermissionOutcome, RequestPermissionRequest,
    SessionId, ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields,
};
use agent_client_protocol::{Client, ConnectionTo};
use agnt_core::conversation::runtime::PendingPermissions;
use agnt_core::protocol::event::{PermissionDecision, PermissionRequestContext};

use crate::error::Result;
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
        }
    }

    /// Ask the client to authorize the tool call described by `context` and
    /// resolve the pending decision registered under `request_id`.
    ///
    /// A `tool_key` already in the deny set is denied without asking the
    /// client: the user chose "reject always" for it earlier in the session.
    pub async fn handle_permission_request(
        &mut self,
        request_id: &str,
        context: PermissionRequestContext,
    ) -> Result<PermissionDecision> {
        let decision = if self.denied.contains(&context.tool_key) {
            PermissionDecision::Deny
        } else {
            let options = build_permission_options();
            let tool_call = build_tool_call(&context);
            let outcome = self
                .sink
                .send_request(&self.session_id, tool_call, options)
                .await;
            let decision = map_outcome_to_decision(&outcome);
            if is_reject_always(&outcome) {
                self.denied.insert(context.tool_key.clone());
            }
            decision
        };
        if let Some(tx) = self.pending.lock().unwrap().remove(request_id) {
            let _ = tx.send(decision);
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
        let mut pending = self.pending.lock().unwrap();
        for (_, tx) in pending.drain() {
            let _ = tx.send(PermissionDecision::Deny);
        }
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
/// The permission event carries no ACP tool-call id, so the exact call key is
/// used as the id: it is unique per call within a session.
fn build_tool_call(context: &PermissionRequestContext) -> ToolCallUpdate {
    let mut fields = ToolCallUpdateFields::new()
        .title(context.summary.clone())
        .status(ToolCallStatus::Pending);
    if let Some(display) = &context.pre_authorize_display {
        fields = fields.content(content_from_display(display));
    }
    ToolCallUpdate::new(context.tool_key.clone(), fields)
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
