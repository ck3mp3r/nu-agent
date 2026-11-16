//! The delivery contract between the text lane and the event ring: a hook that
//! declares a discrete event receives it, however many streamed deltas the same
//! implementation produces.

use super::*;

/// WHEN an implementation burst interleaves 10_000 streamed deltas with 20 tool
/// starts and the hook's reader is wedged inside its first invocation, THEN on
/// the wedge's release the hook SHALL receive all 20 invocations in publication
/// order.
#[tokio::test(start_paused = true)]
async fn test_hooks_runner_delivers_every_tool_event_under_a_chunk_burst() -> Result<()> {
    // -- Setup & Fixtures
    const CHUNKS: usize = 10_000;
    const TOOLS: usize = 20;
    let bus = Bus::default();
    let process = WedgedProcess::default();
    let hooks = vec![hook("audit", &[HookEvent::ToolStarted])];
    let handles = spawn_with(&bus, &hooks, process.clone());

    // -- Exec: the reader wedges inside its first invocation, then the burst
    // arrives while nothing drains its lane.
    publish_tool_started(&bus, 0);
    process.entered().await;
    // Spread the remaining tool starts across the burst: the first chunk of
    // every stride carries one, and the tail carries any left over.
    let stride = CHUNKS / (TOOLS - 1);
    let mut next_tool = 1usize;
    for index in 0..CHUNKS {
        publish_inference_chunk(&bus, index);
        if index % stride == 0 && next_tool < TOOLS {
            publish_tool_started(&bus, next_tool);
            next_tool += 1;
        }
    }
    while next_tool < TOOLS {
        publish_tool_started(&bus, next_tool);
        next_tool += 1;
    }
    process.release();
    drop(bus);
    finish(handles).await?;

    // -- Check: every tool start invoked the hook, in publication order
    let calls = process.calls();
    let events = process.events();
    assert_eq!(
        calls.len(),
        TOOLS,
        "every tool start must invoke the hook; got {events:?}"
    );
    for (index, call) in calls.iter().enumerate() {
        assert_eq!(call.event, "tool_started", "invocation {index}");
        let body = payload(call)?;
        assert_eq!(
            body["arguments"],
            format!("{{\"n\":{index}}}"),
            "the invocations must arrive in publication order"
        );
    }
    Ok(())
}
