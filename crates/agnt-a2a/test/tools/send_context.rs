//! The send tool's `contextId` handling: pass-through, refusal, omission.

use crate::tools::send::{SEND_CONTEXT_RULE, resolve_send_context};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn test_tools_send_context_omitted_stays_absent() -> Result<()> {
    // -- Exec
    let resolved = resolve_send_context(None).map_err(|e| format!("{e:?}"))?;

    // -- Check: no param, no contextId field; the target boundary mints.
    assert_eq!(
        resolved, None,
        "an omitted contextId must put no contextId field on the wire"
    );
    Ok(())
}

#[test]
fn test_tools_send_context_forwards_an_explicit_id_verbatim() -> Result<()> {
    // -- Exec
    let resolved = resolve_send_context(Some("ctx-explicit")).map_err(|e| format!("{e:?}"))?;

    // -- Check
    assert_eq!(
        resolved.as_deref(),
        Some("ctx-explicit"),
        "a provided contextId must be forwarded exactly as given"
    );
    Ok(())
}

#[test]
fn test_tools_send_context_refuses_an_empty_id() {
    // -- Exec
    let error = resolve_send_context(Some("")).err().unwrap_or_default();

    // -- Check: refused with the round-trip rule, before any HTTP call.
    assert!(
        error.contains(SEND_CONTEXT_RULE),
        "the refusal must carry the round-trip rule; got: {error}"
    );
}

#[test]
fn test_tools_send_context_refuses_a_whitespace_id() {
    // -- Exec
    let error = resolve_send_context(Some("   ")).err().unwrap_or_default();

    // -- Check
    assert!(
        error.contains(SEND_CONTEXT_RULE),
        "the refusal must carry the round-trip rule; got: {error}"
    );
}
