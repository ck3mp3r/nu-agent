use crate::session_key::{derive_session_key, resolve_context_id};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn test_session_key_derive_joins_prefix_and_context() -> Result<()> {
    // -- Setup & Fixtures
    let prefix = "9dad1e4e08b0b11c";
    let context_id = "ctx-abc";

    // -- Exec
    let key = derive_session_key(prefix, context_id);

    // -- Check
    assert_eq!(key, "9dad1e4e08b0b11c-ctx-abc");
    Ok(())
}

#[test]
fn test_session_key_derive_is_stable_for_same_inputs() -> Result<()> {
    // -- Setup & Fixtures
    let prefix = "9dad1e4e08b0b11c";

    // -- Exec
    let a = derive_session_key(prefix, "ctx-abc");
    let b = derive_session_key(prefix, "ctx-abc");

    // -- Check
    assert_eq!(a, b);
    Ok(())
}

#[test]
fn test_session_key_derive_differs_per_context() -> Result<()> {
    // -- Setup & Fixtures
    let prefix = "9dad1e4e08b0b11c";

    // -- Exec
    let a = derive_session_key(prefix, "ctx-abc");
    let b = derive_session_key(prefix, "ctx-def");

    // -- Check
    assert_ne!(a, b);
    Ok(())
}

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
