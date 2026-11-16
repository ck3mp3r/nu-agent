//! Fold order between the chunk lanes and the event lane.
//!
//! A chunk lane must drain before an event-lane record that closes the streamed
//! block the chunk feeds; otherwise the block re-opens as a second entry. The
//! driver's imports arrive through `use super::*`.

use super::*;

use agnt_runtime::protocol::events::{
    CompactionRecord, CompactionSource, InferenceId, InferenceRecord, StopCause, ToolCall,
    ToolOutcome, ToolRecord, ToolStatus, Usage,
};

/// WHEN the final text chunk and the turn's stop record are pending at one
/// wake, THE render loop SHALL fold the text lane before the event lane: the
/// stop clears the streamed block's cursor, so a chunk folded after it re-pushes
/// the whole answer as a second transcript entry.
#[tokio::test]
async fn text_lane_chunk_folds_before_a_stop_pending_on_the_same_wake() -> Result<()> {
    // -- Setup & Fixtures
    const ROUNDS: usize = 16;
    let mut driver = RenderLoopDriver::new(160, 40);

    // -- Exec & Check: which subscription the loop's select wakes on is the
    // loop's own business, so one round pins nothing. Each round opens the
    // streamed block on the text lane alone, then publishes the final chunk and
    // the stop record before any yield: both are pending at the wake that
    // follows, exactly as the two lanes deliver them in production.
    for round in 0..ROUNDS {
        let answer = format!("answer {round}");
        let call = InferenceId::mint();
        driver
            .advance(&[DriveEvent::TextRecord(Box::new(streamed_chunk(
                call, &answer,
            )))])
            .await?;
        driver
            .advance_burst(&[
                DriveEvent::TextRecord(Box::new(streamed_chunk(call, &format!("{answer} final")))),
                DriveEvent::Record(Box::new(SessionRecord::Inference(
                    InferenceRecord::Stopped {
                        cause: StopCause::DoomLoop,
                        reason: format!("stopped {round}"),
                    },
                ))),
            ])
            .await?;

        let rendered = driver
            .state()
            .transcript
            .entries()
            .iter()
            .filter(|entry| {
                matches!(entry, TranscriptEntry::Message(message) if message.markdown.contains(&answer))
            })
            .count();
        assert_eq!(
            rendered, 1,
            "round {round}: the turn's answer must render exactly once, rendered {rendered}"
        );
    }
    Ok(())
}

/// WHEN a compaction's final summary chunk and its completion are pending at one
/// wake, THE render loop SHALL fold the text lane before the event lane: the
/// completion closes the streamed summary block, so a chunk folded after it
/// pushes the whole summary into a second transcript entry.
#[tokio::test]
async fn summary_chunk_folds_before_a_compaction_completion_pending_on_the_same_wake() -> Result<()>
{
    // -- Setup & Fixtures
    const ROUNDS: usize = 16;
    let mut driver = RenderLoopDriver::new(160, 40);

    // -- Exec & Check: one round pins nothing — the loop's select picks the arm
    // that wakes it. Each round opens the summary block on the text lane alone,
    // then publishes the final chunk and the completion before any yield.
    for round in 0..ROUNDS {
        let summary = format!("summary-r{round:02}");
        driver
            .advance(&[DriveEvent::TextRecord(Box::new(summary_chunk(&summary)))])
            .await?;
        driver
            .advance_burst(&[
                DriveEvent::TextRecord(Box::new(summary_chunk(&format!("{summary} final")))),
                DriveEvent::Record(Box::new(SessionRecord::Compaction(
                    CompactionRecord::Completed {
                        source: CompactionSource::Auto,
                        summary: format!("{summary} final"),
                    },
                ))),
            ])
            .await?;

        let rendered = driver
            .state()
            .transcript
            .entries()
            .iter()
            .filter(|entry| {
                matches!(entry, TranscriptEntry::Message(message) if message.markdown.contains(&summary))
            })
            .count();
        assert_eq!(
            rendered, 1,
            "round {round}: the summary must render exactly once, rendered {rendered}"
        );
    }
    Ok(())
}

