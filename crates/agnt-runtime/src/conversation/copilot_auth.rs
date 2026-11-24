//! The one boundary that reads rig's Copilot auth text.
//!
//! rig 0.47's `AuthError` (`rig-core-0.47.0/src/providers/auth.rs`) has six
//! variants: `Message`, `Io`, `Json`, `Http`, `Reply`, `Store`. None of them
//! names a missing credential. The Copilot authenticator reports that state as
//! `AuthError::Message` carrying the sentence written in `sign_in_to_github`
//! (`rig-core-0.47.0/src/providers/copilot/auth/native.rs`), and the same
//! variant also carries unrelated failures, so the variant alone does not
//! classify. This module reads the sentence, and nothing else does.

use rig::providers::auth::AuthError;

/// The sentence rig's Copilot authenticator writes when it holds no credential
/// and the device flow is not allowed.
///
/// `copilot_without_a_credential_reports_sign_in_required` pins it against the
/// real producer through a cold token store; the `copilot_auth_error` unit
/// tests pin both branches from a typed `AuthError`.
const SIGN_IN_REQUIRED: &str =
    "GitHub Copilot sign-in required. Reconnect Copilot in Settings before using this provider.";

/// Whether `error` is rig reporting a credential that only an interactive
/// sign-in can supply.
pub(super) fn is_sign_in_required(error: &AuthError) -> bool {
    matches!(error, AuthError::Message(text) if text.as_str() == SIGN_IN_REQUIRED)
}
