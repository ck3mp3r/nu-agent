//! Producer narration: the hook chain publishes session records on the kernel
//! stream in addition to the events it still sends on the hub.
//!
//! These tests drive a real rig turn through the real `HookChain` and read what
//! the producer published, so they pin the producer rather than the vocabulary.

use super::*;

use agnt_kernel::bus::Subscription;
use agnt_kernel::{InferenceRecord, SessionRecord};

#[tokio::test]
async fn test_narration_inference_lifecycle_brackets_the_completion() -> Result<()> {
    // -- Setup & Fixtures
    let fixture = RepetitionTurnFixture::new()?;
    fixture.queue_turn(vec![vec![
        MockStreamEvent::Text("hello ".to_string()),
        MockStreamEvent::Text("world".to_string()),
        MockStreamEvent::final_response_with_total_tokens(42),
    ]]);
    let mut published = fixture.bus.stream().subscribe();

    // -- Exec
    fixture.turn(false).await?;

    // -- Check
    let inference = inference_records(&mut published);
    let started = inference
        .first()
        .ok_or("the turn must narrate inference records")?;
    if !matches!(started, InferenceRecord::Started) {
        return Err(format!("the first record must be Started, got {started:?}").into());
    }
    let completed = inference
        .last()
        .ok_or("the turn must narrate an inference record")?;
    let InferenceRecord::Completed { usage } = completed else {
        return Err(format!("the last record must be Completed, got {completed:?}").into());
    };
    if usage.total_tokens != Some(42) {
        return Err(format!("the completion must carry the wire's total, got {usage:?}").into());
    }
    Ok(())
}

#[tokio::test]
async fn test_narration_chunks_accumulate_from_the_call_start() -> Result<()> {
    // -- Setup & Fixtures
    let fixture = RepetitionTurnFixture::new()?;
    fixture.queue_turn(vec![vec![
        MockStreamEvent::Text("hello ".to_string()),
        MockStreamEvent::Text("world".to_string()),
        MockStreamEvent::final_response_with_default_usage(),
    ]]);
    let mut published = fixture.bus.stream().subscribe();

    // -- Exec
    fixture.turn(false).await?;

    // -- Check
    let chunks = chunk_pairs(&mut published);
    let expected: Vec<(String, String)> = vec![
        ("hello ".to_string(), "hello ".to_string()),
        ("world".to_string(), "hello world".to_string()),
    ];
    if chunks != expected {
        return Err(format!("chunks must accumulate from the call start, got {chunks:?}").into());
    }
    Ok(())
}

#[tokio::test]
async fn test_narration_chunks_match_the_events_the_hub_receives() -> Result<()> {
    // -- Setup & Fixtures
    let fixture = RepetitionTurnFixture::new()?;
    fixture.queue_turn(vec![vec![
        MockStreamEvent::Text("alpha ".to_string()),
        MockStreamEvent::Text("beta".to_string()),
        MockStreamEvent::final_response_with_default_usage(),
    ]]);
    let mut published = fixture.bus.stream().subscribe();
    let mut events = fixture.bus.narration().subscribe();

    // -- Exec
    fixture.turn(false).await?;

    // -- Check
    let from_stream = chunk_pairs(&mut published);
    let from_hub = inference_chunks(&mut events);
    if from_stream != from_hub {
        return Err(format!(
            "the stream and the hub must narrate the same deltas, stream: {from_stream:?}, hub: {from_hub:?}"
        )
        .into());
    }
    Ok(())
}