/// WHEN the final narrated chunk and the turn's stop record are pending at one
/// wake, THE render loop SHALL fold the narration lane before the event lane: the
/// stop clears the streamed block's cursor, so a chunk folded after it re-pushes
/// the whole answer as a second transcript entry.
#[tokio::test]
async fn narration_chunk_folds_before_a_stop_pending_on_the_same_wake() -> Result<()> {
    // -- Setup & Fixtures
    const ROUNDS: usize = 16;
    let mut driver = RenderLoopDriver::new(160, 40);

    // -- Exec & Check: the loop's select decides which arm wakes first, so one
    // round pins nothing. Each round opens the streamed block on the narration
    // lane alone, then publishes the final chunk and the stop record before any
    // yield: both are pending at the wake that follows.
    for round in 0..ROUNDS {
        let answer = format!("narrated {round}");
        let call = InferenceId::mint();
        driver.advance(&[narration_chunk(call, &answer)]).await?;
        driver
            .advance_burst(&[
                narration_chunk(call, &format!("{answer} final")),
                DriveEvent::Record(Box::new(SessionRecord::Inference(
                    InferenceRecord::Stopped {
                        cause: StopCause::DoomLoop,
                        reason: format!("stopped {round}"),
                    },
                ))),
            ])
            .await?;

        let rendered = driver
            .state()
            .transcript
            .entries()
            .iter()
            .filter(|entry| {
                matches!(entry, TranscriptEntry::Message(message) if message.markdown.contains(&answer))
            })
            .count();
        assert_eq!(
            rendered, 1,
            "round {round}: the narrated answer must render exactly once, rendered {rendered}"
        );
    }
    Ok(())
}

/// WHEN the final narrated chunk and the turn's spine completion are pending at
/// one wake, THE render loop SHALL fold the narration lane before the spine arm:
/// the completion closes the streamed block, so a chunk folded after it pushes
/// the whole answer into a second transcript entry.
#[tokio::test]
async fn narration_chunk_folds_before_a_turn_completion_pending_on_the_same_wake() -> Result<()> {
    // -- Setup & Fixtures
    const ROUNDS: usize = 16;
    let mut driver = RenderLoopDriver::new(160, 40);

    // -- Exec & Check: the turn's release rides the spine, not the ring record,
    // so the spine step is the completion this pin pairs with the chunk.
    for round in 0..ROUNDS {
        let answer = format!("narrated {round}");
        let call = InferenceId::mint();
        driver.advance(&[narration_chunk(call, &answer)]).await?;
        driver
            .advance_burst(&[
                narration_chunk(call, &format!("{answer} final")),
                DriveEvent::Lifecycle(Box::new(LifecycleEvent::TurnCompleted {
                    task_id: None,
                    duration_ms: 1,
                })),
            ])
            .await?;

        let rendered = driver
            .state()
            .transcript
            .entries()
            .iter()
            .filter(|entry| {
                matches!(entry, TranscriptEntry::Message(message) if message.markdown.contains(&answer))
            })
            .count();
        assert_eq!(
            rendered, 1,
            "round {round}: the narrated answer must render exactly once, rendered {rendered}"
        );
    }
    Ok(())
}

/// WHEN an inference's `Started` and its own first chunk are pending at one
/// wake, THE render loop SHALL render the streamed opening in exactly one
/// transcript entry: the text lane drains first, so the chunk folds before the
/// record that opens its block, and the fold must adopt it rather than orphan
/// it.
#[tokio::test]
async fn started_pending_behind_its_own_first_chunk_renders_the_opening_once() -> Result<()> {
    // -- Setup & Fixtures
    const ROUNDS: usize = 16;

    // -- Exec & Check: which subscription the loop's select wakes on is the
    // loop's own business, so each round publishes both lanes before any yield:
    // the `Started` record and the call's first chunk are pending at the wake
    // that follows. The call's next delta lands on a later wake, exactly as a
    // slow first token does in production.
    for round in 0..ROUNDS {
        let mut driver = RenderLoopDriver::new(160, 40);
        let opening = format!("opening {round}");
        let call = InferenceId::mint();
        driver
            .advance_burst(&[
                DriveEvent::Record(Box::new(SessionRecord::Inference(
                    InferenceRecord::Started { inference: call },
                ))),
                DriveEvent::TextRecord(Box::new(streamed_chunk(call, &opening))),
            ])
            .await?;
        driver
            .advance(&[DriveEvent::TextRecord(Box::new(streamed_chunk(
                call,
                &format!("{opening} and more"),
            )))])
            .await?;

        let rendered = driver
            .state()
            .transcript
            .entries()
            .iter()
            .filter(|entry| {
                matches!(entry, TranscriptEntry::Message(message) if message.markdown.contains(&opening))
            })
            .count();
        assert_eq!(
            rendered, 1,
            "round {round}: the streamed opening must render exactly once, rendered {rendered}"
        );
    }
    Ok(())
}

