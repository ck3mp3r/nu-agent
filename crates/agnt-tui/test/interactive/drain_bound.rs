use super::*;

// ---------------------------------------------------------------------------
// The stream arm's drain bound (task 49f6f14c)
// ---------------------------------------------------------------------------

/// WHEN more records are queued than the cap, THE drain SHALL take at most the
/// cap, leave the rest on the ring, and continue from the ring's next record on
/// the following wake — publication order preserved across the split.
#[test]
fn test_render_loop_stream_drain_is_capped_per_wake() -> Result<()> {
    // -- Setup & Fixtures: the cap plus five records queued on the event lane.
    let bus = Bus::default();
    let mut stream_rx = bus.stream().subscribe();
    let total = u64::try_from(STREAM_DRAIN_MAX)? + 5;
    for duration_ms in 0..total {
        bus.stream()
            .publish(turn_completed(duration_ms))
            .map_err(|_| "should publish every fixture record")?;
    }

    // -- Exec: the first wake takes the record it awaited plus its queued tail.
    let first = stream_rx
        .try_recv()
        .map_err(|_| "the awaited record must be on the lane")?;
    let batch =
        super::super::render_loop::cap_stream_batch(&mut stream_rx, first, STREAM_DRAIN_MAX);

    // -- Check: exactly the cap folds in one wake, in publication order.
    assert_eq!(
        batch.len(),
        STREAM_DRAIN_MAX,
        "one wake folds at most the cap"
    );
    assert_eq!(
        record_durations(&batch),
        (0..u64::try_from(STREAM_DRAIN_MAX)?).collect::<Vec<_>>(),
        "the first wake folds the oldest records"
    );

    // -- Exec: the next wake continues from the ring's next record.
    let next = stream_rx
        .try_recv()
        .map_err(|_| "the tail past the cap must stay queued")?;
    let rest = super::super::render_loop::cap_stream_batch(&mut stream_rx, next, STREAM_DRAIN_MAX);

    // -- Check: the remainder folds in order, and both wakes drain the ring.
    assert_eq!(
        record_durations(&rest),
        (u64::try_from(STREAM_DRAIN_MAX)?..total).collect::<Vec<_>>(),
        "the later wake resumes at the ring's next record"
    );
    assert!(
        stream_rx.try_recv().is_err(),
        "the ring is empty once both wakes folded"
    );
    Ok(())
}

/// REGRESSION: WHEN fewer records than the cap are queued, THE drain SHALL fold
/// them in one wake exactly as it did before the bound.
#[test]
fn test_render_loop_stream_drain_below_the_cap_folds_in_one_wake() -> Result<()> {
    // -- Setup & Fixtures: three records, well under the cap.
    let bus = Bus::default();
    let mut stream_rx = bus.stream().subscribe();
    for duration_ms in 0..3 {
        bus.stream()
            .publish(turn_completed(duration_ms))
            .map_err(|_| "should publish every fixture record")?;
    }

    // -- Exec
    let first = stream_rx
        .try_recv()
        .map_err(|_| "the awaited record must be on the lane")?;
    let batch =
        super::super::render_loop::cap_stream_batch(&mut stream_rx, first, STREAM_DRAIN_MAX);

    // -- Check: one wake folds all three, and nothing is left queued.
    assert_eq!(record_durations(&batch), vec![0, 1, 2]);
    assert!(
        stream_rx.try_recv().is_err(),
        "a burst under the cap leaves nothing queued"
    );
    Ok(())
}

/// WHEN a burst beyond the cap lands in one wake, THE loop SHALL fold every
/// record across the split wakes: the tail is queued, never dropped, and the
/// entries land in publication order.
#[tokio::test]
async fn test_render_loop_folds_a_burst_beyond_the_cap_across_wakes() -> Result<()> {
    // -- Setup & Fixtures: a live loop that hands back its transcript when it
    // stops.
    let bus = Bus::default();
    let bus_for_loop = bus.clone();
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    let cancel_controller = CancelController::default();
    let (event_tx, _event_rx) = mpsc::channel::<OrchestratorEvent>(64);
    let (terminal_tx, terminal_rx) = mpsc::channel::<TerminalInput>(64);
    let (_branch_tx, branch_rx) = mpsc::channel::<()>(8);
    let mut live: Option<&mut ratatui::Terminal<ratatui::backend::TestBackend>> = None;

    let loop_handle = tokio::spawn(async move {
        run_render_loop(
            &mut coordinator,
            &cancel_controller,
            event_tx,
            terminal_rx,
            &mut live,
            crate::interactive::RenderLoopChannels {
                branch_rx,
                theme_persist_tx: None,
                stream: bus_for_loop.stream(),
                narration: bus_for_loop.narration(),
                control: bus_for_loop.control(),
                lifecycle: bus_for_loop.lifecycle(),
                facts: bus_for_loop.facts(),
            },
        )
        .await;
        coordinator
            .state
            .transcript
            .entries()
            .iter()
            .map(entry_text)
            .collect::<Vec<_>>()
    });

    // -- Exec: wait for the loop to subscribe (a ring with no subscriber
    // rejects a publish), then publish cap plus five distinct tool calls in one
    // burst and give the loop room to fold them across its split wakes.
    for _ in 0..100 {
        if bus.stream().subscriber_count() > 0 {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(
        bus.stream().subscriber_count() > 0,
        "the loop must subscribe before the burst is published"
    );
    let total = u64::try_from(STREAM_DRAIN_MAX)? + 5;
    for ordinal in 0..total {
        bus.stream()
            .publish(tool_start(ordinal))
            .map_err(|_| "should publish every fixture record")?;
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    drop(terminal_tx);
    let texts = tokio::time::timeout(Duration::from_secs(1), loop_handle).await??;

    // -- Check: every record folded exactly once, in publication order.
    assert_eq!(
        texts.len(),
        usize::try_from(total)?,
        "the tail past the cap must fold on later wakes"
    );
    for (ordinal, text) in texts.iter().enumerate() {
        assert!(
            text.contains(&call_argument(u64::try_from(ordinal)?)),
            "entry {ordinal} must be the record published at that position: {text}"
        );
    }
    Ok(())
}

// -- Test Support

/// One turn-completion record, distinguished by its duration.
fn turn_completed(duration_ms: u64) -> SessionRecord {
    SessionRecord::Turn(TurnRecord::Completed { duration_ms })
}

/// The duration each drained turn-completion record carries.
fn record_durations(records: &[SessionRecord]) -> Vec<u64> {
    records
        .iter()
        .filter_map(|record| match record {
            SessionRecord::Turn(TurnRecord::Completed { duration_ms }) => Some(*duration_ms),
            _ => None,
        })
        .collect()
}

/// One started tool call, distinguished by its ordinal.
fn tool_start(ordinal: u64) -> SessionRecord {
    SessionRecord::Tool(ToolRecord::Started {
        call: ToolCall::new(
            ToolName("read".to_string()),
            agnt_runtime::tools::handler::ToolSource::Unknown,
            format!("{{\"ordinal\":{ordinal}}}"),
        ),
    })
}

/// The argument text a fixture call carries, so an entry can be matched to the
/// record that produced it.
fn call_argument(ordinal: u64) -> String {
    format!("ordinal\":{ordinal}")
}
