//! Parsing helpers for tool status lines.

const TOOL_PREFIX: &str = "tool[";
const TOOL_ARGS_MARKER: &str = "] → ";
const TOOL_DONE_SUFFIX: &str = " · done";
const TOOL_FAILED_SUFFIX: &str = " · failed";

/// Extract the tool name from a tool status line (e.g. `tool[k8s__list] → ...`).
pub(crate) fn extract_tool_name(line: &str) -> &str {
    if let Some(rest) = line.strip_prefix(TOOL_PREFIX)
        && let Some((name, _)) = rest.split_once(']')
    {
        return name;
    }
    "unknown"
}

/// Parse a persisted tool status line into `(name, arguments, success)`.
pub(crate) fn parse_persisted_tool_status_line(line: &str) -> Option<(&str, &str, bool)> {
    let remainder = line.strip_prefix(TOOL_PREFIX)?;
    let (name, after_name) = remainder.split_once(TOOL_ARGS_MARKER)?;
    if let Some(arguments) = after_name.strip_suffix(TOOL_DONE_SUFFIX) {
        return Some((name, arguments, true));
    }
    if let Some(arguments) = after_name.strip_suffix(TOOL_FAILED_SUFFIX) {
        return Some((name, arguments, false));
    }
    None
}
