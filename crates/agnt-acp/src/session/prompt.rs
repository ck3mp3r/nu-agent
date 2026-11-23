//! Prompt-turn dispatch: run a turn, and cancel one.

use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex as StdMutex};

use agent_client_protocol::schema::v1::StopReason;
use agnt_runtime::bus::{ChannelError, RequestCancel};
use agnt_runtime::protocol::contracts::CoreRuntime;
use agnt_runtime::protocol::events::CompactionSource;
use agnt_runtime::session::management::SessionPersistence;
use nu_protocol::Span;

use crate::error::{AcpError, Result};
use crate::permission::{AcpPermissionBridge, PermissionRequestSink};
use crate::translator::{EventTranslator, SessionUpdateSink, run_turn_with_notifications};

use super::support::stop_category;
use super::types::{
    AcpSessionManager, RuntimeBuilder, StopCategory, map_stop_category_to_stop_reason,
};

impl<R: RuntimeBuilder> AcpSessionManager<R> {
    /// Run one prompt turn on `session_id` and report why it stopped.
    ///
    /// The turn runs concurrently with the bus drain loop, so every
    /// `session/update` reaches `sink` as it is published and every permission
    /// request reaches the bridge. The bridge is built here from the pending
    /// map installed on the session runtime, so it resolves the same oneshots
    /// the turn's permission resolver registers. A cancel requested while the
    /// turn runs wins over the turn's own result: ACP requires
    /// `stopReason: cancelled` for every `session/cancel`.
    ///
    /// The registry lock is held only long enough to clone the session handle;
    /// the turn then runs under the session's own runtime lock, so
    /// [`Self::cancel`] can interleave.
    pub(crate) async fn prompt<S>(
        &self,
        session_id: &str,
        prompt_text: String,
        sink: &S,
    ) -> Result<StopReason>
    where
        S: SessionUpdateSink + PermissionRequestSink,
    {
        let session = self.session(session_id)?;
        session.cancel_requested.store(false, Ordering::SeqCst);

        // Each concern is subscribed before the turn starts: the rings retain
        // nothing, so a subscriber that connects late never sees what it missed.
        let mut stream_rx = session.bus.stream().subscribe();
        let mut narration_rx = session.bus.narration().subscribe();
        let mut control_rx = session.bus.control().subscribe();
        let mut translator = EventTranslator::new(session_id);
        let result = {
            let mut runtime = session.runtime.lock().await;
            let pending = runtime
                .interactive_pending_arc()
                .unwrap_or_else(|| Arc::new(StdMutex::new(HashMap::new())));
            let mut bridge = AcpPermissionBridge::new(sink, session_id, pending)
                .with_control(session.bus.control().clone());
            let turn = runtime.execute_turn(prompt_text, None, Span::unknown(), None);
            let (result, compaction_requested) = run_turn_with_notifications(
                &mut stream_rx,
                &mut narration_rx,
                &mut control_rx,
                sink,
                &mut translator,
                &mut bridge,
                turn,
            )
            .await;
            // Post-turn compaction dispatch. The chain's auto rule fires in
            // every mode; ACP is the consumer that runs the request. It runs
            // here, still under the runtime lock, so it is serialized with
            // turns exactly as the TUI's worker-busy queue is. The compaction
            // is itself drained, so the client sees its events live.
            if compaction_requested {
                // The chain is the only ACP requester and it always asks for
                // the auto source; there is no ACP `/compact`.
                let outcome = run_turn_with_notifications(
                    &mut stream_rx,
                    &mut narration_rx,
                    &mut control_rx,
                    sink,
                    &mut translator,
                    &mut bridge,
                    runtime.run_compaction(CompactionSource::Auto),
                )
                .await;
                // The compaction's own outcome never reaches the stop reason:
                // the turn already ended. Its failure sites emit `Failed`
                // inside the drain window, so the client is already informed;
                // a post-drain `Failed` would reach no client.
                if let Err(err) = outcome.0 {
                    log::warn!("session {session_id}: post-turn compaction failed: {err}");
                }
            }
            result
        };

        let outcome = if session.cancel_requested.load(Ordering::SeqCst) {
            StopCategory::Cancelled
        } else {
            stop_category(session_id, &result)
        };
        Ok(map_stop_category_to_stop_reason(outcome))
    }

    /// Ask the session's turn to stop and report `stopReason: cancelled`.
    ///
    /// Never takes the session's runtime lock, so it completes while a turn is
    /// in flight. Returns [`AcpError::SessionNotFound`] when no live session has
    /// that id.
    pub async fn cancel(&self, session_id: &str) -> Result<()> {
        let session = self.session(session_id)?;
        session.cancel_requested.store(true, Ordering::SeqCst);
        match session.bus.cancel().request_cancel().await {
            // No turn is listening yet: the flag alone carries the cancel into
            // the next turn, so this is a no-op rather than a failure.
            Ok(()) | Err(ChannelError::NoReceiver) => Ok(()),
            Err(err) => Err(AcpError::Runtime(format!(
                "failed to cancel session '{session_id}': {err}"
            ))),
        }
    }
}
