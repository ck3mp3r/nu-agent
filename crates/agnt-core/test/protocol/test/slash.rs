use super::*;

// === Tests: slash ===

#[test]
fn parse_slash_command_exact_matches() {
    let cases: &[(&str, SlashCommand)] = &[
        ("   /compact   ", SlashCommand::Compact),
        (" /mcp ", SlashCommand::Mcp),
        (" /help ", SlashCommand::Help),
        (" /status ", SlashCommand::Status),
        (" /models ", SlashCommand::Models),
        ("/agent", SlashCommand::Agent),
        ("/session", SlashCommand::Session),
        ("/SESSION", SlashCommand::Session),
        ("/session abc123", SlashCommand::Session),
    ];

    for (input, expected) in cases {
        assert_eq!(
            parse_slash_command(input),
            SlashParseResult::Command(*expected),
            "input={input}"
        );
    }
}

#[test]
fn parse_slash_command_negative_cases() {
    assert_eq!(
        parse_slash_command("hello world"),
        SlashParseResult::NotSlash
    );
    assert_eq!(
        parse_slash_command("/compact now"),
        SlashParseResult::Unknown("/compact now".to_string())
    );
    assert_eq!(
        parse_slash_command("/agents"),
        SlashParseResult::Unknown("/agents".to_string())
    );
}

#[test]
fn inline_slash_filter_is_prefix_based_and_deterministic() {
    assert_eq!(
        filter_inline_slash_suggestions("/"),
        vec![
            SlashCommand::Compact,
            SlashCommand::Mcp,
            SlashCommand::Help,
            SlashCommand::Status,
            SlashCommand::Models,
            SlashCommand::Agent,
            SlashCommand::New,
            SlashCommand::Session,
            SlashCommand::Theme,
            SlashCommand::Skills,
        ]
    );
    assert_eq!(
        filter_inline_slash_suggestions("/c"),
        vec![SlashCommand::Compact]
    );
    assert_eq!(
        filter_inline_slash_suggestions("/co"),
        vec![SlashCommand::Compact]
    );
    assert_eq!(
        filter_inline_slash_suggestions("/m"),
        vec![SlashCommand::Mcp, SlashCommand::Models]
    );
    assert!(filter_inline_slash_suggestions("/x").is_empty());
    assert!(filter_inline_slash_suggestions("hello").is_empty());
}

#[test]
fn slash_command_catalog_exports_expected_labels_and_order() {
    assert_eq!(
        SLASH_COMMAND_ORDER,
        [
            SlashCommand::Compact,
            SlashCommand::Mcp,
            SlashCommand::Help,
            SlashCommand::Status,
            SlashCommand::Models,
            SlashCommand::Agent,
            SlashCommand::New,
            SlashCommand::Session,
            SlashCommand::Theme,
            SlashCommand::Skills,
        ]
    );

    assert_eq!(SlashCommand::Compact.label(), "/compact");
    assert_eq!(SlashCommand::Mcp.label(), "/mcp");
    assert_eq!(SlashCommand::Help.label(), "/help");
    assert_eq!(SlashCommand::Status.label(), "/status");
    assert_eq!(SlashCommand::Models.label(), "/models");
    assert_eq!(SlashCommand::Agent.label(), "/agent");
    assert_eq!(SlashCommand::New.label(), "/new");
    assert_eq!(SlashCommand::Session.label(), "/session");
    assert_eq!(SlashCommand::Theme.label(), "/theme");
    assert_eq!(SlashCommand::Skills.label(), "/skills");

    assert!(!SlashCommand::Compact.summary().is_empty());
    assert!(!SlashCommand::Mcp.summary().is_empty());
    assert!(!SlashCommand::Help.summary().is_empty());
    assert!(!SlashCommand::Status.summary().is_empty());
    assert!(!SlashCommand::Models.summary().is_empty());
    assert!(!SlashCommand::Agent.summary().is_empty());
    assert!(!SlashCommand::New.summary().is_empty());
    assert!(!SlashCommand::Session.summary().is_empty());
    assert!(!SlashCommand::Theme.summary().is_empty());
    assert!(!SlashCommand::Skills.summary().is_empty());

    assert_eq!(SlashCommand::Agent.label(), "/agent");
    assert_eq!(SlashCommand::Agent.summary(), "Switch agent persona");
}

#[test]
fn extract_session_id_variants() {
    assert_eq!(extract_session_id("/session"), None);
    assert_eq!(extract_session_id("/session abc123"), Some("abc123"));
    assert_eq!(extract_session_id("/SESSION abc123"), Some("abc123"));
    assert_eq!(extract_session_id("/Session abc123"), Some("abc123"));
    assert_eq!(extract_session_id("  /session   abc123  "), Some("abc123"));
}
