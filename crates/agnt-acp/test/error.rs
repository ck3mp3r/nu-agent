use super::*;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// ---------------------------------------------------------------------------
// AcpError — Display output for every variant
// ---------------------------------------------------------------------------

#[test]
fn test_error_display_config() -> Result<()> {
    // -- Setup & Fixtures
    let err = AcpError::Config("missing provider".to_string());

    // -- Exec
    let display = err.to_string();

    // -- Check
    assert_eq!(display, "configuration error: missing provider");
    Ok(())
}

#[test]
fn test_error_display_session_not_found() -> Result<()> {
    // -- Setup & Fixtures
    let err = AcpError::SessionNotFound("sess-1".to_string());

    // -- Exec
    let display = err.to_string();

    // -- Check
    assert_eq!(display, "session not found: sess-1");
    Ok(())
}

#[test]
fn test_error_display_session_already_exists() -> Result<()> {
    // -- Setup & Fixtures
    let err = AcpError::SessionAlreadyExists("sess-2".to_string());

    // -- Exec
    let display = err.to_string();

    // -- Check
    assert_eq!(display, "session already exists: sess-2");
    Ok(())
}

#[test]
fn test_error_display_invalid_session_state() -> Result<()> {
    // -- Setup & Fixtures
    let err = AcpError::InvalidSessionState("prompt in flight".to_string());

    // -- Exec
    let display = err.to_string();

    // -- Check
    assert_eq!(display, "invalid session state: prompt in flight");
    Ok(())
}

#[test]
fn test_error_display_runtime() -> Result<()> {
    // -- Setup & Fixtures
    let err = AcpError::Runtime("transport closed".to_string());

    // -- Exec
    let display = err.to_string();

    // -- Check
    assert_eq!(display, "runtime error: transport closed");
    Ok(())
}

// ---------------------------------------------------------------------------
// From<agent_client_protocol::Error>
// ---------------------------------------------------------------------------

#[test]
fn test_error_from_protocol_error() -> Result<()> {
    // -- Setup & Fixtures
    let protocol_err = agent_client_protocol::Error::invalid_params();

    // -- Exec
    let acp_err: AcpError = protocol_err.into();

    // -- Check
    assert!(matches!(acp_err, AcpError::Runtime(_)));
    assert!(acp_err.to_string().starts_with("runtime error:"));
    Ok(())
}

// ---------------------------------------------------------------------------
// Clone + PartialEq
// ---------------------------------------------------------------------------

#[test]
fn test_error_clone_and_partial_eq() -> Result<()> {
    // -- Setup & Fixtures
    let err = AcpError::Runtime("boom".to_string());

    // -- Exec
    let cloned = err.clone();

    // -- Check
    assert_eq!(err, cloned);
    Ok(())
}

#[test]
fn test_error_different_variants_not_equal() -> Result<()> {
    // -- Setup & Fixtures
    let a = AcpError::Config("x".to_string());
    let b = AcpError::Runtime("x".to_string());

    // -- Exec & Check
    assert_ne!(a, b);
    Ok(())
}
