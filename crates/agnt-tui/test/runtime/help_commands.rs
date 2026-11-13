//! The help page's command list, pinned to the slash parser.
//!
//! The page is a checked-in markdown fixture, so nothing in the renderer can
//! notice it drifting from the commands the parser accepts: the row listed seven
//! of eleven once already. This test reads the fixture and compares it to the
//! parser's own order.

use agnt_runtime::protocol::slash::SLASH_COMMAND_ORDER;

use super::*;

// region:    --- Fixtures

/// The help page, read where it is checked in beside the renderer.
const HELP_PAGE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/src/runtime/help/help.md");

/// The prefix every supported-commands row carries.
const ROW_PREFIX: &str = "- Supported commands:";

// endregion: --- Fixtures

/// WHEN the help page lists its supported commands, THEN the page SHALL carry
/// exactly one such row and it SHALL name the parser's commands, in the
/// parser's order — a command added to the parser or dropped from the page
/// fails here.
#[test]
fn test_help_supported_commands_match_the_slash_parser() -> Result<()> {
    // -- Setup & Fixtures
    let page = fs::read_to_string(HELP_PAGE)?;
    let rows: Vec<&str> = page
        .lines()
        .filter(|line| line.trim_start().starts_with(ROW_PREFIX))
        .collect();

    // -- Exec
    let listed: Vec<String> = rows
        .first()
        .ok_or("the help page must list its supported commands")?
        .trim_start()
        .trim_start_matches(ROW_PREFIX)
        .split(',')
        .map(|entry| entry.trim().trim_end_matches('.').trim_matches('`'))
        .filter(|entry| !entry.is_empty())
        .map(str::to_string)
        .collect();
    let parsed: Vec<String> = SLASH_COMMAND_ORDER
        .iter()
        .map(|command| command.label().to_string())
        .collect();

    // -- Check
    assert_eq!(
        rows.len(),
        1,
        "one supported-commands row, or the page says two things: {rows:#?}"
    );
    assert_eq!(
        listed, parsed,
        "the help row must name the parser's commands, in its order"
    );
    Ok(())
}
