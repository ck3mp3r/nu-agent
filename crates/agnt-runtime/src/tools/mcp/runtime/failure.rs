//! The text a failed connect leaves behind: one row for the surface, the whole
//! dump in the log.

/// Collapse a connect failure into the single row the surface shows.
///
/// A connect failure can carry a child's own dump — a clap usage block, a panic
/// trace, an ANSI-coloured warning. The surface paints rows, so every control
/// byte becomes a space and each whitespace run becomes one space: the text
/// keeps every word, loses only its layout, and can never move the cursor of
/// the frame the panel owns. The log keeps the dump as it arrived.
pub(crate) fn collapse_failure_reason(reason: &str) -> String {
    reason
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
