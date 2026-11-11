//! Shared fixtures for the translator test suite.

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex};

use agent_client_protocol::schema::v1::{
    PermissionOption, RequestPermissionOutcome, SelectedPermissionOutcome, SessionUpdate,
    ToolCallUpdate,
};
use agnt_kernel::{
    PermissionRecord, RequestId, SessionRecord, ToolCall, ToolCallId, ToolOutcome, ToolRecord,
    ToolSource, ToolStatus,
};
use agnt_render::transcript::ir::ToolName as RenderToolName;
use agnt_runtime::bus::{OneshotRx, OneshotTx};
use agnt_runtime::conversation::runtime::PendingPermissions;
use agnt_runtime::protocol::event::PermissionDecision;

use crate::error::AcpError;
use crate::permission::{AcpPermissionBridge, PermissionRequestSink};
use crate::translator::{EventTranslator, SessionUpdateSink};

pub const SESSION: &str = "sess-1";

pub fn translator() -> EventTranslator {
    EventTranslator::new(SESSION)
}

/// The call under test, with its id derived by the one derivation.
fn call(name: &str, arguments: &str) -> ToolCall {
    ToolCall::new(
        RenderToolName(name.to_string()),
        ToolSource::Builtin,
        arguments,
    )
}

pub fn tool_started(name: &str, arguments: &str) -> SessionRecord {
    SessionRecord::Tool(ToolRecord::Started {
        call: call(name, arguments),
    })
}

pub fn tool_completed(name: &str, arguments: &str, success: bool) -> SessionRecord {
    SessionRecord::Tool(ToolRecord::Completed {
        call: call(name, arguments),
        outcome: ToolOutcome {
            status: if success {
                ToolStatus::Success
            } else {
                ToolStatus::Failed
            },
            payload: None,
            text: "{}".to_string(),
            message: None,
            kind: None,
        },
    })
}

/// The card a permission gate opens for the `edit` call `arguments` names.
pub fn permission_requested(request_id: &str, call_key: &str) -> PermissionRecord {
    PermissionRecord::Requested {
        request_id: RequestId::from_minted(request_id),
        call_id: ToolCallId::from_key(call_key),
        tool: "edit".to_string(),
        source: "builtin".to_string(),
        rule_identity: "rule".to_string(),
        scope: "global".to_string(),
        pattern: "*".to_string(),
        target_field: None,
        mode: None,
        summary: "→ edit".to_string(),
        shows_preview: false,
        preview: None,
    }
}

/// The default card: request `perm-1` for the `edit` call over `{}`.
pub fn default_requested() -> PermissionRecord {
    permission_requested("perm-1", "edit\n{}")
}

/// One `session/request_permission` call captured by the mock sink.
#[derive(Debug, Clone)]
pub struct RecordedPermissionRequest {
    pub session_id: String,
    pub tool_call: ToolCallUpdate,
    pub options: Vec<PermissionOption>,
}

/// Test double for [`PermissionRequestSink`]: records every request and
/// answers with a fixed outcome.
#[derive(Clone)]
pub struct RecordingPermissionSink {
    requests: Arc<Mutex<Vec<RecordedPermissionRequest>>>,
    outcome: RequestPermissionOutcome,
}

impl RecordingPermissionSink {
    pub fn new(outcome: RequestPermissionOutcome) -> Self {
        Self {
            requests: Arc::new(Mutex::new(Vec::new())),
            outcome,
        }
    }

    pub fn requests(&self) -> Vec<RecordedPermissionRequest> {
        self.requests
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

impl PermissionRequestSink for RecordingPermissionSink {
    fn send_request(
        &self,
        session_id: &str,
        tool_call: ToolCallUpdate,
        options: Vec<PermissionOption>,
    ) -> impl Future<Output = RequestPermissionOutcome> + Send {
        self.requests
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(RecordedPermissionRequest {
                session_id: session_id.to_string(),
                tool_call,
                options,
            });
        let outcome = self.outcome.clone();
        async move { outcome }
    }
}

pub fn selected(option_id: &'static str) -> RequestPermissionOutcome {
    RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(option_id))
}

pub fn empty_pending() -> PendingPermissions {
    Arc::new(Mutex::new(HashMap::new()))
}

pub fn permission_bridge(
    outcome: RequestPermissionOutcome,
    pending: PendingPermissions,
) -> AcpPermissionBridge<RecordingPermissionSink> {
    AcpPermissionBridge::new(RecordingPermissionSink::new(outcome), SESSION, pending)
}

pub fn register_pending(
    pending: &PendingPermissions,
    request_id: &str,
) -> OneshotRx<PermissionDecision> {
    let (tx, rx) = OneshotTx::<PermissionDecision>::channel("permission");
    pending
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(request_id.to_string(), tx);
    rx
}

/// Test double for [`SessionUpdateSink`]: records every notification.
#[derive(Default)]
pub struct RecordingSink {
    updates: Mutex<Vec<SessionUpdate>>,
    fail: bool,
    updated: tokio::sync::Notify,
}

impl RecordingSink {
    pub fn failing() -> Self {
        Self {
            updates: Mutex::new(Vec::new()),
            fail: true,
            updated: tokio::sync::Notify::new(),
        }
    }

    pub fn taken(&self) -> Vec<SessionUpdate> {
        self.updates
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    /// Awaits until at least `count` updates have been recorded, so tests can
    /// wait for the drain to forward an event instead of sleeping.
    pub async fn wait_for_updates(&self, count: usize) {
        while self.taken().len() < count {
            self.updated.notified().await;
        }
    }
}

impl SessionUpdateSink for RecordingSink {
    fn send_update(
        &self,
        _session_id: &str,
        update: SessionUpdate,
    ) -> core::result::Result<(), AcpError> {
        if self.fail {
            return Err(AcpError::Runtime("transport closed".to_string()));
        }
        if let Ok(mut guard) = self.updates.lock() {
            guard.push(update);
        }
        self.updated.notify_one();
        Ok(())
    }
}
