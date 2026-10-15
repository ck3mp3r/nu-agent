use super::*;

#[test]
fn test_dispatch_agent_resolve_mode_with_prompt_is_stderr() {
    // -- Setup & Fixtures
    // input_is_nothing=false means a prompt was supplied, so the mode is
    // stderr regardless of the terminal state.

    // -- Exec
    let mode = resolve_mode(false);

    // -- Check
    assert_eq!(mode, CliMode::Stderr);
}
