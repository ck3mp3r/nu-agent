use super::*;

/// A settled turn round-trips every fact it carries.
#[tokio::test]
async fn test_services_turns_record_round_trips_every_fact() -> Result<()> {
    // -- Setup & Fixtures
    let fixture = Fixture::new()?;
    let facts = fixture.turn_facts(0);

    // -- Exec
    fixture.turns().record(facts.clone()).await?;
    let read = fixture.turns().read(SESSION).await?;

    // -- Check
    assert_eq!(read, vec![facts]);
    Ok(())
}

/// The same turn recorded twice replaces its row, keeping one per key.
#[tokio::test]
async fn test_services_turns_record_replaces_the_row_by_key() -> Result<()> {
    // -- Setup & Fixtures
    let fixture = Fixture::new()?;
    let mut facts = fixture.turn_facts(0);
    fixture.turns().record(facts.clone()).await?;

    // -- Exec: the same turn settles again with a later instant.
    facts.settled_millis += 1_000;
    fixture.turns().record(facts.clone()).await?;
    let read = fixture.turns().read(SESSION).await?;

    // -- Check
    assert_eq!(read, vec![facts], "the second write replaces the row");
    Ok(())
}

/// A session reads its turns in order and never sees another session's.
#[tokio::test]
async fn test_services_turns_are_ordered_and_scoped_to_their_session() -> Result<()> {
    // -- Setup & Fixtures
    let fixture = Fixture::new()?;
    let second = fixture.turn_facts(1);
    let other = TurnFacts {
        session_id: "session-other".to_owned(),
        ..fixture.turn_facts(0)
    };
    fixture.turns().record(second.clone()).await?;
    let first = fixture.turn_facts(0);
    fixture.turns().record(first.clone()).await?;
    fixture.turns().record(other).await?;

    // -- Exec
    let read = fixture.turns().read(SESSION).await?;

    // -- Check
    assert_eq!(
        read,
        vec![first, second],
        "ordered by turn_seq, one session"
    );
    Ok(())
}

/// A session that never settled a turn reads none.
#[tokio::test]
async fn test_services_turns_read_empty_for_an_unknown_session() -> Result<()> {
    // -- Setup & Fixtures
    let fixture = Fixture::new()?;

    // -- Exec
    let read = fixture.turns().read("session-unknown").await?;

    // -- Check
    assert!(read.is_empty(), "no turn, no row");
    Ok(())
}
