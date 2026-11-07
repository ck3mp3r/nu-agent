use super::*;

#[test]
fn plan_search_replace_from_content_table() -> Result<()> {
    // -- Setup & Fixtures
    let content = "hello world";
    let operation = EditOperation {
        search: "world".to_string(),
        replacement: "rust".to_string(),
        match_mode: EditMatchMode::Literal,
        occurrence: EditOccurrence::First,
    };

    // -- Exec
    let changed = plan_search_replace_from_content(content, &version_token(content), &operation)
        .map_err(|e| format!("{e:?}"))?;
    let stale = plan_search_replace_from_content(content, "stale-version", &operation)
        .map_err(|e| format!("{e:?}"))?;

    // -- Check
    assert_eq!(changed.new_content, "hello rust");
    assert_eq!(changed.replacements, 1);
    assert!(changed.would_change);
    assert!(!changed.conflict);

    assert!(stale.conflict);
    assert!(!stale.would_change);
    assert_eq!(stale.replacements, 0);
    assert_eq!(stale.new_content, content);
    Ok(())
}

#[test]
fn plan_create_from_content_table() -> Result<()> {
    // -- Setup & Fixtures
    let content = "hello world\n";

    // -- Exec
    let fresh = plan_create_from_content(content, None);
    let existing = plan_create_from_content("new content", Some("existing".to_string()));

    // -- Check
    assert!(fresh.would_change);
    assert!(!fresh.conflict);
    assert_eq!(fresh.previous_content, "");
    assert_eq!(fresh.new_content, content);

    assert!(existing.conflict);
    assert!(!existing.would_change);
    assert_eq!(existing.previous_content, "existing");
    assert_eq!(existing.new_content, "existing");
    Ok(())
}
