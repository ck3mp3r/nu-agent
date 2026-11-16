//! The translator's public types: the intake vocabulary, the concern set one
//! turn drains, the notification sink, and the stateful translator itself.

use std::collections::VecDeque;

use agent_client_protocol::schema::v1::{
    SessionId, SessionNotification, SessionUpdate, ToolCallId,
};
use agent_client_protocol::{Client, ConnectionTo};
use agnt_kernel::SessionRecord;
use agnt_runtime::bus::{Control, Narration, NarrationEvent, Stream};

use crate::error::AcpError;

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
    pub(super) session_id: String,
    pub(super) max_context_tokens: u64,
    pub(super) next_call_seq: u64,
    pub(super) pending: VecDeque<PendingCall>,
}

/// A started tool call awaiting its completion event.
pub(super) struct PendingCall {
    pub(super) name: String,
    pub(super) arguments: String,
    pub(super) id: ToolCallId,
}
