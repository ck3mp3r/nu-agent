use crate::session_key::resolve_context_id;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn test_session_key_resolve_context_id_keeps_provided_value() -> Result<()> {
    // -- Exec
    let resolved = resolve_context_id(Some("ctx-abc".to_string()));

    // -- Check
    assert_eq!(resolved, "ctx-abc");
    Ok(())
}

#[test]
fn test_session_key_resolve_context_id_mints_uuid_when_absent() -> Result<()> {
    // -- Exec
    let resolved = resolve_context_id(None);

    // -- Check
    assert_eq!(resolved.len(), 36);
    assert!(uuid::Uuid::parse_str(&resolved).is_ok());
    Ok(())
}

#[test]
fn test_session_key_resolve_context_id_mints_distinct_uuids() -> Result<()> {
    // -- Exec
    let a = resolve_context_id(None);
    let b = resolve_context_id(None);

    // -- Check
    assert_ne!(a, b);
    Ok(())
}

#[test]
fn test_session_key_resolve_context_id_mints_on_empty() -> Result<()> {
    // -- Exec
    let resolved = resolve_context_id(Some(String::new()));

    // -- Check: an empty id is an absent id, so the boundary mints.
    assert!(
        uuid::Uuid::parse_str(&resolved).is_ok(),
        "an empty id must mint a fresh UUID; got: {resolved}"
    );
    Ok(())
}

#[test]
fn test_session_key_resolve_context_id_mints_on_whitespace() -> Result<()> {
    // -- Exec
    let resolved = resolve_context_id(Some("   ".to_string()));

    // -- Check
    assert!(
        uuid::Uuid::parse_str(&resolved).is_ok(),
        "a whitespace-only id must mint a fresh UUID; got: {resolved}"
    );
    Ok(())
}
