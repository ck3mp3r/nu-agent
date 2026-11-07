//! Level 3 domain methods on the Level 2 channel aliases.
//!
//! These methods encapsulate repeated event construction so call sites express
//! intent instead of building raw events. Only the operations that remove real
//! boilerplate live here — see the 3-level channel design note.
//!
//! Each alias is declared by the kernel, so the methods are declared as
//! extension traits: a local trait on a foreign type keeps this crate's event
//! vocabulary out of the kernel.

use std::future::Future;

use agnt_kernel::bus::{CancelEvent, CancelTx, ChannelResult};

use super::channel::UiEventTx;
use crate::protocol::event::PermissionRequestContext;
use crate::protocol::events::{RequestId, SystemEvent};

// region:    --- Domain Methods

/// Permission-request publication on the ui-event channel.
pub trait RequestPermission {
    /// Publish a `SystemEvent::PermissionRequested` and return the generated
    /// request ID.
    ///
    /// Generates a fresh request ID, sends the event with the unboxed context,
    /// and returns the ID on success. On a send failure the error is returned
    /// and no ID is handed back to the caller.
    fn request_permission(
        &self,
        context: PermissionRequestContext,
    ) -> impl Future<Output = ChannelResult<RequestId>> + Send;
}

impl RequestPermission for UiEventTx {
    async fn request_permission(
        &self,
        context: PermissionRequestContext,
    ) -> ChannelResult<RequestId> {
        let request_id = RequestId::mint();
        self.send(SystemEvent::PermissionRequested {
            request_id: request_id.clone(),
            context,
        })
        .await?;
        Ok(request_id)
    }
}

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
