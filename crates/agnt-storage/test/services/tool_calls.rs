use super::*;

/// Every settled status round-trips with its own instant.
#[tokio::test]
async fn test_services_tool_calls_record_round_trips_every_status() -> Result<()> {
    // -- Setup & Fixtures
    let fixture = Fixture::new()?;
    let turn = fixture.settled_turn(0).await?;
    let calls = [
        call_fact(turn.turn_seq, "call-ok", ToolCallStatus::Success),
        call_fact(turn.turn_seq, "call-failed", ToolCallStatus::Failed),
        call_fact(turn.turn_seq, "call-cancelled", ToolCallStatus::Cancelled),
    ];

    // -- Exec
    for call in &calls {
        fixture.tool_calls().record(call.clone()).await?;
    }
    let read = fixture.tool_calls().read(SESSION).await?;

    // -- Check
    assert_eq!(read.len(), 3, "one row per call: {read:?}");
    for call in calls {
        assert!(
            read.contains(&call),
            "the row of {} must carry its status and instant: {read:?}",
            call.call
        );
    }
    Ok(())
}

/// Re-recording a call replaces its row, keeping one per call id.
#[tokio::test]
async fn test_services_tool_calls_record_replaces_the_row_by_key() -> Result<()> {
    // -- Setup & Fixtures
    let fixture = Fixture::new()?;
    let turn = fixture.settled_turn(0).await?;
    let mut call = call_fact(turn.turn_seq, "call-1", ToolCallStatus::Cancelled);
    fixture.tool_calls().record(call.clone()).await?;

    // -- Exec: the call settles for real, later.
    call.status = ToolCallStatus::Success;
    call.settled_millis += 5_000;
    fixture.tool_calls().record(call.clone()).await?;
    let read = fixture.tool_calls().read(SESSION).await?;

    // -- Check
    assert_eq!(read, vec![call], "the second write replaces the row");
    Ok(())
}

/// A call whose turn was never recorded is refused: the edge never dangles.
#[tokio::test]
async fn test_services_tool_calls_record_refuses_a_missing_turn() -> Result<()> {
    // -- Setup & Fixtures
    let fixture = Fixture::new()?;
    let call = call_fact(7, "call-1", ToolCallStatus::Success);

    // -- Exec
    let refused = fixture.tool_calls().record(call).await;

    // -- Check
    assert!(refused.is_err(), "the turn edge must hold");
    assert!(fixture.tool_calls().read(SESSION).await?.is_empty());
    Ok(())
}

/// A session reads its own calls and never another session's.
#[tokio::test]
async fn test_services_tool_calls_are_scoped_to_their_session() -> Result<()> {
    // -- Setup & Fixtures
    let fixture = Fixture::new()?;
    let turn = fixture.settled_turn(0).await?;
    let other = TurnFacts {
        session_id: "session-other".to_owned(),
        ..fixture.turn_facts(0)
    };
    fixture.turns().record(other.clone()).await?;
    fixture
        .tool_calls()
        .record(CallFact {
            session_id: other.session_id.clone(),
            ..call_fact(other.turn_seq, "call-other", ToolCallStatus::Success)
        })
        .await?;

    // -- Exec
    fixture
        .tool_calls()
        .record(call_fact(turn.turn_seq, "call-1", ToolCallStatus::Success))
        .await?;
    let read = fixture.tool_calls().read(SESSION).await?;

    // -- Check
    assert_eq!(read.len(), 1, "one session's calls only: {read:?}");
    assert_eq!(read[0].call, "call-1");
    Ok(())
}
