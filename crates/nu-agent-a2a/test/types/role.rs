use super::*;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// ---------------------------------------------------------------------------
// Role
// ---------------------------------------------------------------------------

#[test]
fn role_roundtrip() {
    use serde_test::{Token, assert_tokens};

    assert_tokens(
        &Role::User,
        &[Token::UnitVariant {
            name: "Role",
            variant: "ROLE_USER",
        }],
    );
    assert_tokens(
        &Role::Agent,
        &[Token::UnitVariant {
            name: "Role",
            variant: "ROLE_AGENT",
        }],
    );
}

#[test]
fn role_deserialize_spec_format() -> Result<()> {
    let role: Role = serde_json::from_str("\"ROLE_USER\"")?;
    assert_eq!(role, Role::User);

    let role: Role = serde_json::from_str("\"ROLE_AGENT\"")?;
    assert_eq!(role, Role::Agent);
    Ok(())
}

#[test]
fn role_deserialize_legacy_format_fails() {
    let result: core::result::Result<Role, _> = serde_json::from_str("\"USER\"");
    assert!(result.is_err(), "legacy 'USER' must not deserialize");

    let result: core::result::Result<Role, _> = serde_json::from_str("\"AGENT\"");
    assert!(result.is_err(), "legacy 'AGENT' must not deserialize");
}

#[test]
fn role_label() {
    assert_eq!(Role::User.label(), "user");
    assert_eq!(Role::Agent.label(), "agent");
}
