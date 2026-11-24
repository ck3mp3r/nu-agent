//!
//! `copilot_auth_error` classifies a rig `AuthError` by variant, not by shape.
//!
//! rig 0.47's `AuthError` has no missing-credential variant: the Copilot
//! authenticator reports that state as `AuthError::Message` carrying one fixed
//! sentence (`rig-core-0.47.0/src/providers/copilot/auth/native.rs`,
//! `sign_in_to_github`). These pins fix both branches from typed inputs — the
//! sentence earns the sign-in hint, every other `AuthError` keeps the generic
//! message, and the sibling sentence rig's ChatGPT authenticator writes
//! (`providers/chatgpt/auth/native.rs`, `sign_in_to_chatgpt`) stays unclaimed by
//! the Copilot boundary.

use super::*;

/// The sentence rig's Copilot authenticator writes when it holds no credential
/// and the device flow is not allowed. Written out here rather than imported, so
/// a reword on either side breaks this pin.
const SIGN_IN_REQUIRED: &str =
    "GitHub Copilot sign-in required. Reconnect Copilot in Settings before using this provider.";

/// The sibling sentence rig's ChatGPT authenticator writes, in the same shape
/// and from the same `AuthError::Message` variant. Written out here for the same
/// reason: it pins the boundary's exactness, not a shared substring.
const CHATGPT_SIGN_IN_REQUIRED: &str =
    "ChatGPT sign-in required. Reconnect ChatGPT in Settings before using this provider.";

#[test]
fn test_providers_copilot_auth_error_names_the_login_command_for_a_missing_credential() -> Result<()>
{
    // -- Setup & Fixtures
    let error = crate::types::AuthError::Message(SIGN_IN_REQUIRED.to_string());

    // -- Exec
    let message = copilot_auth_error(error).msg;

    // -- Check
    assert!(
        message.contains("agent provider auth login github-copilot"),
        "a missing credential must name the sign-in command: {message}"
    );
    Ok(())
}

#[test]
fn test_providers_copilot_auth_error_keeps_the_generic_message_for_another_message() -> Result<()> {
    // -- Setup & Fixtures: rig's other `Message` text, from the token exchange
    // rather than from the sign-in.
    let error = crate::types::AuthError::Message(
        "GitHub Copilot API key response did not include a token".to_string(),
    );

    // -- Exec
    let message = copilot_auth_error(error).msg;

    // -- Check
    assert!(
        message.contains("GitHub Copilot authentication failed:"),
        "a failed token exchange keeps the generic message: {message}"
    );
    assert!(
        !message.contains("agent provider auth login"),
        "a failed token exchange must not claim the user is signed out: {message}"
    );
    Ok(())
}

#[test]
fn test_providers_copilot_auth_error_keeps_the_generic_message_for_a_store_failure() -> Result<()> {
    // -- Setup & Fixtures
    let error = crate::types::AuthError::Store("token store unreadable".into());

    // -- Exec
    let message = copilot_auth_error(error).msg;

    // -- Check
    assert!(
        message.contains("token store failed: token store unreadable"),
        "rig's text stays in the message: {message}"
    );
    assert!(
        !message.contains("agent provider auth login"),
        "a broken store must not claim the user is signed out: {message}"
    );
    Ok(())
}

#[test]
fn test_providers_copilot_auth_error_never_claims_a_chatgpt_sign_in() -> Result<()> {
    // -- Setup & Fixtures: rig's other authenticator, whose sentence carries the
    // same shape and names a different provider.
    let error = crate::types::AuthError::Message(CHATGPT_SIGN_IN_REQUIRED.to_string());

    // -- Exec
    let message = copilot_auth_error(error).msg;

    // -- Check
    assert!(
        message.contains("GitHub Copilot authentication failed:"),
        "a sibling provider's sentence keeps the generic message: {message}"
    );
    assert!(
        !message.contains("agent provider auth login"),
        "the Copilot hint must not answer a ChatGPT credential: {message}"
    );
    Ok(())
}
