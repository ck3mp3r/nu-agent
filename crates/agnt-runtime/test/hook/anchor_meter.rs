//! The compaction trigger's meter: what the decision records, what a settled
//! completion writes, and what a compaction forgets.

use crate::hook::anchor_meter::AnchorMeter;

use rig::completion::Usage;
use rig::conversation::{Conversation, MessageId};
use rig::message::Message;
use rig_memory::{Prepared, RequestStart};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// A request start over a two-entry conversation.
fn request_start() -> Result<RequestStart> {
    let mut conversation = Conversation::new();
    conversation.extend([Message::user("older"), Message::assistant("reply")]);
    Ok(Prepared {
        model: "gpt-4o",
        prompt: None,
        tools: &[],
        history: &conversation,
    }
    .start())
}

/// A usage reporting both counters and no total.
fn usage(input: u64, output: u64) -> Usage {
    Usage::new().input_tokens(input).output_tokens(output)
}

/// WHEN a request is recorded and a completion settles, THE meter SHALL answer
/// that request and an anchor through the entry the request metered.
#[test]
fn test_hook_anchor_meter_records_the_request_and_anchors_the_usage() -> Result<()> {
    // -- Setup & Fixtures
    let meter = AnchorMeter::default();
    let sent = request_start()?;
    let through = MessageId::default();

    // -- Exec
    meter.record_request(sent, through);
    meter.record_usage(usage(1_000, 10));
    let (anchor, recorded) = meter.metering();

    // -- Check
    let anchor = anchor.ok_or("a recorded request anchors the usage")?;
    assert_eq!(
        anchor.through, through,
        "the anchor counts through the metered entry"
    );
    assert_eq!(
        anchor.usage.context_tokens(),
        Some(1_010),
        "the anchor carries the reported usage"
    );
    assert_eq!(
        recorded,
        Some(sent),
        "the metered request is kept as recorded"
    );
    assert_eq!(
        meter.context_tokens(),
        Some(1_010),
        "the switch gate reads the last reported count"
    );
    Ok(())
}

/// WHEN a completion settles for a request the decision never recorded, THE
/// meter SHALL count the usage for the switch gate and SHALL NOT anchor it:
/// there is no entry the usage counts through.
#[test]
fn test_hook_anchor_meter_usage_without_a_request_counts_but_does_not_anchor() -> Result<()> {
    // -- Setup & Fixtures
    let meter = AnchorMeter::default();

    // -- Exec
    meter.record_usage(usage(700, 20));
    let (anchor, sent) = meter.metering();

    // -- Check
    assert!(
        anchor.is_none(),
        "an unrecorded request has no entry to anchor on"
    );
    assert!(sent.is_none(), "no request was recorded");
    assert_eq!(
        meter.context_tokens(),
        Some(720),
        "the reported count is kept for the model-switch gate"
    );
    Ok(())
}

/// WHEN the context is rewritten by a compaction, THE meter SHALL forget the
/// request, the anchor and the count: they describe what was sent before it.
#[test]
fn test_hook_anchor_meter_reset_forgets_everything() -> Result<()> {
    // -- Setup & Fixtures
    let meter = AnchorMeter::default();
    meter.record_request(request_start()?, MessageId::default());
    meter.record_usage(usage(1_000, 10));

    // -- Exec
    meter.reset();

    // -- Check
    let (anchor, sent) = meter.metering();
    assert!(anchor.is_none() && sent.is_none(), "the pair is forgotten");
    assert_eq!(meter.context_tokens(), None, "the count is forgotten");
    Ok(())
}

/// WHEN one clone records, THE meter those clones share SHALL answer it: the
/// turn executor, the hook chain and the runtime each hold one.
#[test]
fn test_hook_anchor_meter_clones_share_one_cell() -> Result<()> {
    // -- Setup & Fixtures
    let meter = AnchorMeter::default();
    let holder = meter.clone();

    // -- Exec
    holder.record_request(request_start()?, MessageId::default());
    holder.record_usage(usage(300, 30));

    // -- Check
    assert_eq!(
        meter.context_tokens(),
        Some(330),
        "a clone writes the cell the original reads"
    );
    Ok(())
}
