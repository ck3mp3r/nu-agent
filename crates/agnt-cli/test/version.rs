use super::*;

use clap::CommandFactory;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn test_version_message_format() -> Result<()> {
    // -- Setup & Fixtures
    let version = env!("CARGO_PKG_VERSION");

    // -- Exec
    let suffix = VERSION_MESSAGE
        .strip_prefix(version)
        .ok_or("VERSION_MESSAGE should start with the cargo version")?;

    // -- Check
    if !suffix.is_empty() {
        let sha = suffix
            .strip_prefix(" (")
            .and_then(|rest| rest.strip_suffix(')'))
            .ok_or("version suffix should be wrapped in ' (' and ')'")?;
        assert!(!sha.is_empty(), "version suffix should be non-empty");
        assert!(
            !sha.contains(char::is_whitespace),
            "version suffix should contain no whitespace, got: {sha}"
        );
        assert!(
            !sha.contains(['(', ')']),
            "version suffix should contain no parenthesis, got: {sha}"
        );
    }
    Ok(())
}

#[test]
fn test_version_message_clap_wiring() -> Result<()> {
    // -- Setup & Fixtures
    let command = Cli::command();

    // -- Exec
    let version = command.get_version();

    // -- Check
    assert_eq!(version, Some(VERSION_MESSAGE));
    Ok(())
}