/// WHEN one call's records and the next call's first chunk are pending at one
/// wake, THE render loop SHALL keep the previous call's block and its tool
/// entries: a chunk from another call closes the block it finds and opens its
/// own, it never truncates the answer that came before it.
#[tokio::test]
async fn next_calls_chunk_pending_behind_a_tool_boundary_keeps_both_blocks() -> Result<()> {
    // -- Setup & Fixtures
    const ROUNDS: usize = 16;

    // -- Exec & Check: the setup round streams inference N and opens its block
    // on its own wake; the burst then holds N's remaining records and inference
    // N+1's first chunk, so the text lane drains N+1's chunk ahead of every
    // record in the batch.
    for round in 0..ROUNDS {
        let mut driver = RenderLoopDriver::new(160, 40);
        let answer = format!("answer {round}");
        let call = InferenceId::mint();
        driver
            .advance(&[
                DriveEvent::Record(Box::new(SessionRecord::Inference(
                    InferenceRecord::Started { inference: call },
                ))),
                DriveEvent::TextRecord(Box::new(streamed_chunk(call, &answer))),
            ])
            .await?;
        let next = InferenceId::mint();
        driver
            .advance_burst(&[
                DriveEvent::TextRecord(Box::new(streamed_chunk(call, &format!("{answer} final")))),
                DriveEvent::Record(Box::new(SessionRecord::Inference(
                    InferenceRecord::Completed {
                        usage: Usage {
                            input_tokens: 0,
                            output_tokens: 0,
                            total_tokens: None,
                            cost: None,
                            cost_complete: false,
                        },
                    },
                ))),
                DriveEvent::Record(Box::new(SessionRecord::Tool(ToolRecord::Started {
                    call: tool_call(&answer),
                }))),
                DriveEvent::Record(Box::new(SessionRecord::Tool(ToolRecord::Completed {
                    call: tool_call(&answer),
                    outcome: ToolOutcome {
                        status: ToolStatus::Success,
                        payload: None,
                        text: "ok".to_string(),
                        message: None,
                        kind: None,
                    },
                }))),
                DriveEvent::TextRecord(Box::new(streamed_chunk(next, "next answer"))),
            ])
            .await?;

        let entries = driver.state().transcript.entries();
        let answer_rank = entries
            .iter()
            .position(|entry| {
                matches!(entry, TranscriptEntry::Message(message) if message.markdown.contains(&answer))
            })
            .ok_or("the previous call's answer must keep its block")?;
        let answer_count = entries
            .iter()
            .filter(|entry| {
                matches!(entry, TranscriptEntry::Message(message) if message.markdown.contains(&answer))
            })
            .count();
        assert_eq!(
            answer_count, 1,
            "round {round}: the previous call's answer must render exactly once, rendered {answer_count}"
        );

        let tool_ranks = entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| matches!(entry, TranscriptEntry::Tool(_)))
            .map(|(rank, _)| rank)
            .collect::<Vec<_>>();
        assert_eq!(
            tool_ranks.len(),
            1,
            "round {round}: one call must leave one tool entry, got {tool_ranks:?}"
        );
        assert!(
            answer_rank < tool_ranks[0],
            "round {round}: the tool entry must land below the answer it followed, \
             got the answer at {answer_rank} and the tool at {}",
            tool_ranks[0]
        );
    }
    Ok(())
}

// -- Test Support

/// The call the tool-boundary pin dispatches: one named call with fixed
/// arguments, so its started and completed records name the same call.
fn tool_call(label: &str) -> ToolCall {
    ToolCall::new(
        "sentinel_tool",
        agnt_runtime::tools::handler::ToolSource::Builtin,
        format!("{{\"label\":\"{label}\"}}"),
    )
}

/// The text-lane record a compaction's summary stream emits: one delta carrying
/// the whole summary so far.
fn summary_chunk(aggregated: &str) -> SessionRecord {
    SessionRecord::Compaction(CompactionRecord::SummaryChunk {
        source: CompactionSource::Auto,
        delta: aggregated.to_string(),
        aggregated: aggregated.to_string(),
    })
}

/// The narration event a streaming producer emits: one delta carrying the whole
/// text so far, on the narration lane, under the call it belongs to.
fn narration_chunk(inference: InferenceId, aggregated: &str) -> DriveEvent {
    DriveEvent::Narration(Box::new(NarrationEvent::InferenceChunk {
        inference,
        delta: aggregated.to_string(),
        aggregated: aggregated.to_string(),
    }))
}
