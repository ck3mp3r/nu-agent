use serde_json::Value as JsonValue;

pub fn summarize_tool_arguments(arguments: &str) -> String {
    const MAX_LEN: usize = 120;
    let compact = arguments.split_whitespace().collect::<Vec<_>>().join(" ");
    let spaced = compact.replace(", ", ",").replace(",", ", ");
    if spaced.chars().count() <= MAX_LEN {
        return spaced;
    }

    let mut truncated = spaced.chars().take(MAX_LEN).collect::<String>();
    truncated.push('…');
    truncated
}

/// Extract the raw nu command string from a nu tool-call arguments JSON
/// (`{"command": "..."}` shape). Returns `None` when the JSON is invalid,
/// the key is missing, or the value is not a non-empty string. CRLF line
/// endings are normalized to LF so the command renders one row per line.
pub fn nu_command_from_args(arguments: &str) -> Option<String> {
    let json = serde_json::from_str::<JsonValue>(arguments).ok()?;
    let command = json.get("command")?.as_str()?;
    if command.is_empty() {
        return None;
    }
    Some(command.replace("\r\n", "\n").replace('\r', "\n"))
}
