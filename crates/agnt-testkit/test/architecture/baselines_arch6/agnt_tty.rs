//! The ARCH-6 rows recorded for `agnt-tty`.

pub const ROWS: &[&str] = &[
    "ARCH-6 agnt-tty/src/ansi.rs:5: `style_text` has no consumer outside `agnt-tty`",
    "ARCH-6 agnt-tty/src/formatter.rs:17: `format_tool_start` has no consumer outside `agnt-tty`",
    "ARCH-6 agnt-tty/src/formatter.rs:45: `ToolEndView` has no consumer outside `agnt-tty`",
    "ARCH-6 agnt-tty/src/markdown_buffer.rs:10: `StreamingMarkdownBuffer` has no consumer outside `agnt-tty`",
    "ARCH-6 agnt-tty/src/renderer/backend.rs:22: `TtyBackend` has no consumer outside `agnt-tty`",
    "ARCH-6 agnt-tty/src/renderer/mod.rs:97: `StderrUiRenderer` has no consumer outside `agnt-tty`",
    "ARCH-6 agnt-tty/src/spinner.rs:2: `SpinnerState` has no consumer outside `agnt-tty`",
];

/// The impl-member rows: a `pub` member on a type this crate surfaces, with no
/// consumer outside it.
pub const MEMBER_ROWS: &[&str] = &[
    "ARCH-6 agnt-tty/src/factory.rs:21: `new` has no consumer outside `agnt-tty`",
    "ARCH-6 agnt-tty/src/policy.rs:17: `allows_spinner` has no consumer outside `agnt-tty`",
    "ARCH-6 agnt-tty/src/renderer/backend.rs:34: `new` has no consumer outside `agnt-tty`",
    "ARCH-6 agnt-tty/src/renderer/backend.rs:43: `with_now_millis` has no consumer outside `agnt-tty`",
    "ARCH-6 agnt-tty/src/renderer/backend.rs:60: `line_count` has no consumer outside `agnt-tty`",
    "ARCH-6 agnt-tty/src/renderer/mod.rs:151: `new` has no consumer outside `agnt-tty`",
    "ARCH-6 agnt-tty/src/spinner.rs:14: `new` has no consumer outside `agnt-tty`",
    "ARCH-6 agnt-tty/src/spinner.rs:32: `new_with_charset` has no consumer outside `agnt-tty`",
    "ARCH-6 agnt-tty/src/spinner.rs:46: `is_enabled` has no consumer outside `agnt-tty`",
    "ARCH-6 agnt-tty/src/spinner.rs:50: `is_active` has no consumer outside `agnt-tty`",
    "ARCH-6 agnt-tty/src/spinner.rs:54: `current_frame` has no consumer outside `agnt-tty`",
    "ARCH-6 agnt-tty/src/spinner.rs:66: `stop` has no consumer outside `agnt-tty`",
    "ARCH-6 agnt-tty/src/spinner.rs:72: `suspend` has no consumer outside `agnt-tty`",
    "ARCH-6 agnt-tty/src/spinner.rs:78: `resume` has no consumer outside `agnt-tty`",
];
