use super::*;

use std::io::IsTerminal;

#[test]
fn test_dispatch_a2a_resolve_mode_matches_terminal_state() {
    // -- Setup & Fixtures
    // resolve_mode() always passes input_is_nothing=true, so TUI is selected
    // only when both stdin and stderr are terminals.
    let both_tty = std::io::stdin().is_terminal() && std::io::stderr().is_terminal();

    // -- Exec
    let mode = resolve_mode();

    // -- Check
    if both_tty {
        assert_eq!(mode, CliMode::Tui);
    } else {
        assert_eq!(mode, CliMode::Stderr);
    }
}
