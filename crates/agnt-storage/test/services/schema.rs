use super::*;

/// M2 is the schema this build migrates a database to.
#[tokio::test]
async fn test_services_migration_migrates_to_m2() -> Result<()> {
    // -- Setup & Fixtures
    let fixture = Fixture::new()?;

    // -- Exec
    let version = version(&fixture.storage).await?;

    // -- Check
    assert_eq!(version, 2, "a fresh database migrates to M2");
    Ok(())
}

/// M1's tables survive and M2's three tables are added.
#[tokio::test]
async fn test_services_migration_holds_the_baseline_and_the_fact_tables() -> Result<()> {
    // -- Setup & Fixtures
    let fixture = Fixture::new()?;

    // -- Exec
    let tables = tables(&fixture.storage).await?;

    // -- Check
    assert_eq!(
        tables,
        [
            "entry_turns",
            "files",
            "logs",
            "sessions",
            "tool_calls",
            "turns"
        ],
        "the baseline and the fact tables coexist"
    );
    Ok(())
}

/// No M2 column is nullable: an unknown fact is a missing row, never a NULL.
#[tokio::test]
async fn test_services_migration_no_fact_column_is_nullable() -> Result<()> {
    // -- Setup & Fixtures
    let fixture = Fixture::new()?;
    let mut nullable = Vec::new();

    // -- Exec
    for table in ["turns", "entry_turns", "tool_calls"] {
        for (column, not_null) in columns(&fixture.storage, table).await? {
            if !not_null {
                nullable.push(format!("{table}.{column}"));
            }
        }
    }

    // -- Check
    assert!(
        nullable.is_empty(),
        "every fact column is NOT NULL; nullable: {nullable:?}"
    );
    Ok(())
}

/// The call status is checked against the three outcomes the host settles.
#[tokio::test]
async fn test_services_migration_tool_status_is_checked() -> Result<()> {
    // -- Setup & Fixtures
    let fixture = Fixture::new()?;

    // -- Exec
    let ddl = table_sql(&fixture.storage, "tool_calls").await?;

    // -- Check
    assert!(
        ddl.contains("CHECK"),
        "the status SHALL be constrained, got: {ddl}"
    );
    for status in ["success", "failed", "cancelled"] {
        assert!(
            ddl.contains(status),
            "the constraint SHALL name {status}, got: {ddl}"
        );
    }
    Ok(())
}

/// The entry edge references the turn it was written under.
#[tokio::test]
async fn test_services_migration_entry_edge_references_turns() -> Result<()> {
    // -- Setup & Fixtures
    let fixture = Fixture::new()?;

    // -- Exec
    let ddl = table_sql(&fixture.storage, "entry_turns").await?;

    // -- Check
    assert!(
        ddl.contains("REFERENCES turns"),
        "the edge SHALL reference turns, got: {ddl}"
    );
    Ok(())
}
