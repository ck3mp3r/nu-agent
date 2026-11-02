use super::*;

/// A settled turn lands its row, its entry keys and its call rows together.
#[tokio::test]
async fn test_services_settle_records_the_turn_its_keys_and_its_calls() -> Result<()> {
    // -- Setup & Fixtures
    let fixture = Fixture::new()?;
    let turn = fixture.turn_facts(0);
    let keys = [
        MessageKey {
            made: 1_790_974_800_213,
            id: 0,
        },
        MessageKey {
            made: 1_790_974_800_214,
            id: 1,
        },
    ];
    let calls = [
        call_fact(0, "call-a", ToolCallStatus::Success),
        call_fact(0, "call-b", ToolCallStatus::Cancelled),
    ];
    let record = SettleRecord {
        turn: turn.clone(),
        keys: keys.to_vec(),
        calls: calls.to_vec(),
    };

    // -- Exec
    fixture.settle().record(record).await?;

    // -- Check
    assert_eq!(fixture.turns().read(SESSION).await?, vec![turn]);
    assert_eq!(
        fixture.entries().read(SESSION).await?,
        vec![
            EntryTurn {
                key: keys[0],
                turn_seq: 0
            },
            EntryTurn {
                key: keys[1],
                turn_seq: 0
            },
        ]
    );
    assert_eq!(fixture.tool_calls().read(SESSION).await?, calls.to_vec());
    Ok(())
}

/// A unit that fails on one edge writes none of its rows: the turn edge, its
/// keys and its calls land whole or not at all.
#[tokio::test]
async fn test_services_settle_writes_all_three_or_none() -> Result<()> {
    // -- Setup & Fixtures: a call whose session has no turn row to point at.
    let fixture = Fixture::new()?;
    let turn = fixture.turn_facts(0);
    let stray = CallFact {
        session_id: "session-stray".to_owned(),
        ..call_fact(0, "call-a", ToolCallStatus::Success)
    };
    let record = SettleRecord {
        turn: turn.clone(),
        keys: vec![MessageKey {
            made: 1_790_974_800_213,
            id: 0,
        }],
        calls: vec![stray],
    };

    // -- Exec
    let written = fixture.settle().record(record).await;

    // -- Check
    assert!(written.is_err(), "the stray edge is refused");
    assert!(
        fixture.turns().read(SESSION).await?.is_empty(),
        "the refused unit rolled its turn row back"
    );
    assert!(fixture.entries().read(SESSION).await?.is_empty());
    Ok(())
}

/// A turn settled again replaces its rows by key: one turn, one row each.
#[tokio::test]
async fn test_services_settle_replaces_the_rows_by_key() -> Result<()> {
    // -- Setup & Fixtures
    let fixture = Fixture::new()?;
    let turn = fixture.turn_facts(0);
    let key = MessageKey {
        made: 1_790_974_800_213,
        id: 0,
    };
    let call = call_fact(0, "call-a", ToolCallStatus::Failed);
    fixture
        .settle()
        .record(SettleRecord {
            turn: turn.clone(),
            keys: vec![key],
            calls: vec![call.clone()],
        })
        .await?;

    // -- Exec: the same turn settles again, its call having run.
    let mut settled = turn.clone();
    settled.settled_millis += 5_000;
    let success = call_fact(0, "call-a", ToolCallStatus::Success);
    fixture
        .settle()
        .record(SettleRecord {
            turn: settled.clone(),
            keys: vec![key],
            calls: vec![success.clone()],
        })
        .await?;

    // -- Check
    assert_eq!(fixture.turns().read(SESSION).await?, vec![settled]);
    assert_eq!(
        fixture.entries().read(SESSION).await?,
        vec![EntryTurn { key, turn_seq: 0 }],
        "one row per key"
    );
    assert_eq!(fixture.tool_calls().read(SESSION).await?, vec![success]);
    Ok(())
}