/// A tool turn narrates the call pair: one Started and one Completed record for
/// the same call, carrying the call's identity and arguments as data — never a
/// rendered line.
#[tokio::test]
async fn test_narration_tool_call_narrates_the_started_completed_pair() -> Result<()> {
    // -- Setup & Fixtures
    let fixture = RepetitionTurnFixture::new()?;
    fixture.queue_turn(vec![
        vec![
            MockStreamEvent::tool_call("tc1", "add", serde_json::json!({"x": 1, "y": 2})),
            MockStreamEvent::final_response_with_default_usage(),
        ],
        vec![
            MockStreamEvent::Text("tool done".to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ]);
    let mut published = fixture.bus.stream().subscribe();

    // -- Exec
    fixture.turn(true).await?;

    // -- Check
    let tool = tool_records(&mut published);
    let started = tool
        .iter()
        .find_map(|record| match record {
            agnt_kernel::ToolRecord::Started { call } => Some(call),
            _ => None,
        })
        .ok_or("the turn must narrate a started tool record")?;
    let (call, outcome) = tool
        .iter()
        .find_map(|record| match record {
            agnt_kernel::ToolRecord::Completed { call, outcome } => Some((call, outcome)),
            _ => None,
        })
        .ok_or("the turn must narrate a completed tool record")?;
    if call.id() != started.id() || call.name.0 != "add" {
        return Err(
            format!("the pair must name the same call, started {started:?}, got {call:?}").into(),
        );
    }
    if started.arguments != r#"{"x":1,"y":2}"# {
        return Err(format!(
            "the record must carry the raw arguments, got {:?}",
            started.arguments
        )
        .into());
    }
    // The fixture's safe-default policy denies this call, so the pair brackets a
    // call that never ran: the disposition is the enum, never the denial text.
    if outcome.status != agnt_kernel::ToolStatus::Cancelled {
        return Err(format!("a denied call settles as Cancelled, got {outcome:?}").into());
    }
    Ok(())
}

/// The inference records the subscriber buffered, in publish order.
fn inference_records(rx: &mut Subscription<SessionRecord>) -> Vec<InferenceRecord> {
    let mut out = Vec::new();
    while let Ok(record) = rx.try_recv() {
        if let SessionRecord::Inference(inference) = record {
            out.push(inference);
        }
    }
    out
}

/// The `(delta, aggregated)` pairs the subscriber buffered, in publish order.
fn chunk_pairs(rx: &mut Subscription<SessionRecord>) -> Vec<(String, String)> {
    let mut out = Vec::new();
    while let Ok(record) = rx.try_recv() {
        if let SessionRecord::Inference(InferenceRecord::Chunk {
            delta, aggregated, ..
        }) = record
        {
            out.push((delta, aggregated));
        }
    }
    out
}

/// The tool records the subscriber buffered, in publish order.
fn tool_records(rx: &mut Subscription<SessionRecord>) -> Vec<agnt_kernel::ToolRecord> {
    let mut out = Vec::new();
    while let Ok(record) = rx.try_recv() {
        if let SessionRecord::Tool(tool) = record {
            out.push(tool);
        }
    }
    out
}

/// A call the turn never dispatched still narrates the pair: the record stream
/// carries every call the hub renders, so a consumer folding records sees the
/// never-run class too.
#[tokio::test]
async fn test_narration_not_run_dispatch_narrates_the_pair() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::default();
    let mut published = bus.stream().subscribe();
    let arguments = r#"{"x":1}"#;

    // -- Exec
    crate::hook::chain::emit_not_run_dispatch(
        bus.stream(),
        crate::hook::chain::NotRunCall {
            tool_name: "add",
            source: crate::tools::handler::ToolSource::Builtin,
            arguments,
            start: crate::hook::chain::NotRunStart::Publish,
        },
        crate::hook::chain::NotRunOutcome::Cancelled,
    )
    .await;

    // -- Check
    let tool = tool_records(&mut published);
    if tool.len() != 2 {
        return Err(format!("the not-run class must narrate a pair, got {tool:?}").into());
    }
    let agnt_kernel::ToolRecord::Started { call } = &tool[0] else {
        return Err(format!("the pair must open with Started, got {:?}", tool[0]).into());
    };
    let agnt_kernel::ToolRecord::Completed {
        call: settled,
        outcome,
    } = &tool[1]
    else {
        return Err(format!("the pair must close with Completed, got {:?}", tool[1]).into());
    };
    if call.id() != settled.id() || call.arguments != arguments {
        return Err(
            format!("both records must name the same call, got {call:?} and {settled:?}").into(),
        );
    }
    if outcome.status != agnt_kernel::ToolStatus::Cancelled {
        return Err(format!("a call that never ran settles as Cancelled, got {outcome:?}").into());
    }
    if outcome.kind != Some(agnt_kernel::ToolErrorKind::Cancelled) {
        return Err(format!("the completion classifies itself, got {outcome:?}").into());
    }
    Ok(())
}

/// A gate refusal is a different class from a turn stop: both produce no result,
/// and the record tells them apart by its enumerated kind — never by the text.
#[tokio::test]
async fn test_narration_denied_dispatch_classifies_as_denied() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::default();
    let mut published = bus.stream().subscribe();
    let arguments = r#"{"x":1}"#;

    // -- Exec
    crate::hook::chain::emit_not_run_dispatch(
        bus.stream(),
        crate::hook::chain::NotRunCall {
            tool_name: "add",
            source: crate::tools::handler::ToolSource::Builtin,
            arguments,
            start: crate::hook::chain::NotRunStart::AlreadyPublished,
        },
        crate::hook::chain::NotRunOutcome::Denied { reason: "blocked" },
    )
    .await;

    // -- Check
    let tool = tool_records(&mut published);
    if tool.len() != 1 {
        return Err(
            format!("an already-published start must not be narrated twice, got {tool:?}").into(),
        );
    }
    let agnt_kernel::ToolRecord::Completed { outcome, .. } = &tool[0] else {
        return Err(format!("the only record must be the completion, got {:?}", tool[0]).into());
    };
    if outcome.kind != Some(agnt_kernel::ToolErrorKind::Denied) {
        return Err(format!("a gate refusal classifies as Denied, got {outcome:?}").into());
    }
    Ok(())
}
