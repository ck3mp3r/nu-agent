//! Level 3 domain methods on the Level 2 channel aliases.
//!
//! These methods encapsulate repeated event construction so call sites express
//! intent instead of building raw events. Only the operations that remove real
//! boilerplate live here — see the 3-level channel design note.

use std::sync::atomic::{AtomicU64, Ordering};

use super::channel::{CancelTx, ChannelResult, UiEventTx};
use super::events::CancelEvent;
use crate::protocol::event::{PermissionRequestContext, UiEvent};

// region:    --- Support

static NEXT_REQUEST_ID: AtomicU64 = AtomicU64::new(1);

/// Generate a unique permission request ID (atomic counter, no uuid dep).
fn next_request_id() -> String {
    format!(
        "perm-{:016x}",
        NEXT_REQUEST_ID.fetch_add(1, Ordering::SeqCst)
    )
}

// endregion: --- Support

// region:    --- Domain Methods

impl UiEventTx {
    /// Publish a `UiEvent::PermissionRequested` and return the generated request ID.
    ///
    /// Generates a fresh request ID, sends the event with the unboxed context,
    /// and returns the ID on success. On a send failure the error is returned
    /// and no ID is handed back to the caller.
    pub async fn request_permission(
        &self,
        context: PermissionRequestContext,
    ) -> ChannelResult<String> {
        let request_id = next_request_id();
        self.send(UiEvent::PermissionRequested {
            request_id: request_id.clone(),
            context,
        })
        .await?;
        Ok(request_id)
    }
}

impl CancelTx {
    /// Publish a `CancelEvent::Requested`.
    pub async fn request_cancel(&self) -> ChannelResult<()> {
        self.send(CancelEvent::Requested).await
    }
}

// endregion: --- Domain Methods
