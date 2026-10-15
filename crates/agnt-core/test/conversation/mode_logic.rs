use super::{AgentMode, resolve_agent_mode, should_enter_foreground};

#[test]
fn test_conversation_mode_logic_agent_mode_is_tui() {
    // -- Setup & Fixtures
    let cases = [(AgentMode::Tui, true), (AgentMode::Stderr, false)];

    // -- Exec & Check
    for (mode, expected) in cases {
        assert_eq!(mode.is_tui(), expected, "mode={mode:?}");
    }
}

#[test]
fn test_conversation_mode_logic_should_enter_foreground_table() {
    // -- Setup & Fixtures
    let cases = [
        (AgentMode::Tui, false, true),
        (AgentMode::Tui, true, true),
        (AgentMode::Stderr, true, true),
        (AgentMode::Stderr, false, false),
    ];

    // -- Exec & Check
    for (mode, stderr_is_tty, expected) in cases {
        assert_eq!(
            should_enter_foreground(mode, stderr_is_tty),
            expected,
            "mode={mode:?} stderr_is_tty={stderr_is_tty}"
        );
    }
}

#[test]
fn test_conversation_mode_logic_resolve_agent_mode_table() {
    // -- Setup & Fixtures
    let cases = [
        (true, true, true, AgentMode::Tui),
        (false, true, true, AgentMode::Stderr),
        (true, false, true, AgentMode::Stderr),
        (true, true, false, AgentMode::Stderr),
        (false, false, false, AgentMode::Stderr),
    ];

    // -- Exec & Check
    for (input_is_nothing, stdin_is_tty, stderr_is_tty, expected) in cases {
        assert_eq!(
            resolve_agent_mode(input_is_nothing, stdin_is_tty, stderr_is_tty),
            expected,
            "input_is_nothing={input_is_nothing} stdin_is_tty={stdin_is_tty} \
             stderr_is_tty={stderr_is_tty}"
        );
    }
}
