use super::*;

/// The keys a turn persisted read back with the turn they belong to.
#[tokio::test]
async fn test_services_entries_record_round_trips_the_keys() -> Result<()> {
    // -- Setup & Fixtures
    let fixture = Fixture::new()?;
    let turn = fixture.settled_turn(0).await?;
    let keys = vec![
        MessageKey {
            made: 1_790_974_800_344,
            id: 1,
        },
        MessageKey {
            made: 1_790_974_802_201,
            id: 2,
        },
    ];

    // -- Exec
    fixture
        .entries()
        .record(SESSION.to_owned(), turn.turn_seq, keys.clone())
        .await?;
    let read = fixture.entries().read(SESSION).await?;

    // -- Check
    assert_eq!(read.len(), 2, "one row per persisted entry: {read:?}");
    assert!(
        keys.iter()
            .all(|key| read.iter().any(|row| row.key == *key))
    );
    assert!(read.iter().all(|row| row.turn_seq == turn.turn_seq));
    Ok(())
}

/// Re-recording a key moves its attribution, keeping one row per key.
#[tokio::test]
async fn test_services_entries_record_replaces_the_attribution() -> Result<()> {
    // -- Setup & Fixtures
    let fixture = Fixture::new()?;
    let first = fixture.settled_turn(0).await?;
    let second = fixture.settled_turn(1).await?;
    let key = MessageKey {
        made: 1_790_974_800_344,
        id: 1,
    };
    fixture
        .entries()
        .record(SESSION.to_owned(), first.turn_seq, vec![key])
        .await?;

    // -- Exec
    fixture
        .entries()
        .record(SESSION.to_owned(), second.turn_seq, vec![key])
        .await?;
    let read = fixture.entries().read(SESSION).await?;

    // -- Check
    assert_eq!(read.len(), 1, "the key keeps one row: {read:?}");
    assert_eq!(read[0].turn_seq, second.turn_seq);
    Ok(())
}

/// A key whose turn was never recorded is refused: the edge never dangles.
#[tokio::test]
async fn test_services_entries_record_refuses_a_missing_turn() -> Result<()> {
    // -- Setup & Fixtures: no turn row at all.
    let fixture = Fixture::new()?;
    let key = MessageKey { made: 1, id: 1 };

    // -- Exec
    let refused = fixture
        .entries()
        .record(SESSION.to_owned(), 7, vec![key])
        .await;

    // -- Check
    assert!(refused.is_err(), "the turn edge must hold");
    assert!(
        fixture.entries().read(SESSION).await?.is_empty(),
        "nothing is written"
    );
    Ok(())
}

/// A session reads its own edges and never another session's.
#[tokio::test]
async fn test_services_entries_are_scoped_to_their_session() -> Result<()> {
    // -- Setup & Fixtures
    let fixture = Fixture::new()?;
    let turn = fixture.settled_turn(0).await?;
    let other = TurnFacts {
        session_id: "session-other".to_owned(),
        ..fixture.turn_facts(0)
    };
    fixture.turns().record(other.clone()).await?;
    fixture
        .entries()
        .record(
            other.session_id.clone(),
            other.turn_seq,
            vec![MessageKey { made: 9, id: 9 }],
        )
        .await?;

    // -- Exec
    fixture
        .entries()
        .record(
            SESSION.to_owned(),
            turn.turn_seq,
            vec![MessageKey { made: 1, id: 1 }],
        )
        .await?;
    let read = fixture.entries().read(SESSION).await?;

    // -- Check
    assert_eq!(read.len(), 1, "one session's edge only: {read:?}");
    assert_eq!(read[0].key, MessageKey { made: 1, id: 1 });
    Ok(())
}
