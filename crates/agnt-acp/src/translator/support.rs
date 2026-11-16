//! Drain plumbing: route one record to the bridge or the client, and send one
//! translated datum.

use agnt_kernel::{PermissionRecord, SessionRecord};

use crate::permission::{AcpPermissionBridge, PermissionRequestSink};

use super::types::{EventTranslator, SessionUpdateSink, TranslateInput};

/// Handle one stream record during a drain.
///
/// A permission request goes to the bridge — the translator has no ACP
/// equivalent for it. Everything else is translated and sent. Returns `false`
/// when the transport is gone.
pub(super) async fn handle_record<S, P>(
    sink: &S,
    translator: &mut EventTranslator,
    bridge: &mut AcpPermissionBridge<P>,
    record: &SessionRecord,
) -> bool
where
    S: SessionUpdateSink,
    P: PermissionRequestSink,
{
    if let SessionRecord::Permission(requested @ PermissionRecord::Requested { .. }) = record {
        if let Err(err) = bridge.handle_permission_request(requested).await {
            log::warn!(
                "session {}: failed to handle permission request: {err}",
                translator.session_id()
            );
        }
        return true;
    }
    send_input(sink, translator, TranslateInput::Record(record))
}

/// Send one translated datum. Returns `false` when the transport is gone.
pub(super) fn send_input<S: SessionUpdateSink>(
    sink: &S,
    translator: &mut EventTranslator,
    input: TranslateInput<'_>,
) -> bool {
    let Some(update) = translator.translate(input) else {
        return true;
    };
    match sink.send_update(translator.session_id(), update) {
        Ok(()) => true,
        Err(err) => {
            log::warn!(
                "session {}: failed to send session/update: {err}",
                translator.session_id()
            );
            false
        }
    }
}
