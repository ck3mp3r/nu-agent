//! Level 3 domain methods: the one publication point for each concern they
//! own.
//!
//! A gate record is minted and published in exactly one place, so the facts
//! snapshot a surface draws and the stream record every other consumer folds
//! are the same value.

use std::future::Future;

use agnt_kernel::bus::{
    CancelEvent, CancelTx, ChannelResult, FactRecord, FactsTx, PermissionGate, SessionRecord,
    Stream,
};
use agnt_kernel::{PermissionRecord, RequestId, ToolCallId};

use crate::protocol::event::PermissionRequestContext;

// region:    --- Types

/// The card a call has, as producer-side data.
///
/// `shows_preview` says whether the call has a card at all; `content` is the
/// one datum only the producer can compute (an edit's proposed diff). A card
/// whose content the drawer derives from the call alone carries `None`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PermissionCard {
    pub shows_preview: bool,
    pub content: Option<String>,
}

// endregion: --- Types

// region:    --- Domain Functions

/// Open the permission gate for `context` and publish it.
///
/// Mints the request id, publishes the gate snapshot the surface draws and the
/// `PermissionRecord::Requested` the stream records — one value, two concerns —
/// and hands the id back to the caller that awaits the answer.
pub fn request_permission(
    stream: &Stream,
    facts: &FactsTx,
    context: PermissionRequestContext,
    card: PermissionCard,
) -> ChannelResult<RequestId> {
    let request_id = RequestId::mint();
    let record = PermissionRecord::Requested {
        request_id: request_id.clone(),
        call_id: ToolCallId::from_key(context.tool_key),
        tool: context.tool,
        source: context.source,
        rule_identity: context.matched_rule_identity,
        scope: context.scope,
        pattern: context.pattern,
        target_field: context.target_field,
        mode: context.mode,
        summary: context.summary,
        shows_preview: card.shows_preview,
        preview: card.content,
    };
    let _ = facts.publish(FactRecord::PermissionGate(PermissionGate::Prompting(
        Box::new(record.clone()),
    )));
    let _ = stream.publish(SessionRecord::Permission(record));
    Ok(request_id)
}

/// Record one resolution of the open gate and close the gate.
///
/// The resolution rides the stream for every consumer that folds records; the
/// snapshot returns to `Idle`, because no card is open any more.
pub fn settle_permission(stream: &Stream, facts: &FactsTx, record: PermissionRecord) {
    let _ = stream.publish(SessionRecord::Permission(record));
    let _ = facts.publish(FactRecord::PermissionGate(PermissionGate::Idle));
}

/// Publish a policy resolution that opened no card.
pub fn auto_resolve_permission(stream: &Stream, facts: &FactsTx, record: PermissionRecord) {
    let _ = facts.publish(FactRecord::PermissionGate(PermissionGate::AutoResolved(
        Box::new(record.clone()),
    )));
    let _ = stream.publish(SessionRecord::Permission(record));
}

// endregion: --- Domain Functions

/// Cancellation publication on the cancel channel.
pub trait RequestCancel {
    /// Publish a `CancelEvent::Requested`.
    fn request_cancel(&self) -> impl Future<Output = ChannelResult<()>> + Send;
}

impl RequestCancel for CancelTx {
    async fn request_cancel(&self) -> ChannelResult<()> {
        self.send(CancelEvent::Requested).await
    }
}

// endregion: --- Domain Methods
