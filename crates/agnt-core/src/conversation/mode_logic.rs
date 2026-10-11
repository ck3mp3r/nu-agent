//! Execution-mode selection shared by the plugin and the CLI.
//!
//! The plugin and the CLI both decide between TUI and stderr rendering from
//! the same inputs (piped prompt, stdin TTY, stderr TTY). The decision logic
//! lives here so neither binary duplicates it.

// region:    --- Types

/// How the agent runtime renders its output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentMode {
    Tui,
    Stderr,
}

impl AgentMode {
    pub fn is_tui(self) -> bool {
        matches!(self, Self::Tui)
    }
}

// endregion: --- Types

// region:    --- Functions

/// Whether the caller should request terminal foreground ownership to receive SIGINT.
///
/// True for TUI (always needs it) and for stderr mode when stderr is a TTY
/// (the user has a terminal and may press Ctrl+C).
pub fn should_enter_foreground(mode: AgentMode, stderr_is_tty: bool) -> bool {
    mode.is_tui() || stderr_is_tty
}

/// Select the execution mode from the input and terminal state.
pub fn resolve_agent_mode(
    input_is_nothing: bool,
    stdin_is_tty: bool,
    stderr_is_tty: bool,
) -> AgentMode {
    if input_is_nothing && stdin_is_tty && stderr_is_tty {
        AgentMode::Tui
    } else {
        AgentMode::Stderr
    }
}

// endregion: --- Functions
